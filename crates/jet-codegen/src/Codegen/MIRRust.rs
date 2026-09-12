//! Rust source emission for optimized canonical MIR.
//!
//! This module is deliberately a printer, not a second lowering pipeline.  It
//! consumes the checked `MirProgram` rows and emits Rust items and a small CFG
//! machine.  Names, calls, ownership, failure edges, and target facts come from
//! MIR; the narrow Core path ABI adapter only projects canonical sema signature
//! metadata and performs no ad-hoc source or Core-name lookup.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use jet_foundation::CanonicalPass;
use jet_foundation::Layout::TargetLayout;
use jet_foundation::MIROptimization::Acceleration::{AccelerationTransform, D_FRED1_FIXED_ORDER};
use jet_foundation::Names::{mangle, mangle_path};
use jet_foundation::Shape::ShapeProjectionKind;
use jet_foundation::Syntax::CoreCallSymbol;
use jet_foundation::MIR::*;

/// Which MIR target partition this printer emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirRustTarget {
    Native,
    WebWasm,
}
#[derive(Clone, Copy)]
enum HistoryScalarKind {
    Integer,
    Boolean,
    Text,
    Char,
    SignedInteger,
    UnsignedInteger,
}
fn quote_rust_string(value: &str) -> String {
    crate::Codegen::escape_rust_str(value)
}

fn allocator_view_inner(ty: &MirType) -> Option<&MirType> {
    match ty.kind() {
        MirTypeKind::Tagged {
            marker: MirTagMarker::Internal(MirInternalTag::AllocatorView),
            inner,
        } => Some(inner),
        _ => None,
    }
}

fn is_allocator_result_type(ty: &MirType) -> bool {
    matches!(
        ty.kind(),
        MirTypeKind::Result { err, .. } if err.name().starts_with("AllocError")
    )
}



#[derive(Debug)]
enum ModelDimensionFact {
    Static(u64),
    Dynamic { name: String, min: u64, max: u64 },
}

#[derive(Debug)]
struct ModelTensorFact {
    name: String,
    dtype: String,
    shape: Vec<ModelDimensionFact>,
}

fn model_unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return value.to_string();
    }
    let mut result = String::with_capacity(value.len() - 2);
    let mut escaped = false;
    for character in value[1..value.len() - 1].chars() {
        if escaped {
            result.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    if escaped {
        result.push('\\');
    }
    result
}

fn model_list_items(value: &str) -> Vec<String> {
    let value = value.trim();
    let body = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(value);
    let characters = body.chars().collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for index in 0..=characters.len() {
        let character = characters.get(index).copied();
        if quoted {
            if escaped {
                escaped = false;
            } else if character == Some('\\') {
                escaped = true;
            } else if character == Some('"') {
                quoted = false;
            }
            continue;
        }
        match character {
            Some('"') => quoted = true,
            Some('{') | Some('[') => depth += 1,
            Some('}') | Some(']') => depth = depth.saturating_sub(1),
            Some(',') if depth == 0 => {
                let item = body
                    .chars()
                    .skip(start)
                    .take(index.saturating_sub(start))
                    .collect::<String>();
                if !item.trim().is_empty() {
                    result.push(item.trim().to_string());
                }
                start = index + 1;
            }
            Some(character) if character.is_whitespace() && depth == 0 => {
                let item = body
                    .chars()
                    .skip(start)
                    .take(index.saturating_sub(start))
                    .collect::<String>();
                if !item.trim().is_empty() {
                    result.push(item.trim().to_string());
                }
                start = index + 1;
            }
            None => {
                let item = body
                    .chars()
                    .skip(start)
                    .take(index.saturating_sub(start))
                    .collect::<String>();
                if !item.trim().is_empty() {
                    result.push(item.trim().to_string());
                }
            }
            _ => {}
        }
    }
    result
}

fn model_object_entries(value: &str) -> BTreeMap<String, String> {
    let value = value.trim();
    let body = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(value);
    let characters = body.chars().collect::<Vec<_>>();
    let mut result = BTreeMap::new();
    let mut index = 0usize;
    while index < characters.len() {
        while index < characters.len()
            && (characters[index].is_whitespace() || characters[index] == ',')
        {
            index += 1;
        }
        if index >= characters.len() {
            break;
        }
        let key_start = index;
        while index < characters.len() && characters[index] != ':' {
            index += 1;
        }
        if index >= characters.len() {
            break;
        }
        let key = model_unquote(&characters[key_start..index].iter().collect::<String>());
        index += 1;
        while index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        let value_start = index;
        if characters.get(index) == Some(&'"') {
            index += 1;
            let mut escaped = false;
            while index < characters.len() {
                let character = characters[index];
                index += 1;
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    break;
                }
            }
        } else if matches!(characters.get(index), Some('{') | Some('[')) {
            let mut depth = 0usize;
            let mut quoted = false;
            let mut escaped = false;
            while index < characters.len() {
                let character = characters[index];
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if character == '\\' {
                        escaped = true;
                    } else if character == '"' {
                        quoted = false;
                    }
                } else {
                    match character {
                        '"' => quoted = true,
                        '{' | '[' => depth += 1,
                        '}' | ']' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                index += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                index += 1;
            }
        } else {
            while index < characters.len()
                && !characters[index].is_whitespace()
                && characters[index] != ','
            {
                index += 1;
            }
        }
        let parsed = characters[value_start..index]
            .iter()
            .collect::<String>()
            .trim()
            .trim_end_matches(',')
            .trim()
            .to_string();
        result.insert(key, parsed);
    }
    result
}

fn model_tensor_fact(raw: &str) -> Option<ModelTensorFact> {
    let fields = model_object_entries(raw);
    let name = model_unquote(fields.get("name")?);
    let dtype = model_unquote(fields.get("dtype")?);
    let shape = model_list_items(fields.get("shape")?)
        .into_iter()
        .filter_map(|dimension| {
            if dimension.starts_with('{') {
                let fields = model_object_entries(&dimension);
                let name = model_unquote(fields.get("name")?);
                let min = model_unquote(fields.get("min")?).parse().ok()?;
                let max = model_unquote(fields.get("max")?).parse().ok()?;
                Some(ModelDimensionFact::Dynamic { name, min, max })
            } else {
                Some(ModelDimensionFact::Static(
                    model_unquote(&dimension).parse().ok()?,
                ))
            }
        })
        .collect::<Vec<_>>();
    (!name.is_empty() && !dtype.is_empty() && !shape.is_empty()).then_some(ModelTensorFact {
        name,
        dtype,
        shape,
    })
}

/// Formatting-only policy for the MIR Rust adapter.
///
/// All semantic choices are MIR facts.  The adapter may choose the generated
/// module root and whether item classes are included in this fragment, but it
/// cannot provide a semantic or symbol lookup table.
#[derive(Debug, Clone)]
pub struct MirRustConfig<'a> {
    pub target: TargetLayout,
    pub target_kind: MirRustTarget,
    pub root_prefix: String,
    pub execution: MirRustExecutionConfig<'a>,
}

#[derive(Debug, Clone)]
pub struct MirRustExecutionConfig<'a> {
    /// The checked artifact plan whose rows this adapter is allowed to emit.
    pub artifact: MirArtifactId,
    /// Prepared physical bridge binding; language checks remain canonical MIR facts.
    pub ffi: Option<&'a crate::AST::FfiLink>,
    pub emit_types: bool,
    pub emit_foreign: bool,
    pub emit_metadata: bool,
    pub semantic_digest: Option<[u8; 32]>,
    pub emit_debug_linemap: bool,
    /// Include the complete cached Prelude/Core runtime in this artifact.
    ///
    /// Standalone MIR emission keeps this enabled. An enclosing target
    /// assembler may disable it only when that assembler supplies the same
    /// complete runtime closure.
    pub emit_runtime: bool,
    /// D-DX-PROD1: one typed release policy controls every emitted DevTools
    /// producer.  The code generator never reconstructs a parallel bool.
    pub release_devtools_policy: jet_pkg_model::Package::ReleaseDevtoolsPolicy,
}

impl<'a> MirRustExecutionConfig<'a> {
    pub fn for_artifact(artifact: MirArtifactId) -> Self {
        Self {
            artifact,
            ffi: None,
            emit_types: true,
            emit_foreign: true,
            emit_metadata: false,
            semantic_digest: None,
            emit_debug_linemap: false,
            emit_runtime: true,
            release_devtools_policy: jet_pkg_model::Package::ReleaseDevtoolsPolicy::development(),
        }
    }
}

fn mir_type_uses_atomic_word(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Apply { name, args } => {
            name.name == "Atomic"
                || name.name.ends_with("::Atomic")
                || args.iter().any(mir_type_uses_atomic_word)
        }
        MirTypeKind::List(inner)
        | MirTypeKind::Shared(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => mir_type_uses_atomic_word(inner),
        MirTypeKind::Map { key, value }
        | MirTypeKind::Result {
            ok: key,
            err: value,
        } => mir_type_uses_atomic_word(key) || mir_type_uses_atomic_word(value),
        MirTypeKind::Fn(signature) => {
            signature.params.iter().any(mir_type_uses_atomic_word)
                || signature
                    .ret
                    .as_deref()
                    .is_some_and(mir_type_uses_atomic_word)
        }
        MirTypeKind::SendFn { params, ret } => {
            params.iter().any(mir_type_uses_atomic_word)
                || ret.as_deref().is_some_and(mir_type_uses_atomic_word)
        }
        MirTypeKind::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| mir_type_uses_atomic_word(field)),
        MirTypeKind::Union(members) => members.iter().any(mir_type_uses_atomic_word),
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Measure(_) => false,
    }
}

fn mir_program_uses_atomic_word(program: &MirProgram) -> bool {
    program.type_instances.iter().any(mir_type_uses_atomic_word)
}

fn mir_type_uses_shared_kernel(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Apply { name, args } => {
            matches!(name.name.as_str(), "Pool" | "Id" | "Shared")
                || args.iter().any(mir_type_uses_shared_kernel)
        }
        MirTypeKind::List(inner)
        | MirTypeKind::Shared(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => mir_type_uses_shared_kernel(inner),
        MirTypeKind::Map { key, value }
        | MirTypeKind::Result {
            ok: key,
            err: value,
        } => mir_type_uses_shared_kernel(key) || mir_type_uses_shared_kernel(value),
        MirTypeKind::Fn(signature) => {
            signature.params.iter().any(mir_type_uses_shared_kernel)
                || signature
                    .ret
                    .as_deref()
                    .is_some_and(mir_type_uses_shared_kernel)
        }
        MirTypeKind::SendFn { params, ret } => {
            params.iter().any(mir_type_uses_shared_kernel)
                || ret.as_deref().is_some_and(mir_type_uses_shared_kernel)
        }
        MirTypeKind::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| mir_type_uses_shared_kernel(field)),
        MirTypeKind::Union(members) => members.iter().any(mir_type_uses_shared_kernel),
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Measure(_) => false,
    }
}

fn mir_program_uses_shared_kernel(program: &MirProgram) -> bool {
    program
        .type_instances
        .iter()
        .any(mir_type_uses_shared_kernel)
}

fn mir_program_uses_arrow(program: &MirProgram) -> bool {
    // `core_calls` is the static ABI registry, not the set of calls reached
    // by this package. Reachability is carried through checked package facts.
    program.facts.uses_arrow
}

/// Emit a complete Rust fragment from optimized canonical MIR.
pub fn emit_mir_program(program: &MirProgram, config: &MirRustConfig) -> String {
    let mut out = String::new();
    emit_mir_program_into(program, config, &mut out);
    out
}

/// Append a complete Rust fragment from optimized canonical MIR.
pub fn emit_mir_program_into(program: &MirProgram, config: &MirRustConfig, out: &mut String) {
    let canonical_input = CanonicalPass::enabled().then(|| canonical_payload(program));
    let canonical_input_identity = CanonicalPass::enabled().then(|| canonical_identity(program));
    if let Err(error) = program.cffi.validate_boundaries() {
        panic!("MIR Rust emission received invalid foreign boundary facts: {error}");
    }
    let artifact_identity = program
        .artifact_identity(config.execution.artifact)
        .unwrap_or_else(|error| {
            panic!("MIR Rust emission requires canonical artifact identity: {error}")
        });
    let emitter = RustEmitter::new(program, config, artifact_identity.clone());
    let target_supports_atomic =
        program
            .facts
            .target_dossier
            .machine
            .as_deref()
            .is_some_and(|machine| {
                machine
                    .provides_capability(jet_foundation::TargetMachine::TargetCapability::Atomic64)
            });
    assert!(
        !mir_program_uses_atomic_word(program) || target_supports_atomic,
        "MIR Atomic<T> reached Rust emission without Target.Atomic64 sema admission for target `{}`",
        config.target.triple
    );
    let mut emitted_methods = BTreeSet::new();
    if config.execution.emit_runtime {
        if emitter.is_no_os() {
            out.push_str("#![no_std]\n#![no_main]\n");
            super::push_portable_corelib_prelude(out, program, emitter.artifact);
        } else {
            let uses_arrow = mir_program_uses_arrow(program)
                || emitter
                    .artifact
                    .runtime_parts
                    .contains(&MirRuntimePartId::Data);
            let active_os = jet_foundation::OSTarget::OSTarget::parse(&program.facts.active_os)
                .or_else(|| jet_foundation::OSTarget::OSTarget::from_triple(&config.target.triple))
                .unwrap_or_else(jet_foundation::OSTarget::OSTarget::host);
            super::push_cached_runtime_begin_with_policy(
                out,
                config.execution.ffi,
                uses_arrow,
                &config.execution.release_devtools_policy,
            );
            out.push_str(super::CACHED_RUNTIME_END);
            out.push_str(super::CACHED_CORE_BEGIN);
            let test_harness = matches!(
                emitter.artifact.kind,
                MirArtifactKind::TestExecutable
                    | MirArtifactKind::FuzzExecutable
                    | MirArtifactKind::TestOverride
            );
            super::push_full_corelib_prelude_with_policy(
                out,
                active_os,
                test_harness,
                &program.facts.edition,
                &emitter.artifact.runtime_parts,
                mir_program_uses_shared_kernel(program),
                uses_arrow,
                &config.execution.release_devtools_policy,
            );
            out.push_str(super::CACHED_CORE_END);
            if let Some(link) = config.execution.ffi {
                let needs_http_bridge = program.prelude_calls.iter().any(|call| {
                    let symbol = match &call.symbol {
                        MirSymbol::Prelude(symbol) | MirSymbol::Runtime(symbol) => symbol.as_str(),
                    };
                    matches!(
                        symbol,
                        "jet_http_client_get"
                            | "jet_http_client_post"
                            | "jet_http_client_request_send"
                    )
                });
                if needs_http_bridge {
                    let _ = writeln!(out, "jet_http_client_bridge!({});", link.crate_name);
                }
            }
            out.push('\n');
        }
    }
    if emitter.coverage_enabled() {
        super::push_coverage_prelude(out);
    }
    if !program.facts.model_outputs.is_empty() {
        out.push_str("extern crate jet_rt;\n");
    }
    emitter.emit_hardware_facts(out);
    out.push('\n');

    if config.execution.emit_metadata {
        emitter.emit_program_metadata(out);
        emitter.emit_modules_and_imports(out);
    }
    if config.execution.emit_types {
        for handle in &program.handles {
            if handle.payload.library != "core.process" {
                emitter.emit_handle_type(handle, out);
            }
        }
        for def in &program.types {
            // Compiler-owned type rows (`Ordering`, default `Err`) exist so
            // every adapter constructs variants and fields from one checked
            // declaration; their Rust spellings are declared once by the
            // cached runtime (`push_cached_runtime_traits`), never per program.
            if emitter.module_selected(def.module)
                && !emitter.is_handle_name(&def.name)
                && !emitter.is_handle_name(&def.key)
                && !crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_type(&def.key)
            {
                emitter.emit_type_def(def, out);
            }
        }
        emitter.emit_period_anchor_impls(out);
        emitter.emit_history_strategies(out);
        for trait_def in &program.traits {
            if emitter.module_selected(trait_def.module)
                && !crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_trait(&trait_def.name)
                && !crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_trait(&trait_def.key)
            {
                emitter.emit_trait_def(trait_def, &mut emitted_methods, out);
            }
        }
        for constant in &program.constants {
            if emitter.module_selected(constant.module) {
                emitter.emit_constant_def(constant, out);
            }
        }
    }
    emitter.emit_link_closure(out);
    if config.execution.emit_foreign {
        for foreign in &program.foreign {
            if emitter.module_selected(foreign.module_id) {
                emitter.emit_foreign(foreign, out);
            }
        }
    }
    emitter.emit_callback_trampolines(out);
    for implementation in &program.impls {
        if !emitter.module_selected(implementation.module)
            || !emitter.impl_selected_for_target(implementation)
        {
            continue;
        }
        emitter.emit_impl(implementation, &mut emitted_methods, out);
    }
    if !program.facts.model_outputs.is_empty() {
        emitter.emit_model_adapters(out);
    }
    for function in &program.functions {
        if !emitter.module_selected(function.module_id) || !emitter.selected_for_target(function) {
            continue;
        }
        match &function.form {
            MirFunctionForm::TopLevel => emitter.emit_function(function, out),
            MirFunctionForm::Method { .. } | MirFunctionForm::TraitMethod { .. } => {
                if !emitted_methods.contains(&function.id) {
                    panic!(
                        "MIR method {:?} has no selected implementation row",
                        function.id
                    );
                }
            }
        }
    }
    emitter.emit_web_data_type_registration(out);
    emitter.emit_exports(out);
    emitter.emit_entry(out);
    emitter.emit_harness(out);
    for encoder in emitter.history_capture_encoders.borrow().values() {
        out.push_str(encoder);
    }
    for arity in emitter.history_callable_arities.borrow().iter() {
        if matches!(*arity, 0 | 1 | 3) {
            continue;
        }
        let args = (0..*arity)
            .map(|index| format!(", A{index}"))
            .collect::<String>();
        let _ = writeln!(out, "jet_history_callable!(JetHistoryFn{arity}{args});");
    }
    for (arity, modes) in emitter.history_callable_modes.borrow().iter() {
        let args = modes
            .chars()
            .enumerate()
            .map(|(index, mode)| {
                let mode = match mode {
                    'O' => "owned",
                    'R' => "read",
                    'W' => "write",
                    other => panic!("MIR history callable has unknown mode `{other}`"),
                };
                format!("A{index} => {mode}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "jet_history_callable!(JetHistoryFn{arity}B{modes}; {args});"
        );
    }
    if let (Some(canonical_input), Some(canonical_input_identity)) =
        (canonical_input, canonical_input_identity)
    {
        CanonicalPass::record(
            "lowering",
            "mir.rust-emitter",
            "crates/jet-codegen/src/Codegen/MIRRust.rs",
            "mir",
            canonical_input,
            canonical_input_identity,
            "mir",
            CanonicalPass::debug_payload(
                "mir",
                "mir.rust-emitter",
                &(out, artifact_identity.clone(), &config.target.triple),
            ),
            format!(
                "{{\"representation\":\"mir\",\"artifact_id\":\"{}\",\"target\":\"{}\"}}",
                artifact_identity.artifact.0,
                jet_foundation::JSON::json_escape(&config.target.triple),
            ),
            "checked",
        );
    }
    CanonicalPass::persist_process("emit");
}

/// Public descriptor used by integration diagnostics and source-map tooling.
pub fn mir_layout_descriptor(layout: &MirLayout) -> String {
    let abi = match layout.abi {
        MirAbi::Scalar(kind) => match kind {
            jet_foundation::MIR::MirScalarKind::Int => "scalar:int",
            jet_foundation::MIR::MirScalarKind::Float => "scalar:float",
            jet_foundation::MIR::MirScalarKind::Float32 => "scalar:float32",
            jet_foundation::MIR::MirScalarKind::Bool => "scalar:bool",
            jet_foundation::MIR::MirScalarKind::Char => "scalar:char",
            jet_foundation::MIR::MirScalarKind::Pointer => "scalar:pointer",
        },
        MirAbi::Aggregate => "aggregate",
        MirAbi::Sequence => "sequence",
        MirAbi::Function => "function",
        MirAbi::Nominal => "nominal",
        MirAbi::Dynamic => "dynamic",
        MirAbi::Never => "never",
    };
    let size = match layout.size {
        MirSize::Static(value) => format!("static:{value}"),
        MirSize::Dynamic => "dynamic".to_string(),
    };
    let align = match layout.align {
        MirSize::Static(value) => format!("static:{value}"),
        MirSize::Dynamic => "dynamic".to_string(),
    };
    format!("abi={abi};size={size};align={align}")
}
fn select_artifact(
    program: &MirProgram,
    id: MirArtifactId,
    target: MirRustTarget,
) -> &MirArtifactPlan {
    let plan = program
        .artifacts
        .iter()
        .find(|plan| plan.id == id)
        .unwrap_or_else(|| panic!("MIR artifact ID {:?} has no plan row", id));
    let target_ok = match (target, plan.target) {
        (MirRustTarget::Native, MirArtifactTarget::RustAot) => true,
        (MirRustTarget::WebWasm, MirArtifactTarget::Web) => true,
        (
            MirRustTarget::Native,
            MirArtifactTarget::Cranelift | MirArtifactTarget::Interpreter | MirArtifactTarget::Web,
        )
        | (
            MirRustTarget::WebWasm,
            MirArtifactTarget::RustAot
            | MirArtifactTarget::Cranelift
            | MirArtifactTarget::Interpreter,
        ) => false,
    };
    let kind_ok = match (target, plan.kind) {
        (
            MirRustTarget::Native,
            MirArtifactKind::NativeExecutable
            | MirArtifactKind::NativeLibrary
            | MirArtifactKind::SandboxPlugin
            | MirArtifactKind::TestExecutable
            | MirArtifactKind::FuzzExecutable
            | MirArtifactKind::TestOverride,
        ) => true,
        (MirRustTarget::WebWasm, MirArtifactKind::WebApplication) => true,
        (MirRustTarget::Native, MirArtifactKind::WebApplication)
        | (
            MirRustTarget::WebWasm,
            MirArtifactKind::NativeExecutable
            | MirArtifactKind::NativeLibrary
            | MirArtifactKind::SandboxPlugin
            | MirArtifactKind::TestExecutable
            | MirArtifactKind::FuzzExecutable
            | MirArtifactKind::TestOverride,
        ) => false,
    };
    if !target_ok || !kind_ok {
        panic!(
            "MIR artifact {:?} kind {} is not applicable to {:?} Rust emission",
            id,
            plan.kind.as_str(),
            target
        );
    }
    if plan.modules.is_empty() {
        panic!("MIR artifact {:?} has no selected module rows", id);
    }
    plan
}

fn function_has_debug_only(function: &MirFunction) -> bool {
    function
        .scopes
        .iter()
        .any(|scope| scope.kind == MirScopeKind::DebugOnly)
}

fn debug_only_block_states(function: &MirFunction) -> BTreeMap<MirBlockId, bool> {
    let is_debug_scope = |scope_id: MirScopeId| {
        function
            .scopes
            .iter()
            .any(|scope| scope.id == scope_id && scope.kind == MirScopeKind::DebugOnly)
    };
    let mut states = BTreeMap::new();
    states.insert(function.entry, false);
    let mut pending = vec![function.entry];
    while let Some(block_id) = pending.pop() {
        let Some(block) = function.blocks.iter().find(|block| block.id == block_id) else {
            continue;
        };
        let mut active = states.get(&block_id).copied().unwrap_or(false);
        for instruction in &block.instructions {
            match &instruction.operation {
                MirOperation::ScopeEnter { scope, .. } if is_debug_scope(*scope) => active = true,
                MirOperation::ScopeExit { scope } if is_debug_scope(*scope) => active = false,
                _ => {}
            }
        }
        for target in block.terminator.targets() {
            let next = states.get(&target).copied().unwrap_or(false) || active;
            if states.get(&target).copied() != Some(next) {
                states.insert(target, next);
                pending.push(target);
            }
        }
    }
    states
}

fn wrap_cfg_not_release(out: &mut String, start: usize, indent: usize) {
    if start >= out.len() {
        return;
    }
    let body = out[start..].to_string();
    out.truncate(start);
    let pad = " ".repeat(indent);
    let _ = writeln!(out, "{pad}#[cfg(not(jet_release))]");
    let _ = writeln!(out, "{pad}{{");
    for line in body.lines() {
        let _ = writeln!(out, "{pad}    {line}");
    }
    let _ = writeln!(out, "{pad}}}");
}

struct RustEmitter<'a> {
    program: &'a MirProgram,
    config: &'a MirRustConfig<'a>,
    artifact: &'a MirArtifactPlan,
    artifact_identity: MirArtifactIdentity,
    functions: BTreeMap<MirFunctionId, String>,
    test_functions: BTreeSet<MirFunctionId>,
    foreigns: BTreeMap<MirForeignId, String>,
    types: BTreeMap<MirTypeId, String>,
    type_instances: BTreeMap<MirTypeId, &'a MirType>,
    types_by_name: BTreeMap<String, String>,
    traits: BTreeMap<MirTraitId, String>,
    traits_by_name: BTreeMap<String, String>,
    fields: BTreeMap<MirFieldId, String>,
    history_capture_encoders: std::cell::RefCell<BTreeMap<String, String>>,
    history_callable_arities: std::cell::RefCell<BTreeSet<usize>>,
    history_callable_modes: std::cell::RefCell<BTreeSet<(usize, String)>>,
    generic_scopes: std::cell::RefCell<Vec<&'a [MirGenericParam]>>,
    history_current_function: std::cell::Cell<Option<MirFunctionId>>,
    history_callback_lifetime: std::cell::Cell<&'static str>,
}

impl<'a> RustEmitter<'a> {
    fn new(
        program: &'a MirProgram,
        config: &'a MirRustConfig<'a>,
        artifact_identity: MirArtifactIdentity,
    ) -> Self {
        let artifact = select_artifact(program, config.execution.artifact, config.target_kind);
        if artifact.id != artifact_identity.artifact {
            panic!("MIR artifact identity does not match the selected artifact row");
        }
        let functions = program
            .functions
            .iter()
            .map(|function| (function.id, mangle_path(&function.key)))
            .collect();
        let mut foreigns = BTreeMap::new();
        for foreign in &program.foreign {
            let name = if foreign.name.is_empty() {
                mangle_path(&foreign.key)
            } else {
                mangle(&foreign.name)
            };
            if foreigns.insert(foreign.id, name).is_some() {
                panic!("MIR foreign ID {:?} is duplicated", foreign.id);
            }
        }
        let mut types = BTreeMap::new();
        let type_instances = program
            .type_instances
            .iter()
            .filter_map(|instance| instance.identity.map(|id| (id, instance)))
            .collect();
        let mut types_by_name = BTreeMap::new();
        let mut fields = BTreeMap::new();
        let mut method_owners = BTreeMap::new();
        for def in &program.types {
            let symbol = mangle_path(&def.key);
            types.insert(def.id, symbol.clone());
            types_by_name.insert(def.key.clone(), symbol.clone());
            types_by_name.insert(def.name.clone(), symbol);
            Self::collect_type_fields(def, &mut fields);
            if let MirTypeDefKind::Struct { methods, .. } | MirTypeDefKind::Enum { methods, .. } =
                &def.kind
            {
                for function in methods {
                    if let Some(previous) = method_owners.insert(*function, def.id) {
                        if previous != def.id {
                            panic!(
                                "MIR method {:?} is listed by type IDs {:?} and {:?}",
                                function, previous, def.id
                            );
                        }
                    }
                }
            }
        }
        for ty in &program.type_instances {
            if let MirTypeKind::Tuple(declared) = ty.kind() {
                let identity = ty.identity_key();
                for (index, (name, _)) in declared.iter().enumerate() {
                    let id = MirFieldId(stable_id("mir-field", &format!("{identity}::{name}")));
                    fields.insert(id, index.to_string());
                }
            }
        }
        for handle in &program.handles {
            if let MirTypeKind::Apply { name, .. } = handle.ty.kind() {
                let symbol = mangle_path(&name.name);
                types_by_name
                    .entry(name.name.clone())
                    .or_insert_with(|| symbol.clone());
            }
        }
        for implementation in &program.impls {
            let owner =
                Self::type_identity_from_instances(&type_instances, &implementation.self_type);
            for function in &implementation.methods {
                if let Some(previous) = method_owners.insert(*function, owner) {
                    if previous != owner {
                        panic!(
                            "MIR method {:?} is listed by type IDs {:?} and {:?}",
                            function, previous, owner
                        );
                    }
                }
            }
        }
        let mut traits = BTreeMap::new();
        let mut traits_by_name = BTreeMap::new();
        for definition in &program.traits {
            let symbol =
                if crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_trait(&definition.name)
                {
                    crate::Codegen::rust_trait_name(&definition.name)
                } else {
                    mangle_path(&definition.key)
                };
            traits.insert(definition.id, symbol.clone());
            traits_by_name.insert(definition.key.clone(), symbol.clone());
            traits_by_name.insert(definition.name.clone(), symbol);
        }
        Self {
            program,
            config,
            artifact,
            artifact_identity,
            functions,
            test_functions: if artifact.mode == MirArtifactBuildMode::Coverage {
                program.tests.iter().map(|test| test.function).collect()
            } else {
                BTreeSet::new()
            },
            foreigns,
            types,
            type_instances,
            types_by_name,
            traits,
            traits_by_name,
            fields,
            history_capture_encoders: std::cell::RefCell::new(BTreeMap::new()),
            history_callable_arities: std::cell::RefCell::new(BTreeSet::new()),
            history_callable_modes: std::cell::RefCell::new(BTreeSet::new()),
            generic_scopes: std::cell::RefCell::new(Vec::new()),
            history_current_function: std::cell::Cell::new(None),
            history_callback_lifetime: std::cell::Cell::new("'static"),
        }
    }

    fn type_identity_from_instances(
        instances: &BTreeMap<MirTypeId, &'a MirType>,
        ty: &MirType,
    ) -> MirTypeId {
        if let Some(id) = ty.identity {
            if instances.contains_key(&id) {
                return id;
            }
            panic!("MIR type identity {:?} has no instance row", id);
        }
        instances
            .iter()
            .find(|(_, instance)| instance.same_checked_type(ty))
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("MIR structural type has no canonical instance"))
    }

    fn collect_type_fields(def: &MirTypeDef, fields: &mut BTreeMap<MirFieldId, String>) {
        let native = crate::Codegen::core_rust_type_name(&def.key).is_some()
            || crate::Codegen::root_prelude_rust_type_name(&def.key).is_some()
            || crate::Codegen::compute_handle_rust_type(&def.key).is_some();
        let field_name = |field: &MirField| {
            if native {
                field.name.clone()
            } else {
                mangle(&field.name)
            }
        };
        match &def.kind {
            MirTypeDefKind::Struct {
                fields: declared, ..
            } => {
                for field in declared {
                    fields.insert(field.id, field_name(field));
                }
            }
            MirTypeDefKind::Enum { variants, .. } => {
                for variant in variants {
                    if let MirVariantPayload::Named(declared) = &variant.payload {
                        for field in declared {
                            fields.insert(field.id, field_name(field));
                        }
                    }
                }
            }
            MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => {}
        }
    }

    fn selected_for_target(&self, function: &MirFunction) -> bool {
        match self.config.target_kind {
            MirRustTarget::Native => function.target_applicability.rust_aot,
            MirRustTarget::WebWasm => {
                function.target_applicability.web
                    && function.web_bucket == Some(jet_foundation::WebPartition::WebBucket::Wasm)
            }
        }
    }
    fn coverage_enabled(&self) -> bool {
        self.artifact.mode == MirArtifactBuildMode::Coverage
    }
    fn coverage_for(&self, function: &MirFunction) -> bool {
        self.coverage_enabled()
            && function.kind == MirFunctionKind::Jet
            && !self.test_functions.contains(&function.id)
    }
    fn impl_selected_for_target(&self, implementation: &MirImplDef) -> bool {
        self.target_applicable(implementation.target_applicability)
    }
    fn target_applicable(&self, applicability: MirTargetApplicability) -> bool {
        match self.config.target_kind {
            MirRustTarget::Native => {
                if self.artifact.target != MirArtifactTarget::RustAot {
                    panic!("MIR Rust native adapter selected a non-Rust artifact target");
                }
                applicability.rust_aot
            }
            MirRustTarget::WebWasm => {
                if self.artifact.target != MirArtifactTarget::Web {
                    panic!("MIR Rust web adapter selected a non-web artifact target");
                }
                applicability.web
            }
        }
    }

    fn module_selected(&self, module: MirModuleId) -> bool {
        self.artifact.modules.contains(&module)
    }

    fn function_name(&self, id: MirFunctionId) -> String {
        self.functions
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("MIR function ID {:?} has no function row", id))
    }

    fn has_test_scope(&self, function: &MirFunction) -> bool {
        function.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {
            matches!(&instruction.operation, MirOperation::ScopeEnter { test_member: Some(_), .. })
        })
    }

    fn has_expected_test_scope(&self, function: &MirFunction) -> bool {
        function.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {
            matches!(
                &instruction.operation,
                MirOperation::ScopeEnter {
                    test_member: Some(MirTestScopeMember::ExpectFail { .. }),
                    ..
                }
            )
        })
    }

    fn expected_scope_exit_blocks(
        &self,
        function: &MirFunction,
        states: &BTreeMap<MirBlockId, Vec<MirScopeId>>,
    ) -> BTreeMap<MirScopeId, MirBlockId> {
        let mut exits = BTreeMap::new();
        for block in &function.blocks {
            for instruction in &block.instructions {
                let MirOperation::ScopeExit { scope } = &instruction.operation else {
                    continue;
                };
                if matches!(
                    function.test_scope_member(*scope),
                    Some(MirTestScopeMember::ExpectFail { .. })
                ) {
                    exits.entry(*scope).or_insert(block.id);
                }
            }
        }
        for scope in &function.scopes {
            if !matches!(
                function.test_scope_member(scope.id),
                Some(MirTestScopeMember::ExpectFail { .. })
            ) || exits.contains_key(&scope.id)
            {
                continue;
            }
            if let Some(block) = function.blocks.iter().find(|block| {
                matches!(&block.terminator, MirTerminator::Return { .. })
                    && states
                        .get(&block.id)
                        .is_some_and(|active| active.contains(&scope.id))
            }) {
                exits.insert(scope.id, block.id);
            }
        }
        exits
    }

    fn test_scope_block_states(
        &self,
        function: &MirFunction,
    ) -> BTreeMap<MirBlockId, Vec<MirScopeId>> {
        let mut entry = BTreeMap::new();
        let mut exits = BTreeMap::new();
        entry.insert(function.entry, Vec::new());
        let mut pending = vec![function.entry];
        while let Some(block_id) = pending.pop() {
            let Some(block) = function.blocks.iter().find(|block| block.id == block_id) else {
                continue;
            };
            let mut active = entry.get(&block_id).cloned().unwrap_or_default();
            for instruction in &block.instructions {
                match &instruction.operation {
                    MirOperation::ScopeEnter { scope, .. }
                        if function.test_scope_member(*scope).is_some() =>
                    {
                        active.push(*scope);
                    }
                    MirOperation::ScopeExit { scope }
                        if function.test_scope_member(*scope).is_some() =>
                    {
                        if let Some(index) = active.iter().rposition(|active| active == scope) {
                            active.remove(index);
                        }
                    }
                    _ => {}
                }
            }
            exits.insert(block_id, active.clone());
            for target in block.terminator.targets() {
                match entry.get(&target).cloned() {
                    None => {
                        entry.insert(target, active.clone());
                        pending.push(target);
                    }
                    Some(previous) if previous == active => {}
                    Some(previous) => {
                        let merged = previous
                            .iter()
                            .zip(&active)
                            .take_while(|(left, right)| left == right)
                            .map(|(scope, _)| *scope)
                            .collect::<Vec<_>>();
                        if merged != previous {
                            entry.insert(target, merged);
                            pending.push(target);
                        }
                    }
                }
            }
        }
        exits
    }

    fn scope_operation_expression(
        &self,
        function: &MirFunction,
        scope_id: MirScopeId,
        enter: bool,
    ) -> String {
        let root = &self.config.root_prefix;
        let member = function.test_scope_member(scope_id);
        if enter {
            return match member {
                Some(MirTestScopeMember::ExpectFail { expected_code }) => {
                    let expected = expected_code
                        .as_deref()
                        .map(|code| format!("Some({code:?})"))
                        .unwrap_or_else(|| "None".to_string());
                    format!("{root}jet_test_expect_fail_enter_scope({}, {expected})", scope_id.0)
                }
                Some(MirTestScopeMember::Timeout { duration }) => {
                    format!(
                        "{root}jet_test_timeout_enter({}, ({}).ns)",
                        scope_id.0,
                        self.value_read(*duration)
                    )
                }
                _ => format!("/* MIR scope enter {} */", scope_id.0),
            };
        }
        match member {
            Some(MirTestScopeMember::ExpectFail { expected_code }) => {
                let expected = expected_code
                    .as_deref()
                    .map(|code| format!("Some({code:?})"))
                    .unwrap_or_else(|| "None".to_string());
                format!(
                    "if {root}jet_test_expect_fail_leave_scope({}).is_none() {{ {root}jet_test_expect_fail_unmet({expected}); }}",
                    scope_id.0
                )
            }
            Some(MirTestScopeMember::Timeout { .. }) => format!(
                "if let Some((__jet_elapsed, __jet_limit)) = {root}jet_test_timeout_leave({}) {{ if __jet_elapsed > __jet_limit {{ {root}jet_test_timeout_failure(__jet_elapsed, __jet_limit); }} }}",
                scope_id.0
            ),
            Some(MirTestScopeMember::Skip { whole_test }) => {
                format!("{root}jet_test_skip_scope({whole_test})")
            }
            _ => format!("/* MIR scope exit {} */", scope_id.0),
        }
    }

    fn emit_test_scope_exits(
        &self,
        function: &MirFunction,
        scopes: &[MirScopeId],
        out: &mut String,
        indent: usize,
    ) {
        for scope in scopes.iter().rev() {
            let _ = writeln!(
                out,
                "{:indent$}{};",
                "",
                self.scope_operation_expression(function, *scope, false),
                indent = indent
            );
        }
    }
    /// Native fix-and-continue is deliberately a typed, debug-only callable
    /// boundary.  Release artifacts keep the ordinary direct Rust calls and
    /// do not carry a mutable dispatch table.
    fn native_dispatch_eligible(
        &self,
        function: &MirFunction,
        method_form: Option<&MirFunctionForm>,
        serde_codec: Option<MirSerdeCodec>,
        return_override: Option<&String>,
    ) -> bool {
        if self.config.target_kind != MirRustTarget::Native
        || self.is_no_os()
            || self.has_test_scope(function)
            || !self.config.execution.emit_debug_linemap
            || method_form.is_some()
            || serde_codec.is_some()
            || return_override.is_some()
            || !matches!(function.form, MirFunctionForm::TopLevel)
            || function.kind != MirFunctionKind::Jet
            || !function.generic_params.is_empty()
            || !function.capture_params.is_empty()
            || function.generator.is_some()
            || function.is_unsafe
            || function.params.iter().any(|param| {
                param.access != MirAccess::Read || !self.native_dispatch_type_supported(&param.ty)
            })
            || !self.native_dispatch_type_supported(&function.return_type)
        {
            return false;
        }
        // A candidate shared object may not bring a second effect/resource
        // universe into the stopped process.  Leaf, effect-free callables are
        // the only ones whose code can be loaded without a state transfer.
        function.effects.direct.is_empty()
            && function.effects.solved.is_empty()
            && function.effects.call_edges.is_empty()
    }

    fn native_dispatch_type_supported(&self, ty: &MirType) -> bool {
        matches!(
            ty.kind(),
            MirTypeKind::Float
                | MirTypeKind::Bool
                | MirTypeKind::Char
                | MirTypeKind::IntN { .. }
                | MirTypeKind::Float32
        ) && matches!(ty.layout.abi, MirAbi::Scalar(_))
    }
    /// D-TEST1 / #2502: generated property inputs are admitted only for the
    /// same closed scalar/container family checked by sema. The emitter keeps
    /// this mirror deliberately structural: MIR has already erased source
    /// aliases, while unsupported nominal carriers remain an explicit E0613
    /// unavailable case rather than reaching a `<T as JetGen>` compile error.
    fn property_param_generator_supported(&self, ty: &MirType) -> bool {
        match ty.kind() {
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::Float32
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. } => true,
            MirTypeKind::List(inner)
            | MirTypeKind::Option(inner)
            | MirTypeKind::FixedList { elem: inner, .. } => {
                self.property_param_generator_supported(inner)
            }
            MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::Tagged { .. }
            | MirTypeKind::Quantity { .. }
            | MirTypeKind::Union(_)
            | MirTypeKind::Measure(_) => false,
        }
    }

    /// Return the one unavailable reason that should be surfaced for a
    /// property case instead of emitting a runner that guesses at its inputs.
    fn property_generation_unavailable_reason(&self, function: &MirFunction) -> Option<String> {
        if !function.effects.direct.is_empty()
            || !function.effects.solved.is_empty()
            || !function.effects.call_edges.is_empty()
        {
            return Some(format!(
                "E0613: property test `{}` requires an effect-free callable; \
                 generated inputs are unavailable for effectful code",
                function.name
            ));
        }
        function
            .params
            .iter()
            .find(|param| !self.property_param_generator_supported(&param.ty))
            .map(|param| {
                format!(
                    "E0613: property test `{}` parameter `{}` has no built-in generator \
                     for `{}`",
                    function.name,
                    param.name,
                    self.rust_type(&param.ty)
                )
            })
    }

    fn native_dispatch_signature(&self, function: &MirFunction) -> (String, String, String) {
        let params = function
            .params
            .iter()
            .map(|param| format!("{}: {}", mangle(&param.name), self.parameter_type(param)))
            .collect::<Vec<_>>()
            .join(", ");
        let args = function
            .params
            .iter()
            .map(|param| mangle(&param.name))
            .collect::<Vec<_>>()
            .join(", ");
        (params, args, self.rust_type(&function.return_type))
    }

    fn native_artifact_suffix(&self) -> String {
        self.artifact_identity
            .program_digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn native_entry_name(&self, function: &MirFunction) -> String {
        format!(
            "__jet_native_entry_{}_{}",
            self.native_artifact_suffix(),
            self.function_name(function.id)
        )
    }

    fn native_install_name(&self, function: &MirFunction) -> String {
        format!(
            "__jet_native_install_{}_{}",
            self.native_artifact_suffix(),
            self.function_name(function.id)
        )
    }

    fn native_signature_digest(&self, function: &MirFunction) -> String {
        let (params, _, ret) = self.native_dispatch_signature(function);
        jet_foundation::SHA256::sha256_hex(format!("{params}->{ret}").as_bytes())
    }

    fn native_body_digest(&self, function: &MirFunction) -> String {
        jet_foundation::SHA256::sha256_hex(format!("{:?}", function.blocks).as_bytes())
    }

    fn emit_native_dispatch(&self, function: &MirFunction, out: &mut String) {
        let (params, args, ret) = self.native_dispatch_signature(function);
        let implementation = format!("__jet_native_impl_{}", self.function_name(function.id));
        let entry = self.native_entry_name(function);
        let install = self.native_install_name(function);
        self.emit_callable_named(function, out, None, Some(&implementation), None, None);
        let _ = writeln!(
            out,
            "// jet-native-callable: schema=1 artifact={} name={} jet={} entry={} install={} signature={} body={} state=none",
            self.native_artifact_suffix(),
            self.function_name(function.id),
            function.name,
            entry,
            install,
            self.native_signature_digest(function),
            self.native_body_digest(function),
        );
        let _ = writeln!(
            out,
            "#[doc(hidden)]\nstatic __JET_NATIVE_SLOT_{}: ::std::sync::LazyLock<::std::sync::RwLock<extern \"C\" fn({params}) -> {ret}>> = ::std::sync::LazyLock::new(|| ::std::sync::RwLock::new({entry}));",
            self.function_name(function.id)
        );
        let _ = writeln!(
            out,
            "#[inline(never)]\nfn {}({params}) -> {ret} {{\n    let __jet_call = *__JET_NATIVE_SLOT_{}.read().unwrap_or_else(|__jet_poison| __jet_poison.into_inner());\n    __jet_call({args})\n}}",
            self.function_name(function.id),
            self.function_name(function.id),
        );
        let _ = writeln!(
            out,
            "#[no_mangle]\n#[inline(never)]\npub extern \"C\" fn {entry}({params}) -> {ret} {{ {implementation}({args}) }}\n"
        );
        let _ = writeln!(
            out,
            "#[no_mangle]\n#[inline(never)]\npub extern \"C\" fn {install}(candidate: extern \"C\" fn({params}) -> {ret}) {{\n    *__JET_NATIVE_SLOT_{}.write().unwrap_or_else(|__jet_poison| __jet_poison.into_inner()) = candidate;\n}}\n",
            self.function_name(function.id),
        );
    }
    fn function_row(&self, id: MirFunctionId) -> &MirFunction {
        self.program
            .functions
            .iter()
            .find(|function| function.id == id)
            .unwrap_or_else(|| panic!("MIR function ID {:?} has no function row", id))
    }

    fn coverage_branch_id(
        &self,
        function: &MirFunction,
        block: MirBlockId,
        arm: usize,
    ) -> String {
        let mut ordinal = 0;
        for candidate in &function.blocks {
            match &candidate.terminator {
                MirTerminator::Branch { .. } => {
                    ordinal += 1;
                    if candidate.id == block && arm == 0 {
                        return format!("{}#branch{ordinal}", function.key);
                    }
                }
                MirTerminator::Switch { arms, .. } => {
                    for index in 0..arms.len() {
                        ordinal += 1;
                        if candidate.id == block && index == arm {
                            return format!("{}#branch{ordinal}", function.key);
                        }
                    }
                }
                MirTerminator::Jump { .. }
                | MirTerminator::Return { .. }
                | MirTerminator::Yield { .. }
                | MirTerminator::Break { .. }
                | MirTerminator::Continue { .. }
                | MirTerminator::Unreachable { .. } => {}
            }
        }
        panic!(
            "MIR coverage branch has no ordinal for function {:?}, block {:?}, arm {}",
            function.id, block, arm
        );
    }

    fn coverage_branch_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for function in &self.program.functions {
            if !self.coverage_for(function)
                || !self.module_selected(function.module_id)
                || !self.selected_for_target(function)
            {
                continue;
            }
            let mut ordinal = 0;
            for block in &function.blocks {
                let count = match &block.terminator {
                    MirTerminator::Branch { .. } => 1,
                    MirTerminator::Switch { arms, .. } => arms.len(),
                    MirTerminator::Jump { .. }
                    | MirTerminator::Return { .. }
                    | MirTerminator::Yield { .. }
                    | MirTerminator::Break { .. }
                    | MirTerminator::Continue { .. }
                    | MirTerminator::Unreachable { .. } => 0,
                };
                for _ in 0..count {
                    ordinal += 1;
                    rows.push((
                        format!("{}#branch{ordinal}", function.key),
                        function.name.clone(),
                    ));
                }
            }
        }
        rows
    }
    fn foreign_name(&self, id: MirForeignId) -> String {
        self.foreigns
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("MIR foreign ID {:?} has no foreign row", id))
    }

    fn type_name(&self, id: MirTypeId) -> String {
        if let Some(name) = self
            .history_type_def(id)
            .and_then(|def| self.history_native_type_name(&def.key))
        {
            return name;
        }
        if let Some(def) = self.program.types.iter().find(|def| def.id == id) {
            if crate::Codegen::core_rust_type_name(&def.key).is_some()
                || crate::Codegen::root_prelude_rust_type_name(&def.key).is_some()
                || crate::Codegen::compute_handle_rust_type(&def.key).is_some()
            {
                return self.rust_apply_type(&MirNominalRef::from_name(&def.key), &[]);
            }
        }
        self.types
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("MIR type ID {:?} has no type row", id))
    }

    fn push_generic_scope(&self, params: &'a [MirGenericParam]) {
        self.generic_scopes.borrow_mut().push(params);
    }

    fn pop_generic_scope(&self) {
        self.generic_scopes
            .borrow_mut()
            .pop()
            .expect("MIR generic scope stack underflow");
    }

    fn generic_params_for_type(&self, ty: &MirType) -> &'a [MirGenericParam] {
        let Some(definition) = self.structural_type_def_for(ty) else {
            return &[];
        };
        let binds_definition_params = match ty.kind() {
            MirTypeKind::Apply { args, .. } if args.is_empty() => true,
            MirTypeKind::Apply { args, .. }
                if args.len() == definition.generic_params.len() =>
            {
                args.iter()
                    .zip(&definition.generic_params)
                    .all(|(arg, param)| {
                        matches!(
                            arg.kind(),
                            MirTypeKind::Apply { name, args }
                                if args.is_empty() && name.name == param.name
                        )
                    })
            }
            _ => false,
        };
        if binds_definition_params {
            &definition.generic_params
        } else {
            &[]
        }
    }

    fn is_active_generic_param(&self, name: &str) -> bool {
        if self
            .generic_scopes
            .borrow()
            .iter()
            .rev()
            .any(|scope| scope.iter().any(|param| param.name == name))
        {
            return true;
        }
        self.history_current_function
            .get()
            .is_some_and(|function| {
                self.function_row(function)
                    .generic_params
                    .iter()
                    .any(|param| param.name == name)
            })
    }

    fn nominal_name(&self, name: &str) -> String {
        if self.is_active_generic_param(name) {
            return mangle(name);
        }
        if crate::Codegen::core_rust_type_name(name).is_some()
            || crate::Codegen::root_prelude_rust_type_name(name).is_some()
            || crate::Codegen::compute_handle_rust_type(name).is_some()
            || crate::Codegen::alloc_handle_rust_type(name).is_some()
        {
            return self.rust_apply_type(&MirNominalRef::from_name(name), &[]);
        }
        if let Some(root_name) = crate::Codegen::file_handle_rust_type(name) {
            return format!("{}{}", self.config.root_prefix, root_name);
        }
        if name == "ScopeGuard" {
            return "_".to_string();
        }
        self.types_by_name
            .get(name)
            .cloned()
            .or_else(|| {
                crate::Codegen::core_crypto_rust_type_name(name)
                    .map(|name| format!("jet_ffi::{name}"))
            })
            .unwrap_or_else(|| panic!("MIR nominal type {name:?} has no declaration row"))
    }

    fn canonical_type(&self, ty: &MirType) -> &MirType {
        let id = ty
            .identity
            .unwrap_or_else(|| panic!("MIR type has no canonical identity"));
        self.type_instances
            .get(&id)
            .copied()
            .unwrap_or_else(|| panic!("MIR type identity {:?} has no instance row", id))
    }

    fn trait_object_type(&self, id: MirTypeId) -> &MirType {
        let ty = self
            .type_instances
            .get(&id)
            .copied()
            .unwrap_or_else(|| panic!("MIR trait coercion target {:?} has no instance row", id));
        if !matches!(ty.kind(), MirTypeKind::TraitObject(_)) {
            panic!("MIR trait coercion target {:?} is not a trait object", id);
        }
        ty
    }

    fn type_identity(&self, ty: &MirType) -> MirTypeId {
        if let Some(id) = ty.identity {
            if self.type_instances.contains_key(&id) {
                return id;
            }
            panic!("MIR type identity {:?} has no instance row", id);
        }
        self.type_instances
            .iter()
            .find(|(_, instance)| instance.same_checked_type(ty))
            .map(|(id, _)| *id)
            .unwrap_or_else(|| {
                panic!(
                    "MIR structural type {:?} has no canonical instance",
                    ty.display_name()
                )
            })
    }
    fn columnar_type_def(&self, ty: &MirType) -> Option<&MirTypeDef> {
        let ty = if ty.identity.is_some() {
            self.canonical_type(ty)
        } else {
            ty
        };
        let name = match ty.kind() {
            MirTypeKind::Apply { name, .. } => &name.name,
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::Map { .. }
            | MirTypeKind::List(_)
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Tagged { .. }
            | MirTypeKind::Quantity { .. }
            | MirTypeKind::Union(_)
            | MirTypeKind::Measure(_) => return None,
        };
        self.program.types.iter().find(|definition| {
            definition.layout == Some(MirStructLayout::Columnar)
                && (definition.key == name.as_str() || definition.name == name.as_str())
        })
    }

    fn is_columnar_list(&self, ty: &MirType) -> bool {
        match ty.kind() {
            MirTypeKind::List(inner) => self.columnar_type_def(inner).is_some(),
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Tagged { .. }
            | MirTypeKind::Quantity { .. }
            | MirTypeKind::Union(_)
            | MirTypeKind::Measure(_) => false,
        }
    }

    fn field_name(&self, id: MirFieldId) -> String {
        self.fields
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("MIR field ID {:?} has no field row", id))
    }

    fn boxed_field(&self, id: MirFieldId) -> bool {
        let row = self
            .program
            .fields
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("MIR field ID {:?} has no field row", id));
        if self
            .type_instances
            .get(&row.owner)
            .is_some_and(|owner| matches!(owner.kind(), MirTypeKind::Tuple(_)))
        {
            return false;
        }
        self.type_def(row.owner)
            .boxed_edges
            .iter()
            .any(|edge| edge == &row.field.name)
    }

    fn type_def(&self, id: MirTypeId) -> &MirTypeDef {
        let id = match self.type_instances.get(&id).map(|instance| instance.kind()) {
            Some(MirTypeKind::Apply { name, .. }) => name.id,
            _ => id,
        };
        self.program
            .types
            .iter()
            .find(|def| def.id == id)
            .unwrap_or_else(|| panic!("MIR type ID {:?} has no type definition", id))
    }
    fn rust_decl_type(&self, definition: &MirTypeDef, edge: &str, ty: &MirType) -> String {
        let rendered = self.rust_type(ty);
        if definition
            .boxed_edges
            .iter()
            .any(|candidate| candidate == edge)
        {
            format!("Box<{rendered}>")
        } else {
            rendered
        }
    }

    fn is_default_err_type(&self, id: MirTypeId) -> bool {
        self.type_def(id).key == jet_foundation::Syntax::TYPE_ERR
    }

    fn named_struct_field_value(
        &self,
        type_id: MirTypeId,
        fields: &[(MirFieldId, MirValueId)],
        name: &str,
    ) -> Option<String> {
        let MirTypeDefKind::Struct {
            fields: declared, ..
        } = &self.type_def(type_id).kind
        else {
            return None;
        };
        let field = declared.iter().find(|field| field.name == name)?;
        let (_, value) = fields.iter().find(|(id, _)| *id == field.id)?;
        Some(self.value_move(*value))
    }

    fn default_err_constructor(
        &self,
        type_id: MirTypeId,
        fields: &[(MirFieldId, MirValueId)],
    ) -> String {
        let root = &self.config.root_prefix;
        let absent = format!("{root}JetOutcome::Err({root}JetAbsent)");
        let message = self
            .named_struct_field_value(type_id, fields, "message")
            .unwrap_or_else(|| panic!("MIR default Err literal is missing message"));
        let code = self
            .named_struct_field_value(type_id, fields, "code")
            .unwrap_or_else(|| absent.clone());
        let cause = self
            .named_struct_field_value(type_id, fields, "cause")
            .unwrap_or_else(|| absent);
        // JetErr fields are private; every adapter constructs through this
        // Prelude function so cause boxing and identity stay in one place.
        format!("{root}jet_err({message}, {code}, {cause})")
    }

    fn rust_struct_literal(
        &self,
        type_id: MirTypeId,
        fields: &[(MirFieldId, MirValueId)],
        boxed_fields: &[MirFieldId],
        apply_history: bool,
    ) -> String {
        if self.is_default_err_type(type_id) {
            return self.default_err_constructor(type_id, fields);
        }
        let fields = fields
            .iter()
            .map(|(field, value)| {
                let mut value = self.value_move(*value);
                if apply_history {
                    value = self.history_struct_field_value(type_id, *field, value);
                }
                if boxed_fields.contains(field) {
                    format!("{}: Box::new({value})", self.field_name(*field))
                } else {
                    format!("{}: {value}", self.field_name(*field))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} {{ {fields} }}", self.type_name(type_id))
    }

    fn prelude_row(&self, id: MirPreludeCallId) -> &MirPreludeCall {
        self.program
            .prelude_calls
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("MIR Prelude call ID {:?} has no row", id))
    }
    fn history_scalar_spec(&self, ty: &MirType) -> Option<(HistoryScalarKind, Option<(i64, i64)>)> {
        match ty.kind() {
            MirTypeKind::Int => Some((HistoryScalarKind::Integer, None)),
            MirTypeKind::Bool => Some((HistoryScalarKind::Boolean, None)),
            MirTypeKind::String => Some((HistoryScalarKind::Text, None)),
            MirTypeKind::Char => Some((HistoryScalarKind::Char, None)),
            MirTypeKind::IntN { signed, .. } => Some((
                if *signed {
                    HistoryScalarKind::SignedInteger
                } else {
                    HistoryScalarKind::UnsignedInteger
                },
                None,
            )),
            MirTypeKind::InlineRange { base, lo, hi } if lo <= hi => self
                .history_scalar_spec(base)
                .map(|(kind, _)| (kind, Some((*lo, *hi)))),
            MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                self.history_scalar_spec(inner)
            }
            _ => None,
        }
    }

    fn history_command_type_ids(&self) -> Vec<MirTypeId> {
        let mut ids = BTreeSet::new();
        for function in &self.program.functions {
            for block in &function.blocks {
                for instruction in &block.instructions {
                    let (call, type_args) = match &instruction.operation {
                        MirOperation::CoreCall {
                            call, type_args, ..
                        }
                        | MirOperation::Call {
                            callee: MirCallee::Core(call),
                            type_args,
                            ..
                        } => (*call, type_args),
                        _ => continue,
                    };
                    let Some(row) = self.program.core_calls.iter().find(|row| row.id == call)
                    else {
                        continue;
                    };
                    if row.module != "core.testing" || row.member != "histories" {
                        continue;
                    }
                    let Some(target) = type_args.first() else {
                        continue;
                    };
                    let Some(id) = target.identity.or_else(|| {
                        target.nominal_name().and_then(|name| {
                            self.program
                                .types
                                .iter()
                                .find(|definition| {
                                    definition.name == name || definition.key == name
                                })
                                .map(|definition| definition.id)
                        })
                    }) else {
                        continue;
                    };
                    ids.insert(id);
                }
            }
        }
        ids.into_iter().collect()
    }

    fn history_type_def(&self, id: MirTypeId) -> Option<&MirTypeDef> {
        self.program
            .types
            .iter()
            .find(|definition| definition.id == id)
    }

    fn prelude_symbol(&self, id: MirPreludeCallId) -> String {
        match &self.prelude_row(id).symbol {
            MirSymbol::Prelude(symbol) => format!("{}{}", self.config.root_prefix, symbol),
            MirSymbol::Runtime(symbol) => symbol.clone(),
        }
    }

    fn core_symbol(&self, id: MirCoreCallId) -> String {
        let row = self
            .program
            .core_calls
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("MIR Core call ID {:?} has no row", id));
        match row.symbol {
            CoreCallSymbol::Prelude(symbol) => format!("{}{}", self.config.root_prefix, symbol),
            CoreCallSymbol::Rust(symbol) => symbol.to_string(),
        }
    }
    fn core_row(&self, id: MirCoreCallId) -> &MirCoreCall {
        self.program
            .core_calls
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("MIR Core call ID {:?} has no row", id))
    }

    fn validate_core_arity(&self, row: &MirCoreCall, args: &[MirCallArg]) {
        if args.len() < row.arity || args.len() > row.max_arity {
            panic!(
                "MIR Core call {:?} has {} arguments, expected {}..={}",
                row.id,
                args.len(),
                row.arity,
                row.max_arity
            );
        }
        if row.borrow_mask.len() < row.arity {
            panic!(
                "MIR Core call {:?} borrow mask has {} entries for its minimum {} arguments",
                row.id,
                row.borrow_mask.len(),
                row.arity
            );
        }
    }

    fn model_open_call(
        &self,
        function: &MirFunction,
        args: &[MirCallArg],
        type_args: &[MirType],
        row: &MirCoreCall,
    ) -> String {
        let (definition, _) = self
            .model_trait_definition(type_args)
            .unwrap_or_else(|| panic!("model.open has no checked source trait binding"));
        self.call_symbol_for_function(
            function,
            self.model_adapter_symbol(definition),
            args,
            &[],
            Some(&row.borrow_mask),
        )
    }

    fn core_direct_call(
        &self,
        function: &MirFunction,
        id: MirCoreCallId,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let row = self.core_row(id);
        self.validate_core_arity(row, args);
        if row.module == "core.testing" && row.member == "histories" {
            return self.history_call_expression(function, args, type_args, &row.borrow_mask);
        }
        if row.module == "core.models" && row.member == "open" {
            return self.model_open_call(function, args, type_args, row);
        }
        self.call_symbol_for_function(
            function,
            self.core_symbol(id),
            args,
            type_args,
            Some(&row.borrow_mask),
        )
    }
    fn validate_prelude_count(&self, row: &MirPreludeCall, count: usize) {
        if count < row.signature.arity || count > row.signature.max_arity {
            panic!(
                "MIR Prelude call {:?} has {} arguments, expected {}..={}",
                row.id, count, row.signature.arity, row.signature.max_arity
            );
        }
        if row.signature.borrow_mask.len() != count {
            panic!(
                "MIR Prelude call {:?} borrow mask has {} entries for {} arguments",
                row.id,
                row.signature.borrow_mask.len(),
                count
            );
        }
    }

    fn validate_prelude_arity(&self, row: &MirPreludeCall, args: &[MirCallArg]) {
        self.validate_prelude_count(row, args.len());
    }
    fn append_prelude_context(
        &self,
        call: MirPreludeCallId,
        args: &mut Vec<String>,
        extras: &[String],
    ) {
        let route = self.prelude_row(call);
        let additional = route
            .signature
            .arity
            .checked_sub(args.len())
            .unwrap_or_else(|| {
                panic!(
                    "MIR Prelude call {:?} has fewer arguments than its emitted prefix",
                    call
                )
            });
        if additional > extras.len() {
            panic!(
                "MIR Prelude call {:?} requires {} context arguments but MIR supplied {}",
                call,
                additional,
                extras.len()
            );
        }
        args.extend(extras.iter().take(additional).cloned());
    }

    fn rust_local_type(&self, ty: &MirType) -> String {
        if let MirTypeKind::Result { ok, err } = ty.kind() {
            if let Some(inner) = allocator_view_inner(ok) {
                if err.name().starts_with("AllocError") {
                    return format!(
                        "{}JetOutcome<{}, {}>",
                        self.config.root_prefix,
                        self.rust_type(inner),
                        self.rust_type(err)
                    );
                }
            }
        }
        let previous = self.history_callback_lifetime.replace("'_");
        let rendered = self.rust_type(ty);
        self.history_callback_lifetime.set(previous);
        rendered
    }

    fn rust_parameter_type(&self, ty: &MirType) -> String {
        if self.history_callback_lifetime.get() == "'__jet_callback" {
            self.rust_type(ty)
        } else {
            self.rust_local_type(ty)
        }
    }

    fn rust_type(&self, ty: &MirType) -> String {
        let ty = if ty.identity.is_some() {
            self.canonical_type(ty)
        } else {
            ty
        };
        match ty.kind() {
            MirTypeKind::Int => "jet_foundation::Numeric::JetInt".to_string(),
            MirTypeKind::Float => "f64".to_string(),
            MirTypeKind::Bool => "bool".to_string(),
            MirTypeKind::String => {
                if self.is_core_layer() {
                    "&'static str".to_string()
                } else {
                    "String".to_string()
                }
            }
            MirTypeKind::Char => "char".to_string(),
            MirTypeKind::List(inner) => {
                if self.columnar_type_def(inner).is_some() {
                    format!(
                        "{}JetColumnList<{}>",
                        self.config.root_prefix,
                        self.rust_type(inner)
                    )
                } else {
                    format!("Vec<{}>", self.rust_type(inner))
                }
            }
            MirTypeKind::Map { key, value } => format!(
                "{}JetMap<{}, {}>",
                self.config.root_prefix,
                self.rust_type(key),
                self.rust_type(value)
            ),
            MirTypeKind::Shared(inner) => format!(
                "{}jet_std::JetShared<{}>",
                self.config.root_prefix,
                self.rust_type(inner)
            ),
            MirTypeKind::Option(inner) => format!(
                "{}JetOutcome<{}, {}JetAbsent>",
                self.config.root_prefix,
                self.rust_type(inner),
                self.config.root_prefix
            ),
            MirTypeKind::Result { ok, err } => format!(
                "{}JetOutcome<{}, {}>",
                self.config.root_prefix,
                self.rust_type(ok),
                self.rust_type(err)
            ),
            MirTypeKind::Fn(signature) => {
                let conventions = signature
                    .call_metadata
                    .as_ref()
                    .map(|metadata| metadata.conventions.as_slice());
                let parameter_types = signature
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        let access = Self::callable_parameter_access(conventions, index);
                        let rendered = self.rust_type(param);
                        let mode = self.callable_parameter_mode(param, access);
                        let callable = match mode {
                            'W' => format!("&mut {rendered}"),
                            'R' => format!("&{rendered}"),
                            'O' => rendered.clone(),
                            other => panic!("MIR callable has unknown mode `{other}`"),
                        };
                        (rendered, callable, mode)
                    })
                    .collect::<Vec<_>>();
                let params = parameter_types
                    .iter()
                    .map(|(_, callable, _)| callable.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = signature
                    .ret
                    .as_ref()
                    .map(|ret| self.rust_type(ret))
                    .unwrap_or_else(|| "()".to_string());
                if self.history_runtime_metadata_enabled() {
                    let arity = signature.params.len();
                    let modes = parameter_types
                        .iter()
                        .map(|(_, _, mode)| *mode)
                        .collect::<String>();
                    let borrowed_trait = modes.chars().any(|mode| mode != 'O');
                    let callable_name = if borrowed_trait {
                        self.history_callable_modes
                            .borrow_mut()
                            .insert((arity, modes.clone()));
                        format!(
                            "{}JetHistoryFn{arity}B{modes}",
                            self.config.root_prefix
                        )
                    } else {
                        self.history_callable_arities.borrow_mut().insert(arity);
                        format!("{}JetHistoryFn{arity}", self.config.root_prefix)
                    };
                    let generic_params = if borrowed_trait {
                        parameter_types
                            .iter()
                            .map(|(rendered, _, _)| rendered.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        params.clone()
                    };
                    let generic_params = if generic_params.is_empty() {
                        String::new()
                    } else {
                        format!("{generic_params}, ")
                    };
                    let lifetime = self.history_callback_lifetime.get();
                    format!(
                        "Box<dyn {callable_name}<{lifetime}, {generic_params}{ret}> + {lifetime}>"
                    )
                } else {
                    format!(
                        "std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnMut({params}) -> {ret}>>>>"
                    )
                }
            }
            MirTypeKind::SendFn { params, ret } => {
                let params = params
                    .iter()
                    .map(|param| self.callable_parameter_type(param, MirAccess::Read))
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = ret
                    .as_ref()
                    .map(|ret| self.rust_type(ret))
                    .unwrap_or_else(|| "()".to_string());
                format!("std::sync::Arc<dyn Fn({params}) -> {ret} + Send + Sync + 'static>")
            }
            MirTypeKind::Apply { name, args } => self.rust_apply_type(name, args),
            MirTypeKind::TraitObject(bounds) => format!(
                "Box<dyn {}>",
                bounds
                    .iter()
                    .map(|bound| {
                        self.traits_by_name
                            .get(&bound.name)
                            .cloned()
                            .unwrap_or_else(|| {
                                panic!("MIR trait {:?} has no declaration row", bound.name)
                            })
                    })
                    .collect::<Vec<_>>()
                    .join(" + ")
            ),
            MirTypeKind::Tuple(fields) => {
                let values = fields
                    .iter()
                    .map(|(_, field)| self.rust_type(field))
                    .collect::<Vec<_>>();
                match values.len() {
                    0 => "()".to_string(),
                    1 => format!("({},)", values[0]),
                    _ => format!("({})", values.join(", ")),
                }
            }
            MirTypeKind::FixedList { elem, len } => {
                format!("[{}; {}]", self.rust_type(elem), len.expression())
            }
            MirTypeKind::IntN { signed, bits } => {
                format!("{}{}", if *signed { 'i' } else { 'u' }, bits)
            }
            MirTypeKind::Tagged {
                marker: MirTagMarker::Internal(MirInternalTag::AllocatorView),
                inner,
            } => format!("&mut {}", self.rust_type(inner)),
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.rust_type(base),

            MirTypeKind::Float32 => "f32".to_string(),
            MirTypeKind::Union(_) => ty
                .identity
                .map(|id| self.type_name(id))
                .unwrap_or_else(|| panic!("MIR union type has no canonical type identity")),
            MirTypeKind::Measure(_) => "f64".to_string(),
        }
    }
    fn emit_web_data_type_registration(&self, out: &mut String) {
        if self.config.target_kind != MirRustTarget::WebWasm {
            return;
        }
        let mut types = BTreeMap::<String, String>::new();
        for function in &self.program.functions {
            for block in &function.blocks {
                for instruction in &block.instructions {
                    let MirOperation::CoreCall {
                        route, type_args, ..
                    } = &instruction.operation
                    else {
                        continue;
                    };
                    let row = self
                        .program
                        .prelude_calls
                        .iter()
                        .find(|row| row.id == *route)
                        .unwrap_or_else(|| panic!("MIR Prelude call ID {:?} has no row", route));
                    let MirSymbol::Prelude(name) = &row.symbol else {
                        continue;
                    };
                    if !(name.starts_with("jet_data") || name.starts_with("jet_data.loader.")) {
                        continue;
                    }
                    let Some(ty) = type_args.first() else {
                        continue;
                    };
                    types.entry(ty.name()).or_insert_with(|| self.rust_type(ty));
                }
            }
        }
        if types.is_empty() {
            return;
        }
        out.push_str("#[cfg(target_arch = \"wasm32\")]\n");
        out.push_str("#[no_mangle]\n");
        out.push_str("pub extern \"C\" fn jet_data_web_register_types() {\n");
        for (key, ty) in types {
            let _ = writeln!(
                out,
                "    {}::<{}>({:?});",
                format!("{}jet_data_web_register_type", self.config.root_prefix),
                ty,
                key
            );
        }
        out.push_str("}\n");
    }
    fn rust_root_prelude_type(&self, root_name: &str, args: &[MirType]) -> String {
        let head = format!("{}{}", self.config.root_prefix, root_name);
        if args.is_empty() {
            return head;
        }
        let args = args
            .iter()
            .map(|arg| self.rust_type(arg))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{head}<{args}>")
    }

    fn rust_apply_type(&self, name: &MirNominalRef, args: &[MirType]) -> String {
        if self.is_active_generic_param(&name.name) {
            if !args.is_empty() {
                panic!(
                    "MIR generic type parameter {:?} has type arguments",
                    name.name
                );
            }
            return self.nominal_name(&name.name);
        }
        if args.is_empty() && name.name == jet_foundation::Syntax::INTERNAL_UNIT_TYPE {
            return "()".to_string();
        }
        if args.is_empty() && name.name == jet_foundation::Syntax::TYPE_NEVER {
            return "std::convert::Infallible".to_string();
        }
        if name.name == jet_foundation::Syntax::TYPE_PTR {
            let [element] = args else {
                panic!("MIR Ptr type must have exactly one element argument");
            };
            return format!("*mut {}", self.rust_type(element));
        }
        if args.is_empty() && name.name == jet_foundation::Syntax::TYPE_TASKGROUP {
            return format!("{}jet_std::JetTaskGroup", self.config.root_prefix);
        }
        if name.name == "Instant" {
            return self.rust_root_prelude_type("JetInstant", args);
        }
        if name.name == "RealtimeStream" {
            return format!("{}jet_std::JetRealtimeStream", self.config.root_prefix);
        }
        if name.name == "RealtimeReceipt" {
            return format!("{}jet_std::JetRealtimeReceipt", self.config.root_prefix);
        }
        if let Some(root_name) = crate::Codegen::file_handle_rust_type(&name.name) {
            return format!("{}{}", self.config.root_prefix, root_name);
        }
        // ScopeGuard is an inference-only carrier. The checked Core call
        // supplies its closure initializer, so no nominal declaration row is
        // required in the MIR type table.
        if name.name == "ScopeGuard" {
            return "_".to_string();
        }
        if name.name == "RangeCursor" {
            return "JetLoopRangeCursor".to_string();
        }
        if name.name == "IterCursor" {
            return "JetLoopIterCursor".to_string();
        }
        if name.name == "Atomic" {
            if args.len() != 1 {
                panic!("MIR Atomic type must have exactly one value argument");
            }
            let inner = if matches!(args[0].kind(), MirTypeKind::Int) {
                "JetAtomicInt".to_string()
            } else {
                self.rust_type(&args[0])
            };
            return format!("{}JetAtomic<{inner}>", self.config.root_prefix);
        }
        if matches!(name.name.as_str(), "Tensor" | "Vec" | "Matrix")
            && (name.name != "Tensor" || args.len() <= 1)
        {
            return self.rust_root_prelude_type("JetTensor", &[]);
        }
        if name.name == "Task" {
            let [item] = args else {
                panic!("MIR Task type must have exactly one value argument");
            };
            let carrier = self.rust_type(item);
            let carrier = if item.result_parts().is_some() {
                carrier
            } else {
                format!("Result<{carrier}, {}JetErr>", self.config.root_prefix)
            };
            return format!("{}jet_std::JetTask<{carrier}>", self.config.root_prefix);
        }
        if let Some(email_name) = crate::Codegen::core_email_rust_type_name(&name.name) {
            let root_name = format!("jet_email::{email_name}");
            return self.rust_root_prelude_type(&root_name, args);
        }
        if let Some(root_name) = crate::Codegen::root_prelude_rust_type_name(&name.name) {
            return self.rust_root_prelude_type(root_name, args);
        }
        if let Some(history_name) = crate::Codegen::history_rust_type_name(&name.name) {
            let carrier = self
                .history_native_type_name(&name.name)
                .unwrap_or_else(|| {
                    format!("crate::jet_testing_history_foundation::{history_name}")
                });
            if args.is_empty() {
                return carrier;
            }
            let args = args
                .iter()
                .map(|arg| self.rust_type(arg))
                .collect::<Vec<_>>()
                .join(", ");
            return format!("{carrier}<{args}>");
        }
        if let Some(compute_name) = crate::Codegen::compute_handle_rust_type(&name.name) {
            return self.rust_root_prelude_type(compute_name, args);
        }
        if let Some(service_name) = crate::Codegen::service_handle_rust_type(&name.name) {
            return self.rust_root_prelude_type(service_name, args);
        }
        // `Pool` is both the allocator sentinel and the generic collection
        // family.  A bare `Pool` is the allocator handle; `Pool<T>` remains
        // the collection runtime type.
        if args.is_empty() {
            if let Some(allocator) = crate::Codegen::alloc_handle_rust_type(&name.name) {
                return format!("{}{}", self.config.root_prefix, allocator);
            }
        }
        if let Some(core_name) = crate::Codegen::core_rust_type_name(&name.name) {
            // D-CONC-FAIL1=A: TaskFailure is Foundation's root carrier, unlike
            // the other core names that live below `jet_std`.
            if core_name == "JetTaskFailure" {
                return self.rust_root_prelude_type(core_name, args);
            }
            if let Some(root_name) = crate::Codegen::root_prelude_rust_type_name(core_name) {
                return self.rust_root_prelude_type(root_name, args);
            }
            let mut native = format!("{}jet_std::{core_name}", self.config.root_prefix);
            if !args.is_empty() {
                native.push('<');
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        native.push_str(", ");
                    }
                    native.push_str(&self.rust_type(arg));
                }
                native.push('>');
            }
            return native;
        }
        if name.name == "__JetDmaTransfer" {
            if args.len() != 1 {
                panic!("MIR DMA transfer carrier must have exactly one buffer type argument");
            }
            return format!(
                "{}jet_std::JetDmaTransfer<'static, {}>",
                self.config.root_prefix,
                self.rust_type(&args[0])
            );
        }
        if name.name == "StreamEventTime" {
            if args.len() != 1 {
                panic!("MIR StreamEventTime type must have exactly one item argument");
            }
            return format!(
                "{}jet_std::JetStream<{}jet_std::JetStreamEvent<{}>>",
                self.config.root_prefix,
                self.config.root_prefix,
                self.rust_type(&args[0])
            );
        }
        if name.name == "KeyedStream" {
            if args.len() != 2 {
                panic!("MIR KeyedStream type must have key and item arguments");
            }
            return format!(
                "{}jet_std::JetKeyedStream<{}, {}>",
                self.config.root_prefix,
                self.rust_type(&args[0]),
                self.rust_type(&args[1])
            );
        }
        if name.name == "Window" {
            if args.len() != 2 {
                panic!("MIR Window type must have key and item arguments");
            }
            return format!(
                "{}jet_std::JetStreamWindow<{}, {}>",
                self.config.root_prefix,
                self.rust_type(&args[0]),
                self.rust_type(&args[1])
            );
        }
        if name.name == jet_foundation::Syntax::TYPE_STREAM {
            if args.len() != 1 {
                panic!("MIR Stream type must have exactly one item argument");
            }
            return format!(
                "{}jet_std::JetStream<{}>",
                self.config.root_prefix,
                self.rust_type(&args[0])
            );
        }
        if let Some(allocator) = crate::Codegen::alloc_handle_rust_type(&name.name) {
            return format!("{}{}", self.config.root_prefix, allocator);
        }
        let head = self.nominal_name(&name.name);
        if args.is_empty() {
            head
        } else {
            let args = args
                .iter()
                .map(|arg| self.rust_type(arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{head}<{args}>")
        }
    }

    fn is_scalar(&self, ty: &MirType) -> bool {
        if self.is_core_layer() && ty.is_string() {
            return true;
        }
        // JetInt has scalar layout but owns a Foundation node. Treat it as an
        // aggregate for parameter passing so Read never moves its owner.
        if matches!(ty.kind(), MirTypeKind::Int) {
            return false;
        }
        match ty.layout.abi {
            MirAbi::Scalar(_) => true,
            MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic
            | MirAbi::Never => false,
        }
    }
    fn is_handle_name(&self, name: &str) -> bool {
        self.program
            .handles
            .iter()
            .any(|handle| handle.ty.nominal_name() == Some(name))
    }

    fn is_handle_type(&self, ty: &MirType) -> bool {
        self.program.handles.iter().any(|handle| {
            handle.ty.same_checked_type(ty)
                || handle.ty.nominal_name() == ty.nominal_name() && ty.nominal_name().is_some()
        })
    }

    fn handle_for_type(&self, ty: &MirType) -> Option<&MirHandleLifecycle> {
        self.program.handles.iter().find(|handle| {
            handle.ty.same_checked_type(ty)
                || handle.ty.nominal_name() == ty.nominal_name() && ty.nominal_name().is_some()
        })
    }

    fn emit_handle_type(&self, handle: &MirHandleLifecycle, out: &mut String) {
        let name = self.rust_type(&handle.ty);
        let handle_id = handle.id.0;
        let close_foreign = handle.close_foreign.filter(|close_id| {
            self.program
                .foreign
                .iter()
                .find(|foreign| foreign.id == *close_id)
                .is_some_and(|foreign| self.target_applicable(foreign.target_applicability))
        });
        let close_function = handle
            .close
            .filter(|close_id| self.selected_for_target(self.function_row(*close_id)));
        let drop_impl = if matches!(handle.ownership, MirHandleOwnership::Owned) {
            if let Some(close_id) = close_foreign {
                let close = self.foreign_name(close_id);
                format!(
                    "impl Drop for {name} {{\n\
                         fn drop(&mut self) {{\n\
                             if let Some((_, __jet_raw)) = self.__jet_token.take_raw() {{\n\
                                 unsafe {{ let _ = {close}(__jet_raw as *mut core::ffi::c_void); }}\n\
                             }}\n\
                         }}\n\
                     }}\n"
                )
            } else if let Some(close_id) = close_function {
                let close = self.function_name(close_id);
                let call = if self.function_row(close_id).is_unsafe {
                    format!("unsafe {{ {close}(__jet_handle) }}")
                } else {
                    format!("{close}(__jet_handle)")
                };
                format!(
                    "impl Drop for {name} {{\n\
                         fn drop(&mut self) {{\n\
                             if let Some((_, __jet_raw)) = self.__jet_token.take_raw() {{\n\
                                 let __jet_handle = {name}::from_raw(__jet_raw as *mut core::ffi::c_void);\n\
                                 let _ = {call};\n\
                             }}\n\
                         }}\n\
                     }}\n"
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "#[repr(transparent)]\n#[derive(Debug)]\npub struct {name} {{\n\
                 __jet_token: jet_foundation::MIR::MirHandleToken,\n\
             }}\n\
             impl {name} {{\n\
                 pub fn from_raw(raw: *mut core::ffi::c_void) -> Self {{\n\
                     Self {{\n\
                         __jet_token: jet_foundation::MIR::MirHandleToken::new(\n\
                             jet_foundation::MIR::MirHandleId({handle_id}), raw as i64\n\
                         ),\n\
                     }}\n\
                 }}\n\
                 pub fn as_raw(&self) -> *mut core::ffi::c_void {{\n\
                     self.__jet_token.raw()\n\
                         .map(|raw| raw as *mut core::ffi::c_void)\n\
                         .unwrap_or_else(|| panic!(\"opaque handle was already moved\"))\n\
                 }}\n\
                 pub fn into_raw(self) -> *mut core::ffi::c_void {{\n\
                     self.__jet_token\n\
                         .take_raw()\n\
                         .map(|(_, raw)| raw as *mut core::ffi::c_void)\n\
                         .unwrap_or_else(|| panic!(\"opaque handle was already moved\"))\n\
                 }}\n\
                 pub fn token_identity(&self) -> usize {{ self.__jet_token.identity() }}\n\
             }}\n"
        );
        out.push_str(&drop_impl);
    }

    fn foreign_param_type(
        &self,
        foreign: &MirForeign,
        param: &jet_foundation::MIR::MirParam,
    ) -> String {
        if foreign.callback_transport.as_deref() == Some("native-start") {
            if let MirTypeKind::Fn(signature) = param.ty.kind() {
                let payload = signature
                    .params
                    .first()
                    .and_then(|ty| match ty.kind() {
                        MirTypeKind::Apply { name, args }
                            if name.name == "FfiCallbackEvent" && args.len() == 1 =>
                        {
                            Some(args[0].clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        panic!("native callback start row has no FfiCallbackEvent payload")
                    });
                let ret = signature
                    .ret
                    .as_deref()
                    .map(|ty| format!(" -> {}", self.rust_type(ty)))
                    .unwrap_or_default();
                return format!(
                    "Option<unsafe extern \"C\" fn(*mut core::ffi::c_void, {}){}>",
                    self.rust_type(&payload),
                    ret
                );
            }
        }
        if foreign.handle.is_some() && self.is_handle_type(&param.ty) {
            "*mut core::ffi::c_void".to_string()
        } else {
            self.parameter_type(param)
        }
    }

    fn foreign_return_type(&self, foreign: &MirForeign) -> String {
        if foreign.callback_transport.as_deref() == Some("native-start") {
            return "*mut core::ffi::c_void".to_string();
        }
        foreign
            .return_type
            .as_ref()
            .map(|ty| {
                if foreign.handle.is_some() && self.is_handle_type(ty) {
                    "*mut core::ffi::c_void".to_string()
                } else {
                    self.rust_type(ty)
                }
            })
            .unwrap_or_else(|| "()".to_string())
    }

    fn callable_parameter_access(conventions: Option<&[MirAccess]>, index: usize) -> MirAccess {
        conventions
            .and_then(|conventions| conventions.get(index))
            .copied()
            .unwrap_or(MirAccess::Read)
    }

    /// A `Read` parameter of non-scalar type crosses the call as `&T`. This
    /// one predicate decides the callee signature, the callee's parameter
    /// reads, and the caller's argument marshalling, so the three sides can
    /// never disagree on the Rust calling convention.
    fn callable_parameter_borrowed(&self, ty: &MirType, access: MirAccess) -> bool {
        access == MirAccess::Write
            || access == MirAccess::Read && !self.is_scalar(ty)
    }

    fn callable_parameter_mode(&self, ty: &MirType, access: MirAccess) -> char {
        if access == MirAccess::Write {
            'W'
        } else if self.callable_parameter_borrowed(ty, access) {
            'R'
        } else {
            'O'
        }
    }

    fn callable_parameter_type(&self, ty: &MirType, access: MirAccess) -> String {
        let rendered = self.rust_type(ty);
        match access {
            MirAccess::Write => format!("&mut {rendered}"),
            MirAccess::Read if self.callable_parameter_borrowed(ty, access) => {
                format!("&{rendered}")
            }
            MirAccess::Read | MirAccess::Move => rendered,
        }
    }

    fn callable_argument_from_owned(
        &self,
        ty: &MirType,
        access: MirAccess,
        value: String,
    ) -> String {
        match access {
            MirAccess::Write => format!("&mut {value}"),
            MirAccess::Read if self.callable_parameter_borrowed(ty, access) => {
                format!("&{value}")
            }
            MirAccess::Read | MirAccess::Move => value,
        }
    }

    fn callable_parameters(
        ty: &MirType,
    ) -> impl ExactSizeIterator<Item = (&MirType, MirAccess)> {
        let (params, conventions) = match ty.kind() {
            MirTypeKind::Fn(signature) => (
                signature.params.as_slice(),
                signature
                    .call_metadata
                    .as_ref()
                    .map(|metadata| metadata.conventions.as_slice()),
            ),
            MirTypeKind::SendFn { params, .. } => (params.as_slice(), None),
            other => panic!(
                "MIR callable ABI requested for non-function type {}",
                other.display_name()
            ),
        };
        params.iter().enumerate().map(move |(index, param)| {
            (param, Self::callable_parameter_access(conventions, index))
        })
    }

    fn callable_borrow_mask(&self, ty: &MirType) -> Vec<bool> {
        Self::callable_parameters(ty)
            .map(|(param, access)| self.callable_parameter_borrowed(param, access))
            .collect()
    }

    fn parameter_borrowed(&self, param: &jet_foundation::MIR::MirParam) -> bool {
        self.callable_parameter_borrowed(&param.ty, param.access)
    }

    /// The receiver row of an instance method. The receiver is MIR parameter
    /// 0 whenever the function form carries a receiver access; the form spells
    /// it (`&self`/`&mut self`/`self`), so it is never emitted or marshalled
    /// as a named parameter.
    fn receiver_param<'f>(
        &self,
        function: &'f MirFunction,
    ) -> Option<&'f jet_foundation::MIR::MirParam> {
        let access = match &function.form {
            MirFunctionForm::Method { self_access, .. }
            | MirFunctionForm::TraitMethod { self_access, .. } => *self_access,
            MirFunctionForm::TopLevel => None,
        }?;
        let receiver = function
            .params
            .first()
            .filter(|param| param.index == 0 && param.name == jet_foundation::Syntax::KW_SELF)
            .unwrap_or_else(|| {
                panic!(
                    "MIR instance method {:?} declares a receiver but parameter 0 is not `self`",
                    function.id
                )
            });
        if receiver.access != access {
            panic!(
                "MIR instance method {:?} receiver access {:?} disagrees with its form {:?}",
                function.id, receiver.access, access
            );
        }
        Some(receiver)
    }

    fn is_receiver(&self, function: &MirFunction, param: &jet_foundation::MIR::MirParam) -> bool {
        self.receiver_param(function)
            .is_some_and(|receiver| receiver.index == param.index)
    }

    /// The declared parameters of a function: every row except the receiver.
    /// These line up with a trait method's parameter rows and with the
    /// arguments a caller passes after the receiver.
    fn declared_params<'f>(
        &self,
        function: &'f MirFunction,
    ) -> &'f [jet_foundation::MIR::MirParam] {
        match self.receiver_param(function) {
            Some(_) => &function.params[1..],
            None => &function.params,
        }
    }

    fn parameter_borrow_mask(&self, params: &[jet_foundation::MIR::MirParam]) -> Vec<bool> {
        params
            .iter()
            .map(|param| self.parameter_borrowed(param))
            .collect()
    }

    fn parameter_type(&self, param: &jet_foundation::MIR::MirParam) -> String {
        let ty = self.rust_parameter_type(&param.ty);
        match param.access {
            MirAccess::Write => format!("&mut {ty}"),
            MirAccess::Read if self.parameter_borrowed(param) => format!("&{ty}"),
            MirAccess::Read | MirAccess::Move => ty,
        }
    }
    fn capture_param_name(&self, slot: usize) -> String {
        format!("__jet_capture_{slot}")
    }

    fn capture_parameter_type(&self, function: &MirFunction, capture: &MirCaptureParam) -> String {
        let ty = self.rust_parameter_type(&capture.ty);
        if self.capture_move_required(function, capture.slot) {
            return ty;
        }
        match capture.access {
            MirAccess::Read => format!("&{ty}"),
            MirAccess::Write => format!("&mut {ty}"),
            MirAccess::Move => ty,
        }
    }

    fn emit_program_metadata(&self, out: &mut String) {
        let identity = self.artifact_identity.canonical_json();
        let _ = writeln!(out, "// jet-mir-identity: {identity}");
        let _ = writeln!(out, "// MIR schema: {}", self.program.schema_version);
        let _ = writeln!(out, "// MIR package: {}", self.program.package_identity);
        let _ = writeln!(
            out,
            "// MIR target: {} pointer={} align={}",
            self.config.target.triple,
            self.config.target.pointer_size,
            self.config.target.pointer_alignment
        );
    }

    fn visibility(&self, visibility: MirVisibility) -> &'static str {
        match visibility {
            MirVisibility::Private => "",
            MirVisibility::Package => "pub(crate) ",
            MirVisibility::Public => "pub ",
        }
    }
    fn generic_params_from(&self, params: &[jet_foundation::MIR::MirGenericParam]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let params = params
            .iter()
            .map(|param| {
                let bounds = param
                    .bounds
                    .iter()
                    .map(|bound| self.trait_nominal_name(bound))
                    .collect::<Vec<_>>();
                if bounds.is_empty() {
                    mangle(&param.name)
                } else {
                    format!("{}: {}", mangle(&param.name), bounds.join(" + "))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("<{params}>")
    }

    fn module_row(&self, id: MirModuleId) -> &MirModule {
        self.program
            .modules
            .iter()
            .find(|module| module.id == id)
            .unwrap_or_else(|| panic!("MIR module ID {:?} has no row", id))
    }
    fn coverage_function_line(&self, function: &MirFunction) -> usize {
        let module = self.module_row(function.module_id);
        let source = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == module.source_file)
            .unwrap_or_else(|| panic!("MIR source file {:?} has no row", module.source_file));
        jet_foundation::Diagnostics::span_line_col(&source.source, function.span.start).0
    }
    fn instruction_location(
        &self,
        function: &MirFunction,
        instruction: &MirInstruction,
    ) -> MirPanicLoc {
        let module = self.module_row(function.module_id);
        let source = self
            .program
            .source_files
            .iter()
            .find(|source| source.id == module.source_file)
            .unwrap_or_else(|| panic!("MIR source file {:?} has no row", module.source_file));
        let line = instruction
            .source_line
            .map(|line| line as usize)
            .unwrap_or_else(|| {
                jet_foundation::Diagnostics::span_line_col(&source.source, instruction.span.start).0
            });
        let line = u32::try_from(line)
            .unwrap_or_else(|_| panic!("MIR instruction source line {line} exceeds u32"));
        MirPanicLoc {
            file: module.source_file,
            line,
            column: 0,
        }
    }

    fn trait_name(&self, id: MirTraitId) -> String {
        self.traits
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("MIR trait ID {:?} has no declaration row", id))
    }

    fn trait_nominal_name(&self, reference: &jet_foundation::MIR::MirTraitRef) -> String {
        let name = self
            .traits
            .get(&reference.id)
            .cloned()
            .or_else(|| self.traits_by_name.get(&reference.name).cloned())
            .unwrap_or_else(|| {
                panic!(
                    "MIR trait reference {:?} has no declaration row",
                    reference.id
                )
            });
        name
    }

    fn emit_modules_and_imports(&self, out: &mut String) {
        for module_id in &self.artifact.modules {
            let module = self.module_row(*module_id);
            let _ = writeln!(
                out,
                "// jet-mir-module: id={} key={} name={} path={}",
                module.id.0, module.key, module.name, module.path
            );
            for import_id in &module.imports {
                let import = self
                    .program
                    .imports
                    .iter()
                    .find(|import| import.id == *import_id)
                    .unwrap_or_else(|| panic!("MIR import ID {:?} has no row", import_id));
                if import.module != module.id {
                    panic!("MIR import {:?} belongs to another module", import.id);
                }
                let visibility = self.visibility(import.visibility);
                match &import.kind {
                    MirImportKind::File { path } => {
                        let _ = writeln!(
                            out,
                            "// jet-mir-import: {visibility}file {} as {}",
                            path,
                            mangle(&import.alias)
                        );
                    }
                    MirImportKind::Module { path } => {
                        let _ = writeln!(
                            out,
                            "// jet-mir-import: {visibility}module {} as {}",
                            path,
                            mangle(&import.alias)
                        );
                    }
                    MirImportKind::Unqualified { module, items } => {
                        if !self.module_selected(*module) {
                            panic!(
                                "MIR selected module {:?} imports unselected module {:?}",
                                module_id, module
                            );
                        }
                        let items = items
                            .iter()
                            .map(|item| {
                                format!("{}:{}", item.original, self.item_ref_name(item.item))
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        let _ = writeln!(
                            out,
                            "// jet-mir-import: {visibility}unqualified {} as {} [{}]",
                            self.module_row(*module).key,
                            mangle(&import.alias),
                            items
                        );
                    }
                }
            }
        }
        out.push('\n');
    }

    fn item_ref_name(&self, item: MirItemRef) -> String {
        match item {
            MirItemRef::Type(id) => self.type_name(id),
            MirItemRef::Trait(id) => self.trait_name(id),
            MirItemRef::Function(id) => self.function_name(id),
            MirItemRef::Constant(id) => self
                .program
                .constants
                .iter()
                .find(|constant| constant.id == id)
                .map(|constant| mangle_path(&constant.key))
                .unwrap_or_else(|| panic!("MIR constant ID {:?} has no row", id)),
            MirItemRef::Impl(id) => self
                .program
                .impls
                .iter()
                .find(|implementation| implementation.id == id)
                .map(|implementation| mangle_path(&implementation.key))
                .unwrap_or_else(|| panic!("MIR impl ID {:?} has no row", id)),
            MirItemRef::Foreign(id) => self.foreign_name(id),
            MirItemRef::Import(id) => self
                .program
                .imports
                .iter()
                .find(|import| import.id == id)
                .map(|import| mangle(&import.alias))
                .unwrap_or_else(|| panic!("MIR import ID {:?} has no row", id)),
        }
    }
    fn model_trait_definition<'b>(
        &'b self,
        type_args: &[MirType],
    ) -> Option<(&'b MirTraitDef, &'b jet_foundation::AST::ModelOutputFact)> {
        let nominal = match type_args.first()?.kind() {
            MirTypeKind::Apply { name, .. } => name.name.as_str(),
            MirTypeKind::TraitObject(bounds) => bounds.first()?.name.as_str(),
            _ => return None,
        };
        let leaf = nominal
            .rsplit("::")
            .next()
            .unwrap_or(nominal)
            .rsplit('.')
            .next()
            .unwrap_or(nominal);
        let definition = self.program.traits.iter().find(|definition| {
            definition.name == leaf
                || definition.key == nominal
                || definition.key.ends_with(&format!("::{leaf}"))
                || definition.key.ends_with(&format!(".{leaf}"))
        })?;
        let fact = self
            .program
            .facts
            .model_outputs
            .iter()
            .find(|fact| fact.signature_name.as_deref() == Some(definition.name.as_str()))?;
        Some((definition, fact))
    }

    fn model_type_definition(&self, ty: &MirType) -> &MirTypeDef {
        if let Some(identity) = ty.identity {
            if let Some(definition) = self
                .program
                .types
                .iter()
                .find(|definition| definition.id == identity)
            {
                return definition;
            }
        }
        let name = match ty.kind() {
            MirTypeKind::Apply { name, .. } => name.name.clone(),
            _ => ty.display_name(),
        };
        self.program
            .types
            .iter()
            .find(|definition| definition.name == name || definition.key == name)
            .unwrap_or_else(|| panic!("model carrier type `{name}` has no MIR declaration"))
    }

    fn model_tensor_expr(&self, tensor: &ModelTensorFact) -> String {
        let dtype = match tensor.dtype.as_str() {
            "bool" => "Bool",
            "i8" => "I8",
            "i16" => "I16",
            "i32" => "I32",
            "i64" => "I64",
            "u8" => "U8",
            "u16" => "U16",
            "u32" => "U32",
            "u64" => "U64",
            "f16" => "F16",
            "bf16" => "BF16",
            "f32" => "F32",
            "f64" => "F64",
            other => panic!("checked model tensor has unsupported dtype `{other}`"),
        };
        let dimensions = tensor
            .shape
            .iter()
            .map(|dimension| match dimension {
                ModelDimensionFact::Static(value) => {
                    format!("jet_rt::model::TensorDimension::Static({value})")
                }
                ModelDimensionFact::Dynamic { name, min, max } => format!(
                    "jet_rt::model::TensorDimension::Dynamic {{ name: {}, min: {min}, max: {max} }}",
                    quote_rust_string(name)
                ),
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "jet_rt::model::TensorSpec {{ name: {}, dtype: jet_rt::model::TensorDType::{dtype}, shape: jet_rt::model::TensorShape {{ dimensions: vec![{dimensions}] }} }}",
            quote_rust_string(&tensor.name)
        )
    }

    fn model_descriptor_expr(&self, fact: &jet_foundation::AST::ModelOutputFact) -> String {
        let field = |name: &str| {
            fact.fields
                .get(name)
                .unwrap_or_else(|| panic!("checked model output is missing `{name}`"))
        };
        let artifact = |role: &str| {
            format!(
                "jet_rt::model::ModelArtifact {{ path: {}, sha256: {} }}",
                quote_rust_string(model_unquote(field(role)).as_str()),
                quote_rust_string(model_unquote(field(&format!("{role}_sha256"))).as_str())
            )
        };
        let optional_artifact = |role: &str| {
            fact.fields
                .get(role)
                .map(|path| {
                    format!(
                        "Some(jet_rt::model::ModelArtifact {{ path: {}, sha256: {} }})",
                        quote_rust_string(model_unquote(path).as_str()),
                        quote_rust_string(model_unquote(field(&format!("{role}_sha256"))).as_str())
                    )
                })
                .unwrap_or_else(|| "None".to_string())
        };
        let tensor_list = |name: &str| {
            model_list_items(field(name))
                .into_iter()
                .map(|raw| {
                    model_tensor_fact(&raw)
                        .unwrap_or_else(|| panic!("checked model tensor `{name}` is malformed"))
                })
                .map(|tensor| self.model_tensor_expr(&tensor))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let option_u64 = |name: &str| {
            fact.fields
                .get(name)
                .map(|value| {
                    format!(
                        "Some({})",
                        model_unquote(value)
                            .parse::<u64>()
                            .unwrap_or_else(|_| panic!(
                                "checked model limit `{name}` is not an integer"
                            ))
                    )
                })
                .unwrap_or_else(|| "None".to_string())
        };
        let custom_operators = fact
            .fields
            .get("custom_operators")
            .map(|value| model_unquote(value))
            .unwrap_or_else(|| "false".to_string());
        let signature_name = fact
            .signature_name
            .as_deref()
            .map(|name| format!("Some({})", quote_rust_string(name)))
            .unwrap_or_else(|| "None".to_string());
        format!(
            "jet_rt::model::ModelDescriptor {{ package: {}, output: {}, signature_name: {signature_name}, package_version: {}, license: {}, graph: {}, weights: {}, tokenizer: {}, adapter: {}, preprocessing: {}, pooling: {}, normalization: {}, output_meaning: {}, metric: {}, contract: jet_rt::model::ModelContract {{ inputs: vec![{}], outputs: vec![{}], provider: {}, custom_operators: {custom_operators}, max_context: {}, max_batch: {}, max_buffer_bytes: {} }} }}",
            quote_rust_string(&fact.package),
            quote_rust_string(&fact.output),
            quote_rust_string(&fact.package_version),
            quote_rust_string(&fact.license),
            artifact("graph"),
            artifact("weights"),
            artifact("tokenizer"),
            optional_artifact("adapter"),
            quote_rust_string(model_unquote(field("preprocessing")).as_str()),
            quote_rust_string(model_unquote(field("pooling")).as_str()),
            quote_rust_string(model_unquote(field("normalization")).as_str()),
            quote_rust_string(model_unquote(field("output_meaning")).as_str()),
            quote_rust_string(model_unquote(field("metric")).as_str()),
            tensor_list("inputs"),
            tensor_list("outputs"),
            quote_rust_string(model_unquote(field("provider")).as_str()),
            option_u64("max_context"),
            option_u64("max_batch"),
            option_u64("max_buffer_bytes"),
        )
    }
    fn model_web_files_expr(&self, fact: &jet_foundation::AST::ModelOutputFact) -> String {
        let mut entries = Vec::new();
        for role in ["graph", "weights", "tokenizer", "adapter"] {
            let Some(raw_path) = fact.fields.get(role) else {
                continue;
            };
            let relative = model_unquote(raw_path);
            let path = fact.package_root.join(&relative);
            entries.push(format!(
                "({}, include_bytes!({}).to_vec())",
                quote_rust_string(&relative),
                quote_rust_string(&path.display().to_string()),
            ));
        }
        format!("std::collections::BTreeMap::from([{}])", entries.join(", "))
    }

    fn model_adapter_types<'b>(
        &'b self,
        definition: &'b MirTraitDef,
    ) -> (&'b MirTraitMethod, &'b MirTypeDef, &'b MirTypeDef) {
        let method = definition
            .methods
            .iter()
            .find(|method| method.name == "embed")
            .unwrap_or_else(|| panic!("model trait `{}` has no embed method", definition.name));
        let MirTypeKind::Result { ok, .. } = method.return_type.kind() else {
            panic!(
                "model trait `{}` embed method is not fallible",
                definition.name
            );
        };
        let batch = self.model_type_definition(ok);
        let space_type = match &batch.kind {
            MirTypeDefKind::Struct { fields, .. } => fields
                .iter()
                .find(|field| field.name == "space")
                .map(|field| &field.ty)
                .unwrap_or_else(|| panic!("model batch `{}` has no space field", batch.name)),
            _ => panic!("model batch `{}` is not a struct", batch.name),
        };
        let space = self.model_type_definition(space_type);
        (method, batch, space)
    }

    fn model_space_expr(&self, space: &MirTypeDef, batch: &str) -> String {
        let MirTypeDefKind::Struct { fields, .. } = &space.kind else {
            panic!("model space `{}` is not a struct", space.name);
        };
        let values = fields
            .iter()
            .map(|field| {
                let value = match field.name.as_str() {
                    "model_digest" => format!("{batch}.space().model_digest().to_string()"),
                    "dimension" => format!("{batch}.space().dimension() as _"),
                    "metric" => format!("{batch}.space().metric().to_string()"),
                    "normalization" => format!("{batch}.space().normalization().to_string()"),
                    other => panic!(
                        "model space `{}` has unsupported field `{other}`",
                        space.name
                    ),
                };
                format!("{}: {value}", mangle(&field.name))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} {{ {values} }}", self.type_name(space.id))
    }

    fn model_batch_expr(&self, batch: &MirTypeDef, space: &MirTypeDef) -> String {
        let MirTypeDefKind::Struct { fields, .. } = &batch.kind else {
            panic!("model batch `{}` is not a struct", batch.name);
        };
        let values = fields
            .iter()
            .map(|field| {
                let value = match field.name.as_str() {
                    "values" => "jet_batch.values().iter().map(|row| row.iter().map(|value| *value as _).collect()).collect()".to_string(),
                    "space" => self.model_space_expr(space, "jet_batch"),
                    other => panic!("model batch `{}` has unsupported field `{other}`", batch.name),
                };
                format!("{}: {value}", mangle(&field.name))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} {{ {values} }}", self.type_name(batch.id))
    }

    fn model_adapter_symbol(&self, definition: &MirTraitDef) -> String {
        format!("__jet_model_open_{}", mangle_path(&definition.key))
    }

    fn emit_model_web_bridge(
        &self,
        out: &mut String,
        definition: &MirTraitDef,
        fact: &jet_foundation::AST::ModelOutputFact,
    ) {
        if self.config.target_kind != MirRustTarget::WebWasm {
            return;
        }
        let prefix = mangle_path(&definition.key);
        let module = format!("__jet_model_web_{prefix}");
        let descriptor = self.model_descriptor_expr(fact);
        let web_files = self.model_web_files_expr(fact);
        let output = quote_rust_string(&fact.output);
        let mut add = |line: &str| {
            out.push_str(line);
            out.push('\n');
        };
        add("#[cfg(target_arch = \"wasm32\")]");
        add(&format!("mod {module} {{\n"));
        add("    use super::*;");
        add("    use std::cell::{Cell, RefCell};");
        add("    use std::collections::BTreeMap;");
        add("    use std::future::Future;");
        add("    use std::pin::Pin;");
        add("    use std::sync::Arc;");
        add("    use std::task::{Context, Poll, Wake, Waker};");
        add("");
        add("    type OpenFuture = Pin<Box<dyn Future<Output = Result<jet_rt::model::provider::OnnxRuntimeSession, String>>>>;");
        add("    type EmbedFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, String>>>>;");
        add("    enum FutureState { Open(Option<OpenFuture>), Embed(Option<EmbedFuture>) }");
        add("    struct Job { state: FutureState, status: u32, result: Vec<u8>, cancellation: jet_rt::model::provider::CancellationToken }");
        add("");
        add("    thread_local! {");
        add("        static NEXT_ID: Cell<u32> = const { Cell::new(1) };");
        add("        static INPUTS: RefCell<BTreeMap<u32, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };");
        add("        static SESSIONS: RefCell<BTreeMap<u32, jet_rt::model::provider::OnnxRuntimeSession>> = const { RefCell::new(BTreeMap::new()) };");
        add("        static JOBS: RefCell<BTreeMap<u32, Job>> = const { RefCell::new(BTreeMap::new()) };");
        add("    }");
        add("");
        add("    fn next_id() -> u32 {");
        add("        NEXT_ID.with(|next| {");
        add("            let id = next.get();");
        add("            if id == 0 || id == u32::MAX { return 0; }");
        add("            next.set(id + 1);");
        add("            id");
        add("        })");
        add("    }");
        add("");
        add("    fn task_token() -> jet_rt::model::provider::CancellationToken {");
        add("        let control = jet_scheduler_current_task_control().unwrap_or_else(jet_scheduler_root_task_control);");
        add("        jet_rt::model::provider::CancellationToken::from_cancel_flag(control.cancelled.clone())");
        add("    }");
        add("");
        add("    fn insert_error(handle: u32, open: bool, message: String) {");
        add("        JOBS.with(|jobs| {");
        add("            jobs.borrow_mut().insert(handle, Job {");
        add("                state: if open { FutureState::Open(None) } else { FutureState::Embed(None) },");
        add("                status: 2,");
        add("                result: message.into_bytes(),");
        add("                cancellation: task_token(),");
        add("            });");
        add("        });");
        add("    }");
        add("");
        add("    fn read_input(pointer: u32, length: u32) -> Result<Vec<u8>, String> {");
        add("        INPUTS.with(|inputs| {");
        add("            let inputs = inputs.borrow();");
        add("            let bytes = inputs.get(&pointer).ok_or_else(|| \"model Web input handle is stale\".to_string())?;");
        add("            if bytes.len() != usize::try_from(length).map_err(|_| \"model Web input length is invalid\".to_string())? {");
        add("                return Err(\"model Web input length differs from its allocation\".to_string());");
        add("            }");
        add("            Ok(bytes.clone())");
        add("        })");
        add("    }");
        add("");
        add("    fn decode_documents(bytes: &[u8]) -> Result<Vec<String>, String> {");
        add("        let mut cursor = 0usize;");
        add("        let take_u32 = |bytes: &[u8], cursor: &mut usize| -> Result<u32, String> {");
        add("            let end = cursor.checked_add(4).ok_or_else(|| \"model Web document packet overflow\".to_string())?;");
        add("            let value = bytes.get(*cursor..end).ok_or_else(|| \"truncated model Web document packet\".to_string())?;");
        add("            *cursor = end;");
        add("            Ok(u32::from_le_bytes(value.try_into().map_err(|_| \"invalid model Web document integer\".to_string())?))");
        add("        };");
        add("        let count = usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| \"model Web document count is invalid\".to_string())?;");
        add("        let mut documents = Vec::with_capacity(count);");
        add("        for _ in 0..count {");
        add("            let length = usize::try_from(take_u32(bytes, &mut cursor)?).map_err(|_| \"model Web document length is invalid\".to_string())?;");
        add("            let end = cursor.checked_add(length).ok_or_else(|| \"model Web document packet overflow\".to_string())?;");
        add("            let text = String::from_utf8(bytes.get(cursor..end).ok_or_else(|| \"truncated model Web document\".to_string())?.to_vec()).map_err(|_| \"model Web document is not UTF-8\".to_string())?;");
        add("            cursor = end;");
        add("            documents.push(text);");
        add("        }");
        add("        if cursor != bytes.len() { return Err(\"trailing model Web document packet bytes\".to_string()); }");
        add("        Ok(documents)");
        add("    }");
        add("");
        add("    fn encode_text(output: &mut Vec<u8>, value: &str) {");
        add("        output.extend_from_slice(&(value.len() as u32).to_le_bytes());");
        add("        output.extend_from_slice(value.as_bytes());");
        add("    }");
        add("");
        add("    fn encode_batch(batch: jet_rt::model::EmbeddingBatch) -> Vec<u8> {");
        add("        let mut output = Vec::new();");
        add("        output.extend_from_slice(&(batch.values().len() as u32).to_le_bytes());");
        add("        for row in batch.values() {");
        add("            output.extend_from_slice(&(row.len() as u32).to_le_bytes());");
        add("            for value in row { output.extend_from_slice(&value.to_le_bytes()); }");
        add("        }");
        add("        let space = batch.space();");
        add("        encode_text(&mut output, space.model_digest());");
        add("        output.extend_from_slice(&space.dimension().to_le_bytes());");
        add("        encode_text(&mut output, space.metric());");
        add("        encode_text(&mut output, space.normalization());");
        add("        output");
        add("    }");
        add("");
        add("    struct NoopWaker;");
        add("    impl Wake for NoopWaker { fn wake(self: Arc<Self>) {} }");
        add("");
        add("    fn poll_open(handle: u32) -> u32 {");
        add("        JOBS.with(|jobs| {");
        add("            let mut jobs = jobs.borrow_mut();");
        add("            let Some(job) = jobs.get_mut(&handle) else { return 2; };");
        add("            let FutureState::Open(future_slot) = &mut job.state else { return 2; };");
        add("            let Some(mut future) = future_slot.take() else { return job.status; };");
        add("            let waker = Waker::from(Arc::new(NoopWaker));");
        add("            let mut context = Context::from_waker(&waker);");
        add("            match future.as_mut().poll(&mut context) {");
        add("                Poll::Pending => { *future_slot = Some(future); 0 }");
        add("                Poll::Ready(Ok(session)) => {");
        add("                    let session_handle = next_id();");
        add("                    if session_handle == 0 { job.status = 2; job.result = b\"model Web session handle space exhausted\".to_vec(); return 2; }");
        add("                    SESSIONS.with(|sessions| { sessions.borrow_mut().insert(session_handle, session); });");
        add("                    job.result = session_handle.to_le_bytes().to_vec();");
        add("                    job.status = 1;");
        add("                    1");
        add("                }");
        add("                Poll::Ready(Err(error)) => { job.result = error.into_bytes(); job.status = 2; 2 }");
        add("            }");
        add("        })");
        add("    }");
        add("");
        add("    fn poll_embed(handle: u32) -> u32 {");
        add("        JOBS.with(|jobs| {");
        add("            let mut jobs = jobs.borrow_mut();");
        add("            let Some(job) = jobs.get_mut(&handle) else { return 2; };");
        add("            let FutureState::Embed(future_slot) = &mut job.state else { return 2; };");
        add("            let Some(mut future) = future_slot.take() else { return job.status; };");
        add("            let waker = Waker::from(Arc::new(NoopWaker));");
        add("            let mut context = Context::from_waker(&waker);");
        add("            match future.as_mut().poll(&mut context) {");
        add("                Poll::Pending => { *future_slot = Some(future); 0 }");
        add(
            "                Poll::Ready(Ok(result)) => { job.result = result; job.status = 1; 1 }",
        );
        add("                Poll::Ready(Err(error)) => { job.result = error.into_bytes(); job.status = 2; 2 }");
        add("            }");
        add("        })");
        add("    }");
        add("");
        add("    fn result_ptr(handle: u32, open: bool) -> u32 {");
        add("        JOBS.with(|jobs| {");
        add("            let jobs = jobs.borrow();");
        add("            let Some(job) = jobs.get(&handle) else { return 0; };");
        add("            if job.status == 0 || matches!((&job.state, open), (FutureState::Open(_), false) | (FutureState::Embed(_), true)) { return 0; }");
        add("            job.result.as_ptr() as usize as u32");
        add("        })");
        add("    }");
        add("    fn result_len(handle: u32, open: bool) -> u32 {");
        add("        JOBS.with(|jobs| {");
        add("            let jobs = jobs.borrow();");
        add("            let Some(job) = jobs.get(&handle) else { return 0; };");
        add("            if job.status == 0 || matches!((&job.state, open), (FutureState::Open(_), false) | (FutureState::Embed(_), true)) { return 0; }");
        add("            job.result.len().try_into().unwrap_or(0)");
        add("        })");
        add("    }");
        add("");
        add("    fn result_free(handle: u32, open: bool) {");
        add("        JOBS.with(|jobs| {");
        add("            let mut jobs = jobs.borrow_mut();");
        add("            if jobs.get(&handle).is_some_and(|job| matches!((&job.state, open), (FutureState::Open(_), true) | (FutureState::Embed(_), false))) { jobs.remove(&handle); }");
        add("        });");
        add("    }");
        add("");
        add("    fn cancel(handle: u32, open: bool) {");
        add("        JOBS.with(|jobs| {");
        add("            let mut jobs = jobs.borrow_mut();");
        add("            let matches_operation = jobs.get(&handle).is_some_and(|job| matches!((&job.state, open), (FutureState::Open(_), true) | (FutureState::Embed(_), false)));");
        add("            if !matches_operation { return; }");
        add("            let job = jobs.remove(&handle).expect(\"model Web job disappeared\");");
        add("            job.cancellation.cancel();");
        add("        });");
        add("    }");
        add("");
        out.push_str(&format!(
            "    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_input_alloc_{prefix}(length: u32) -> u32 {{\n        let length = match usize::try_from(length) {{ Ok(value) => value, Err(_) => return 0 }};\n        let mut bytes = Vec::new();\n        if bytes.try_reserve_exact(length.max(1)).is_err() {{ return 0; }}\n        bytes.resize(length, 0);\n        let pointer = bytes.as_mut_ptr() as usize;\n        if pointer == 0 || pointer > u32::MAX as usize {{ return 0; }}\n        INPUTS.with(|inputs| inputs.borrow_mut().insert(pointer as u32, bytes));\n        pointer as u32\n    }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_input_free_{prefix}(pointer: u32) {{ INPUTS.with(|inputs| {{ inputs.borrow_mut().remove(&pointer); }}); }}\n",
        ));
        out.push_str(&format!(
            "    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_open_start_{prefix}(pointer: u32, length: u32) -> u32 {{\n",
            prefix = prefix,
        ));
        out.push_str(
            r#"        let handle = next_id();
        if handle == 0 { return 0; }
        let bytes = match read_input(pointer, length) {
            Ok(bytes) => bytes,
            Err(error) => {
                INPUTS.with(|inputs| { inputs.borrow_mut().remove(&pointer); });
                insert_error(handle, true, error);
                return handle;
            }
        };
        INPUTS.with(|inputs| { inputs.borrow_mut().remove(&pointer); });
        let output = match String::from_utf8(bytes) {
            Ok(value) => value,
            Err(_) => {
                insert_error(handle, true, "model Web output name is not UTF-8".to_string());
                return handle;
            }
        };
"#,
        );
        out.push_str(&format!(
            "        if output != {output} {{ insert_error(handle, true, format!(\"model output {{output:?}} is not declared by this adapter\")); return handle; }}\n",
            output = output,
        ));
        out.push_str(&format!(
            r#"        let __jet_descriptor = {descriptor};
        let __jet_package = match jet_rt::model::ModelPackage::from_descriptor(__jet_descriptor) {{
            Ok(package) => package,
            Err(error) => {{ insert_error(handle, true, error.to_string()); return handle; }}
        }};
        let __jet_files = {web_files};
        let __jet_graph = match __jet_package.artifacts.first() {{
            Some(artifact) => match __jet_files.get(&artifact.path) {{
                Some(bytes) => bytes.clone(),
                None => {{ insert_error(handle, true, "declared model graph bytes are absent".to_string()); return handle; }}
            }},
            None => {{ insert_error(handle, true, "model package has no graph artifact".to_string()); return handle; }}
        }};
        let __jet_policy = match jet_rt::model::provider::OnnxRuntimePolicy::cpu_for_graph(&__jet_graph) {{
            Ok(policy) => policy,
            Err(error) => {{ insert_error(handle, true, error.to_string()); return handle; }}
        }};
"#,
            descriptor = descriptor,
            web_files = web_files,
        ));
        out.push_str(
            r#"        let __jet_provider = match jet_rt::model::provider::WebOnnxProvider::browser(__jet_policy) {
            Ok(provider) => jet_rt::model::provider::OnnxRuntimeProvider::Web(provider),
            Err(error) => {
                insert_error(handle, true, error.to_string());
                return handle;
            }
        };
        let __jet_control = jet_scheduler_current_task_control().unwrap_or_else(jet_scheduler_root_task_control);
        let __jet_cancel = jet_rt::model::provider::CancellationToken::from_cancel_flag(__jet_control.cancelled.clone());
        let __jet_callback_token = __jet_cancel.clone();
        let __jet_cancel_guard = __jet_control.register_cancel_callback(std::sync::Arc::new(move || __jet_callback_token.cancel()));
        let __jet_job_cancel = __jet_cancel.clone();
        let future: OpenFuture = Box::pin(async move {
            let _jet_cancel_guard = __jet_cancel_guard;
            let session = jet_rt::model::ModelPackage::open_with(&__jet_package, jet_rt::model::ModelSource::Bytes(&__jet_files), &__jet_provider, &__jet_cancel).await.map_err(|error| error.to_string())?;
            Ok(session)
        });
        JOBS.with(|jobs| jobs.borrow_mut().insert(handle, Job { state: FutureState::Open(Some(future)), status: 0, result: Vec::new(), cancellation: __jet_job_cancel }));
        handle
    }
"#,
        );
        out.push_str(&format!(
            "    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_open_poll_{prefix}(handle: u32) -> u32 {{ poll_open(handle) }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_open_result_ptr_{prefix}(handle: u32) -> u32 {{ result_ptr(handle, true) }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_open_result_len_{prefix}(handle: u32) -> u32 {{ result_len(handle, true) }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_open_result_free_{prefix}(handle: u32) {{ result_free(handle, true); }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_open_cancel_{prefix}(handle: u32) {{ cancel(handle, true); }}\n",
        ));
        out.push_str(&format!(
            "    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_embed_start_{prefix}(session: u32, pointer: u32, length: u32) -> u32 {{\n        let handle = next_id();\n        if handle == 0 {{ return 0; }}\n        let bytes = match read_input(pointer, length) {{ Ok(bytes) => bytes, Err(error) => {{ INPUTS.with(|inputs| inputs.borrow_mut().remove(&pointer)); insert_error(handle, false, error); return handle; }} }};\n        INPUTS.with(|inputs| inputs.borrow_mut().remove(&pointer));\n        let documents = match decode_documents(&bytes) {{ Ok(documents) => documents, Err(error) => {{ insert_error(handle, false, error); return handle; }} }};\n        let session = match SESSIONS.with(|sessions| sessions.borrow_mut().remove(&session)) {{ Some(session) => session, None => {{ insert_error(handle, false, \"model Web session handle is stale\".to_string()); return handle; }} }};\n        let __jet_control = jet_scheduler_current_task_control().unwrap_or_else(jet_scheduler_root_task_control);\n        let __jet_cancel = jet_rt::model::provider::CancellationToken::from_cancel_flag(__jet_control.cancelled.clone());\n        let __jet_callback_token = __jet_cancel.clone();\n        let __jet_cancel_guard = __jet_control.register_cancel_callback(std::sync::Arc::new(move || __jet_callback_token.cancel()));\n        let __jet_job_cancel = __jet_cancel.clone();\n        let future: EmbedFuture = Box::pin(async move {{\n            let _jet_cancel_guard = __jet_cancel_guard;\n            let mut session = session;\n            let batch = session.embed_documents(&documents, &__jet_cancel).await.map_err(|error| error.to_string())?;\n            Ok(encode_batch(batch))\n        }});\n        JOBS.with(|jobs| jobs.borrow_mut().insert(handle, Job {{ state: FutureState::Embed(Some(future)), status: 0, result: Vec::new(), cancellation: __jet_job_cancel }}));\n        handle\n    }}\n",
        ));
        out.push_str(&format!(
            "    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_embed_poll_{prefix}(handle: u32) -> u32 {{ poll_embed(handle) }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_embed_result_ptr_{prefix}(handle: u32) -> u32 {{ result_ptr(handle, false) }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_embed_result_len_{prefix}(handle: u32) -> u32 {{ result_len(handle, false) }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_embed_result_free_{prefix}(handle: u32) {{ result_free(handle, false); }}\n    #[no_mangle]\n    pub extern \"C\" fn __jet_model_web_embed_cancel_{prefix}(handle: u32) {{ cancel(handle, false); }}\n",
        ));
        out.push_str("}\n\n");
    }

    fn emit_model_adapters(&self, out: &mut String) {
        for definition in &self.program.traits {
            let Some(fact) = self
                .program
                .facts
                .model_outputs
                .iter()
                .find(|fact| fact.signature_name.as_deref() == Some(definition.name.as_str()))
            else {
                continue;
            };
            if !self.module_selected(definition.module) {
                continue;
            }
            let (method, batch, space) = self.model_adapter_types(definition);
            let trait_name = self.trait_name(definition.id);
            let adapter_name = format!("__JetModelAdapter_{}", mangle_path(&definition.key));
            let return_type = self.rust_type(&method.return_type);
            let root = &self.config.root_prefix;
            let descriptor = self.model_descriptor_expr(fact);
            let output = quote_rust_string(&fact.output);
            let web_files = self.model_web_files_expr(fact);
            let root_path = quote_rust_string(&fact.package_root.display().to_string());
            let batch_expr = self.model_batch_expr(batch, space);
            let _ = writeln!(
                out,
                "struct {adapter_name} {{ session: jet_rt::model::provider::OnnxRuntimeSession }}\n\
                 impl {trait_name} for {adapter_name} {{\n\
                     fn {}(self, documents: Vec<String>) -> {return_type} {{\n\
                         let mut __jet_session = self.session;\n\
                         let __jet_control = jet_scheduler_current_task_control().unwrap_or_else(jet_scheduler_root_task_control);\n\
                         let __jet_cancel = jet_rt::model::provider::CancellationToken::from_cancel_flag(__jet_control.cancelled.clone());\n\
                         let __jet_callback_token = __jet_cancel.clone();\n\
                         let __jet_cancel_guard = __jet_control.register_cancel_callback(std::sync::Arc::new(move || __jet_callback_token.cancel()));\n\
                         let jet_batch = match jet_rt::model::provider::run_ready(__jet_session.embed_documents(&documents, &__jet_cancel)) {{\n\
                             Ok(value) => value,\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }};\n\
                         drop(__jet_cancel_guard);\n\
                         Ok({batch_expr})\n\
                     }}\n\
                 }}\n\
                 fn {}(output: &str) -> {root}JetOutcome<Box<dyn {trait_name}>, {root}JetErr> {{\n\
                     if output != {output} {{\n\
                         return Err({root}jet_err_from_message(format!(\"model output {{output:?}} is not declared by this adapter\")));\n\
                     }}\n\
                     let __jet_root = std::path::PathBuf::from({root_path});\n\
                     let __jet_descriptor = {descriptor};\n\
                     let __jet_package = match jet_rt::model::ModelPackage::from_descriptor(__jet_descriptor) {{\n\
                         Ok(package) => package,\n\
                         Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                     }};\n\
                     let __jet_graph = match __jet_package.artifacts.first() {{\n\
                         Some(artifact) => match __jet_package.read_artifact(&__jet_root, artifact) {{\n\
                             Ok(bytes) => bytes,\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }},\n\
                         None => return Err({root}jet_err_from_message(\"model package has no graph artifact\".to_string())),\n\
                     }};\n\
                     let __jet_policy = match jet_rt::model::provider::OnnxRuntimePolicy::cpu_for_graph(&__jet_graph) {{\n\
                         Ok(policy) => policy,\n\
                         Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                     }};\n\
                     let __jet_control = jet_scheduler_current_task_control().unwrap_or_else(jet_scheduler_root_task_control);\n\
                     let __jet_cancel = jet_rt::model::provider::CancellationToken::from_cancel_flag(__jet_control.cancelled.clone());\n\
                     let __jet_callback_token = __jet_cancel.clone();\n\
                     let __jet_cancel_guard = __jet_control.register_cancel_callback(std::sync::Arc::new(move || __jet_callback_token.cancel()));\n\
                     #[cfg(not(target_arch = \"wasm32\"))]\n\
                     let __jet_session = {{\n\
                         let __jet_runtime = match std::env::var_os(\"JET_ONNX_RUNTIME_LIBRARY\") {{\n\
                             Some(path) => path,\n\
                             None => return Err({root}jet_err_from_message(\"JET_ONNX_RUNTIME_LIBRARY is required for model execution\".to_string())),\n\
                         }};\n\
                         let __jet_pin = match jet_rt::model::provider::RuntimePin::official_linux_x64(__jet_runtime) {{\n\
                             Ok(pin) => pin,\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }};\n\
                         let __jet_provider = match jet_rt::model::provider::OnnxRuntimeProvider::native(__jet_pin, __jet_policy) {{\n\
                             Ok(provider) => provider,\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }};\n\
                         match jet_rt::model::provider::run_ready(__jet_package.open_with(jet_rt::model::ModelSource::Directory(&__jet_root), &__jet_provider, &__jet_cancel)) {{\n\
                             Ok(session) => session,\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }}\n\
                     }};\n\
                     #[cfg(target_arch = \"wasm32\")]\n\
                     let __jet_files = {web_files};\n\
                     #[cfg(target_arch = \"wasm32\")]\n\
                     let __jet_session = {{\n\
                         let __jet_provider = match jet_rt::model::provider::WebOnnxProvider::browser(__jet_policy) {{\n\
                             Ok(provider) => jet_rt::model::provider::OnnxRuntimeProvider::Web(provider),\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }};\n\
                         match jet_rt::model::provider::run_ready(__jet_package.open_with(jet_rt::model::ModelSource::Bytes(&__jet_files), &__jet_provider, &__jet_cancel)) {{\n\
                             Ok(session) => session,\n\
                             Err(error) => return Err({root}jet_err_from_message(error.to_string())),\n\
                         }}\n\
                     }};\n\
                     Ok(Box::new({adapter_name} {{ session: __jet_session }}) as Box<dyn {trait_name}>)\n\
                 }}\n",
                mangle(&method.name),
                self.model_adapter_symbol(definition),
            );
            self.emit_model_web_bridge(out, definition, fact);
        }
    }
    fn emit_trait_def(
        &self,
        definition: &MirTraitDef,
        emitted_methods: &mut BTreeSet<MirFunctionId>,
        out: &mut String,
    ) {
        let visibility = self.visibility(definition.visibility);
        let _ = writeln!(
            out,
            "{visibility}trait {} {{",
            self.trait_name(definition.id)
        );
        for associated in &definition.associated_types {
            let _ = writeln!(out, "    type {};", mangle(&associated.name));
        }
        for method in &definition.methods {
            self.emit_trait_method(definition, method, emitted_methods, out);
        }
        let _ = writeln!(out, "}}\n");
    }

    fn emit_trait_method(
        &self,
        definition: &MirTraitDef,
        method: &MirTraitMethod,
        emitted_methods: &mut BTreeSet<MirFunctionId>,
        out: &mut String,
    ) {
        let previous_lifetime = self.history_callback_lifetime.replace("'__jet_callback");
        let mut params =
            Vec::with_capacity(method.params.len() + usize::from(method.self_access.is_some()));
        if let Some(access) = method.self_access {
            params.push(match access {
                MirAccess::Read => "&self".to_string(),
                MirAccess::Write => "&mut self".to_string(),
                MirAccess::Move => "self".to_string(),
            });
        }
        params.extend(
            method
                .params
                .iter()
                .map(|param| format!("{}: {}", mangle(&param.name), self.parameter_type(param))),
        );
        let params = params.join(", ");
        let ret = self.rust_type(&method.return_type);
        self.history_callback_lifetime.set(previous_lifetime);
        let generics = if params.contains("'__jet_callback") || ret.contains("'__jet_callback") {
            "<'__jet_callback>"
        } else {
            ""
        };
        let Some(default_id) = method.default else {
            let _ = writeln!(
                out,
                "    fn {}{generics}({params}) -> {ret};",
                mangle(&method.name)
            );
            return;
        };
        let function = self.function_row(default_id);
        if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
            panic!(
                "MIR trait default {:?} is not selected for the emitted artifact",
                default_id
            );
        }
        let MirFunctionForm::TraitMethod {
            owner,
            trait_ref,
            self_access,
            serde,
        } = &function.form
        else {
            panic!(
                "MIR trait default {:?} has a non-trait function form",
                default_id
            );
        };
        if trait_ref.id != definition.id || trait_ref.name != definition.name {
            panic!(
                "MIR trait default {:?} references {:?}, expected {:?}",
                default_id, trait_ref, definition.id
            );
        }
        if *self_access != method.self_access || serde.is_some() {
            panic!(
                "MIR trait default {:?} receiver/codec contract does not match {}",
                default_id, method.name
            );
        }
        if function.capture_params.len() != 0
            || self.declared_params(function).len() != method.params.len()
            || self
                .declared_params(function)
                .iter()
                .zip(&method.params)
                .any(|(actual, expected)| {
                    actual.index != expected.index
                        || actual.name != expected.name
                        || actual.access != expected.access
                        || !actual
                            .ty
                            .same_checked_type(&self.normalize_trait_type(&expected.ty, owner))
                })
            || !function
                .return_type
                .same_checked_type(&self.normalize_trait_type(&method.return_type, owner))
            || function.failure != method.failure
        {
            panic!(
                "MIR trait default {:?} signature does not match {}",
                default_id, method.name
            );
        }
        self.emit_callable_named(
            function,
            out,
            Some(&function.form),
            Some(&mangle(&method.name)),
            None,
            None,
        );
        if !emitted_methods.insert(function.id) {
            panic!(
                "MIR trait default {:?} was emitted more than once",
                function.id
            );
        }
    }

    fn emit_constant_def(&self, constant: &MirConstantDef, out: &mut String) {
        let name = mangle_path(&constant.key);
        let visibility = self.visibility(constant.visibility);
        let ty = self.rust_type(&constant.ty);
        let value = self.constant_for_type(&constant.value, &constant.ty);
        // The value is already folded; materializing a heap-backed carrier is not a Rust const expression.
        let _ = writeln!(
            out,
            "#[inline]\n{visibility}fn {name}() -> {ty} {{ {value} }}\n"
        );
    }

    fn global_expression(&self, name: &str) -> String {
        let path = format!("{}{}", self.config.root_prefix, mangle_path(name));
        if self
            .program
            .constants
            .iter()
            .any(|constant| constant.key == name || constant.name == name)
        {
            format!("{path}()")
        } else {
            path
        }
    }
    fn emit_impl(
        &self,
        implementation: &MirImplDef,
        emitted_methods: &mut BTreeSet<MirFunctionId>,
        out: &mut String,
    ) {
        if !self.module_selected(implementation.module)
            || !self.impl_selected_for_target(implementation)
        {
            panic!(
                "MIR implementation {:?} is not selected for the emitted artifact",
                implementation.id
            );
        }
        if implementation.serde.is_some() && implementation.trait_ref.is_none() {
            panic!(
                "MIR implementation {:?} carries a serde codec without a trait",
                implementation.id
            );
        }
        if let Some(field) = implementation.delegation {
            if implementation.trait_ref.is_none() {
                panic!(
                    "MIR implementation {:?} delegates without a trait",
                    implementation.id
                );
            }
            let row = self
                .program
                .fields
                .iter()
                .find(|row| row.id == field)
                .unwrap_or_else(|| {
                    panic!(
                        "MIR implementation {:?} delegation field {:?} has no row",
                        implementation.id, field
                    )
                });
            if row.owner != self.type_identity(&implementation.self_type) {
                panic!(
                    "MIR implementation {:?} delegation field {:?} belongs to {:?}, expected {:?}",
                    implementation.id,
                    field,
                    row.owner,
                    self.type_identity(&implementation.self_type)
                );
            }
        }
        let impl_generic_params = self.generic_params_for_type(&implementation.self_type);
        self.push_generic_scope(impl_generic_params);
        let generics = self.generic_params_from(impl_generic_params);
        let owner = self.rust_type(&implementation.self_type);
        let owner = if !impl_generic_params.is_empty()
            && matches!(implementation.self_type.kind(), MirTypeKind::Apply { args, .. } if args.is_empty())
        {
            let arguments = impl_generic_params
                .iter()
                .map(|param| mangle(&param.name))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{owner}<{arguments}>")
        } else {
            owner
        };
        let trait_definition = implementation.trait_ref.as_ref().map(|trait_ref| {
            self.program
                .traits
                .iter()
                .find(|definition| {
                    definition.id == trait_ref.id && definition.name == trait_ref.name
                })
                .unwrap_or_else(|| {
                    panic!(
                        "MIR implementation {:?} trait reference {:?} has no declaration row",
                        implementation.id, trait_ref
                    )
                })
        });
        let operator = implementation.operator_rhs.as_ref();
        let rust_operator = operator.filter(|_| {
            implementation.trait_ref.as_ref().is_some_and(|trait_ref| {
                matches!(
                    trait_ref.name.as_str(),
                    jet_foundation::Syntax::TRAIT_ADD
                        | jet_foundation::Syntax::TRAIT_SUB
                        | jet_foundation::Syntax::TRAIT_MUL
                        | jet_foundation::Syntax::TRAIT_DIV
                )
            })
        });
        if let Some(rhs) = operator {
            let trait_ref = implementation.trait_ref.as_ref().unwrap_or_else(|| {
                panic!(
                    "MIR operator implementation {:?} has no trait reference",
                    implementation.id
                )
            });
            if !matches!(
                trait_ref.name.as_str(),
                jet_foundation::Syntax::TRAIT_ADD
                    | jet_foundation::Syntax::TRAIT_SUB
                    | jet_foundation::Syntax::TRAIT_MUL
                    | jet_foundation::Syntax::TRAIT_DIV
                    | jet_foundation::Syntax::TRAIT_EQUATABLE
                    | jet_foundation::Syntax::TRAIT_COMPARABLE
            ) {
                panic!(
                    "MIR operator implementation {:?} has non-operator trait {:?}",
                    implementation.id, trait_ref.name
                );
            }
            if rust_operator.is_some() {
                let _ = writeln!(
                    out,
                    "impl{generics} {}<{}> for {owner} {{",
                    self.trait_nominal_name(trait_ref),
                    self.rust_type(rhs)
                );
            } else {
                let _ = writeln!(
                    out,
                    "impl{generics} {} for {owner} {{",
                    self.trait_nominal_name(trait_ref)
                );
            }
        } else if let Some(trait_ref) = &implementation.trait_ref {
            let _ = writeln!(
                out,
                "impl{generics} {} for {owner} {{",
                self.trait_nominal_name(trait_ref)
            );
        } else {
            if implementation.delegation.is_some()
                || implementation.serde.is_some()
                || !implementation.associated_types.is_empty()
            {
                panic!(
                    "MIR inherent implementation {:?} carries trait-only metadata",
                    implementation.id
                );
            }
            let _ = writeln!(out, "impl{generics} {owner} {{");
        }

        if let Some(trait_definition) = trait_definition {
            for associated in &implementation.associated_types {
                if !trait_definition
                    .associated_types
                    .iter()
                    .any(|declared| declared.name == associated.name)
                {
                    panic!(
                        "MIR implementation {:?} defines unknown associated type {}",
                        implementation.id, associated.name
                    );
                }
            }
            for declared in &trait_definition.associated_types {
                if !implementation
                    .associated_types
                    .iter()
                    .any(|associated| associated.name == declared.name)
                    && !(rust_operator.is_some() && declared.name == "Output")
                {
                    panic!(
                        "MIR implementation {:?} omits associated type {}",
                        implementation.id, declared.name
                    );
                }
            }
        } else if !implementation.associated_types.is_empty() {
            panic!(
                "MIR inherent implementation {:?} has associated types",
                implementation.id
            );
        }
        for associated in &implementation.associated_types {
            let _ = writeln!(
                out,
                "    type {} = {};",
                mangle(&associated.name),
                self.rust_type(&associated.ty)
            );
        }

        let mut functions = Vec::with_capacity(implementation.methods.len());
        for function_id in &implementation.methods {
            let function = self.function_row(*function_id);
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!(
                    "MIR implementation {:?} method {:?} is not selected for the emitted artifact",
                    implementation.id, function.id
                );
            }
            functions.push(function);
        }
        let mut operator_return: Option<MirType> = None;
        for function in functions.iter().copied() {
            match (&implementation.trait_ref, &function.form) {
                (
                    None,
                    MirFunctionForm::Method {
                        owner: declared_owner,
                        ..
                    },
                ) => {
                    if !declared_owner.same_checked_type(&implementation.self_type) {
                        panic!(
                            "MIR inherent implementation {:?} method {:?} owner mismatch",
                            implementation.id, function.id
                        );
                    }
                    if !self.owner_independent_static(function) {
                        self.emit_method(function, out);
                    }
                }
                (
                    Some(trait_ref),
                    MirFunctionForm::TraitMethod {
                        owner: declared_owner,
                        trait_ref: declared_trait,
                        self_access,
                        serde,
                    },
                ) => {
                    if !declared_owner.same_checked_type(&implementation.self_type)
                        || declared_trait.id != trait_ref.id
                        || declared_trait.name != trait_ref.name
                    {
                        panic!(
                            "MIR trait implementation {:?} method {:?} declaration mismatch",
                            implementation.id, function.id
                        );
                    }
                    let codec = implementation.serde;
                    if codec != *serde {
                        panic!(
                            "MIR trait implementation {:?} method {:?} codec mismatch",
                            implementation.id, function.id
                        );
                    }
                    if let Some(codec) = codec {
                        let expected_name = match codec {
                            MirSerdeCodec::Encode => "encode",
                            MirSerdeCodec::Decode => "decode",
                        };
                        if self.function_leaf_name(function) != expected_name
                            || function.capture_params.len() != 0
                        {
                            panic!(
                                "MIR serde implementation {:?} method {:?} has invalid codec shape",
                                implementation.id, function.id
                            );
                        }
                        match codec {
                            MirSerdeCodec::Encode
                                if *self_access != Some(MirAccess::Read)
                                    || !self.declared_params(function).is_empty() =>
                            {
                                panic!(
                                    "MIR Encode implementation {:?} method {:?} must be an instance method without arguments",
                                    implementation.id, function.id
                                );
                            }
                            MirSerdeCodec::Decode
                                if self_access.is_some()
                                    || self.declared_params(function).len() != 1 =>
                            {
                                panic!(
                                    "MIR Decode implementation {:?} method {:?} must be static with one argument",
                                    implementation.id, function.id
                                );
                            }
                            MirSerdeCodec::Encode | MirSerdeCodec::Decode => {}
                        }
                    }
                    if let Some(trait_definition) = trait_definition {
                        // Compiler-owned trait rows carry identity only; sema
                        // checked their signatures against the Prelude registry.
                        if codec.is_none()
                            && !implementation.compiler_generated
                            && !crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_trait(
                                &trait_definition.name,
                            )
                        {
                            self.validate_trait_method_signature(
                                implementation,
                                trait_definition,
                                function,
                                *self_access,
                            );
                        }
                    } else if codec.is_none() {
                        panic!(
                            "MIR trait implementation {:?} method {:?} has no trait declaration",
                            implementation.id, function.id
                        );
                    }
                    let (name, return_override) = match codec {
                        Some(MirSerdeCodec::Encode) => (
                            Some("jet_encode"),
                            Some(format!("{}jet_std::DataTree", self.config.root_prefix)),
                        ),
                        Some(MirSerdeCodec::Decode) => (
                            Some("jet_decode"),
                            Some(format!(
                                "Result<Self, Vec<{}jet_std::FieldError>>",
                                self.config.root_prefix
                            )),
                        ),
                        None => (None, None),
                    };
                    self.emit_callable_named(
                        function,
                        out,
                        Some(&function.form),
                        name,
                        codec,
                        return_override,
                    );
                    if rust_operator.is_some() {
                        if let Some(previous) = &operator_return {
                            if !previous.same_checked_type(&function.return_type) {
                                panic!(
                                    "MIR operator implementation {:?} methods disagree on Output",
                                    implementation.id
                                );
                            }
                        } else {
                            operator_return = Some(function.return_type.clone());
                        }
                    }
                }
                (None, MirFunctionForm::TopLevel)
                | (None, MirFunctionForm::TraitMethod { .. })
                | (Some(_), MirFunctionForm::TopLevel)
                | (Some(_), MirFunctionForm::Method { .. }) => panic!(
                    "MIR implementation {:?} method {:?} has incompatible function form",
                    implementation.id, function.id
                ),
            }
            if !emitted_methods.insert(function.id) {
                panic!(
                    "MIR implementation method {:?} was emitted more than once",
                    function.id
                );
            }
        }
        if implementation.trait_ref.is_some() {
            if let Some(trait_definition) = trait_definition {
                for method in &trait_definition.methods {
                    if implementation.serde.is_none()
                        && (method.default.is_none() || implementation.delegation.is_some())
                        && !functions
                            .iter()
                            .any(|function| self.function_leaf_name(function) == method.name)
                    {
                        panic!(
                            "MIR implementation {:?} omits required trait method {}",
                            implementation.id, method.name
                        );
                    }
                }
            }
            if rust_operator.is_some() {
                let output = operator_return.unwrap_or_else(|| {
                    panic!(
                        "MIR operator implementation {:?} has no materialized method",
                        implementation.id
                    )
                });
                if let Some(associated) = implementation
                    .associated_types
                    .iter()
                    .find(|associated| associated.name == "Output")
                {
                    if !associated.ty.same_checked_type(&output) {
                        panic!(
                            "MIR operator implementation {:?} Output type disagrees with method return",
                            implementation.id
                        );
                    }
                } else {
                    let _ = writeln!(out, "    type Output = {};", self.rust_type(&output));
                }
            }
        }
        let _ = writeln!(out, "}}\n");
        self.pop_generic_scope();
        for function in functions {
            if self.owner_independent_static(function) {
                self.emit_function(function, out);
            }
        }
    }

    fn arithmetic_trait_method(&self, function: &MirFunction) -> bool {
        matches!(
            &function.form,
            MirFunctionForm::TraitMethod { trait_ref, .. }
                if matches!(trait_ref.name.as_str(), "Add" | "Sub" | "Mul" | "Div")
        )
    }

    fn function_leaf_name(&self, function: &MirFunction) -> String {
        if let MirFunctionForm::TraitMethod { trait_ref, .. } = &function.form {
            if matches!(
                trait_ref.name.as_str(),
                crate::Syntax::TRAIT_ADD
                    | crate::Syntax::TRAIT_SUB
                    | crate::Syntax::TRAIT_MUL
                    | crate::Syntax::TRAIT_DIV
                    | crate::Syntax::TRAIT_EQUATABLE
                    | crate::Syntax::TRAIT_COMPARABLE
            ) {
                // operator_method_identity appends the checked RHS after `<`.
                // Remove that suffix before splitting the qualified owner.
                return function
                    .name
                    .split('<')
                    .next()
                    .unwrap()
                    .rsplit("::")
                    .next()
                    .unwrap()
                    .to_string();
            }
        }
        function
            .name
            .rsplit("::")
            .next()
            .unwrap_or_else(|| panic!("MIR function {:?} has an empty name", function.id))
            .to_string()
    }

    /// Inherent methods live in `impl Type` as `__jet_<leaf>`. Compiler-owned
    /// trait methods keep the source leaf (`equal`, `compare`, `display`)
    /// because the cached runtime traits declare those names unmangled.
    fn rust_method_symbol(&self, function: &MirFunction) -> String {
        let leaf = self.function_leaf_name(function);
        match &function.form {
            MirFunctionForm::TraitMethod {
                serde: Some(MirSerdeCodec::Encode),
                ..
            } => "jet_encode".to_string(),
            MirFunctionForm::TraitMethod {
                serde: Some(MirSerdeCodec::Decode),
                ..
            } => "jet_decode".to_string(),
            MirFunctionForm::TraitMethod { trait_ref, .. }
                if crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_trait(
                    &trait_ref.name,
                ) =>
            {
                leaf
            }
            MirFunctionForm::TraitMethod { .. }
            | MirFunctionForm::Method { .. }
            | MirFunctionForm::TopLevel => mangle(&leaf),
        }
    }

    // A bare generic owner is only a namespace for a checked static method.
    // Its unused owner parameters must not become Rust inference obligations.
    fn owner_independent_static(&self, function: &MirFunction) -> bool {
        matches!(
            &function.form,
            MirFunctionForm::Method { owner, self_access: None }
                if matches!(owner.kind(), MirTypeKind::Apply { args, .. } if args.is_empty())
                    && !self.generic_params_for_type(owner).is_empty()
        )
    }

    fn normalize_trait_type(&self, ty: &MirType, owner: &MirType) -> MirType {
        if ty.nominal_name() == Some("Self") {
            return owner.clone();
        }
        let mut normalized = ty.clone();
        match &mut normalized.kind {
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Float32
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::IntN { .. }
            | MirTypeKind::Measure(_) => {}
            MirTypeKind::List(inner) | MirTypeKind::Shared(inner) | MirTypeKind::Option(inner) => {
                **inner = self.normalize_trait_type(inner, owner);
            }
            MirTypeKind::Map { key, value } => {
                **key = self.normalize_trait_type(key, owner);
                **value = self.normalize_trait_type(value, owner);
            }
            MirTypeKind::Result { ok, err } => {
                **ok = self.normalize_trait_type(ok, owner);
                **err = self.normalize_trait_type(err, owner);
            }
            MirTypeKind::Fn(signature) => {
                for parameter in &mut signature.params {
                    *parameter = self.normalize_trait_type(parameter, owner);
                }
                if let Some(ret) = &mut signature.ret {
                    **ret = self.normalize_trait_type(ret, owner);
                }
            }
            MirTypeKind::SendFn { params, ret } => {
                for parameter in params {
                    *parameter = self.normalize_trait_type(parameter, owner);
                }
                if let Some(ret) = ret {
                    **ret = self.normalize_trait_type(ret, owner);
                }
            }
            MirTypeKind::Apply { args, .. } => {
                for argument in args {
                    *argument = self.normalize_trait_type(argument, owner);
                }
            }
            MirTypeKind::Tuple(fields) => {
                for (_, field) in fields {
                    *field = self.normalize_trait_type(field, owner);
                }
            }
            MirTypeKind::FixedList { elem, .. } => {
                **elem = self.normalize_trait_type(elem, owner);
            }
            MirTypeKind::InlineRange { base, .. } => {
                **base = self.normalize_trait_type(base, owner);
            }
            MirTypeKind::Tagged { inner, .. } => {
                **inner = self.normalize_trait_type(inner, owner);
            }
            MirTypeKind::Union(members) => {
                for member in members {
                    *member = self.normalize_trait_type(member, owner);
                }
            }
            MirTypeKind::Quantity { base, .. } => {
                **base = self.normalize_trait_type(base, owner);
            }
        }
        normalized.identity = None;
        normalized
    }

    fn validate_trait_method_signature(
        &self,
        implementation: &MirImplDef,
        definition: &MirTraitDef,
        function: &MirFunction,
        self_access: Option<MirAccess>,
    ) {
        let name = self.function_leaf_name(function);
        let method = definition
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "MIR implementation {:?} method {:?} is not declared by trait {}",
                    implementation.id, function.id, definition.name
                )
            });
        if method.self_access != self_access
            || function.capture_params.len() != 0
            || self.declared_params(function).len() != method.params.len()
            || self
                .declared_params(function)
                .iter()
                .zip(&method.params)
                .any(|(actual, expected)| {
                    actual.index != expected.index + usize::from(self_access.is_some())
                        || actual.name != expected.name
                        || actual.access != expected.access
                        || !actual.ty.same_checked_type(
                            &self.normalize_trait_type(&expected.ty, &implementation.self_type),
                        )
                })
            || !function.return_type.same_checked_type(
                &self.normalize_trait_type(&method.return_type, &implementation.self_type),
            )
            || function.failure != method.failure
        {
            panic!(
                "MIR implementation {:?} method {:?} signature disagrees with trait {}: receiver {:?}/{:?}, params {:?}/{:?}, return {:?}/{:?}, failure {:?}/{:?}",
                implementation.id, function.id, definition.name,
                self_access, method.self_access,
                self.declared_params(function), method.params,
                function.return_type, self.normalize_trait_type(&method.return_type, &implementation.self_type),
                function.failure, method.failure,
            );
        }
    }

    fn validate_derived_trait(&self, def: &MirTypeDef, trait_id: MirTraitId, trait_name: &str) {
        let definition = self
            .program
            .traits
            .iter()
            .find(|definition| definition.id == trait_id && definition.name == trait_name)
            .unwrap_or_else(|| panic!("MIR derive trait ID {:?} has no row", trait_id));
        let implementation = self
            .program
            .impls
            .iter()
            .find(|implementation| {
                implementation.trait_ref.as_ref().is_some_and(|trait_ref| {
                    trait_ref.id == trait_id && trait_ref.name == trait_name
                }) && self.type_identity(&implementation.self_type) == def.id
                    && self.module_selected(implementation.module)
                    && self.impl_selected_for_target(implementation)
            })
            .unwrap_or_else(|| {
                panic!(
                    "MIR type {:?} derives {} without a selected implementation",
                    def.id, trait_name
                )
            });
        for method in &definition.methods {
            if method.default.is_none()
                && !implementation.methods.iter().any(|function_id| {
                    self.function_leaf_name(self.function_row(*function_id)) == method.name
                })
            {
                panic!(
                    "MIR derived implementation {:?} omits required method {}",
                    implementation.id, method.name
                );
            }
        }
    }

    fn selected_trait_impl_for_type(&self, def: &MirTypeDef, trait_name: &str) -> bool {
        self.program.impls.iter().any(|implementation| {
            implementation
                .trait_ref
                .as_ref()
                .is_some_and(|trait_ref| trait_ref.name == trait_name)
                && self.type_identity(&implementation.self_type) == def.id
                && self.module_selected(implementation.module)
                && self.impl_selected_for_target(implementation)
        })
    }

    fn structural_generics(&self, def: &MirTypeDef, trait_name: &str) -> String {
        if def.generic_params.is_empty() {
            return String::new();
        }
        let required_bound = match crate::Generics::rust_trait_bound(trait_name) {
            Some(bound) => bound,
            None => trait_name,
        };
        let params = def
            .generic_params
            .iter()
            .map(|param| {
                let mut bounds = param
                    .bounds
                    .iter()
                    .map(|bound| {
                        crate::Generics::rust_trait_bound(&bound.name)
                            .map(str::to_string)
                            .unwrap_or_else(|| self.trait_nominal_name(bound))
                    })
                    .collect::<Vec<_>>();
                if !bounds.iter().any(|bound| bound == required_bound) {
                    bounds.push(required_bound.to_string());
                }
                format!("{}: {}", mangle(&param.name), bounds.join(" + "))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("<{params}>")
    }

    fn structural_field_value(
        &self,
        def: &MirTypeDef,
        edge: &str,
        receiver: &str,
        field: &MirField,
        method: &str,
    ) -> String {
        let value = format!("({receiver}.{})", self.field_name(field.id));
        if def.boxed_edges.iter().any(|candidate| candidate == edge) {
            format!("{value}.as_ref().{method}()")
        } else {
            format!("{value}.{method}()")
        }
    }

    fn derives_trait(&self, def: &MirTypeDef, trait_name: &str) -> bool {
        def.derives.iter().any(|trait_id| {
            self.program
                .traits
                .iter()
                .any(|definition| definition.id == *trait_id && definition.name == trait_name)
        })
    }

    fn emit_structural_show_impl(&self, def: &MirTypeDef, out: &mut String) {
        let emit_show = def.auto_printable
            && !self.selected_trait_impl_for_type(def, crate::Generics::PRINTABLE);
        let emit_debug = self.derives_trait(def, crate::Generics::DEBUG)
            && !self.selected_trait_impl_for_type(def, crate::Generics::DEBUG);
        if (!emit_show && !emit_debug) || matches!(&def.kind, MirTypeDefKind::Alias { .. }) {
            return;
        }
        if emit_show {
            self.emit_structural_impl(
                def,
                crate::Generics::PRINTABLE,
                "JetShow",
                "jet_show",
                out,
            );
        }
        if emit_debug {
            self.emit_structural_impl(def, crate::Generics::DEBUG, "JetDebug", "jet_debug", out);
        }
    }

    fn emit_structural_impl(
        &self,
        def: &MirTypeDef,
        trait_name: &str,
        rust_trait: &str,
        value_method: &str,
        out: &mut String,
    ) {
        let debug = value_method == "jet_debug";
        let name = self.type_name(def.id);
        let type_args = if def.generic_params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                def.generic_params
                    .iter()
                    .map(|param| mangle(&param.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let impl_generics = self.structural_generics(def, trait_name);
        let source_name = quote_rust_string(
            def.name
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(def.name.as_str()),
        );
        let _ = writeln!(out, "impl{impl_generics} {rust_trait} for {name}{type_args} {{");
        let _ = writeln!(out, "    fn {value_method}(&self) -> String {{");
        match &def.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                let record = if debug {
                    "jet_debug_record_fields"
                } else {
                    "jet_debug_record"
                };
                let _ = writeln!(out, "        {record}({source_name}, [");
                for (storage_index, field) in fields.iter().enumerate() {
                    if field.computed {
                        continue;
                    }
                    let field_name = quote_rust_string(
                        field
                            .name
                            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                            .unwrap_or(field.name.as_str()),
                    );
                    let value =
                        self.structural_field_value(def, &field.name, "self", field, value_method);
                    if debug {
                        let _ = writeln!(
                            out,
                            "            JetDebugField {{ name: {field_name}.to_string(), value: {value}, storage_index: {storage_index}, redacted: false }},"
                        );
                    } else {
                        let _ = writeln!(out, "            ({field_name}.to_string(), {value}),");
                    }
                }
                let _ = writeln!(out, "        ])");
            }
            MirTypeDefKind::Enum { variants, .. } => {
                let _ = writeln!(out, "        match self {{");
                for variant in variants {
                    let variant_name = quote_rust_string(
                        variant
                            .name
                            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                            .unwrap_or(variant.name.as_str()),
                    );
                    let variant_path = format!("Self::{}", mangle(&variant.name));
                    match &variant.payload {
                        MirVariantPayload::Unit => {
                            let _ = writeln!(
                                out,
                                "            {variant_path} => jet_debug_variant({variant_name}, None),"
                            );
                        }
                        MirVariantPayload::Single(_) => {
                            let value = if def
                                .boxed_edges
                                .iter()
                                .any(|candidate| candidate == &variant.name)
                            {
                                format!("payload.as_ref().{value_method}()")
                            } else {
                                format!("payload.{value_method}()")
                            };
                            let _ = writeln!(
                                out,
                                "            {variant_path}(payload) => jet_debug_variant({variant_name}, Some({value})),"
                            );
                        }
                        MirVariantPayload::Named(fields) => {
                            let visible = fields
                                .iter()
                                .enumerate()
                                .filter(|(_, field)| !field.computed)
                                .collect::<Vec<_>>();
                            let pattern = if visible.is_empty() {
                                format!("{variant_path} {{ .. }}")
                            } else {
                                let bindings = visible
                                    .iter()
                                    .enumerate()
                                    .map(|(index, (_, field))| {
                                        format!(
                                            "{}: __jet_show_field_{index}",
                                            self.field_name(field.id)
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("{variant_path} {{ {bindings}, .. }}")
                            };
                            if visible.is_empty() {
                                let _ = writeln!(
                                    out,
                                    "            {pattern} => jet_debug_record({variant_name}, []),"
                                );
                            } else {
                                let values = visible
                                    .iter()
                                    .enumerate()
                                    .map(|(index, (storage_index, field))| {
                                        let edge = format!("{}.{}", variant.name, field.name);
                                        let value = if def
                                            .boxed_edges
                                            .iter()
                                            .any(|candidate| candidate == &edge)
                                        {
                                            format!(
                                                "__jet_show_field_{index}.as_ref().{value_method}()"
                                            )
                                        } else {
                                            format!(
                                                "__jet_show_field_{index}.{value_method}()"
                                            )
                                        };
                                        let field_name = quote_rust_string(
                                            field
                                                .name
                                                .strip_prefix(
                                                    jet_foundation::Syntax::GENERATED_NAME_PREFIX,
                                                )
                                                .unwrap_or(field.name.as_str()),
                                        );
                                        if debug {
                                            format!(
                                                "JetDebugField {{ name: {field_name}.to_string(), value: {value}, storage_index: {storage_index}, redacted: false }}"
                                            )
                                        } else {
                                            format!("({field_name}.to_string(), {value})")
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let record = if debug {
                                    "jet_debug_record_fields"
                                } else {
                                    "jet_debug_record"
                                };
                                let _ = writeln!(
                                    out,
                                    "            {pattern} => {record}({variant_name}, [{values}]),"
                                );
                            }
                        }
                    }
                }
                let _ = writeln!(out, "        }}");
            }
            MirTypeDefKind::Distinct { .. } => {
                let _ = writeln!(out, "        self.0.{value_method}()");
            }
            MirTypeDefKind::UnitFamily { members } => {
                let _ = writeln!(out, "        match self {{");
                for member in members {
                    let member_name = quote_rust_string(
                        member
                            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                            .unwrap_or(member.as_str()),
                    );
                    let member_path = format!("Self::{}", mangle(member));
                    let _ = writeln!(
                        out,
                        "            {member_path} => jet_debug_variant({member_name}, None),"
                    );
                }
                let _ = writeln!(out, "        }}");
            }
            MirTypeDefKind::Alias { .. } => {
                unreachable!("aliases do not own Printable implementations")
            }
        }
        let _ = writeln!(out, "    }}\n}}\n");
    }

    fn emit_type_def(&self, def: &'a MirTypeDef, out: &mut String) {
        if self.history_native_type_name(&def.key).is_some() {
            return;
        }
        self.push_generic_scope(&def.generic_params);
        let name = self.type_name(def.id);
        let generics = self.generic_params_from(&def.generic_params);
        let visibility = self.visibility(if def.public {
            MirVisibility::Public
        } else if def.package_public {
            MirVisibility::Package
        } else {
            MirVisibility::Private
        });
        if let Some(layout) = def.layout {
            match layout {
                MirStructLayout::C => {
                    let _ = writeln!(out, "#[repr(C)]");
                }
                MirStructLayout::CAligned { alignment, .. } => {
                    let fact = def.layout_alignment.as_ref().unwrap_or_else(|| {
                        panic!(
                            "MIR aligned type {:?} has no sema layout alignment fact",
                            def.id
                        )
                    });
                    if fact.requested_alignment != alignment {
                        panic!(
                            "MIR aligned type {:?} fact/request mismatch: {} != {}",
                            def.id, fact.requested_alignment, alignment
                        );
                    }
                    let _ = writeln!(out, "#[repr(C, align({}))]", fact.effective_alignment);
                }
                MirStructLayout::Columnar => {}
            }
        }
        let mut derives = vec!["Clone", "Debug"];
        for trait_id in &def.derives {
            let trait_name = self
                .program
                .traits
                .iter()
                .find(|trait_def| trait_def.id == *trait_id)
                .map(|trait_def| trait_def.name.as_str())
                .unwrap_or_else(|| panic!("MIR derive trait ID {:?} has no row", trait_id));
            if matches!(
                trait_name,
                "Clone"
                    | "Copy"
                    | "Debug"
                    | "Default"
                    | "Eq"
                    | "Hash"
                    | "Ord"
                    | "PartialEq"
                    | "PartialOrd"
            ) {
                if !derives.contains(&trait_name) {
                    derives.push(trait_name);
                }
            } else {
                self.validate_derived_trait(def, *trait_id, trait_name);
            }
        }
        if !matches!(&def.kind, MirTypeDefKind::Alias { .. }) {
            let _ = writeln!(out, "#[derive({})]", derives.join(", "));
        }
        if def.must_use {
            let _ = writeln!(out, "#[must_use]");
        }
        for attribute in &def.serde {
            match (&attribute.kind, &attribute.value) {
                (MirSerdeAttributeKind::RenameAll, Some(_))
                | (MirSerdeAttributeKind::Tag, Some(_))
                | (MirSerdeAttributeKind::Untagged, None)
                | (MirSerdeAttributeKind::DenyUnknownFields, None) => {}
                (MirSerdeAttributeKind::RenameAll, None)
                | (MirSerdeAttributeKind::Tag, None)
                | (MirSerdeAttributeKind::Untagged, Some(_))
                | (MirSerdeAttributeKind::DenyUnknownFields, Some(_)) => {
                    panic!("MIR serde attribute has an invalid payload");
                }
            }
        }
        match &def.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                let _ = writeln!(out, "{visibility}struct {name}{generics} {{");
                for field in fields.iter().filter(|field| !field.computed) {
                    let _ = writeln!(
                        out,
                        "    {}{}: {},",
                        self.field_visibility(field),
                        self.field_name(field.id),
                        self.rust_decl_type(def, &field.name, &field.ty)
                    );
                }
                let _ = writeln!(out, "}}\n");
            }
            MirTypeDefKind::Enum { variants, .. } => {
                let _ = writeln!(out, "{visibility}enum {name}{generics} {{");
                for variant in variants {
                    match &variant.payload {
                        MirVariantPayload::Unit => {
                            let _ = writeln!(out, "    {},", mangle(&variant.name));
                        }
                        MirVariantPayload::Single(ty) => {
                            let _ = writeln!(
                                out,
                                "    {}({}),",
                                mangle(&variant.name),
                                self.rust_decl_type(def, &variant.name, ty)
                            );
                        }
                        MirVariantPayload::Named(fields) => {
                            let _ = writeln!(out, "    {} {{", mangle(&variant.name));
                            for field in fields {
                                let edge = format!("{}.{}", variant.name, field.name);
                                let _ = writeln!(
                                    out,
                                    "        {}: {},",
                                    self.field_name(field.id),
                                    self.rust_decl_type(def, &edge, &field.ty)
                                );
                            }
                            let _ = writeln!(out, "    }},");
                        }
                    }
                }
                let _ = writeln!(out, "}}\n");
            }
            MirTypeDefKind::Distinct { base, .. } => {
                let _ = writeln!(
                    out,
                    "#[repr(transparent)]\n{visibility}struct {name}{generics}(pub {});\n",
                    self.rust_type(base)
                );
            }
            MirTypeDefKind::Alias { target } => {
                let _ = writeln!(
                    out,
                    "{visibility}type {name}{generics} = {};\n",
                    self.rust_type(target)
                );
            }
            MirTypeDefKind::UnitFamily { members } => {
                let _ = writeln!(out, "{visibility}enum {name}{generics} {{");
                for member in members {
                    let _ = writeln!(out, "    {},", mangle(member));
                }
                let _ = writeln!(out, "}}\n");
            }
        }
        self.emit_structural_show_impl(def, out);
        if matches!(def.layout, Some(MirStructLayout::Columnar)) {
            match &def.kind {
                MirTypeDefKind::Struct { fields, .. } => {
                    self.emit_columnar_row_adapter(def, fields, out);
                }
                MirTypeDefKind::Enum { .. }
                | MirTypeDefKind::Distinct { .. }
                | MirTypeDefKind::Alias { .. }
                | MirTypeDefKind::UnitFamily { .. } => {
                    panic!("MIR columnar layout requires a struct type definition");
                }
            }
        }
        self.pop_generic_scope();
    }
    fn history_generate_expression(&self, ty: &MirType) -> Option<String> {
        let (kind, range) = self.history_scalar_spec(ty)?;
        match kind {
            HistoryScalarKind::Boolean => Some("rng.below(2) != 0".to_string()),
            HistoryScalarKind::Text => Some("format!(\"history-{}\", rng.below(17))".to_string()),
            HistoryScalarKind::Char => {
                Some("char::from_u32('a' as u32 + rng.below(26) as u32).unwrap_or('a')".to_string())
            }
            HistoryScalarKind::Integer
            | HistoryScalarKind::SignedInteger
            | HistoryScalarKind::UnsignedInteger => {
                let (mut lo, hi) = range.unwrap_or(match kind {
                    HistoryScalarKind::UnsignedInteger => (0, 16),
                    _ => (-8, 8),
                });
                if matches!(kind, HistoryScalarKind::UnsignedInteger) {
                    lo = lo.max(0);
                }
                let span = (i128::from(hi) - i128::from(lo) + 1).max(1) as u64;
                Some(format!(
                    "({lo}i128 + (rng.below({span}) as i128)) as {}",
                    self.rust_type(ty)
                ))
            }
        }
    }

    fn history_value_expression(kind: HistoryScalarKind, value: &str) -> String {
        match kind {
            HistoryScalarKind::Integer
            | HistoryScalarKind::SignedInteger
            | HistoryScalarKind::UnsignedInteger => {
                format!("HistoryValue::Integer({value} as i64)")
            }
            HistoryScalarKind::Boolean => format!("HistoryValue::Boolean({value})"),
            HistoryScalarKind::Text => format!("HistoryValue::Text({value}.clone())"),
            HistoryScalarKind::Char => format!("HistoryValue::Text({value}.to_string())"),
        }
    }

    fn history_tree_expression(kind: Option<HistoryScalarKind>, value: &str) -> String {
        match kind {
            Some(
                HistoryScalarKind::Integer
                | HistoryScalarKind::SignedInteger
                | HistoryScalarKind::UnsignedInteger,
            ) => format!("jet_std::DataTree::Int({value} as i64)"),
            Some(HistoryScalarKind::Boolean) => {
                format!("jet_std::DataTree::Bool({value})")
            }
            Some(HistoryScalarKind::Text) => {
                format!("jet_std::DataTree::Text({value}.clone())")
            }
            Some(HistoryScalarKind::Char) => {
                format!("jet_std::DataTree::Text({value}.to_string())")
            }
            None => "jet_std::DataTree::Null".to_string(),
        }
    }

    fn history_command_tree_expression(&self, field: &MirType, value: &str) -> String {
        if let Some((kind, _)) = self.history_scalar_spec(field) {
            return Self::history_tree_expression(Some(kind), value);
        }
        let nominal_name = field.nominal_name().unwrap_or_default();
        let nominal_name = nominal_name
            .rsplit_once("::")
            .or_else(|| nominal_name.rsplit_once('.'))
            .map_or(nominal_name, |(_, leaf)| leaf);
        match nominal_name {
            "HandleId" | "TaskId" | "EventId" => {
                format!("jet_std::DataTree::Int({value}.value as i64)")
            }
            _ => format!("{value}.jet_encode()"),
        }
    }

    fn history_rebuild_expression(
        &self,
        ty: &MirType,
        kind: HistoryScalarKind,
        index: usize,
    ) -> String {
        let rust_ty = self.rust_type(ty);
        let (_, range) = self
            .history_scalar_spec(ty)
            .unwrap_or_else(|| panic!("unsupported generated history field"));
        let argument = format!("&__history_arguments[{index}]");
        match kind {
            HistoryScalarKind::Integer
            | HistoryScalarKind::SignedInteger
            | HistoryScalarKind::UnsignedInteger => {
                let mut bounds = String::new();
                if let Some((lo, hi)) = range {
                    bounds.push_str(&format!(
                        "if *value < {lo} || *value > {hi} {{ return None; }}"
                    ));
                }
                if matches!(kind, HistoryScalarKind::UnsignedInteger) {
                    bounds.push_str("if *value < 0 { return None; }");
                }
                let cast = if matches!(kind, HistoryScalarKind::Integer) {
                    String::new()
                } else {
                    format!(" as {rust_ty}")
                };
                format!(
                    "match {argument} {{ HistoryValue::Integer(value) => {{ {bounds} *value{cast} }}, _ => return None }}"
                )
            }
            HistoryScalarKind::Boolean => {
                format!("match {argument} {{ HistoryValue::Boolean(value) => *value, _ => return None }}")
            }
            HistoryScalarKind::Text => {
                format!("match {argument} {{ HistoryValue::Text(value) => value.clone(), _ => return None }}")
            }
            HistoryScalarKind::Char => format!(
                "match {argument} {{ HistoryValue::Text(value) => {{ let mut chars = value.chars(); let value = chars.next()?; if chars.next().is_some() {{ return None; }} value }}, _ => return None }}"
            ),
        }
    }

    fn history_valid_expression(kind: HistoryScalarKind, index: usize, value: &str) -> String {
        match kind {
            HistoryScalarKind::Integer
            | HistoryScalarKind::SignedInteger
            | HistoryScalarKind::UnsignedInteger => format!(
                "matches!(__history_arguments.get({index}), Some(HistoryValue::Integer(raw)) if *raw == *{value} as i64)"
            ),
            HistoryScalarKind::Boolean => format!(
                "matches!(__history_arguments.get({index}), Some(HistoryValue::Boolean(raw)) if *raw == *{value})"
            ),
            HistoryScalarKind::Text => format!(
                "matches!(__history_arguments.get({index}), Some(HistoryValue::Text(raw)) if raw == {value})"
            ),
            HistoryScalarKind::Char => format!(
                "matches!(__history_arguments.get({index}), Some(HistoryValue::Text(raw)) if raw == &(*{value}).to_string())"
            ),
        }
    }

    fn history_base_provenance(&self) -> (String, String, String) {
        let source = self.artifact_identity.identity_digest();
        let tool = format!(
            "{}:{}",
            jet_foundation::TestingHistory::HISTORY_ENGINE,
            self.program.facts.target_dossier.compiler_identity
        );
        let target = jet_foundation::SHA256::sha256_hex(
            &self
                .program
                .facts
                .target_dossier
                .cache_bytes(&self.config.target.triple),
        );
        (source, tool, target)
    }

    fn history_runtime_metadata_enabled(&self) -> bool {
        self.program
            .core_calls
            .iter()
            .any(|call| call.module == "core.testing" && call.member == "histories")
    }

    fn history_native_type_name(&self, name: &str) -> Option<String> {
        let name = crate::Codegen::history_rust_type_name(name)?;
        Some(match name {
            "HistoryStrategy" => format!("{}JetHistoryStrategy", self.config.root_prefix),
            "HistoryRng" => format!("{}JetHistoryRng", self.config.root_prefix),
            _ => format!("crate::jet_testing_history_foundation::{name}"),
        })
    }

    fn history_call_expression(
        &self,
        function: &MirFunction,
        args: &[MirCallArg],
        type_args: &[MirType],
        borrow_mask: &[bool],
    ) -> String {
        let [command_type] = type_args else {
            panic!("MIR histories requires one checked command type");
        };
        if args.len() != 6 {
            panic!("MIR histories requires six checked arguments");
        }
        let mut values = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                self.call_arg_for_function(
                    function,
                    arg,
                    borrow_mask.get(index).copied().unwrap_or(false),
                )
            })
            .collect::<Vec<_>>();
        values[2] = format!("({}).ok()", values[2]);
        format!(
            "{}jet_testing_histories_with_provenance::<{}>({}, {})",
            self.config.root_prefix,
            self.rust_type(command_type),
            values.join(", "),
            self.history_provenance_expression()
        )
    }

    fn history_provenance_expression(&self) -> String {
        let (source, tool, target) = self.history_base_provenance();
        format!(
            "Some(crate::jet_testing_history_foundation::HistoryProvenance {{ source: {}.to_string(), tool: {}.to_string(), target: {}.to_string() }})",
            quote_rust_string(&source), quote_rust_string(&tool), quote_rust_string(&target),
        )
    }

    // Emit codecs from checked types, but read their values only when the
    // selected runtime callable is bound. Recursive records call the same
    // codec; no operand ID or source-expression evaluation enters the hash.
    fn history_capture_expression(&self, ty: &MirType, value: &str) -> String {
        if let MirTypeKind::Apply { name, args } = ty.kind() {
            if args.is_empty() && self.history_type_parameters(ty).contains(&name.name) {
                return format!("{}jet_history_encode_type(&__jet_history_types, {}, {value}, __jet_history_depth + 1)?", self.config.root_prefix, quote_rust_string(&name.name));
            }
        }
        let key = ty.canonical_key();
        let name = format!(
            "__jet_history_capture_{}",
            jet_foundation::SHA256::sha256_hex(key.as_bytes())
        );
        let call = format!(
            "{}{}({value}, __jet_history_depth + 1, &__jet_history_types)?",
            self.config.root_prefix, name
        );
        if self.history_capture_encoders.borrow().contains_key(&key) {
            return call;
        }
        // Reserve before traversing fields so recursive types terminate here.
        self.history_capture_encoders
            .borrow_mut()
            .insert(key.clone(), String::new());
        let tree = format!("{}jet_std::DataTree", self.config.root_prefix);
        let unavailable = "return Err(\"history capture is opaque or redacted\".to_string())";
        let inner = |ty: &MirType, value: &str| self.history_capture_expression(ty, value);
        let body = if self.is_handle_type(ty)
            // `Group` is also the source spelling of the zero-arity task-group
            // handle. Do not resolve that builtin through the generic
            // collection `Group<K, V>` definition when emitting history code.
            || matches!(
                ty.kind(),
                MirTypeKind::Apply { name, args }
                    if name.name == jet_foundation::Syntax::TYPE_TASKGROUP && args.is_empty()
            ) {
            unavailable.to_string()
        } else {
            match ty.kind() {
                MirTypeKind::Int | MirTypeKind::IntN { .. } => {
                    format!("{tree}::Text(value.to_string())")
                }
                MirTypeKind::Float | MirTypeKind::Float32 | MirTypeKind::Measure(_) => {
                    format!("{tree}::Text(value.to_bits().to_string())")
                }
                MirTypeKind::Bool => format!("{tree}::Bool(*value)"),
                MirTypeKind::String | MirTypeKind::Char => {
                    format!("{tree}::Text(value.to_string())")
                }
                MirTypeKind::InlineRange { base, .. } | MirTypeKind::Quantity { base, .. } => {
                    inner(base, "value")
                }
                MirTypeKind::Tagged {
                    marker,
                    inner: base,
                } => {
                    if marker.to_string().to_ascii_lowercase().contains("secret") {
                        unavailable.to_string()
                    } else {
                        inner(base, "value")
                    }
                }
                MirTypeKind::List(elem) | MirTypeKind::FixedList { elem, .. } => {
                    let item = inner(elem, "item");
                    let list = if self.columnar_type_def(elem).is_some() {
                        "value.to_aos()"
                    } else {
                        "value"
                    };
                    format!("{{ let mut items = Vec::new(); for item in {list}.iter() {{ items.push({item}); }} {tree}::Array(items) }}")
                }
                MirTypeKind::Map {
                    key,
                    value: item_ty,
                } => {
                    let key = inner(key, "key");
                    let item = inner(item_ty, "item");
                    format!("{{ let mut items = Vec::new(); for (key, item) in value.iter() {{ items.push({tree}::Array(vec![{key}, {item}])); }} {tree}::Array(items) }}")
                }
                MirTypeKind::Shared(base) => {
                    let item = inner(base, "item");
                    format!("value.read(|item| -> Result<{tree}, String> {{ Ok({item}) }})?")
                }
                MirTypeKind::Option(base) => {
                    let item = inner(base, "item");
                    format!("match value {{ Ok(item) => {tree}::Array(vec![{item}]), Err(_) => {tree}::Array(vec![]) }}")
                }
                MirTypeKind::Result { ok, err } => {
                    let ok = inner(ok, "item");
                    let err = inner(err, "item");
                    format!("match value {{ Ok(item) => {tree}::Array(vec![{tree}::Text(\"ok\".to_string()), {ok}]), Err(item) => {tree}::Array(vec![{tree}::Text(\"err\".to_string()), {err}]) }}")
                }
                MirTypeKind::Tuple(fields) => {
                    let fields = fields
                        .iter()
                        .enumerate()
                        .map(|(index, (_, ty))| inner(ty, &format!("&value.{index}")))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{tree}::Array(vec![{fields}])")
                }
                MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. } => {
                    format!(
                        "{}jet_history_callback_captures(value.as_ref())?",
                        self.config.root_prefix
                    )
                }
                MirTypeKind::Apply {
                    name: nominal,
                    args,
                } => {
                    if nominal.name == jet_foundation::Syntax::INTERNAL_UNIT_TYPE {
                        format!("{tree}::Null")
                    } else if crate::Codegen::history_rust_type_name(&nominal.name) == Some("Count")
                    {
                        format!("{tree}::Text(value.to_string())")
                    } else if crate::Codegen::history_rust_type_name(&nominal.name)
                        == Some("HistoryRng")
                    {
                        unavailable.to_string()
                    } else if matches!(
                        nominal.name.as_str(),
                        "DataTree" | "core.DataTree" | "core::DataTree"
                    ) {
                        format!(
                            "{}jet_history_data_tree_capture(value, __jet_history_depth)?",
                            self.config.root_prefix
                        )
                    } else if nominal.name.to_ascii_lowercase().contains("secret") {
                        unavailable.to_string()
                    } else if let Some(def) =
                        self.program.types.iter().find(|def| def.id == nominal.id)
                    {
                        let bindings = def
                            .generic_params
                            .iter()
                            .zip(args)
                            .map(|(param, ty)| (param.name.clone(), ty.clone()))
                            .collect::<BTreeMap<_, _>>();
                        let field_value = |ty: &MirType, value: &str| {
                            inner(&Self::history_capture_type(ty, &bindings), value)
                        };
                        match &def.kind {
                            MirTypeDefKind::Struct { fields, .. } => {
                                if fields.iter().any(|field| field.skip) {
                                    unavailable.to_string()
                                } else {
                                    let fields = fields.iter().filter(|field| !field.computed).map(|field| {
                                        let field_name = self.field_name(field.id);
                                        let raw = format!("value.{field_name}.clone()");
                                        let converted = if def.key == jet_foundation::Syntax::TYPE_ERR
                                            && field.name == "cause"
                                        {
                                            format!(
                                                "match &value.{field_name} {{ Ok(item) => Ok((**item).clone()), Err(_) => Err({}JetAbsent) }}",
                                                self.config.root_prefix,
                                            )
                                        } else {
                                            self.history_source_field_value(&def.key, &field.name, raw.clone())
                                        };
                                        let encoded = if converted != raw {
                                            let encoded = field_value(&field.ty, "&__jet_history_field");
                                            format!("{{ let __jet_history_field = {converted}; {encoded} }}")
                                        } else {
                                            field_value(&field.ty, &format!("&value.{field_name}"))
                                        };
                                        format!("({}.to_string(), {encoded})", quote_rust_string(&field.name))
                                    }).collect::<Vec<_>>().join(", ");
                                    format!("{tree}::Object(vec![{fields}])")
                                }
                            }
                            MirTypeDefKind::Enum { variants, .. } => {
                                let head = self.type_name(def.id);
                                let arms = variants.iter().map(|variant| {
                                    let path = format!("{head}::{}", mangle(&variant.name));
                                    let tag = quote_rust_string(&variant.name);
                                    let (pattern, fields) = match &variant.payload {
                                        MirVariantPayload::Unit => (path, String::new()),
                                        MirVariantPayload::Single(ty) => (format!("{path}(item)"), field_value(ty, "item")),
                                        MirVariantPayload::Named(fields) => {
                                            let pattern = fields.iter().enumerate().map(|(index, field)| format!("{}: item_{index}", self.field_name(field.id))).collect::<Vec<_>>().join(", ");
                                            if fields.iter().any(|field| field.skip) {
                                                return format!("{path} {{ .. }} => {{ {unavailable} }}");
                                            }
                                            let values = fields.iter().enumerate().map(|(index, field)| field_value(&field.ty, &format!("item_{index}"))).collect::<Vec<_>>().join(", ");
                                            (format!("{path} {{ {pattern} }}"), values)
                                        }
                                    };
                                    format!("{pattern} => {tree}::Array(vec![{tree}::Text({tag}.to_string()), {fields}])")
                                }).collect::<Vec<_>>().join(", ");
                                format!("match value {{ {arms} }}")
                            }
                            MirTypeDefKind::Distinct { base, .. } => field_value(base, "&value.0"),
                            MirTypeDefKind::Alias { target } => field_value(target, "value"),
                            MirTypeDefKind::UnitFamily { members } => {
                                let head = self.type_name(def.id);
                                let arms = members
                                    .iter()
                                    .map(|member| {
                                        format!(
                                            "{head}::{} => {tree}::Text({}.to_string())",
                                            mangle(member),
                                            quote_rust_string(member)
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("match value {{ {arms} }}")
                            }
                        }
                    } else {
                        // A genuinely unresolved nominal carrier has no codec.
                        self.history_capture_encoders.borrow_mut().remove(&key);
                        return format!("{{ {unavailable} }}");
                    }
                }
                MirTypeKind::Union(_) => {
                    if let Some(def) = ty.identity.and_then(|id| self.history_type_def(id)) {
                        let nominal = MirType::from_kind(MirTypeKind::Apply {
                            name: MirNominalRef {
                                id: def.id,
                                name: def.key.clone(),
                            },
                            args: Vec::new(),
                        });
                        inner(&nominal, "value")
                    } else {
                        unavailable.to_string()
                    }
                }
                MirTypeKind::TraitObject(_) => unavailable.to_string(),
            }
        };
        let rust_ty = self.rust_type(ty);
        let parameters = self.history_type_parameters(ty);
        let generics = if parameters.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                parameters
                    .iter()
                    .map(|name| format!("{}: 'static", mangle(name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let checked_key = self.history_capture_key(ty);
        let constraints = self.history_codec_constraints(ty);
        let constraints = if constraints.is_empty() {
            String::new()
        } else {
            format!(
                " where {}",
                constraints.into_iter().collect::<Vec<_>>().join(", ")
            )
        };
        let encoded = format!(
            "fn {name}{generics}(value: &{rust_ty}, __jet_history_depth: usize, __jet_history_types: &{}JetHistoryTypes) -> Result<{tree}, String>{constraints} {{\n    if __jet_history_depth >= 64 {{ return Err(\"history capture exceeds the nesting bound\".to_string()); }}\n    let value = {{ {body} }};\n    Ok({tree}::Array(vec![{tree}::Text({checked_key}), value]))\n}}\n",
            self.config.root_prefix,
        );
        self.history_capture_encoders
            .borrow_mut()
            .insert(key, encoded);
        call
    }

    fn history_codec_constraints(&self, ty: &MirType) -> BTreeSet<String> {
        let mut constraints = BTreeSet::new();
        let mut children = Vec::new();
        match ty.kind() {
            MirTypeKind::Apply { name, args } => {
                if let Some(def) = self.program.types.iter().find(|def| def.id == name.id) {
                    for (param, argument) in def.generic_params.iter().zip(args) {
                        if !param.bounds.is_empty() {
                            constraints.insert(format!(
                                "{}: {}",
                                self.rust_type(argument),
                                param
                                    .bounds
                                    .iter()
                                    .map(|bound| self.trait_nominal_name(bound))
                                    .collect::<Vec<_>>()
                                    .join(" + ")
                            ));
                        }
                    }
                }
                children.extend(args);
            }
            MirTypeKind::List(ty)
            | MirTypeKind::Shared(ty)
            | MirTypeKind::Option(ty)
            | MirTypeKind::FixedList { elem: ty, .. }
            | MirTypeKind::InlineRange { base: ty, .. }
            | MirTypeKind::Tagged { inner: ty, .. }
            | MirTypeKind::Quantity { base: ty, .. } => children.push(ty),
            MirTypeKind::Map { key, value }
            | MirTypeKind::Result {
                ok: key,
                err: value,
            } => {
                children.push(key);
                children.push(value);
            }
            MirTypeKind::Tuple(fields) => children.extend(fields.iter().map(|(_, ty)| ty)),
            MirTypeKind::Union(types) => children.extend(types),
            MirTypeKind::Fn(signature) => {
                children.extend(&signature.params);
                if let Some(ty) = &signature.ret {
                    children.push(ty);
                }
            }
            MirTypeKind::SendFn { params, ret } => {
                children.extend(params);
                if let Some(ty) = ret {
                    children.push(ty);
                }
            }
            _ => {}
        }
        for child in children {
            constraints.extend(self.history_codec_constraints(child));
        }
        constraints
    }

    fn history_type_parameters(&self, ty: &MirType) -> BTreeSet<String> {
        let known = self
            .program
            .functions
            .iter()
            .flat_map(|function| &function.generic_params)
            .chain(
                self.program
                    .types
                    .iter()
                    .flat_map(|def| &def.generic_params),
            )
            .map(|param| param.name.as_str())
            .collect::<BTreeSet<_>>();
        fn visit(ty: &MirType, known: &BTreeSet<&str>, found: &mut BTreeSet<String>) {
            match ty.kind() {
                MirTypeKind::Apply { name, args } => {
                    if args.is_empty() && known.contains(name.name.as_str()) {
                        found.insert(name.name.clone());
                    }
                    for ty in args {
                        visit(ty, known, found);
                    }
                }
                MirTypeKind::List(ty)
                | MirTypeKind::Shared(ty)
                | MirTypeKind::Option(ty)
                | MirTypeKind::FixedList { elem: ty, .. }
                | MirTypeKind::InlineRange { base: ty, .. }
                | MirTypeKind::Tagged { inner: ty, .. }
                | MirTypeKind::Quantity { base: ty, .. } => visit(ty, known, found),
                MirTypeKind::Map { key, value }
                | MirTypeKind::Result {
                    ok: key,
                    err: value,
                } => {
                    visit(key, known, found);
                    visit(value, known, found);
                }
                MirTypeKind::Tuple(fields) => {
                    for (_, ty) in fields {
                        visit(ty, known, found);
                    }
                }
                MirTypeKind::Union(types) => {
                    for ty in types {
                        visit(ty, known, found);
                    }
                }
                MirTypeKind::Fn(signature) => {
                    for ty in &signature.params {
                        visit(ty, known, found);
                    }
                    if let Some(ty) = &signature.ret {
                        visit(ty, known, found);
                    }
                }
                MirTypeKind::SendFn { params, ret } => {
                    for ty in params {
                        visit(ty, known, found);
                    }
                    if let Some(ty) = ret {
                        visit(ty, known, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = BTreeSet::new();
        visit(ty, &known, &mut found);
        found
    }

    fn history_capture_key(&self, ty: &MirType) -> String {
        let parameters = self.history_type_parameters(ty);
        if parameters.is_empty() {
            return format!("{}.to_string()", quote_rust_string(&ty.canonical_key()));
        }
        // Substitute checked type nodes, never text in a nominal name. The
        // temporary markers only split the compiler's canonical type template.
        let bindings = parameters
            .iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    name.clone(),
                    MirType::from_kind(MirTypeKind::Apply {
                        name: MirNominalRef {
                            id: MirTypeId(0),
                            name: format!("\0{index}\0"),
                        },
                        args: Vec::new(),
                    }),
                )
            })
            .collect();
        let template = Self::history_capture_type(ty, &bindings).canonical_key();
        let mut parts = Vec::new();
        for (index, part) in template.split('\0').enumerate() {
            if index % 2 == 0 {
                parts.push(format!("{}.to_string()", quote_rust_string(part)));
            } else {
                let name = parameters
                    .iter()
                    .nth(part.parse::<usize>().expect("type template marker"))
                    .expect("type template binding");
                parts.push(format!("__jet_history_types.get({}).ok_or_else(|| \"history capture type is unresolved\".to_string())?.checked_type.clone()", quote_rust_string(name)));
            }
        }
        format!("[{}].concat()", parts.join(", "))
    }

    fn history_instantiated_call(
        &self,
        callee: &MirCallee,
        type_args: &[MirType],
        invocation: String,
    ) -> String {
        if !self.history_runtime_metadata_enabled() {
            return invocation;
        }
        let (target, owner) = match callee {
            MirCallee::User(id) => (self.function_row(*id), None),
            MirCallee::Associated { function, owner } | MirCallee::Method { function, owner } => {
                (self.function_row(*function), Some(owner))
            }
            _ => return invocation,
        };
        let mut bindings = target
            .generic_params
            .iter()
            .zip(type_args)
            .map(|(param, ty)| (param.name.clone(), ty))
            .collect::<BTreeMap<_, _>>();
        if let Some(MirType {
            kind: MirTypeKind::Apply { name, args },
            ..
        }) = owner
        {
            if let Some(def) = self.program.types.iter().find(|def| def.id == name.id) {
                bindings.extend(
                    def.generic_params
                        .iter()
                        .zip(args)
                        .map(|(param, ty)| (param.name.clone(), ty)),
                );
            }
        }
        if bindings.is_empty() {
            return invocation;
        }
        let root = &self.config.root_prefix;
        let mut setup = String::new();
        for (name, ty) in bindings {
            let encoded = self.history_capture_expression(ty, "__jet_value");
            let key = self.history_capture_key(ty);
            let ty = self.rust_type(ty);
            // Missing dictionaries remain errors when the codec is used. A
            // missing type argument never becomes an opaque success identity.
            let _ = writeln!(setup, "__jet_instantiated.insert({}.to_string(), {{ let __jet_history_types = __jet_history_types.clone(); let __jet_key = (|| -> Result<String, String> {{ Ok({key}) }})(); {root}jet_history_type_codec::<{ty}>(__jet_key.unwrap_or_default(), move |__jet_value, __jet_history_depth| {{ Ok({encoded}) }}) }});", quote_rust_string(&name));
        }
        format!("{{ let mut __jet_instantiated = {root}JetHistoryTypes::new(); {setup} {root}jet_history_with_types(__jet_instantiated, || {invocation}) }}")
    }

    fn history_capture_type(ty: &MirType, bindings: &BTreeMap<String, MirType>) -> MirType {
        let sub = |ty: &MirType| Self::history_capture_type(ty, bindings);
        let kind = match ty.kind() {
            MirTypeKind::Apply { name, args } => {
                if args.is_empty() {
                    if let Some(ty) = bindings.get(&name.name) {
                        return ty.clone();
                    }
                }
                MirTypeKind::Apply {
                    name: name.clone(),
                    args: args.iter().map(sub).collect(),
                }
            }
            MirTypeKind::List(ty) => MirTypeKind::List(Box::new(sub(ty))),
            MirTypeKind::Shared(ty) => MirTypeKind::Shared(Box::new(sub(ty))),
            MirTypeKind::Option(ty) => MirTypeKind::Option(Box::new(sub(ty))),
            MirTypeKind::Map { key, value } => MirTypeKind::Map {
                key: Box::new(sub(key)),
                value: Box::new(sub(value)),
            },
            MirTypeKind::Result { ok, err } => MirTypeKind::Result {
                ok: Box::new(sub(ok)),
                err: Box::new(sub(err)),
            },
            MirTypeKind::Tuple(fields) => MirTypeKind::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), sub(ty)))
                    .collect(),
            ),
            MirTypeKind::Union(types) => MirTypeKind::Union(types.iter().map(sub).collect()),
            MirTypeKind::FixedList { elem, len } => MirTypeKind::FixedList {
                elem: Box::new(sub(elem)),
                len: len.clone(),
            },
            MirTypeKind::InlineRange { base, lo, hi } => MirTypeKind::InlineRange {
                base: Box::new(sub(base)),
                lo: *lo,
                hi: *hi,
            },
            MirTypeKind::Tagged { marker, inner } => MirTypeKind::Tagged {
                marker: marker.clone(),
                inner: Box::new(sub(inner)),
            },
            MirTypeKind::Quantity { base, dimension } => MirTypeKind::Quantity {
                base: Box::new(sub(base)),
                dimension: dimension.clone(),
            },
            MirTypeKind::Fn(signature) => {
                let mut signature = signature.clone();
                signature.params = signature.params.iter().map(sub).collect();
                signature.ret = signature.ret.as_deref().map(|ty| Box::new(sub(ty)));
                MirTypeKind::Fn(signature)
            }
            MirTypeKind::SendFn { params, ret } => MirTypeKind::SendFn {
                params: params.iter().map(sub).collect(),
                ret: ret.as_deref().map(|ty| Box::new(sub(ty))),
            },
            _ => return ty.clone(),
        };
        if bindings.is_empty() {
            ty.clone()
        } else {
            MirType::from_kind(kind)
        }
    }

    fn emit_history_strategies(&self, out: &mut String) {
        let foundation = "crate::jet_testing_history_foundation";
        let ids = self.history_command_type_ids();
        if ids.is_empty() {
            return;
        }
        out.push_str("use crate::jet_testing_history_foundation::HistoryValue;\n\n");
        let mut registrations = Vec::new();
        for id in ids {
            let Some(def) = self.history_type_def(id) else {
                continue;
            };
            if !self.module_selected(def.module) {
                continue;
            }
            let MirTypeDefKind::Enum { variants, .. } = &def.kind else {
                continue;
            };
            if !def.generic_params.is_empty() {
                continue;
            }
            let Some(command_ty) = self.types.get(&id) else {
                continue;
            };
            let suffix = id.0.to_string();
            let mut supported = Vec::new();
            let mut unsupported_variant = None;
            for (variant_index, variant) in variants.iter().enumerate() {
                let fields = match &variant.payload {
                    MirVariantPayload::Unit => Vec::new(),
                    MirVariantPayload::Single(ty) => vec![ty],
                    MirVariantPayload::Named(fields) => {
                        fields.iter().map(|field| &field.ty).collect()
                    }
                };
                let specs = fields
                    .iter()
                    .map(|field| self.history_scalar_spec(field))
                    .collect::<Option<Vec<_>>>();
                let generate_name = format!("__jet_history_generate_{suffix}_{variant_index}");
                let rebuild_name = format!("__jet_history_rebuild_{suffix}_{variant_index}");
                let valid_name = format!("__jet_history_valid_{suffix}_{variant_index}");
                if specs.is_none() {
                    if unsupported_variant.is_none() {
                        unsupported_variant = Some(variant.name.clone());
                    }
                    let _ = writeln!(
                        out,
                        "fn {generate_name}(_: &mut {foundation}::HistoryRng, _: u32) -> Option<{foundation}::HistoryGeneratedStep<{command_ty}>> {{ None }}\n\
                         fn {rebuild_name}(_: &{foundation}::HistoryOperation) -> Option<{command_ty}> {{ None }}\n\
                         fn {valid_name}(_: &{foundation}::HistoryOperation, _: &{command_ty}) -> bool {{ false }}\n"
                    );
                    continue;
                }
                let specs = specs.unwrap_or_default();
                let _ = writeln!(
                    out,
                    "fn {generate_name}(rng: &mut {foundation}::HistoryRng, index: u32) -> Option<{foundation}::HistoryGeneratedStep<{command_ty}>> {{"
                );
                for (field_index, field) in fields.iter().enumerate() {
                    let expression = self
                        .history_generate_expression(field)
                        .unwrap_or_else(|| "return None".to_string());
                    let _ = writeln!(
                        out,
                        "    let __history_value_{field_index}: {} = {expression};",
                        self.rust_type(field)
                    );
                }
                let mut operation = format!(
                    "    let mut operation = {foundation}::HistoryOperation::new(index, {});",
                    quote_rust_string(&variant.wire_name)
                );
                for (field_index, (kind, _)) in specs.iter().enumerate() {
                    operation.push_str(&format!(
                        "\n    operation = operation.with_argument({}::{});",
                        foundation,
                        Self::history_value_expression(
                            *kind,
                            &format!("__history_value_{field_index}"),
                        )
                    ));
                }
                out.push_str(&operation);
                let command_expression = match &variant.payload {
                    MirVariantPayload::Unit => {
                        format!("{command_ty}::{}", mangle(&variant.name))
                    }
                    MirVariantPayload::Single(_) => {
                        format!("{command_ty}::{}(__history_value_0)", mangle(&variant.name))
                    }
                    MirVariantPayload::Named(fields) => {
                        let fields = fields
                            .iter()
                            .enumerate()
                            .map(|(index, field)| {
                                format!("{}: __history_value_{index}", self.field_name(field.id))
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{command_ty}::{} {{ {fields} }}", mangle(&variant.name))
                    }
                };
                let _ = writeln!(
                    out,
                    "\n    Some({foundation}::HistoryGeneratedStep {{ operation, command: {command_expression} }})\n}}\n"
                );

                let _ = writeln!(
                    out,
                    "fn {rebuild_name}(operation: &{foundation}::HistoryOperation) -> Option<{command_ty}> {{"
                );
                let _ = writeln!(
                    out,
                    "    if operation.name != {} || operation.arguments.len() != {} {{ return None; }}",
                    quote_rust_string(&variant.wire_name),
                    fields.len()
                );
                out.push_str("    let __history_arguments = operation.arguments.as_slice();\n");
                for (field_index, field) in fields.iter().enumerate() {
                    let (kind, _) = specs[field_index];
                    let expression = self.history_rebuild_expression(field, kind, field_index);
                    let _ = writeln!(
                        out,
                        "    let __history_value_{field_index}: {} = {expression};",
                        self.rust_type(field)
                    );
                }
                let _ = writeln!(out, "    Some({command_expression})\n}}\n");

                let _ = writeln!(
                    out,
                    "fn {valid_name}(operation: &{foundation}::HistoryOperation, command: &{command_ty}) -> bool {{"
                );
                let _ = writeln!(
                    out,
                    "    if operation.name != {} {{ return false; }}\n    let __history_arguments = operation.arguments.as_slice();",
                    quote_rust_string(&variant.wire_name)
                );
                let checks = specs
                    .iter()
                    .enumerate()
                    .map(|(field_index, (kind, _))| {
                        Self::history_valid_expression(
                            *kind,
                            field_index,
                            &format!("__history_value_{field_index}"),
                        )
                    })
                    .collect::<Vec<_>>();
                let check = if checks.is_empty() {
                    "true".to_string()
                } else {
                    checks.join(" && ")
                };
                let pattern = match &variant.payload {
                    MirVariantPayload::Unit => {
                        format!("{command_ty}::{}", mangle(&variant.name))
                    }
                    MirVariantPayload::Single(_) => {
                        format!("{command_ty}::{}(__history_value_0)", mangle(&variant.name))
                    }
                    MirVariantPayload::Named(fields) => {
                        let fields = fields
                            .iter()
                            .enumerate()
                            .map(|(index, field)| {
                                format!("{}: __history_value_{index}", self.field_name(field.id))
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{command_ty}::{} {{ {fields} }}", mangle(&variant.name))
                    }
                };
                let _ = writeln!(
                    out,
                    "    match command {{ {pattern} => __history_arguments.len() == {} && {check}, _ => false }}\n}}\n",
                    fields.len()
                );
                supported.push((
                    variant_index,
                    variant,
                    generate_name,
                    rebuild_name,
                    valid_name,
                ));
            }

            let encode_name = format!("__jet_history_commands_to_tree_{suffix}");
            let _ = writeln!(
                out,
                "fn {encode_name}(commands: &[{command_ty}]) -> jet_std::DataTree {{\n    jet_std::DataTree::Array(commands.iter().map(|command| match command {{"
            );
            for variant in variants {
                let fields = match &variant.payload {
                    MirVariantPayload::Unit => Vec::new(),
                    MirVariantPayload::Single(ty) => vec![ty],
                    MirVariantPayload::Named(fields) => {
                        fields.iter().map(|field| &field.ty).collect()
                    }
                };
                let bindings = fields
                    .iter()
                    .enumerate()
                    .map(|(index, _)| format!("__history_value_{index}"))
                    .collect::<Vec<_>>();
                let pattern = match &variant.payload {
                    MirVariantPayload::Unit => {
                        format!("{command_ty}::{}", mangle(&variant.name))
                    }
                    MirVariantPayload::Single(_) => {
                        format!("{command_ty}::{}(__history_value_0)", mangle(&variant.name))
                    }
                    MirVariantPayload::Named(fields) => {
                        let fields = fields
                            .iter()
                            .enumerate()
                            .map(|(index, field)| {
                                format!("{}: __history_value_{index}", self.field_name(field.id))
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{command_ty}::{} {{ {fields} }}", mangle(&variant.name))
                    }
                };
                let values = fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        self.history_command_tree_expression(field, &bindings[index])
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "        {pattern} => jet_std::DataTree::Object(vec![(\"tag\".to_string(), jet_std::DataTree::Text({}.to_string())), (\"values\".to_string(), jet_std::DataTree::Array(vec![{values}]))]),",
                    quote_rust_string(&variant.wire_name)
                );
            }
            let unsupported_reason = if variants.is_empty() {
                Some(jet_foundation::TestingHistory::history_unsupported_type_reason(&def.name))
            } else {
                unsupported_variant.as_deref().map(|variant| {
                    jet_foundation::TestingHistory::history_unsupported_variant_reason(
                        &def.name, variant,
                    )
                })
            };
            out.push_str("    }).collect())\n}\n\n");

            let variants_name = format!("__JET_HISTORY_VARIANTS_{suffix}");
            let _ = writeln!(
                out,
                "static {variants_name}: &[{foundation}::HistoryGeneratedVariant<{command_ty}>] = &["
            );
            for (_, variant, generate_name, rebuild_name, valid_name) in &supported {
                let _ = writeln!(
                    out,
                    "    {foundation}::HistoryGeneratedVariant {{ operation: {}, weight: 1, generate: {generate_name}, rebuild: {rebuild_name}, valid: {valid_name} }},",
                    quote_rust_string(&variant.wire_name)
                );
            }
            out.push_str("];\n\n");
            let strategy_constructor = unsupported_reason.as_deref().map_or_else(
                || {
                    format!(
                        "{foundation}::HistoryGeneratedStrategy::new({}, {variants_name}, {encode_name})",
                        quote_rust_string(&def.name)
                    )
                },
                |reason| {
                    format!(
                        "{foundation}::HistoryGeneratedStrategy::new_with_unsupported({}, {variants_name}, {encode_name}, {})",
                        quote_rust_string(&def.name),
                        quote_rust_string(reason)
                    )
                },
            );
            let codec_method = format!(
                "    fn history_commands_to_data_tree(commands: &[Self]) -> Option<jet_std::DataTree> {{ Some({encode_name}(commands)) }}\n"
            );
            let (provenance_source, provenance_tool, provenance_target) =
                self.history_base_provenance();
            let provenance_method = format!(
                "    fn history_provenance() -> Option<{foundation}::HistoryProvenance> {{\n        Some({foundation}::HistoryProvenance {{ source: {}.to_string(), tool: {}.to_string(), target: {}.to_string() }})\n    }}\n",
                quote_rust_string(&provenance_source),
                quote_rust_string(&provenance_tool),
                quote_rust_string(&provenance_target),
            );
            let _ = writeln!(
                out,
                "impl {foundation}::HistoryCommand for {command_ty} {{\n    type Strategy = {foundation}::HistoryGeneratedStrategy<{command_ty}>;\n    fn history_command_type() -> &'static str {{ {} }}\n    fn history_strategy() -> Self::Strategy {{\n        {strategy_constructor}\n    }}\n{codec_method}{provenance_method}}}\n",
                quote_rust_string(&def.name)
            );
            if self.config.target_kind == MirRustTarget::WebWasm {
                let dispatch_name = format!("__jet_history_dispatch_{suffix}");
                let _ = writeln!(
                    out,
                    "#[cfg(target_arch = \"wasm32\")]\nfn {dispatch_name}(root: &jet_std::DataTree) -> Result<jet_std::JetTestComparison, String> {{\n    crate::jet_testing_history_web::run_wire_with_strategy::<{command_ty}, {foundation}::HistoryGeneratedStrategy<{command_ty}>>(root, <{command_ty} as {foundation}::HistoryCommand>::history_strategy())\n}}\n"
                );
                registrations.push((def.name.clone(), dispatch_name));
            }
        }
        if self.config.target_kind == MirRustTarget::WebWasm && !registrations.is_empty() {
            out.push_str(
                "#[cfg(target_arch = \"wasm32\")]\n#[no_mangle]\npub extern \"C\" fn jet_testing_history_web_register_strategies() {\n",
            );
            for (name, dispatch) in &registrations {
                let _ = writeln!(
                    out,
                    "    crate::jet_testing_history_web::register_strategy({}, {dispatch});",
                    quote_rust_string(name)
                );
            }
            out.push_str("}\n\n");
        }
    }

    fn emit_period_anchor_impls(&self, out: &mut String) {
        for def in &self.program.types {
            if !self.module_selected(def.module) {
                continue;
            }
            let MirTypeDefKind::Enum { variants, .. } = &def.kind else {
                continue;
            };
            if variants.len() != 3 {
                continue;
            }
            let mut has_date = false;
            let mut has_local_date = false;
            let mut has_date_time = false;
            for variant in variants {
                let MirVariantPayload::Single(payload) = &variant.payload else {
                    has_date = false;
                    has_local_date = false;
                    has_date_time = false;
                    break;
                };
                let MirTypeKind::Apply { name: nominal, .. } = payload.kind() else {
                    has_date = false;
                    has_local_date = false;
                    has_date_time = false;
                    break;
                };
                match nominal.name.as_str() {
                    "Date" => has_date = true,
                    "LocalDate" => has_local_date = true,
                    "DateTime" => has_date_time = true,
                    _ => {
                        has_date = false;
                        has_local_date = false;
                        has_date_time = false;
                        break;
                    }
                }
            }
            if !(has_date && has_local_date && has_date_time) {
                continue;
            }

            let name = self.type_name(def.id);
            let trait_name = format!("{}JetPeriodAnchor", self.config.root_prefix);
            let _ = writeln!(out, "impl {trait_name} for {name} {{");
            let _ = writeln!(
                out,
                "    fn period_total_in(&self, period: &{}JetPeriod, unit: &String) -> f64 {{",
                self.config.root_prefix
            );
            let _ = writeln!(out, "        match self {{");
            for variant in variants {
                let MirVariantPayload::Single(payload) = &variant.payload else {
                    unreachable!("validated period anchor union variant payload");
                };
                let MirTypeKind::Apply { name: nominal, .. } = payload.kind() else {
                    unreachable!("validated period anchor union payload type");
                };
                let _ = writeln!(
                    out,
                    "            {name}::{}(anchor) => anchor.period_total_in(period, unit),",
                    mangle(&variant.name)
                );
                debug_assert!(matches!(
                    nominal.name.as_str(),
                    "Date" | "LocalDate" | "DateTime"
                ));
            }
            let _ = writeln!(out, "        }}");
            let _ = writeln!(out, "    }}");
            let _ = writeln!(out, "}}\n");
        }
    }

    fn emit_columnar_row_adapter(&self, def: &MirTypeDef, declared: &[MirField], out: &mut String) {
        let fields = declared
            .iter()
            .filter(|field| !field.computed)
            .collect::<Vec<_>>();
        let cell = mangle_path(&format!("{}_cell", def.name));
        let generic_args = if def.generic_params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                def.generic_params
                    .iter()
                    .map(|param| mangle(&param.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let impl_generics = if def.generic_params.is_empty() {
            String::new()
        } else {
            let params = def
                .generic_params
                .iter()
                .map(|param| {
                    let mut bounds = param
                        .bounds
                        .iter()
                        .map(|bound| self.trait_nominal_name(bound))
                        .collect::<Vec<_>>();
                    bounds.push("Clone".to_string());
                    bounds.push("core::fmt::Debug".to_string());
                    format!("{}: {}", mangle(&param.name), bounds.join(" + "))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("<{params}>")
        };
        let cell_ty = format!("{cell}{generic_args}");
        let row_ty = format!("{}{}", self.type_name(def.id), generic_args);
        let _ = writeln!(
            out,
            "#[derive(Clone, Debug)]\n#[allow(dead_code, non_camel_case_types)]\npub enum {cell}{generic_args} {{"
        );
        for field in &fields {
            let name = self.field_name(field.id);
            let _ = writeln!(out, "    {name}({}),", self.rust_type(&field.ty));
        }
        let _ = writeln!(out, "}}\n");
        let _ = writeln!(
            out,
            "#[allow(dead_code, unreachable_patterns)]\nimpl{impl_generics} {cell_ty} {{"
        );
        for field in &fields {
            let name = self.field_name(field.id);
            let accessor = format!("jet_col_{name}");
            let _ = writeln!(
                out,
                "    pub fn {accessor}(self) -> {} {{\n        match self {{\n            {cell_ty}::{name}(__value) => __value,\n            _ => unreachable!(\"columnar cell does not match its column\"),\n        }}\n    }}",
                self.rust_type(&field.ty)
            );
        }
        let _ = writeln!(out, "}}\n");
        let _ = writeln!(
            out,
            "impl{impl_generics} {}JetRow for {row_ty} {{",
            self.config.root_prefix
        );
        let _ = writeln!(out, "    type Cell = {cell_ty};");
        let _ = writeln!(
            out,
            "    fn jet_row_width() -> usize {{ {} }}",
            fields.len()
        );
        let _ = writeln!(out, "    fn jet_row_split(self) -> Vec<Self::Cell> {{");
        let _ = writeln!(out, "        vec![");
        for field in &fields {
            let name = self.field_name(field.id);
            let _ = writeln!(out, "            {cell_ty}::{name}(self.{name}),");
        }
        let _ = writeln!(out, "        ]\n    }}");
        let _ = writeln!(
            out,
            "    fn jet_row_join(__cells: Vec<Self::Cell>) -> Self {{\n        let mut __cells = __cells.into_iter();\n        Self {{"
        );
        for field in &fields {
            let name = self.field_name(field.id);
            let _ = writeln!(
                out,
                "            {name}: match __cells.next() {{\n                Some({cell_ty}::{name}(__value)) => __value,\n                _ => unreachable!(\"columnar row is narrower than its column set\"),\n            }},"
            );
        }
        let _ = writeln!(out, "        }}\n    }}\n}}\n");
    }

    fn field_visibility(&self, field: &MirField) -> &'static str {
        if field.public {
            "pub "
        } else if field.package_public {
            "pub(crate) "
        } else {
            ""
        }
    }

    fn emit_foreign(&self, foreign: &MirForeign, out: &mut String) {
        if matches!(
            foreign.callback_transport.as_deref(),
            Some("managed" | "managed-close" | "emit-task")
        ) {
            return;
        }
        if !self.module_selected(foreign.module_id) {
            panic!(
                "MIR foreign {:?} belongs to an unselected module",
                foreign.id
            );
        }
        if !self.target_applicable(foreign.target_applicability) {
            panic!(
                "MIR foreign {:?} is not applicable to the selected artifact target",
                foreign.id
            );
        }
        let abi = self.foreign_abi_name(&foreign.foreign_abi);
        let language = self.foreign_language_name(foreign.foreign_language);
        let _ = writeln!(
            out,
            "// jet-mir-foreign: id={} module={} language={} abi={} symbol={} path={:?} callback={:?} handle={:?} close={:?} undo={:?}",
            foreign.id.0,
            foreign.module,
            language,
            abi,
            foreign.symbol,
            foreign.path,
            foreign.callback,
            foreign.handle,
            foreign.close_function,
            foreign.undo_function
        );
        let mut params = foreign
            .params
            .iter()
            .map(|param| {
                format!(
                    "{}: {}",
                    mangle(&param.name),
                    self.foreign_param_type(foreign, param)
                )
            })
            .collect::<Vec<_>>();
        if foreign.callback_transport.as_deref() == Some("native-start") {
            params.push("__jet_callback_ctx: *mut core::ffi::c_void".to_string());
        }
        let params = params.join(", ");
        let ret = self.foreign_return_type(foreign);
        let rust_name = self.foreign_name(foreign.id);
        if rust_name != foreign.symbol {
            let _ = writeln!(out, "    #[link_name = {:?}]", foreign.symbol);
        }
        let _ = writeln!(out, "    fn {rust_name}({params}) -> {ret};\n}}\n");
    }
    fn emit_link_closure(&self, out: &mut String) {
        let mut seen = BTreeSet::new();
        for link in &self.artifact.links {
            self.emit_link_unit(*link, &mut seen, out);
        }
    }

    fn emit_link_unit(
        &self,
        id: jet_foundation::MIR::MirLinkUnitId,
        seen: &mut BTreeSet<jet_foundation::MIR::MirLinkUnitId>,
        out: &mut String,
    ) {
        if !seen.insert(id) {
            return;
        }
        let link = self
            .program
            .links
            .iter()
            .find(|link| link.id == id)
            .unwrap_or_else(|| panic!("MIR link unit ID {:?} has no row", id));
        if !self.target_applicable(link.target_applicability) {
            panic!(
                "MIR link unit {:?} is not applicable to the selected artifact target",
                link.id
            );
        }
        for dependency in &link.link_closure {
            self.emit_link_unit(*dependency, seen, out);
        }
        let _ = writeln!(
            out,
            "// jet-mir-link-unit: id={} crate={} cache={}",
            link.id.0, link.crate_spec, link.cache_identity
        );
        for directory in &link.dependency_dirs {
            let _ = writeln!(out, "// jet-mir-link-search-path: {directory}");
        }
        for artifact in &link.artifacts {
            match artifact.kind {
                MirLinkArtifactKind::Object
                | MirLinkArtifactKind::StaticLibrary
                | MirLinkArtifactKind::DynamicLibrary
                | MirLinkArtifactKind::Framework => {
                    let _ = writeln!(out, "#[link(name = {:?})] extern \"C\" {{}}", artifact.path);
                }
                MirLinkArtifactKind::GeneratedSource => {
                    let _ = writeln!(out, "// jet-mir-generated-link-source: {}", artifact.path);
                }
            }
        }
    }

    fn callback_ids(&self) -> BTreeSet<MirCallbackId> {
        let mut callbacks = BTreeSet::new();
        for function in &self.program.functions {
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                continue;
            }
            for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
                if let MirOperation::Semantic(MirSemanticOp::CCallback { callback, .. }) =
                    &instruction.operation
                {
                    callbacks.insert(*callback);
                }
            }
        }
        callbacks
    }
    fn callback_boundary_symbol(&self, callback_id: MirCallbackId) -> String {
        let mut route = None;
        for function in &self.program.functions {
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                continue;
            }
            for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
                if let MirOperation::Semantic(MirSemanticOp::CCallback { call, callback, .. }) =
                    &instruction.operation
                {
                    if *callback != callback_id {
                        continue;
                    }
                    match route {
                        Some(previous) if previous != *call => {
                            panic!(
                                "MIR callback {:?} uses multiple boundary Prelude routes",
                                callback_id
                            );
                        }
                        Some(_) => {}
                        None => route = Some(*call),
                    }
                }
            }
        }
        let call = route.unwrap_or_else(|| {
            panic!(
                "MIR callback {:?} has no boundary Prelude route",
                callback_id
            )
        });
        self.prelude_symbol(call)
    }

    fn emit_callback_trampolines(&self, out: &mut String) {
        let mut symbols = BTreeSet::new();
        for callback_id in self.callback_ids() {
            let callback = self
                .program
                .callbacks
                .iter()
                .find(|callback| callback.id == callback_id)
                .unwrap_or_else(|| panic!("MIR callback ID {:?} has no adapter row", callback_id));
            if !symbols.insert(callback.symbol.clone()) {
                panic!(
                    "MIR callback symbol {:?} is emitted more than once",
                    callback.symbol
                );
            }
            if callback.symbol.is_empty() {
                panic!(
                    "MIR callback {:?} has an empty exported symbol",
                    callback.id
                );
            }
            let target = self.function_row(callback.function);
            if !matches!(target.form, MirFunctionForm::TopLevel) {
                panic!(
                    "MIR callback {:?} target is not a top-level callable",
                    callback.id
                );
            }
            if !callback.managed && !target.capture_params.is_empty() {
                panic!(
                    "MIR callback {:?} target has captures not representable by its adapter row",
                    callback.id
                );
            }
            if !self.module_selected(target.module_id) || !self.selected_for_target(target) {
                panic!(
                    "MIR callback {:?} target is not selected for this artifact",
                    callback.id
                );
            }
            if callback.managed {
                if target.params.len() != callback.params.len()
                    || target
                        .params
                        .iter()
                        .zip(&callback.params)
                        .any(|(left, right)| !left.ty.same_checked_type(&right.ty))
                {
                    panic!("managed MIR callback adapter parameters disagree with target function");
                }
                match (&target.declared_return, &callback.return_type) {
                    (Some(target), Some(callback)) if !target.same_checked_type(callback) => {
                        panic!("managed MIR callback adapter return type disagrees with target function")
                    }
                    (Some(_), None) if !target.return_type.is_unit() => {
                        panic!("managed MIR callback adapter omits a non-unit return type")
                    }
                    (None, Some(_)) => {
                        panic!("managed MIR callback adapter adds a return type to a unit target")
                    }
                    (None, None) | (Some(_), Some(_)) | (Some(_), None) => {}
                }
                let payload = match callback.params.as_slice() {
                    [param] => match param.ty.kind() {
                        MirTypeKind::Apply { name, args }
                            if name.name == "FfiCallbackEvent" && args.len() == 1 =>
                        {
                            args[0].clone()
                        }
                        _ => panic!(
                            "managed MIR callback {:?} target is not FfiCallbackEvent<T>",
                            callback.id
                        ),
                    },
                    _ => panic!(
                        "managed MIR callback {:?} must carry exactly one event payload",
                        callback.id
                    ),
                };
                if callback.return_type.is_some() || !target.return_type.is_unit() {
                    panic!("managed MIR callback {:?} must return Unit", callback.id);
                }
                let payload = self.rust_type(&payload);
                let abi = self.foreign_abi_name(&callback.abi);
                let _ = writeln!(
                    out,
                    "#[no_mangle]\npub unsafe extern \"{abi}\" fn {}(__jet_callback_ctx: *mut core::ffi::c_void, __jet_callback_value: {payload}) {{ let _ = {}jet_std::jet_ffi_callback_registration_invoke_i64(__jet_callback_ctx, __jet_callback_value as i64); }}\n",
                    callback.symbol,
                    self.config.root_prefix,
                );
                continue;
            }
            if target.params.len() != callback.params.len()
                || target
                    .params
                    .iter()
                    .zip(&callback.params)
                    .any(|(left, right)| !left.ty.same_checked_type(&right.ty))
            {
                panic!("MIR callback adapter parameters disagree with target function");
            }
            match (&target.declared_return, &callback.return_type) {
                (Some(target), Some(callback)) if !target.same_checked_type(callback) => {
                    panic!("MIR callback adapter return type disagrees with target function")
                }
                (Some(_), None) if !target.return_type.is_unit() => {
                    panic!("MIR callback adapter omits a non-unit return type")
                }
                (None, Some(_)) => {
                    panic!("MIR callback adapter adds a return type to a unit target")
                }
                (None, None) | (Some(_), Some(_)) | (Some(_), None) => {}
            }
            let params = callback
                .params
                .iter()
                .map(|param| format!("{}: {}", mangle(&param.name), self.parameter_type(param)))
                .collect::<Vec<_>>()
                .join(", ");
            let args = callback
                .params
                .iter()
                .map(|param| mangle(&param.name))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = callback
                .return_type
                .as_ref()
                .map(|ty| format!(" -> {}", self.rust_type(ty)))
                .unwrap_or_default();
            let target_call = self.function_name(callback.function);
            let target_call = if target.is_unsafe {
                format!("unsafe {{ {target_call}({args}) }}")
            } else {
                format!("{target_call}({args})")
            };
            let boundary = self.callback_boundary_symbol(callback_id);
            let abi = self.foreign_abi_name(&callback.abi);
            let _ = writeln!(
                out,
                "#[no_mangle]\npub extern \"{abi}\" fn {}({params}){ret} {{ {boundary}(|| {target_call}) }}\n",
                callback.symbol,
            );
        }
    }

    fn c_callback(
        &self,
        outer: &MirFunction,
        call: MirPreludeCallId,
        callback: MirCallbackId,
        lambda: MirValueId,
    ) -> String {
        let _ = self.prelude_row(call);
        let row = self
            .program
            .callbacks
            .iter()
            .find(|row| row.id == callback)
            .unwrap_or_else(|| panic!("MIR callback ID {:?} has no adapter row", callback));
        let (lambda_target, captures) = outer
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if instruction.result != Some(lambda) {
                    return None;
                }
                match &instruction.operation {
                    MirOperation::Closure {
                        function, captures, ..
                    } => Some((*function, captures.clone())),
                    _ => panic!("MIR C callback lambda is not a Closure operation"),
                }
            })
            .unwrap_or_else(|| panic!("MIR C callback lambda {:?} has no producer", lambda));
        if lambda_target != row.function {
            panic!(
                "MIR C callback {:?} lambda target {:?} disagrees with adapter target {:?}",
                callback, lambda_target, row.function
            );
        }
        let target = self.function_row(row.function);
        if !matches!(&target.form, MirFunctionForm::TopLevel) {
            panic!("MIR C callback target is not a top-level callable");
        }
        if !row.managed && !target.capture_params.is_empty() {
            panic!("MIR C callback target has captures not representable by its adapter");
        }
        if target.params.len() != row.params.len()
            || target
                .params
                .iter()
                .zip(&row.params)
                .any(|(left, right)| !left.ty.same_checked_type(&right.ty))
        {
            panic!("MIR C callback adapter parameters disagree with target function");
        }
        match (&target.declared_return, &row.return_type) {
            (Some(target), Some(callback)) if !target.same_checked_type(callback) => {
                panic!("MIR C callback adapter return type disagrees with target function")
            }
            (Some(_), None) if !target.return_type.is_unit() => {
                panic!("MIR C callback adapter omits a non-unit return type")
            }
            (None, Some(_)) => {
                panic!("MIR C callback adapter adds a return type to a unit target")
            }
            (None, None) | (Some(_), Some(_)) | (Some(_), None) => {}
        }
        if row.managed {
            if captures.len() != target.capture_params.len() {
                panic!(
                    "managed MIR callback {:?} captures {} values, expected {}",
                    callback,
                    captures.len(),
                    target.capture_params.len()
                );
            }
            let mut setup = String::new();
            let mut call_args = Vec::with_capacity(captures.len() + target.params.len());
            for (index, (operand, capture)) in
                captures.iter().zip(&target.capture_params).enumerate()
            {
                if capture.slot != index || capture.access != MirAccess::Read {
                    panic!("managed MIR callback captures must be ordered read captures");
                }
                let captured = match operand {
                    MirCaptureOperand::Place(place) => {
                        self.place_reference(outer, *place, MirAccess::Read)
                    }
                    MirCaptureOperand::Value(_) => {
                        panic!("managed MIR callback read capture is not a place")
                    }
                };
                let name = self.capture_param_name(capture.slot);
                let _ = writeln!(setup, "let {name} = ({captured}).clone();");
                let call_arg = format!("&{name}");
                call_args.push(call_arg);
            }
            let event_param = target
                .params
                .first()
                .unwrap_or_else(|| panic!("managed MIR callback target has no event parameter"));
            let event_name = "__jet_callback_event_value";
            let event_arg = if self.parameter_borrowed(event_param) {
                format!("&{event_name}")
            } else {
                event_name.to_string()
            };
            call_args.push(event_arg);
            let invocation = format!(
                "{}({})",
                self.function_name(row.function),
                call_args.join(", ")
            );
            let invocation = if target.is_unsafe {
                format!("unsafe {{ {invocation} }}")
            } else {
                invocation
            };
            return format!(
                "{{ {setup}std::sync::Arc::new(move |__jet_callback_event| {{ let {event_name} = {}jet_std::JetFfiCallbackEventValue::new((*__jet_callback_event.value()).clone()); {invocation}; Ok(()) }}) }}",
                self.config.root_prefix,
            );
        }
        let _abi = self.foreign_abi_name(&row.abi);
        row.symbol.clone()
    }

    fn emit_exports(&self, out: &mut String) {
        if self.config.target_kind == MirRustTarget::WebWasm {
            self.emit_web_exports(out);
            return;
        }
        let mut symbols = BTreeSet::new();
        for export in &self.artifact.exports {
            let function = self.function_row(export.function);
            if export.symbol.is_empty() {
                panic!(
                    "MIR export for function {:?} has an empty symbol",
                    export.function
                );
            }
            if self.callback_ids().iter().any(|callback_id| {
                self.program
                    .callbacks
                    .iter()
                    .any(|callback| callback.id == *callback_id && callback.symbol == export.symbol)
            }) {
                panic!(
                    "MIR export symbol {:?} collides with a callback trampoline",
                    export.symbol
                );
            }
            if !symbols.insert(export.symbol.clone()) {
                panic!(
                    "MIR export symbol {:?} is emitted more than once",
                    export.symbol
                );
            }
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!(
                    "MIR export {:?} targets an unselected function",
                    export.symbol
                );
            }
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
            {
                panic!(
                    "MIR export {:?} targets a non-exportable function",
                    export.symbol
                );
            }
            let params = function
                .params
                .iter()
                .map(|param| format!("{}: {}", mangle(&param.name), self.parameter_type(param)))
                .collect::<Vec<_>>()
                .join(", ");
            let args = function
                .params
                .iter()
                .map(|param| mangle(&param.name))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = self.rust_type(&function.return_type);
            let call = self.function_name(function.id);
            let call = if function.is_unsafe {
                format!("unsafe {{ {call}({args}) }}")
            } else {
                format!("{call}({args})")
            };
            let abi = self.foreign_abi_name(&export.abi);
            let _ = writeln!(
                out,
                "#[no_mangle]\npub extern \"{abi}\" fn {}({params}) -> {ret} {{ {call} }}\n",
                export.symbol
            );
        }
    }

    fn web_abi_type(&self, ty: &MirType) -> (&'static str, Option<&'static str>) {
        if ty.is_unit() {
            return ("()", None);
        }
        match ty.kind() {
            MirTypeKind::Tagged { inner, .. } => self.web_abi_type(inner),
            MirTypeKind::InlineRange { base, .. } | MirTypeKind::Quantity { base, .. } => {
                self.web_abi_type(base)
            }
            MirTypeKind::Int => ("u64", Some("int")),
            MirTypeKind::String => ("u64", Some("string")),
            MirTypeKind::Float => ("f64", None),
            MirTypeKind::Float32 => ("f32", None),
            MirTypeKind::Bool => ("i32", None),
            MirTypeKind::Char => ("u32", None),
            MirTypeKind::IntN {
                bits: 8,
                signed: true,
            } => ("i8", None),
            MirTypeKind::IntN {
                bits: 8,
                signed: false,
            } => ("u8", None),
            MirTypeKind::IntN {
                bits: 16,
                signed: true,
            } => ("i16", None),
            MirTypeKind::IntN {
                bits: 16,
                signed: false,
            } => ("u16", None),
            MirTypeKind::IntN {
                bits: 32,
                signed: true,
            } => ("i32", None),
            MirTypeKind::IntN {
                bits: 32,
                signed: false,
            } => ("u32", None),
            MirTypeKind::IntN {
                bits: 64,
                signed: true,
            } => ("i64", None),
            MirTypeKind::IntN {
                bits: 64,
                signed: false,
            } => ("u64", None),
            MirTypeKind::List(inner) => match self.web_abi_type(inner) {
                (_, Some("int")) => ("u64", Some("list_int")),
                (_, Some("string")) => ("u64", Some("list_string")),
                ("i64", None) => ("u64", Some("list_i64")),
                _ => panic!("checked Web list has no ABI rail: {ty:?}"),
            },
            MirTypeKind::Map { key, value, .. }
                if self.web_abi_type(key) == ("u64", Some("string"))
                    && self.web_abi_type(value) == ("u64", Some("int")) =>
            {
                ("u64", Some("map_string_int"))
            }
            _ => panic!("checked Web export has no ABI rail: {ty:?}"),
        }
    }

    fn web_abi_decode(&self, ty: &MirType, value: &str) -> String {
        if let Some(rail) = self.web_abi_type(ty).1 {
            return format!("jet_abi_{rail}_arg({value})");
        }
        match ty.kind() {
            MirTypeKind::Tagged { inner, .. } => self.web_abi_decode(inner, value),
            MirTypeKind::InlineRange { base, .. } | MirTypeKind::Quantity { base, .. } => {
                self.web_abi_decode(base, value)
            }
            MirTypeKind::Bool => format!("({value} != 0)"),
            MirTypeKind::Char => format!("char::from_u32({value}).expect(\"Web Char scalar\")"),
            _ => value.to_string(),
        }
    }

    fn web_abi_encode(&self, ty: &MirType, value: &str) -> String {
        if let Some(rail) = self.web_abi_type(ty).1 {
            return format!("jet_abi_{rail}_ret({value})");
        }
        match ty.kind() {
            MirTypeKind::Tagged { inner, .. } => self.web_abi_encode(inner, value),
            MirTypeKind::InlineRange { base, .. } | MirTypeKind::Quantity { base, .. } => {
                self.web_abi_encode(base, value)
            }
            MirTypeKind::Bool => format!("if {value} {{ 1 }} else {{ 0 }}"),
            MirTypeKind::Char => format!("({value} as u32)"),
            _ => value.to_string(),
        }
    }

    fn emit_web_time_zones(out: &mut String) -> std::io::Result<()> {
        fn visit(
            root: &std::path::Path,
            dir: &std::path::Path,
            out: &mut String,
        ) -> std::io::Result<()> {
            let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    visit(root, &path, out)?;
                } else if kind.is_file() {
                    let bytes = std::fs::read(&path)?;
                    if !bytes.starts_with(b"TZif") {
                        continue;
                    }
                    let relative = path.strip_prefix(root).expect("timezone path below root");
                    let name = relative.to_str().ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "non-UTF-8 timezone name",
                        )
                    })?;
                    let name = if std::path::MAIN_SEPARATOR == '/' {
                        std::borrow::Cow::Borrowed(name)
                    } else {
                        std::borrow::Cow::Owned(name.replace(std::path::MAIN_SEPARATOR, "/"))
                    };
                    write!(out, "{name:?} => Some(b\"").unwrap();
                    for byte in bytes {
                        write!(out, "\\x{byte:02x}").unwrap();
                    }
                    out.push_str("\"),\n");
                }
            }
            Ok(())
        }
        out.push_str(
            "\nfn jet_time_embedded_zone_bytes(name: &str) -> Option<&'static [u8]> {\n\
             let name = name.strip_prefix(\"posix/\").unwrap_or(name);\n\
             let name = name.strip_prefix(\"right/\").unwrap_or(name);\n\
             match name {\n",
        );
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corelib/tzdb");
        visit(&root, &root, out)?;
        out.push_str("_ => None,\n}\n}\n");
        Ok(())
    }

    fn emit_web_exports(&self, out: &mut String) {
        let exports = self
            .program
            .functions
            .iter()
            .filter(|function| {
                self.module_selected(function.module_id)
                    && self.selected_for_target(function)
                    && function.web_marker
                        == Some(jet_foundation::WebPartition::WebPartitionMarker::WasmExport)
            })
            .collect::<Vec<_>>();
        out.push_str(include_str!("../Prelude/Core/WebAbi.rs"));
        Self::emit_web_time_zones(out)
            .unwrap_or_else(|error| panic!("cannot embed checked Web timezone data: {error}"));
        for function in exports {
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
            {
                panic!("checked Web export is not a top-level callable");
            }
            let mut params = Vec::new();
            let mut decode = String::new();
            let mut args = Vec::new();
            for param in &function.params {
                let name = mangle(&param.name);
                if let Some(reconstruction) = function
                    .web_param_reconstructions
                    .iter()
                    .find(|reconstruction| reconstruction.local == param.name)
                {
                    let mut fields = Vec::new();
                    for field in &reconstruction.fields {
                        let flat = mangle(&field.parameter);
                        params.push(format!("{flat}: {}", self.web_abi_type(&field.ty).0));
                        fields.push(format!(
                            "{}: {}",
                            mangle(&field.field),
                            self.web_abi_decode(&field.ty, &flat),
                        ));
                    }
                    let _ = writeln!(
                        decode,
                        "    let mut {name} = {} {{ {} }};",
                        self.rust_type(&reconstruction.ty),
                        fields.join(", "),
                    );
                } else {
                    params.push(format!("{name}: {}", self.web_abi_type(&param.ty).0));
                    let _ = writeln!(
                        decode,
                        "    let mut {name} = {};",
                        self.web_abi_decode(&param.ty, &name),
                    );
                }
                args.push(if param.access == MirAccess::Write {
                    format!("&mut {name}")
                } else if self.parameter_borrowed(param) {
                    format!("&{name}")
                } else {
                    name
                });
            }
            let call = format!("{}({})", self.function_name(function.id), args.join(", "));
            let call = if function.is_unsafe {
                format!("unsafe {{ {call} }}")
            } else {
                call
            };
            let success = match &function.failure {
                MirFailureCarrier::Result { success, .. } => success,
                MirFailureCarrier::Optional { value } => value,
                MirFailureCarrier::Infallible | MirFailureCarrier::Diverges { .. } => {
                    &function.return_type
                }
            };
            let ret = self.web_abi_type(success).0;
            let encoded = self.web_abi_encode(success, "__jet_value");
            let body = match &function.failure {
                MirFailureCarrier::Result { .. } => format!(
                    "match {call} {{ Ok(__jet_value) => {encoded}, Err(__jet_error) => {{ jet_wasm_store_error(&__jet_error); Default::default() }} }}"
                ),
                MirFailureCarrier::Optional { .. } => format!(
                    "match {call} {{ Ok(__jet_value) => {encoded}, Err(_) => {{ jet_wasm_store_absent(); Default::default() }} }}"
                ),
                MirFailureCarrier::Infallible | MirFailureCarrier::Diverges { .. } => {
                    format!("let __jet_value = {call};\n    {encoded}")
                }
            };
            let _ = writeln!(
                out,
                "#[export_name = {:?}]\npub extern \"C\" fn __jet_web_export_{}({}) -> {ret} {{\n    jet_wasm_error_clear();\n{decode}    {body}\n}}\n",
                format!("jet_export_{}", function.name),
                function.id.0,
                params.join(", "),
            );
        }
    }

    fn emit_cli_input_metadata(&self, input: &MirCliInput, out: &mut String) {
        let shape = match &input.shape {
            MirCliInputShape::Flag => "flag".to_string(),
            MirCliInputShape::Value {
                kind,
                optional,
                default,
            } => format!("value:{kind:?}:optional={optional}:default={default:?}"),
        };
        let _ = writeln!(
            out,
            "// jet-mir-cli-input: parameter={} name={} label={} type={} zone={:?} short={:?} env={:?} metavar={:?} positional={:?} variadic={} shape={}",
            input.parameter,
            input.name,
            input.label,
            self.rust_type(&input.ty),
            input.zone,
            input.short,
            input.env,
            input.metavar,
            input.positional,
            input.variadic,
            shape
        );
    }
    fn cli_value_kind(&self, kind: MirCliValueKind) -> &'static str {
        match kind {
            MirCliValueKind::Bool | MirCliValueKind::String | MirCliValueKind::Path => "String",
            MirCliValueKind::Int => "Int",
            MirCliValueKind::Float => "Float",
        }
    }

    fn cli_default_expr(&self, default: &Option<MirCliDefault>, ty: &MirType) -> String {
        match default {
            None => "None".to_string(),
            Some(MirCliDefault::TypeDefault) => {
                format!(
                    "Some((Default::default() as {}).jet_show())",
                    self.rust_type(ty)
                )
            }
            Some(MirCliDefault::Value(value)) => {
                format!("Some(({}).jet_show())", self.constant(value))
            }
        }
    }

    fn cli_spec_expr(
        &self,
        inputs: &[MirCliInput],
        description: Option<&str>,
        standard: bool,
        version: Option<&str>,
        program: &str,
    ) -> String {
        let root = &self.config.root_prefix;
        let mut spec = format!("{root}jet_args_program({root}jet_args_spec(), {program})");
        if let Some(description) = description {
            spec = format!("{root}jet_args_description({spec}, &{description:?}.to_string())");
        }
        for input in inputs {
            if input.name.is_empty() || input.help.is_empty() {
                panic!("MIR CLI input has no canonical name/help");
            }
            match &input.shape {
                MirCliInputShape::Flag => {
                    if !input.ty.is_bool() || input.variadic {
                        panic!("MIR CLI flag row is not a non-variadic Bool");
                    }
                    spec = match &input.short {
                        Some(short) => format!(
                            "{root}jet_args_flag_short({spec}, &{:?}.to_string(), &{:?}.to_string(), &{:?}.to_string())",
                            input.name, short, input.help
                        ),
                        None => format!(
                            "{root}jet_args_flag({spec}, &{:?}.to_string(), &{:?}.to_string())",
                            input.name, input.help
                        ),
                    };
                }
                MirCliInputShape::Value {
                    kind,
                    optional,
                    default,
                } => {
                    let metavar = input.metavar.as_deref().unwrap_or("VALUE");
                    let default = self.cli_default_expr(default, &input.ty);
                    let required = !optional && default == "None" && input.positional.is_none();
                    let value_kind = self.cli_value_kind(*kind);
                    let short = input
                        .short
                        .as_ref()
                        .map(|short| format!("Some({short:?}.to_string())"))
                        .unwrap_or_else(|| "None".to_string());
                    let env = input
                        .env
                        .as_ref()
                        .map(|env| format!("Some({env:?}.to_string())"))
                        .unwrap_or_else(|| "None".to_string());
                    spec = format!(
                        "{root}jet_args_option_base({spec}, &{:?}.to_string(), {short}, &{:?}.to_string(), &{:?}.to_string(), {default}, {env}, {required}, {}, {root}JetArgValueKind::{value_kind})",
                        input.name,
                        input.help,
                        metavar,
                        input.variadic
                    );
                    if input.positional.is_some() {
                        spec = format!(
                            "{root}jet_args_positional({spec}, &{:?}.to_string(), &{:?}.to_string())",
                            input.name, input.help
                        );
                    }
                }
            }
        }
        if standard {
            spec = format!(
                "{root}jet_args_flag_short({spec}, &\"verbose\".to_string(), &\"v\".to_string(), &\"print extra detail\".to_string())"
            );
            spec = format!(
                "{root}jet_args_flag_short({spec}, &\"quiet\".to_string(), &\"q\".to_string(), &\"suppress normal output\".to_string())"
            );
            spec = format!(
                "{root}jet_args_option_choice({spec}, &\"color\".to_string(), &\"control terminal color\".to_string(), &\"MODE\".to_string(), &\"auto,always,never\".to_string())"
            );
            if let Some(version) = version {
                spec = format!("{root}jet_args_version({spec}, &{version:?}.to_string())");
            }
        }
        spec
    }

    fn cli_scalar_expr(&self, input: &MirCliInput, raw: &str) -> String {
        let root = &self.config.root_prefix;
        let MirCliInputShape::Value { kind, .. } = &input.shape else {
            panic!("MIR CLI scalar decoder received a flag row");
        };
        match kind {
            MirCliValueKind::Bool => format!(
                "match {raw}.to_ascii_lowercase().as_str() {{ \"true\" => true, \"false\" => false, _ => return Err(format!(\"invalid value for --{{}}: `{{}}` is not true or false\\n\\n{{}}\", {:?}, {raw}, __spec.help())) }}",
                input.name
            ),
            MirCliValueKind::Int => match input.ty.kind() {
                MirTypeKind::Int => format!(
                    "match {root}jet_std::jet_int_owned_from_str(&{raw}) {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a whole number\\n\\n{{}}\", {:?}, {raw}, __spec.help())) }}",
                    input.name
                ),
                MirTypeKind::IntN { .. } => format!(
                    "(match {root}jet_std::jet_int_from_str(&{raw}) {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a whole number\\n\\n{{}}\", {:?}, {raw}, __spec.help())) }}) as {}",
                    input.name,
                    self.rust_type(&input.ty)
                ),
                MirTypeKind::InlineRange { lo, hi, .. } => format!(
                    "match {root}jet_inline_range_from_int(match {raw}.parse::<i64>() {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a whole number\\n\\n{{}}\", {:?}, {raw}, __spec.help())) }}, {lo}, {hi}) {{ Ok(__n) => __n, Err(__reason) => return Err(format!(\"invalid value for --{{}}: {{}}\\n\\n{{}}\", {:?}, __reason, __spec.help())) }}",
                    input.name,
                    input.name
                ),
                MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
                    let mut nested = input.clone();
                    nested.ty = (**inner).clone();
                    self.cli_scalar_expr(&nested, raw)
                }
                MirTypeKind::Float
                | MirTypeKind::Bool
                | MirTypeKind::String
                | MirTypeKind::Char
                | MirTypeKind::List(_)
                | MirTypeKind::Map { .. }
                | MirTypeKind::Shared(_)
                | MirTypeKind::Option(_)
                | MirTypeKind::Result { .. }
                | MirTypeKind::Fn(_)
                | MirTypeKind::SendFn { .. }
                | MirTypeKind::Apply { .. }
                | MirTypeKind::TraitObject(_)
                | MirTypeKind::Tuple(_)
                | MirTypeKind::FixedList { .. }
                | MirTypeKind::Float32
                | MirTypeKind::Union(_)
                | MirTypeKind::Measure(_) => {
                    panic!("MIR CLI Int row has a non-integer checked type")
                }
            },
            MirCliValueKind::Float => match input.ty.kind() {
                MirTypeKind::Float => format!(
                    "match {raw}.parse::<f64>() {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a number\\n\\n{{}}\", {:?}, {raw}, __spec.help())) }}",
                    input.name
                ),
                MirTypeKind::Float32 => format!(
                    "(match {raw}.parse::<f64>() {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a number\\n\\n{{}}\", {:?}, {raw}, __spec.help())) }}) as f32",
                    input.name
                ),
                MirTypeKind::Int
                | MirTypeKind::IntN { .. }
                | MirTypeKind::Bool
                | MirTypeKind::String
                | MirTypeKind::Char
                | MirTypeKind::List(_)
                | MirTypeKind::Map { .. }
                | MirTypeKind::Shared(_)
                | MirTypeKind::Option(_)
                | MirTypeKind::Result { .. }
                | MirTypeKind::Fn(_)
                | MirTypeKind::SendFn { .. }
                | MirTypeKind::Apply { .. }
                | MirTypeKind::TraitObject(_)
                | MirTypeKind::Tuple(_)
                | MirTypeKind::FixedList { .. }
                | MirTypeKind::InlineRange { .. }
                | MirTypeKind::Tagged { .. }
                | MirTypeKind::Quantity { .. }
                | MirTypeKind::Union(_)
                | MirTypeKind::Measure(_) => {
                    panic!("MIR CLI Float row has a non-floating checked type")
                }
            },
            MirCliValueKind::String => {
                if !matches!(input.ty.kind(), MirTypeKind::String) {
                    panic!("MIR CLI String row has a non-String checked type");
                }
                format!("{raw}.clone()")
            }
            MirCliValueKind::Path => {
                if input.ty.nominal_name() != Some("Path") {
                    panic!("MIR CLI Path row has a non-Path checked type");
                }
                format!("{root}jet_path_from(&{raw})")
            }
        }
    }

    fn cli_scalar_result_expr(&self, input: &MirCliInput, raw: &str) -> String {
        format!("Ok({})", self.cli_scalar_expr(input, raw))
    }

    fn cli_decode_lines(&self, inputs: &[MirCliInput], indent: usize) -> String {
        let root = &self.config.root_prefix;
        let pad = " ".repeat(indent);
        let mut lines = String::new();
        for input in inputs {
            let variable = format!("__jet_cli_{}", input.parameter);
            match &input.shape {
                MirCliInputShape::Flag => {
                    if !input.ty.is_bool() {
                        panic!("MIR CLI flag decoder has a non-Bool type");
                    }
                    let _ = writeln!(
                        lines,
                        "{pad}let {variable}: bool = {root}jet_parsed_flag(&__parsed, &{:?}.to_string());",
                        input.name
                    );
                }
                MirCliInputShape::Value {
                    optional, default, ..
                } if input.variadic => {
                    let MirTypeKind::List(inner) = input.ty.kind() else {
                        panic!("MIR variadic CLI input must have a List type");
                    };
                    let element_ty = self.rust_type(inner);
                    let mut scalar_input = input.clone();
                    scalar_input.ty = (**inner).clone();
                    let result = self.cli_scalar_result_expr(&scalar_input, "__v");
                    let _ = writeln!(
                        lines,
                        "{pad}let {variable}: Vec<{element_ty}> = match {root}jet_parsed_options(&__parsed, &{:?}.to_string()).into_iter().map(|__v| {result}).collect::<Result<Vec<_>, String>>() {{ Ok(__v) => __v, Err(__e) => return Err(__e) }};",
                        input.name
                    );
                    let _ = (optional, default);
                }
                MirCliInputShape::Value { optional: true, .. } => {
                    let MirTypeKind::Option(inner) = input.ty.kind() else {
                        panic!("MIR optional CLI input must have an Option type");
                    };
                    let inner_ty = self.rust_type(inner);
                    let mut scalar_input = input.clone();
                    scalar_input.ty = (**inner).clone();
                    let value = self.cli_scalar_expr(&scalar_input, "__v");
                    let _ = writeln!(
                        lines,
                        "{pad}let {variable}: {root}JetOutcome<{inner_ty}, {root}JetAbsent> = match {root}jet_parsed_option(&__parsed, &{:?}.to_string()) {{ Ok(__v) => Ok({value}), Err({root}JetAbsent) => Err({root}JetAbsent) }};",
                        input.name
                    );
                }
                MirCliInputShape::Value {
                    optional: false,
                    default,
                    ..
                } => {
                    let value = self.cli_scalar_expr(input, "__v");
                    let fallback = match default {
                        None => format!(
                            "return Err(format!(\"missing required argument --{{}}\\n\\n{{}}\", {:?}, __spec.help()))",
                            input.name
                        ),
                        Some(MirCliDefault::TypeDefault) => "Default::default()".to_string(),
                        Some(MirCliDefault::Value(value)) => self.constant(value),
                    };
                    let ty = self.rust_type(&input.ty);
                    let _ = writeln!(
                        lines,
                        "{pad}let {variable}: {ty} = match {root}jet_parsed_option(&__parsed, &{:?}.to_string()) {{ Ok(__v) => {value}, Err(_) => {fallback} }};",
                        input.name
                    );
                }
            }
        }
        lines
    }
    fn cli_decode_and_invoke(
        &self,
        function: &MirFunction,
        inputs: &[MirCliInput],
        output: MirEntryOutput,
        serves_until_stopped: bool,
        service: bool,
        indent: &str,
    ) -> String {
        if !matches!(function.form, MirFunctionForm::TopLevel) {
            panic!(
                "MIR CLI entry function {:?} is not a top-level callable",
                function.id
            );
        }
        if !function.capture_params.is_empty() {
            panic!("MIR CLI entry function {:?} has captures", function.id);
        }
        let mut seen_parameters = BTreeSet::new();
        for input in inputs {
            if !seen_parameters.insert(input.parameter) {
                panic!("MIR CLI input parameter {} is duplicated", input.parameter);
            }
        }
        for input in inputs {
            let parameter = function
                .params
                .iter()
                .find(|parameter| parameter.index == input.parameter)
                .unwrap_or_else(|| {
                    panic!(
                        "MIR CLI input parameter {} is absent from function {:?}",
                        input.parameter, function.id
                    )
                });
            if !parameter.ty.same_checked_type(&input.ty) {
                panic!(
                    "MIR CLI input parameter {} type disagrees with function {:?}",
                    input.parameter, function.id
                );
            }
        }
        for parameter in &function.params {
            if !seen_parameters.contains(&parameter.index) {
                panic!(
                    "MIR CLI entry function {:?} has no input row for parameter {}",
                    function.id, parameter.index
                );
            }
        }
        let args = function
            .params
            .iter()
            .map(|parameter| self.cli_argument(parameter))
            .collect::<Vec<_>>()
            .join(", ");
        let call = format!("{}({args})", self.function_name(function.id));
        let pad = " ".repeat(indent.len() + 4);
        let decode = self.cli_decode_lines(inputs, indent.len() + 4);
        let invoke =
            self.entry_invoke(function, &call, output, serves_until_stopped, service, &pad);
        format!(
            "{indent}let __jet_cli_result = (|| -> Result<(), String> {{\n{decode}{invoke}{pad}Ok(())\n{indent}}})();\n{indent}if let Err(__jet_error) = __jet_cli_result {{ eprint!(\"{{}}\", {root}jet_cli_banner(&__jet_error)); std::process::exit(2); }}\n",
            root = self.config.root_prefix,
        )
    }

    fn cli_argument(&self, parameter: &MirParam) -> String {
        let variable = format!("__jet_cli_{}", parameter.index);
        match parameter.access {
            MirAccess::Write => format!("&mut {variable}"),
            MirAccess::Read if self.is_scalar(&parameter.ty) => variable,
            MirAccess::Read => format!("&{variable}"),
            MirAccess::Move => variable,
        }
    }

    fn entry_error_exit(&self, function: &MirFunction, error: &str, service: bool) -> String {
        let function = match &function.failure {
            MirFailureCarrier::Result { error: ty, .. } => Some(ty),
            MirFailureCarrier::Optional { .. } => None,
            MirFailureCarrier::Diverges { .. } => {
                panic!("MIR entry error edge requested for a diverging function")
            }
            MirFailureCarrier::Infallible => {
                panic!("MIR entry error edge requested for an infallible function")
            }
        };
        let root = &self.config.root_prefix;
        if self.config.target_kind == MirRustTarget::WebWasm {
            return if function
                .is_some_and(|ty| ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_ERR))
            {
                format!("{root}jet_wasm_store_error(&{error})")
            } else if function.is_none() {
                format!("{root}jet_wasm_store_error(&{root}jet_err_from_message(\"entry returned no value\".to_string()))")
            } else {
                format!("{root}jet_wasm_store_error(&{root}jet_err_from_message(format!(\"{{:?}}\", {error})))")
            };
        }
        if self.is_no_os() {
            return if function.is_none() {
                format!("{root}jet_entry_error_exit(\"entry returned no value\")")
            } else {
                format!("{root}jet_entry_error_exit({error})")
            };
        }

        let helper = if service {
            if function
                .is_some_and(|ty| ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_ERR))
            {
                "jet_service_edge_report_jet"
            } else {
                "jet_service_edge_report"
            }
        } else if function
            .is_some_and(|ty| ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_ERR))
        {
            "jet_entry_error_exit_jet"
        } else {
            "jet_entry_error_exit"
        };
        if helper.ends_with("_jet") {
            format!("{root}{helper}({error})")
        } else if function.is_none() {
            format!("{root}{helper}(\"entry returned no value\".to_string())")
        } else {
            format!("{root}{helper}(format!(\"{{:?}}\", {error}))")
        }
    }

    fn is_no_os(&self) -> bool {
        self.program
            .facts
            .target_dossier
            .machine
            .as_deref()
            .is_some_and(|machine| machine.no_os)
    }
    fn is_core_layer(&self) -> bool {
        self.program.facts.target_dossier.layer == jet_foundation::RingLayer::RuntimeLayer::Core
    }

    fn entry_output(&self, output: MirEntryOutput, value: &str, app: bool) -> String {
        if app {
            return format!("{value}.serve();");
        }
        match output {
            MirEntryOutput::None | MirEntryOutput::ReturnValue => format!("let _ = {value};"),
            MirEntryOutput::StandardOutput => format!("print!(\"{{}}\", {value});"),
            MirEntryOutput::ExitStatus => {
                if self.is_no_os() {
                    return format!(
                        "{}jet_target_exit(({value}) as i32);",
                        self.config.root_prefix
                    );
                }
                format!("std::process::exit(({value}) as i32);")
            }
        }
    }

    fn entry_invoke(
        &self,
        function: &MirFunction,
        call: &str,
        output: MirEntryOutput,
        serves_until_stopped: bool,
        service: bool,
        indent: &str,
    ) -> String {
        let app = serves_until_stopped
            && (function.return_type.nominal_name() == Some("App")
                || function
                    .return_type
                    .result_parts()
                    .is_some_and(|(ok, _)| ok.nominal_name() == Some("App")));
        let action = match &function.failure {
            MirFailureCarrier::Diverges { .. } => call.to_string(),
            MirFailureCarrier::Infallible => self.entry_output(output, call, app),
            MirFailureCarrier::Result { .. } | MirFailureCarrier::Optional { .. } => {
                let error = self.entry_error_exit(function, "__jet_error", service);
                let value = self.entry_output(output, "__jet_value", app);
                format!(
                    "match {call} {{ Ok(__jet_value) => {{ {value} }}, Err(__jet_error) => {{ {error} }} }}"
                )
            }
        };
        format!(
            "{indent}{root}jet_runtime_boundary(|| {{\n{indent}    {action}\n{indent}}});\n",
            root = self.config.root_prefix,
        )
    }

    fn emit_hardware_facts(&self, out: &mut String) {
        let Some(profile) = self.program.facts.hardware_profile.as_ref() else {
            return;
        };
        let root = &self.config.root_prefix;
        for (block_index, block) in profile.register_blocks.iter().enumerate() {
            let _ = writeln!(
                out,
                "static __JET_HARDWARE_REGISTERS_{block_index}: &[{root}JetRegisterSpec<'static>] = &["
            );
            for register in &block.registers {
                let width = match register.width {
                    jet_foundation::TargetMachine::RegisterWidth::U8 => "U8",
                    jet_foundation::TargetMachine::RegisterWidth::U16 => "U16",
                    jet_foundation::TargetMachine::RegisterWidth::U32 => "U32",
                    jet_foundation::TargetMachine::RegisterWidth::U64 => "U64",
                };
                let access = match register.access {
                    jet_foundation::TargetMachine::TargetRegisterAccessMode::ReadOnly => "ReadOnly",
                    jet_foundation::TargetMachine::TargetRegisterAccessMode::WriteOnly => {
                        "WriteOnly"
                    }
                    jet_foundation::TargetMachine::TargetRegisterAccessMode::ReadWrite => {
                        "ReadWrite"
                    }
                };
                let _ = writeln!(
                    out,
                    "    {root}JetRegisterSpec::new({:?}, {}u64, {root}JetRegisterWidth::{width}, {root}JetRegisterAccessMode::{access}, {}),",
                    register.name,
                    register.offset,
                    register.volatile,
                );
            }
            out.push_str("];\n");
        }
        let _ = writeln!(
            out,
            "static __JET_HARDWARE_BLOCKS: &[{root}JetRegisterBlockSpec<'static>] = &["
        );
        for (block_index, block) in profile.register_blocks.iter().enumerate() {
            let _ = writeln!(
                out,
                "    {root}JetRegisterBlockSpec::new({:?}, {}u64, {}u64, __JET_HARDWARE_REGISTERS_{block_index}),",
                block.name,
                block.base,
                block.size.bytes,
            );
        }
        out.push_str("];\n");

        for (interrupt_index, interrupt) in profile.interrupts.iter().enumerate() {
            let _ = writeln!(
                out,
                "static __JET_HARDWARE_INTERRUPT_EFFECTS_{interrupt_index}: &[&'static str] = &["
            );
            for effect in &interrupt.forbidden_effects {
                let _ = writeln!(out, "    {:?},", effect);
            }
            out.push_str("];\n");
        }
        let _ = writeln!(
            out,
            "static __JET_HARDWARE_INTERRUPTS: &[{root}JetInterruptSpec<'static>] = &["
        );
        for (interrupt_index, interrupt) in profile.interrupts.iter().enumerate() {
            if interrupt.forbidden_effects.is_empty() {
                let _ = writeln!(
                    out,
                    "    {root}JetInterruptSpec::new({:?}, {}u16, {}),",
                    interrupt.name, interrupt.vector, interrupt.bounded,
                );
            } else {
                let _ = writeln!(
                    out,
                    "    {root}JetInterruptSpec::with_effects({:?}, {}u16, __JET_HARDWARE_INTERRUPT_EFFECTS_{interrupt_index}, {}),",
                    interrupt.name,
                    interrupt.vector,
                    interrupt.bounded,
                );
            }
        }
        out.push_str("];\n");

        let _ = writeln!(
            out,
            "static __JET_HARDWARE_DMA_CHANNELS: &[{root}JetDmaChannelSpec<'static>] = &["
        );
        for channel in &profile.dma_channels {
            let width = match channel.transfer_width {
                jet_foundation::TargetMachine::RegisterWidth::U8 => "U8",
                jet_foundation::TargetMachine::RegisterWidth::U16 => "U16",
                jet_foundation::TargetMachine::RegisterWidth::U32 => "U32",
                jet_foundation::TargetMachine::RegisterWidth::U64 => "U64",
            };
            let ownership = match channel.ownership {
                jet_foundation::TargetMachine::TargetDmaOwnership::Borrowed => "Borrowed",
                jet_foundation::TargetMachine::TargetDmaOwnership::Transfer => "Transfer",
            };
            let limit = channel
                .max_transfer_bytes
                .map(|bytes| format!("Some({bytes}u64)"))
                .unwrap_or_else(|| "None".to_string());
            let _ = writeln!(
                out,
                "    {root}JetDmaChannelSpec::with_transfer({:?}, {}u16, {root}JetRegisterWidth::{width}, {root}JetDmaOwnership::{ownership}, {limit}),",
                channel.name,
                channel.channel,
            );
        }
        out.push_str("];\n");

        let capabilities = self
            .program
            .facts
            .hardware_capabilities
            .iter()
            .map(|capability| format!("{capability:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let svd_source = profile
            .svd
            .as_ref()
            .map(|svd| format!("Some({:?})", svd.source))
            .unwrap_or_else(|| "None".to_string());
        let svd_sha256 = profile
            .svd
            .as_ref()
            .map(|svd| format!("Some({:?})", svd.sha256))
            .unwrap_or_else(|| "None".to_string());
        let _ = writeln!(
            out,
            "static __JET_HARDWARE_FACTS: {root}JetHardwareFacts<'static> = {root}JetHardwareFacts::with_profile({:?}, &[{capabilities}], {svd_source}, {svd_sha256}, __JET_HARDWARE_BLOCKS, __JET_HARDWARE_INTERRUPTS, __JET_HARDWARE_DMA_CHANNELS);",
            self.program.facts.hardware_profile_id,
        );
        out.push('\n');
        out.push_str("#[cfg(target_os = \"none\")]\n");
        let _ = writeln!(
            out,
            "static __JET_HARDWARE_VOLATILE_BACKEND: {root}JetVolatileRegisterBackend = {root}JetVolatileRegisterBackend;"
        );
        self.emit_hardware_register_helpers(out, profile);
        self.emit_hardware_interrupt_runtime(out);
        out.push('\n');
    }
    fn hardware_register_helper_name(
        &self,
        operation: &str,
        profile_id: &str,
        block: &str,
        register: &str,
    ) -> String {
        mangle_path(&format!(
            "hardware_register_{operation}.{profile_id}.{block}.{register}"
        ))
    }

    fn emit_hardware_register_helpers(
        &self,
        out: &mut String,
        profile: &jet_foundation::TargetMachine::TargetHardwareFacts,
    ) {
        let profile_id = self.program.facts.hardware_profile_id.as_str();
        for (block_index, block) in profile.register_blocks.iter().enumerate() {
            for (register_index, register) in block.registers.iter().enumerate() {
                if register.access.can_read() {
                    self.emit_hardware_register_helper(
                        out,
                        profile_id,
                        block_index,
                        register_index,
                        block,
                        register,
                        "read",
                    );
                }
                if register.access.can_write() {
                    self.emit_hardware_register_helper(
                        out,
                        profile_id,
                        block_index,
                        register_index,
                        block,
                        register,
                        "write",
                    );
                }
            }
        }
    }

    fn emit_hardware_register_helper(
        &self,
        out: &mut String,
        profile_id: &str,
        block_index: usize,
        _register_index: usize,
        block: &jet_foundation::TargetMachine::TargetRegisterBlockFact,
        register: &jet_foundation::TargetMachine::TargetRegisterFact,
        operation: &str,
    ) {
        let root = &self.config.root_prefix;
        let name =
            self.hardware_register_helper_name(operation, profile_id, &block.name, &register.name);
        let (value_type, width) = match register.width {
            jet_foundation::TargetMachine::RegisterWidth::U8 => ("u8", 1u64),
            jet_foundation::TargetMachine::RegisterWidth::U16 => ("u16", 2u64),
            jet_foundation::TargetMachine::RegisterWidth::U32 => ("u32", 4u64),
            jet_foundation::TargetMachine::RegisterWidth::U64 => ("u64", 8u64),
        };
        let access_type = match register.access {
            jet_foundation::TargetMachine::TargetRegisterAccessMode::ReadOnly => "JetReadOnly",
            jet_foundation::TargetMachine::TargetRegisterAccessMode::WriteOnly => "JetWriteOnly",
            jet_foundation::TargetMachine::TargetRegisterAccessMode::ReadWrite => "JetReadWrite",
        };
        let _ = writeln!(out, "#[inline]");
        if operation == "read" {
            let _ = writeln!(out, "fn {name}() -> i64 {{");
        } else {
            let _ = writeln!(out, "fn {name}(value: i64) -> i64 {{");
        }
        let _ = writeln!(out, "    #[cfg(target_os = \"none\")]");
        out.push_str("    {\n");
        let _ = writeln!(out, "        let __jet_block = {root}JetRegisterBlock::<{root}JetVolatileRegisterBackend>::new(");
        let _ = writeln!(
            out,
            "            &__JET_HARDWARE_VOLATILE_BACKEND, &__JET_HARDWARE_BLOCKS[{block_index}],"
        );
        out.push_str("        );\n");
        let _ = writeln!(
            out,
            "        let __jet_register = __jet_block.register::<{value_type}, {root}{access_type}, {}>({register:?})",
            register.volatile,
        );
        let _ = writeln!(
            out,
            "            .unwrap_or_else(|_| panic!(\"hardware register {}.{} has inconsistent generated facts\"));",
            block.name,
            register.name,
        );
        if operation == "read" {
            out.push_str("        __jet_register.read() as i64\n");
        } else {
            let _ = writeln!(
                out,
                "        __jet_register.write(<{value_type} as {root}JetRegisterValue>::from_bits(value as u64));"
            );
            out.push_str("        0\n");
        }
        out.push_str("    }\n");
        let _ = writeln!(out, "    #[cfg(not(target_os = \"none\"))]");
        out.push_str("    {\n");
        if operation == "read" {
            let _ = writeln!(
                out,
                "        {root}jet_hardware_register_read_typed({profile_id:?}, {block:?}, {register:?}, {width}i64)"
            );
        } else {
            let _ = writeln!(
                out,
                "        {root}jet_hardware_register_write_typed({profile_id:?}, {block:?}, {register:?}, {width}i64, value)"
            );
        }
        out.push_str("    }\n");
        out.push_str("}\n\n");
    }

    fn has_hardware_interrupts(&self) -> bool {
        self.program
            .facts
            .hardware_setups
            .iter()
            .any(|setup| matches!(setup, MirHardwareSetup::InterruptBind { .. }))
    }

    fn interrupt_handler_function(&self, handler_symbol: &str) -> &MirFunction {
        let mut functions = self
            .program
            .functions
            .iter()
            .filter(|function| function.key == handler_symbol);
        let function = functions.next().unwrap_or_else(|| {
            panic!("MIR interrupt handler `{handler_symbol}` has no function row")
        });
        if functions.next().is_some() {
            panic!("MIR interrupt handler `{handler_symbol}` resolves to multiple function rows");
        }
        if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
            panic!("MIR interrupt handler `{handler_symbol}` targets an unselected function");
        }
        if !matches!(&function.form, MirFunctionForm::TopLevel)
            || !function.generic_params.is_empty()
            || !function.capture_params.is_empty()
            || !function.params.is_empty()
            || !function.return_type.is_unit()
            || !matches!(&function.failure, MirFailureCarrier::Infallible)
            || function.generator.is_some()
            || function.is_unsafe
        {
            panic!(
                "MIR interrupt handler `{handler_symbol}` must have a capture-free zero-argument Unit ABI"
            );
        }
        function
    }

    fn emit_hardware_interrupt_runtime(&self, out: &mut String) {
        if !self.has_hardware_interrupts() {
            return;
        }
        let root = &self.config.root_prefix;
        for (setup_index, setup) in self.program.facts.hardware_setups.iter().enumerate() {
            let MirHardwareSetup::InterruptBind { handler_symbol, .. } = setup else {
                continue;
            };
            let function = self.interrupt_handler_function(handler_symbol);
            let handler = self.function_name(function.id);
            let _ = writeln!(
                out,
                "extern \"C\" fn __jet_hardware_interrupt_trampoline_{setup_index}() {{"
            );
            let _ = writeln!(out, "    {handler}();");
            out.push_str("}\n\n");
        }
        if self.is_no_os() {
            let _ = writeln!(
                out,
                "static mut __JET_HARDWARE_INTERRUPT_REGISTRY: {root}JetHardwareInterruptRegistry = {root}JetHardwareInterruptRegistry::new();"
            );
            out.push_str(
                "static __JET_HARDWARE_INTERRUPT_REGISTRY_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);\n",
            );
            let _ = writeln!(
                out,
                "fn __jet_hardware_registry() -> &'static {root}JetHardwareInterruptRegistry {{"
            );
            out.push_str(
                "    if !__JET_HARDWARE_INTERRUPT_REGISTRY_READY.swap(true, core::sync::atomic::Ordering::AcqRel) {\n",
            );
            out.push_str("        unsafe {\n");
            out.push_str(
                "            let __jet_hardware_registry = &mut *core::ptr::addr_of_mut!(__JET_HARDWARE_INTERRUPT_REGISTRY);\n",
            );
            for (setup_index, setup) in self.program.facts.hardware_setups.iter().enumerate() {
                let MirHardwareSetup::InterruptBind { vector, .. } = setup else {
                    continue;
                };
                let _ = writeln!(
                    out,
                    "            if !__jet_hardware_registry.register({vector}u16, {root}JetHardwareInterruptHandler {{ fn_ptr: __jet_hardware_interrupt_trampoline_{setup_index} as usize as i64, env: 0, has_env: false }}) {{"
                );
                out.push_str(
                    "                panic!(\"duplicate MIR hardware interrupt vector binding\");\n            }\n",
                );
            }
            out.push_str("        }\n    }\n");
            out.push_str(
                "    unsafe { &*core::ptr::addr_of!(__JET_HARDWARE_INTERRUPT_REGISTRY) }\n}\n\n",
            );
            let _ = writeln!(out, "#[inline]");
            let _ = writeln!(out, "fn __jet_hardware_poll() {{");
            let _ = writeln!(
                out,
                "    let __jet_hardware_registry = __jet_hardware_registry();"
            );
            let _ = writeln!(
                out,
                "    if !{root}jet_hardware_has_pending() {{ return; }}"
            );
            let _ = writeln!(
                out,
                "    let __jet_hardware_callbacks = {root}jet_hardware_dispatch_pending(__jet_hardware_registry);"
            );
            let _ = writeln!(
                out,
                "    {root}jet_hardware_invoke_pending(__jet_hardware_callbacks);"
            );
            out.push_str("}\n");
            return;
        }

        let _ = writeln!(
            out,
            "static __JET_HARDWARE_INTERRUPT_REGISTRY: std::sync::LazyLock<{root}JetHardwareInterruptRegistry> = std::sync::LazyLock::new(|| {{"
        );
        let _ = writeln!(
            out,
            "    let mut __jet_hardware_registry = {root}JetHardwareInterruptRegistry::default();"
        );
        for (setup_index, setup) in self.program.facts.hardware_setups.iter().enumerate() {
            let MirHardwareSetup::InterruptBind { vector, .. } = setup else {
                continue;
            };
            let _ = writeln!(
                out,
                "    if !__jet_hardware_registry.register({vector}u16, {root}JetHardwareInterruptHandler {{ fn_ptr: __jet_hardware_interrupt_trampoline_{setup_index} as usize as i64, env: 0, has_env: false }}) {{"
            );
            out.push_str(
                "        panic!(\"duplicate MIR hardware interrupt vector binding\");\n    }\n",
            );
        }
        out.push_str("    __jet_hardware_registry\n});\n\n");
        let _ = writeln!(out, "#[inline]");
        let _ = writeln!(out, "fn __jet_hardware_poll() {{");
        let _ = writeln!(
            out,
            "    if !{root}jet_hardware_has_pending() {{ return; }}"
        );
        let _ = writeln!(
            out,
            "    let __jet_hardware_callbacks = {root}jet_hardware_dispatch_pending(&*__JET_HARDWARE_INTERRUPT_REGISTRY);"
        );
        let _ = writeln!(
            out,
            "    {root}jet_hardware_invoke_pending(__jet_hardware_callbacks);"
        );
        out.push_str("}\n");
    }

    fn emit_hardware_setups(&self, out: &mut String, indent: &str) {
        let root = &self.config.root_prefix;
        if self.is_no_os() {
            // Interrupt bindings are installed by the fixed no-OS registry
            // emitted above. DMA needs a target-owned mapping/provider ABI;
            // fail explicitly rather than linking a hosted replay bridge or a
            // success-shaped no-op.
            for setup in &self.program.facts.hardware_setups {
                if matches!(setup, MirHardwareSetup::DmaConfigure { .. }) {
                    let _ = writeln!(
                        out,
                        "{indent}{root}jet_target_failure_bytes(b\"DMA setup requires a target provider\", -1);"
                    );
                }
            }
            return;
        }

        for setup in &self.program.facts.hardware_setups {
            let call = match setup {
                MirHardwareSetup::DmaConfigure {
                    profile_id,
                    channel,
                    transfer_width,
                    ownership,
                } => format!(
                    "{root}jet_hardware_setup_typed({profile_id:?}, {root}JET_HARDWARE_SETUP_DMA_CONFIGURE_NAME, {channel:?}, {}i64, {:?})",
                    transfer_width.bytes(),
                    ownership.as_str(),
                ),
                MirHardwareSetup::InterruptBind {
                    profile_id,
                    interrupt,
                    vector,
                    handler_symbol,
                    forbidden_effects: _,
                } => format!(
                    "{root}jet_hardware_setup_typed({profile_id:?}, {root}JET_HARDWARE_SETUP_INTERRUPT_BIND_NAME, {interrupt:?}, {}i64, {handler_symbol:?})",
                    vector
                ),
            };
            let _ = writeln!(out, "{indent}let _ = {call};");
        }
    }

    fn emit_entry(&self, out: &mut String) {
        // JS owns the loader, but a checked Wasm entry remains Wasm code.
        if self.config.target_kind == MirRustTarget::WebWasm {
            let Some(entry) = &self.artifact.entry else {
                return;
            };
            let Some(function_id) = entry.function else {
                return;
            };
            let function = self.function_row(function_id);
            if function.web_bucket == Some(jet_foundation::WebPartition::WebBucket::JS) {
                return;
            }
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!("MIR Web entry function is not selected for this artifact");
            }
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
                || !function.params.is_empty()
            {
                panic!("MIR Web entry has no checked zero-argument invocation");
            }
            let root = &self.config.root_prefix;
            let _ = writeln!(
                out,
                "#[no_mangle]\npub extern \"C\" fn jet_entry_{}() {{\n    jet_wasm_error_clear();",
                function.id.0,
            );
            if entry.initialize_environment {
                let _ = writeln!(out, "    {root}jet_std_env_init();");
            }
            if entry.initialize_gc {
                let _ = writeln!(
                    out,
                    "    {root}jet_gc::runtime_or_exit({root}jet_gc::initialize_trace());",
                );
            }
            let call = format!("{}()", self.function_name(function.id));
            out.push_str(&self.entry_invoke(function, &call, entry.output, false, false, "    "));
            out.push_str("}\n\n");
            return;
        }
        let Some(entry) = &self.artifact.entry else {
            return;
        };
        let service = match entry.kind {
            MirEntryKind::Library
            | MirEntryKind::Command
            | MirEntryKind::App
            | MirEntryKind::Test => false,
            MirEntryKind::Service => true,
        };
        let _ = writeln!(
            out,
            "// jet-mir-entry: kind={:?} output={:?} version={} init_env={} init_gc={} serves_until_stopped={}",
            entry.kind,
            entry.output,
            entry.package_version,
            entry.initialize_environment,
            entry.initialize_gc,
            entry.serves_until_stopped
        );
        if let Some(cli) = &entry.cli {
            let _ = writeln!(
                out,
                "// jet-mir-cli: standard={} description={:?}",
                cli.standard, cli.description
            );
            for input in &cli.inputs {
                self.emit_cli_input_metadata(input, out);
            }
            for command in &cli.commands {
                let _ = writeln!(
                    out,
                    "// jet-mir-cli-command: name={} description={:?} receiver={:?}",
                    command.name, command.description, command.receiver
                );
                let function = self.function_row(command.function);
                if !self.module_selected(function.module_id) || !self.selected_for_target(function)
                {
                    panic!(
                        "MIR CLI command {:?} targets an unselected function",
                        command.name
                    );
                }
                for input in &command.inputs {
                    self.emit_cli_input_metadata(input, out);
                }
            }
        }
        if matches!(entry.kind, MirEntryKind::Library) {
            return;
        }
        let root = &self.config.root_prefix;
        let mut generated = String::new();
        if self.is_no_os() {
            generated.push_str("#[no_mangle]\npub extern \"C\" fn __jet_program_entry() {\n");
        } else {
            generated.push_str("fn main() {\n");
        }
        let has_hardware_host = self.program.facts.hardware_profile.is_some() && !self.is_no_os();
        if !self.artifact.jobs.is_empty() {
            let _ = writeln!(
                generated,
                "    let _jet_job_registry = {root}jet_job_register_specs(__JET_JOB_SPECS);"
            );
        }
        if has_hardware_host {
            let _ = writeln!(
                generated,
                "    let mut __jet_hardware_host = {root}JetHardwareReplayHost::new(__JET_HARDWARE_FACTS);"
            );
            let _ = writeln!(
                generated,
                "    let _jet_hardware_facts_scope = {root}jet_hardware_facts_scope(&__JET_HARDWARE_FACTS);"
            );
            let _ = writeln!(
                generated,
                "    {root}jet_hardware_with_host(&mut __jet_hardware_host, || {{"
            );
        }
        self.emit_hardware_setups(&mut generated, "    ");
        if self.has_hardware_interrupts() {
            let _ = writeln!(generated, "    __jet_hardware_poll();");
        }
        if entry.initialize_environment {
            let _ = writeln!(generated, "    {root}jet_std_env_init();");
        }
        if entry.initialize_gc {
            let _ = writeln!(
                generated,
                "    {root}jet_gc::runtime_or_exit({root}jet_gc::initialize_trace());"
            );
        }
        let needs_argv = !self.artifact.jobs.is_empty() || entry.cli.is_some();
        if needs_argv {
            let _ = writeln!(generated, "    let __argv = {root}jet_std_io_args();");
            if !self.artifact.jobs.is_empty() {
                let _ = writeln!(
                    generated,
                    "    if __jet_job_dispatch(&__argv, {service}) {{ return; }}"
                );
            }
        }
        if matches!(entry.kind, MirEntryKind::Test) && self.artifact.harness.is_some() {
            let _ = writeln!(
                generated,
                "    {root}jet_runtime_boundary(|| {{ if !__jet_harness_run() {{ std::process::exit(1); }} }});"
            );
        } else if let Some(cli) = &entry.cli {
            let program = format!(
                "&{root}jet_args_source_program_name(__argv.first().map(String::as_str).unwrap_or(\"\"))"
            );
            let mut spec = self.cli_spec_expr(
                &cli.inputs,
                cli.description.as_deref(),
                cli.standard,
                cli.standard.then_some(entry.package_version.as_str()),
                &program,
            );
            for command in &cli.commands {
                let command_spec = self.cli_spec_expr(
                    &command.inputs,
                    command.description.as_deref(),
                    false,
                    None,
                    &program,
                );
                spec = format!(
                    "{root}jet_args_subcommand({spec}, &{:?}.to_string(), &{:?}.to_string(), {command_spec})",
                    command.name,
                    command.description.clone().unwrap_or_default()
                );
            }
            let _ = writeln!(generated, "    let __spec = {spec};");
            let _ = writeln!(
                generated,
                "    let __argv = match {root}jet_args_guided_argv_tui(&__spec, &__argv) {{ Ok(__guided) => __guided, Err(__e) => {{ eprint!(\"{{}}\", {root}jet_cli_banner(&__e)); std::process::exit(2); }} }};"
            );
            let _ = writeln!(
                generated,
                "    match {root}jet_args_parse(&__spec, &__argv) {{"
            );
            let _ = writeln!(generated, "        Ok(__parsed) => {{");
            let _ = writeln!(
                generated,
                "            if {root}jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ print!(\"{{}}\", {root}jet_cli_banner(&__spec.help())); return; }}"
            );
            if cli.standard {
                let _ = writeln!(
                    generated,
                    "            if {root}jet_parsed_flag(&__parsed, &\"version\".to_string()) {{ println!({:?}); return; }}",
                    entry.package_version
                );
            }
            if cli.commands.is_empty() {
                let function_id = entry
                    .function
                    .unwrap_or_else(|| panic!("MIR CLI entry has no function row"));
                let function = self.function_row(function_id);
                if !self.module_selected(function.module_id) || !self.selected_for_target(function)
                {
                    panic!("MIR entry function is not selected for this artifact");
                }
                generated.push_str(&self.cli_decode_and_invoke(
                    function,
                    &cli.inputs,
                    entry.output,
                    entry.serves_until_stopped,
                    service,
                    "            ",
                ));
            } else {
                let _ = writeln!(
                    generated,
                    "            match {root}jet_parsed_subcommand(&__parsed).ok().as_deref() {{"
                );
                for command in &cli.commands {
                    let function = self.function_row(command.function);
                    generated
                        .push_str(&format!("                Some({:?}) => {{\n", command.name));
                    generated.push_str(&self.cli_decode_and_invoke(
                        function,
                        &command.inputs,
                        entry.output,
                        entry.serves_until_stopped,
                        service,
                        "                    ",
                    ));
                    generated.push_str("                }\n");
                }
                if let Some(function_id) = entry.function {
                    let function = self.function_row(function_id);
                    generated.push_str("                None => {\n");
                    generated.push_str(&self.cli_decode_and_invoke(
                        function,
                        &cli.inputs,
                        entry.output,
                        entry.serves_until_stopped,
                        service,
                        "                    ",
                    ));
                    generated.push_str("                }\n");
                } else {
                    generated.push_str(
                        "                None => { eprintln!(\"no command selected\"); std::process::exit(2); }\n",
                    );
                }
                generated.push_str(
                    "                Some(__other) => { eprintln!(\"unknown command: {}\", __other); std::process::exit(2); }\n            }\n",
                );
            }
            generated.push_str("        }\n");
            generated.push_str(&format!(
                "        Err(__e) => {{ eprint!(\"{{}}\", {root}jet_cli_banner(&__e)); std::process::exit(2); }}\n    }}\n"
            ));
        } else {
            let function_id = entry
                .function
                .unwrap_or_else(|| panic!("MIR executable entry has no function row"));
            let function = self.function_row(function_id);
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!("MIR entry function is not selected for this artifact");
            }
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
                || !function.params.is_empty()
            {
                panic!("MIR entry function has no checked CLI decoder row");
            }
            let call = format!("{}()", self.function_name(function.id));
            generated.push_str(&self.entry_invoke(
                function,
                &call,
                entry.output,
                entry.serves_until_stopped,
                service,
                "    ",
            ));
        }
        if has_hardware_host {
            generated.push_str("    });\n");
        }
        generated.push_str("}\n\n");
        out.push_str(&generated);
    }

    fn emit_jobs(&self, out: &mut String) {
        let root = &self.config.root_prefix;
        for job_id in &self.artifact.jobs {
            let job = self
                .program
                .jobs
                .iter()
                .find(|job| job.id == *job_id)
                .unwrap_or_else(|| panic!("MIR job ID {:?} has no row", job_id));
            let function = self.function_row(job.function);
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!("MIR job {:?} targets an unselected function", job.name);
            }
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
            {
                panic!(
                    "MIR job {:?} requires a top-level non-capturing function",
                    job.name
                );
            }
            let wrapper = format!("__jet_job_{}", job.id.0);
            let validator = format!("__jet_job_validate_{}", job.id.0);
            if job.inputs.is_empty() {
                let _ = writeln!(
                    out,
                    "fn {validator}(__argv: &[String]) -> Result<(), String> {{ if __argv.is_empty() {{ Ok(()) }} else {{ Err(\"job takes no arguments\".to_string()) }} }}"
                );
            } else {
                let validator_spec =
                    self.cli_spec_expr(&job.inputs, Some(&job.name), false, None, "\"job\"");
                let _ = writeln!(
                    out,
                    "fn {validator}(__argv: &[String]) -> Result<(), String> {{ let mut __job_argv = vec![\"job\".to_string()]; __job_argv.extend_from_slice(__argv); let __spec = {validator_spec}; match {root}jet_args_parse(&__spec, &__job_argv) {{ Ok(_) => Ok(()), Err(__e) => Err(__e) }} }}"
                );
            }
            let _ = writeln!(out, "fn {wrapper}(__program: &str, __argv: &[String]) {{");
            if job.inputs.is_empty() {
                if !function.params.is_empty() {
                    panic!(
                        "MIR job {:?} has parameters but no checked input rows",
                        job.name
                    );
                }
                let call = format!("{}()", self.function_name(function.id));
                out.push_str(&self.entry_invoke(
                    function,
                    &call,
                    MirEntryOutput::None,
                    false,
                    false,
                    "    ",
                ));
            } else {
                let program = "__program";
                let spec = self.cli_spec_expr(&job.inputs, Some(&job.name), false, None, program);
                let _ = writeln!(out, "    let mut __job_argv = vec![__program.to_string()];");
                let _ = writeln!(out, "    __job_argv.extend_from_slice(__argv);");
                let _ = writeln!(out, "    let __spec = {spec};");
                let _ = writeln!(
                    out,
                    "    match {root}jet_args_parse(&__spec, &__job_argv) {{\n        Ok(__parsed) => {{"
                );
                let _ = writeln!(
                    out,
                    "            if {root}jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ print!(\"{{}}\", {root}jet_cli_banner(&__spec.help())); return; }}"
                );
                out.push_str(&self.cli_decode_and_invoke(
                    function,
                    &job.inputs,
                    MirEntryOutput::None,
                    false,
                    false,
                    "            ",
                ));
                let _ = writeln!(
                    out,
                    "        }}\n        Err(__e) => {{ eprint!(\"{{}}\", {root}jet_cli_banner(&__e)); std::process::exit(2); }}\n    }}"
                );
            }
            let _ = writeln!(out, "}}\n");
            if job.inputs.len() == 1 && function.params.len() == 1 {
                let queue_wrapper = format!("__jet_job_queue_{}", job.id.0);
                let payload_rust = self.rust_type(&function.params[0].ty);
                let result_encoding = if function.return_type.is_unit() {
                    format!(
                        "    {}(__value);\n\
                         Ok({root}JetJobResult {{ type_id: \"Unit\".to_string(), bytes: Vec::new(), publish: false }})\n",
                        self.function_name(function.id)
                    )
                } else {
                    match function.return_type.kind() {
                        MirTypeKind::Result { ok, .. } => {
                            let ok_type = ok.display_name();
                            format!(
                                "    let __job_result = {}(__value);\n\
                                 match __job_result {{\n\
                                     Ok(__ok) => {{\n\
                                         let __bytes = match {root}jet_enc_cbor_to_bytes_canonical(&__ok) {{\n\
                                             Ok(__bytes) => __bytes,\n\
                                             Err(__error) => return Err({root}JetJobError {{ type_id: __payload.type_id.clone(), reason: \"encode\".to_string(), detail: Some(format!(\"{{:?}}\", __error)) }}),\n\
                                         }};\n\
                                         Ok({root}JetJobResult {{ type_id: {:?}.to_string(), bytes: __bytes, publish: false }})\n\
                                     }}\n\
                                 }}\n",
                                self.function_name(function.id),
                                ok_type
                            )
                        }
                        _ => {
                            let result_type = function.return_type.display_name();
                            format!(
                                "    let __job_result = {}(__value);\n\
                                 let __bytes = match {root}jet_enc_cbor_to_bytes_canonical(&__job_result) {{\n\
                                     Ok(__bytes) => __bytes,\n\
                                     Err(__error) => return Err({root}JetJobError {{ type_id: __payload.type_id.clone(), reason: \"encode\".to_string(), detail: Some(format!(\"{{:?}}\", __error)) }}),\n\
                                 }};\n\
                                 Ok({root}JetJobResult {{ type_id: {:?}.to_string(), bytes: __bytes, publish: false }})\n",
                                self.function_name(function.id),
                                result_type
                            )
                        }
                    }
                };
                let _ = writeln!(
                    out,
                    "fn {queue_wrapper}(__payload: &{root}JetJobPayload) -> Result<{root}JetJobResult, {root}JetJobError> {{"
                );
                let _ = writeln!(
                    out,
                    "    let __bytes = __payload.bytes.clone();\n    let __value = match {root}jet_enc_cbor_decode::<{payload_rust}>(&__bytes, {root}jet_std::CBOROptions::safe()) {{ Ok(__value) => __value, Err(__error) => return Err({root}JetJobError {{ type_id: __payload.type_id.clone(), reason: \"decode\".to_string(), detail: Some(format!(\"{{:?}}\", __error)) }}), }};"
                );
                out.push_str(&result_encoding);
                out.push_str("}\n\n");
            }
        }
        let _ = writeln!(out, "static __JET_JOB_SPECS: &[{root}JetJobSpec] = &[");
        for job_id in &self.artifact.jobs {
            let job = self
                .program
                .jobs
                .iter()
                .find(|job| job.id == *job_id)
                .unwrap_or_else(|| panic!("MIR job ID {:?} has no row", job_id));
            let function = self.function_row(job.function);
            let scope = match job.scope {
                MirJobScope::Dev => format!("{root}JetJobScope::Dev"),
                MirJobScope::Ship => format!("{root}JetJobScope::Ship"),
                MirJobScope::Internal => format!("{root}JetJobScope::Internal"),
            };
            let schedule = match job.schedule {
                None => "None".to_string(),
                Some(MirJobSchedule::Duration { nanos }) => {
                    format!("Some({root}JetJobSchedule::Duration {{ nanos: {nanos} }})")
                }
                Some(MirJobSchedule::WallClockTime { hour, minute }) => format!(
                    "Some({root}JetJobSchedule::WallClockTime {{ hour: {hour}, minute: {minute} }})"
                ),
            };
            let dispatch = match job.dispatch {
                MirJobDispatch::Direct => format!("{root}JetJobDispatch::Direct"),
                MirJobDispatch::Spawn => format!("{root}JetJobDispatch::Spawn"),
                MirJobDispatch::Scheduled => format!("{root}JetJobDispatch::Scheduled"),
            };
            let skip = match &job.skip {
                None => "None".to_string(),
                Some(MirJobSkip::Always(reason)) => {
                    format!("Some({root}JetJobSkip::Always({reason:?}))")
                }
                Some(MirJobSkip::UnlessPlatform(platform)) => {
                    format!("Some({root}JetJobSkip::UnlessPlatform({platform:?}))")
                }
            };
            let cache = match job.cache {
                MirJobCachePolicy::Uncached => format!("{root}JetJobCachePolicy::Uncached"),
                MirJobCachePolicy::Local => format!("{root}JetJobCachePolicy::Local"),
                MirJobCachePolicy::Shared => format!("{root}JetJobCachePolicy::Shared"),
            };
            let packages = format!(
                "&[{}]",
                job.packages
                    .iter()
                    .map(|package| format!("{package:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let input_paths = format!(
                "&[{}]",
                job.input_paths
                    .iter()
                    .map(|path| format!("{path:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let output_paths = format!(
                "&[{}]",
                job.output_paths
                    .iter()
                    .map(|path| format!("{path:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let limits = format!(
                "&[{}]",
                job.limits
                    .iter()
                    .map(|(name, value)| format!("({name:?}, {value:?})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let after = format!(
                "&[{}]",
                job.after
                    .iter()
                    .map(|dependency| format!("{dependency:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let working_directory = job
                .working_directory
                .as_ref()
                .map(|directory| format!("Some({directory:?})"))
                .unwrap_or_else(|| "None".to_string());
            let queue_enabled = job.inputs.len() == 1 && function.params.len() == 1;
            let payload_type = if queue_enabled {
                job.inputs[0]
                    .ty
                    .nominal_name()
                    .map(|name| format!("Some({name:?})"))
                    .unwrap_or_else(|| "None".to_string())
            } else {
                "None".to_string()
            };
            let queue_invoke = if queue_enabled {
                format!("Some(__jet_job_queue_{})", job.id.0)
            } else {
                "None".to_string()
            };
            let _ = writeln!(
                out,
                "        {root}JetJobSpec {{ entry: {root}JetJobEntry {{ name: {:?}, scope: {scope}, schedule: {schedule}, invoke: __jet_job_{} }}, payload_type: {payload_type}, queue_invoke: {queue_invoke}, dispatch: {dispatch}, packages: {packages}, working_directory: {working_directory}, input_paths: {input_paths}, output_paths: {output_paths}, skip: {skip}, cache: {cache}, limits: {limits}, after: {after}, parallel: {}, validate: Some(__jet_job_validate_{}) }},",
                job.name,
                job.id.0,
                job.parallel.max(1),
                job.id.0
            );
        }
        let _ = writeln!(out, "];");
        let _ = writeln!(
            out,
            "fn __jet_job_dispatch(__argv: &[String], __service: bool) -> bool {{"
        );
        let _ = writeln!(out, "    let __jobs = __JET_JOB_SPECS;");
        if self
            .artifact
            .runtime_parts
            .contains(&MirRuntimePartId::Services)
        {
            out.push_str(&format!(
                r#"    let __job_service_endpoint = match {root}jet_services_active_execution_endpoint_if_present() {{
        Ok(__endpoint) => Ok(__endpoint),
        Err(__error) => Err(format!("{{__error:?}}")),
    }};
    let __job_scope = move || -> Result<Option<{root}JetServiceExecutionScope>, String> {{
        match &__job_service_endpoint {{
            Ok(Some(__endpoint)) => {root}jet_services_execution_scope(__endpoint)
                .map(Some)
                .map_err(|__error| format!("{{__error:?}}")),
            Ok(None) => Ok(None),
            Err(__reason) => Err(__reason.clone()),
        }}
    }};
    if {root}jet_job_dispatch_specs(__argv, __jobs, __job_scope.clone()) {{ return true; }}
    if __service {{
        {root}jet_job_service_tick_specs_retained(
            __jobs,
            __argv.first().map(String::as_str).unwrap_or("program"),
            __job_scope,
        );
    }}
    false
}}
"#,
                root = root,
            ));
        } else {
            out.push_str(&format!(
                r#"    if {root}jet_job_dispatch_specs(__argv, __jobs, {root}jet_job_no_scope) {{ return true; }}
    if __service {{
        {root}jet_job_service_tick_specs_retained(
            __jobs,
            __argv.first().map(String::as_str).unwrap_or("program"),
            {root}jet_job_no_scope,
        );
    }}
    false
}}
"#,
                root = root,
            ));
        }
    }

    fn emit_harness(&self, out: &mut String) {
        let Some(harness_id) = self.artifact.harness else {
            if !self.artifact.jobs.is_empty() {
                self.emit_jobs(out);
            }
            return;
        };
        let harness = self
            .program
            .harnesses
            .iter()
            .find(|harness| harness.id == harness_id)
            .unwrap_or_else(|| panic!("MIR harness ID {:?} has no row", harness_id));
        let root = &self.config.root_prefix;
        let _ = writeln!(
            out,
            "// jet-mir-harness: id={} kind={:?} selected_test={:?} command_override={}",
            harness.id.0, harness.kind, harness.selected_test, harness.command_override
        );
        let mut selected_tests = harness
            .tests
            .iter()
            .copied()
            .filter(|id| harness.selected_test.is_none_or(|selected| selected == *id))
            .collect::<Vec<_>>();
        selected_tests.sort_by_key(|id| {
            let test = self
                .program
                .tests
                .iter()
                .find(|test| test.id == *id)
                .unwrap_or_else(|| panic!("MIR test ID {:?} has no row", id));
            let function = self.function_row(test.function);
            (&function.module, test.span.start, &test.name)
        });
        for test_id in &selected_tests {
            let test = self
                .program
                .tests
                .iter()
                .find(|test| test.id == *test_id)
                .unwrap_or_else(|| panic!("MIR test ID {:?} has no row", test_id));
            let function = self.function_row(test.function);
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!("MIR test {:?} targets an unselected function", test.name);
            }
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
            {
                panic!("MIR test {:?} is not a top-level callable", test.name);
            }
            if matches!(test.kind, MirTestKind::Unit) && !function.params.is_empty() {
                panic!("MIR unit test {:?} has checked parameters", test.name);
            }
            if matches!(test.kind, MirTestKind::Property)
                && test.generation_unavailable_reason.is_none()
                && (function.params.is_empty() || function.params.len() != test.parameters.len())
            {
                panic!(
                    "MIR property test {:?} has inconsistent checked parameters",
                    test.name
                );
            }
            let wrapper = format!("__jet_test_{}", test.id.0);
            let name_literal = format!("{:?}", test.name);
            let expected_failure = test.expected_failure;
            let _ = writeln!(
                out,
                "// jet-mir-test: id={} name={} kind={:?} expected_failure={} faults={:?}",
                test.id.0, test.name, test.kind, test.expected_failure, test.faults
            );
            if matches!(test.kind, MirTestKind::Property) {
                if let Some(reason) = test.generation_unavailable_reason.as_deref() {
                    let reason = format!("E0613: {reason}");
                    let _ = writeln!(
                        out,
                        "fn {wrapper}() -> JetTestOutcome {{ JetTestOutcome {{ name: {name_literal}.to_string(), ok: false, expected_failure: {expected_failure}, skipped: false, stdout: String::new(), stderr: {reason:?}.to_string(), property_cases: Some(0) }} }}"
                    );
                    continue;
                }
                if let Some(reason) = self.property_generation_unavailable_reason(function) {
                    let _ = writeln!(
                        out,
                        "fn {wrapper}() -> JetTestOutcome {{ JetTestOutcome {{ name: {name_literal}.to_string(), ok: false, expected_failure: {expected_failure}, skipped: false, stdout: String::new(), stderr: {reason:?}.to_string(), property_cases: Some(0) }} }}"
                    );
                    continue;
                }
                let arg_names = (0..function.params.len())
                    .map(|index| format!("__jet_arg{index}"))
                    .collect::<Vec<_>>();
                let declarations = function
                    .params
                    .iter()
                    .zip(&arg_names)
                    .map(|(param, name)| {
                        format!(
                            "        let {name}: {} = <{} as JetGen>::generate(&mut __jet_rng);",
                            self.rust_type(&param.ty),
                            self.rust_type(&param.ty)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let input = function
                    .params
                    .iter()
                    .zip(&arg_names)
                    .map(|(param, name)| {
                        format!(
                            "{}={}",
                            param.name,
                            format!("<{} as JetGen>::render(&{name})", self.rust_type(&param.ty))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let call_args = function
                    .params
                    .iter()
                    .zip(&arg_names)
                    .map(|(param, name)| match param.access {
                        MirAccess::Write => format!("&mut {name}"),
                        MirAccess::Read if self.is_scalar(&param.ty) => name.clone(),
                        MirAccess::Read => format!("&{name}"),
                        MirAccess::Move => name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let call = format!("{}({call_args})", self.function_name(function.id));
                let body = match &function.failure {
                    MirFailureCarrier::Diverges { .. } => format!("{call}; Ok(())"),
                    MirFailureCarrier::Infallible => format!("let _ = {call}; Ok(())"),
                    MirFailureCarrier::Result { .. } | MirFailureCarrier::Optional { .. } => {
                        format!(
                            "match {call} {{ Ok(_) => Ok(()), Err(__e) => Err(format!(\"{{:?}}\", __e)) }}"
                        )
                    }
                };
                // #2502: a sampled contract row classifies each generated
                // input BEFORE the callable runs, by calling the typed
                // eligibility predicate synthesized from the candidate's own
                // checked `#Pre` conditions. A rejected input never executes
                // the callable; an accepted input runs under ordinary
                // semantics, so a nested callee's `#Pre` failure inside the
                // body stays real failure evidence.
                let test_runner = if test.contract_generated {
                    let eligibility = test.eligibility.unwrap_or_else(|| {
                        panic!(
                            "MIR contract test {:?} has no eligibility predicate",
                            test.name
                        )
                    });
                    let predicate = self.function_row(eligibility);
                    if !self.module_selected(predicate.module_id)
                        || !self.selected_for_target(predicate)
                    {
                        panic!(
                            "MIR contract test {:?} eligibility predicate is unselected",
                            test.name
                        );
                    }
                    if predicate.params.len() != function.params.len() {
                        panic!(
                            "MIR contract test {:?} eligibility predicate arity mismatch",
                            test.name
                        );
                    }
                    let predicate_args = predicate
                        .params
                        .iter()
                        .zip(&arg_names)
                        .map(|(param, name)| match param.access {
                            MirAccess::Read if self.is_scalar(&param.ty) => name.clone(),
                            MirAccess::Read => format!("&{name}"),
                            MirAccess::Write | MirAccess::Move => panic!(
                                "MIR contract test {:?} eligibility predicate must borrow its parameters",
                                test.name
                            ),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "{{ let __jet_eligible = jet_test_contract_eligibility(|| {}({predicate_args})); \
                         match __jet_eligible {{ \
                         Ok(false) => Ok(JetPropertyCaseResult::Rejected), \
                         Ok(true) => jet_test_run_property(|| {{ {body} }}), \
                         Err(__jet_pre_error) => Err(__jet_pre_error) }} }}",
                        self.function_name(predicate.id)
                    )
                } else {
                    format!("jet_test_run_property(|| {{ {body} }})")
                };
                let property_ok = if test.contract_generated {
                    format!("{} && __jet_accepted_cases > 0", !test.expected_failure)
                } else {
                    format!("{}", !test.expected_failure)
                };
                let property_stderr = if test.contract_generated {
                    "if __jet_accepted_cases == 0 { \"E0613: no accepted contract examples\".to_string() } else { String::new() }"
                } else {
                    "String::new()"
                };
                let replay_declarations = declarations.replace("__jet_rng", "__jet_replay_rng");
                let source = format!(
                    "fn {wrapper}() -> JetTestOutcome {{\n\
                         let __jet_seed = jet_prop_seed();\n\
                         let __jet_replay_seed = jet_prop_replay_seed();\n\
                         let __jet_replay_case = jet_prop_replay_case();\n\
                         let mut __jet_stdout = String::new();\n\
                         let mut __jet_accepted_cases: u64 = 0;\n\
                         if let (Some(__jet_replay_seed), Some(__jet_replay_case)) = (__jet_replay_seed, __jet_replay_case) {{\n\
                         if __jet_replay_seed == __jet_seed && __jet_replay_case < jet_prop_cases() {{\n\
                         let mut __jet_replay_rng = JetRng::new(__jet_replay_seed);\n\
                         for __jet_replay_index in 0..=__jet_replay_case {{\n\
                         {replay_declarations}\n\
                         if __jet_replay_index != __jet_replay_case {{ continue; }}\n\
                         let __jet_input = {input:?}.to_string();\n\
                         jet_prop_trace_sample(\"aot\", __jet_replay_index, __jet_replay_seed, &__jet_input);\n\
                         let __jet_result = {test_runner};\n\
                         let __jet_case_stdout = jet_test_take_output();\n\
                         __jet_stdout.push_str(&__jet_case_stdout);\n\
                         match __jet_result {{\n\
                         Ok(JetPropertyCaseResult::Accepted) => {{\n\
                         __jet_accepted_cases += 1;\n\
                         }}\n\
                         Ok(JetPropertyCaseResult::Rejected) => {{}},\n\
                         Err(__jet_error) => {{\n\
                         __jet_accepted_cases += 1;\n\
                         return JetTestOutcome {{ name: {name_literal}.to_string(), ok: {}, expected_failure: {expected_failure}, skipped: false, stdout: __jet_stdout, stderr: format!(\"seed {{}} case {{}}: {{}} (input {{}})\", __jet_replay_seed, __jet_replay_case, __jet_error, __jet_input), property_cases: Some(__jet_accepted_cases) }};\n\
                         }}\n\
                         }}\n\
                         }}\n\
                         }}\n\
                         }}\n\
                         let mut __jet_rng = JetRng::new(__jet_seed);\n\
                         for __jet_case_index in 0..jet_prop_cases() {{\n\
                         if __jet_replay_seed == Some(__jet_seed) && __jet_replay_case == Some(__jet_case_index) {{ continue; }}\n\
                         {declarations}\n\
                         let __jet_input = {input:?}.to_string();\n\
                         jet_prop_trace_sample(\"aot\", __jet_case_index, __jet_seed, &__jet_input);\n\
                         let __jet_result = {test_runner};\n\
                         let __jet_case_stdout = jet_test_take_output();\n\
                         __jet_stdout.push_str(&__jet_case_stdout);\n\
                         match __jet_result {{\n\
                         Ok(JetPropertyCaseResult::Accepted) => {{ __jet_accepted_cases += 1; }}\n\
                         Ok(JetPropertyCaseResult::Rejected) => {{}},\n\
                         Err(__jet_error) => {{\n\
                         __jet_accepted_cases += 1;\n\
                         return JetTestOutcome {{ name: {name_literal}.to_string(), ok: {}, expected_failure: {expected_failure}, skipped: false, stdout: __jet_stdout, stderr: format!(\"seed {{}} case {{}}: {{}} (input {{}})\", __jet_seed, __jet_case_index, __jet_error, __jet_input), property_cases: Some(__jet_accepted_cases) }};\n\
                         }}\n\
                         }}\n\
                         }}\n\
                         JetTestOutcome {{ name: {name_literal}.to_string(), ok: {property_ok}, expected_failure: {expected_failure}, skipped: false, stdout: __jet_stdout, stderr: {property_stderr}, property_cases: Some(__jet_accepted_cases) }}\n\
                     }}\n",
                    test.expected_failure,
                    test.expected_failure,
                );
                out.push_str(&source);
            } else {
                let call = format!("{}()", self.function_name(function.id));
                let body = match &function.failure {
                    MirFailureCarrier::Diverges { .. } => format!("{call}; Ok(())"),
                    MirFailureCarrier::Infallible => format!("let _ = {call}; Ok(())"),
                    MirFailureCarrier::Result { .. } | MirFailureCarrier::Optional { .. } => {
                        format!(
                            "match {call} {{ Ok(_) => Ok(()), Err(__e) => Err(format!(\"{{:?}}\", __e)) }}"
                        )
                    }
                };
                let source = format!(
                    "fn {wrapper}() -> JetTestOutcome {{\n\
                         let __jet_result = jet_test_run(|| {{ {body} }});\n\
                         let __jet_stdout = jet_test_take_output();\n\
                         let (__jet_failed, __jet_stderr) = match __jet_result {{ Ok(()) => (false, String::new()), Err(__jet_error) => (true, __jet_error) }};\n\
                         let __jet_skipped = jet_test_take_whole_skip();\n\
                         JetTestOutcome {{ name: {name_literal}.to_string(), ok: __jet_skipped || (__jet_failed == {}), expected_failure: {expected_failure}, skipped: __jet_skipped, stdout: __jet_stdout, stderr: __jet_stderr, property_cases: None }}\n\
                     }}\n",
                    test.expected_failure,
                );
                out.push_str(&source);
            }
        }
        for check in &harness.output_checks {
            let function = self.function_row(check.function);
            if !self.module_selected(function.module_id) || !self.selected_for_target(function) {
                panic!(
                    "MIR output check {:?} targets an unselected function",
                    check.name
                );
            }
            if !matches!(function.form, MirFunctionForm::TopLevel)
                || !function.capture_params.is_empty()
                || !function.params.is_empty()
            {
                panic!(
                    "MIR output check {:?} has no checked zero-argument invocation",
                    check.name
                );
            }
            let call = format!("{}()", self.function_name(check.function));
            let _ = writeln!(
                out,
                "// jet-mir-output-check: id={} name={} function={}",
                check.id.0,
                check.name,
                self.function_name(check.function)
            );
            match function.return_type.kind() {
                MirTypeKind::Bool => {
                    let _ = writeln!(
                        out,
                        "fn __jet_output_check_{}() {{ assert!({call}, \"MIR output check {:?}\"); }}",
                        check.id.0, check.name
                    );
                }
                MirTypeKind::Int
                | MirTypeKind::Float
                | MirTypeKind::String
                | MirTypeKind::Char
                | MirTypeKind::List(_)
                | MirTypeKind::Map { .. }
                | MirTypeKind::Shared(_)
                | MirTypeKind::Option(_)
                | MirTypeKind::Result { .. }
                | MirTypeKind::Fn(_)
                | MirTypeKind::SendFn { .. }
                | MirTypeKind::Apply { .. }
                | MirTypeKind::TraitObject(_)
                | MirTypeKind::Tuple(_)
                | MirTypeKind::FixedList { .. }
                | MirTypeKind::IntN { .. }
                | MirTypeKind::InlineRange { .. }
                | MirTypeKind::Float32
                | MirTypeKind::Tagged { .. }
                | MirTypeKind::Quantity { .. }
                | MirTypeKind::Union(_)
                | MirTypeKind::Measure(_) => {
                    let _ = writeln!(
                        out,
                        "fn __jet_output_check_{}() {{ let _ = {call}; }}",
                        check.id.0
                    );
                }
            }
        }
        for point in &harness.coverage_points {
            let function = self.function_row(point.function);
            if !self.module_selected(function.module_id) {
                panic!(
                    "MIR coverage point {:?} targets an unselected module",
                    point.id
                );
            }
            if !self.selected_for_target(function) {
                if matches!(self.config.target_kind, MirRustTarget::WebWasm) {
                    continue;
                }
                panic!(
                    "MIR coverage point {:?} targets an unselected function",
                    point.id
                );
            }
            let _ = writeln!(
                out,
                "// jet-mir-coverage-point: id={} function={} block={} span={}..{}",
                point.id.0,
                self.function_name(point.function),
                point.block.0,
                point.span.start,
                point.span.end
            );
        }
        if selected_tests.is_empty() && harness.output_checks.is_empty() {
            if !self.artifact.jobs.is_empty() {
                self.emit_jobs(out);
            }
            return;
        }
        let _ = writeln!(out, "fn __jet_harness_run() -> bool {{");
        let _ = writeln!(out, "    jet_test_install_panic_hook();");
        let _ = writeln!(out, "    jet_test_trace_tier();");
        if self.coverage_enabled() {
            for (id, function) in self.coverage_branch_rows() {
                let _ = writeln!(
                    out,
                    "    {}jet_cov_register_branch({}, {});",
                    root,
                    quote_rust_string(&id),
                    quote_rust_string(&function),
                );
            }
        }
        let _ = writeln!(
            out,
            "    let mut __jet_cases: Vec<(&'static str, fn() -> JetTestOutcome)> = vec!["
        );
        for test_id in &selected_tests {
            let test = self
                .program
                .tests
                .iter()
                .find(|test| test.id == *test_id)
                .unwrap_or_else(|| panic!("MIR test ID {:?} has no row", test_id));
            let _ = writeln!(
                out,
                "        ({:?}, || jet_evidence_with_expectation({}, __jet_test_{})),",
                test.name, test.expected_failure, test.id.0
            );
        }
        let _ = writeln!(
            out,
            "    ];\n    if let Some(__jet_filter) = jet_test_filter() {{ __jet_cases.retain(|(__jet_name, _)| __jet_name.contains(&__jet_filter)); }}\n    let mut __jet_order: Vec<usize> = (0..__jet_cases.len()).collect();\n    if let Some(__jet_seed) = jet_test_shuffle_seed() {{ __jet_order = jet_test_shuffle_order(__jet_cases.len(), __jet_seed); }}\n    let __jet_serial = jet_test_serial();\n    let mut __jet_results = Vec::with_capacity(__jet_order.len());\n    if __jet_serial {{\n        for __jet_index in __jet_order {{ __jet_results.push((__jet_cases[__jet_index].1)()); }}\n    }} else {{\n        std::thread::scope(|__jet_scope| {{\n            let mut __jet_handles = Vec::new();\n            for __jet_index in __jet_order {{\n                let __jet_run = __jet_cases[__jet_index].1;\n                __jet_handles.push(__jet_scope.spawn(move || __jet_run()));\n            }}\n            for __jet_handle in __jet_handles {{ __jet_results.push(__jet_handle.join().unwrap()); }}\n        }});\n    }}"
        );
        for check in &harness.output_checks {
            let _ = writeln!(out, "    __jet_output_check_{}();", check.id.0);
        }
        if self.coverage_enabled() {
            let _ = writeln!(out, "    {}jet_cov_flush();", root);
        }
        let _ = writeln!(out, "    jet_test_finish(__jet_results)\n}}\n");
        if self.artifact.entry.is_none()
            && self.artifact.kind != MirArtifactKind::NativeLibrary
            && self.artifact.kind != MirArtifactKind::SandboxPlugin
        {
            let _ = writeln!(out, "fn main() {{");
            let _ = writeln!(out, "    {root}jet_std_env_init();");
            let _ = writeln!(
                out,
                "    {root}jet_gc::runtime_or_exit({root}jet_gc::initialize_trace());"
            );
            if !self.artifact.jobs.is_empty() {
                let _ = writeln!(
                    out,
                    "    let __argv = {root}jet_std_io_args();\n    if __jet_job_dispatch(&__argv, false) {{ return; }}"
                );
            }
            let _ = writeln!(
                out,
                "    {root}jet_runtime_boundary(|| {{ if !__jet_harness_run() {{ std::process::exit(1); }} }});"
            );
            let _ = writeln!(out, "}}\n");
        }
        if !self.artifact.jobs.is_empty() {
            self.emit_jobs(out);
        }
    }

    fn emit_function(&self, function: &MirFunction, out: &mut String) {
        self.emit_callable(function, out, None);
    }

    fn emit_method(&self, function: &MirFunction, out: &mut String) {
        if !matches!(&function.form, MirFunctionForm::Method { .. }) {
            panic!(
                "MIR method emitter received non-inherent function {:?}",
                function.id
            );
        }
        self.emit_callable(function, out, Some(&function.form));
    }

    fn emit_callable(
        &self,
        function: &MirFunction,
        out: &mut String,
        method_form: Option<&MirFunctionForm>,
    ) {
        if self.native_dispatch_eligible(function, method_form, None, None) {
            self.emit_native_dispatch(function, out);
        } else {
            self.emit_callable_named(function, out, method_form, None, None, None);
        }
    }

    fn emit_callable_named(
        &self,
        function: &MirFunction,
        out: &mut String,
        method_form: Option<&MirFunctionForm>,
        name_override: Option<&str>,
        serde_codec: Option<MirSerdeCodec>,
        return_override: Option<String>,
    ) {
        let previous_function = self.history_current_function.replace(Some(function.id));
        let owner_generic_params = match method_form {
            Some(MirFunctionForm::Method { owner, .. })
            | Some(MirFunctionForm::TraitMethod { owner, .. }) => {
                self.generic_params_for_type(owner)
            }
            _ => &[],
        };
        self.push_generic_scope(owner_generic_params);
        if function.is_inline_always {
            let _ = writeln!(out, "#[inline(always)]");
        } else if function.is_inline {
            let _ = writeln!(out, "#[inline]");
        }
        let visibility = if method_form.is_some() {
            ""
        } else {
            self.visibility(function.visibility)
        };
        let unsafe_prefix = if function.is_unsafe { "unsafe " } else { "" };
        let generics = self.generic_params(function);
        let previous_lifetime = self.history_callback_lifetime.replace("'__jet_callback");
        let mut params = Vec::new();
        if let Some(form) = method_form {
            match form {
                MirFunctionForm::Method { self_access, .. }
                | MirFunctionForm::TraitMethod { self_access, .. } => {
                    if let Some(access) = self_access {
                        params.push(match access {
                            MirAccess::Read => "&self".to_string(),
                            MirAccess::Write => "&mut self".to_string(),
                            MirAccess::Move => "mut self".to_string(),
                        });
                    }
                }
                MirFunctionForm::TopLevel => {
                    panic!(
                        "MIR callable method context has a top-level function {:?}",
                        function.id
                    );
                }
            }
        }
        params.extend(
            function
                .capture_params
                .iter()
                .enumerate()
                .map(|(slot, capture)| {
                    if capture.slot != slot {
                        panic!("MIR capture slots are not ordered");
                    }
                    format!(
                        "{}{}: {}",
                        if capture.access == MirAccess::Move {
                            "mut "
                        } else {
                            ""
                        },
                        self.capture_param_name(capture.slot),
                        self.capture_parameter_type(function, capture)
                    )
                }),
        );
        let declared = self.declared_params(function);
        if serde_codec == Some(MirSerdeCodec::Decode) && declared.len() != 1 {
            panic!(
                "MIR Decode method {:?} has {} parameters; expected one DataTree parameter",
                function.id,
                declared.len()
            );
        }
        params.extend(declared.iter().map(|param| {
            let name = mangle(&param.name);
            if serde_codec == Some(MirSerdeCodec::Decode) {
                format!("{name}: &{}jet_std::DataTree", self.config.root_prefix)
            } else if self.arithmetic_trait_method(function) && self.is_scalar(&param.ty) {
                format!("{name}: &{}", self.rust_parameter_type(&param.ty))
            } else {
                format!(
                    "{}{name}: {}",
                    if param.access == MirAccess::Move {
                        "mut "
                    } else {
                        ""
                    },
                    self.parameter_type(param)
                )
            }
        }));
        let params = params.join(", ");
        let ret = return_override.unwrap_or_else(|| self.rust_type(&function.return_type));
        self.history_callback_lifetime.set(previous_lifetime);
        // Forwarding or returning a callable preserves its capture lifetime.
        // Owned factories can instantiate this lifetime without borrowing.
        let generics = if params.contains("'__jet_callback") || ret.contains("'__jet_callback") {
            if generics.is_empty() {
                "<'__jet_callback>".to_string()
            } else {
                format!("<'__jet_callback, {}", &generics[1..])
            }
        } else {
            generics
        };
        let name = name_override.map(str::to_string).unwrap_or_else(|| {
            if method_form.is_some() {
                self.rust_method_symbol(function)
            } else {
                self.function_name(function.id)
            }
        });
        let _ = writeln!(
            out,
            "{visibility}{unsafe_prefix}fn {name}{generics}({params}) -> {ret} {{"
        );
        if self.arithmetic_trait_method(function) {
            for param in declared.iter().filter(|param| self.is_scalar(&param.ty)) {
                let name = mangle(&param.name);
                let _ = writeln!(out, "    let {name} = *{name};");
            }
        }
        if self.config.execution.emit_metadata {
            let _ = writeln!(
                out,
                "    // pure={} reactive={} scalar={} gc_return={}",
                function.is_pure, function.is_reactive, function.is_scalar, function.gc_return
            );
            if let Some(gate) = &function.unsafe_gate {
                let _ = writeln!(
                    out,
                    "    // unsafe gate {}:{} enabled={} fenced={}",
                    gate.file, gate.line, gate.enabled, gate.fenced
                );
            }
        }
        if self.history_runtime_metadata_enabled() {
            let _ = writeln!(
                out,
                "    let __jet_history_types = {}jet_history_current_types();",
                self.config.root_prefix
            );
        }
        self.emit_slots(function, out);
        let generator = function.generator.is_some();
        if generator {
            let _ = writeln!(
                out,
                "    {}jet_std::jet_stream_task(move |__jet_yield_tx| {{",
                self.config.root_prefix
            );
        }
        let body_indent = if generator { 8 } else { 4 };
        if self.coverage_for(function) {
            let line = self.coverage_function_line(function);
            let _ = writeln!(
                out,
                "{:indent$}{}jet_cov_function({line});",
                "",
                self.config.root_prefix,
                indent = body_indent
            );
        }
        let _ = writeln!(
            out,
            "{:indent$}let mut __jet_pc: u64 = {};",
            "",
            function.entry.0,
            indent = body_indent
        );
        let _ = writeln!(
            out,
            "{:indent$}let mut __jet_prev: u64 = 0;",
            "",
            indent = body_indent
        );
        let _ = writeln!(
            out,
            "{:indent$}'mir_dispatch: loop {{",
            "",
            indent = body_indent
        );
        let _ = writeln!(
            out,
            "{:indent$}match __jet_pc {{",
            "",
            indent = body_indent + 4
        );
        let debug_only_states = debug_only_block_states(function);
        let has_debug_only = function_has_debug_only(function);
        let has_test_scope = self.has_test_scope(function)
            && self.config.target_kind == MirRustTarget::Native;
        let has_expected_test_scope = self.has_expected_test_scope(function)
            && self.config.target_kind == MirRustTarget::Native;
        let test_scope_states = if has_test_scope {
            self.test_scope_block_states(function)
        } else {
            BTreeMap::new()
        };
        let expected_scope_exit_blocks = if has_expected_test_scope {
            self.expected_scope_exit_blocks(function, &test_scope_states)
        } else {
            BTreeMap::new()
        };
        for block in &function.blocks {
            let _ = writeln!(
                out,
                "{:indent$}{} => {{",
                "",
                block.id.0,
                indent = body_indent + 8
            );
            let block_debug_active = debug_only_states.get(&block.id).copied().unwrap_or(false);
            if self.has_hardware_interrupts() {
                let start = out.len();
                let _ = writeln!(
                    out,
                    "{:indent$}__jet_hardware_poll();",
                    "",
                    indent = body_indent + 12
                );
                if block_debug_active {
                    wrap_cfg_not_release(out, start, body_indent + 12);
                }
            }
            if !has_debug_only
                && !has_test_scope
                && self.emit_acceleration_block(function, block, out, body_indent + 12)
            {
                let _ = writeln!(out, "{:indent$}}}", "", indent = body_indent + 8);
                continue;
            }
            if !has_debug_only
                && !has_test_scope
                && self.emit_vector_block(function, block, out, body_indent + 12)
            {
                let _ = writeln!(out, "{:indent$}}}", "", indent = body_indent + 8);
                continue;
            }
            let mut debug_active = block_debug_active;
            if has_expected_test_scope {
                let _ = writeln!(
                    out,
                    "{:indent$}let __jet_scope_step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{",
                    "",
                    indent = body_indent + 12
                );
            }
            for instruction in &block.instructions {
                let is_debug_scope = match &instruction.operation {
                    MirOperation::ScopeEnter { scope, .. } | MirOperation::ScopeExit { scope } => {
                        function.scopes.iter().any(|candidate| {
                            candidate.id == *scope && candidate.kind == MirScopeKind::DebugOnly
                        })
                    }
                    _ => false,
                };
                match &instruction.operation {
                    MirOperation::ScopeEnter { .. } if is_debug_scope => {
                        debug_active = true;
                        continue;
                    }
                    MirOperation::ScopeExit { .. } if is_debug_scope => {
                        debug_active = false;
                        continue;
                    }
                    _ => {}
                }
                self.emit_instruction(function, instruction, out, body_indent + 12, debug_active);
            }
            let cleanup_return_in_catch = has_expected_test_scope
                && matches!(&block.terminator, MirTerminator::Return { .. });
            if cleanup_return_in_catch {
                if let Some(scopes) = test_scope_states.get(&block.id) {
                    self.emit_test_scope_exits(function, scopes, out, body_indent + 12);
                }
            }
            if has_expected_test_scope {
                let _ = writeln!(
                    out,
                    "{:indent$}}}));",
                    "",
                    indent = body_indent + 12
                );
                let _ = writeln!(
                    out,
                    "{:indent$}match __jet_scope_step {{",
                    "",
                    indent = body_indent + 12
                );
                let _ = writeln!(
                    out,
                    "{:indent$}Ok(()) => {{}},",
                    "",
                    indent = body_indent + 16
                );
                let _ = writeln!(
                    out,
                    "{:indent$}Err(__jet_payload) => {{",
                    "",
                    indent = body_indent + 16
                );
                let _ = writeln!(
                    out,
                    "{:indent$}if __jet_payload.is::<JetRenderedRuntimeStop>() {{",
                    "",
                    indent = body_indent + 20
                );
                let _ = writeln!(
                    out,
                    "{:indent$}if let Some(__jet_scope) = {}jet_test_expect_fail_matching_scope() {{",
                    "",
                    self.config.root_prefix,
                    indent = body_indent + 24
                );
                let _ = writeln!(
                    out,
                    "{:indent$}match __jet_scope {{",
                    "",
                    indent = body_indent + 28
                );
                for (scope, exit) in &expected_scope_exit_blocks {
                    let _ = writeln!(
                        out,
                        "{:indent$}{} => {{ {}jet_test_expect_fail_catch_scope(); __jet_pc = {}; continue 'mir_dispatch; }},",
                        "",
                        scope.0,
                        self.config.root_prefix,
                        exit.0,
                        indent = body_indent + 32
                    );
                }
                let _ = writeln!(
                    out,
                    "{:indent$}_ => std::panic::resume_unwind(__jet_payload),",
                    "",
                    indent = body_indent + 32
                );
                let _ = writeln!(
                    out,
                    "{:indent$}}}",
                    "",
                    indent = body_indent + 28
                );
                let _ = writeln!(
                    out,
                    "{:indent$}}} else {{ std::panic::resume_unwind(__jet_payload); }}",
                    "",
                    indent = body_indent + 24
                );
                let _ = writeln!(
                    out,
                    "{:indent$}}} else {{ std::panic::resume_unwind(__jet_payload); }}",
                    "",
                    indent = body_indent + 20
                );
                let _ = writeln!(
                    out,
                    "{:indent$}}},",
                    "",
                    indent = body_indent + 16
                );
                let _ = writeln!(
                    out,
                    "{:indent$}}}",
                    "",
                    indent = body_indent + 12
                );
            }
            let active_test_scopes = if cleanup_return_in_catch {
                None
            } else {
                test_scope_states.get(&block.id).map(Vec::as_slice)
            };
            self.emit_terminator(
                function,
                block,
                out,
                body_indent + 12,
                active_test_scopes,
            );
            let _ = writeln!(out, "{:indent$}}}", "", indent = body_indent + 8);
        }
        let _ = writeln!(
            out,
            "{:indent$}_ => unreachable!(\"invalid MIR block\"),",
            "",
            indent = body_indent + 8
        );
        let _ = writeln!(out, "{:indent$}}}", "", indent = body_indent + 4);
        let _ = writeln!(out, "{:indent$}}}", "", indent = body_indent);
        if generator {
            let _ = writeln!(out, "    }})");
        }
        let _ = writeln!(out, "}}\n");
        self.pop_generic_scope();
        self.history_current_function.set(previous_function);
    }

    fn emit_acceleration_block(
        &self,
        function: &MirFunction,
        block: &MirBasicBlock,
        out: &mut String,
        indent: usize,
    ) -> bool {
        if self.coverage_enabled()
            || self.config.target_kind != MirRustTarget::Native
            || self.is_no_os()
            || function.is_scalar
            || function.is_reactive
            || !function.reactive_upgrades.is_empty()
        {
            return false;
        }
        let Some(vector) = function.optimization.vector_facts.iter().find(|fact| {
            fact.loop_header == block.id
                && fact.decision.is_eligible()
                && matches!(
                    fact.rule,
                    MirVectorRule::Elementwise | MirVectorRule::FieldAccess
                )
        }) else {
            return false;
        };
        if block.instructions.iter().any(|instruction| {
            matches!(&instruction.operation, MirOperation::InitializeUninit { .. })
        }) || vector
            .body_blocks
            .iter()
            .filter_map(|block_id| function.blocks.iter().find(|candidate| candidate.id == *block_id))
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                matches!(&instruction.operation, MirOperation::InitializeUninit { .. })
            })
        {
            return false;
        }
        let Some(parallel) = function
            .optimization
            .acceleration_facts
            .iter()
            .find(|fact| {
                fact.loop_header == block.id
                    && fact.transform == AccelerationTransform::PooledParallelChunks
                    && fact
                        .proof
                        .proves(AccelerationTransform::PooledParallelChunks)
            })
        else {
            return false;
        };
        let Some(column) = function
            .optimization
            .acceleration_facts
            .iter()
            .find(|fact| {
                fact.loop_header == block.id
                    && fact.transform == AccelerationTransform::TransientColumnCopy
                    && fact
                        .proof
                        .proves(AccelerationTransform::TransientColumnCopy)
            })
        else {
            return false;
        };
        let Some(row) = function
            .optimization
            .loop_facts
            .iter()
            .find(|candidate| candidate.header == block.id && candidate.canonical)
        else {
            return false;
        };
        let Some(cursor) = vector.cursor else {
            return false;
        };
        let Some(init) = function
            .blocks
            .iter()
            .flat_map(|candidate| candidate.instructions.iter())
            .find(|instruction| {
                instruction.result == Some(cursor)
                    && matches!(&instruction.operation, MirOperation::LoopRangeInit { .. })
            })
        else {
            return false;
        };
        let MirOperation::LoopRangeInit {
            start,
            end,
            exclusive: true,
            ..
        } = &init.operation
        else {
            return false;
        };
        let Some(exit) = row.exit else {
            return false;
        };
        let Some((outputs, loop_places)) =
            self.acceleration_output_writes(function, vector, cursor)
        else {
            return false;
        };
        let column_slots = self.acceleration_column_slots(vector);
        let use_column_copy = column.workload.column_copy_applicable()
            && column_slots.as_ref().is_some_and(|slots| !slots.is_empty());
        let Some(source) = self.acceleration_loop_source(
            function,
            vector,
            cursor,
            *start,
            *end,
            &outputs,
            &loop_places,
            parallel,
            column,
            column_slots.as_ref(),
            use_column_copy,
            exit,
            indent,
        ) else {
            return false;
        };
        out.push_str(&source);
        true
    }

    fn acceleration_loop_source(
        &self,
        function: &MirFunction,
        vector: &MirVectorFact,
        cursor: MirValueId,
        start: MirValueId,
        end: MirValueId,
        outputs: &[(MirPlaceId, MirValueId)],
        loop_places: &BTreeSet<MirPlaceId>,
        parallel: &MirAccelerationFact,
        column: &MirAccelerationFact,
        column_slots: Option<&BTreeMap<(MirValueId, MirFieldId), usize>>,
        use_column_copy: bool,
        exit: MirBlockId,
        indent: usize,
    ) -> Option<String> {
        let pad = " ".repeat(indent);
        let start_expr = self.value_read(start);
        let end_expr = self.value_read(end);
        let function_key = format!("{:?}", function.key);
        let loop_header = vector.loop_header.0 as u32;
        let source_start = vector.span.start;
        let source_end = vector.span.end;
        let proof_parallel = parallel
            .proof
            .proves(AccelerationTransform::PooledParallelChunks);
        let proof_column = column
            .proof
            .proves(AccelerationTransform::TransientColumnCopy);
        let cross_mode_parity_parallel = vector.no_aliasing
            && vector.no_early_exit
            && vector.effect_free_body
            && vector.no_cross_iteration_dependencies
            && proof_parallel;
        let cross_mode_parity_column = vector.no_aliasing
            && vector.no_early_exit
            && vector.effect_free_body
            && vector.no_cross_iteration_dependencies
            && proof_column;
        let nested_reuse = column.workload.nested_reuse;
        let single_pass = column.workload.single_pass;
        // The gate asks whether the emitted binary runs under a release
        // profile, so the answer is decided by the emitted program itself.
        let release = "cfg!(not(debug_assertions))";
        let mut source = String::new();
        let _ = writeln!(
            source,
            "{pad}let __jet_accel_start = {start_expr};\n{pad}let __jet_accel_end = {end_expr};\n{pad}let __jet_accel_start_index = (__jet_accel_start as usize);\n{pad}let __jet_accel_range = (__jet_accel_start_index..(__jet_accel_end as usize));"
        );
        if use_column_copy {
            let Some(slots) = column_slots else {
                return None;
            };
            let Some((sample_body, sample_result)) = self.acceleration_callback_body(
                function,
                vector,
                cursor,
                outputs,
                loop_places,
                "(__jet_accel_index as _)",
                None,
                indent + 16,
            ) else {
                return None;
            };
            let Some((copied_body, copied_result)) = self.acceleration_callback_body(
                function,
                vector,
                cursor,
                outputs,
                loop_places,
                "(__jet_accel_index as _)",
                Some(("__jet_accel_columns", "__jet_accel_offset", slots)),
                indent + 20,
            ) else {
                return None;
            };
            let Some((original_body, original_result)) = self.acceleration_callback_body(
                function,
                vector,
                cursor,
                outputs,
                loop_places,
                "(__jet_accel_index as _)",
                None,
                indent + 12,
            ) else {
                return None;
            };
            let _ = writeln!(
                source,
                "{pad}let (__jet_accel_column_sample, __jet_accel_column_decision) = if !{release} {{"
            );
            let _ = writeln!(
                source,
                "{pad}    let mut __jet_accel_input = JetAccelerationGateInput::deferred(__jet_accel_range.len(), {release});"
            );
            let _ = writeln!(
                source,
                "{pad}    __jet_accel_input.cross_mode_parity_proven = {cross_mode_parity_column};"
            );
            let _ = writeln!(
                source,
                "{pad}    __jet_accel_input.nested_reuse = {nested_reuse};"
            );
            let _ = writeln!(
                source,
                "{pad}    __jet_accel_input.single_pass = {single_pass};"
            );
            let _ = writeln!(
                source,
                "{pad}    __jet_accel_input.proof_proven = {proof_column};"
            );
            let _ = writeln!(
                source,
                "{pad}    (None, JetAccelerationGate::d_accel1().evaluate(JetAccelerationTransform::TransientColumnCopy, __jet_accel_input))"
            );
            let _ = writeln!(source, "{pad}}} else {{");
            let _ = writeln!(
                source,
                "{pad}    match jet_acceleration_first_chunk(__jet_accel_range.len(), |__jet_accel_sample_range| {{"
            );
            let _ = writeln!(
                source,
                "{pad}        __jet_accel_sample_range.map(|__jet_accel_offset| {{"
            );
            let _ = writeln!(
                source,
                "{pad}            let __jet_accel_index = __jet_accel_start_index + __jet_accel_offset;"
            );
            source.push_str(&sample_body);
            let _ = writeln!(source, "{pad}            {sample_result}");
            let _ = writeln!(source, "{pad}        }}).collect::<Vec<_>>()");
            let _ = writeln!(source, "{pad}    }}) {{");
            let _ = writeln!(source, "{pad}        Some(__jet_accel_sample) => {{");
            let _ = writeln!(
                source,
                "{pad}            let __jet_accel_costs = JetAccelerationRuntimeCosts::once(__jet_accel_range.len());"
            );
            let _ = writeln!(
                source,
                "{pad}            let __jet_accel_input = JetAccelerationGateInput::measured(__jet_accel_range.len(), __jet_accel_sample.sample_items, __jet_accel_sample.sample_nanos, __jet_accel_costs, {release}, {cross_mode_parity_column}, {proof_column}, {nested_reuse}, {single_pass}, None);"
            );
            let _ = writeln!(
                source,
                "{pad}            let __jet_accel_decision = JetAccelerationGate::d_accel1().evaluate(JetAccelerationTransform::TransientColumnCopy, __jet_accel_input);"
            );
            let _ = writeln!(
                source,
                "{pad}            (Some(__jet_accel_sample), __jet_accel_decision)"
            );
            let _ = writeln!(source, "{pad}        }}");
            let _ = writeln!(source, "{pad}        None => {{");
            let _ = writeln!(
                source,
                "{pad}            let mut __jet_accel_input = JetAccelerationGateInput::deferred(__jet_accel_range.len(), {release});"
            );
            let _ = writeln!(
                source,
                "{pad}            __jet_accel_input.cross_mode_parity_proven = {cross_mode_parity_column};"
            );
            let _ = writeln!(
                source,
                "{pad}            __jet_accel_input.nested_reuse = {nested_reuse};"
            );
            let _ = writeln!(
                source,
                "{pad}            __jet_accel_input.single_pass = {single_pass};"
            );
            let _ = writeln!(
                source,
                "{pad}            __jet_accel_input.proof_proven = {proof_column};"
            );
            let _ = writeln!(
                source,
                "{pad}            (None, JetAccelerationGate::d_accel1().evaluate(JetAccelerationTransform::TransientColumnCopy, __jet_accel_input))"
            );
            let _ = writeln!(source, "{pad}        }}");
            let _ = writeln!(source, "{pad}    }}");
            let _ = writeln!(source, "{pad}}};");
            let _ = writeln!(
                source,
                "{pad}jet_acceleration_publish({function_key}, {loop_header}, {source_start}, {source_end}, &__jet_accel_column_decision);"
            );
            let project = slots
                .iter()
                .map(|((collection, field), _column_slot)| {
                    let access = vector.accesses.iter().find(|access| {
                        access.field == Some(*field)
                            && access.column_index.is_some()
                            && matches!(
                                access.root,
                                MirVectorAccessRoot::Value(value) if value == *collection
                            )
                    })?;
                    let column_index = access.column_index?;
                    Some(format!(
                        "({}).column({column_index})[((__jet_accel_source_index) as usize)].clone().jet_col_{}()",
                        self.value_read(*collection),
                        self.field_name(*field)
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            let project = if project.len() == 1 {
                format!("({},)", project[0])
            } else {
                format!("({},)", project.join(", "))
            };
            let _ = writeln!(
                source,
                "{pad}let __jet_accel_results = jet_list_accel_column_range_map_with_sample("
            );
            let _ = writeln!(
                source,
                "{pad}    __jet_accel_range.clone(),\n{pad}    __jet_accel_column_sample,\n{pad}    __jet_accel_column_decision.selected(),"
            );
            let _ = writeln!(source, "{pad}    |__jet_accel_source_index| {project},");
            let _ = writeln!(
                source,
                "{pad}    |__jet_accel_columns, __jet_accel_range| {{\n{pad}        let __jet_accel_block_start = __jet_accel_range.start;\n{pad}        __jet_accel_range.map(|__jet_accel_absolute| {{\n{pad}            let __jet_accel_offset = __jet_accel_absolute - __jet_accel_block_start;\n{pad}            let __jet_accel_index = __jet_accel_absolute;"
            );
            source.push_str(&copied_body);
            let _ = writeln!(
                source,
                "{pad}            {copied_result}\n{pad}        }}).collect()\n{pad}    }},"
            );
            let _ = writeln!(
                source,
                "{pad}    |__jet_accel_source_range| {{\n{pad}        __jet_accel_source_range.map(|__jet_accel_index| {{"
            );
            source.push_str(&original_body);
            let _ = writeln!(
                source,
                "{pad}            {original_result}\n{pad}        }}).collect()\n{pad}    }},\n{pad});"
            );
        } else {
            let Some((body, result)) = self.acceleration_callback_body(
                function,
                vector,
                cursor,
                outputs,
                loop_places,
                "(__jet_accel_index as _)",
                None,
                indent + 8,
            ) else {
                return None;
            };
            let _ = writeln!(
                source,
                "{pad}let (__jet_accel_results, __jet_accel_parallel_decision) = jet_list_accel_range_map(\n{pad}    __jet_accel_range.clone(),\n{pad}    std::thread::available_parallelism().map(|workers| workers.get()).unwrap_or(1),\n{pad}    {release},\n{pad}    {cross_mode_parity_parallel},\n{pad}    {proof_parallel},\n{pad}    None,\n{pad}    |__jet_accel_index| {{"
            );
            source.push_str(&body);
            let _ = writeln!(
                source,
                "{pad}        {result}\n{pad}    }}\n{pad});\n{pad}jet_acceleration_publish({function_key}, {loop_header}, {source_start}, {source_end}, &__jet_accel_parallel_decision);"
            );
        }
        let _ = writeln!(
            source,
            "{pad}for (__jet_accel_offset, __jet_accel_result) in __jet_accel_results.into_iter().enumerate() {{\n{pad}    let __jet_accel_index = __jet_accel_start_index + __jet_accel_offset;"
        );
        let names = outputs
            .iter()
            .enumerate()
            .map(|(index, _)| format!("__jet_accel_output_{index}"))
            .collect::<Vec<_>>();
        let _ = writeln!(
            source,
            "{pad}    let ({},) = __jet_accel_result;",
            names.join(", ")
        );
        for (index, (place, _)) in outputs.iter().enumerate() {
            let write = self.acceleration_write_place_expr(
                function,
                *place,
                "__jet_accel_index",
                &names[index],
            )?;
            let _ = writeln!(source, "{pad}    {write};");
        }
        let _ = writeln!(source, "{pad}}}");
        self.set_pc(vector.loop_header, exit, &mut source, indent);
        Some(source)
    }

    fn acceleration_output_writes(
        &self,
        function: &MirFunction,
        vector: &MirVectorFact,
        cursor: MirValueId,
    ) -> Option<(Vec<(MirPlaceId, MirValueId)>, BTreeSet<MirPlaceId>)> {
        let mut outputs = Vec::new();
        let mut loop_places = BTreeSet::new();
        for block_id in &vector.body_blocks {
            let block = function.blocks.iter().find(|block| block.id == *block_id)?;
            if !matches!(block.terminator, MirTerminator::Jump { .. }) {
                return None;
            }
            for instruction in &block.instructions {
                let MirOperation::WritePlace { place, value } = &instruction.operation else {
                    continue;
                };
                let row = function
                    .places
                    .iter()
                    .find(|candidate| candidate.id == *place)?;
                let cursor_index = row.projections.iter().enumerate().find_map(|(index, projection)| {
                    matches!(
                        projection,
                        MirProjection::Index { index: candidate, kind: MirIndexKind::FixedListProof, .. }
                            if *candidate == cursor
                    )
                    .then_some(index)
                });
                if let Some(index) = cursor_index {
                    if row.projections[..index]
                        .iter()
                        .any(|projection| matches!(projection, MirProjection::Index { .. }))
                        || row.projections[index + 1..]
                            .iter()
                            .any(|projection| !matches!(projection, MirProjection::Field { .. }))
                    {
                        return None;
                    }
                    outputs.push((*place, *value));
                } else if row.projections.is_empty()
                    && self.acceleration_is_cursor_value(function, *value, cursor)
                {
                    loop_places.insert(*place);
                } else {
                    return None;
                }
            }
        }
        (!outputs.is_empty()).then_some((outputs, loop_places))
    }

    fn acceleration_is_cursor_value(
        &self,
        function: &MirFunction,
        value: MirValueId,
        cursor: MirValueId,
    ) -> bool {
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                instruction.result == Some(value)
                    && matches!(
                        &instruction.operation,
                        MirOperation::LoopRangeValue { cursor: candidate, .. }
                            if *candidate == cursor
                    )
            })
    }
    fn acceleration_column_slots(
        &self,
        vector: &MirVectorFact,
    ) -> Option<BTreeMap<(MirValueId, MirFieldId), usize>> {
        let mut slots = BTreeMap::new();
        for access in &vector.accesses {
            let (MirVectorAccessRoot::Value(collection), Some(field), Some(_)) =
                (access.root, access.field, access.column_index)
            else {
                continue;
            };
            if access.layout != MirVectorLayout::ColumnarDirect {
                continue;
            }
            if !slots.contains_key(&(collection, field)) {
                let slot = slots.len();
                slots.insert((collection, field), slot);
            }
        }
        Some(slots)
    }

    fn acceleration_callback_body(
        &self,
        function: &MirFunction,
        vector: &MirVectorFact,
        cursor: MirValueId,
        outputs: &[(MirPlaceId, MirValueId)],
        loop_places: &BTreeSet<MirPlaceId>,
        index_expr: &str,
        column_context: Option<(&str, &str, &BTreeMap<(MirValueId, MirFieldId), usize>)>,
        indent: usize,
    ) -> Option<(String, String)> {
        let mut defined = BTreeMap::new();
        let mut source = String::new();
        let mut values = Vec::with_capacity(outputs.len());
        for (_, value) in outputs {
            values.push(self.acceleration_value_expr(
                function,
                *value,
                vector,
                cursor,
                loop_places,
                index_expr,
                column_context,
                &mut defined,
                &mut source,
                indent,
            )?);
        }
        let result = format!("({},)", values.join(", "));
        Some((source, result))
    }

    fn acceleration_value_expr(
        &self,
        function: &MirFunction,
        value: MirValueId,
        vector: &MirVectorFact,
        cursor: MirValueId,
        loop_places: &BTreeSet<MirPlaceId>,
        index_expr: &str,
        column_context: Option<(&str, &str, &BTreeMap<(MirValueId, MirFieldId), usize>)>,
        defined: &mut BTreeMap<MirValueId, String>,
        source: &mut String,
        indent: usize,
    ) -> Option<String> {
        if let Some(name) = defined.get(&value) {
            return Some(name.clone());
        }
        let instruction = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(value))?;
        let expression = match &instruction.operation {
            MirOperation::Constant(constant) => {
                self.constant_for_type(constant, self.value_type(function, value))
            }
            MirOperation::LoopRangeValue {
                cursor: candidate, ..
            } if *candidate == cursor => index_expr.to_string(),
            MirOperation::ReadPlace(place) => self.acceleration_place_expr(
                function,
                *place,
                vector,
                cursor,
                loop_places,
                index_expr,
                column_context,
            )?,
            MirOperation::Copy { value } | MirOperation::AttachTag { value, .. } => self
                .acceleration_value_expr(
                    function,
                    *value,
                    vector,
                    cursor,
                    loop_places,
                    index_expr,
                    column_context,
                    defined,
                    source,
                    indent,
                )?,
            MirOperation::Index {
                base,
                index,
                kind: MirIndexKind::FixedListProof,
                ..
            } if *index == cursor && column_context.is_none() => format!(
                "({})[(({} ) as usize)].clone()",
                self.value_read(*base),
                index_expr
            ),
            MirOperation::Field { base, field } => self.acceleration_field_expr(
                function,
                *base,
                *field,
                vector,
                cursor,
                loop_places,
                index_expr,
                column_context,
                defined,
                source,
                indent,
            )?,
            MirOperation::Unary { op, value } => {
                let operand = self.acceleration_value_expr(
                    function,
                    *value,
                    vector,
                    cursor,
                    loop_places,
                    index_expr,
                    column_context,
                    defined,
                    source,
                    indent,
                )?;
                self.unary(*op, operand, self.value_type(function, *value))
            }
            MirOperation::Binary {
                op,
                dispatch,
                left,
                right,
            } => {
                let left = self.acceleration_value_expr(
                    function,
                    *left,
                    vector,
                    cursor,
                    loop_places,
                    index_expr,
                    column_context,
                    defined,
                    source,
                    indent,
                )?;
                let right = self.acceleration_value_expr(
                    function,
                    *right,
                    vector,
                    cursor,
                    loop_places,
                    index_expr,
                    column_context,
                    defined,
                    source,
                    indent,
                )?;
                self.acceleration_binary(*op, dispatch, left, right)
            }
            MirOperation::Semantic(MirSemanticOp::ColumnarRead {
                base,
                column,
                column_index,
                index,
                ..
            }) if *index == cursor => {
                if let Some((columns, offset, slots)) = column_context {
                    if let Some(slot) = slots.get(&(*base, *column)) {
                        format!("({columns}[{offset}].{slot}).clone()")
                    } else {
                        return None;
                    }
                } else {
                    format!(
                        "({}).column({column_index})[(({} ) as usize)].clone().jet_col_{}()",
                        self.value_read(*base),
                        index_expr,
                        self.field_name(*column)
                    )
                }
            }
            MirOperation::Parameter { index, .. } => self.parameter_expression(function, *index),
            MirOperation::Capture { slot } => self.capture_expression(function, *slot),
            MirOperation::Global { name } => self.global_expression(name),
            _ => return None,
        };
        let name = format!("__jet_accel_v{}", value.0);
        let ty = self.rust_type(self.value_type(function, value));
        let _ = writeln!(
            source,
            "{:indent$}let {name}: {ty} = {expression};",
            "",
            indent = indent
        );
        defined.insert(value, name.clone());
        Some(name)
    }

    fn acceleration_field_expr(
        &self,
        function: &MirFunction,
        base: MirValueId,
        field: MirFieldId,
        vector: &MirVectorFact,
        cursor: MirValueId,
        loop_places: &BTreeSet<MirPlaceId>,
        index_expr: &str,
        column_context: Option<(&str, &str, &BTreeMap<(MirValueId, MirFieldId), usize>)>,
        defined: &mut BTreeMap<MirValueId, String>,
        source: &mut String,
        indent: usize,
    ) -> Option<String> {
        let base_instruction = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(base))?;
        if let MirOperation::Index {
            base: collection,
            index,
            kind: MirIndexKind::FixedListProof,
            ..
        } = &base_instruction.operation
        {
            if *index != cursor {
                return None;
            }
            if let Some((columns, offset, slots)) = column_context {
                if let Some(slot) = slots.get(&(*collection, field)) {
                    return Some(format!("({columns}[{offset}].{slot}).clone()"));
                }
                return None;
            }
            if let Some(access) = vector.accesses.iter().find(|access| {
                access.field == Some(field)
                    && matches!(
                        access.root,
                        MirVectorAccessRoot::Value(value) if value == *collection
                    )
            }) {
                if access.layout == MirVectorLayout::ColumnarDirect {
                    let column = access.column_index?;
                    return Some(format!(
                        "({}).column({column})[(({} ) as usize)].clone().jet_col_{}()",
                        self.value_read(*collection),
                        index_expr,
                        self.field_name(field)
                    ));
                }
            }
            return Some(format!(
                "({})[(({} ) as usize)].{}.clone()",
                self.value_read(*collection),
                index_expr,
                self.field_name(field)
            ));
        }
        let base = self.acceleration_value_expr(
            function,
            base,
            vector,
            cursor,
            loop_places,
            index_expr,
            column_context,
            defined,
            source,
            indent,
        )?;
        Some(format!("({base}).{}.clone()", self.field_name(field)))
    }
    fn acceleration_place_expr(
        &self,
        function: &MirFunction,
        place_id: MirPlaceId,
        _vector: &MirVectorFact,
        cursor: MirValueId,
        loop_places: &BTreeSet<MirPlaceId>,
        index_expr: &str,
        column_context: Option<(&str, &str, &BTreeMap<(MirValueId, MirFieldId), usize>)>,
    ) -> Option<String> {
        let place = function.places.iter().find(|place| place.id == place_id)?;
        if place.projections.is_empty() {
            if loop_places.contains(&place_id) {
                return Some(index_expr.to_string());
            }
            return Some(self.place_read(function, place_id));
        }
        let index_pos = place.projections.iter().position(|projection| {
            matches!(
                projection,
                MirProjection::Index {
                    kind: MirIndexKind::FixedListProof,
                    index,
                    ..
                } if *index == cursor
            )
        })?;
        if place.projections[..index_pos]
            .iter()
            .any(|projection| matches!(projection, MirProjection::Index { .. }))
            || place.projections[index_pos + 1..]
                .iter()
                .any(|projection| !matches!(projection, MirProjection::Field { .. }))
        {
            return None;
        }
        if column_context.is_some() {
            return None;
        }
        let base = self.place_base(
            function,
            &place.base,
            false,
            &place.projections[..index_pos],
        );
        let suffix = place.projections[index_pos + 1..]
            .iter()
            .map(|projection| match projection {
                MirProjection::Field { field, .. } => Some(format!(".{}", self.field_name(*field))),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?
            .join("");
        Some(format!(
            "({base})[(({} ) as usize)]{suffix}.clone()",
            index_expr
        ))
    }

    fn acceleration_binary(
        &self,
        op: MirBinaryOp,
        dispatch: &MirBinaryDispatch,
        left: String,
        right: String,
    ) -> String {
        match dispatch {
            MirBinaryDispatch::Primitive => self.binary(op, left, right),
            MirBinaryDispatch::Prelude { call, location } => {
                let mut args = vec![left, right];
                let extras = location
                    .as_ref()
                    .map(|location| {
                        vec![
                            format!("{:?}", self.source_file_path(location.file)),
                            format!("{}u32", location.line),
                        ]
                    })
                    .unwrap_or_default();
                self.append_prelude_context(*call, &mut args, &extras);
                let row = self.prelude_row(*call);
                self.validate_prelude_count(row, args.len());
                let args = args
                    .into_iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        if row.signature.borrow_mask[index] {
                            format!("&({arg})")
                        } else {
                            arg
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({args})", self.prelude_symbol(*call))
            }
        }
    }

    fn acceleration_write_place_expr(
        &self,
        function: &MirFunction,
        place_id: MirPlaceId,
        index_expr: &str,
        value: &str,
    ) -> Option<String> {
        let place = function.places.iter().find(|place| place.id == place_id)?;
        let index_pos = place.projections.iter().position(|projection| {
            matches!(
                projection,
                MirProjection::Index {
                    kind: MirIndexKind::FixedListProof,
                    ..
                }
            )
        })?;
        let base = self.place_base(function, &place.base, true, &place.projections[..index_pos]);
        let suffix = place.projections[index_pos + 1..]
            .iter()
            .map(|projection| match projection {
                MirProjection::Field { field, .. } => Some(format!(".{}", self.field_name(*field))),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?
            .join("");
        if suffix.is_empty() {
            return None;
        }
        Some(format!(
            "({base})[(({} ) as usize)]{suffix} = {value}",
            index_expr
        ))
    }

    fn emit_vector_block(
        &self,
        function: &MirFunction,
        block: &MirBasicBlock,
        out: &mut String,
        indent: usize,
    ) -> bool {
        if self.coverage_enabled() || function.is_scalar {
            return false;
        }
        let Some(fact) = function
            .optimization
            .vector_facts
            .iter()
            .find(|fact| fact.loop_header == block.id && fact.decision.is_eligible())
        else {
            return false;
        };
        if block.instructions.iter().any(|instruction| {
            matches!(&instruction.operation, MirOperation::InitializeUninit { .. })
        }) || fact
            .body_blocks
            .iter()
            .filter_map(|block_id| function.blocks.iter().find(|candidate| candidate.id == *block_id))
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                matches!(&instruction.operation, MirOperation::InitializeUninit { .. })
            })
        {
            return false;
        }
        match fact.rule {
            MirVectorRule::Elementwise | MirVectorRule::FieldAccess => {}
            MirVectorRule::ConditionalAccumulate => {
                if !D_FRED1_FIXED_ORDER.is_canonical() {
                    panic!("MIR conditional vector emission requires canonical D-FRED1 order");
                }
                if self.vector_rmw_plan(function, fact).is_none() {
                    panic!("MIR conditional vector fact has no canonical accumulator plan");
                }
            }
            MirVectorRule::EarlyExitSearch => {}
            MirVectorRule::Reduction => {
                if !D_FRED1_FIXED_ORDER.is_canonical() {
                    panic!("MIR reduction vector emission requires canonical D-FRED1 order");
                }
                if fact.fixed_reduction.is_none() {
                    panic!("MIR reduction vector fact is missing its fixed reduction row");
                }
            }
        }
        let Some(row) = function
            .optimization
            .loop_facts
            .iter()
            .find(|row| row.header == block.id)
        else {
            return false;
        };
        let Some(cursor) = fact.cursor else {
            return false;
        };
        let Some(width) = fact.lane_width else {
            return false;
        };
        let Some(init) = function
            .blocks
            .iter()
            .flat_map(|candidate| candidate.instructions.iter())
            .find(|instruction| {
                instruction.result == Some(cursor)
                    && matches!(&instruction.operation, MirOperation::LoopRangeInit { .. })
            })
        else {
            return false;
        };
        let MirOperation::LoopRangeInit {
            start,
            end,
            exclusive: true,
            ..
        } = &init.operation
        else {
            return false;
        };
        let Some(exit) = fact
            .fixed_reduction
            .as_ref()
            .map(|fixed| fixed.exit)
            .or(row.exit)
        else {
            return false;
        };
        let source = match width {
            2 => self.vector_loop_source::<2>(function, fact, cursor, *start, *end, exit, indent),
            4 => self.vector_loop_source::<4>(function, fact, cursor, *start, *end, exit, indent),
            8 => self.vector_loop_source::<8>(function, fact, cursor, *start, *end, exit, indent),
            _ => None,
        };
        let Some(source) = source else {
            return false;
        };
        out.push_str(&source);
        true
    }

    fn vector_loop_source<const N: usize>(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
        cursor: MirValueId,
        start: MirValueId,
        end: MirValueId,
        exit: MirBlockId,
        indent: usize,
    ) -> Option<String> {
        let mut loop_places = BTreeSet::new();
        for block_id in &fact.body_blocks {
            let block = function.blocks.iter().find(|block| block.id == *block_id)?;
            for instruction in &block.instructions {
                let MirOperation::WritePlace { place, value } = &instruction.operation else {
                    continue;
                };
                if self.vector_is_loop_value(function, *value, cursor) {
                    loop_places.insert(*place);
                }
            }
        }
        let rmw = if matches!(
            fact.rule,
            MirVectorRule::ConditionalAccumulate | MirVectorRule::Reduction
        ) {
            Some(
                self.vector_rmw_plan(function, fact)
                    .unwrap_or_else(|| panic!("MIR vector fact has no canonical accumulator plan")),
            )
        } else {
            None
        };
        let condition = if fact.rule == MirVectorRule::ConditionalAccumulate {
            Some(self.vector_condition(function, fact)?)
        } else {
            None
        };
        let early = if fact.rule == MirVectorRule::EarlyExitSearch {
            Some(self.vector_early_return(function, fact)?)
        } else {
            None
        };
        let chunk = self.vector_chunk_source::<N>(
            function,
            fact,
            cursor,
            &loop_places,
            rmw.as_ref(),
            condition,
            early.as_ref(),
            "__jet_vec_index",
            "__jet_vec_start",
            indent + 4,
        )?;
        let tail = self.vector_chunk_source::<1>(
            function,
            fact,
            cursor,
            &loop_places,
            rmw.as_ref(),
            condition,
            early.as_ref(),
            "__jet_vec_index",
            "__jet_vec_start",
            indent + 4,
        )?;
        let pad = " ".repeat(indent);
        let mut out = String::new();
        let start = self.value_read(start);
        let end = self.value_read(end);
        let _ = writeln!(out, "{pad}let __jet_vec_start = {start};");
        let _ = writeln!(out, "{pad}let mut __jet_vec_index = __jet_vec_start;");
        let _ = writeln!(out, "{pad}let __jet_vec_end = {end};");
        if let Some((place, _, seed)) = rmw.as_ref() {
            let row = function
                .places
                .iter()
                .find(|candidate| candidate.id == *place)?;
            let ty = self.rust_type(&row.ty);
            let seed = self.vector_seed_expr(function, *seed);
            let _ = writeln!(out, "{pad}let __jet_vec_seed: {ty} = {seed};");
            let _ = writeln!(
                out,
                "{pad}let mut __jet_vec_acc: [{ty}; 8] = jet_simd_seed_fixed(__jet_vec_seed);"
            );
            let _ = writeln!(out, "{pad}let mut __jet_vec_seen = false;");
        }
        let default_int_range = self.vector_default_int_range(function, fact);
        let (chunk_condition, chunk_advance, tail_condition, tail_advance) = if default_int_range {
            let width = format!("{}jet_std::jet_int_from_i64({N})", self.config.root_prefix);
            (
                format!(
                    "{}jet_std::jet_int_compare(__jet_vec_index, __jet_vec_end) < 0 \
                     && {}jet_std::jet_int_compare({}jet_std::jet_int_add(__jet_vec_index, {width}), __jet_vec_end) <= 0",
                    self.config.root_prefix,
                    self.config.root_prefix,
                    self.config.root_prefix,
                ),
                format!(
                    "{}jet_std::jet_int_add(__jet_vec_index, {width})",
                    self.config.root_prefix
                ),
                format!(
                    "{}jet_std::jet_int_compare(__jet_vec_index, __jet_vec_end) < 0",
                    self.config.root_prefix
                ),
                format!(
                    "{}jet_std::jet_int_add(__jet_vec_index, {}jet_std::jet_int_from_i64(1))",
                    self.config.root_prefix,
                    self.config.root_prefix,
                ),
            )
        } else {
            (
                format!("__jet_vec_index < __jet_vec_end && __jet_vec_index.abs_diff(__jet_vec_end) >= {N}"),
                format!("__jet_vec_index + {N}"),
                "__jet_vec_index < __jet_vec_end".to_string(),
                "__jet_vec_index + 1".to_string(),
            )
        };
        let _ = writeln!(out, "{pad}while {chunk_condition} {{");
        out.push_str(&chunk);
        let _ = writeln!(out, "{pad}    __jet_vec_index = {chunk_advance};");
        let _ = writeln!(out, "{pad}}}");
        let _ = writeln!(out, "{pad}while {tail_condition} {{");
        out.push_str(&tail);
        let _ = writeln!(out, "{pad}    __jet_vec_index = {tail_advance};");
        let _ = writeln!(out, "{pad}}}");
        if let Some((place, _, _)) = rmw {
            let row = function
                .places
                .iter()
                .find(|candidate| candidate.id == place)?;
            let ty = self.rust_type(&row.ty);
            let _ = writeln!(
                out,
                "{pad}let __jet_vec_result: {ty} = if __jet_vec_seen {{ jet_simd_finish_fixed(&__jet_vec_acc) }} else {{ __jet_vec_seed }};"
            );
            let _ = writeln!(
                out,
                "{pad}{};",
                self.write_place_expr(function, place, None, "__jet_vec_result")
            );
        }
        self.set_pc(fact.loop_header, exit, &mut out, indent);
        Some(out)
    }

    fn vector_chunk_source<const N: usize>(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
        _cursor: MirValueId,
        loop_places: &BTreeSet<MirPlaceId>,
        rmw: Option<&(MirPlaceId, MirValueId, MirValueId)>,
        condition: Option<MirValueId>,
        early: Option<&(MirValueId, MirValueId, bool)>,
        index_name: &str,
        start_name: &str,
        indent: usize,
    ) -> Option<String> {
        let mut defined = BTreeSet::new();
        let mut source = String::new();
        let pad = " ".repeat(indent);
        for block_id in &fact.body_blocks {
            let block = function.blocks.iter().find(|block| block.id == *block_id)?;
            for instruction in &block.instructions {
                if fact
                    .fixed_reduction
                    .as_ref()
                    .is_some_and(|fixed| !fixed.source_operations.contains(&instruction.id))
                {
                    continue;
                }
                match &instruction.operation {
                    MirOperation::WritePlace { place, value } => {
                        if loop_places.contains(place)
                            || rmw.is_some_and(|(target, _, _)| target == place)
                        {
                            continue;
                        }
                        let expression = self.vector_value_expr(
                            function,
                            *value,
                            fact,
                            index_name,
                            &defined,
                            loop_places,
                            N,
                        )?;
                        source.push_str(&self.vector_store(
                            function,
                            *place,
                            fact,
                            &expression,
                            index_name,
                            N,
                        )?);
                    }
                    MirOperation::ScopeEnter { .. } | MirOperation::ScopeExit { .. } => {}
                    operation => {
                        let Some(result) = instruction.result else {
                            if !matches!(
                                operation,
                                MirOperation::LoopRangeValue { .. }
                                    | MirOperation::LoopIterValue { .. }
                            ) {
                                continue;
                            }
                            return None;
                        };
                        let expression = self.vector_value_expr(
                            function,
                            result,
                            fact,
                            index_name,
                            &defined,
                            loop_places,
                            N,
                        )?;
                        let ty = self.rust_type(self.value_type(function, result));
                        let _ = writeln!(
                            source,
                            "{pad}let __jet_vec_v{}: [{ty}; {N}] = {expression};",
                            result.0
                        );
                        defined.insert(result);
                    }
                }
            }
            match &block.terminator {
                MirTerminator::Jump { .. } => {}
                MirTerminator::Branch {
                    condition: branch_condition,
                    ..
                } if fact.rule == MirVectorRule::ConditionalAccumulate
                    && Some(*branch_condition) == condition => {}
                MirTerminator::Branch { condition, .. }
                    if fact.rule == MirVectorRule::EarlyExitSearch
                        && early.is_some_and(|&(value, _, _)| value == *condition) => {}
                _ => {
                    if !matches!(
                        fact.rule,
                        MirVectorRule::ConditionalAccumulate | MirVectorRule::EarlyExitSearch
                    ) {
                        return None;
                    }
                }
            }
        }
        let mut required = BTreeSet::new();
        if let Some(condition) = condition {
            required.insert(condition);
        }
        if let Some((condition, return_value, _)) = early {
            required.insert(*condition);
            required.insert(*return_value);
        }
        for value in required {
            if defined.contains(&value) {
                continue;
            }
            let expression = self.vector_value_expr(
                function,
                value,
                fact,
                index_name,
                &defined,
                loop_places,
                N,
            )?;
            let ty = self.rust_type(self.value_type(function, value));
            let _ = writeln!(
                source,
                "{pad}let __jet_vec_v{}: [{ty}; {N}] = {expression};",
                value.0
            );
            defined.insert(value);
        }
        if let Some(condition) = condition {
            let mask = self.vector_array_name(condition);
            let Some((_, addend, _)) = rmw else {
                return None;
            };
            let addend = self.vector_array_name(*addend);
            let lane = self.fixed_reduction_lane_expr(function, fact, index_name, start_name);
            let _ = writeln!(
                source,
                "{pad}jet_simd_accumulate_masked_fixed(&mut __jet_vec_acc, &{addend}, &{mask}, {lane});"
            );
            let _ = writeln!(
                source,
                "{pad}if {mask}.iter().any(|selected| *selected) {{ __jet_vec_seen = true; }}"
            );
        } else if let Some((_, addend, _)) = rmw {
            let addend = self.vector_array_name(*addend);
            let lane = self.fixed_reduction_lane_expr(function, fact, index_name, start_name);
            let _ = writeln!(
                source,
                "{pad}jet_simd_accumulate_fixed(&mut __jet_vec_acc, &{addend}, {lane});"
            );
            source.push_str(&format!("{pad}__jet_vec_seen = true;\n"));
        }
        if let Some((condition, return_value, early_on_then)) = early {
            let condition = self.vector_array_name(*condition);
            let mask = if *early_on_then {
                condition
            } else {
                format!("core::array::from_fn(|lane| !{condition}[lane])")
            };
            let value = self.vector_array_name(*return_value);
            let _ = writeln!(
                source,
                "{pad}if let Some(__jet_vec_lane) = {mask}.iter().position(|value| *value) {{"
            );
            let _ = writeln!(source, "{pad}    return {value}[__jet_vec_lane];");
            let _ = writeln!(source, "{pad}}}");
        }
        Some(source)
    }
    fn vector_default_int_type(ty: &MirType) -> bool {
        match ty.kind() {
            // Exact Int is an owned node, not a machine-vector lane.
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => Self::vector_default_int_type(base),
            _ => false,
        }
    }

    fn vector_default_int_range(&self, function: &MirFunction, fact: &MirVectorFact) -> bool {
        let Some(cursor) = fact.cursor else {
            return false;
        };
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                let MirOperation::LoopRangeInit { start, .. } = &instruction.operation else {
                    return None;
                };
                (instruction.result == Some(cursor))
                    .then(|| Self::vector_default_int_type(self.value_type(function, *start)))
            })
            .unwrap_or(false)
    }

    fn vector_index_expr(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
        index_name: &str,
        lane_name: &str,
    ) -> String {
        if self.vector_default_int_range(function, fact) {
            format!(
                "{}jet_std::jet_int_add(({index_name}), ({lane_name}))",
                self.config.root_prefix
            )
        } else {
            format!("({index_name}) + ({lane_name})")
        }
    }

    fn vector_index_usize_expr(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
        index_name: &str,
        lane_name: &str,
    ) -> String {
        let index = self.vector_index_expr(function, fact, index_name, lane_name);
        if self.vector_default_int_range(function, fact) {
            format!(
                "{}jet_std::jet_int_to_i64({index}).expect(\"vector index exceeds host range\") as usize",
                self.config.root_prefix
            )
        } else {
            format!("({index}) as usize")
        }
    }

    fn fixed_reduction_lane_expr(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
        index_name: &str,
        start_name: &str,
    ) -> String {
        let order = D_FRED1_FIXED_ORDER;
        if !order.is_canonical() {
            panic!("MIR reduction vector emission requires canonical D-FRED1 order");
        }
        if self.vector_default_int_range(function, fact) {
            return format!(
                "jet_simd_fixed_lane_from_int::<{}>({index_name}, {start_name}, {}jet_std::jet_int_sub, {}jet_std::jet_int_to_i64)",
                order.lanes,
                self.config.root_prefix,
                self.config.root_prefix,
            );
        }
        format!(
            "(({index_name}).wrapping_sub({start_name}) as usize) % {}",
            order.lanes
        )
    }

    fn vector_value_expr(
        &self,
        function: &MirFunction,
        value: MirValueId,
        fact: &MirVectorFact,
        index_name: &str,
        defined: &BTreeSet<MirValueId>,
        loop_places: &BTreeSet<MirPlaceId>,
        width: usize,
    ) -> Option<String> {
        if defined.contains(&value) {
            return Some(self.vector_array_name(value));
        }
        let instruction = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(value))?;
        match &instruction.operation {
            MirOperation::Constant(constant) => Some(format!(
                "[{}; {width}]",
                self.constant_for_type(constant, self.value_type(function, value))
            )),
            MirOperation::LoopRangeValue { cursor, .. }
            | MirOperation::LoopIterValue { cursor, .. }
                if Some(*cursor) == fact.cursor =>
            {
                let index = self.vector_index_expr(function, fact, index_name, "lane as _");
                Some(format!("core::array::from_fn(|lane| {index})"))
            }
            MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                self.vector_place_load(function, *place, fact, index_name, loop_places, width)
            }
            MirOperation::Copy { value }
            | MirOperation::Move { value }
            | MirOperation::AttachTag { value, .. } => self.vector_value_expr(
                function,
                *value,
                fact,
                index_name,
                defined,
                loop_places,
                width,
            ),
            MirOperation::Index {
                base, index, kind, ..
            } if Some(*index) == fact.cursor && *kind == MirIndexKind::FixedListProof => {
                self.vector_index_load(function, *base, fact, index_name, width)
            }
            MirOperation::Field { base, field } => self.vector_field_load(
                function,
                *base,
                *field,
                fact,
                index_name,
                defined,
                loop_places,
                width,
            ),
            MirOperation::Binary {
                op, left, right, ..
            } => self.vector_binary(
                function,
                *op,
                *left,
                *right,
                fact,
                index_name,
                defined,
                loop_places,
                width,
            ),
            MirOperation::Unary { op, value } => {
                let default_int = Self::vector_default_int_type(self.value_type(function, *value));
                let value = self.vector_value_expr(
                    function,
                    *value,
                    fact,
                    index_name,
                    defined,
                    loop_places,
                    width,
                )?;
                let body = match op {
                    MirUnaryOp::Neg if default_int => {
                        format!(
                            "{}jet_std::jet_int_neg({value}[lane])",
                            self.config.root_prefix
                        )
                    }
                    MirUnaryOp::Neg => format!("-{value}[lane]"),
                    MirUnaryOp::Not => format!("!{value}[lane]"),
                };
                Some(format!("core::array::from_fn(|lane| {body})"))
            }
            MirOperation::Semantic(MirSemanticOp::ColumnarRead {
                base,
                column,
                column_index,
                index,
                ..
            }) if Some(*index) == fact.cursor => {
                let index = self.vector_index_usize_expr(function, fact, index_name, "lane as _");
                Some(format!(
                    "core::array::from_fn(|lane| ({}) .column({column_index})[{index}].clone().jet_col_{}())",
                    self.value_read(*base),
                    self.field_name(*column)
                ))
            }
            MirOperation::Parameter { .. }
            | MirOperation::Capture { .. }
            | MirOperation::Global { .. } => Some(format!(
                "[{}; {width}]",
                self.operation_expression(function, &instruction.operation, Some(value), None)
            )),
            _ => None,
        }
    }

    fn vector_binary(
        &self,
        function: &MirFunction,
        op: MirBinaryOp,
        left: MirValueId,
        right: MirValueId,
        fact: &MirVectorFact,
        index_name: &str,
        defined: &BTreeSet<MirValueId>,
        loop_places: &BTreeSet<MirPlaceId>,
        width: usize,
    ) -> Option<String> {
        let left_expr = self.vector_value_expr(
            function,
            left,
            fact,
            index_name,
            defined,
            loop_places,
            width,
        )?;
        let right_expr = self.vector_value_expr(
            function,
            right,
            fact,
            index_name,
            defined,
            loop_places,
            width,
        )?;
        let left_name = self.vector_array_name(left);
        let right_name = self.vector_array_name(right);
        let left_expr = format!("{{ let {left_name} = {left_expr}; {left_name} }}");
        let right_expr = format!("{{ let {right_name} = {right_expr}; {right_name} }}");
        let left_default_int = Self::vector_default_int_type(self.value_type(function, left));
        let right_default_int = Self::vector_default_int_type(self.value_type(function, right));
        if op.is_comparison() {
            let compare = match op {
                MirBinaryOp::Eq => "Eq",
                MirBinaryOp::Ne => "Ne",
                MirBinaryOp::Lt => "Lt",
                MirBinaryOp::Gt => "Gt",
                MirBinaryOp::Le => "Le",
                MirBinaryOp::Ge => "Ge",
                _ => return None,
            };
            if left_default_int || right_default_int {
                if !left_default_int || !right_default_int {
                    return None;
                }
                return Some(format!(
                    "jet_simd_compare_int_array(&{left_expr}, &{right_expr}, JetSimdCompareOp::{compare}, {}jet_std::jet_int_is_inline, {}jet_std::jet_int_compare)",
                    self.config.root_prefix,
                    self.config.root_prefix,
                ));
            }
            return Some(format!(
                "jet_simd_compare_array(&{left_expr}, &{right_expr}, JetSimdCompareOp::{compare})"
            ));
        }
        if left_default_int || right_default_int {
            if !left_default_int || !right_default_int {
                return None;
            }
            let helper = match op {
                MirBinaryOp::Add => "jet_int_add",
                MirBinaryOp::Sub => "jet_int_sub",
                MirBinaryOp::Mul => "jet_int_mul",
                // Int division carries source-location arguments and must
                // remain on the scalar MIR path.
                MirBinaryOp::Div => return None,
                _ => return None,
            };
            return Some(format!(
                "core::array::from_fn(|lane| {}jet_std::{helper}({left_expr}[lane], {right_expr}[lane]))",
                self.config.root_prefix
            ));
        }
        let helper = match (self.value_type(function, left).kind(), width, op) {
            (MirTypeKind::Float32, 4, MirBinaryOp::Add) => "jet_simd_f32x4_add_array",
            (MirTypeKind::Float32, 4, MirBinaryOp::Sub) => "jet_simd_f32x4_sub_array",
            (MirTypeKind::Float32, 4, MirBinaryOp::Mul) => "jet_simd_f32x4_mul_array",
            (MirTypeKind::Float32, 4, MirBinaryOp::Div) => "jet_simd_f32x4_div_array",
            (MirTypeKind::Float32, 8, MirBinaryOp::Add) => "jet_simd_f32x8_add_array",
            (MirTypeKind::Float32, 8, MirBinaryOp::Sub) => "jet_simd_f32x8_sub_array",
            (MirTypeKind::Float32, 8, MirBinaryOp::Mul) => "jet_simd_f32x8_mul_array",
            (MirTypeKind::Float32, 8, MirBinaryOp::Div) => "jet_simd_f32x8_div_array",
            (MirTypeKind::Float, 2, MirBinaryOp::Add) => "jet_simd_f64x2_add_array",
            (MirTypeKind::Float, 2, MirBinaryOp::Sub) => "jet_simd_f64x2_sub_array",
            (MirTypeKind::Float, 2, MirBinaryOp::Mul) => "jet_simd_f64x2_mul_array",
            (MirTypeKind::Float, 2, MirBinaryOp::Div) => "jet_simd_f64x2_div_array",
            (MirTypeKind::Float, 4, MirBinaryOp::Add) => "jet_simd_f64x4_add_array",
            (MirTypeKind::Float, 4, MirBinaryOp::Sub) => "jet_simd_f64x4_sub_array",
            (MirTypeKind::Float, 4, MirBinaryOp::Mul) => "jet_simd_f64x4_mul_array",
            (MirTypeKind::Float, 4, MirBinaryOp::Div) => "jet_simd_f64x4_div_array",
            (_, _, MirBinaryOp::Add) => "jet_simd_add_array",
            (_, _, MirBinaryOp::Sub) => "jet_simd_sub_array",
            (_, _, MirBinaryOp::Mul) => "jet_simd_mul_array",
            (_, _, MirBinaryOp::Div) => "jet_simd_div_array",
            (_, _, MirBinaryOp::And) => {
                return Some(format!(
                    "core::array::from_fn(|lane| {left_expr}[lane] && {right_expr}[lane])"
                ))
            }
            (_, _, MirBinaryOp::Or) => {
                return Some(format!(
                    "core::array::from_fn(|lane| {left_expr}[lane] || {right_expr}[lane])"
                ))
            }
            _ => return None,
        };
        Some(format!("{helper}(&{left_expr}, &{right_expr})"))
    }

    fn vector_place_load(
        &self,
        function: &MirFunction,
        place_id: MirPlaceId,
        fact: &MirVectorFact,
        index_name: &str,
        loop_places: &BTreeSet<MirPlaceId>,
        width: usize,
    ) -> Option<String> {
        let place = function.places.iter().find(|place| place.id == place_id)?;
        if place.projections.is_empty() {
            let index = self.vector_index_expr(function, fact, index_name, "lane as _");
            if loop_places.contains(&place_id) {
                return Some(format!("core::array::from_fn(|lane| {index})"));
            }
            return Some(format!(
                "[{}; {width}]",
                self.place_read(function, place_id)
            ));
        }
        let index_pos = place.projections.iter().position(|projection| {
            matches!(
                projection,
                MirProjection::Index { index, kind, .. }
                    if Some(*index) == fact.cursor && *kind == MirIndexKind::FixedListProof
            )
        })?;
        if place.projections[..index_pos]
            .iter()
            .any(|projection| matches!(projection, MirProjection::Index { .. }))
        {
            return None;
        }
        let base = self.place_base(
            function,
            &place.base,
            false,
            &place.projections[..index_pos],
        );
        let suffix = &place.projections[index_pos + 1..];
        let field = suffix
            .iter()
            .try_fold(String::new(), |mut field, projection| {
                let MirProjection::Field { field: id, .. } = projection else {
                    return None;
                };
                field.push('.');
                field.push_str(&self.field_name(*id));
                Some(field)
            })?;
        let index = self.vector_index_usize_expr(function, fact, index_name, "lane as _");
        Some(format!(
            "core::array::from_fn(|lane| (({base})[{index}]{field}).clone())"
        ))
    }
    fn vector_index_load(
        &self,
        function: &MirFunction,
        base: MirValueId,
        fact: &MirVectorFact,
        index_name: &str,
        _width: usize,
    ) -> Option<String> {
        let index = self.vector_index_usize_expr(function, fact, index_name, "lane as _");
        Some(format!(
            "core::array::from_fn(|lane| ({}[{index}].clone()))",
            self.value_read(base)
        ))
    }

    fn vector_field_load(
        &self,
        function: &MirFunction,
        base: MirValueId,
        field: MirFieldId,
        fact: &MirVectorFact,
        index_name: &str,
        defined: &BTreeSet<MirValueId>,
        loop_places: &BTreeSet<MirPlaceId>,
        width: usize,
    ) -> Option<String> {
        let base_instruction = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(base))?;
        if let MirOperation::Index {
            base: collection,
            index,
            kind: MirIndexKind::FixedListProof,
            ..
        } = &base_instruction.operation
        {
            if Some(*index) != fact.cursor {
                return None;
            }
            let access = fact
                .accesses
                .iter()
                .find(|access| access.field == Some(field))?;
            if access.layout == MirVectorLayout::ColumnarDirect {
                let column = access.column_index?;
                let index = self.vector_index_usize_expr(function, fact, index_name, "lane as _");
                return Some(format!(
                    "core::array::from_fn(|lane| ({}) .column({column})[{index}].clone().jet_col_{}())",
                    self.value_read(*collection),
                    self.field_name(field)
                ));
            }
            let index = self.vector_index_usize_expr(function, fact, index_name, "lane as _");
            return Some(format!(
                "core::array::from_fn(|lane| ({}[{index}].{}).clone())",
                self.value_read(*collection),
                self.field_name(field)
            ));
        }
        let base = self.vector_value_expr(
            function,
            base,
            fact,
            index_name,
            defined,
            loop_places,
            width,
        )?;
        Some(format!(
            "core::array::from_fn(|lane| ({}[lane].{}).clone())",
            base,
            self.field_name(field)
        ))
    }

    fn vector_store(
        &self,
        function: &MirFunction,
        place_id: MirPlaceId,
        fact: &MirVectorFact,
        value: &str,
        index_name: &str,
        width: usize,
    ) -> Option<String> {
        let place = function.places.iter().find(|place| place.id == place_id)?;
        let index_pos = place.projections.iter().position(|projection| {
            matches!(
                projection,
                MirProjection::Index { index, kind, .. }
                    if Some(*index) == fact.cursor && *kind == MirIndexKind::FixedListProof
            )
        })?;
        let base = self.place_base(function, &place.base, true, &place.projections[..index_pos]);
        let suffix = place.projections[index_pos + 1..]
            .iter()
            .map(|projection| match projection {
                MirProjection::Field { field, .. } => Some(format!(".{}", self.field_name(*field))),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?
            .join("");
        let index = self.vector_index_usize_expr(function, fact, index_name, "__jet_vec_lane as _");
        Some(format!(
            "for __jet_vec_lane in 0..{width} {{ ({base})[{index}]{suffix} = {value}[__jet_vec_lane]; }}\n"
        ))
    }

    fn vector_is_loop_value(
        &self,
        function: &MirFunction,
        value: MirValueId,
        cursor: MirValueId,
    ) -> bool {
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                instruction.result == Some(value)
                    && matches!(
                        &instruction.operation,
                        MirOperation::LoopRangeValue { cursor: candidate, .. }
                            | MirOperation::LoopIterValue { cursor: candidate, .. }
                            if *candidate == cursor
                    )
            })
    }

    fn vector_rmw_plan(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
    ) -> Option<(MirPlaceId, MirValueId, MirValueId)> {
        if let Some(fixed) = &fact.fixed_reduction {
            return Some((fixed.accumulator, fixed.addend, fixed.seed));
        }
        let instructions = fact
            .body_blocks
            .iter()
            .filter_map(|id| function.blocks.iter().find(|block| block.id == *id))
            .flat_map(|block| block.instructions.iter())
            .collect::<Vec<_>>();
        instructions.iter().find_map(|instruction| {
            let MirOperation::WritePlace { place, value } = &instruction.operation else {
                return None;
            };
            let place_row = function
                .places
                .iter()
                .find(|candidate| candidate.id == *place)?;
            if !place_row.projections.is_empty() {
                return None;
            }
            let defining = instructions
                .iter()
                .find(|candidate| candidate.result == Some(*value))?;
            let MirOperation::Binary {
                op: MirBinaryOp::Add,
                left,
                right,
                ..
            } = &defining.operation
            else {
                return None;
            };
            let is_read = |candidate: MirValueId| {
                instructions.iter().any(|instruction| {
                    instruction.result == Some(candidate)
                        && matches!(
                            &instruction.operation,
                            MirOperation::ReadPlace(read) if *read == *place
                        )
                })
            };
            let (seed, addend) = if is_read(*left) {
                (*left, *right)
            } else if is_read(*right) {
                (*right, *left)
            } else {
                return None;
            };
            Some((*place, addend, seed))
        })
    }

    fn vector_condition(&self, function: &MirFunction, fact: &MirVectorFact) -> Option<MirValueId> {
        if let Some(fixed) = &fact.fixed_reduction {
            return fixed.condition;
        }
        fact.body_blocks
            .iter()
            .filter_map(|id| function.blocks.iter().find(|block| block.id == *id))
            .find_map(|block| match &block.terminator {
                MirTerminator::Branch { condition, .. } => Some(*condition),
                _ => None,
            })
    }

    fn vector_early_return(
        &self,
        function: &MirFunction,
        fact: &MirVectorFact,
    ) -> Option<(MirValueId, MirValueId, bool)> {
        let mut candidate = None;
        for block_id in &fact.body_blocks {
            let block = function.blocks.iter().find(|block| block.id == *block_id)?;
            let MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            } = &block.terminator
            else {
                continue;
            };
            for (target, other, early_on_then) in [
                (*then_target, *else_target, true),
                (*else_target, *then_target, false),
            ] {
                if target == other {
                    continue;
                }
                let target_block = function.blocks.iter().find(|block| block.id == target)?;
                let MirTerminator::Return { value: Some(value) } = &target_block.terminator else {
                    continue;
                };
                let next = (*condition, *value, early_on_then);
                if candidate.replace(next).is_some() {
                    return None;
                }
            }
        }
        candidate
    }

    fn vector_seed_expr(&self, function: &MirFunction, value: MirValueId) -> String {
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if instruction.result != Some(value) {
                    return None;
                }
                match &instruction.operation {
                    MirOperation::ReadPlace(place) => Some(self.place_read(function, *place)),
                    _ => None,
                }
            })
            .unwrap_or_else(|| self.value_read(value))
    }

    fn vector_array_name(&self, value: MirValueId) -> String {
        format!("__jet_vec_v{}", value.0)
    }

    fn generic_params(&self, function: &MirFunction) -> String {
        let mut declared = function.generic_params.clone();
        if self.history_runtime_metadata_enabled()
            && matches!(&function.form, MirFunctionForm::TopLevel)
        {
            let mut needed = function
                .capture_params
                .iter()
                .flat_map(|capture| self.history_type_parameters(&capture.ty))
                .collect::<BTreeSet<_>>();
            for param in &function.params {
                needed.extend(self.history_type_parameters(&param.ty));
            }
            needed.extend(self.history_type_parameters(&function.return_type));
            let mut children = BTreeSet::from([function.id]);
            let mut parents = BTreeSet::new();
            loop {
                let next = self.program.functions.iter().filter(|parent| !parents.contains(&parent.id)
                    && parent.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {
                        matches!(&instruction.operation, MirOperation::Closure { function, .. } if children.contains(function))
                    })).collect::<Vec<_>>();
                if next.is_empty() {
                    break;
                }
                for parent in next {
                    for param in &parent.generic_params {
                        if needed.contains(&param.name)
                            && !declared.iter().any(|own| own.name == param.name)
                        {
                            declared.push(param.clone());
                        }
                    }
                    parents.insert(parent.id);
                    children.insert(parent.id);
                }
            }
        }
        let params = declared
            .iter()
            .map(|param| {
                let mut bounds = param
                    .bounds
                    .iter()
                    .map(|bound| self.trait_nominal_name(bound))
                    .collect::<Vec<_>>();
                if self.history_runtime_metadata_enabled() {
                    bounds.push("'static".to_string());
                }
                let name = mangle(&param.name);
                if bounds.is_empty() {
                    name
                } else {
                    format!("{name}: {}", bounds.join(" + "))
                }
            })
            .collect::<Vec<_>>();
        if params.is_empty() {
            String::new()
        } else {
            format!("<{}>", params.join(", "))
        }
    }

    // Binding SSA rows name native places; they do not copy borrowed referents.
    // This applies to every emitted function. History mode only adds runtime
    // metadata around the same checked binding facts.
    fn history_binding(&self, value: MirValueId) -> Option<(MirPlaceBase, MirAccess)> {
        let function = self.function_row(self.history_current_function.get()?);
        function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| {
                if instruction.result != Some(value) {
                    return None;
                }
                match &instruction.operation {
                    MirOperation::Capture { slot } => Some((
                        MirPlaceBase::Capture(value),
                        self.capture_param(function, *slot).access,
                    )),
                    MirOperation::Parameter { index, .. } => {
                        let param = function.params.iter().find(|param| param.index == *index)?;
                        Some((MirPlaceBase::Parameter(value), param.access))
                    }
                    _ => None,
                }
            })
    }

    fn value_definition<'value>(
        &self,
        function: &'value MirFunction,
        value: MirValueId,
    ) -> Option<&'value MirOperation> {
        function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| {
                (instruction.result == Some(value)).then_some(&instruction.operation)
            })
    }

    fn borrowed_value_reference(
        &self,
        function: &MirFunction,
        value: MirValueId,
        access: MirAccess,
    ) -> String {
        let mutable = access == MirAccess::Write;
        match self.value_definition(function, value) {
            Some(MirOperation::AddressOf { place, .. } | MirOperation::ReadPlace(place)) => {
                self.place_reference(function, *place, access)
            }
            Some(MirOperation::Parameter { .. } | MirOperation::Capture { .. }) => {
                let borrow = if mutable { "&mut " } else { "&" };
                format!(
                    "{borrow}({})",
                    self.parameter_place(function, value, mutable)
                )
            }
            _ => self.value_slot_reference(value, mutable),
        }
    }

    fn call_argument_borrowed(
        &self,
        function: &MirFunction,
        callee: &MirCallee,
        index: usize,
    ) -> bool {
        match callee {
            MirCallee::User(function)
            | MirCallee::Associated { function, .. }
            | MirCallee::Method { function, .. } => self
                .function_row(*function)
                .params
                .get(index)
                .is_some_and(|param| {
                    param.access == MirAccess::Write || self.parameter_borrowed(param)
                }),
            MirCallee::TraitMethod { method, .. } => {
                let method = self
                    .program
                    .traits
                    .iter()
                    .flat_map(|trait_def| &trait_def.methods)
                    .find(|candidate| candidate.id == *method)
                    .unwrap_or_else(|| panic!("MIR trait method {:?} has no row", method));
                if index == 0 {
                    method
                        .self_access
                        .is_some_and(|access| access != MirAccess::Move)
                } else {
                    method
                        .params
                        .get(index - 1)
                        .is_some_and(|param| {
                            param.access == MirAccess::Write || self.parameter_borrowed(param)
                        })
                }
            }
            MirCallee::Core(call) => self
                .core_row(*call)
                .borrow_mask
                .get(index)
                .copied()
                .unwrap_or(false),
            MirCallee::Prelude(call) => self
                .prelude_row(*call)
                .signature
                .borrow_mask
                .get(index)
                .copied()
                .unwrap_or(false),
            MirCallee::Foreign(_) => false,
            MirCallee::Indirect(value) => Self::callable_parameters(self.value_type(function, *value))
                .nth(index)
                .is_some_and(|(param, access)| self.callable_parameter_borrowed(param, access)),
        }
    }
    fn direct_borrow_call_arg(&self, arg: &MirCallArg, borrowed: bool) -> bool {
        (borrowed && matches!(arg.access, MirAccess::Read | MirAccess::Write))
            && !arg.authority_boundary
            && !arg.implicit_clone
            && !arg.shared_auto_clone
            && !arg.widen_fixed_to_list
            && arg.widen_to_union.is_none()
            && arg.box_as_trait.is_none()
            && arg.fn_coercion.is_none()
    }

    fn core_call_argument_borrowed(&self, route: MirPreludeCallId, index: usize) -> bool {
        self.prelude_row(route)
            .signature
            .borrow_mask
            .get(index)
            .copied()
            .unwrap_or(false)
    }

    fn borrowed_operation_use(
        &self,
        function: &MirFunction,
        operation: &MirOperation,
        value: MirValueId,
    ) -> bool {
        match operation {
            MirOperation::Call { callee, args, .. } => {
                let mut found = false;
                for (index, argument) in args.iter().enumerate() {
                    if argument.value == value {
                        found = true;
                        if !self.direct_borrow_call_arg(
                            argument,
                            self.call_argument_borrowed(function, callee, index),
                        ) {
                            return false;
                        }
                    }
                }
                found
            }
            MirOperation::CoreCall { route, args, .. }
            | MirOperation::Semantic(MirSemanticOp::HostCall { call: route, args })
            | MirOperation::Semantic(MirSemanticOp::StaticPreludeCall {
                call: route, args, ..
            }) => {
                let mut found = false;
                for (index, argument) in args.iter().enumerate() {
                    if argument.value == value {
                        found = true;
                        if !self.direct_borrow_call_arg(
                            argument,
                            self.core_call_argument_borrowed(*route, index),
                        ) {
                            return false;
                        }
                    }
                }
                found
            }
            MirOperation::Semantic(MirSemanticOp::AllocNew { call, args, .. }) => {
                let row = self.prelude_row(*call);
                let mut found = false;
                for (index, argument) in args.iter().enumerate() {
                    if argument.value == value {
                        found = true;
                        if !self.direct_borrow_call_arg(
                            argument,
                            row.signature
                                .borrow_mask
                                .get(index)
                                .copied()
                                .unwrap_or(false),
                        ) {
                            return false;
                        }
                    }
                }
                found
            }
            MirOperation::Index { base, index, .. } => *base == value && *index != value,
            MirOperation::Semantic(MirSemanticOp::HandleMethod {
                call,
                receiver,
                args,
                ..
            }) => {
                let row = self.prelude_row(*call);
                let mut found = false;
                if *receiver == value {
                    found = true;
                    if !row.signature.borrow_mask.first().copied().unwrap_or(false) {
                        return false;
                    }
                }
                for (index, argument) in args.iter().enumerate() {
                    if *argument == value {
                        found = true;
                        if !row
                            .signature
                            .borrow_mask
                            .get(index + 1)
                            .copied()
                            .unwrap_or(false)
                        {
                            return false;
                        }
                    }
                }
                found
            }
            MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
                call,
                receiver,
                args,
                ..
            }) => {
                let row = self.prelude_row(*call);
                let mut found = false;
                if *receiver == value {
                    found = true;
                    if !row.signature.borrow_mask.first().copied().unwrap_or(false) {
                        return false;
                    }
                }
                for (index, argument) in args.iter().enumerate() {
                    if *argument == value {
                        found = true;
                        if !row
                            .signature
                            .borrow_mask
                            .get(index + 1)
                            .copied()
                            .unwrap_or(false)
                        {
                            return false;
                        }
                    }
                }
                found
            }
            MirOperation::Semantic(MirSemanticOp::ClosureMethod {
                call,
                receiver,
                args,
            }) => {
                let row = self.prelude_row(*call);
                let mut found = false;
                if *receiver == value {
                    found = true;
                    if !row.signature.borrow_mask.first().copied().unwrap_or(false) {
                        return false;
                    }
                }
                for (index, argument) in args.iter().enumerate() {
                    if argument.value == value {
                        found = true;
                        if argument.access == MirAccess::Move
                            || !row
                                .signature
                                .borrow_mask
                                .get(index + 1)
                                .copied()
                                .unwrap_or(false)
                        {
                            return false;
                        }
                    }
                }
                found
            }
            _ => false,
        }
    }
    fn direct_borrow_only(&self, function: &MirFunction, value: MirValueId) -> bool {
        let mut used = false;
        for block in &function.blocks {
            for instruction in &block.instructions {
                if instruction.result == Some(value)
                    || !instruction.operation.value_uses().contains(&value)
                {
                    continue;
                }
                if !self.borrowed_operation_use(function, &instruction.operation, value) {
                    return false;
                }
                used = true;
            }
            if block.terminator.value_uses().contains(&value) {
                return false;
            }
        }
        used
    }

    fn value_ownership(&self, value: MirValueId) -> MirOwnership {
        let function = self.function_row(
            self.history_current_function
                .get()
                .expect("current native function"),
        );
        function
            .values
            .iter()
            .find(|(id, _, _, _)| *id == value)
            .map(|(_, _, _, ownership)| *ownership)
            .unwrap_or_else(|| panic!("MIR value ID {:?} has no ownership row", value))
    }

    fn value_transfer(&self, value: MirValueId) -> String {
        if self.history_binding(value).is_none()
            && self
                .history_current_function
                .get()
                .is_some_and(|id| {
                    allocator_view_inner(self.value_type(self.function_row(id), value)).is_some()
                })
        {
            return self.value_move(value);
        }
        match self.value_ownership(value).mode {
            MirOwnershipMode::Owned | MirOwnershipMode::Move => self.value_move(value),
            MirOwnershipMode::Copy
            | MirOwnershipMode::Shared
            | MirOwnershipMode::ReadBorrow
            | MirOwnershipMode::WriteBorrow => self.value_read(value),
        }
    }

    fn temporary_direct_move_storage(&self, function: &MirFunction, value: MirValueId) -> bool {
        function.places.iter().any(|place| {
            matches!(&place.base, MirPlaceBase::Temporary(root) if *root == value)
                && (place.access == MirAccess::Move && !place.projections.is_empty()
                    || matches!(place.projections.first(), Some(MirProjection::Field { .. })))
        })
    }

    fn local_storage(&self, _function: &MirFunction, local: MirLocalId) -> String {
        local_slot(local)
    }

    fn local_allocator_view(&self, function: &MirFunction, local: MirLocalId) -> bool {
        function
            .locals
            .iter()
            .find(|candidate| candidate.id == local)
            .is_some_and(|candidate| allocator_view_inner(&candidate.ty).is_some())
    }


    fn local_direct_move_storage(&self, function: &MirFunction, local: MirLocalId) -> bool {
        // A field move needs an owning root so Rust can retain the unmoved
        // siblings for their normal drop.  Ordinary field reads/writes and
        // collection-index moves can use the checked Option slot; making all
        // projections direct leaves CFG-dispatched locals uninitialized.
        function.places.iter().any(|place| {
            matches!(&place.base, MirPlaceBase::Local(candidate) if *candidate == local)
                && matches!(place.projections.first(), Some(MirProjection::Field { .. }))
                && place.access == MirAccess::Move
        })
    }

    fn local_uninit_fixed_type<'f>(
        &self,
        function: &'f MirFunction,
        local: MirLocalId,
    ) -> Option<&'f MirType> {
        function.locals.iter().find_map(|candidate| {
            (candidate.id == local
                && candidate.uninit
                && matches!(candidate.ty.kind(), MirTypeKind::FixedList { .. }))
                .then_some(&candidate.ty)
        })
    }

    fn emit_fixed_inline_backings(&self, function: &MirFunction, out: &mut String) {
        for block in &function.blocks {
            for instruction in &block.instructions {
                let MirOperation::Semantic(MirSemanticOp::AllocNew {
                    call,
                    kind: MirAllocatorKind::Fixed,
                    inline_size: Some(size),
                    ..
                }) = &instruction.operation
                else {
                    continue;
                };
                if self.prelude_row(*call).member != "fixed.new" {
                    continue;
                }
                let result = instruction
                    .result
                    .unwrap_or_else(|| panic!("MIR Fixed.new has no result slot"));
                let _ = writeln!(
                    out,
                    "    let mut {}: [std::mem::MaybeUninit<u8>; {size}] = [std::mem::MaybeUninit::<u8>::uninit(); {size}];",
                    fixed_inline_backing_name(result),
                );
            }
        }
    }

    fn initialize_uninit_expression(&self, function: &MirFunction, place_id: MirPlaceId) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == place_id)
            .unwrap_or_else(|| panic!("MIR place ID {:?} has no row", place_id));
        let MirPlaceBase::Local(local) = &place.base else {
            panic!("MIR uninitialized place {:?} is not a local root", place_id);
        };
        if self.local_uninit_fixed_type(function, *local).is_some() {
            format!(
                "{} = Some({}jet_mem::JetUninitFixed::new())",
                self.local_storage(function, *local),
                self.config.root_prefix,
            )
        } else if self.local_direct_move_storage(function, *local) {
            "()".to_string()
        } else {
            format!("{} = None", self.local_storage(function, *local))
        }
    }


    fn emit_slots(&self, function: &MirFunction, out: &mut String) {
        self.emit_fixed_inline_backings(function, out);
        // Referents are declared before callable SSA slots so native borrows
        // are dropped before the storage they retain.
        for local in &function.locals {
            if local.uninit && matches!(local.ty.kind(), MirTypeKind::FixedList { .. }) {
                let MirTypeKind::FixedList { elem, len } = local.ty.kind() else {
                    unreachable!("checked MIR uninit local is not a fixed list");
                };
                let elem = self.rust_local_type(elem);
                let _ = writeln!(out, "    let mut {}: Option<{}jet_mem::JetUninitFixed<{}, {}>> = None;", local_slot(local.id), self.config.root_prefix, elem, len.expression());
            } else {
                let ty = self.rust_local_type(&local.ty);
                if self.local_direct_move_storage(function, local.id) {
                    let _ = writeln!(out, "    let mut {}: {ty};", local_slot(local.id));
                } else {
                    let _ = writeln!(
                        out,
                        "    let mut {}: Option<{ty}> = None;",
                        local_slot(local.id)
                    );
                }
            }
        }
        for (value, ty, _, _) in &function.values {
            if self.history_binding(*value).is_some() {
                continue;
            }
            let ty = self.rust_local_type(ty);
            if self.temporary_direct_move_storage(function, *value) {
                let _ = writeln!(out, "    let mut {}: {ty};", value_slot(*value));
            } else {
                let _ = writeln!(
                    out,
                    "    let mut {}: Option<{ty}> = None;",
                    value_slot(*value)
                );
            }
        }
    }
    fn write_place_expr(
        &self,
        function: &MirFunction,
        id: MirPlaceId,
        value_id: Option<MirValueId>,
        value: &str,
    ) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR place ID {:?} has no row", id));
        if place.projections.is_empty() {
            match &place.base {
                MirPlaceBase::Local(local)
                    if self.local_uninit_fixed_type(function, *local).is_some() =>
                {
                    return format!(
                        "{} = Some({}jet_mem::JetUninitFixed::from_array({value}))",
                        self.local_storage(function, *local),
                        self.config.root_prefix,
                    );
                }
                MirPlaceBase::Local(local)
                    if self.local_allocator_view(function, *local) =>
                {
                    if value_id
                        .and_then(|value| allocator_view_inner(self.value_type(function, value)))
                        .is_some()
                    {
                        return format!("{} = Some({value})", self.local_storage(function, *local));
                    }
                    return format!(
                        "**{}.as_mut().expect(\"MIR local\") = {value}",
                        self.local_storage(function, *local)
                    );
                }
                MirPlaceBase::Local(local) if !self.local_direct_move_storage(function, *local) => {
                    return format!("{} = Some({value})", self.local_storage(function, *local));
                }
                MirPlaceBase::Temporary(value_id) => {
                    if self.temporary_direct_move_storage(function, *value_id) {
                        return format!("{} = {value}", value_slot(*value_id));
                    }
                    return format!("{} = Some({value})", value_slot(*value_id));
                }
                MirPlaceBase::Static(name) => {
                    return format!(
                        "({}{}).set({value})",
                        self.config.root_prefix,
                        mangle_path(name)
                    );
                }
                MirPlaceBase::Local(_) | MirPlaceBase::Parameter(_) | MirPlaceBase::Capture(_) => {}
            }
        }

        if let Some(MirProjection::Index {
            index,
            write_call,
            location,
            context,
            kind,
            ..
        }) = place.projections.last()
        {
            if place.projections.len() == 1
                && matches!(kind, MirIndexKind::FixedListProof)
            {
                if let MirPlaceBase::Local(local) = &place.base {
                    if self.local_uninit_fixed_type(function, *local).is_some() {
                        let index = self.index_operand(function, *index, *location);
                        return format!(
                            "{}.as_mut().expect(\"MIR local\").write(({index}) as usize, {value})",
                            self.local_storage(function, *local)
                        );
                    }
                }
            }
            let call = write_call.as_ref().copied().unwrap_or_else(|| {
                panic!("MIR writable index place has no exact setter Prelude route")
            });
            let prefix = &place.projections[..place.projections.len() - 1];
            let base = self.place_base(function, &place.base, true, prefix);
            let index = if matches!(kind, MirIndexKind::Map) {
                self.value_read(*index)
            } else {
                self.index_operand(function, *index, *location)
            };
            let mut args = vec![format!("&mut ({base})"), index, value.to_string()];
            let mut extras = vec![
                format!("{:?}", self.source_file_path(location.file)),
                format!("{}u32", location.line),
            ];
            if let Some(context) = context.as_ref() {
                extras.push(format!("{:?}", context.function));
                extras.push(format!("{:?}", context.source_line));
            }
            self.append_prelude_context(call, &mut args, &extras);
            return self.prelude_call_args_exact(call, &args);
        }
        let value = match place.projections.last() {
            Some(MirProjection::Field { field, .. }) if self.boxed_field(*field) => {
                format!("Box::new({value})")
            }
            _ => value.to_string(),
        };
        format!("{} = {value}", self.place_lvalue(function, id))
    }

    fn persist_renderable_type(&self, ty: &MirType) -> bool {
        match ty.kind() {
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::IntN { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Measure(_) => true,
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.persist_renderable_type(base),
            _ => false,
        }
    }

    fn persist_value_update(&self, function: &MirFunction, id: MirPlaceId) -> Option<String> {
        if !self.config.execution.release_devtools_policy.local_rail {
            return None;
        }
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR place ID {:?} has no row", id));
        let value_id = place.persist_key.as_ref()?;
        let type_identity = place.ty.canonical_key();
        let rendered = if self.persist_renderable_type(&place.ty) {
            format!("Some(&({}).jet_show())", self.place_read(function, id))
        } else {
            "None".to_string()
        };
        Some(format!(
            "{}jet_observe_live_value_update({:?}, {:?}, {})",
            self.config.root_prefix, value_id, type_identity, rendered,
        ))
    }

    fn emits_debug_linemap(&self, _function: &MirFunction) -> bool {
        self.config.execution.emit_debug_linemap
    }
    fn emit_instruction(
        &self,
        function: &MirFunction,
        instruction: &MirInstruction,
        out: &mut String,
        indent: usize,
        debug_active: bool,
    ) {
        if matches!(
            &instruction.operation,
            MirOperation::Parameter { .. } | MirOperation::Capture { .. }
        ) && instruction
            .result
            .is_some_and(|value| self.history_binding(value).is_some())
        {
            // Parameter/Capture results name the retained place. Their users
            // borrow or move that place at the checked use, not at function entry.
            return;
        }
        // Place-only bindings are type-bearing MIR placeholders. A read place
        // used solely by borrowed consumers must stay a native borrow; storing
        // an owned intermediate would impose Clone on the referent.
        if matches!(
            &instruction.operation,
            MirOperation::ReadPlace(_) | MirOperation::AddressOf { .. }
        ) && instruction
            .result
            .is_some_and(|value| self.direct_borrow_only(function, value))
        {
            return;
        }
        if matches!(
            &instruction.operation,
            MirOperation::Semantic(MirSemanticOp::HostBorrowCallback { .. })
        ) {
            if instruction.result.is_none() {
                panic!("MIR HostBorrowCallback must produce a value");
            }
            // Host-borrow callbacks are one-use boundary adapters. The
            // consuming call inlines the adapter from its checked source
            // operation; storing it in the ordinary owned callable slot would
            // erase the borrowed callback ABI before the native call.
            return;
        }
        let start = out.len();
        let pad = " ".repeat(indent);
        if self.emits_debug_linemap(function) {
            if let Some(line) = instruction.source_line {
                let _ = writeln!(out, "{pad}// jet:line {line}");
            }
        }
        match &instruction.operation {
            MirOperation::InitializeUninit { place } => {
                let _ = writeln!(
                    out,
                    "{pad}{};",
                    self.initialize_uninit_expression(function, *place)
                );
            }
            MirOperation::WritePlace { place, value } => {
                let value_id = *value;
                let value = self.value_transfer(value_id);
                let _ = writeln!(
                    out,
                    "{pad}{};",
                    self.write_place_expr(function, *place, Some(value_id), &value)
                );
                if let Some(update) = self.persist_value_update(function, *place) {
                    let _ = writeln!(out, "{pad}{update};");
                }
            }
            MirOperation::ScopeEnter { scope, .. } => {
                let _ = writeln!(
                    out,
                    "{pad}{};",
                    self.scope_operation_expression(function, *scope, true)
                );
            }
            MirOperation::ScopeExit { scope } => {
                let _ = writeln!(
                    out,
                    "{pad}{};",
                    self.scope_operation_expression(function, *scope, false)
                );
            }
            operation => {
                let location = match &instruction.operation {
                    MirOperation::Binary { .. }
                    | MirOperation::Semantic(MirSemanticOp::ColumnarRead { .. })
                    | MirOperation::Semantic(MirSemanticOp::HandleMethod { .. })
                    | MirOperation::LoopRangeInit { .. }
                    | MirOperation::LoopIterInit { .. } => {
                        Some(self.instruction_location(function, instruction))
                    }
                    _ => None,
                };
                let expression =
                    self.operation_expression(function, operation, instruction.result, location);
                if let Some(result) = instruction.result {
                    if self.temporary_direct_move_storage(function, result) {
                        let _ = writeln!(out, "{pad}{} = {expression};", value_slot(result));
                    } else {
                        let _ = writeln!(out, "{pad}{} = Some({expression});", value_slot(result));
                    }
                } else {
                    let _ = writeln!(out, "{pad}{expression};");
                }
            }
        }
        if debug_active {
            wrap_cfg_not_release(out, start, indent);
        }
    }

    fn emit_terminator(
        &self,
        function: &MirFunction,
        block: &MirBasicBlock,
        out: &mut String,
        indent: usize,
        active_test_scopes: Option<&[MirScopeId]>,
    ) {
        let pad = " ".repeat(indent);
        match &block.terminator {
            MirTerminator::Jump { target } => {
                self.emit_drops(function, &MirDropEdge::Normal, out, indent);
                self.set_pc(block.id, *target, out, indent);
            }
            MirTerminator::Branch {
                condition,
                then_target,
                else_target,
            } => {
                self.emit_drops(function, &MirDropEdge::Normal, out, indent);
                if self.coverage_for(function) {
                    let id = self.coverage_branch_id(function, block.id, 0);
                    let _ = writeln!(out, "{pad}if {} {{", self.value_read(*condition));
                    let _ = writeln!(
                        out,
                        "{pad}    {}jet_cov_branch({}, {}, true);",
                        self.config.root_prefix,
                        quote_rust_string(&id),
                        quote_rust_string(&function.name),
                    );
                    self.set_pc(block.id, *then_target, out, indent + 4);
                    let _ = writeln!(out, "{pad}}} else {{");
                    let _ = writeln!(
                        out,
                        "{pad}    {}jet_cov_branch({}, {}, false);",
                        self.config.root_prefix,
                        quote_rust_string(&id),
                        quote_rust_string(&function.name),
                    );
                    self.set_pc(block.id, *else_target, out, indent + 4);
                    let _ = writeln!(out, "{pad}}}");
                } else {
                    let _ = writeln!(out, "{pad}if {} {{", self.value_read(*condition));
                    self.set_pc(block.id, *then_target, out, indent + 4);
                    let _ = writeln!(out, "{pad}}} else {{");
                    self.set_pc(block.id, *else_target, out, indent + 4);
                    let _ = writeln!(out, "{pad}}}");
                }
            }
            MirTerminator::Switch {
                subject,
                arms,
                otherwise,
            } => {
                self.emit_drops(function, &MirDropEdge::Normal, out, indent);
                self.emit_switch(function, block.id, *subject, arms, *otherwise, out, indent);
            }
            MirTerminator::Return { value } => {
                if self.config.target_kind == MirRustTarget::Native {
                    if let Some(scopes) = active_test_scopes {
                        self.emit_test_scope_exits(function, scopes, out, indent);
                    }
                }
                self.emit_drops(function, &MirDropEdge::Return, out, indent);
                if function.generator.is_some() {
                    if value.is_some() {
                        panic!("MIR generator return cannot carry a value");
                    }
                    let _ = writeln!(out, "{pad}return;");
                } else {
                    let value = value
                        .map(|id| self.value_move(id))
                        .unwrap_or_else(|| self.default_return(function));
                    let _ = writeln!(out, "{pad}return {value};");
                }
            }
            MirTerminator::Yield { value, resume } => {
                if function.generator.is_none() {
                    panic!("MIR yield requires generator facts");
                }
                self.emit_drops(function, &MirDropEdge::Normal, out, indent);
                let _ = writeln!(
                    out,
                    "{pad}let _ = __jet_yield_tx.send_stream({});",
                    self.value_move(*value)
                );
                self.set_pc(block.id, *resume, out, indent);
            }
            MirTerminator::Break { target, value } => {
                self.emit_drops(function, &MirDropEdge::Normal, out, indent);
                if let Some(value) = value {
                    let _ = writeln!(out, "{pad}let _ = {};", self.value_move(*value));
                }
                self.set_pc(block.id, *target, out, indent);
            }
            MirTerminator::Continue { target } => {
                self.emit_drops(function, &MirDropEdge::Normal, out, indent);
                self.set_pc(block.id, *target, out, indent);
            }
            MirTerminator::Unreachable { reason } => {
                let _ = writeln!(out, "{pad}unreachable!({:?});", reason);
            }
        }
    }

    fn emit_switch(
        &self,
        function: &MirFunction,
        block: MirBlockId,
        subject: MirValueId,
        arms: &[jet_foundation::MIR::MirSwitchArm],
        otherwise: MirBlockId,
        out: &mut String,
        indent: usize,
    ) {
        let pad = " ".repeat(indent);
        if arms.is_empty() {
            self.set_pc(block, otherwise, out, indent);
            return;
        }
        if self.coverage_for(function) {
            self.emit_switch_coverage(function, block, arms, otherwise, out, indent, 0);
            let _ = subject;
            return;
        }
        let mut first = true;
        for arm in arms {
            let keyword = if first { "if" } else { "else if" };
            let _ = writeln!(out, "{pad}{keyword} {} {{", self.value_read(arm.condition));
            self.set_pc(block, arm.target, out, indent + 4);
            let _ = writeln!(out, "{pad}}}");
            first = false;
        }
        let _ = writeln!(out, "{pad}else {{");
        self.set_pc(block, otherwise, out, indent + 4);
        let _ = writeln!(out, "{pad}}}");
        let _ = subject;
    }

    fn emit_switch_coverage(
        &self,
        function: &MirFunction,
        block: MirBlockId,
        arms: &[jet_foundation::MIR::MirSwitchArm],
        otherwise: MirBlockId,
        out: &mut String,
        indent: usize,
        arm: usize,
    ) {
        let pad = " ".repeat(indent);
        let Some(switch_arm) = arms.get(arm) else {
            self.set_pc(block, otherwise, out, indent);
            return;
        };
        let id = self.coverage_branch_id(function, block, arm);
        let _ = writeln!(
            out,
            "{pad}if {} {{",
            self.value_read(switch_arm.condition)
        );
        let _ = writeln!(
            out,
            "{pad}    {}jet_cov_branch({}, {}, true);",
            self.config.root_prefix,
            quote_rust_string(&id),
            quote_rust_string(&function.name),
        );
        self.set_pc(block, switch_arm.target, out, indent + 4);
        let _ = writeln!(out, "{pad}}} else {{");
        let _ = writeln!(
            out,
            "{pad}    {}jet_cov_branch({}, {}, false);",
            self.config.root_prefix,
            quote_rust_string(&id),
            quote_rust_string(&function.name),
        );
        self.emit_switch_coverage(function, block, arms, otherwise, out, indent + 4, arm + 1);
        let _ = writeln!(out, "{pad}}}");
    }

    fn set_pc(&self, block: MirBlockId, target: MirBlockId, out: &mut String, indent: usize) {
        let pad = " ".repeat(indent);
        let _ = writeln!(out, "{pad}__jet_prev = {};", block.0);
        let _ = writeln!(out, "{pad}__jet_pc = {};", target.0);
        let _ = writeln!(out, "{pad}continue 'mir_dispatch;");
    }

    fn emit_drops(
        &self,
        function: &MirFunction,
        edge: &MirDropEdge,
        out: &mut String,
        indent: usize,
    ) {
        for action in &function.drops {
            let applies = match (&action.edge, edge) {
                (MirDropEdge::Normal, MirDropEdge::Normal) => true,
                (MirDropEdge::Return, MirDropEdge::Return) => true,
                (MirDropEdge::Failure(left), MirDropEdge::Failure(right)) => left == right,
                (MirDropEdge::Unwind(left), MirDropEdge::Unwind(right)) => left == right,
                (MirDropEdge::Normal, MirDropEdge::Return)
                | (MirDropEdge::Normal, MirDropEdge::Failure(_))
                | (MirDropEdge::Normal, MirDropEdge::Unwind(_))
                | (MirDropEdge::Return, MirDropEdge::Normal)
                | (MirDropEdge::Return, MirDropEdge::Failure(_))
                | (MirDropEdge::Return, MirDropEdge::Unwind(_))
                | (MirDropEdge::Failure(_), MirDropEdge::Normal)
                | (MirDropEdge::Failure(_), MirDropEdge::Return)
                | (MirDropEdge::Failure(_), MirDropEdge::Unwind(_))
                | (MirDropEdge::Unwind(_), MirDropEdge::Normal)
                | (MirDropEdge::Unwind(_), MirDropEdge::Return)
                | (MirDropEdge::Unwind(_), MirDropEdge::Failure(_)) => false,
            };
            if applies {
                let pad = " ".repeat(indent);
                let place = function
                    .places
                    .iter()
                    .find(|place| place.id == action.place)
                    .unwrap_or_else(|| panic!("MIR drop place {:?} has no row", action.place));
                let expression = if let Some(handle) = self.handle_for_type(&place.ty) {
                    let moved = self.place_move_for_drop(function, place);
                    self.close_handle_drop(handle, moved)
                } else {
                    format!("drop({})", self.place_read(function, action.place))
                };
                let _ = writeln!(out, "{pad}{expression};");
            }
        }
    }

    fn default_return(&self, function: &MirFunction) -> String {
        if function.return_type.is_unit() {
            return "()".to_string();
        }
        // A block-bodied fallible unit function has an implicit successful
        // return even when MIR carries no value on its return terminator.
        if matches!(
            &function.failure,
            MirFailureCarrier::Result { success, .. } if success.is_unit()
        ) {
            return "Ok(())".to_string();
        }
        panic!("MIR non-unit return terminator has no value");
    }
    fn value_type<'b>(&self, function: &'b MirFunction, value: MirValueId) -> &'b MirType {
        function
            .values
            .iter()
            .find(|(id, _, _, _)| *id == value)
            .map(|(_, ty, _, _)| ty)
            .unwrap_or_else(|| panic!("MIR value ID {:?} has no type row", value))
    }

    fn build_list(
        &self,
        function: &MirFunction,
        result: Option<MirValueId>,
        values: &[MirValueId],
    ) -> String {
        let values = values
            .iter()
            .map(|value| self.value_move(*value))
            .collect::<Vec<_>>()
            .join(", ");
        let Some(result) = result else {
            return format!("vec![{values}]");
        };
        let result_ty = self.value_type(function, result);
        if matches!(result_ty.kind(), MirTypeKind::FixedList { .. }) {
            return format!("[{values}]");
        }
        if !self.is_columnar_list(result_ty) {
            return format!("vec![{values}]");
        }
        let list_ty = self.rust_type(result_ty);
        let path = match list_ty.split_once('<') {
            Some((head, args)) => format!("{head}::<{args}"),
            None => panic!("MIR columnar list type is missing its row argument"),
        };
        format!("{path}::from_aos(vec![{values}])")
    }

    fn operation_expression(
        &self,
        function: &MirFunction,
        operation: &MirOperation,
        result: Option<MirValueId>,
        location: Option<MirPanicLoc>,
    ) -> String {
        match operation {
            MirOperation::Parameter { index, .. } => self.parameter_expression(function, *index),
            MirOperation::Capture { slot } => self.capture_expression(function, *slot),
            MirOperation::Global { name } => self.global_expression(name),
            MirOperation::Phi { incoming } => self.phi_expression(function, incoming),
            MirOperation::ReadPlace(place) => self.place_read(function, *place),
            MirOperation::MovePlace { place } => self.move_place(function, *place),
            MirOperation::WritePlace { .. } => "()".to_string(),
            MirOperation::InitializeUninit { .. } => "()".to_string(),
            MirOperation::Copy { value } => {
                let value_ty = self.value_type(function, *value);
                if is_allocator_result_type(value_ty) {
                    self.value_move(*value)
                } else if matches!(value_ty.kind(), MirTypeKind::Int) {
                    self.value_read(*value)
                } else {
                    self.value_copy(*value)
                }
            }
            MirOperation::Move { value } => self.value_move(*value),
            MirOperation::Constant(constant) => result
                .map(|value| self.constant_for_type(constant, self.value_type(function, value)))
                .unwrap_or_else(|| self.constant(constant)),
            MirOperation::Unary { op, value } => self.unary(*op, format!("*({})", self.value_slot_reference(*value, false)), self.value_type(function, *value)),
            MirOperation::Binary { op, dispatch, left, right } => self.binary_with_dispatch(function, *op, dispatch, *left, *right, location.as_ref()),
            MirOperation::BuildString { parts } => self.build_string(parts),
            MirOperation::BuildList { values } => self.build_list(function, result, values),
            MirOperation::BuildMap { entries } => format!("[{pairs}].into_iter().collect::<{root}JetMap<_,_>>()", root = self.config.root_prefix, pairs = entries.iter().map(|(key, value)| format!("({}, {})", self.value_move(*key), self.value_move(*value))).collect::<Vec<_>>().join(", ")),
            MirOperation::EnumIs { subject, owner, variant } => self.enum_is(*subject, *owner, variant),
            MirOperation::EnumPayload { subject, owner, variant, index } => self.enum_payload(function, result, *subject, *owner, variant, *index),
            MirOperation::OptionIsSome { subject } => format!("matches!({}, Ok(_))", self.value_slot_reference(*subject, false)),
            MirOperation::OptionValue { subject } => format!("match {} {{ Ok(value) => value, Err(_) => unreachable!(\"MIR option payload missing\") }}", self.value_move(*subject)),
            MirOperation::ResultIsOk { subject } => format!("matches!({}, Ok(_))", self.value_slot_reference(*subject, false)),
            MirOperation::ResultValue { subject, ok } => {
                if *ok {
                    if is_allocator_result_type(self.value_type(function, *subject)) {
                        format!("match {} {{ Ok(value) => value, Err(_) => unreachable!(\"MIR result success payload missing\") }}", self.value_move(*subject))
                    } else {
                        format!("match {} {{ Ok(value) => value, Err(_) => unreachable!(\"MIR result success payload missing\") }}", self.value_move(*subject))
                    }
                } else {
                    if is_allocator_result_type(self.value_type(function, *subject)) {
                        format!("match {} {{ Ok(_) => unreachable!(\"MIR result error payload missing\"), Err(error) => error }}", self.value_move(*subject))
                    } else {
                        format!("match {} {{ Ok(_) => unreachable!(\"MIR result error payload missing\"), Err(error) => error }}", self.value_move(*subject))
                    }
                }
            }
            MirOperation::PatternCapture { matched, index } => self.pattern_capture(function, *matched, *index, result),
            MirOperation::PatternMatched { matched } => format!("matches!({}, Ok(_))", self.value_slot_reference(*matched, false)),
            MirOperation::ProjectMembers { base, members } => self.project_members(*base, members),
            MirOperation::Index { call, base, index, kind, access, location, context } => {
                self.index(function, *call, *base, *index, *kind, *access, *location, context)
            }
            MirOperation::Slice { call, base, start, end, range, location } => self.slice(*call, *base, *start, *end, *range, *location),
            MirOperation::Range { start, end, exclusive } => format!("{}{}{}", self.value_read(*start), if *exclusive { ".." } else { "..=" }, self.value_read(*end)),
            MirOperation::Field { base, field } => self.history_field_expression(function, *base, *field),
            MirOperation::Struct { type_id, fields } => {
                self.rust_struct_literal(*type_id, fields, &[], false)
            },
            MirOperation::Enum { type_id, variant, args } => self.enum_value(function, *type_id, variant, args, location.as_ref()),
            MirOperation::Tuple { fields, .. } => format!(
                "({})",
                fields
                    .iter()
                    .map(|(_, value)| self.value_move(*value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirOperation::Present { value } | MirOperation::ResultOk { value } => format!("Ok({})", self.value_move(*value)),
            MirOperation::Convert { value, parameters, target, conversion } => self.conversion(function, *value, parameters, target, conversion),
            MirOperation::Absent => format!("{}JetOutcome::Err({}JetAbsent)", self.config.root_prefix, self.config.root_prefix),
            MirOperation::ResultErr { value } => format!("Err({})", self.value_move(*value)),
            MirOperation::Call { callee, args, type_args } => {
                let emitted = self.call(function, callee, args, type_args);
                if matches!(callee, MirCallee::Prelude(_) | MirCallee::Core(_)) {
                    result
                        .and_then(|value| {
                            self.native_int_result(&emitted, self.value_type(function, value))
                        })
                        .unwrap_or(emitted)
                } else {
                    emitted
                }
            }
            MirOperation::CoreCall { call, route, args, type_args, data_plan, fallibility, .. } => {
                let emitted = self.core_call(
                    function,
                    *call,
                    *route,
                    args,
                    type_args,
                    data_plan.as_ref(),
                    result.map(|value| self.value_type(function, value)),
                );
                match fallibility {
                    MirCallFallibility::Failure(MirFailureCarrier::Optional { .. }) => {
                        format!(
                            "({emitted}).ok_or({}JetAbsent)",
                            self.config.root_prefix
                        )
                    }
                    _ => emitted,
                }
            }
            MirOperation::IndirectCall { callee, args, type_args } => {
                self.indirect_call(function, *callee, args, type_args)
            }
            MirOperation::Closure { function: target, captures, facts } => {
                self.closure(function, *target, captures, facts, result)
            }
            MirOperation::PtrFromAddr { addr, element } => format!("({} as usize as *const {})", self.value_read(*addr), self.rust_type(element)),
            MirOperation::Deref { value } => format!("unsafe {{ (*({})).clone() }}", self.value_read(*value)),
            MirOperation::RawAddressOf { place } => self.raw_address_of(function, *place),
            MirOperation::AddressOf { place, access: MirAccess::Read } => {
                // Borrow-only users were emitted directly from the place.
                // An ordinary SSA slot holds an owned value, not its address.
                self.place_read(function, *place)
            }
            MirOperation::AddressOf { place, access } => self.address_of(function, *place, *access),
            MirOperation::AttachTag { value, .. } => self.value_move(*value),
            MirOperation::Todo { call, location, expected_type } => {
                let emitted = self.prelude_call_args_exact(*call, &[]);
                let expected = expected_type
                    .as_ref()
                    .map(|ty| self.rust_type(ty))
                    .unwrap_or_else(|| "()".to_string());
                format!(
                    "{{ let _: {expected} = {emitted}; unreachable!(\"MIR Todo at {}:{}:{}\") }}",
                    location.file.0, location.line, location.column
                )
            }
            MirOperation::Never { reason } => format!("unreachable!({:?})", reason),
            MirOperation::Semantic(operation) => self.semantic(function, operation, result, location),
            MirOperation::LoopRangeInit { call, start, end, step, exclusive } => {
                self.loop_range_init(function, *call, *start, *end, *step, *exclusive,
                    location.expect("MIR range cursor is missing its instruction source location"))
            }
            MirOperation::LoopRangeHasNext { call, cursor } => self.prelude_cursor_call(*call, *cursor, MirAccess::Read),
            MirOperation::LoopRangeValue { call, cursor } => {
                let emitted = self.prelude_cursor_call(*call, *cursor, MirAccess::Read);
                result.and_then(|value| self.native_int_result(&emitted, self.value_type(function, value)))
                    .unwrap_or(emitted)
            }
            MirOperation::LoopRangeAdvance { call, cursor } => self.prelude_cursor_call(*call, *cursor, MirAccess::Write),
            MirOperation::LoopIterInit { call, collection, step, by_value, source_kind } => {
                self.loop_iter_init(function, *call, *collection, *step, *by_value, source_kind,
                    location.expect("MIR iterator cursor is missing its instruction source location"))
            }
            MirOperation::LoopIterHasNext { call, cursor } => self.prelude_cursor_call(*call, *cursor, MirAccess::Read),
            MirOperation::LoopIterValue { call, cursor } => self.prelude_cursor_call(*call, *cursor, MirAccess::Write),
            MirOperation::LoopIterAdvance { call, cursor } => self.prelude_cursor_call(*call, *cursor, MirAccess::Write),
            MirOperation::ScopeEnter { scope, .. } => format!("{{ let _ = {}; }}", scope.0),
            MirOperation::ScopeExit { scope } => format!("{{ let _ = {}; }}", scope.0),
            MirOperation::Drop { value, kind } => self.drop_expression(function, *value, *kind),
        }
    }

    fn capture_param<'b>(&self, function: &'b MirFunction, slot: usize) -> &'b MirCaptureParam {
        let capture = function
            .capture_params
            .iter()
            .find(|capture| capture.slot == slot)
            .unwrap_or_else(|| panic!("MIR capture slot {slot} has no row"));
        if function
            .capture_params
            .get(slot)
            .is_none_or(|candidate| candidate.slot != slot)
        {
            panic!("MIR capture slots are not ordered");
        }
        capture
    }

    fn capture_move_required(&self, function: &MirFunction, slot: usize) -> bool {
        let value = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if let MirOperation::Capture { slot: candidate } = &instruction.operation {
                    (*candidate == slot).then_some(instruction.result).flatten()
                } else {
                    None
                }
            });
        value.is_some_and(|value| {
            function.places.iter().any(|place| {
                place.access == MirAccess::Move
                    && matches!(&place.base, MirPlaceBase::Capture(captured) if *captured == value)
            })
        })
    }

    fn capture_expression(&self, function: &MirFunction, slot: usize) -> String {
        let capture = self.capture_param(function, slot);
        let name = self.capture_param_name(slot);
        match capture.access {
            MirAccess::Read | MirAccess::Write => format!("(*{name}).clone()"),
            MirAccess::Move => name,
        }
    }

    fn parameter_expression(&self, function: &MirFunction, index: usize) -> String {
        let param = function
            .params
            .iter()
            .find(|param| param.index == index)
            .unwrap_or_else(|| panic!("MIR parameter index {index} has no row"));
        // The receiver is bound by the signature as `&self`/`&mut self`/`self`;
        // a borrowed receiver is always a reference, whatever its type.
        if self.is_receiver(function, param) {
            return match param.access {
                MirAccess::Read | MirAccess::Write => "(*self).clone()".to_string(),
                MirAccess::Move => "self".to_string(),
            };
        }
        let name = mangle(&param.name);
        match param.access {
            MirAccess::Write => format!("(*{name}).clone()"),
            MirAccess::Read if self.parameter_borrowed(param) => format!("(*{name}).clone()"),
            MirAccess::Read | MirAccess::Move => name,
        }
    }

    fn phi_expression(
        &self,
        _function: &MirFunction,
        incoming: &[(MirBlockId, MirValueId)],
    ) -> String {
        let arms = incoming
            .iter()
            .map(|(block, value)| format!("{} => {}", block.0, self.value_transfer(*value)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("match __jet_prev {{ {arms}, _ => unreachable!(\"invalid MIR phi predecessor\") }}")
    }

    fn unary(&self, op: MirUnaryOp, value: String, ty: &MirType) -> String {
        match ty.kind() {
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.unary(op, value, base),
            MirTypeKind::Int => {
                let operation = match op {
                    MirUnaryOp::Neg => "neg",
                    MirUnaryOp::Not => "not",
                };
                format!(
                    "{}jet_std::jet_int_owned_{operation}(&({value}))",
                    self.config.root_prefix,
                )
            }
            _ => match op {
                MirUnaryOp::Neg => format!("(-({value}))"),
                MirUnaryOp::Not => format!("(!({value}))"),
            },
        }
    }

    fn exact_int_binary(
        &self,
        function: &MirFunction,
        op: MirBinaryOp,
        left: MirValueId,
        right: MirValueId,
        location: Option<&MirPanicLoc>,
    ) -> Option<String> {
        if !matches!(
            (
                self.value_type(function, left).kind(),
                self.value_type(function, right).kind(),
            ),
            (MirTypeKind::Int, MirTypeKind::Int)
        ) {
            return None;
        }
        let (helper, contextual) = match op {
            MirBinaryOp::Add => ("jet_int_owned_add", false),
            MirBinaryOp::Sub => ("jet_int_owned_sub", false),
            MirBinaryOp::Mul => ("jet_int_owned_mul", false),
            MirBinaryOp::BitAnd => ("jet_int_owned_bit_and", false),
            MirBinaryOp::BitOr => ("jet_int_owned_bit_or", false),
            MirBinaryOp::BitXor => ("jet_int_owned_bit_xor", false),
            MirBinaryOp::Div => ("jet_int_owned_div", true),
            MirBinaryOp::FloorDiv => ("jet_int_owned_floor_div", true),
            MirBinaryOp::Mod => ("jet_int_owned_mod", true),
            MirBinaryOp::Rem => ("jet_int_owned_rem", true),
            MirBinaryOp::Pow => ("jet_int_owned_pow", true),
            MirBinaryOp::Shl => ("jet_int_owned_shl", true),
            MirBinaryOp::Shr => ("jet_int_owned_shr", true),
            _ => return None,
        };
        let left = self.value_slot_reference(left, false);
        let right = self.value_slot_reference(right, false);
        let args = if contextual {
            let location = location
                .unwrap_or_else(|| panic!("MIR exact Int arithmetic is missing source location"));
            format!(
                "{left}, {right}, {:?}, {}u32",
                self.source_file_path(location.file),
                location.line
            )
        } else {
            format!("{left}, {right}")
        };
        Some(format!(
            "{}jet_std::{helper}({args})",
            self.config.root_prefix
        ))
    }

    fn selected_equatable_impl_for_type(&self, ty: &MirType) -> bool {
        let nominal = ty.nominal_name();
        self.program.impls.iter().any(|implementation| {
            implementation
                .trait_ref
                .as_ref()
                .is_some_and(|trait_ref| trait_ref.name == crate::Generics::EQUATABLE)
                && self.module_selected(implementation.module)
                && self.impl_selected_for_target(implementation)
                && (implementation.self_type.same_checked_type(ty)
                    || nominal
                        .is_some_and(|name| implementation.self_type.nominal_name() == Some(name)))
        })
    }

    fn structural_type_def_for(&self, ty: &MirType) -> Option<&'a MirTypeDef> {
        match ty.kind() {
            MirTypeKind::Apply { name, .. } => self.program.types.iter().find(|definition| {
                Some(definition.id) == ty.identity
                    || definition.id == name.id
                    || definition.key == name.name
                    || definition.name == name.name
            }),
            MirTypeKind::Union(_) => ty.identity.and_then(|identity| {
                self.program
                    .types
                    .iter()
                    .find(|definition| definition.id == identity)
            }),
            _ => None,
        }
    }

    fn canonical_equality_needed(&self, ty: &MirType) -> bool {
        let ty = if ty.identity.is_some() {
            self.canonical_type(ty)
        } else {
            ty
        };
        match ty.kind() {
            MirTypeKind::List(_)
            | MirTypeKind::Map { .. }
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. } => true,
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.canonical_equality_needed(base),
            MirTypeKind::Apply { .. } | MirTypeKind::Union(_) => {
                self.structural_type_def_for(ty).is_some()
                    && !self.selected_equatable_impl_for_type(ty)
            }
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::Shared(_)
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::IntN { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Measure(_) => false,
        }
    }

    fn canonical_equality_field(
        &self,
        definition: &MirTypeDef,
        edge: &str,
        receiver: &str,
        field: &MirField,
    ) -> String {
        let value = format!("({receiver}).{}", self.field_name(field.id));
        if definition
            .boxed_edges
            .iter()
            .any(|candidate| candidate == edge)
        {
            format!("{value}.as_ref()")
        } else {
            format!("&{value}")
        }
    }

    fn canonical_equality_expression(
        &self,
        ty: &MirType,
        left: &str,
        right: &str,
        seen: &mut BTreeSet<MirTypeId>,
    ) -> String {
        let ty = if ty.identity.is_some() {
            self.canonical_type(ty)
        } else {
            ty
        };
        match ty.kind() {
            MirTypeKind::List(inner) => {
                let element = if self.is_columnar_list(inner) {
                    self.canonical_equality_expression(inner, "&__jet_left", "&__jet_right", seen)
                } else {
                    self.canonical_equality_expression(inner, "__jet_left", "__jet_right", seen)
                };
                let iterator = if self.is_columnar_list(inner) {
                    format!(
                        "({left}).iter_aos().zip(({right}).iter_aos()).all(|(__jet_left,__jet_right)| {element})"
                    )
                } else {
                    format!(
                        "({left}).iter().zip(({right}).iter()).all(|(__jet_left,__jet_right)| {element})"
                    )
                };
                format!("(({left}).len() == ({right}).len() && {iterator})")
            }
            MirTypeKind::Map { key, value } => {
                let key_equal = self.canonical_equality_expression(
                    key,
                    "__jet_left_key",
                    "__jet_right_key",
                    seen,
                );
                let value_equal = self.canonical_equality_expression(
                    value,
                    "__jet_left_value",
                    "__jet_right_value",
                    seen,
                );
                format!(
                    "(({left}).len() == ({right}).len() && ({left}).iter().zip(({right}).iter()).all(|((__jet_left_key,__jet_left_value),(__jet_right_key,__jet_right_value))| {key_equal} && {value_equal}))"
                )
            }
            MirTypeKind::Option(inner) => {
                let inner_equal =
                    self.canonical_equality_expression(inner, "__jet_left", "__jet_right", seen);
                format!(
                    "match ({left}, {right}) {{ (Ok(__jet_left), Ok(__jet_right)) => {inner_equal}, (Err(_), Err(_)) => true, _ => false }}"
                )
            }
            MirTypeKind::Result { ok, err } => {
                let ok_equal =
                    self.canonical_equality_expression(ok, "__jet_left", "__jet_right", seen);
                let err_equal =
                    self.canonical_equality_expression(err, "__jet_left", "__jet_right", seen);
                format!(
                    "match ({left}, {right}) {{ (Ok(__jet_left), Ok(__jet_right)) => {ok_equal}, (Err(__jet_left), Err(__jet_right)) => {err_equal}, _ => false }}"
                )
            }
            MirTypeKind::Tuple(fields) => {
                let expression = fields
                    .iter()
                    .enumerate()
                    .map(|(index, (_, field))| {
                        let left_field = format!("&({left}).{index}");
                        let right_field = format!("&({right}).{index}");
                        self.canonical_equality_expression(field, &left_field, &right_field, seen)
                    })
                    .collect::<Vec<_>>()
                    .join(" && ");
                if expression.is_empty() {
                    "true".to_string()
                } else {
                    format!("({expression})")
                }
            }
            MirTypeKind::FixedList { elem, .. } => {
                let element =
                    self.canonical_equality_expression(elem, "__jet_left", "__jet_right", seen);
                format!(
                    "({left}).iter().zip(({right}).iter()).all(|(__jet_left,__jet_right)| {element})"
                )
            }
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => {
                self.canonical_equality_expression(base, left, right, seen)
            }
            MirTypeKind::Apply { .. } | MirTypeKind::Union(_) => {
                if self.selected_equatable_impl_for_type(ty) {
                    let trait_name = crate::Codegen::rust_trait_name(crate::Generics::EQUATABLE);
                    return format!(
                        "<{} as {}>::equal({left}, {right})",
                        self.rust_type(ty),
                        trait_name
                    );
                }
                let Some(definition) = self.structural_type_def_for(ty) else {
                    return format!("(({left}) == ({right}))");
                };
                let inserted = seen.insert(definition.id);
                if !inserted {
                    return format!("(({left}) == ({right}))");
                }
                let expression = match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. } => {
                        let expression = fields
                            .iter()
                            .filter(|field| !field.computed)
                            .map(|field| {
                                let edge = field.name.as_str();
                                let left_field =
                                    self.canonical_equality_field(definition, edge, left, field);
                                let right_field =
                                    self.canonical_equality_field(definition, edge, right, field);
                                self.canonical_equality_expression(
                                    &field.ty,
                                    &left_field,
                                    &right_field,
                                    seen,
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" && ");
                        if expression.is_empty() {
                            "true".to_string()
                        } else {
                            format!("({expression})")
                        }
                    }
                    MirTypeDefKind::Enum { variants, .. } => {
                        let arms = variants
                            .iter()
                            .map(|variant| {
                                let head =
                                    self.variant_path(Some(definition.id), &variant.name, false);
                                match &variant.payload {
                                    MirVariantPayload::Unit => {
                                        format!("({head}, {head}) => true")
                                    }
                                    MirVariantPayload::Single(payload) => {
                                        let left_value = if definition
                                            .boxed_edges
                                            .iter()
                                            .any(|candidate| candidate == &variant.name)
                                        {
                                            "__jet_left.as_ref()"
                                        } else {
                                            "__jet_left"
                                        };
                                        let right_value = if definition
                                            .boxed_edges
                                            .iter()
                                            .any(|candidate| candidate == &variant.name)
                                        {
                                            "__jet_right.as_ref()"
                                        } else {
                                            "__jet_right"
                                        };
                                        let equal = self.canonical_equality_expression(
                                            payload,
                                            left_value,
                                            right_value,
                                            seen,
                                        );
                                        format!(
                                            "({head}(__jet_left), {head}(__jet_right)) => {equal}"
                                        )
                                    }
                                    MirVariantPayload::Named(fields) => {
                                        let visible = fields
                                            .iter()
                                            .filter(|field| !field.computed)
                                            .collect::<Vec<_>>();
                                        let left_bindings = visible
                                            .iter()
                                            .enumerate()
                                            .map(|(index, field)| {
                                                format!(
                                                    "{}: __jet_left_{index}",
                                                    self.field_name(field.id)
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        let right_bindings = visible
                                            .iter()
                                            .enumerate()
                                            .map(|(index, field)| {
                                                format!(
                                                    "{}: __jet_right_{index}",
                                                    self.field_name(field.id)
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        let expression = visible
                                            .iter()
                                            .enumerate()
                                            .map(|(index, field)| {
                                                let edge =
                                                    format!("{}.{}", variant.name, field.name);
                                                let left_value = if definition
                                                    .boxed_edges
                                                    .iter()
                                                    .any(|candidate| candidate == &edge)
                                                {
                                                    format!("__jet_left_{index}.as_ref()")
                                                } else {
                                                    format!("__jet_left_{index}")
                                                };
                                                let right_value = if definition
                                                    .boxed_edges
                                                    .iter()
                                                    .any(|candidate| candidate == &edge)
                                                {
                                                    format!("__jet_right_{index}.as_ref()")
                                                } else {
                                                    format!("__jet_right_{index}")
                                                };
                                                self.canonical_equality_expression(
                                                    &field.ty,
                                                    &left_value,
                                                    &right_value,
                                                    seen,
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" && ");
                                        let equal = if expression.is_empty() {
                                            "true".to_string()
                                        } else {
                                            format!("({expression})")
                                        };
                                        let left_pattern = if left_bindings.is_empty() {
                                            format!("{head} {{ .. }}")
                                        } else {
                                            format!("{head} {{ {left_bindings}, .. }}")
                                        };
                                        let right_pattern = if right_bindings.is_empty() {
                                            format!("{head} {{ .. }}")
                                        } else {
                                            format!("{head} {{ {right_bindings}, .. }}")
                                        };
                                        format!("({left_pattern}, {right_pattern}) => {equal}")
                                    }
                                }
                            })
                            .chain(std::iter::once("_ => false".to_string()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("match ({left}, {right}) {{ {arms} }}")
                    }
                    MirTypeDefKind::Distinct { base, .. } => {
                        let left_base = format!("&({left}).0");
                        let right_base = format!("&({right}).0");
                        self.canonical_equality_expression(base, &left_base, &right_base, seen)
                    }
                    MirTypeDefKind::UnitFamily { members } => {
                        let arms = members
                            .iter()
                            .map(|member| {
                                let head = format!(
                                    "{}::{}",
                                    self.type_name(definition.id),
                                    mangle(member)
                                );
                                format!("({head}, {head}) => true")
                            })
                            .chain(std::iter::once("_ => false".to_string()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("match ({left}, {right}) {{ {arms} }}")
                    }
                    MirTypeDefKind::Alias { target } => {
                        self.canonical_equality_expression(target, left, right, seen)
                    }
                };
                seen.remove(&definition.id);
                expression
            }
            MirTypeKind::Int
            | MirTypeKind::Float
            | MirTypeKind::Bool
            | MirTypeKind::String
            | MirTypeKind::Char
            | MirTypeKind::Shared(_)
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::IntN { .. }
            | MirTypeKind::Float32
            | MirTypeKind::Measure(_) => format!("(({left}) == ({right}))"),
        }
    }

    fn canonical_equality(
        &self,
        function: &MirFunction,
        left: MirValueId,
        right: MirValueId,
    ) -> Option<String> {
        let ty = self.value_type(function, left);
        if !self.canonical_equality_needed(ty) {
            return None;
        }
        let left = self.value_read(left);
        let right = self.value_read(right);
        let mut seen = BTreeSet::new();
        let equal =
            self.canonical_equality_expression(ty, "&__jet_left", "&__jet_right", &mut seen);
        Some(format!(
            "{{ let __jet_left = {left}; let __jet_right = {right}; {equal} }}"
        ))
    }

    fn binary_with_dispatch(
        &self,
        function: &MirFunction,
        op: MirBinaryOp,
        dispatch: &MirBinaryDispatch,
        left: MirValueId,
        right: MirValueId,
        location: Option<&MirPanicLoc>,
    ) -> String {
        match dispatch {
            MirBinaryDispatch::Primitive => {
                if matches!(op, MirBinaryOp::Eq | MirBinaryOp::Ne) {
                    if let Some(equal) = self.canonical_equality(function, left, right) {
                        return if op == MirBinaryOp::Eq {
                            equal
                        } else {
                            format!("!({equal})")
                        };
                    }
                }
                self.exact_int_binary(function, op, left, right, location)
                    .unwrap_or_else(|| {
                        let operand = |value| {
                            if op == MirBinaryOp::Compare {
                                self.value_slot_reference(value, false)
                            } else {
                                self.value_read(value)
                            }
                        };
                        self.binary(op, operand(left), operand(right))
                    })
            }
            MirBinaryDispatch::Prelude { call, location } => {
                let row = self.prelude_row(*call);
                let symbol = self.prelude_symbol(*call);
                let adapter = self.exact_prelude_adapter(&symbol);
                let mut args = [left, right]
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        if adapter.is_some_and(|(_, positions)| positions.contains(&index))
                            || row
                                .signature
                                .borrow_mask
                                .get(index)
                                .copied()
                                .unwrap_or(false)
                        {
                            self.value_slot_reference(value, false)
                        } else {
                            self.value_read(value)
                        }
                    })
                    .collect::<Vec<_>>();
                let extras = location
                    .as_ref()
                    .map(|location| {
                        vec![
                            format!("{:?}", self.source_file_path(location.file)),
                            format!("{}u32", location.line),
                        ]
                    })
                    .unwrap_or_default();
                self.append_prelude_context(*call, &mut args, &extras);
                self.validate_prelude_count(row, args.len());
                let args = args
                    .into_iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        if index >= 2 && row.signature.borrow_mask[index] {
                            format!("&({arg})")
                        } else {
                            arg
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let symbol = adapter
                    .map(|(symbol, _)| format!("{}{symbol}", self.config.root_prefix))
                    .unwrap_or(symbol);
                format!("{symbol}({args})")
            }
        }
    }

    fn binary(&self, op: MirBinaryOp, left: String, right: String) -> String {
        let operator = match op {
            MirBinaryOp::Add => "+",
            MirBinaryOp::Sub => "-",
            MirBinaryOp::Mul => "*",
            MirBinaryOp::Div => "/",
            MirBinaryOp::FloorDiv => ".jet_floor_div",
            MirBinaryOp::Mod => ".jet_mod",
            MirBinaryOp::Rem => ".rem",
            MirBinaryOp::Pow => ".jet_pow",
            MirBinaryOp::BitAnd => "&",
            MirBinaryOp::BitOr => "|",
            MirBinaryOp::BitXor => "^",
            MirBinaryOp::Shl => "<<",
            MirBinaryOp::Shr => ">>",
            MirBinaryOp::Eq => "==",
            MirBinaryOp::Ne => "!=",
            MirBinaryOp::Lt => "<",
            MirBinaryOp::Gt => ">",
            MirBinaryOp::Le => "<=",
            MirBinaryOp::Ge => ">=",
            MirBinaryOp::Compare => {
                let root = &self.config.root_prefix;
                return format!(
                    "match ({left}).cmp(&({right})) {{ std::cmp::Ordering::Less => {root}__jet_Ordering::__jet_Less, std::cmp::Ordering::Equal => {root}__jet_Ordering::__jet_Equal, std::cmp::Ordering::Greater => {root}__jet_Ordering::__jet_Greater }}"
                );
            }
            MirBinaryOp::And => "&&",
            MirBinaryOp::Or => "||",
        };
        if matches!(
            op,
            MirBinaryOp::FloorDiv | MirBinaryOp::Mod | MirBinaryOp::Rem | MirBinaryOp::Pow
        ) {
            format!("({left}){operator}({right})")
        } else {
            format!("(({left}) {operator} ({right}))")
        }
    }
    fn overflow_option(
        &self,
        call: MirPreludeCallId,
        left: MirValueId,
        right: MirValueId,
        location: &Option<jet_foundation::MIR::MirPanicLoc>,
    ) -> String {
        let mut args = vec![self.value_read(left), self.value_read(right)];
        let extras = location
            .as_ref()
            .map(|location| {
                vec![
                    format!("{:?}", self.source_file_path(location.file)),
                    format!("{}u32", location.line),
                ]
            })
            .unwrap_or_default();
        self.append_prelude_context(call, &mut args, &extras);
        self.prelude_call_args_exact(call, &args)
    }
    fn require_locals(&self, function: &MirFunction, context: &MirPanicContext) -> String {
        if self.is_core_layer() {
            if context.locals.is_empty() {
                return "None::<core::fmt::Arguments<'_>>".to_string();
            }
            let format_string = context
                .locals
                .iter()
                .map(|(name, _)| format!("{name} = {{}}"))
                .collect::<Vec<_>>()
                .join(", ");
            let values = context
                .locals
                .iter()
                .map(|(_, local)| format!("({})", self.local_read(function, *local)))
                .collect::<Vec<_>>()
                .join(", ");
            return format!("Some(core::format_args!({format_string:?}, {values}))");
        }
        if context.locals.is_empty() {
            return "String::new()".to_string();
        }
        let format_string = context
            .locals
            .iter()
            .map(|(name, _)| format!("{name} = {{}}"))
            .collect::<Vec<_>>()
            .join(", ");
        let values = context
            .locals
            .iter()
            .map(|(_, local)| format!("({}).jet_debug()", self.local_read(function, *local)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("format!({format_string:?}, {values})")
    }

    fn require_stop(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        kind: MirRequireKind,
        condition: Option<MirValueId>,
        location: MirPanicLoc,
        context: &MirPanicContext,
        values: &[MirValueId],
    ) -> String {
        let file = format!("{:?}", self.source_file_path(location.file));
        let line = format!("{}u32", location.line);
        let function_name = format!("{:?}", context.function);
        let source_line = format!("{:?}", context.source_line);
        let column = format!("{}u32", location.column);
        let caret = format!("{}u32", context.caret);
        let locals = if self.is_core_layer() {
            format!(
                "if cfg!(debug_assertions) {{ {} }} else {{ None::<core::fmt::Arguments<'_>> }}",
                self.require_locals(function, context)
            )
        } else {
            format!(
                "&if cfg!(debug_assertions) {{ {} }} else {{ String::new() }}",
                self.require_locals(function, context)
            )
        };
        let args = match kind {
            MirRequireKind::Require => {
                let condition = condition
                    .unwrap_or_else(|| panic!("MIR require route has no checked condition value"));
                let message = match values {
                    [] => "\"condition failed\"".to_string(),
                    [message] => {
                        if self.is_core_layer() {
                            self.value_read(*message)
                        } else {
                            format!("&({})", self.value_read(*message))
                        }
                    }
                    _ => panic!("MIR require expects only an optional diagnostic message"),
                };
                vec![
                    self.value_read(condition),
                    message,
                    file,
                    line,
                    function_name,
                    source_line,
                    column,
                    caret,
                    locals,
                ]
            }
            MirRequireKind::RequireEq => {
                let condition = condition.unwrap_or_else(|| {
                    panic!("MIR require_eq route has no checked condition value")
                });
                let [left, right] = values else {
                    panic!("MIR require_eq expects exactly two diagnostic values");
                };
                vec![
                    self.value_read(condition),
                    if self.is_core_layer() {
                        self.value_read(*left)
                    } else {
                        format!("&({}).jet_debug()", self.value_read(*left))
                    },
                    if self.is_core_layer() {
                        self.value_read(*right)
                    } else {
                        format!("&({}).jet_debug()", self.value_read(*right))
                    },
                    file,
                    line,
                    function_name,
                    source_line,
                    column,
                    caret,
                    locals,
                ]
            }
            MirRequireKind::Panic => {
                if condition.is_some() {
                    panic!("MIR panic must not carry a checked condition value");
                }
                let [message] = values else {
                    panic!("MIR panic expects exactly one message value");
                };
                vec![
                    file,
                    line,
                    function_name,
                    source_line,
                    column,
                    caret,
                    if self.is_core_layer() {
                        self.value_read(*message)
                    } else {
                        format!("&({})", self.value_read(*message))
                    },
                    locals,
                ]
            }
        };
        self.prelude_call_args_exact(call, &args)
    }
    fn http_route_handler_adapter(
        &self,
        function: &MirFunction,
        handler: MirValueId,
        handler_param_names: &[String],
        contract_json: &str,
    ) -> String {
        let parameter_types = match self.value_type(function, handler).kind() {
            MirTypeKind::Fn(signature) => signature.params.iter().collect::<Vec<_>>(),
            MirTypeKind::SendFn { params, .. } => params.iter().collect::<Vec<_>>(),
            other => panic!(
                "MIR HTTP route handler has non-callable type {}",
                other.display_name()
            ),
        };
        if parameter_types.len() != handler_param_names.len() {
            panic!(
                "MIR HTTP route handler metadata has {} names for {} parameters",
                handler_param_names.len(),
                parameter_types.len()
            );
        }
        let request_type = format!("{}JetHTTPRequest", self.config.root_prefix);
        let handler_name = format!("__jet_http_route_handler_{}", handler.0);
        let invalid = format!(
            "{}jet_http_srv_response_owned(400, \"invalid route parameter\".to_string())",
            self.config.root_prefix
        );
        let invalid_body = format!(
            "{}jet_http_srv_response_owned(400, \"invalid request body\".to_string())",
            self.config.root_prefix
        );
        let request_validation = format!(
            "let __jet_request = match {}jet_http_route_validate_body(__jet_request, {:?}) {{ \
             Ok(request) => request, \
             Err(()) => return Ok({invalid_body}), \
             }};",
            self.config.root_prefix, contract_json,
        );
        let mut bindings = String::new();
        let mut call_args = Vec::with_capacity(parameter_types.len());
        for (index, (parameter_type, access)) in
            Self::callable_parameters(self.value_type(function, handler)).enumerate()
        {
            if parameter_type.nominal_name() == Some("HTTPRequest") {
                call_args.push(self.callable_argument_from_owned(
                    parameter_type,
                    access,
                    "__jet_request".to_string(),
                ));
                continue;
            }
            let name = &handler_param_names[index];
            if name.is_empty() {
                panic!(
                    "MIR HTTP route handler parameter {} has no source name",
                    index
                );
            }
            let rust_type = self.rust_type(parameter_type);
            let helper = if matches!(parameter_type.kind(), MirTypeKind::Option(_)) {
                "jet_http_route_optional"
            } else {
                "jet_http_route_param"
            };
            let route_value = format!(
                "{}{}::<{}>(&__jet_request, {:?})",
                self.config.root_prefix, helper, rust_type, name
            );
            let binding = if helper == "jet_http_route_optional" {
                format!(
                    "let {}__jet_route_arg_{index}: {rust_type} = match {route_value} {{ \
                     Ok(value) => value, \
                     Err(_) => return Ok({invalid}), \
                     }};",
                    if access == MirAccess::Write { "mut " } else { "" }
                )
            } else {
                format!(
                    "let {}__jet_route_arg_{index}: {rust_type} = match {route_value} {{ \
                     Ok(Some(value)) => value, \
                     _ => return Ok({invalid}), \
                     }};",
                    if access == MirAccess::Write { "mut " } else { "" }
                )
            };
            bindings.push_str(&binding);
            let argument = format!("__jet_route_arg_{index}");
            call_args.push(self.callable_argument_from_owned(
                parameter_type,
                access,
                argument,
            ));
        }
        let handler_call = if !self.history_runtime_metadata_enabled()
            && matches!(
                self.value_type(function, handler).kind(),
                MirTypeKind::Fn(_)
            ) {
            self.fn_value_call(format!("{handler_name}.clone()"), &call_args)
        } else {
            format!("({handler_name})({})", call_args.join(", "))
        };
        format!(
            "{{ let {handler_name} = {}; std::sync::Arc::new(move |mut __jet_request: {request_type}| {{ {request_validation} {bindings} {handler_call} }}) }}",
            self.value_move(handler),
        )
    }
    fn http_route_zero_handler_adapter(
        &self,
        function: &MirFunction,
        handler: MirValueId,
        handler_param_names: &[String],
    ) -> String {
        let parameter_count = match self.value_type(function, handler).kind() {
            MirTypeKind::Fn(signature) => signature.params.len(),
            MirTypeKind::SendFn { params, .. } => params.len(),
            other => panic!(
                "MIR HTTP zero route handler has non-callable type {}",
                other.display_name()
            ),
        };
        if parameter_count != 0 || !handler_param_names.is_empty() {
            panic!("MIR HTTP zero route handler has checked parameters");
        }
        let handler_name = format!("__jet_http_zero_route_handler_{}", handler.0);
        let handler_call = if !self.history_runtime_metadata_enabled()
            && matches!(
                self.value_type(function, handler).kind(),
                MirTypeKind::Fn(_)
            ) {
            self.fn_value_call(format!("{handler_name}.clone()"), &[])
        } else {
            format!("({handler_name})()")
        };
        format!(
            "{{ let {handler_name} = {}; std::sync::Arc::new(move || {{ {handler_call} }}) }}",
            self.value_move(handler),
        )
    }

    fn http_router_register(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        receiver: MirValueId,
        path: MirValueId,
        handler: MirValueId,
        method: MirHttpMethod,
        handler_param_names: &[String],
        contract_json: &str,
        location: MirPanicLoc,
    ) -> String {
        let row = self.prelude_row(call);
        let adapted_handler = if row.member == "mux_add_zero" {
            self.http_route_zero_handler_adapter(function, handler, handler_param_names)
        } else {
            self.http_route_handler_adapter(function, handler, handler_param_names, contract_json)
        };
        let args = if matches!(row.member.as_str(), "mux_add" | "mux_add_zero") {
            vec![
                self.value_borrow_mut(receiver),
                format!("{:?}.to_string()", method.as_str()),
                self.value_move(path),
                adapted_handler,
            ]
        } else {
            let file = format!("{:?}", self.source_file_path(location.file));
            let line = format!("{}u32", location.line);
            vec![
                self.value_borrow_mut(receiver),
                format!("{:?}.to_string()", method.as_str()),
                self.value_move(path),
                adapted_handler,
                file,
                line,
                format!("{:?}.to_string()", contract_json),
            ]
        };
        self.prelude_call_args_exact(call, &args)
    }

    fn columnar_read(
        &self,
        accessor: MirPreludeCallId,
        base: MirValueId,
        column: MirFieldId,
        column_index: usize,
        index: MirValueId,
        location: Option<MirPanicLoc>,
    ) -> String {
        let location = location.unwrap_or_else(|| {
            panic!("MIR columnar read is missing its instruction source location")
        });
        let file = format!("{:?}", self.source_file_path(location.file));
        let line = format!("{}u32", location.line);
        let emitted = self.prelude_call_args_exact(
            accessor,
            &[
                format!("&({})", self.value_read(base)),
                format!("{column_index}usize"),
                self.value_read(index),
            ],
        );
        format!(
            "{{ match {emitted} {{ Ok(__cell) => __cell.jet_col_{}(), Err(__error) => {}jet_arithmetic_stop({file}, {line}, &__error.message()) }} }}",
            self.field_name(column),
            self.config.root_prefix
        )
    }

    fn build_string(&self, parts: &[MirStringPart]) -> String {
        if self.is_core_layer() {
            let mut literal = String::new();
            for part in parts {
                match part {
                    MirStringPart::Literal(value) => literal.push_str(value),
                    MirStringPart::Value(_) => {
                        panic!("MIR Core string interpolation requires an allocation layer")
                    }
                }
            }
            return format!("{literal:?}");
        }
        let mut out = String::from("{ let mut __jet_string = String::new();");
        for part in parts {
            match part {
                MirStringPart::Literal(value) => {
                    let _ = write!(out, " __jet_string.push_str({value:?});");
                }
                MirStringPart::Value(value) => {
                    let _ = write!(
                        out,
                        " __jet_string.push_str(&({}).jet_show());",
                        self.value_read(*value)
                    );
                }
            }
        }
        out.push_str(" __jet_string }");
        out
    }

    fn project_members(&self, base: MirValueId, members: &[MirFieldId]) -> String {
        let mut expression = self.value_read(base);
        for member in members {
            expression = format!("({expression}).{}", self.field_name(*member));
        }
        expression
    }

    fn call(
        &self,
        function: &MirFunction,
        callee: &MirCallee,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let invocation = match callee {
            MirCallee::User(id) => {
                let row = self.function_row(*id);
                if !row.capture_params.is_empty() {
                    panic!("MIR captured function called without a Closure operation");
                }
                let borrow_mask = self.parameter_borrow_mask(&row.params);
                self.call_symbol_for_function(
                    function,
                    self.function_name(*id),
                    args,
                    if row.generic_params.is_empty() {
                        &[]
                    } else {
                        type_args
                    },
                    Some(&borrow_mask),
                )
            }
            MirCallee::Associated {
                function: target,
                owner,
            } => self.associated_call(function, *target, owner, args, type_args),
            MirCallee::Method {
                function: target,
                owner,
            } => self.method_call(function, *target, owner, args, type_args),
            MirCallee::TraitMethod {
                method,
                trait_ref,
                receiver,
            } => self.trait_method_call(function, *method, trait_ref, receiver, args, type_args),
            MirCallee::Core(id) => self.core_direct_call(function, *id, args, type_args),
            MirCallee::Prelude(id) => {
                let row = self.prelude_row(*id);
                self.validate_prelude_arity(row, args);
                self.call_symbol_for_function(
                    function,
                    self.prelude_symbol(*id),
                    args,
                    type_args,
                    Some(&row.signature.borrow_mask),
                )
            }
            MirCallee::Foreign(id) => self.foreign_call(function, *id, args, type_args),
            MirCallee::Indirect(value) => {
                let borrow_mask = self.callable_borrow_mask(self.value_type(function, *value));
                self.call_symbol_for_function(
                    function,
                    self.value_read(*value),
                    args,
                    type_args,
                    Some(&borrow_mask),
                )
            }
        };
        self.history_instantiated_call(callee, type_args, invocation)
    }

    fn associated_call(
        &self,
        caller: &MirFunction,
        function: MirFunctionId,
        owner: &MirType,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let row = self.function_row(function);
        let symbol = match &row.form {
            MirFunctionForm::Method {
                owner: declared_owner,
                self_access,
            } => {
                if !declared_owner.same_checked_type(owner) {
                    panic!(
                        "MIR associated call {:?} owner does not match its function declaration",
                        function
                    );
                }
                if self_access.is_some() {
                    panic!(
                        "MIR associated call {:?} targets an instance method with no receiver",
                        function
                    );
                }
                if self.owner_independent_static(row) {
                    self.function_name(row.id)
                } else {
                    format!(
                        "<{}>::{}",
                        self.rust_type(owner),
                        self.rust_method_symbol(row)
                    )
                }
            }
            MirFunctionForm::TraitMethod {
                owner: trait_owner,
                trait_ref,
                self_access,
                ..
            } => {
                if !trait_owner.same_checked_type(owner) {
                    panic!(
                        "MIR associated call {:?} owner does not match its trait method declaration",
                        function
                    );
                }
                if self_access.is_some() {
                    panic!(
                        "MIR associated call {:?} targets an instance trait method with no receiver",
                        function
                    );
                }
                format!(
                    "<{} as {}>::{}",
                    self.rust_type(owner),
                    self.trait_nominal_name(trait_ref),
                    self.rust_method_symbol(row)
                )
            }
            MirFunctionForm::TopLevel => {
                panic!(
                    "MIR associated call {:?} does not target a checked associated method",
                    function
                );
            }
        };
        let borrow_mask = self.parameter_borrow_mask(&row.params);
        self.call_symbol_for_function(
            caller,
            symbol,
            args,
            if row.generic_params.is_empty() { &[] } else { type_args },
            Some(&borrow_mask),
        )
    }
    fn foreign_abi_name(&self, abi: &MirForeignAbi) -> String {
        match abi {
            MirForeignAbi::C => "C".to_string(),
            MirForeignAbi::CUnwind => "C-unwind".to_string(),
            MirForeignAbi::System => "system".to_string(),
            MirForeignAbi::Stdcall => "stdcall".to_string(),
            MirForeignAbi::Fastcall => "fastcall".to_string(),
            MirForeignAbi::Vectorcall => "vectorcall".to_string(),
            MirForeignAbi::Rust => "Rust".to_string(),
            MirForeignAbi::Platform(name) => name.clone(),
        }
    }
    fn foreign_language_name(&self, language: MirForeignLanguage) -> &'static str {
        match language {
            MirForeignLanguage::C => "C",
            MirForeignLanguage::Cpp => "C++",
            MirForeignLanguage::Rust => "Rust",
            MirForeignLanguage::Assembly => "assembly",
        }
    }

    fn foreign_call_arg(&self, foreign: &MirForeign, arg: &MirCallArg, param: &MirParam) -> String {
        if !(foreign.handle.is_some() && self.is_handle_type(&param.ty)) {
            return self.call_arg(arg, false);
        }
        match arg.access {
            MirAccess::Read => {
                format!("({}).as_raw()", self.value_slot_reference(arg.value, false))
            }
            MirAccess::Write => {
                format!("({}).as_raw()", self.value_slot_reference(arg.value, true))
            }
            MirAccess::Move => format!("({}).into_raw()", self.value_move(arg.value)),
        }
    }

    fn foreign_call(
        &self,
        caller: &MirFunction,
        id: MirForeignId,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        if !type_args.is_empty() {
            panic!(
                "MIR foreign call {:?} carries unexpected type arguments",
                id
            );
        }
        let foreign = self
            .program
            .foreign
            .iter()
            .find(|foreign| foreign.id == id)
            .unwrap_or_else(|| panic!("MIR foreign ID {:?} has no row", id));
        if !self.module_selected(foreign.module_id) {
            panic!("MIR foreign call {:?} targets an unselected module", id);
        }
        if !self.target_applicable(foreign.target_applicability) {
            panic!(
                "MIR foreign call {:?} is not applicable to the selected artifact target",
                id
            );
        }
        if args.len() != foreign.params.len()
            || args
                .iter()
                .zip(&foreign.params)
                .any(|(argument, parameter)| argument.access != parameter.access)
        {
            panic!(
                "MIR foreign call {:?} argument access does not match its row",
                id
            );
        }
        match foreign.callback_transport.as_deref() {
            Some("managed-close") => {
                if args.len() != 1 || args[0].access != MirAccess::Move {
                    panic!("managed callback unsubscribe must consume one registration");
                }
                let registration = self.value_move(args[0].value);
                return format!(
                    "{}jet_std::jet_ffi_callback_registration_unsubscribe_result(({registration}).into_raw())",
                    self.config.root_prefix,
                );
            }
            Some("emit-task") => {
                if args.len() != 1 {
                    panic!("managed callback emission must receive exactly one payload");
                }
                let native = self
                    .program
                    .foreign
                    .iter()
                    .find(|candidate| {
                        candidate.module_id == foreign.module_id
                            && candidate.name == "__jet_native_emit_async"
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "managed callback emission {:?} has no native emit function",
                            foreign.name
                        )
                    });
                let value = self.call_arg(&args[0], false);
                return format!(
                    "{}jet_std::JetTask::spawn(move || Ok(unsafe {{ {}({value}) }}))",
                    self.config.root_prefix,
                    self.foreign_name(native.id),
                );
            }
            Some("managed") => {
                if args.len() != 1 {
                    panic!("managed callback registration must receive exactly one callback");
                }
                let (callback_call, callback_id, lambda) = caller
                    .blocks
                    .iter()
                    .flat_map(|block| block.instructions.iter())
                    .find_map(|instruction| {
                        if instruction.result != Some(args[0].value) {
                            return None;
                        }
                        match &instruction.operation {
                            MirOperation::Semantic(MirSemanticOp::CCallback {
                                call,
                                callback,
                                lambda,
                            }) => Some((*call, *callback, *lambda)),
                            _ => None,
                        }
                    })
                    .unwrap_or_else(|| {
                        panic!("managed callback argument has no C callback adapter operation")
                    });
                let callback_row = self
                    .program
                    .callbacks
                    .iter()
                    .find(|row| row.id == callback_id)
                    .unwrap_or_else(|| {
                        panic!("managed callback {:?} has no adapter row", callback_id)
                    });
                if !callback_row.managed || callback_row.symbol.is_empty() {
                    panic!("managed callback argument has an unmanaged adapter row");
                }
                let native_start = self
                    .program
                    .foreign
                    .iter()
                    .find(|candidate| {
                        candidate.module_id == foreign.module_id
                            && candidate.name == format!("__jet_native_{}", foreign.name)
                            && candidate.callback_transport.as_deref() == Some("native-start")
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "managed callback {:?} has no native start function",
                            foreign.name
                        )
                    });
                let native_stop = self
                    .program
                    .foreign
                    .iter()
                    .find(|candidate| {
                        candidate.module_id == foreign.module_id
                            && candidate.name == "__jet_native_unsubscribe"
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "managed callback {:?} has no native unsubscribe function",
                            foreign.name
                        )
                    });
                let identity = foreign
                    .callback_identity
                    .as_deref()
                    .filter(|identity| !identity.is_empty())
                    .unwrap_or_else(|| {
                        panic!("managed callback {:?} has no identity", foreign.name)
                    });
                let digest = foreign
                    .callback_plan_digest
                    .as_deref()
                    .filter(|digest| !digest.is_empty())
                    .unwrap_or_else(|| {
                        panic!("managed callback {:?} has no plan digest", foreign.name)
                    });
                let handler = self.c_callback(caller, callback_call, callback_id, lambda);
                return format!(
                    "{}jet_std::JetFfiCallbackRegistrationHandle::from_raw({}jet_std::jet_ffi_callback_registration_start_i64(Some({} as unsafe extern \"C\" fn(*mut core::ffi::c_void, i64)), |__jet_callback, __jet_context| unsafe {{ {}(__jet_callback, __jet_context) }}, |__jet_context| unsafe {{ {}(__jet_context) }}, |_| {{}}, {handler}, {:?}, {:?}))",
                    self.config.root_prefix,
                    self.config.root_prefix,
                    callback_row.symbol,
                    self.foreign_name(native_start.id),
                    self.foreign_name(native_stop.id),
                    identity,
                    digest,
                );
            }
            _ => {}
        }
        let call_args = args
            .iter()
            .zip(&foreign.params)
            .map(|(arg, param)| self.foreign_call_arg(foreign, arg, param))
            .collect::<Vec<_>>()
            .join(", ");
        let call = format!("{}({call_args})", self.foreign_name(id));
        if foreign
            .return_type
            .as_ref()
            .is_some_and(|ty| foreign.handle.is_some() && self.is_handle_type(ty))
        {
            let ty = self.rust_type(foreign.return_type.as_ref().expect("handle return type"));
            format!("{ty}::from_raw({call})")
        } else {
            call
        }
    }

    fn method_call(
        &self,
        caller: &MirFunction,
        function: MirFunctionId,
        owner: &MirType,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let row = self.function_row(function);
        let self_access = match &row.form {
            MirFunctionForm::Method {
                owner: declared_owner,
                self_access: Some(self_access),
            }
            | MirFunctionForm::TraitMethod {
                owner: declared_owner,
                self_access: Some(self_access),
                ..
            } => {
                if !declared_owner.same_checked_type(owner) {
                    panic!(
                        "MIR method call {:?} owner does not match its function declaration",
                        function
                    );
                }
                *self_access
            }
            MirFunctionForm::Method {
                owner: declared_owner,
                self_access: None,
            }
            | MirFunctionForm::TraitMethod {
                owner: declared_owner,
                self_access: None,
                ..
            } => {
                if !declared_owner.same_checked_type(owner) {
                    panic!(
                        "MIR method call {:?} owner does not match its function declaration",
                        function
                    );
                }
                panic!(
                    "MIR method call {:?} targets an associated function",
                    function
                );
            }
            MirFunctionForm::TopLevel => {
                panic!("MIR method call {:?} does not target a method", function)
            }
        };
        let receiver = args
            .first()
            .unwrap_or_else(|| panic!("MIR method call {:?} has no receiver argument", function));
        if receiver.access != self_access {
            panic!(
                "MIR method call {:?} receiver access {:?} disagrees with declaration {:?}",
                function, receiver.access, self_access
            );
        }
        let trait_ref = match &row.form {
            MirFunctionForm::TraitMethod { trait_ref, .. } => Some(trait_ref),
            _ => None,
        };
        let receiver = self.call_arg_for_function(caller, receiver, self_access == MirAccess::Read);
        let generic = if row.generic_params.is_empty() || type_args.is_empty() {
            String::new()
        } else {
            format!(
                "::<{}>",
                type_args
                    .iter()
                    .map(|ty| self.rust_type(ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        // The receiver is `row.params[0]`, spelled `self` by `emit_callable_named`;
        // the declared parameters line up with the remaining args by index.
        let declared = self.declared_params(row);
        let params = args[1..]
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let borrowed = self.arithmetic_trait_method(row)
                    || declared
                        .get(index)
                        .is_some_and(|param| self.parameter_borrowed(param));
                self.call_arg_for_function(caller, arg, borrowed)
            })
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(trait_ref) = trait_ref {
            let mut trait_name = self.trait_nominal_name(trait_ref);
            if self.arithmetic_trait_method(row) {
                let rhs = declared.first().expect("checked arithmetic RHS");
                let _ = write!(trait_name, "<{}>", self.rust_type(&rhs.ty));
            }
            return format!(
                "<{} as {}>::{}{}({receiver}{separator}{params})",
                self.rust_type(owner),
                trait_name,
                self.rust_method_symbol(row),
                generic,
                separator = if params.is_empty() { "" } else { ", " },
            );
        }
        format!(
            "({receiver}).{}{}({params})",
            self.rust_method_symbol(row),
            generic
        )
    }
    fn trait_method_call(
        &self,
        caller: &MirFunction,
        method_id: MirTraitMethodId,
        trait_ref: &MirTraitRef,
        receiver: &MirType,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let Some(bounds) = receiver.trait_bounds() else {
            panic!("MIR trait method receiver is not a trait object");
        };
        if !bounds.iter().any(|bound| bound.name == trait_ref.name) {
            panic!(
                "MIR trait method receiver does not contain trait {}",
                trait_ref.name
            );
        }
        let definition = self
            .program
            .traits
            .iter()
            .find(|definition| definition.id == trait_ref.id)
            .unwrap_or_else(|| panic!("MIR trait {:?} is missing", trait_ref.id));
        let method = definition
            .methods
            .iter()
            .find(|method| method.id == method_id)
            .unwrap_or_else(|| panic!("MIR trait method {:?} is missing", method_id));
        let self_access = method
            .self_access
            .unwrap_or_else(|| panic!("MIR trait method {:?} has no receiver", method_id));
        let receiver_arg = args
            .first()
            .unwrap_or_else(|| panic!("MIR trait method {:?} has no receiver argument", method_id));
        if receiver_arg.access != self_access {
            panic!(
                "MIR trait method {:?} receiver access {:?} disagrees with declaration {:?}",
                method_id, receiver_arg.access, self_access
            );
        }
        if args.len() != method.params.len() + 1 {
            panic!(
                "MIR trait method {:?} has {} arguments, expected {}",
                method_id,
                args.len(),
                method.params.len() + 1
            );
        }
        let receiver = self.call_arg_for_function(caller, receiver_arg, false);
        let generic = if type_args.is_empty() {
            String::new()
        } else {
            format!(
                "::<{}>",
                type_args
                    .iter()
                    .map(|ty| self.rust_type(ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let params = args[1..]
            .iter()
            .zip(&method.params)
            .map(|(arg, param)| {
                self.call_arg_for_function(caller, arg, self.parameter_borrowed(param))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let symbol =
            if crate::Codegen::TIR::tir_to_mir_types::is_compiler_owned_trait(&trait_ref.name) {
                method.name.clone()
            } else {
                mangle(&method.name)
            };
        format!("({receiver}).{symbol}{generic}({params})")
    }

    fn compute_runtime_name(&self, name: &str) -> String {
        format!("{}{name}", self.config.root_prefix)
    }

    fn compute_tuple(&self, values: Vec<String>) -> String {
        match values.len() {
            0 => "()".to_string(),
            1 => format!("({},)", values[0]),
            _ => format!("({})", values.join(", ")),
        }
    }

    fn compute_result_shape(&self, output: &MirType) -> String {
        match output.kind() {
            MirTypeKind::Tuple(fields) => format!(
                "{}::TensorTuple({}usize)",
                self.compute_runtime_name("JetComputeResultShape"),
                fields.len()
            ),
            _ => format!(
                "{}::Tensor",
                self.compute_runtime_name("JetComputeResultShape")
            ),
        }
    }

    fn compute_base_result(&self, output: &MirType, value: &str) -> String {
        let result = self.compute_runtime_name("JetComputeBaseResult");
        match output.kind() {
            MirTypeKind::Tuple(fields) => {
                let values = fields
                    .iter()
                    .enumerate()
                    .map(|(index, _)| format!("{value}.{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{result}::TensorTuple(vec![{values}])")
            }
            _ => format!("{result}::Tensor({value})"),
        }
    }

    fn compute_gradient_value(
        &self,
        value_type: &MirType,
        gradients: &str,
        target: usize,
    ) -> String {
        match value_type.kind() {
            MirTypeKind::Tuple(fields) => self.compute_tuple(
                fields
                    .iter()
                    .enumerate()
                    .map(|(index, _)| format!("{gradients}[{target}][{index}].clone()"))
                    .collect(),
            ),
            _ => format!("{gradients}[{target}][0].clone()"),
        }
    }

    fn compute_gradient(&self, gradient_type: &MirType, gradients: &str) -> String {
        let MirTypeKind::Tuple(fields) = gradient_type.kind() else {
            panic!("MIR compute gradient result is not a named tuple");
        };
        self.compute_tuple(
            fields
                .iter()
                .enumerate()
                .map(|(target, (_, value_type))| {
                    self.compute_gradient_value(value_type, gradients, target)
                })
                .collect(),
        )
    }

    fn compute_transform_result(
        &self,
        member: &str,
        result_type: &MirType,
        result: &str,
    ) -> String {
        let result_enum = self.compute_runtime_name("JetComputeCurriedResult");
        match member {
            "gradient" => {
                let gradients = self.compute_gradient(result_type, "__jet_gradients");
                format!(
                    "match {result} {{ {result_enum}::Gradient(__jet_gradients) => {gradients}, \
                     _ => unreachable!(\"MIR compute.gradient result shape mismatch\") }}"
                )
            }
            "value_and_gradient" => {
                let MirTypeKind::Tuple(fields) = result_type.kind() else {
                    panic!("MIR compute.value_and_gradient result is not a tuple");
                };
                let values = fields
                    .iter()
                    .map(|(name, value_type)| match name.as_str() {
                        "value" => "__jet_value".to_string(),
                        "gradients" => self.compute_gradient(value_type, "__jet_gradients"),
                        other => panic!(
                            "MIR compute.value_and_gradient has unknown result field `{other}`"
                        ),
                    })
                    .collect::<Vec<_>>();
                let values = self.compute_tuple(values);
                format!(
                    "match {result} {{ \
                     {result_enum}::ValueAndGradient {{ value: __jet_value, gradients: __jet_gradients }} => {values}, \
                     _ => unreachable!(\"MIR compute.value_and_gradient result shape mismatch\") }}"
                )
            }
            "vjp" => {
                let gradient_type = match result_type.kind() {
                    MirTypeKind::Apply { name, args }
                        if name.name == "VjpRun" && args.len() == 1 =>
                    {
                        &args[0]
                    }
                    _ => panic!("MIR compute.vjp result is not VjpRun"),
                };
                let gradient = self.compute_gradient(gradient_type, "__jet_pull_gradients");
                let gradient_for_grads =
                    self.compute_gradient(gradient_type, "__jet_grads_gradients");
                let call = self.compute_runtime_name("jet_compute_call_curried_or_panic");
                let input = self.compute_runtime_name("JetComputeInputPack");
                let handle = self.compute_runtime_name("JetComputeHandle");
                let tensor = self.compute_runtime_name("JetTensor");
                let vjp_run = self.compute_runtime_name("JetComputeVjpRun");
                format!(
                    "match {result} {{ \
                     {result_enum}::Vjp {{ value: __jet_value, pull: __jet_pull_raw, grads: __jet_grads_raw }} => {{ \
                         let __jet_pull = {handle}::new(__jet_pull_raw); \
                         let __jet_grads = {handle}::new(__jet_grads_raw); \
                         {vjp_run} {{ \
                             value: __jet_value, \
                             pull: std::rc::Rc::new(move |__jet_seed: &{tensor}| {{ \
                                 let __jet_pull_result = {call}(__jet_pull.raw(), \
                                     {input}::new(vec![__jet_seed.clone()], Vec::new()), \
                                     \"core.compute.vjp.pull\"); \
                                 match __jet_pull_result {{ \
                                     {result_enum}::Gradient(__jet_pull_gradients) => {gradient}, \
                                     _ => unreachable!(\"MIR compute.vjp pull result shape mismatch\") \
                                 }} \
                             }}), \
                             grads: std::rc::Rc::new(move || {{ \
                                 let __jet_grads_result = {call}(__jet_grads.raw(), \
                                     {input}::new(Vec::new(), Vec::new()), \
                                     \"core.compute.vjp.grads\"); \
                                 match __jet_grads_result {{ \
                                     {result_enum}::Gradient(__jet_grads_gradients) => {gradient_for_grads}, \
                                     _ => unreachable!(\"MIR compute.vjp grads result shape mismatch\") \
                                 }} \
                             }}) \
                         }} \
                     }}, \
                     _ => unreachable!(\"MIR compute.vjp result shape mismatch\") }}"
                )
            }
            "jvp" => {
                let MirTypeKind::Tuple(fields) = result_type.kind() else {
                    panic!("MIR compute.jvp result is not a tuple");
                };
                let values = fields
                    .iter()
                    .map(|(name, _)| match name.as_str() {
                        "value" => "__jet_value".to_string(),
                        "tangent" => "__jet_tangent".to_string(),
                        other => panic!("MIR compute.jvp has unknown result field `{other}`"),
                    })
                    .collect::<Vec<_>>();
                let values = self.compute_tuple(values);
                format!(
                    "match {result} {{ \
                     {result_enum}::Jvp {{ value: __jet_value, tangent: __jet_tangent }} => {values}, \
                     _ => unreachable!(\"MIR compute.jvp result shape mismatch\") }}"
                )
            }
            other => panic!("MIR compute adapter has unknown member `{other}`"),
        }
    }

    fn compute_call(
        &self,
        function: &MirFunction,
        route: MirPreludeCallId,
        member: &str,
        args: &[MirCallArg],
        type_args: &[MirType],
        result_type: Option<&MirType>,
    ) -> String {
        let [callable_type] = type_args else {
            panic!("MIR compute.{member} needs one callable type argument");
        };
        let Some(result_type) = result_type else {
            panic!("MIR compute.{member} has no checked result type");
        };
        if args.len() < 2 {
            panic!("MIR compute.{member} needs a callable and target list");
        }
        let (parameters, return_type, send_callable) = match callable_type.kind() {
            MirTypeKind::Fn(signature) => {
                (signature.params.as_slice(), signature.ret.as_deref(), false)
            }
            MirTypeKind::SendFn { params, ret } => (params.as_slice(), ret.as_deref(), true),
            other => panic!(
                "MIR compute.{member} callable has non-function type {}",
                other.display_name()
            ),
        };
        let output = return_type
            .unwrap_or_else(|| panic!("MIR compute.{member} callable has no return type"));
        let output = match output.kind() {
            MirTypeKind::Result { ok, .. } => ok.as_ref(),
            _ => output,
        };
        let callable = match args[0].access {
            MirAccess::Read => format!(
                "({}).clone()",
                self.borrowed_value_reference(function, args[0].value, MirAccess::Read)
            ),
            MirAccess::Move => self.value_move(args[0].value),
            MirAccess::Write => {
                panic!("MIR compute.{member} callable cannot be a write argument")
            }
        };
        let callable_borrow_mask = self.callable_borrow_mask(callable_type);
        let call_args = parameters
            .iter()
            .enumerate()
            .map(|(index, _)| {
                if callable_borrow_mask.get(index).copied().unwrap_or(false) {
                    format!("&__jet_compute_inputs[{index}]")
                } else {
                    format!("__jet_compute_inputs[{index}].clone()")
                }
            })
            .collect::<Vec<_>>();
        let base_call = if send_callable || self.history_runtime_metadata_enabled() {
            format!("(__jet_compute_callable)({})", call_args.join(", "))
        } else {
            self.fn_value_call("__jet_compute_callable.clone()".to_string(), &call_args)
        };
        let checked_output =
            return_type.is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Result { .. }));
        let base_value = if checked_output {
            let error = match return_type.map(MirType::kind) {
                Some(MirTypeKind::Result { err, .. })
                    if err.nominal_name() == Some(jet_foundation::Syntax::TYPE_ERR) =>
                {
                    "__jet_compute_error"
                }
                _ => "__jet_compute_error.jet_show()",
            };
            format!(
                "match {base_call} {{ \
                 Ok(__jet_compute_value) => __jet_compute_value, \
                 Err(__jet_compute_error) => return Err({}::Unsupported(format!(\
                     \"autodiff callable failed: {{}}\", {error}))) \
                 }}",
                self.compute_runtime_name("JetComputeError")
            )
        } else {
            base_call
        };
        let base = format!(
            "{{ let __jet_compute_callable = {callable}; \
             {}::new({}usize, move |__jet_compute_inputs: &[{}]| {{ \
                 let __jet_compute_value = {base_value}; \
                 Ok({}) \
             }}) }}",
            self.compute_runtime_name("JetComputeBase"),
            parameters.len(),
            self.compute_runtime_name("JetTensor"),
            self.compute_base_result(output, "__jet_compute_value")
        );
        let target = format!(
            "({}).iter().map(|value| {}jet_std::jet_int_owned_to_i64(value).expect(\"checked autodiff target index\")).collect::<Vec<_>>()",
            self.value_read(args.last().unwrap().value),
            self.config.root_prefix,
        );
        let kind = match member {
            "gradient" => "Gradient",
            "value_and_gradient" => "ValueAndGradient",
            "vjp" => "Vjp",
            "jvp" => "Jvp",
            other => panic!("MIR compute adapter has unknown member `{other}`"),
        };
        let kind = format!(
            "{}::{kind}",
            self.compute_runtime_name("JetComputeTransformKind")
        );
        let plan = format!(
            "{}({base}, {kind}, &{target}, {})",
            self.prelude_symbol(route),
            self.compute_result_shape(output)
        );
        let handle = self.compute_runtime_name("JetComputeHandle");
        let call = self.compute_runtime_name("jet_compute_call_curried_or_panic");
        let input = self.compute_runtime_name("JetComputeInputPack");
        if let MirTypeKind::Fn(signature) = result_type.kind() {
            let ret = signature
                .ret
                .as_deref()
                .unwrap_or_else(|| panic!("MIR compute.{member} transform has no return type"));
            let parameters = signature
                .params
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    let conventions = signature
                        .call_metadata
                        .as_ref()
                        .map(|metadata| metadata.conventions.as_slice());
                    let access =
                        Self::callable_parameter_access(conventions, index);
                    format!(
                        "__jet_compute_arg_{index}: {}",
                        self.callable_parameter_type(ty, access)
                    )
                })
                .collect::<Vec<_>>();
            let values = signature
                .params
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    let conventions = signature
                        .call_metadata
                        .as_ref()
                        .map(|metadata| metadata.conventions.as_slice());
                    let access =
                        Self::callable_parameter_access(conventions, index);
                    if self.callable_parameter_borrowed(ty, access) {
                        format!("(*__jet_compute_arg_{index}).clone()")
                    } else {
                        format!("__jet_compute_arg_{index}")
                    }
                })
                .collect::<Vec<_>>();
            let call = format!(
                "{call}(__jet_compute_handle.raw(), {input}::from_flat(vec![{}]), {:?})",
                values.join(", "),
                format!("core.compute.{member}")
            );
            let result = self.compute_transform_result(member, ret, "__jet_compute_result");
            let closure = format!(
                "move |{}| {{ let __jet_compute_result = {call}; {result} }}",
                parameters.join(", ")
            );
            let carrier = if self.history_runtime_metadata_enabled() {
                format!("Box::new({closure})")
            } else {
                format!("std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new({closure}))))")
            };
            return format!("{{ let __jet_compute_handle = {handle}::new({plan}); {carrier} }}");
        }
        let values = args[1..args.len() - 1]
            .iter()
            .map(|arg| self.call_arg(arg, false))
            .collect::<Vec<_>>();
        let call = format!(
            "{call}(__jet_compute_handle.raw(), {input}::from_flat(vec![{}]), {:?})",
            values.join(", "),
            format!("core.compute.{member}")
        );
        let result = self.compute_transform_result(member, result_type, "__jet_compute_result");
        format!(
            "{{ let __jet_compute_handle = {handle}::new({plan}); \
               let __jet_compute_result = {call}; \
               {result} }}"
        )
    }

    fn core_call(
        &self,
        function: &MirFunction,
        call: MirCoreCallId,
        route: MirPreludeCallId,
        args: &[MirCallArg],
        type_args: &[MirType],
        _data_plan: Option<&MirDataPlan>,
        result_type: Option<&MirType>,
    ) -> String {
        let route_row = self.prelude_row(route);
        let row = self
            .program
            .core_calls
            .iter()
            .find(|row| row.id == call)
            .unwrap_or_else(|| panic!("MIR Core call {:?} has no canonical row", call));
        if row.module == "core.testing" && row.member == "histories" {
            return self.history_call_expression(
                function,
                args,
                type_args,
                &route_row.signature.borrow_mask,
            );
        }
        if row.module == "core.compute"
            && matches!(
                row.member.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            )
        {
            return self.compute_call(function, route, &row.member, args, type_args, result_type);
        }
        if row.module == "core.args" && matches!(row.member.as_str(), "decode" | "merge") {
            return self.cli_shape_core_call(function, row.member.as_str(), args, type_args);
        }
        // D-JSON3 / #2510: the typed form shares the one generated `Decode`
        // engine on computed text; the untyped lenient route keeps its own row.
        if row.module == "core.encoding.json" && row.member == "decode" && !type_args.is_empty() {
            return self.call_symbol_for_function(
                function,
                format!("{}jet_enc_json_decode", self.config.root_prefix),
                args,
                type_args,
                Some(&route_row.signature.borrow_mask),
            );
        }
        // D-SHAPE-ONE1=A: env decoding reads the canonical env names of T's
        // fields; the map rides as one extra slice argument after prefix/file/allow.
        if row.module == "core.sys" && row.member == "decode" {
            let [target] = type_args else {
                panic!("MIR core.sys.decode has no checked type argument");
            };
            let names = self.shape_name_map(target, ShapeProjectionKind::Env, "core.sys.decode");
            let values = args
                .iter()
                .map(|arg| self.call_arg_for_function(function, arg, true))
                .collect::<Vec<_>>();
            return format!(
                "{}jet_std_env_decode::<{}>({}, {names})",
                self.config.root_prefix,
                self.rust_type(target),
                values.join(", ")
            );
        }
        // D-SHAPE-ONE1=A: DB decoding applies the checked Db→Json names to
        // the row carrier, then uses the one typed DataTree decoder. The
        // carrier helper is generic so JIT/interpreter hosts can share it
        // without importing AOT's decoder trait.
        if row.module == "core.db" && row.member == "decode" {
            let [target] = type_args else {
                panic!("MIR core.db.decode has no checked type argument");
            };
            let names = self.shape_name_map(target, ShapeProjectionKind::Db, "core.db.decode");
            let values = args
                .iter()
                .map(|arg| self.call_arg_for_function(function, arg, true))
                .collect::<Vec<_>>();
            let root = &self.config.root_prefix;
            let target_ty = self.rust_type(target);
            return format!(
                "{{ let __jet_db_entries = {root}jet_std::jet_db_row_project({}, {names}, |value| match value {{ {root}jet_std::DBValue::Null => {root}jet_std::DataTree::Null, {root}jet_std::DBValue::Int(value) => {root}jet_std::DataTree::Int(*value), {root}jet_std::DBValue::Float(value) => {root}jet_std::DataTree::Float(*value), {root}jet_std::DBValue::Text(value) => {root}jet_std::DataTree::Text(value.clone()), {root}jet_std::DBValue::Bool(value) => {root}jet_std::DataTree::Bool(*value), {root}jet_std::DBValue::Blob(value) => {root}jet_std::DataTree::Bytes(value.clone()) }}); {root}jet_db_decode::<{target_ty}>(&__jet_db_entries) }}",
                values.join(", ")
            );
        }
        // D-WEBQUERY1: the checked callback carries JetErr, while the
        // current AOT query kernel still accepts text failures. Keep this
        // one explicit narrowing seam until that kernel carrier widens.
        if row.module == "core.web.query" && row.member == "live" {
            let mut values = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    self.call_arg_for_function(
                        function,
                        arg,
                        route_row
                            .signature
                            .borrow_mask
                            .get(index)
                            .copied()
                            .unwrap_or(false),
                    )
                })
                .collect::<Vec<_>>();
            let callback_arg = args
                .last()
                .unwrap_or_else(|| panic!("MIR core.web.query.live has no callback"));
            let callback_is_fn = !self.history_runtime_metadata_enabled()
                && matches!(
                    self.value_type(function, callback_arg.value).kind(),
                    MirTypeKind::Fn(_)
                );
            let callback = values
                .pop()
                .unwrap_or_else(|| panic!("MIR core.web.query.live has no callback"));
            let callback_invocation = if callback_is_fn {
                self.fn_value_call(callback, &[])
            } else {
                format!("({callback})()")
            };
            values.push(format!(
                "move || {callback_invocation}.map_err(|__error| {}jet_err_message(&__error))",
                self.config.root_prefix
            ));
            let generic = if type_args.is_empty() {
                String::new()
            } else {
                format!(
                    "::<{}>",
                    type_args
                        .iter()
                        .map(|ty| self.rust_type(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return format!(
                "{}{generic}({})",
                self.prelude_symbol(route),
                values.join(", ")
            );
        }
        let emitted = self.core_call_symbol_for_function(
            function,
            row,
            &route_row,
            self.prelude_symbol(route),
            args,
            type_args,
        );
        result_type
            .and_then(|ty| self.native_int_result(&emitted, ty))
            .unwrap_or(emitted)
    }

    fn native_int_result(&self, value: &str, ty: &MirType) -> Option<String> {
        match ty.kind() {
            MirTypeKind::Tagged {
                marker: MirTagMarker::Internal(MirInternalTag::AllocatorView),
                ..
            } => None,
            MirTypeKind::Int => Some(format!(
                "{}jet_std::jet_int_owned_from_native_result({value})",
                self.config.root_prefix,
            )),
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.native_int_result(value, base),
            MirTypeKind::Option(inner) => self
                .native_int_result("__jet_value", inner)
                .map(|inner| format!("({value}).map(|__jet_value| {inner})")),
            MirTypeKind::Result { ok, err } => {
                let ok = self.native_int_result("__jet_value", ok);
                let err = self.native_int_result("__jet_error", err);
                match (ok, err) {
                    (Some(ok), Some(err)) => Some(format!(
                        "({value}).map(|__jet_value| {ok}).map_err(|__jet_error| {err})"
                    )),
                    (Some(ok), None) => Some(format!("({value}).map(|__jet_value| {ok})")),
                    (None, Some(err)) => Some(format!("({value}).map_err(|__jet_error| {err})")),
                    (None, None) => None,
                }
            }
            MirTypeKind::List(inner) => self.native_int_result("__jet_value", inner).map(|inner| {
                format!("({value}).into_iter().map(|__jet_value| {inner}).collect::<Vec<_>>()")
            }),
            _ => None,
        }
    }

    /// D-CORE-PATH1: Core path-capable signatures admit `Path` alongside
    /// `String`, while their shared native kernels consume the String carrier.
    fn core_call_path_argument(
        &self,
        function: &MirFunction,
        arg: &MirCallArg,
        declared: Option<&crate::AST::Type>,
    ) -> bool {
        self.value_type(function, arg.value).nominal_name() == Some("Path")
            && declared.is_some_and(|ty| match ty {
                crate::AST::Type::Named(name) => name == "Path",
                crate::AST::Type::Union(members) => members.iter().any(
                    |member| matches!(member, crate::AST::Type::Named(name) if name == "Path"),
                ),
                _ => false,
            })
    }

    fn core_call_symbol_for_function(
        &self,
        function: &MirFunction,
        row: &MirCoreCall,
        route_row: &MirPreludeCall,
        symbol: String,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        if self.exact_prelude_adapter(&symbol).is_some() {
            return self.call_symbol_for_function(
                function,
                symbol,
                args,
                type_args,
                Some(&route_row.signature.borrow_mask),
            );
        }
        let generic = if type_args.is_empty() {
            String::new()
        } else {
            format!(
                "::<{}>",
                type_args
                    .iter()
                    .map(|ty| self.rust_type(ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let signature = crate::Sema::core_call_signature(&row.module, &row.member);
        let args = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                // `jet_ui_box` consumes its child vector. Its legacy registry
                // row marks the argument as a read window for the other
                // projections, but native Rust must preserve the kernel ABI.
                let borrowed = if row.module == "core.ui" && row.member == "box" {
                    false
                } else {
                    route_row
                        .signature
                        .borrow_mask
                        .get(index)
                        .copied()
                        .unwrap_or(false)
                };
                let declared = signature
                    .as_ref()
                    .and_then(|(params, _)| params.get(index))
                    .map(|(_, ty)| ty);
                let raw_int = match declared {
                    Some(crate::AST::Type::Int)
                        if matches!(
                            self.value_type(function, arg.value).kind(),
                            MirTypeKind::Int
                        ) =>
                    {
                        Some(format!(
                            "({}).to_raw()",
                            self.call_arg_for_function(function, arg, true),
                        ))
                    }
                    Some(crate::AST::Type::List(inner))
                        if matches!(inner.as_ref(), crate::AST::Type::Int) =>
                    {
                        Some(format!(
                            "({}).iter().map(|__jet_value| __jet_value.to_raw()).collect::<Vec<_>>()",
                            self.call_arg_for_function(function, arg, true),
                        ))
                    }
                    _ => None,
                };
                if let Some(value) = raw_int {
                    return if borrowed { format!("&({value})") } else { value };
                }
                let path_argument = self.core_call_path_argument(function, arg, declared);
                let value = if path_argument {
                    let mut arg = arg.clone();
                    // The checked union admits Path, but the native kernel
                    // receives the carrier String rather than a union value.
                    arg.widen_to_union = None;
                    self.call_arg_for_function(function, &arg, borrowed)
                } else {
                    self.call_arg_for_function(function, arg, borrowed)
                };
                if !path_argument {
                    return value;
                }
                let path = format!("({value}).inner.to_string_lossy().into_owned()");
                if borrowed {
                    format!("&{path}")
                } else {
                    path
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{symbol}{generic}({args})")
    }

    /// D-SHAPE-PROJECT1=A: `args.decode<T>()` and `T.merge(flags, settings)`
    /// build T's builder spec from the same `MirCliInput` rows the entry uses
    /// (`cli_spec_expr`), so `--help` and diagnostics are byte-identical.
    fn cli_shape_core_call(
        &self,
        function: &MirFunction,
        member: &str,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let root = &self.config.root_prefix;
        let [target] = type_args else {
            panic!("MIR core.args.{member} has no checked type argument");
        };
        let def = self
            .program
            .types
            .iter()
            .find(|def| Some(def.id) == target.identity)
            .unwrap_or_else(|| panic!("MIR core.args.{member} target type has no definition row"));
        let cli = def.cli.as_ref().unwrap_or_else(|| {
            panic!(
                "MIR core.args.{member} target `{}` carries no #CLI shape",
                def.name
            )
        });
        let program = format!(
            "&{root}jet_args_source_program_name(__jet_argv.first().map(String::as_str).unwrap_or(\"\"))"
        );
        let mut spec = self.cli_spec_expr(
            &cli.inputs,
            cli.description.as_deref(),
            cli.standard,
            cli.version.as_deref(),
            &program,
        );
        for command in &cli.commands {
            let command_spec = self.cli_spec_expr(
                &command.inputs,
                command.description.as_deref(),
                false,
                None,
                &program,
            );
            spec = format!(
                "{root}jet_args_subcommand({spec}, &{:?}.to_string(), &{:?}.to_string(), {command_spec})",
                command.name,
                command.description.clone().unwrap_or_default()
            );
        }
        let target_ty = self.rust_type(target);
        let names = self.shape_name_map(target, ShapeProjectionKind::Args, "core.args");
        let values = args
            .iter()
            .map(|arg| self.call_arg_for_function(function, arg, true))
            .collect::<Vec<_>>();
        match member {
            "decode" => format!(
                "{{ let __jet_argv = {root}jet_std_io_args(); let __jet_spec = {spec}; {root}jet_args_decode::<{target_ty}>(&__jet_spec, &__jet_argv, {names}) }}"
            ),
            _ => format!(
                "{{ let __jet_argv = {root}jet_std_io_args(); let __jet_spec = {spec}; {root}jet_config_merge::<{target_ty}>(&__jet_spec, &__jet_argv, {names}, {}) }}",
                values.join(", ")
            ),
        }
    }

    /// Render `&[(selected source name, JSON decode key)]` for every
    /// non-skipped field of `target` from its canonical `ShapeFieldNames`.
    /// Every projection uses its selected name; adapters never recase fields.
    fn shape_name_map(&self, target: &MirType, kind: ShapeProjectionKind, member: &str) -> String {
        let def = self
            .program
            .types
            .iter()
            .find(|def| Some(def.id) == target.identity)
            .unwrap_or_else(|| panic!("MIR {member} target type has no definition row"));
        let MirTypeDefKind::Struct { fields, .. } = &def.kind else {
            panic!("MIR {member} target `{}` is not a struct", def.name);
        };
        let pairs = fields
            .iter()
            .filter(|field| !field.skip && !field.computed)
            .map(|field| {
                let source = field
                    .shape_names
                    .name_for(kind)
                    .expect("checked MIR field is missing its shape name");
                let key = field
                    .shape_names
                    .name_for(ShapeProjectionKind::Json)
                    .expect("checked MIR field is missing its JSON shape name");
                format!("({source:?}, {key:?})")
            })
            .collect::<Vec<_>>();
        format!("&[{}]", pairs.join(", "))
    }

    fn index(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        base: MirValueId,
        index: MirValueId,
        kind: jet_foundation::MIR::MirIndexKind,
        access: MirAccess,
        location: MirPanicLoc,
        context: &MirPanicContext,
    ) -> String {
        let base = match access {
            MirAccess::Read | MirAccess::Write => {
                self.borrowed_value_reference(function, base, access)
            }
            MirAccess::Move => panic!("MIR index move has no checked Prelude route"),
        };
        let index = if matches!(kind, jet_foundation::MIR::MirIndexKind::Map) {
            format!("&({})", self.value_read(index))
        } else {
            self.index_operand(function, index, location)
        };
        self.index_expression(call, base, index, location, Some(context))
    }

    fn index_operand(
        &self,
        function: &MirFunction,
        index: MirValueId,
        location: MirPanicLoc,
    ) -> String {
        let mut ty = self.value_type(function, index);
        loop {
            match ty.kind() {
                MirTypeKind::InlineRange { base, .. }
                | MirTypeKind::Tagged { inner: base, .. }
                | MirTypeKind::Quantity { base, .. } => ty = base,
                MirTypeKind::Int => {
                    let value = self.borrowed_value_reference(function, index, MirAccess::Read);
                    let file = self.source_file_path(location.file);
                    let root = &self.config.root_prefix;
                    return format!(
                        "{root}jet_std::jet_int_owned_to_i64({value}).unwrap_or_else(|_| {root}jet_arithmetic_stop({file:?}, {}u32, \"index exceeds host range\"))",
                        location.line
                    );
                }
                _ => return self.value_read(index),
            }
        }
    }

    fn index_expression(
        &self,
        call: MirPreludeCallId,
        base: String,
        index: String,
        location: MirPanicLoc,
        context: Option<&MirPanicContext>,
    ) -> String {
        let args = self.index_arguments(call, base, index, location, context);
        self.prelude_call_args_exact(call, &args)
    }

    fn index_arguments(
        &self,
        call: MirPreludeCallId,
        base: String,
        index: String,
        location: MirPanicLoc,
        context: Option<&MirPanicContext>,
    ) -> Vec<String> {
        let route = self.prelude_row(call);
        let file = format!("{:?}", self.source_file_path(location.file));
        let line = format!("{}u32", location.line);
        let mut args = vec![base, index, file, line];
        let context_values = context
            .map(|context| {
                vec![
                    format!("{:?}", context.function),
                    format!("{:?}", context.source_line),
                    format!("{}u32", location.column),
                    format!("{}u32", context.caret),
                ]
            })
            .unwrap_or_default();
        let additional = route
            .signature
            .arity
            .checked_sub(args.len())
            .unwrap_or_else(|| {
                panic!(
                    "MIR index route {:?} has fewer arguments than its base/index location prefix",
                    call
                )
            });
        if additional > context_values.len() {
            panic!(
                "MIR index route {:?} requires {} context arguments but MIR supplied {}",
                call,
                additional,
                context_values.len()
            );
        }
        args.extend(context_values.into_iter().take(additional));
        args
    }

    fn slice(
        &self,
        call: MirPreludeCallId,
        base: MirValueId,
        start: MirValueId,
        end: MirValueId,
        range: Option<MirValueId>,
        location: jet_foundation::MIR::MirPanicLoc,
    ) -> String {
        let file = format!("{:?}", self.source_file_path(location.file));
        let line = format!("{}u32", location.line);
        let args = match range {
            Some(range) => vec![self.value_read(base), self.value_read(range), file, line],
            None => vec![
                self.value_read(base),
                self.value_read(start),
                self.value_read(end),
                file,
                line,
            ],
        };
        self.prelude_call_args_exact(call, &args)
    }

    fn prelude_call_args_exact(&self, call: MirPreludeCallId, args: &[String]) -> String {
        let arity = self.prelude_row(call).signature.arity;
        if arity != args.len() {
            panic!(
                "MIR Prelude call {:?} expects {} arguments, adapter supplied {}",
                call,
                arity,
                args.len()
            );
        }
        self.prelude_call_args(call, args)
    }

    fn source_file_path(&self, id: jet_foundation::MIR::MirSourceFileId) -> &str {
        self.program
            .source_files
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.path.as_str())
            .unwrap_or_else(|| panic!("MIR source file ID {:?} has no row", id))
    }

    fn validate_pattern_capture_index(
        &self,
        function: &MirFunction,
        matched: MirValueId,
        index: usize,
    ) {
        let instruction = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(matched))
            .unwrap_or_else(|| panic!("MIR pattern match value {:?} has no producer", matched));
        let valid = match &instruction.operation {
            MirOperation::Semantic(MirSemanticOp::TextPatternMatch { parts, .. }) => parts
                .iter()
                .filter_map(|part| match part {
                    MirTextPatternPart::Literal(_) => None,
                    MirTextPatternPart::Hole { kind, .. } => {
                        match kind {
                            MirTextHoleKind::Text
                            | MirTextHoleKind::Int
                            | MirTextHoleKind::Float
                            | MirTextHoleKind::Bool
                            | MirTextHoleKind::InlineRange { .. } => {}
                        }
                        Some(())
                    }
                })
                .nth(index),
            MirOperation::Semantic(MirSemanticOp::BinaryPatternMatch { parts, .. }) => parts
                .iter()
                .filter_map(|part| match part {
                    MirBinaryPatternPart::Literal(_) => None,
                    MirBinaryPatternPart::Bits { width, little, .. } => {
                        let _ = (width, little);
                        Some(())
                    }
                    MirBinaryPatternPart::Rest { .. } => Some(()),
                })
                .nth(index),
            _ => panic!("MIR pattern capture source is not a canonical pattern match"),
        };
        if valid.is_none() {
            panic!("MIR pattern capture index {index} has no descriptor");
        }
    }

    fn pattern_capture(
        &self,
        function: &MirFunction,
        matched: MirValueId,
        index: usize,
        result: Option<MirValueId>,
    ) -> String {
        result.expect("MIR pattern capture has no result value");
        self.validate_pattern_capture_index(function, matched, index);
        format!(
            "({}).as_ref().unwrap_or_else(|_| unreachable!(\"MIR pattern capture unavailable\")).{index}.clone()",
            self.value_slot_reference(matched, false),
        )
    }

    fn pattern_capture_value(&self, ty: &MirType, capture: &str) -> String {
        match ty.kind() {
            MirTypeKind::String => format!(
                "match {capture} {{ JetPatternCapture::Text(value) => value, _ => unreachable!(\"MIR text capture type mismatch\") }}"
            ),
            MirTypeKind::Char => format!(
                "match {capture} {{ JetPatternCapture::Text(value) => {{ let mut chars = value.chars(); match (chars.next(), chars.next()) {{ (Some(value), None) => value, _ => unreachable!(\"MIR char capture contains more than one scalar\") }} }}, _ => unreachable!(\"MIR char capture type mismatch\") }}"
            ),
            MirTypeKind::Int
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. }
            | MirTypeKind::Measure(_) => format!(
                "match {capture} {{ JetPatternCapture::Int(value) => value as {}, _ => unreachable!(\"MIR integer capture type mismatch\") }}",
                self.rust_type(ty)
            ),
            MirTypeKind::Float | MirTypeKind::Float32 => format!(
                "match {capture} {{ JetPatternCapture::Float(value) => value as {}, _ => unreachable!(\"MIR float capture type mismatch\") }}",
                self.rust_type(ty)
            ),
            MirTypeKind::Bool => format!(
                "match {capture} {{ JetPatternCapture::Bool(value) => value, _ => unreachable!(\"MIR bool capture type mismatch\") }}"
            ),
            MirTypeKind::List(inner) => match inner.kind() {
                MirTypeKind::IntN {
                    signed: false,
                    bits: 8,
                } => format!(
                    "match {capture} {{ JetPatternCapture::Bytes(value) => value, _ => unreachable!(\"MIR byte capture type mismatch\") }}"
                ),
                MirTypeKind::Int
                | MirTypeKind::Float
                | MirTypeKind::Bool
                | MirTypeKind::String
                | MirTypeKind::Char
                | MirTypeKind::List(_)
                | MirTypeKind::Map { .. }
                | MirTypeKind::Shared(_)
                | MirTypeKind::Option(_)
                | MirTypeKind::Result { .. }
                | MirTypeKind::Fn(_)
                | MirTypeKind::SendFn { .. }
                | MirTypeKind::Apply { .. }
                | MirTypeKind::TraitObject(_)
                | MirTypeKind::Tuple(_)
                | MirTypeKind::FixedList { .. }
                | MirTypeKind::IntN { .. }
                | MirTypeKind::InlineRange { .. }
                | MirTypeKind::Tagged { .. }
                | MirTypeKind::Quantity { .. }
                | MirTypeKind::Float32
                | MirTypeKind::Union(_)
                | MirTypeKind::Measure(_) => {
                    panic!("MIR list pattern capture is not a binary rest carrier")
                }
            },
            MirTypeKind::Apply { name, .. } if name.name == jet_foundation::Syntax::TYPE_BYTES => format!(
                "match {capture} {{ JetPatternCapture::Bytes(value) => <{}JetByteBuffer as From<Vec<u8>>>::from(value), _ => unreachable!(\"MIR byte capture type mismatch\") }}",
                self.config.root_prefix,
            ),
            MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::Tagged { .. }
            | MirTypeKind::Union(_)
            | MirTypeKind::Quantity { .. } => {
                panic!("MIR pattern capture result type is not sema-admitted")
            }
        }
    }

    fn pattern_match_result(
        &self,
        function: &MirFunction,
        result: Option<MirValueId>,
        scan: &str,
    ) -> String {
        let result = result.expect("MIR pattern match has no result value");
        let payload = self
            .value_type(function, result)
            .option_inner()
            .expect("MIR pattern match does not return Option");
        let MirTypeKind::Tuple(fields) = payload.kind() else {
            panic!("MIR pattern match does not carry typed captures");
        };
        let tuple = if fields.is_empty() {
            "|_| ()".to_string()
        } else {
            let captures = fields
                .iter()
                .map(|(_, ty)| {
                    self.pattern_capture_value(
                        ty,
                        "__jet_captures.next().expect(\"MIR pattern capture is missing\")",
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("|__jet_captures| {{ let mut __jet_captures = __jet_captures.into_iter(); ({captures},) }}")
        };
        format!(
            "({scan}).map({tuple}).ok_or({}JetAbsent)",
            self.config.root_prefix
        )
    }

    fn conversion(
        &self,
        function: &MirFunction,
        value: MirValueId,
        parameters: &[MirValueId],
        target: &MirType,
        conversion: &MirConversion,
    ) -> String {
        match conversion {
            MirConversion::Transparent => {
                if !parameters.is_empty() {
                    panic!("transparent MIR conversion has parameters");
                }
                self.transparent_conversion(function, value, target)
            }
            MirConversion::NumericCast => {
                let source = self.value_type(function, value);
                let value = self.value_read(value);
                if matches!(source.kind(), MirTypeKind::Int) {
                    if matches!(target.kind(), MirTypeKind::Int) {
                        return value;
                    }
                    if target.is_float() {
                        return format!(
                            "{}jet_std::jet_int_owned_to_f64(&({value})) as {}",
                            self.config.root_prefix,
                            self.rust_type(target),
                        );
                    }
                }
                if matches!(target.kind(), MirTypeKind::Int) {
                    let Some((signed, _)) = source.fixed_int() else {
                        panic!("MIR numeric cast to exact Int requires a fixed integer");
                    };
                    let scalar = if signed { "i64" } else { "u64" };
                    format!(
                        "{}jet_std::jet_int_owned_from_{scalar}(({value}) as {scalar})",
                        self.config.root_prefix,
                    )
                } else {
                    format!("({value}) as {}", self.rust_type(target))
                }
            }
            MirConversion::SendFn => {
                if !parameters.is_empty() {
                    panic!("SendFn MIR conversion has parameters");
                }
                if !matches!(target.kind(), MirTypeKind::SendFn { .. }) {
                    panic!("SendFn MIR conversion target is not a SendFn carrier");
                }
                if !matches!(
                    self.value_type(function, value).kind(),
                    MirTypeKind::SendFn { .. }
                ) {
                    panic!("SendFn MIR conversion source is not a SendFn carrier");
                }
                self.value_move(value)
            }
            MirConversion::Prelude {
                call,
                location,
                fallibility,
            } => {
                if matches!(
                    self.prelude_row(*call).member.as_str(),
                    "checked_widen"
                        | "int_try_from"
                        | "fixed_try_from"
                        | "inline_range"
                        | "int_checked_fixed"
                        | "float_to_int"
                        | "float_narrow"
                ) {
                    self.numeric_conversion(
                        function,
                        value,
                        parameters,
                        target,
                        *call,
                        location,
                        fallibility,
                    )
                } else {
                    let mut args = Vec::with_capacity(parameters.len() + 3);
                    args.push(self.value_read(value));
                    args.extend(
                        parameters
                            .iter()
                            .map(|parameter| self.value_read(*parameter)),
                    );
                    let extras = vec![
                        format!("{:?}", self.source_file_path(location.file)),
                        format!("{}u32", location.line),
                    ];
                    self.append_prelude_context(*call, &mut args, &extras);
                    self.prelude_call_args(*call, &args)
                }
            }
        }
    }

    fn numeric_conversion(
        &self,
        function: &MirFunction,
        value: MirValueId,
        parameters: &[MirValueId],
        target: &MirType,
        call: MirPreludeCallId,
        location: &MirPanicLoc,
        fallibility: &MirCallFallibility,
    ) -> String {
        let source = self.value_type(function, value);
        let member = self.prelude_row(call).member.as_str();
        let expected = match member {
            "checked_widen" if source.fixed_int().is_some() => 2,
            "checked_widen" => 1,
            "int_try_from" => 1,
            "fixed_try_from" => 2,
            "inline_range" => 2,
            "int_checked_fixed" => 1,
            "float_to_int" => 1,
            "float_narrow" => 0,
            _ => panic!("MIR Prelude call {:?} is not a numeric conversion", call),
        };
        if parameters.len() != expected {
            panic!(
                "MIR numeric conversion {member:?} has {} parameters, expected {expected}",
                parameters.len()
            );
        }
        let symbol = self.prelude_symbol(call);
        let adapter = self
            .exact_prelude_adapter(&symbol)
            .filter(|_| matches!(source.kind(), MirTypeKind::Int));
        let input = if adapter.is_some() {
            self.value_slot_reference(value, false)
        } else {
            self.value_read(value)
        };
        let mut args = match member {
            "checked_widen" => {
                let mut args = Vec::with_capacity(5);
                if source.fixed_int().is_some() {
                    args.push(format!("({input}) as u64"));
                    args.push(self.numeric_conversion_parameter(parameters, 0, member));
                    args.push(self.numeric_conversion_parameter(parameters, 1, member));
                } else {
                    args.push(input);
                    args.push(self.numeric_conversion_parameter(parameters, 0, member));
                }
                args
            }
            "int_try_from" => vec![
                input,
                self.numeric_conversion_parameter(parameters, 0, member),
            ],
            "fixed_try_from" => vec![
                format!("({input}) as u64"),
                self.numeric_conversion_parameter(parameters, 0, member),
                self.numeric_conversion_parameter(parameters, 1, member),
            ],
            "inline_range" => vec![
                if adapter.is_some() {
                    input
                } else {
                    format!("({input}) as i64")
                },
                self.numeric_conversion_parameter(parameters, 0, member),
                self.numeric_conversion_parameter(parameters, 1, member),
            ],
            "int_checked_fixed" => vec![
                input,
                self.numeric_conversion_parameter(parameters, 0, member),
            ],
            "float_to_int" => vec![
                format!("({input}) as f64"),
                self.numeric_conversion_parameter(parameters, 0, member),
            ],
            "float_narrow" => vec![format!("({input}) as f64")],
            _ => panic!("MIR Prelude call {:?} is not a numeric conversion", call),
        };
        if matches!(member, "checked_widen" | "int_checked_fixed") {
            let extras = vec![
                format!("{:?}", self.source_file_path(location.file)),
                format!("{}u32", location.line),
            ];
            self.append_prelude_context(call, &mut args, &extras);
        }
        self.validate_prelude_count(self.prelude_row(call), args.len());
        let symbol = adapter
            .map(|(symbol, _)| format!("{}{symbol}", self.config.root_prefix))
            .unwrap_or(symbol);
        let emitted = format!("{symbol}({})", args.join(", "));
        self.numeric_conversion_result(&emitted, target, fallibility)
    }

    fn numeric_conversion_parameter(
        &self,
        parameters: &[MirValueId],
        index: usize,
        member: &str,
    ) -> String {
        parameters
            .get(index)
            .map(|parameter| self.value_read(*parameter))
            .unwrap_or_else(|| {
                panic!("MIR numeric conversion {member:?} is missing parameter {index}")
            })
    }

    fn numeric_conversion_result(
        &self,
        emitted: &str,
        target: &MirType,
        fallibility: &MirCallFallibility,
    ) -> String {
        match fallibility {
            MirCallFallibility::Infallible => self.numeric_conversion_value(emitted, target),
            MirCallFallibility::Failure(MirFailureCarrier::Result { success, .. }) => {
                let Some((ok, _error)) = target.result_parts() else {
                    panic!("MIR numeric conversion Result carrier has a non-Result target");
                };
                if !ok.same_checked_type(success) {
                    panic!("MIR numeric conversion Result carrier disagrees with its target");
                }
                let success = self.numeric_conversion_value("__jet_conversion_value", ok);
                format!(
                    "match {emitted} {{ \
                        Ok(__jet_conversion_value) => Ok({success}), \
                        Err(__jet_conversion_error) => Err(__jet_conversion_error.into()) \
                    }}"
                )
            }
            MirCallFallibility::Failure(carrier) => {
                panic!(
                    "MIR numeric conversion has unsupported failure carrier {:?}",
                    carrier
                )
            }
        }
    }

    fn numeric_conversion_value(&self, raw: &str, target: &MirType) -> String {
        let base = self.distinct_base(target);
        let base_ty = base.unwrap_or(target);
        let cast = format!("({raw}) as {}", self.rust_type(base_ty));
        if base.is_some() {
            format!("{}({cast})", self.rust_type(target))
        } else {
            cast
        }
    }

    fn transparent_conversion(
        &self,
        function: &MirFunction,
        value: MirValueId,
        target: &MirType,
    ) -> String {
        let source = self.value_type(function, value);
        let value = self.value_read(value);
        let target_distinct = self.is_distinct_type(target);
        let source_distinct = self.is_distinct_type(source);
        match (source_distinct, target_distinct) {
            (false, true) => format!("{}({value})", self.rust_type(target)),
            (true, false) => format!("({value}).0"),
            (true, true) | (false, false) => {
                panic!("MIR transparent conversion is not a distinct representation pair")
            }
        }
    }

    fn is_distinct_type(&self, ty: &MirType) -> bool {
        let Some(id) = ty.identity else {
            return false;
        };
        self.program
            .types
            .iter()
            .find(|def| def.id == id)
            .is_some_and(|def| matches!(&def.kind, MirTypeDefKind::Distinct { .. }))
    }
    fn distinct_base(&self, ty: &MirType) -> Option<&MirType> {
        let id = ty.nominal_id()?;
        self.program
            .types
            .iter()
            .find(|def| def.id == id)
            .and_then(|def| match &def.kind {
                MirTypeDefKind::Distinct { base, .. } => Some(base),
                _ => None,
            })
    }

    fn loop_range_init(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        start: MirValueId,
        end: MirValueId,
        step: Option<MirValueId>,
        exclusive: bool,
        location: MirPanicLoc,
    ) -> String {
        let (step, has_step) = step
            .map(|value| {
                (
                    self.index_operand(function, value, location),
                    "true".to_string(),
                )
            })
            .unwrap_or_else(|| ("0".to_string(), "false".to_string()));
        self.prelude_call_args_exact(
            call,
            &[
                self.index_operand(function, start, location),
                self.index_operand(function, end, location),
                step,
                has_step,
                exclusive.to_string(),
            ],
        )
    }
    fn loop_iter_init(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        collection: MirValueId,
        step: Option<MirValueId>,
        by_value: bool,
        source_kind: &MirLoopSourceKind,
        location: MirPanicLoc,
    ) -> String {
        let collection = if by_value {
            self.value_move(collection)
        } else {
            self.value_borrow_mut(collection)
        };
        let (step, has_step) = step
            .map(|value| {
                (
                    self.index_operand(function, value, location),
                    "true".to_string(),
                )
            })
            .unwrap_or_else(|| ("0".to_string(), "false".to_string()));
        self.prelude_call_args_exact(
            call,
            &[
                collection,
                step,
                has_step,
                by_value.to_string(),
                self.loop_source_kind(source_kind),
            ],
        )
    }

    fn loop_source_kind(&self, source_kind: &MirLoopSourceKind) -> String {
        let wire = source_kind.wire();
        format!(
            "JetLoopSourceKind::from_wire({wire:?}).unwrap_or_else(|| jet_panic(\"<core.prelude>\", 0, \"invalid canonical loop source wire\"))"
        )
    }

    fn enum_variant_payload(&self, owner: MirTypeId, variant: &str) -> &MirVariantPayload {
        let def = self
            .program
            .types
            .iter()
            .find(|def| def.id == owner)
            .unwrap_or_else(|| panic!("MIR enum owner type {:?} has no row", owner));
        match &def.kind {
            MirTypeDefKind::Enum { variants, .. } => {
                &variants
                    .iter()
                    .find(|candidate| candidate.name == variant)
                    .unwrap_or_else(|| panic!("MIR enum variant {variant:?} has no row"))
                    .payload
            }
            MirTypeDefKind::Struct { .. }
            | MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => {
                panic!("MIR enum owner type {:?} is not an enum", owner)
            }
        }
    }

    fn prelude_cursor_call(
        &self,
        call: MirPreludeCallId,
        cursor: MirValueId,
        access: MirAccess,
    ) -> String {
        let route = self.prelude_row(call);
        if route.signature.arity != 1 || route.signature.borrow_mask.as_slice() != [true] {
            panic!(
                "MIR cursor route {:?} must have one borrowed cursor argument",
                call
            );
        }
        let cursor = match access {
            MirAccess::Read => self.value_slot_reference(cursor, false),
            MirAccess::Write => self.value_borrow_mut(cursor),
            MirAccess::Move => self.value_move(cursor),
        };
        self.prelude_call_args_exact(call, &[cursor])
    }

    fn prelude_handle_method(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirValueId],
        frame_schedule: Option<&jet_foundation::ResourceSchedule::JetFrameSchedule>,
        frame_schedule_derivation: Option<&jet_foundation::Facts::DerivationRef>,
        location: Option<&MirPanicLoc>,
    ) -> String {
        let route = self.prelude_row(call);
        if route.member == "callback_event_stop" {
            if !args.is_empty() {
                panic!("callback event stop route has no explicit arguments");
            }
            return self.prelude_call_args_exact(call, &[]);
        }
        let mut values = vec![(Some(receiver), None)];
        values.extend(args.iter().map(|value| (Some(*value), None)));
        if let Some(metadata) = route.db_metadata.as_ref() {
            values.push((None, Some(format!("{:?}.to_string()", metadata.to_wire()))));
        }
        if route.member == "game.scene_on_frame" {
            let schedule = frame_schedule.map_or_else(
                || "None".to_string(),
                |schedule| format!("Some({:?})", schedule.canonical_json()),
            );
            let derivation = frame_schedule_derivation.map_or_else(
                || "None".to_string(),
                |reference| format!("Some({:?})", reference.id),
            );
            values.push((None, Some(schedule)));
            values.push((None, Some(derivation)));
        }
        if values.len() != route.signature.arity
            || values.len() != route.signature.borrow_mask.len()
        {
            panic!(
                "MIR HandleMethod route {:?} has inconsistent canonical arity metadata",
                call
            );
        }
        let values = values
            .into_iter()
            .enumerate()
            .map(|(index, (value_id, value))| {
                if let Some(value_id) = value_id {
                    if let Some(callback) = self.host_borrow_callback_value(function, value_id) {
                        return callback;
                    }
                    if (route.module == "core.time"
                        || (route.module == "core.handle"
                            && matches!(route.member.as_str(), "clock.tick" | "clock.advance")))
                        && index > 0
                        && !route.signature.borrow_mask[index]
                        && matches!(self.value_type(function, value_id).kind(), MirTypeKind::Int)
                    {
                        let value = self.value_move(value_id);
                        return self.native_int_argument(value, location);
                    }
                    if route.signature.borrow_mask[index] {
                        let access = match self.value_definition(function, value_id) {
                            Some(MirOperation::AddressOf { access, .. }) => *access,
                            _ => MirAccess::Read,
                        };
                        self.borrowed_value_reference(function, value_id, access)
                    } else {
                        self.value_move(value_id)
                    }
                } else if route.signature.borrow_mask[index] {
                    format!(
                        "&({})",
                        value.unwrap_or_else(|| panic!(
                            "MIR borrowed HandleMethod metadata is missing"
                        ))
                    )
                } else {
                    value.unwrap_or_else(|| panic!("MIR HandleMethod metadata is missing"))
                }
            })
            .collect::<Vec<_>>();
        self.prelude_call_args_exact(call, &values)
    }

    fn native_int_argument(&self, value: String, location: Option<&MirPanicLoc>) -> String {
        let (source_file, source_line) = location
            .map(|location| {
                (
                    self.source_file_path(location.file).to_string(),
                    location.line,
                )
            })
            .unwrap_or_else(|| ("<mir>".to_string(), 0));
        format!(
            "{}jet_std::jet_int_owned_to_i64(&({value})).unwrap_or_else(|_| {}jet_arithmetic_stop({source_file:?}, {source_line}u32, \"native Int argument exceeds host range\"))",
            self.config.root_prefix,
            self.config.root_prefix,
        )
    }

    fn native_int_payload_value(
        &self,
        function: &MirFunction,
        value_id: MirValueId,
        expected: &MirType,
        value: String,
        location: Option<&MirPanicLoc>,
    ) -> String {
        if !matches!(self.value_type(function, value_id).kind(), MirTypeKind::Int) {
            return value;
        }
        match expected.kind() {
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => {
                return self.native_int_payload_value(function, value_id, base, value, location);
            }
            MirTypeKind::IntN { .. } => {}
            _ => return value,
        }
        let source_file = location
            .map(|location| self.source_file_path(location.file).to_string())
            .unwrap_or_else(|| "<mir>".to_string());
        let source_line = location.map_or(0, |location| location.line);
        let raw = format!(
            "{}jet_std::jet_int_owned_to_i64(&({value})).unwrap_or_else(|_| {}jet_arithmetic_stop({source_file:?}, {source_line}u32, \"native Int payload exceeds host range\"))",
            self.config.root_prefix,
            self.config.root_prefix,
        );
        if matches!(
            expected.kind(),
            MirTypeKind::IntN {
                signed: true,
                bits: 64
            }
        ) {
            raw
        } else {
            format!(
                "({raw}).try_into().unwrap_or_else(|_| {}jet_arithmetic_stop({source_file:?}, {source_line}u32, \"native Int payload exceeds target range\"))",
                self.config.root_prefix,
            )
        }
    }

    fn enum_arg_value(
        &self,
        function: &MirFunction,
        arg: &jet_foundation::MIR::MirEnumArg,
        expected: &MirType,
        location: Option<&MirPanicLoc>,
    ) -> String {
        let value = self.value_move(arg.value);
        let value = self.native_int_payload_value(function, arg.value, expected, value, location);
        if arg.boxed {
            format!("Box::new({value})")
        } else {
            value
        }
    }
    fn variant_path(&self, owner: Option<MirTypeId>, variant: &str, qualified: bool) -> String {
        let native = owner.is_some_and(|owner| {
            let key = &self.type_def(owner).key;
            crate::Codegen::core_rust_type_name(key).is_some()
                || crate::Codegen::root_prelude_rust_type_name(key).is_some()
                || crate::Codegen::compute_handle_rust_type(key).is_some()
        });
        let variant = if native {
            variant.to_string()
        } else {
            mangle(variant)
        };
        match (owner, qualified) {
            (Some(owner), _) => format!("{}::{variant}", self.type_name(owner)),
            (None, false) => variant,
            (None, true) => panic!("MIR qualified enum variant has no owner type"),
        }
    }
    fn enum_value(
        &self,
        function: &MirFunction,
        owner: MirTypeId,
        variant: &str,
        args: &[jet_foundation::MIR::MirEnumArg],
        location: Option<&MirPanicLoc>,
    ) -> String {
        let head = self.variant_path(Some(owner), variant, false);
        match self.enum_variant_payload(owner, variant) {
            MirVariantPayload::Unit => {
                if !args.is_empty() {
                    panic!("MIR unit variant construction has payload arguments");
                }
                head
            }
            MirVariantPayload::Single(payload) => {
                if args.len() != 1 {
                    panic!("MIR single variant construction payload arity mismatch");
                }
                let data_tree_int = variant == "Int"
                    && crate::Codegen::core_rust_type_name(&self.type_def(owner).key)
                        == Some("DataTree");
                let mut value = if data_tree_int {
                    // `DataTree::Int` is the one native enum scalar whose
                    // payload is the host `i64` slot. Exact Jet `Int` values
                    // remain managed nodes everywhere else.
                    let raw = self.value_move(args[0].value);
                    self.native_int_payload_value(
                        function,
                        args[0].value,
                        &MirType::from_kind(MirTypeKind::IntN {
                            signed: true,
                            bits: 64,
                        }),
                        raw,
                        location,
                    )
                } else {
                    self.enum_arg_value(function, &args[0], payload, location)
                };
                if variant == "Object"
                    && crate::Codegen::core_rust_type_name(&self.type_def(owner).key)
                        == Some("DataTree")
                    && matches!(
                        self.value_type(function, args[0].value).kind(),
                        MirTypeKind::Map { .. }
                    )
                {
                    value = format!("{}jet_map_into_entries({value})", self.config.root_prefix);
                }
                format!("{head}({value})")
            }
            MirVariantPayload::Named(fields) => {
                if args.len() != fields.len() {
                    panic!("MIR named variant construction payload arity mismatch");
                }
                let rendered = fields
                    .iter()
                    .zip(args)
                    .map(|(field, arg)| {
                        if arg.field != Some(field.id) {
                            panic!("MIR named variant construction field order mismatch");
                        }
                        format!(
                            "{}: {}",
                            self.field_name(field.id),
                            self.enum_arg_value(function, arg, &field.ty, location),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{head} {{ {rendered} }}")
            }
        }
    }

    fn enum_is(&self, subject: MirValueId, owner: MirTypeId, variant: &str) -> String {
        let head = self.variant_path(Some(owner), variant, false);
        match self.enum_variant_payload(owner, variant) {
            MirVariantPayload::Unit => format!(
                "matches!({}, {head})",
                self.value_slot_reference(subject, false)
            ),
            MirVariantPayload::Single(_) => format!(
                "matches!({}, {head}(_))",
                self.value_slot_reference(subject, false)
            ),
            MirVariantPayload::Named(_) => format!(
                "matches!({}, {head} {{ .. }})",
                self.value_slot_reference(subject, false)
            ),
        }
    }

    fn enum_payload(
        &self,
        function: &MirFunction,
        result: Option<MirValueId>,
        subject: MirValueId,
        owner: MirTypeId,
        variant: &str,
        index: usize,
    ) -> String {
        let value = self.value_slot_reference(subject, false);
        let head = self.variant_path(Some(owner), variant, false);
        match self.enum_variant_payload(owner, variant) {
            MirVariantPayload::Unit => panic!("MIR unit variant has no payload"),
            MirVariantPayload::Single(_) => {
                if index != 0 {
                    panic!("MIR single variant payload index out of range")
                }
                let payload = if variant == "Object"
                    && crate::Codegen::core_rust_type_name(&self.type_def(owner).key)
                        == Some("DataTree")
                    && matches!(
                        self.value_type(function, result.expect("MIR enum payload result"))
                            .kind(),
                        MirTypeKind::Map { .. }
                    ) {
                    format!(
                        "{}jet_data_entries_to_map(payload.clone())",
                        self.config.root_prefix
                    )
                } else if self
                    .type_def(owner)
                    .boxed_edges
                    .iter()
                    .any(|edge| edge == variant)
                {
                    "payload.as_ref().clone()".to_string()
                } else {
                    "payload.clone()".to_string()
                };
                format!("match {value} {{ {head}(payload) => {payload}, _ => unreachable!(\"MIR enum payload variant mismatch\") }}")
            }
            MirVariantPayload::Named(fields) => {
                let field = fields
                    .get(index)
                    .unwrap_or_else(|| panic!("MIR named variant payload index out of range"));
                let name = self.field_name(field.id);
                let edge = format!("{variant}.{}", field.name);
                let payload = if self.type_def(owner).boxed_edges.contains(&edge) {
                    "payload.as_ref().clone()"
                } else {
                    "payload.clone()"
                };
                format!("match {value} {{ {head} {{ {name}: payload, .. }} => {payload}, _ => unreachable!(\"MIR enum payload variant mismatch\") }}")
            }
        }
    }

    fn text_hole_kind(&self, kind: MirTextHoleKind) -> String {
        match kind {
            MirTextHoleKind::Text => "JetTextHoleKind::Text".to_string(),
            MirTextHoleKind::Int => "JetTextHoleKind::Int".to_string(),
            MirTextHoleKind::Float => "JetTextHoleKind::Float".to_string(),
            MirTextHoleKind::Bool => "JetTextHoleKind::Bool".to_string(),
            MirTextHoleKind::InlineRange { lo, hi } => {
                format!("JetTextHoleKind::InlineRange {{ lo: {lo}, hi: {hi} }}")
            }
        }
    }

    fn text_pattern(&self, parts: &[MirTextPatternPart]) -> String {
        let rendered = parts
            .iter()
            .map(|part| match part {
                MirTextPatternPart::Literal(value) => {
                    format!("JetTextMatchPart::Literal({value:?})")
                }
                MirTextPatternPart::Hole { kind, .. } => format!(
                    "JetTextMatchPart::Hole {{ kind: {} }}",
                    self.text_hole_kind(*kind)
                ),
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("&[{rendered}]")
    }

    fn binary_pattern(&self, parts: &[MirBinaryPatternPart]) -> String {
        let rendered = parts
            .iter()
            .map(|part| match part {
                MirBinaryPatternPart::Literal(value) => {
                    let bytes = value
                        .iter()
                        .map(|byte| format!("{byte}u8"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("JetBinMatchPart::Lit(&[{bytes}])")
                }
                MirBinaryPatternPart::Bits { width, little, .. } => format!(
                    "JetBinMatchPart::Bits {{ width: {}usize, little: {} }}",
                    width, little
                ),
                MirBinaryPatternPart::Rest { .. } => "JetBinMatchPart::Rest".to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("&[{rendered}]")
    }

    fn fn_value_call(&self, callee: String, args: &[String]) -> String {
        format!(
            "{{ let __jet_fn_slot = {callee}; \
               let mut __jet_fn = __jet_fn_slot.borrow_mut().take().expect(\"MIR function value was empty\"); \
               let __jet_fn_result = (*__jet_fn)({}); \
               __jet_fn_slot.borrow_mut().replace(__jet_fn); \
               __jet_fn_result }}",
            args.join(", ")
        )
    }

    fn indirect_call(
        &self,
        function: &MirFunction,
        callee: MirValueId,
        args: &[MirCallArg],
        type_args: &[MirType],
    ) -> String {
        let borrow_mask = self.callable_borrow_mask(self.value_type(function, callee));
        if !self.history_runtime_metadata_enabled()
            && matches!(self.value_type(function, callee).kind(), MirTypeKind::Fn(_))
        {
            let invocation_args = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    self.call_arg_for_function(
                        function,
                        arg,
                        borrow_mask.get(index).copied().unwrap_or(false),
                    )
                })
                .collect::<Vec<_>>();
            return self.fn_value_call(self.value_read(callee), &invocation_args);
        }
        self.call_symbol_for_function(
            function,
            self.value_read(callee),
            args,
            type_args,
            Some(&borrow_mask),
        )
    }

    fn exact_prelude_adapter(&self, symbol: &str) -> Option<(&'static str, &'static [usize])> {
        let bare = symbol
            .strip_prefix(&self.config.root_prefix)
            .unwrap_or(symbol);
        Some(match bare {
            "jet_fraction_from_parts" => ("jet_fraction_from_owned_parts", &[0, 1]),
            "JetDate::new" => ("jet_local_date_owned", &[0, 1, 2]),
            "JetLocalTime::new" => ("jet_local_time_owned", &[0, 1, 2]),
            "JetPeriod::new" => ("jet_period_owned", &[0, 1, 2]),
            "jet_std::jet_int_abs" => ("jet_std::jet_int_owned_abs", &[0]),
            "jet_std::jet_int_add" => ("jet_std::jet_int_owned_add", &[0, 1]),
            "jet_std::jet_int_sub" => ("jet_std::jet_int_owned_sub", &[0, 1]),
            "jet_std::jet_int_mul" => ("jet_std::jet_int_owned_mul", &[0, 1]),
            "jet_std::jet_int_bit_and" => ("jet_std::jet_int_owned_bit_and", &[0, 1]),
            "jet_std::jet_int_bit_or" => ("jet_std::jet_int_owned_bit_or", &[0, 1]),
            "jet_std::jet_int_bit_xor" => ("jet_std::jet_int_owned_bit_xor", &[0, 1]),
            "jet_std::jet_int_neg" => ("jet_std::jet_int_owned_neg", &[0]),
            "jet_std::jet_int_not" => ("jet_std::jet_int_owned_not", &[0]),
            "jet_std::jet_int_div" => ("jet_std::jet_int_owned_div", &[0, 1]),
            "jet_std::jet_int_rem" => ("jet_std::jet_int_owned_rem", &[0, 1]),
            "jet_std::jet_int_div_euclid" => ("jet_std::jet_int_owned_div_euclid", &[0, 1]),
            "jet_std::jet_int_rem_euclid" => ("jet_std::jet_int_owned_rem_euclid", &[0, 1]),
            "jet_std::jet_int_floor_div" => ("jet_std::jet_int_owned_floor_div", &[0, 1]),
            "jet_std::jet_int_mod" => ("jet_std::jet_int_owned_mod", &[0, 1]),
            "jet_std::jet_int_pow" => ("jet_std::jet_int_owned_pow", &[0, 1]),
            "jet_std::jet_int_shl" => ("jet_std::jet_int_owned_shl", &[0, 1]),
            "jet_std::jet_int_shr" => ("jet_std::jet_int_owned_shr", &[0, 1]),
            "jet_std::jet_int_checked_widen" => ("jet_std::jet_int_owned_checked_widen", &[0]),
            "jet_std::jet_int_try_from_checked" => {
                ("jet_std::jet_int_owned_try_from_checked", &[0])
            }
            "jet_std::jet_int_checked_fixed" => ("jet_std::jet_int_owned_checked_fixed", &[0]),
            "jet_numeric_int_bit_count" => ("jet_std::jet_int_owned_bit_count", &[0]),
            "jet_inline_range_from_int" => ("jet_std::jet_int_owned_inline_range", &[0]),
            "jet_std::jet_int_to_radix" => ("jet_std::jet_int_owned_to_radix", &[0, 1]),
            "jet_std::jet_int_from_radix" => ("jet_std::jet_int_owned_from_radix", &[1]),
            "jet_std::jet_int_parse" => ("jet_std::jet_int_owned_parse", &[]),
            "jet_fmt_decimal_int" => ("jet_std::jet_fmt_decimal_int_owned", &[0, 1]),
            "jet_fmt_grouped_int" => ("jet_std::jet_fmt_grouped_int_owned", &[0, 1]),
            "jet_fmt_hex" => ("jet_std::jet_fmt_hex_owned", &[0, 1]),
            "jet_fmt_bin" => ("jet_std::jet_fmt_bin_owned", &[0]),
            "jet_fmt_oct" => ("jet_std::jet_fmt_oct_owned", &[0]),
            _ => return None,
        })
    }

    fn call_arg(&self, arg: &MirCallArg, borrowed: bool) -> String {
        let mut value = match arg.access {
            MirAccess::Read => self.value_read(arg.value),
            MirAccess::Write => self.value_borrow_mut(arg.value),
            MirAccess::Move => self.value_move(arg.value),
        };
        if arg.authority_boundary {
            value = format!("jet_authority_to_wire(&({value}))");
        }
        if arg.implicit_clone || arg.shared_auto_clone {
            value = format!("({value}).clone()");
        }
        if arg.widen_fixed_to_list {
            value = format!("({value}).to_vec()");
        }
        if let Some(coercion) = &arg.widen_to_union {
            value = format!(
                "{}::{}({value})",
                self.type_name(coercion.union),
                mangle(&coercion.variant)
            );
        }
        if let Some(trait_id) = arg.box_as_trait {
            let trait_object = self.trait_object_type(trait_id);
            value = format!("Box::new({value}) as {}", self.rust_type(trait_object));
        }
        if let Some(coercion) = &arg.fn_coercion {
            if !coercion.already_boxed {
                let rust_ty = self.rust_local_type(&coercion.ty);
                let wrap = if rust_ty.starts_with("std::rc::Rc<") {
                    "std::rc::Rc::new"
                } else {
                    "Box::new"
                };
                value = format!("{wrap}({value}) as {rust_ty}");
            }
        }
        if borrowed && !matches!(arg.access, MirAccess::Write) {
            format!("&({value})")
        } else {
            value
        }
    }
    fn call_arg_for_function(
        &self,
        function: &MirFunction,
        arg: &MirCallArg,
        borrowed: bool,
    ) -> String {
        if arg.access == MirAccess::Write {
            if let Some(place) = arg.place {
                return self.place_reference(function, place, MirAccess::Write);
            }
            if matches!(
                self.value_definition(function, arg.value),
                Some(
                    MirOperation::ReadPlace(_)
                        | MirOperation::Parameter { .. }
                        | MirOperation::Capture { .. }
                )
            ) {
                return self.borrowed_value_reference(function, arg.value, MirAccess::Write);
            }
            panic!("MIR write call argument has no checked place");
        }
        if borrowed
            && arg.access == MirAccess::Read
            && !arg.authority_boundary
            && !arg.implicit_clone
            && !arg.shared_auto_clone
            && !arg.widen_fixed_to_list
            && arg.widen_to_union.is_none()
            && arg.box_as_trait.is_none()
            && arg.fn_coercion.is_none()
        {
            if let Some(place) = arg.place {
                return self.place_reference(function, place, MirAccess::Read);
            }
            if matches!(
                self.value_definition(function, arg.value),
                Some(
                    MirOperation::ReadPlace(_)
                        | MirOperation::Parameter { .. }
                        | MirOperation::Capture { .. }
                )
            ) {
                return self.borrowed_value_reference(function, arg.value, MirAccess::Read);
            }
            return self.value_slot_reference(arg.value, false);
        }
        self.call_arg(arg, borrowed)
    }
    fn prelude_host_call(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        args: &[MirCallArg],
        location: Option<&MirPanicLoc>,
    ) -> String {
        let route = self.prelude_row(call);
        self.validate_prelude_count(route, args.len());
        let needs_numeric_adapter = route.module == "core.clock"
            && args.iter().enumerate().any(|(index, arg)| {
                !route.signature.borrow_mask[index]
                    && matches!(
                        self.value_type(function, arg.value).kind(),
                        MirTypeKind::Int
                    )
            });
        if !needs_numeric_adapter {
            return self.call_symbol_for_function(
                function,
                self.prelude_symbol(call),
                args,
                &[],
                Some(&route.signature.borrow_mask),
            );
        }
        let values = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let borrowed = route.signature.borrow_mask[index];
                let value = self.call_arg_for_function(function, arg, borrowed);
                if !borrowed
                    && matches!(
                        self.value_type(function, arg.value).kind(),
                        MirTypeKind::Int
                    )
                {
                    self.native_int_argument(value, location)
                } else {
                    value
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}({values})", self.prelude_symbol(call))
    }

    fn rust_prelude_type_arg(&self, row: &MirPreludeCall, type_arg: &MirPreludeTypeArg) -> String {
        match type_arg {
            MirPreludeTypeArg::Type(ty)
                if row.module == "::JetAtomic"
                    && matches!(row.member.as_str(), "new" | "try_new")
                    && matches!(ty.kind(), MirTypeKind::Int) =>
            {
                "JetAtomicInt".to_string()
            }
            MirPreludeTypeArg::Type(ty) => self.rust_type(ty),
            MirPreludeTypeArg::HostUsize => "usize".to_string(),
        }
    }

    fn static_prelude_call(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        args: &[MirCallArg],
        owner_type_args: &[MirPreludeTypeArg],
        type_args: &[MirType],
    ) -> String {
        let row = self.prelude_row(call);
        if owner_type_args.is_empty() {
            return self.call_symbol_for_function(
                function,
                self.prelude_symbol(call),
                args,
                type_args,
                Some(&row.signature.borrow_mask),
            );
        }
        let symbol = self.prelude_symbol(call);
        let (owner, method) = symbol
            .rsplit_once("::")
            .unwrap_or_else(|| panic!("MIR static Prelude symbol `{symbol}` has no method"));
        let owner_args = owner_type_args
            .iter()
            .map(|type_arg| self.rust_prelude_type_arg(row, type_arg))
            .collect::<Vec<_>>()
            .join(", ");
        let symbol = format!("{owner}::<{owner_args}>::{method}");
        self.call_symbol_for_function(
            function,
            symbol,
            args,
            type_args,
            Some(&row.signature.borrow_mask),
        )
    }

    fn call_symbol_for_function(
        &self,
        function: &MirFunction,
        symbol: String,
        args: &[MirCallArg],
        type_args: &[MirType],
        borrow_mask: Option<&Vec<bool>>,
    ) -> String {
        let adapter = self
            .exact_prelude_adapter(&symbol)
            .filter(|(_, positions)| {
                positions.iter().all(|index| {
                    args.get(*index).is_some_and(|arg| {
                        matches!(
                            self.value_type(function, arg.value).kind(),
                            MirTypeKind::Int
                        )
                    })
                })
            });
        let symbol = adapter
            .map(|(name, _)| format!("{}{}", self.config.root_prefix, name))
            .unwrap_or(symbol);
        let forced_borrows = adapter.map(|(_, positions)| positions);
        let generic = if type_args.is_empty() {
            String::new()
        } else {
            format!(
                "::<{}>",
                type_args
                    .iter()
                    .map(|ty| self.rust_type(ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let args = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                self.call_arg_for_function(
                    function,
                    arg,
                    borrow_mask
                        .and_then(|mask| mask.get(index))
                        .copied()
                        .unwrap_or(false)
                        || forced_borrows.is_some_and(|positions| positions.contains(&index)),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{symbol}{generic}({args})")
    }

    fn history_closure(
        &self,
        outer: &MirFunction,
        target: &MirFunction,
        captures: &[MirCaptureOperand],
        result: Option<MirValueId>,
    ) -> String {
        let send = result.is_some_and(|value| {
            matches!(
                self.value_type(outer, value).kind(),
                MirTypeKind::SendFn { .. }
            )
        });
        let root = &self.config.root_prefix;
        let tree = format!("{root}jet_std::DataTree");
        let (owner, cell, read, write, reader_ty, register) = if send {
            (
                "std::sync::Arc",
                "std::sync::Mutex",
                "try_lock().map_err(|_| \"history captures are busy\".to_string())?",
                "lock().unwrap_or_else(|error| error.into_inner())",
                "JetHistorySendCaptureReader",
                "jet_history_register_send_callback",
            )
        } else {
            (
                "std::rc::Rc",
                "std::cell::RefCell",
                "try_borrow().map_err(|_| \"history captures are busy\".to_string())?",
                "borrow_mut()",
                "JetHistoryCaptureReader",
                "jet_history_register_callback",
            )
        };
        let edits_environment = target.capture_params.iter().any(|capture| {
            capture.access != MirAccess::Read || self.capture_move_required(target, capture.slot)
        });
        let invoke_borrow = if !send && !edits_environment {
            "borrow()"
        } else {
            write
        };
        let environment_ref = if edits_environment { "&mut" } else { "&" };
        let mut setup = String::new();
        let mut capture_setup = String::new();
        let mut arguments = Vec::new();
        let mut encoded = Vec::new();
        let mut names = Vec::new();
        for (index, (operand, capture)) in captures.iter().zip(&target.capture_params).enumerate() {
            let name = self.capture_param_name(index);
            let move_required = self.capture_move_required(target, capture.slot);
            let slot = format!("__jet_history_env.{index}");
            let owns_local_read = capture.access == MirAccess::Read
                && !move_required
                && matches!(
                        operand,
                        MirCaptureOperand::Place(id)
                            if outer
                                .places
                                .iter()
                                .find(|place| place.id == *id)
                                .is_some_and(|place| matches!(place.base, MirPlaceBase::Local(_)))
                );
            let initial = match operand {
                MirCaptureOperand::Value(value) => self.value_move(*value),
                MirCaptureOperand::Place(id) if move_required => {
                    self.move_place_for_capture(outer, *id)
                }
                MirCaptureOperand::Place(id) if owns_local_read => self.place_read(outer, *id),
                MirCaptureOperand::Place(id) if capture.access == MirAccess::Move => {
                    self.move_place(outer, *id)
                }
                MirCaptureOperand::Place(id) => self.place_reference(outer, *id, capture.access),
            };
            // Only actual move captures need a consumed state. Read/write
            // captures—including sema-cloned/materialized owned slots—retain
            // their environment across every callback invocation. A read of
            // an outer local is cloned because this callback outlives the
            // local's other branch assignments.
            let (initial, referent, argument) = if move_required {
                (
                    format!("Some({initial})"),
                    format!("{slot}.as_ref().ok_or_else(|| \"history capture was consumed\".to_string())?"),
                    format!("{slot}.take().expect(\"MIR move capture\")"),
                )
            } else {
                match capture.access {
                MirAccess::Move => (
                    format!("Some({initial})"),
                    format!("{slot}.as_ref().ok_or_else(|| \"history capture was consumed\".to_string())?"),
                    format!("{slot}.take().expect(\"MIR move capture\")"),
                ),
                MirAccess::Read => {
                    let value = if matches!(operand, MirCaptureOperand::Place(_)) && !owns_local_read {
                        format!("*{slot}")
                    } else {
                        slot.clone()
                    };
                    (initial, format!("&({value})"), format!("&({value})"))
                }
                MirAccess::Write => {
                    let value = if matches!(operand, MirCaptureOperand::Place(_)) { format!("*{slot}") } else { slot.clone() };
                    (initial, format!("&({value})"), format!("&mut ({value})"))
                }
                }
            };
            let encoded_value = self.history_capture_expression(&capture.ty, &referent);
            let encoded_name = format!("__jet_history_capture_value_{index}");
            let _ = writeln!(capture_setup, "let {encoded_name} = {encoded_value};\n");
            let _ = writeln!(setup, "let {name} = {initial};");
            names.push(name);
            encoded.push(encoded_name);
            arguments.push(argument);
        }
        let types_slot = names.len();
        names.push("__jet_history_types.clone()".to_string());
        let env = format!("({},)", names.join(", "));
        let identity = quote_rust_string(&format!(
            "function:{}:{}-{}",
            target.key, target.span.start, target.span.end
        ));
        let captures = encoded.join(", ");
        let params = target
            .params
            .iter()
            .map(|param| {
                let ty = self.rust_local_type(&param.ty);
                let ty = match param.access {
                    MirAccess::Write => format!("&mut {ty}"),
                    MirAccess::Read if self.parameter_borrowed(param) => format!("&{ty}"),
                    MirAccess::Read | MirAccess::Move => ty,
                };
                format!(
                    "mut {}: {}",
                    mangle(&param.name),
                    ty
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        arguments.extend(target.params.iter().map(|param| mangle(&param.name)));
        let invocation = format!(
            "{}({})",
            self.function_name(target.id),
            arguments.join(", ")
        );
        let invocation = if target.is_unsafe {
            format!("unsafe {{ {invocation} }}")
        } else {
            invocation
        };
        let invocation = format!(
            "{root}jet_history_with_types(__jet_history_env.{types_slot}.clone(), || {invocation})"
        );
        let wrap = if send {
            "std::sync::Arc::new"
        } else {
            "Box::new"
        };
        let ticket = if send {
            "JetHistorySendTicket"
        } else {
            "JetHistoryLocalTicket"
        };
        format!(
            "{{ {setup} let __jet_history_owner = {owner}::new({cell}::new({env})); \
             let __jet_history_reader: {owner}<{root}{reader_ty}<'_>> = {{ let __jet_history_owner = __jet_history_owner.clone(); {owner}::new(move || {{ \
             let __jet_history_guard = __jet_history_owner.{read}; let __jet_history_env = &*__jet_history_guard; let __jet_history_types = &__jet_history_env.{types_slot}; let __jet_history_depth = 0usize; {capture_setup} \
             Ok({tree}::Object(vec![(\"function_identity\".to_string(), {tree}::Text({identity}.to_string())), (\"captures\".to_string(), {tree}::Array(vec![{captures}]))])) }}) }}; \
             let __jet_history_keep = __jet_history_reader.clone(); let __jet_history_ticket = {root}{ticket}::default(); let __jet_history_life = __jet_history_ticket.0.clone(); \
             let __jet_history_callback = {wrap}(move |{params}| {{ let _ = (&__jet_history_keep, &__jet_history_ticket); let mut __jet_history_guard = __jet_history_owner.{invoke_borrow}; let __jet_history_env = {environment_ref} *__jet_history_guard; {invocation} }}); \
             {root}{register}(__jet_history_callback, &__jet_history_reader, &__jet_history_life) }}"
        )
    }

    fn closure(
        &self,
        outer: &MirFunction,
        target_id: MirFunctionId,
        captures: &[MirCaptureOperand],
        _facts: &MirCaptureFacts,
        result: Option<MirValueId>,
    ) -> String {
        let target = self.function_row(target_id);
        if captures.len() != target.capture_params.len() {
            panic!(
                "MIR closure for function {:?} has {} captures, expected {}",
                target_id,
                captures.len(),
                target.capture_params.len()
            );
        }
        if self.history_runtime_metadata_enabled() {
            return self.history_closure(outer, target, captures, result);
        }
        let mut setup = String::new();
        let mut call_args = Vec::with_capacity(captures.len() + target.params.len());
        for (index, (operand, capture)) in captures.iter().zip(&target.capture_params).enumerate() {
            if capture.slot != index {
                panic!("MIR closure capture slots are not ordered");
            }
            let name = self.capture_param_name(capture.slot);
            let move_required = self.capture_move_required(target, capture.slot);
            let captured = match (capture.access, operand) {
                (_, MirCaptureOperand::Place(place)) if move_required => {
                    self.move_place_for_capture(outer, *place)
                }
                (_, MirCaptureOperand::Value(value)) if move_required => self.value_move(*value),
                (MirAccess::Read, MirCaptureOperand::Place(place)) => {
                    self.place_reference(outer, *place, MirAccess::Read)
                }
                (MirAccess::Write, MirCaptureOperand::Place(place)) => {
                    self.place_reference(outer, *place, MirAccess::Write)
                }
                (MirAccess::Move, MirCaptureOperand::Value(value))
                | (MirAccess::Read, MirCaptureOperand::Value(value))
                | (MirAccess::Write, MirCaptureOperand::Value(value))
                    if capture.access == MirAccess::Move
                        || matches!(capture.ownership.mode, MirOwnershipMode::Owned) =>
                {
                    self.value_move(*value)
                }
                (access, MirCaptureOperand::Value(_)) => {
                    panic!(
                        "MIR closure capture value operand has incompatible {:?} access",
                        access
                    )
                }
                (access, MirCaptureOperand::Place(_)) => {
                    panic!(
                        "MIR closure capture place operand has incompatible {:?} access",
                        access
                    )
                }
            };
            if capture.access == MirAccess::Write && matches!(operand, MirCaptureOperand::Value(_))
            {
                let _ = writeln!(setup, "let mut {name} = {captured};");
            } else {
                let _ = writeln!(setup, "let {name} = {captured};");
            }
            let call_arg = if move_required {
                name
            } else {
                match (capture.access, operand) {
                    (MirAccess::Read, MirCaptureOperand::Value(_)) => format!("&{name}"),
                    (MirAccess::Write, MirCaptureOperand::Value(_)) => format!("&mut {name}"),
                    _ => name,
                }
            };
            call_args.push(call_arg);
        }
        let params = target
            .params
            .iter()
            .map(|param| format!("{}: {}", mangle(&param.name), self.parameter_type(param)))
            .collect::<Vec<_>>();
        call_args.extend(target.params.iter().map(|param| mangle(&param.name)));
        let invocation = format!(
            "{}({})",
            self.function_name(target_id),
            call_args.join(", ")
        );
        let invocation = if target.is_unsafe {
            format!("unsafe {{ {invocation} }}")
        } else {
            invocation
        };
        let closure = format!("move |{}| {invocation}", params.join(", "));
        let closure = if setup.is_empty() {
            closure
        } else {
            format!("{{ {setup}{closure} }}")
        };
        if result.is_some_and(|value| {
            matches!(
                self.value_type(outer, value).kind(),
                MirTypeKind::SendFn { .. }
            )
        }) {
            format!("std::sync::Arc::new({closure})")
        } else {
            format!("std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new({closure}))))")
        }
    }
    fn host_callback(
        &self,
        function: &MirFunction,
        callable: MirValueId,
        params: &[MirType],
        borrow_inputs: bool,
    ) -> String {
        let send_fn = matches!(
            self.value_type(function, callable).kind(),
            MirTypeKind::SendFn { .. }
        );
        let callback_borrow_mask = self.callable_borrow_mask(self.value_type(function, callable));
        let callback = self.value_move(callable);
        let parameters = params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                format!(
                    "__jet_callback_arg_{index}: {}{}",
                    if borrow_inputs { "&" } else { "" },
                    self.rust_type(ty)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let arguments = params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = format!("__jet_callback_arg_{index}");
                let borrowed = callback_borrow_mask
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| panic!("MIR callback argument {index} has no callable ABI row"));
                if borrow_inputs {
                    if borrowed {
                        name
                    } else if self.is_scalar(ty) {
                        format!("*{name}")
                    } else {
                        panic!(
                            "MIR borrowed callback would move a non-scalar argument {index}"
                        )
                    }
                } else if borrowed {
                    format!("&{name}")
                } else {
                    name
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let wrapper = if send_fn {
            "std::sync::Arc::new"
        } else {
            "Box::new"
        };
        if self.history_runtime_metadata_enabled() {
            let root = &self.config.root_prefix;
            let (owner, reader, register) = if send_fn {
                (
                    "std::sync::Arc",
                    "JetHistorySendCaptureReader",
                    "jet_history_register_send_callback",
                )
            } else {
                (
                    "std::rc::Rc",
                    "JetHistoryCaptureReader",
                    "jet_history_register_callback",
                )
            };
            let ticket = if send_fn {
                "JetHistorySendTicket"
            } else {
                "JetHistoryLocalTicket"
            };
            return format!(
                "{{ let __jet_callback = {owner}::new({callback}); \
                 let __jet_reader: {owner}<{root}{reader}<'_>> = {{ let __jet_callback = __jet_callback.clone(); {owner}::new(move || {root}jet_history_callback_captures(__jet_callback.as_ref().as_ref())) }}; \
                 let __jet_keep = __jet_reader.clone(); let __jet_ticket = {root}{ticket}::default(); let __jet_life = __jet_ticket.0.clone(); \
                 let __jet_wrapper = {wrapper}(move |{parameters}| {{ let _ = (&__jet_keep, &__jet_ticket); __jet_callback({arguments}) }}); \
                 {root}{register}(__jet_wrapper, &__jet_reader, &__jet_life) }}"
            );
        }
        let invocation = if send_fn {
            format!("__jet_callback({arguments})")
        } else {
            self.fn_value_call("__jet_callback.clone()".to_string(), &[arguments])
        };
        format!(
            "{{ let __jet_callback = {callback}; {wrapper}(move |{parameters}| {invocation}) }}"
        )
    }
    fn place_move_for_drop(&self, function: &MirFunction, place: &MirPlace) -> String {
        if place.access != MirAccess::Move {
            panic!("MIR drop place {:?} is not an owned move place", place.id);
        }
        self.move_place(function, place.id)
    }

    fn close_handle_drop(&self, handle: &MirHandleLifecycle, moved: String) -> String {
        if !matches!(handle.ownership, MirHandleOwnership::Owned) {
            return format!("{{ let _ = {moved}; () }}");
        }
        if let Some(close_id) = handle.close_foreign {
            let close = self.foreign_name(close_id);
            return format!(
                "{{ let __jet_handle = {moved}; unsafe {{ let _ = {close}(__jet_handle.into_raw()); }} () }}"
            );
        }
        if let Some(close_id) = handle.close {
            let close = self.function_name(close_id);
            let call = if self.function_row(close_id).is_unsafe {
                format!("unsafe {{ {close}(__jet_handle) }}")
            } else {
                format!("{close}(__jet_handle)")
            };
            return format!("{{ let __jet_handle = {moved}; let _ = {call}; () }}");
        }
        panic!("MIR foreign-handle drop has no canonical close operation");
    }

    fn address_of(&self, function: &MirFunction, place: MirPlaceId, access: MirAccess) -> String {
        self.place_reference(function, place, access)
    }

    fn raw_address_of(&self, function: &MirFunction, place: MirPlaceId) -> String {
        format!("({} as *const _ as *mut _)", self.place_reference(function, place, MirAccess::Read))
    }

    fn drop_expression(
        &self,
        function: &MirFunction,
        value: MirValueId,
        kind: MirDropKind,
    ) -> String {
        if self
            .history_binding(value)
            .is_some_and(|(_, access)| access != MirAccess::Move)
        {
            return "()".to_string();
        }
        if matches!(kind, MirDropKind::ForeignHandle) {
            let ty = self.value_type(function, value);
            let handle = self
                .handle_for_type(ty)
                .unwrap_or_else(|| panic!("MIR foreign-handle drop has no lifecycle row"));
            return self.close_handle_drop(handle, self.value_move(value));
        }
        match kind {
            MirDropKind::None => format!("{{ let _ = {}; () }}", self.value_move(value)),
            MirDropKind::Value => format!("{{ drop({}); () }}", self.value_move(value)),
            MirDropKind::Shared => format!("{{ drop({}); () }}", self.value_move(value)),
            MirDropKind::View => format!("{{ drop({}); () }}", self.value_move(value)),
            MirDropKind::ForeignHandle => unreachable!(),
        }
    }

    fn map_guard_path(
        &self,
        mut expression: String,
        path: &[MirFieldId],
        editable: bool,
        route: &MirPreludeCall,
    ) -> String {
        let method = route.member.as_str();
        let borrow = if editable { "&mut " } else { "&" };
        for field in path {
            let field_name = self.field_name(*field);
            expression = format!(
                "({expression}).{method}({}i64, |__jet_guard_value| {borrow}__jet_guard_value.{field_name})",
                field.0
            );
        }
        expression
    }

    fn shared_guard_map(
        &self,
        call: MirPreludeCallId,
        guard: MirValueId,
        path: &[MirFieldId],
        editable: bool,
    ) -> String {
        if path.is_empty() {
            panic!("MIR shared guard map has no checked field path");
        }
        let route = self.prelude_row(call);
        self.map_guard_path(self.value_move(guard), path, editable, route)
    }

    fn shared_guard_split(
        &self,
        call: MirPreludeCallId,
        map_call: MirPreludeCallId,
        guard: MirValueId,
        first: &[MirFieldId],
        second: &[MirFieldId],
        editable: bool,
    ) -> String {
        if first.is_empty() || second.is_empty() {
            panic!("MIR shared guard split has an empty checked field path");
        }
        let mut common = 0usize;
        while common < first.len() && common < second.len() && first[common] == second[common] {
            common += 1;
        }
        if common == first.len() || common == second.len() {
            panic!("MIR shared guard split paths are identical or prefix-related");
        }
        let map_route = self.prelude_row(map_call);
        let split_route = self.prelude_row(call);
        let prefix = self.map_guard_path(
            self.value_move(guard),
            &first[..common],
            editable,
            map_route,
        );
        let method = split_route.member.as_str();
        let borrow = if editable { "&mut " } else { "&" };
        let first_field = self.field_name(first[common]);
        let second_field = self.field_name(second[common]);
        let project = format!(
            "|__jet_guard_value| ({borrow}__jet_guard_value.{first_field}, {borrow}__jet_guard_value.{second_field})"
        );
        let split = format!(
            "({prefix}).{method}({}i64, {}i64, {project})",
            first[common].0, second[common].0
        );
        let first_mapped = self.map_guard_path(
            "__jet_split_first".to_string(),
            &first[common + 1..],
            editable,
            map_route,
        );
        let second_mapped = self.map_guard_path(
            "__jet_split_second".to_string(),
            &second[common + 1..],
            editable,
            map_route,
        );
        format!(
            "{{ let (__jet_split_first, __jet_split_second) = {split}; ({first_mapped}, {second_mapped}) }}"
        )
    }
    fn validate_dma_buffer_type(
        &self,
        function: &MirFunction,
        actual_value: MirValueId,
        expected: &MirType,
        context: &str,
    ) {
        let actual = self.value_type(function, actual_value);
        if !actual.same_checked_type(expected) {
            panic!("MIR hardware DMA {context} buffer type disagrees with checked buffer_ty");
        }
    }
    fn hardware_register_helper(
        &self,
        operation: &str,
        profile_id: &str,
        block: &str,
        register: &str,
        width: jet_foundation::TargetMachine::RegisterWidth,
    ) -> String {
        let profile = self
            .program
            .facts
            .hardware_profile
            .as_ref()
            .unwrap_or_else(|| panic!("MIR hardware register has no checked profile"));
        if self.program.facts.hardware_profile_id != profile_id {
            panic!("MIR hardware register profile disagrees with checked profile");
        }
        let block_fact = profile
            .register_blocks
            .iter()
            .find(|candidate| candidate.name == block)
            .unwrap_or_else(|| panic!("MIR hardware register has unknown block"));
        let register_fact = block_fact
            .registers
            .iter()
            .find(|candidate| candidate.name == register)
            .unwrap_or_else(|| panic!("MIR hardware register has unknown register"));
        if register_fact.width != width {
            panic!("MIR hardware register width disagrees with checked fact");
        }
        let allowed = match operation {
            "read" => register_fact.access.can_read(),
            "write" => register_fact.access.can_write(),
            _ => panic!("MIR hardware register has unknown operation"),
        };
        if !allowed {
            panic!("MIR hardware register operation disagrees with checked access");
        }
        self.hardware_register_helper_name(operation, profile_id, block, register)
    }

    fn hardware_call(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        op: &MirHardwareOp,
        receiver: Option<MirValueId>,
        args: &[MirCallArg],
    ) -> String {
        let route = self.prelude_row(call);
        self.validate_prelude_count(route, args.len());
        if route.signature.borrow_mask.iter().any(|borrowed| *borrowed) {
            panic!("MIR hardware route {:?} has borrowed arguments", call);
        }
        let root = &self.config.root_prefix;
        match op {
            MirHardwareOp::RegisterRead {
                profile_id,
                block,
                register,
                width,
            } => {
                if receiver.is_some() || !args.is_empty() {
                    panic!("MIR hardware register read has unexpected receiver or arguments");
                }
                let helper =
                    self.hardware_register_helper("read", profile_id, block, register, *width);
                format!("{root}{helper}()")
            }
            MirHardwareOp::RegisterWrite {
                profile_id,
                block,
                register,
                width,
            } => {
                if receiver.is_some() || args.len() != 1 {
                    panic!("MIR hardware register write requires one value argument");
                }
                if args[0].access == MirAccess::Write {
                    panic!("MIR hardware register write value cannot be a borrowed place");
                }
                let value = self.call_arg(&args[0], false);
                let helper =
                    self.hardware_register_helper("write", profile_id, block, register, *width);
                format!("{root}{helper}({value})")
            }
            MirHardwareOp::DmaStart {
                profile_id,
                channel,
                buffer_ty,
            } => {
                if receiver.is_some() || args.len() != 1 {
                    panic!("MIR hardware DMA start requires one buffer argument");
                }
                if args[0].access != MirAccess::Move {
                    panic!("MIR hardware DMA start buffer is not an owned move argument");
                }
                self.validate_dma_buffer_type(function, args[0].value, buffer_ty, "start");
                let buffer = self.call_arg(&args[0], false);
                format!("{root}jet_hardware_dma_start_typed({profile_id:?}, {channel:?}, {buffer})")
            }
            MirHardwareOp::DmaWait {
                profile_id,
                channel,
                buffer_ty,
            } => {
                let transfer = receiver
                    .unwrap_or_else(|| panic!("MIR hardware DMA wait has no transfer receiver"));
                if !args.is_empty() {
                    panic!("MIR hardware DMA wait has unexpected value arguments");
                }
                let transfer_ty = self.value_type(function, transfer);
                let MirTypeKind::Apply { name, args } = transfer_ty.kind() else {
                    panic!("MIR hardware DMA wait receiver is not a checked transfer type");
                };
                if name.name != "__JetDmaTransfer"
                    || args.len() != 1
                    || !args[0].same_checked_type(buffer_ty)
                {
                    panic!("MIR hardware DMA wait receiver disagrees with checked buffer_ty");
                }
                format!(
                    "{root}jet_hardware_dma_wait_typed({profile_id:?}, {channel:?}, {})",
                    self.value_move(transfer)
                )
            }
        }
    }

    fn plugin_wire_path(&self, item: &str) -> String {
        format!("{}jet_std::plugin_wire::{item}", self.config.root_prefix)
    }

    fn plugin_unwrap_type<'b>(&'b self, ty: &'b MirType) -> &'b MirType {
        match ty.kind() {
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.plugin_unwrap_type(base),
            MirTypeKind::Apply { name, .. } => {
                let Some(definition) = self
                    .program
                    .types
                    .iter()
                    .find(|definition| definition.id == name.id)
                else {
                    return ty;
                };
                match &definition.kind {
                    MirTypeDefKind::Alias { target } => self.plugin_unwrap_type(target),
                    _ => ty,
                }
            }
            _ => ty,
        }
    }

    fn plugin_record_def(&self, ty: &MirType, descriptor: &ComponentTypeDescriptor) -> &MirTypeDef {
        let ty = self.plugin_unwrap_type(ty);
        let identity = ty.identity;
        let nominal_name = ty.nominal_name().unwrap_or_default();
        let descriptor_name = match descriptor {
            ComponentTypeDescriptor::Record { name, .. } => name.as_str(),
            _ => panic!("MIR plugin record descriptor has a non-record source type"),
        };
        if let Some(identity) = identity {
            return self
                .program
                .types
                .iter()
                .find(|definition| definition.id == identity)
                .unwrap_or_else(|| {
                    panic!("MIR plugin record `{descriptor_name}` has no checked type definition")
                });
        }
        self.program
            .types
            .iter()
            .find(|definition| {
                definition.key == nominal_name
                    || definition.name == nominal_name
                    || definition.key == descriptor_name
                    || definition.name == descriptor_name
            })
            .unwrap_or_else(|| {
                panic!("MIR plugin record `{descriptor_name}` has no checked type definition")
            })
    }

    fn plugin_record_field<'b>(
        &self,
        definition: &'b MirTypeDef,
        index: usize,
        name: &str,
    ) -> &'b MirField {
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            panic!("MIR plugin record descriptor names a non-struct type");
        };
        fields
            .get(index)
            .filter(|field| {
                field.name == name
                    || mangle(&field.name) == name
                    || field
                        .shape_names
                        .name_for(ShapeProjectionKind::Json)
                        .expect("checked MIR field is missing its JSON shape name")
                        == name
            })
            .or_else(|| {
                fields.iter().find(|field| {
                    field.name == name
                        || mangle(&field.name) == name
                        || field
                            .shape_names
                            .name_for(ShapeProjectionKind::Json)
                            .expect("checked MIR field is missing its JSON shape name")
                            == name
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "MIR plugin record field `{name}` (index {index}) is absent from its type definition"
                )
            })
    }

    fn plugin_encode_value_expr(
        &self,
        descriptor: &ComponentTypeDescriptor,
        ty: &MirType,
        value: &str,
        depth: usize,
    ) -> String {
        let wire = self.plugin_wire_path("PluginValue");
        let ty = self.plugin_unwrap_type(ty);
        match descriptor {
            ComponentTypeDescriptor::Int => {
                format!("{wire}::Int(({value}) as i64)")
            }
            ComponentTypeDescriptor::Float => {
                format!("{wire}::Float(({value}) as f64)")
            }
            ComponentTypeDescriptor::Bool => format!("{wire}::Bool({value})"),
            ComponentTypeDescriptor::String => {
                format!("{wire}::Text(({value}).to_string())")
            }
            ComponentTypeDescriptor::List(inner) => {
                let child_ty = match ty.kind() {
                    MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                        inner.as_ref()
                    }
                    _ => panic!("MIR plugin list descriptor has a non-list value type"),
                };
                let child_name = format!("__jet_plugin_encode_item_{depth}");
                let child = self.plugin_encode_value_expr(
                    inner,
                    child_ty,
                    &child_name,
                    depth.saturating_add(1),
                );
                let iterator = match ty.kind() {
                    MirTypeKind::List(inner) if self.columnar_type_def(inner).is_some() => {
                        format!("({value}).to_aos().into_iter()")
                    }
                    _ => format!("({value}).into_iter()"),
                };
                format!("{wire}::List({iterator}.map(|{child_name}| {child}).collect())")
            }
            ComponentTypeDescriptor::Option(inner) => {
                let child_ty = match ty.kind() {
                    MirTypeKind::Option(inner) => inner.as_ref(),
                    _ => panic!("MIR plugin option descriptor has a non-option value type"),
                };
                let child_name = format!("__jet_plugin_option_value_{depth}");
                let child = self.plugin_encode_value_expr(
                    inner,
                    child_ty,
                    &child_name,
                    depth.saturating_add(1),
                );
                format!(
                    "match ({value}) {{ \
                     Ok({child_name}) => {wire}::Option(Some(Box::new({child}))), \
                     Err(_) => {wire}::Option(None) \
                     }}"
                )
            }
            ComponentTypeDescriptor::Result { ok, err } => {
                let (ok_ty, err_ty) = match ty.kind() {
                    MirTypeKind::Result { ok, err } => (ok.as_ref(), err.as_ref()),
                    _ => panic!("MIR plugin result descriptor has a non-result value type"),
                };
                let ok_name = format!("__jet_plugin_result_ok_{depth}");
                let err_name = format!("__jet_plugin_result_err_{depth}");
                let ok_value =
                    self.plugin_encode_value_expr(ok, ok_ty, &ok_name, depth.saturating_add(1));
                let err_value =
                    self.plugin_encode_value_expr(err, err_ty, &err_name, depth.saturating_add(1));
                format!(
                    "match ({value}) {{ \
                     Ok({ok_name}) => {wire}::ResultOk(Some(Box::new({ok_value}))), \
                     Err({err_name}) => {wire}::ResultErr(Some(Box::new({err_value}))) \
                     }}"
                )
            }
            ComponentTypeDescriptor::Record { fields, .. } => {
                let definition = self.plugin_record_def(ty, descriptor);
                let record_name = format!("__jet_plugin_record_{depth}");
                let encoded_fields = fields
                    .iter()
                    .map(|(index, name, field_descriptor)| {
                        let field = self.plugin_record_field(definition, *index, name);
                        let field_value = format!("({record_name}).{}", mangle(&field.name));
                        let encoded = self.plugin_encode_value_expr(
                            field_descriptor,
                            &field.ty,
                            &field_value,
                            depth.saturating_add(1),
                        );
                        format!("({name:?}.to_string(), {encoded})")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{{ let {record_name} = ({value}); \
                     {wire}::Record(vec![{encoded_fields}]) }}"
                )
            }
        }
    }

    fn plugin_decode_value_expr(
        &self,
        descriptor: &ComponentTypeDescriptor,
        ty: &MirType,
        value: &str,
        depth: usize,
    ) -> String {
        let wire = self.plugin_wire_path("PluginValue");
        let ty = self.plugin_unwrap_type(ty);
        match descriptor {
            ComponentTypeDescriptor::Int => format!(
                "match ({value}) {{ \
                 {wire}::Int(value) => Ok(value), \
                 _ => Err(\"plugin returned a value with the wrong Int shape\".to_string()) \
                 }}"
            ),
            ComponentTypeDescriptor::Float => format!(
                "match ({value}) {{ \
                 {wire}::Float(value) => Ok(value), \
                 _ => Err(\"plugin returned a value with the wrong Float shape\".to_string()) \
                 }}"
            ),
            ComponentTypeDescriptor::Bool => format!(
                "match ({value}) {{ \
                 {wire}::Bool(value) => Ok(value), \
                 _ => Err(\"plugin returned a value with the wrong Bool shape\".to_string()) \
                 }}"
            ),
            ComponentTypeDescriptor::String => format!(
                "match ({value}) {{ \
                 {wire}::Text(value) => Ok(value), \
                 _ => Err(\"plugin returned a value with the wrong String shape\".to_string()) \
                 }}"
            ),
            ComponentTypeDescriptor::List(inner) => {
                let child_ty = match ty.kind() {
                    MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                        inner.as_ref()
                    }
                    _ => panic!("MIR plugin list descriptor has a non-list result type"),
                };
                let child_name = format!("__jet_plugin_decode_item_{depth}");
                let child = self.plugin_decode_value_expr(
                    inner,
                    child_ty,
                    &child_name,
                    depth.saturating_add(1),
                );
                let inner_rust = self.rust_type(child_ty);
                let items_name = format!("__jet_plugin_decode_items_{depth}");
                let result_name = format!("__jet_plugin_decode_list_result_{depth}");
                let converted = match ty.kind() {
                    MirTypeKind::FixedList { .. } => {
                        let output = self.rust_type(ty);
                        format!(
                            "let {result_name}: Result<{output}, Vec<{inner_rust}>> = \
                             {items_name}.try_into(); \
                             match {result_name} {{ \
                             Ok(value) => Ok(value), \
                             Err(_) => Err(\"plugin returned a list with the wrong length\".to_string()) \
                             }}"
                        )
                    }
                    MirTypeKind::List(inner) if self.columnar_type_def(inner).is_some() => {
                        let output = self.rust_type(ty);
                        format!("Ok(<{output}>::from_aos({items_name}))")
                    }
                    _ => format!("Ok({items_name})"),
                };
                format!(
                    "match ({value}) {{ \
                     {wire}::List(values) => {{ \
                     let {items_name}: Result<Vec<{inner_rust}>, String> = \
                     values.into_iter().map(|{child_name}| {child}).collect(); \
                     match {items_name} {{ \
                     Ok({items_name}) => {{ {converted} }}, \
                     Err(error) => Err(error) \
                     }} \
                     }}, \
                     _ => Err(\"plugin returned a value with the wrong List shape\".to_string()) \
                     }}"
                )
            }
            ComponentTypeDescriptor::Option(inner) => {
                let child_ty = match ty.kind() {
                    MirTypeKind::Option(inner) => inner.as_ref(),
                    _ => panic!("MIR plugin option descriptor has a non-option result type"),
                };
                let child_name = format!("__jet_plugin_option_value_{depth}");
                let child = self.plugin_decode_value_expr(
                    inner,
                    child_ty,
                    &child_name,
                    depth.saturating_add(1),
                );
                let absent = format!("{}JetAbsent", self.config.root_prefix);
                format!(
                    "match ({value}) {{ \
                     {wire}::Option(None) => Ok(Err({absent})), \
                     {wire}::Option(Some({child_name})) => match {child} {{ \
                     Ok(value) => Ok(Ok(value)), \
                     Err(error) => Err(error) \
                     }}, \
                     _ => Err(\"plugin returned a value with the wrong Option shape\".to_string()) \
                     }}"
                )
            }
            ComponentTypeDescriptor::Result { ok, err } => {
                let (ok_ty, err_ty) = match ty.kind() {
                    MirTypeKind::Result { ok, err } => (ok.as_ref(), err.as_ref()),
                    _ => panic!("MIR plugin result descriptor has a non-result result type"),
                };
                let ok_name = format!("__jet_plugin_result_ok_{depth}");
                let err_name = format!("__jet_plugin_result_err_{depth}");
                let ok_value =
                    self.plugin_decode_value_expr(ok, ok_ty, &ok_name, depth.saturating_add(1));
                let err_value =
                    self.plugin_decode_value_expr(err, err_ty, &err_name, depth.saturating_add(1));
                format!(
                    "match ({value}) {{ \
                     {wire}::ResultOk(Some({ok_name})) => match {ok_value} {{ \
                     Ok(value) => Ok(Ok(value)), \
                     Err(error) => Err(error) \
                     }}, \
                     {wire}::ResultErr(Some({err_name})) => match {err_value} {{ \
                     Ok(value) => Ok(Err(value)), \
                     Err(error) => Err(error) \
                     }}, \
                     {wire}::ResultOk(None) | {wire}::ResultErr(None) => \
                     Err(\"plugin returned a result without its payload\".to_string()), \
                     _ => Err(\"plugin returned a value with the wrong Result shape\".to_string()) \
                     }}"
                )
            }
            ComponentTypeDescriptor::Record { fields, .. } => {
                let definition = self.plugin_record_def(ty, descriptor);
                let record_name = format!("__jet_plugin_record_{depth}");
                let mut setup = Vec::new();
                let mut decoded = Vec::new();
                for (index, name, field_descriptor) in fields {
                    let field = self.plugin_record_field(definition, *index, name);
                    let raw_name = format!("__jet_plugin_record_field_{depth}_{index}");
                    let value_name = format!("__jet_plugin_record_value_{depth}_{index}");
                    setup.push(format!(
                        "let {raw_name} = match {record_name}.iter().position(|(name, _)| name == {name:?}) \
                         {{ Some(index) => {record_name}.swap_remove(index).1, \
                         None => return Err(\"plugin returned a record missing field {name:?}\".to_string()) }};"
                    ));
                    let child = self.plugin_decode_value_expr(
                        field_descriptor,
                        &field.ty,
                        &raw_name,
                        depth.saturating_add(1),
                    );
                    decoded.push(format!(
                        "let {value_name} = match {child} {{ \
                         Ok(value) => value, Err(error) => return Err(error) }};"
                    ));
                }
                let MirTypeDefKind::Struct {
                    fields: declared_fields,
                    ..
                } = &definition.kind
                else {
                    panic!("MIR plugin record descriptor names a non-struct type");
                };
                let assignments = declared_fields
                    .iter()
                    .filter(|field| !field.computed)
                    .map(|field| {
                        if field.skip {
                            return format!("{}: Default::default()", mangle(&field.name));
                        }
                        let descriptor_index = fields
                            .iter()
                            .find(|(index, name, _)| {
                                self.plugin_record_field(definition, *index, name).id == field.id
                            })
                            .map(|(index, _, _)| *index)
                            .unwrap_or_else(|| {
                                panic!(
                                    "MIR plugin record field `{}` has no descriptor entry",
                                    field.name
                                )
                            });
                        format!(
                            "{}: __jet_plugin_record_value_{depth}_{descriptor_index}",
                            mangle(&field.name)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                setup.extend(decoded);
                let output = self.rust_type(ty);
                format!(
                    "match ({value}) {{ \
                     {wire}::Record(mut {record_name}) => {{ \
                     {} \
                     if !{record_name}.is_empty() {{ \
                     return Err(\"plugin returned a record with unexpected fields\".to_string()); \
                     }} \
                     Ok({output} {{ {assignments} }}) \
                     }}, \
                     _ => Err(\"plugin returned a value with the wrong Record shape\".to_string()) \
                     }}",
                    setup.join(" ")
                )
            }
        }
    }

    fn plugin_invoke(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        handle: MirValueId,
        export_name: &str,
        signature: &ComponentSignatureDescriptor,
        args: &[MirValueId],
        result: Option<MirValueId>,
    ) -> String {
        if args.len() != signature.params.len() {
            panic!(
                "MIR plugin export `{export_name}` has {} checked parameters but {} values",
                signature.params.len(),
                args.len()
            );
        }
        let result = result.unwrap_or_else(|| {
            panic!("MIR plugin export `{export_name}` has no checked result value")
        });
        let result_ty = self.value_type(function, result);
        let MirTypeKind::Result { ok, err } = result_ty.kind() else {
            panic!("MIR plugin export `{export_name}` result is not a checked Result")
        };
        if !matches!(self.plugin_unwrap_type(err).kind(), MirTypeKind::String) {
            panic!("MIR plugin export `{export_name}` result error is not checked String")
        }
        let params = signature
            .params
            .iter()
            .zip(args)
            .map(|(descriptor, value)| {
                self.plugin_encode_value_expr(
                    descriptor,
                    self.value_type(function, *value),
                    &self.value_read(*value),
                    0,
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let wire = self.plugin_wire_path("");
        let call = self.prelude_call_args_exact(
            call,
            &[
                format!("({}).handle", self.value_read(handle)),
                format!("{export_name:?}"),
                "&__jet_plugin_params_wire".to_string(),
            ],
        );
        let result_type = self.rust_type(ok);
        let decoded =
            self.plugin_decode_value_expr(&signature.result, ok, "__jet_plugin_result_value", 0);
        format!(
            "{{ \
             let __jet_plugin_params: Vec<{wire}PluginValue> = vec![{params}]; \
             let __jet_plugin_params_wire = {wire}plugin_encode_params(&__jet_plugin_params); \
             let __jet_plugin_wire = {call}; \
             match {wire}plugin_decode_result(&__jet_plugin_wire) {{ \
             Ok(__jet_plugin_result_value) => \
             (|__jet_plugin_result_value: {wire}PluginValue| -> Result<{result_type}, String> {{ \
             {decoded} \
             }})(__jet_plugin_result_value), \
             Err(__jet_plugin_error) => Err(__jet_plugin_error), \
             }} \
             }}"
        )
    }

    fn history_source_field_value(&self, owner: &str, field: &str, value: String) -> String {
        let Some(owner) = crate::Codegen::history_rust_type_name(owner) else {
            return value;
        };
        if (matches!(owner, "HandleId" | "TaskId" | "EventId") && field == "value")
            || (owner == "HistoryCase" && field == "seed")
            || (owner == "HistoryOperation" && field == "index")
            || (owner == "HistoryScheduleChoice" && field == "operation")
        {
            return format!("usize::try_from({value}).expect(\"history value exceeds Count\")");
        }
        if owner == "HistoryOperation" && field == "depends_on" {
            return format!("({value}).into_iter().map(|value| usize::try_from(value).expect(\"history dependency exceeds Count\")).collect::<Vec<_>>()");
        }
        if matches!(owner, "HistoryOperation" | "HistoryScheduleChoice")
            && matches!(field, "task" | "event")
        {
            return format!("({value}).ok_or({}JetAbsent)", self.config.root_prefix);
        }
        value
    }

    fn history_field_expression(
        &self,
        function: &MirFunction,
        base: MirValueId,
        field: MirFieldId,
    ) -> String {
        let field_name = self.field_name(field);
        let value = if self.boxed_field(field) {
            if self.history_runtime_metadata_enabled() {
                format!(
                    "({}).{}.as_ref().clone()",
                    self.value_slot_reference(base, false),
                    field_name
                )
            } else {
                format!(
                    "({}).{}.as_ref().clone()",
                    self.value_read(base),
                    field_name
                )
            }
        } else if self.history_runtime_metadata_enabled() {
            format!(
                "({}).{}.clone()",
                self.value_slot_reference(base, false),
                field_name
            )
        } else {
            format!("({}).{}", self.value_read(base), field_name)
        };
        let ty = self.value_type(function, base);
        if matches!(ty.nominal_name(), Some("DataSummary" | "DataPivotCell"))
            && field_name == "count"
        {
            return self
                .native_int_result(&value, &MirType::from_kind(MirTypeKind::Int))
                .expect("data count has a checked Int result");
        }
        if ty.nominal_name() == Some("AllocError") && field_name == "requested_bytes" {
            return self
                .native_int_result(&value, &MirType::from_kind(MirTypeKind::Int))
                .expect("AllocError requested_bytes has a checked Int result");
        }
        // Prelude VJP continuations use native Rc<Fn> carriers. Projecting
        // one into a checked Jet function value must cross the callable ABI.
        if ty.nominal_name() == Some("VjpRun") {
            let closure = match field_name.as_str() {
                "pull" => Some(format!(
                    "move |__jet_seed: {}| (__jet_native_callback)(&__jet_seed)",
                    self.compute_runtime_name("JetTensor"),
                )),
                "grads" => Some("move || (__jet_native_callback)()".to_string()),
                _ => None,
            };
            if let Some(closure) = closure {
                let carrier = if self.history_runtime_metadata_enabled() {
                    format!("Box::new({closure})")
                } else {
                    format!("std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new({closure}))))")
                };
                return format!("{{ let __jet_native_callback = {value}; {carrier} }}");
            }
        }
        match ty.nominal_name() {
            Some(owner) => self.history_source_field_value(owner, &field_name, value),
            None => value,
        }
    }

    fn history_struct_field_value(
        &self,
        type_id: jet_foundation::MIR::MirTypeId,
        field: jet_foundation::MIR::MirFieldId,
        value: String,
    ) -> String {
        let type_name = self.type_name(type_id);
        let field_name = self.field_name(field);
        if type_name.ends_with("HandleId")
            || type_name.ends_with("TaskId")
            || type_name.ends_with("EventId")
        {
            if field_name == "value" {
                return format!(
                    "u32::try_from({value}).expect(\"history identity value exceeds u32\")"
                );
            }
        }
        if type_name.ends_with("HistoryCase") && field_name == "seed" {
            return format!("u64::try_from({value}).expect(\"history seed exceeds u64\")");
        }
        if (type_name.ends_with("HistoryOperation") && field_name == "index")
            || (type_name.ends_with("HistoryScheduleChoice") && field_name == "operation")
        {
            return format!("u32::try_from({value}).expect(\"history count exceeds u32\")");
        }
        if type_name.ends_with("HistoryOperation") && field_name == "depends_on" {
            return format!(
                "({value}).into_iter().map(|value| u32::try_from(value).expect(\"history dependency exceeds u32\")).collect::<Vec<_>>()"
            );
        }
        if (type_name.ends_with("HistoryOperation") || type_name.ends_with("HistoryScheduleChoice"))
            && matches!(field_name.as_str(), "task" | "event")
        {
            return format!("({value}).ok()");
        }
        value
    }
    fn semantic(
        &self,
        function: &MirFunction,
        operation: &MirSemanticOp,
        result: Option<MirValueId>,
        location: Option<MirPanicLoc>,
    ) -> String {
        match operation {
            MirSemanticOp::DataEntriesToMap { call, local } => {
                self.prelude_call_args(*call, &[self.local_read(function, *local)])
            }
            MirSemanticOp::MathBuiltin { call, args, .. }
            | MirSemanticOp::PreciseBuiltin { call, args, .. } => self.prelude_values(*call, args),
            MirSemanticOp::Print { call, value } => {
                let value_expr = self.value_read(*value);
                let value_expr = if matches!(
                    self.value_type(function, *value).kind(),
                    MirTypeKind::String
                ) {
                    value_expr
                } else {
                    format!("({value_expr}).jet_show()")
                };
                if matches!(
                    self.artifact.kind,
                    MirArtifactKind::TestExecutable
                        | MirArtifactKind::FuzzExecutable
                        | MirArtifactKind::TestOverride
                ) {
                    format!("jet_test_print({value_expr})")
                } else {
                    format!(
                        "{{ let _ = {}; }}",
                        self.prelude_call_args(*call, &[format!("&({value_expr})"), "true".to_string()])
                    )
                }
            }
            MirSemanticOp::AmbientInput { call, prompt } => {
                let prompt = prompt
                    .map(|value| format!("Some(&({}))", self.value_read(value)))
                    .unwrap_or_else(|| "None".to_string());
                self.prelude_call_args(*call, &[prompt])
            }
            MirSemanticOp::RequireStop {
                call,
                kind,
                condition,
                location,
                context,
                values,
                ..
            } => self.require_stop(
                function, *call, *kind, *condition, *location, context, values,
            ),
            MirSemanticOp::LayoutCompare {
                call,
                op,
                left,
                right,
            } => {
                let emitted = self.prelude_values(*call, &[*left, *right]);
                format!("{emitted} /* layout_compare={op:?} */")
            }
            MirSemanticOp::LayoutLiteral { inner } => format!(
                "{}jet_layout::LinExpr::from_const(({}) as f64)",
                self.config.root_prefix,
                self.value_read(*inner)
            ),
            MirSemanticOp::StructLiteral {
                type_id,
                fields,
                boxed_fields,
                trait_coercion,
                ..
            } => {
                let value = self.rust_struct_literal(*type_id, fields, boxed_fields, true);
                match trait_coercion {
                    Some(trait_id) => {
                        let trait_object = self.trait_object_type(*trait_id);
                        format!("Box::new({value}) as {}", self.rust_type(trait_object))
                    }
                    None => value,
                }
            }
            MirSemanticOp::HardwareCall {
                call,
                op,
                receiver,
                args,
            } => self.hardware_call(function, *call, op, *receiver, args),
            MirSemanticOp::StaticPreludeCall {
                call,
                args,
                owner_type_args,
                type_args,
            } => self.static_prelude_call(function, *call, args, owner_type_args, type_args),
            MirSemanticOp::DecodeUnder {
                call,
                segment,
                inner,
            } => self.prelude_values(*call, &[*segment, *inner]),
            MirSemanticOp::BuiltinMethod {
                call,
                receiver,
                receiver_place,
                args,
                ..
            } => {
                let emitted = self.prelude_values_with_receiver(
                    function,
                    *call,
                    *receiver,
                    *receiver_place,
                    args,
                    result,
                );
                result
                    .and_then(|value| {
                        self.native_int_result(&emitted, self.value_type(function, value))
                    })
                    .unwrap_or(emitted)
            }
            MirSemanticOp::CellGuardProject {
                map_call,
                split_call,
                guard,
                paths,
                editable,
                edit_paths_disjoint,
            } => match paths.as_slice() {
                [path] => self.shared_guard_map(*map_call, *guard, path, *editable),
                [first, second] => {
                    if *editable && !*edit_paths_disjoint {
                        panic!("MIR editable cell guard split paths are not disjoint");
                    }
                    let split_call = split_call.unwrap_or_else(|| {
                        panic!("MIR cell guard split has no checked split route")
                    });
                    self.shared_guard_split(split_call, *map_call, *guard, first, second, *editable)
                }
                _ => panic!("MIR cell guard projection has one or two checked paths"),
            },
            MirSemanticOp::SharedGuardMap {
                call,
                guard,
                path,
                editable,
            } => self.shared_guard_map(*call, *guard, path, *editable),
            MirSemanticOp::SharedGuardSplit {
                call,
                map_call,
                guard,
                first,
                second,
                editable,
            } => self.shared_guard_split(*call, *map_call, *guard, first, second, *editable),
            MirSemanticOp::SharedGuardWait {
                call,
                guard,
                condition,
                predicate,
            } => self.prelude_values(*call, &[*guard, *condition, *predicate]),
            MirSemanticOp::ConditionNotify {
                call,
                condition,
                all,
            } => self.prelude_call_args(*call, &[self.value_move(*condition), all.to_string()]),
            MirSemanticOp::AllocNew {
                call,
                kind,
                inline_size,
                args,
            } => {
                let row = self.prelude_row(*call);
                let expected = match kind {
                    MirAllocatorKind::Arena => row.member == "arena.new",
                    MirAllocatorKind::Bump => row.member == "bump.new",
                    MirAllocatorKind::Pool => row.member == "pool.new",
                    MirAllocatorKind::Fixed => {
                        matches!(row.member.as_str(), "fixed.new" | "fixed.over")
                    }
                };
                if !expected {
                    panic!("MIR allocator constructor route does not match its kind");
                }
                self.validate_prelude_count(row, args.len());
                if row.member == "fixed.new" {
                    let size = inline_size
                        .unwrap_or_else(|| panic!("MIR Fixed.new has no checked inline size"));
                    let result = result
                        .unwrap_or_else(|| panic!("MIR Fixed.new has no result slot"));
                    let backing = fixed_inline_backing_name(result);
                    return format!(
                        "{}jet_mem::JetFixed::over_uninit(&mut {backing}) /* allocator=Fixed, inline_size={size} */",
                        self.config.root_prefix,
                    );
                }
                if row.member == "fixed.over" {
                    let over_local = args.first().and_then(|arg| {
                        let place_id = arg.place?;
                        let place = function
                            .places
                            .iter()
                            .find(|place| place.id == place_id)?;
                        if !place.projections.is_empty() {
                            return None;
                        }
                        match place.base {
                            MirPlaceBase::Local(local)
                                if self.local_uninit_fixed_type(function, local).is_some() =>
                            {
                                Some(local)
                            }
                            _ => None,
                        }
                    });
                    if let Some(local) = over_local {
                        let slot = self.local_storage(function, local);
                        return format!(
                            "{{ let mut __jet_bytes = {slot}.as_mut().expect(\"MIR local\"); {}jet_mem::JetFixed::over_uninit_fixed(&mut __jet_bytes) /* allocator=Fixed */ }}",
                            self.config.root_prefix,
                        );
                    }
                }
                let values = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let borrowed = row.signature.borrow_mask.get(index).copied().unwrap_or(false);
                        let value = self.call_arg_for_function(function, arg, borrowed);
                        let native_count = !borrowed
                            && matches!(
                                row.member.as_str(),
                                "arena.new" | "bump.new" | "pool.new"
                            )
                            && matches!(self.value_type(function, arg.value).kind(), MirTypeKind::Int);
                        if native_count {
                            format!(
                                "usize::try_from({}).unwrap_or_else(|_| {}jet_arithmetic_stop(\"<mir>\", 0, \"allocator size is negative or exceeds usize range\"))",
                                self.native_int_argument(value, location.as_ref()),
                                self.config.root_prefix,
                            )
                        } else {
                            value
                        }
                    })
                    .collect::<Vec<_>>();
                let emitted = self.prelude_call_args(*call, &values);
                format!("{emitted} /* allocator={kind:?}, inline_size={inline_size:?} */")
            }
            MirSemanticOp::ColumnarRead {
                base,
                index,
                column,
                column_index,
                accessor,
                ..
            } => self.columnar_read(*accessor, *base, *column, *column_index, *index, location),
            MirSemanticOp::OptionLift2 {
                call,
                function,
                left,
                right,
            } => {
                let emitted = self.prelude_call_args(
                    *call,
                    &[
                        self.value_read(*function),
                        "left".to_string(),
                        "right".to_string(),
                    ],
                );
                format!(
                    "match ({}, {}) {{ (Ok(left), Ok(right)) => {emitted}, _ => Err({}JetAbsent) }}",
                    self.value_read(*left),
                    self.value_read(*right),
                    self.config.root_prefix
                )
            }
            MirSemanticOp::ClosureMethod {
                receiver,
                args,
                call,
            } => {
                let emitted = self.prelude_receiver_call(function, *call, *receiver, args, result);
                result
                    .and_then(|value| {
                        self.native_int_result(&emitted, self.value_type(function, value))
                    })
                    .unwrap_or(emitted)
            }
            MirSemanticOp::HostBorrowCallback { callable, params } => {
                self.host_callback(function, *callable, params, true)
            }
            MirSemanticOp::NumericMethod { call, receiver } => {
                self.prelude_values(*call, &[*receiver])
            }
            MirSemanticOp::NumericBinaryMethod {
                call,
                receiver,
                argument,
            } => self.prelude_values(*call, &[*receiver, *argument]),
            MirSemanticOp::OverflowOption {
                call,
                left,
                right,
                location,
            } => self.overflow_option(*call, *left, *right, location),
            MirSemanticOp::HandleMethod {
                call,
                receiver,
                args,
                frame_schedule,
                frame_schedule_derivation,
            } => {
                let emitted = self.prelude_handle_method(
                    function,
                    *call,
                    *receiver,
                    args,
                    frame_schedule.as_ref(),
                    frame_schedule_derivation.as_ref(),
                    location.as_ref(),
                );
                if result.is_some_and(|value| is_allocator_result_type(self.value_type(function, value))) {
                    return format!(
                        "{emitted}.map(|value| std::clone::Clone::clone(&*value))"
                    );
                }
                result
                    .and_then(|value| {
                        self.native_int_result(&emitted, self.value_type(function, value))
                    })
                    .unwrap_or(emitted)
            }
            MirSemanticOp::PluginInvoke {
                call,
                handle,
                export_name,
                signature,
                args,
            } => self.plugin_invoke(
                function,
                *call,
                *handle,
                export_name,
                signature,
                args,
                result,
            ),
            MirSemanticOp::HttpRouterRegister {
                call,
                receiver,
                path,
                handler,
                method,
                handler_param_names,
                contract_json,
                location,
            } => self.http_router_register(
                function,
                *call,
                *receiver,
                *path,
                *handler,
                *method,
                handler_param_names,
                contract_json,
                *location,
            ),
            MirSemanticOp::TextPatternMatch {
                call,
                subject,
                parts,
            } => {
                let pattern = self.text_pattern(parts);
                let subject = self.value_read(*subject);
                let scan = self.prelude_call_args(*call, &[subject, pattern]);
                self.pattern_match_result(function, result, &scan)
            }
            MirSemanticOp::BinaryPatternMatch {
                call,
                subject,
                parts,
            } => {
                let pattern = self.binary_pattern(parts);
                let subject = format!("({}).bytes", self.value_slot_reference(*subject, false));
                let scan = self.prelude_call_args(*call, &[subject, pattern]);
                self.pattern_match_result(function, result, &scan)
            }
            MirSemanticOp::CoreClosureCall {
                call,
                kind,
                values,
                closure,
                site,
                label,
            } => self.core_closure_call(
                function,
                *call,
                kind.clone(),
                values,
                *closure,
                *site,
                label,
            ),
            MirSemanticOp::TaskGroup { call, kind, tasks } => {
                let args = tasks
                    .iter()
                    .map(|value| self.value_move(*value))
                    .collect::<Vec<_>>();
                let emitted = self.prelude_call_args(*call, &args);
                format!("{emitted} /* task_group={kind:?} */")
            }
            MirSemanticOp::Select { call, kind, values } => {
                let args = values
                    .iter()
                    .map(|value| self.value_move(*value))
                    .collect::<Vec<_>>();
                let emitted = self.prelude_call_args(*call, &args);
                format!("{emitted} /* select={kind:?} */")
            }
            MirSemanticOp::PolicyFunction { policy, values } => format!(
                "{}({})",
                self.value_read(*policy),
                values
                    .iter()
                    .map(|value| self.value_move(*value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirSemanticOp::InterruptFunction { interrupt, values } => format!(
                "{}({})",
                self.value_read(*interrupt),
                values
                    .iter()
                    .map(|value| self.value_move(*value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirSemanticOp::CarrierFact {
                call,
                receiver,
                field,
                notes: _,
            } => self.carrier_fact(*call, *receiver, *field),
            MirSemanticOp::GcEdit {
                call,
                root,
                edges,
                edit,
                index,
                kind,
                site,
            } => self.gc_edit(*call, *root, edges, *edit, *index, *kind, *site),
            MirSemanticOp::TypedTextInterp {
                call,
                kind,
                literals,
                holes,
            } => self.typed_text_interp(function, *call, *kind, literals, holes),
            MirSemanticOp::CCallback {
                call,
                callback,
                lambda,
            } => self.c_callback(function, *call, *callback, *lambda),
            MirSemanticOp::HostCall { call, args } => {
                self.prelude_host_call(function, *call, args, location.as_ref())
            }
        }
    }

    fn carrier_fact(
        &self,
        call: MirPreludeCallId,
        receiver: MirValueId,
        field: MirFieldId,
    ) -> String {
        format!(
            "{}(&({}), |__jet_report| __jet_report.{}.clone())",
            self.prelude_symbol(call),
            self.value_read(receiver),
            self.field_name(field)
        )
    }

    fn typed_sql_binding(&self, function: &MirFunction, value: MirValueId) -> String {
        let db_value = format!("{}jet_std::DBValue", self.config.root_prefix);
        let ty = self.value_type(function, value);
        if ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_DB_VALUE) {
            return self.value_read(value);
        }
        match ty.kind() {
            MirTypeKind::Int => format!(
                "{db_value}::Int({}jet_std::jet_int_owned_to_i64(&({})).expect(\"SQL Int exceeds i64 wire range\"))",
                self.config.root_prefix,
                self.value_read(value)
            ),
            MirTypeKind::IntN { .. } => {
                format!("{db_value}::Int(({}) as i64)", self.value_read(value))
            }
            MirTypeKind::Float | MirTypeKind::Float32 => {
                format!("{db_value}::Float(({}) as f64)", self.value_read(value))
            }
            MirTypeKind::Bool => format!("{db_value}::Bool({})", self.value_read(value)),
            MirTypeKind::String => {
                format!("{db_value}::Text({})", self.value_read(value))
            }
            MirTypeKind::List(inner)
                if matches!(
                    inner.kind(),
                    MirTypeKind::IntN {
                        signed: false,
                        bits: 8
                    }
                ) =>
            {
                format!("{db_value}::Blob({})", self.value_read(value))
            }
            MirTypeKind::Char
            | MirTypeKind::List(_)
            | MirTypeKind::Map { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. }
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::Apply { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Tuple(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::InlineRange { .. }
            | MirTypeKind::Tagged { .. }
            | MirTypeKind::Quantity { .. }
            | MirTypeKind::Union(_)
            | MirTypeKind::Measure(_) => {
                format!("{db_value}::Text(({}).jet_show())", self.value_read(value))
            }
        }
    }

    fn typed_text_interp(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        kind: jet_foundation::Syntax::TypedHeadKind,
        literals: &[String],
        holes: &[MirValueId],
    ) -> String {
        let literal_array = format!(
            "[{}]",
            literals
                .iter()
                .map(|literal| format!("{literal:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let hole_array = match kind {
            jet_foundation::Syntax::TypedHeadKind::SQL => holes
                .iter()
                .map(|value| self.typed_sql_binding(function, *value))
                .collect::<Vec<_>>(),
            jet_foundation::Syntax::TypedHeadKind::HTML => holes
                .iter()
                .map(|value| {
                    let rendered = self.value_read(*value);
                    if self.value_type(function, *value).nominal_name()
                        == Some(jet_foundation::Syntax::TYPE_HTML)
                    {
                        format!("({rendered})")
                    } else {
                        format!("({rendered}).jet_show()")
                    }
                })
                .collect::<Vec<_>>(),
            jet_foundation::Syntax::TypedHeadKind::Sh
            | jet_foundation::Syntax::TypedHeadKind::URL
            | jet_foundation::Syntax::TypedHeadKind::Path
            | jet_foundation::Syntax::TypedHeadKind::DateTime => holes
                .iter()
                .map(|value| format!("({}).jet_show()", self.value_read(*value)))
                .collect::<Vec<_>>(),
        };
        let hole_expr = format!("vec![{}]", hole_array.join(", "));
        let args = match kind {
            jet_foundation::Syntax::TypedHeadKind::HTML => {
                let trusted = holes
                    .iter()
                    .map(|value| {
                        (self.value_type(function, *value).nominal_name()
                            == Some(jet_foundation::Syntax::TYPE_HTML))
                        .to_string()
                    })
                    .collect::<Vec<_>>();
                vec![
                    literal_array,
                    hole_expr,
                    format!("&[{}]", trusted.join(", ")),
                ]
            }
            jet_foundation::Syntax::TypedHeadKind::SQL
            | jet_foundation::Syntax::TypedHeadKind::Sh
            | jet_foundation::Syntax::TypedHeadKind::URL
            | jet_foundation::Syntax::TypedHeadKind::Path
            | jet_foundation::Syntax::TypedHeadKind::DateTime => {
                vec![literal_array, hole_expr]
            }
        };
        self.validate_prelude_count(self.prelude_row(call), args.len());
        let args = args.join(", ");
        let symbol = if matches!(kind, jet_foundation::Syntax::TypedHeadKind::SQL) {
            format!(
                "{}::<{}jet_std::DBValue>",
                self.prelude_symbol(call),
                self.config.root_prefix
            )
        } else {
            self.prelude_symbol(call)
        };
        format!("{symbol}({args})")
    }
    fn gc_edit(
        &self,
        call: MirPreludeCallId,
        root: MirValueId,
        edges: &[MirValueId],
        edit: MirValueId,
        index: Option<MirValueId>,
        kind: MirGcEditKind,
        site: jet_foundation::MIR::MirGcEditSiteId,
    ) -> String {
        let root = format!("&({})", self.value_read(root));
        let edge_values = format!(
            "&[{}]",
            edges
                .iter()
                .map(|value| self.value_read(*value))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let edit = self.value_move(edit);
        let args = match kind {
            MirGcEditKind::Clear | MirGcEditKind::Pop | MirGcEditKind::Plain => {
                if index.is_some() || !edges.is_empty() {
                    panic!("MIR GC edit carries unused index or edge operands");
                }
                vec![root, edit]
            }
            MirGcEditKind::RemoveIndex => {
                if !edges.is_empty() {
                    panic!("MIR GC remove edit carries edge operands");
                }
                let index = index.unwrap_or_else(|| panic!("MIR GC remove edit has no index"));
                vec![root, self.value_read(index), edit]
            }
            MirGcEditKind::InsertIndex => {
                let index = index.unwrap_or_else(|| panic!("MIR GC insert edit has no index"));
                vec![root, self.value_read(index), edge_values, edit]
            }
            MirGcEditKind::Prepend | MirGcEditKind::Additive => {
                if index.is_some() {
                    panic!("MIR GC edge edit carries an unused index");
                }
                vec![root, edge_values, edit]
            }
            MirGcEditKind::EdgeSlot => {
                if index.is_some() {
                    panic!("MIR GC edge-slot edit carries an unused index");
                }
                vec![root, edge_values, edit, format!("{}u64", site.0)]
            }
        };
        self.prelude_call_args_exact(call, &args)
    }

    fn native_callback_adapter(
        &self,
        function: &MirFunction,
        closure: MirValueId,
        params: usize,
    ) -> String {
        let send_fn = matches!(
            self.value_type(function, closure).kind(),
            MirTypeKind::SendFn { .. }
        );
        let callback = self.value_move(closure);
        match params {
            0 => {
                let invocation = if send_fn || self.history_runtime_metadata_enabled() {
                    "(__jet_host_callback)()".to_string()
                } else {
                    self.fn_value_call("__jet_host_callback.clone()".to_string(), &[])
                };
                format!(
                    "{{ let __jet_host_callback = {callback}; move || {invocation} }}"
                )
            }
            1 => {
                let argument = if self
                    .callable_borrow_mask(self.value_type(function, closure))
                    .first()
                    .copied()
                    .unwrap_or(false)
                {
                    "&__jet_host_arg"
                } else {
                    "__jet_host_arg"
                };
                let invocation = if send_fn || self.history_runtime_metadata_enabled() {
                    format!("(__jet_host_callback)({argument})")
                } else {
                    self.fn_value_call(
                        "__jet_host_callback.clone()".to_string(),
                        &[argument.to_string()],
                    )
                };
                format!(
                    "{{ let __jet_host_callback = {callback}; move |__jet_host_arg| {invocation} }}"
                )
            }
            _ => panic!("native callback adapter supports at most one parameter"),
        }
    }

    fn core_closure_call(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        kind: MirCoreClosureKind,
        values: &[MirValueId],
        closure: Option<MirValueId>,
        _site: jet_foundation::MIR::MirSiteId,
        _label: &str,
    ) -> String {
        let preview_source = match &kind {
            MirCoreClosureKind::UiPreview {
                source_file,
                source_start_line,
                source_start_column,
                source_end_line,
                source_end_column,
                build_id,
                revision,
                ..
            } => Some((
                source_file.clone(),
                *source_start_line,
                *source_start_column,
                *source_end_line,
                *source_end_column,
                build_id.clone(),
                revision.clone(),
            )),
            _ => None,
        };
        let mut args = Vec::new();
        match kind {
            MirCoreClosureKind::Spawn => match values {
                [site, label] => {
                    let closure = closure.unwrap_or_else(|| {
                        panic!("MIR CoreClosureCall Spawn has no checked closure value")
                    });
                    let site = format!(
                        "usize::try_from({}).unwrap_or_else(|_| {}jet_arithmetic_stop(\"<mir>\", 0, \"spawn site is negative or exceeds usize range\"))",
                        self.native_int_argument(self.value_move(*site), None),
                        self.config.root_prefix,
                    );
                    args.push(site);
                    args.push(format!("&({})", self.value_move(*label)));
                    args.push(format!(
                        "{{ let __jet_callback = {}; move || (__jet_callback)() }}",
                        self.value_move(closure),
                    ));
                }
                [group, site, label] => {
                    let closure = closure.unwrap_or_else(|| {
                        panic!("MIR CoreClosureCall Spawn has no checked closure value")
                    });
                    let site = format!(
                        "usize::try_from({}).unwrap_or_else(|_| {}jet_arithmetic_stop(\"<mir>\", 0, \"spawn site is negative or exceeds usize range\"))",
                        self.native_int_argument(self.value_move(*site), None),
                        self.config.root_prefix,
                    );
                    args.push(self.borrowed_value_reference(function, *group, MirAccess::Read));
                    args.push(site);
                    args.push(format!("&({})", self.value_move(*label)));
                    args.push(format!(
                        "{{ let __jet_callback = {}; move || (__jet_callback)() }}",
                        self.value_move(closure),
                    ));
                }
                _ => panic!(
                    "MIR CoreClosureCall Spawn has {} value operands; expected two or three",
                    values.len()
                ),
            },
            MirCoreClosureKind::Realtime => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall Realtime has no checked closure value")
                });
                if values.len() != 2 {
                    panic!(
                        "MIR CoreClosureCall Realtime has {} value operands; expected rate and frames",
                        values.len()
                    );
                }
                args.push(self.value_move(values[0]));
                args.push(self.value_move(values[1]));
                args.push(self.value_move(closure));
            }
            MirCoreClosureKind::Serve => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall Serve has no checked closure value")
                });
                if values.len() != 1 {
                    panic!(
                        "MIR CoreClosureCall Serve has {} value operands; expected one address",
                        values.len()
                    );
                }
                args.push(format!("&({})", self.value_read(values[0])));
                args.push(self.value_move(closure));
            }
            MirCoreClosureKind::OnInterrupt => {
                if closure.is_some() || values.len() != 1 {
                    panic!(
                        "MIR CoreClosureCall OnInterrupt requires one callback value and no closure field"
                    );
                }
                args.push(self.value_move(values[0]));
            }
            MirCoreClosureKind::Guard => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall Guard has no checked closure value")
                });
                if !values.is_empty() {
                    panic!(
                        "MIR CoreClosureCall Guard has {} value operands; expected none",
                        values.len()
                    );
                }
                args.push(self.value_move(closure));
            }
            MirCoreClosureKind::OnCommit => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall OnCommit has no checked closure value")
                });
                if values.len() != 1 {
                    panic!(
                        "MIR CoreClosureCall OnCommit has {} value operands; expected one handle",
                        values.len()
                    );
                }
                args.push(format!("&({})", self.value_read(values[0])));
                args.push(self.value_move(closure));
            }
            MirCoreClosureKind::OnRollback => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall OnRollback has no checked closure value")
                });
                if values.len() != 1 {
                    panic!(
                        "MIR CoreClosureCall OnRollback has {} value operands; expected one handle",
                        values.len()
                    );
                }
                args.push(format!("&({})", self.value_read(values[0])));
                args.push(self.value_move(closure));
            }
            MirCoreClosureKind::ReactiveDerived => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall ReactiveDerived has no checked closure value")
                });
                if !values.is_empty() {
                    panic!(
                        "MIR CoreClosureCall ReactiveDerived has {} value operands; expected none",
                        values.len()
                    );
                }
                args.push(self.native_callback_adapter(function, closure, 0));
            }
            MirCoreClosureKind::ReactiveEffect => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall ReactiveEffect has no checked closure value")
                });
                if !values.is_empty() {
                    panic!(
                        "MIR CoreClosureCall ReactiveEffect has {} value operands; expected none",
                        values.len()
                    );
                }
                args.push(self.native_callback_adapter(function, closure, 0));
            }
            MirCoreClosureKind::UiMount => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall UiMount has no checked closure value")
                });
                if !values.is_empty() {
                    panic!(
                        "MIR CoreClosureCall UiMount has {} value operands; expected none",
                        values.len()
                    );
                }
                args.push(self.native_callback_adapter(function, closure, 0));
            }
            MirCoreClosureKind::UiPreview { .. } => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall UiPreview has no checked closure value")
                });
                if values.len() != 2 {
                    panic!(
                        "MIR CoreClosureCall UiPreview has {} value operands; expected name and viewport",
                        values.len()
                    );
                }
                args.push(format!("&({})", self.value_read(values[0])));
                args.push(self.value_move(values[1]));
                args.push(self.native_callback_adapter(function, closure, 0));
            }
            MirCoreClosureKind::UiAction => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall UiAction has no checked closure value")
                });
                if values.len() != 3 {
                    panic!(
                        "MIR CoreClosureCall UiAction has {} value operands; expected display, shortcut, accessible label",
                        values.len()
                    );
                }
                args.push(format!("&({})", self.value_read(values[0])));
                args.push(self.value_move(values[1]));
                args.push(self.value_move(values[2]));
                args.push(self.native_callback_adapter(function, closure, 0));
            }
            MirCoreClosureKind::UiTextInputOnDrop => {
                let closure = closure.unwrap_or_else(|| {
                    panic!("MIR CoreClosureCall UiTextInputOnDrop has no checked closure value")
                });
                if values.len() != 2 {
                    panic!(
                        "MIR CoreClosureCall UiTextInputOnDrop has {} value operands; expected state and IME mode",
                        values.len()
                    );
                }
                args.push(format!("&({})", self.value_read(values[0])));
                args.push(self.value_move(values[1]));
                args.push(self.native_callback_adapter(function, closure, 1));
            }
        }
        let emitted = self.prelude_call_args_exact(call, &args);
        if let Some((
            source_file,
            source_start_line,
            source_start_column,
            source_end_line,
            source_end_column,
            build_id,
            revision,
        )) = preview_source
        {
            let source_id = source_file.clone();
            format!(
                "{}jet_ui_preview_attach_compiler_source({emitted}, {source_id:?}, {source_file:?}, {build_id:?}, {revision:?}, {source_start_line}u32, {source_start_column}u32, {source_end_line}u32, {source_end_column}u32)",
                self.config.root_prefix,
            )
        } else {
            emitted
        }
    }

    fn prelude_values(&self, call: MirPreludeCallId, values: &[MirValueId]) -> String {
        let row = self.prelude_row(call);
        self.validate_prelude_count(row, values.len());
        let symbol = self.prelude_symbol(call);
        let adapter = self.exact_prelude_adapter(&symbol);
        let args = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if row.signature.borrow_mask[index]
                    || adapter.is_some_and(|(_, positions)| positions.contains(&index))
                {
                    self.value_slot_reference(*value, false)
                } else {
                    self.value_read(*value)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let symbol = adapter
            .map(|(name, _)| format!("{}{name}", self.config.root_prefix))
            .unwrap_or(symbol);
        format!("{symbol}({args})")
    }

    fn prelude_call_args(&self, call: MirPreludeCallId, args: &[String]) -> String {
        let row = self.prelude_row(call);
        self.validate_prelude_count(row, args.len());
        format!("{}({})", self.prelude_symbol(call), args.join(", "))
    }

    fn list_aggregate_builder(
        &self,
        function: &MirFunction,
        receiver: MirValueId,
        result: Option<MirValueId>,
    ) -> String {
        if self.value_type(function, receiver).list_element().is_none() {
            panic!("MIR list aggregate route has a non-list receiver");
        }
        let result = result.unwrap_or_else(|| panic!("MIR list aggregate route has no result"));
        let result_type = self.value_type(function, result);
        let result_type = result_type
            .option_inner()
            .or_else(|| result_type.result_parts().map(|(ok, _)| ok))
            .unwrap_or(result_type);
        match result_type.kind() {
            MirTypeKind::Tuple(fields) => {
                if fields.len() != 2
                    || fields[0].0 != "min"
                    || fields[1].0 != "max"
                {
                    panic!("MIR list aggregate result is not the checked min/max shape");
                }
                "|min, max| (min, max)".to_string()
            }
            MirTypeKind::Apply { .. } => {
                let type_id = result_type
                    .nominal_id()
                    .unwrap_or_else(|| panic!("MIR list aggregate result has no nominal type"));
                let definition = self.type_def(type_id);
                let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
                    panic!("MIR list aggregate result is not a record");
                };
                if fields.len() != 2
                    || !fields.iter().any(|field| field.name == "min")
                    || !fields.iter().any(|field| field.name == "max")
                {
                    panic!("MIR list aggregate result is not the checked min/max shape");
                }
                let min = fields
                    .iter()
                    .find(|field| field.name == "min")
                    .expect("checked min field");
                let max = fields
                    .iter()
                    .find(|field| field.name == "max")
                    .expect("checked max field");
                format!(
                    "|min, max| {} {{ {}: min, {}: max }}",
                    self.rust_type(result_type),
                    self.field_name(min.id),
                    self.field_name(max.id),
                )
            }
            _ => panic!("MIR list aggregate result is not the checked min/max shape"),
        }
    }
    fn map_aggregate_builder(
        &self,
        function: &MirFunction,
        result: Option<MirValueId>,
    ) -> String {
        let result = result.unwrap_or_else(|| panic!("MIR map aggregate route has no result"));
        let element = self
            .value_type(function, result)
            .list_element()
            .unwrap_or_else(|| panic!("MIR map aggregate route has a non-list result"));
        let MirTypeKind::Tuple(fields) = element.kind() else {
            panic!("MIR map aggregate result is not a named tuple");
        };
        if fields.len() != 2 || fields[0].0 != "key" || fields[1].0 != "value" {
            panic!("MIR map aggregate result is not the checked key/value shape");
        }
        "|key, value| (key, value)".to_string()
    }


    fn prelude_values_with_receiver(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        receiver: MirValueId,
        receiver_place: Option<MirPlaceId>,
        args: &[MirValueId],
        result: Option<MirValueId>,
    ) -> String {
        let row = self.prelude_row(call);
        self.validate_prelude_count(row, args.len() + 1);
        if row.signature.borrow_mask.len() != args.len() + 1 {
            panic!(
                "MIR builtin route {:?} has inconsistent receiver/argument borrow metadata",
                call
            );
        }
        // `String.bytes()` on an owned rvalue consumes the String, matching
        // the checked ownership fact instead of cloning it through the
        // generic Prelude call path.
        if row.family == MirPreludeFamily::BuiltinMethod
            && row.module == "core.builtin"
            && row.member == "bytes"
            && receiver_place.is_none()
            && row.signature.borrow_mask.first() == Some(&false)
        {
            return format!("({}).into_bytes()", self.value_move(receiver));
        }
        let receiver_value = receiver;
        let receiver = if let Some(place) = receiver_place {
            if !row.signature.borrow_mask[0] {
                panic!(
                    "MIR builtin mutating receiver route {:?} is not borrowed",
                    call
                );
            }
            self.place_reference(function, place, MirAccess::Write)
        } else if row.signature.borrow_mask[0] {
            self.borrowed_value_reference(function, receiver, MirAccess::Read)
        } else {
            self.value_move(receiver)
        };
        if row.abi == MirPreludeAbi::Aggregate
            && row.module == "core.list"
            && row.member == "min_max"
        {
            assert!(args.is_empty(), "MIR List.min_max route has unexpected arguments");
            let symbol = if self
                .value_type(function, receiver_value)
                .list_element()
                .is_some_and(|element| matches!(element.kind(), MirTypeKind::Float))
            {
                format!("{}jet_list_min_max_float", self.config.root_prefix)
            } else {
                self.prelude_symbol(call)
            };
            return format!(
                "{}({receiver}, {})",
                symbol,
                self.list_aggregate_builder(function, receiver_value, result),
            );
        }
        if row.abi == MirPreludeAbi::Aggregate
            && row.module == "core.map"
            && row.member == "to_list"
        {
            assert!(args.is_empty(), "MIR Map.to_list route has unexpected arguments");
            return format!(
                "{}({receiver}, {})",
                self.prelude_symbol(call),
                self.map_aggregate_builder(function, result),
            );
        }
        let mut values = vec![receiver];
        values.extend(args.iter().enumerate().map(|(index, value_id)| {
            let value = if row.signature.borrow_mask[index + 1] {
                self.borrowed_value_reference(function, *value_id, MirAccess::Read)
            } else {
                self.value_read(*value_id)
            };
            if (row.module == "core.builtin"
                && matches!(
                    row.member.as_str(),
                    "iter_take" | "iter_skip" | "iter_step_by" | "iter_chunks"
                        | "iter_windows" | "iter_repeat" | "iter_cycle" | "iter_drop_last"
                        | "map_top_n"
                )
                || row.symbol.name() == "jet_list_insert")
                && index == 0
                && matches!(
                    self.value_type(function, *value_id).kind(),
                    MirTypeKind::Int
                )
            {
                self.native_int_argument(value, None)
            } else {
                value
            }
        }));
        format!("{}({})", self.prelude_symbol(call), values.join(", "))
    }

    fn prelude_receiver_is_mutating(&self, row: &MirPreludeCall) -> bool {
        row.family == MirPreludeFamily::ClosureMethod
            && matches!(
                row.symbol.name(),
                "jet_edit_disjoint"
                    | "jet_list_sort_by"
                    | "jet_list_sort_by_desc"
                    | "jet_list_try_sort_by"
                    | "jet_list_try_sort_by_desc"
                    | "jet_list_sort_by_compare"
                    | "jet_list_update_first"
            )
    }

    fn closure_callback_params(
        &self,
        function: &MirFunction,
        value: MirValueId,
    ) -> Option<Vec<MirType>> {
        match self.value_type(function, value).kind() {
            MirTypeKind::Fn(signature) => Some(signature.params.clone()),
            MirTypeKind::SendFn { params, .. } => Some(params.clone()),
            _ => None,
        }
    }

    fn closure_route_uses_borrowed_callback(
        &self,
        row: &MirPreludeCall,
        function: &MirFunction,
        value: MirValueId,
    ) -> bool {
        if row.family != MirPreludeFamily::ClosureMethod
            || self.closure_callback_params(function, value).is_none()
        {
            return false;
        }
        match row.module.as_str() {
            "core.list" | "core.iter" | "core.map" | "core.view" | "core.option" | "core.bag" => {
                true
            }
            _ => false,
        }
    }

    fn host_borrow_callback_value(
        &self,
        function: &MirFunction,
        value: MirValueId,
    ) -> Option<String> {
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if instruction.result != Some(value) {
                    return None;
                }
                match &instruction.operation {
                    MirOperation::Semantic(MirSemanticOp::HostBorrowCallback {
                        callable,
                        params,
                    }) => Some(self.host_callback(function, *callable, params, true)),
                    _ => None,
                }
            })
    }

    fn mutable_receiver_reference(&self, function: &MirFunction, value: MirValueId) -> String {
        let instruction = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(value))
            .unwrap_or_else(|| panic!("MIR mutating receiver value {:?} has no definition", value));
        match &instruction.operation {
            MirOperation::ReadPlace(place) => {
                self.place_reference(function, *place, MirAccess::Write)
            }
            MirOperation::Parameter { .. } | MirOperation::Capture { .. } => {
                format!("&mut ({})", self.parameter_place(function, value, true))
            }
            _ => self.value_slot_reference(value, true),
        }
    }

    fn closure_call_argument(
        &self,
        function: &MirFunction,
        row: &MirPreludeCall,
        index: usize,
        arg: &MirCallArg,
    ) -> String {
        let borrowed = row.signature.borrow_mask[index + 1];
        if let Some(callback) = self.host_borrow_callback_value(function, arg.value) {
            return callback;
        }
        if self.closure_route_uses_borrowed_callback(row, function, arg.value) {
            if !matches!(
                self.value_definition(function, arg.value),
                Some(MirOperation::Closure { .. })
            ) {
                panic!(
                    "MIR borrowed callback argument {:?} is not an owned closure value",
                    arg.value
                );
            }
            let params = self
                .closure_callback_params(function, arg.value)
                .unwrap_or_else(|| panic!("MIR borrowed callback has no callable signature"));
            let borrow_inputs = !(row.module == "core.iter"
                && matches!(row.member.as_str(), "zip" | "zip_strict" | "zip_pad"));
            return self.host_callback(function, arg.value, &params, borrow_inputs);
        }
        self.call_arg_for_function(function, arg, borrowed)
    }

    fn prelude_receiver_call(
        &self,
        function: &MirFunction,
        call: MirPreludeCallId,
        receiver: MirValueId,
        args: &[MirCallArg],
        result: Option<MirValueId>,
    ) -> String {
        let row = self.prelude_row(call);
        self.validate_prelude_count(row, args.len() + 1);
        if row.signature.borrow_mask.len() != args.len() + 1 {
            panic!(
                "MIR closure route {:?} has inconsistent receiver/argument borrow metadata",
                call
            );
        }
        let receiver_value = receiver;
        let receiver = if self.prelude_receiver_is_mutating(row) {
            if !row.signature.borrow_mask[0] {
                panic!("MIR mutating closure route {:?} is not borrowed", call);
            }
            self.mutable_receiver_reference(function, receiver)
        } else if row.signature.borrow_mask[0] {
            self.borrowed_value_reference(function, receiver, MirAccess::Read)
        } else {
            self.value_move(receiver)
        };
        if row.abi == MirPreludeAbi::Aggregate
            && row.module == "core.list"
            && row.member == "min_max_by"
        {
            assert_eq!(args.len(), 1, "MIR List.min_max_by route expects one callback");
            let callback = self.closure_call_argument(function, row, 0, &args[0]);
            return format!(
                "{}({receiver}, {callback}, {})",
                self.prelude_symbol(call),
                self.list_aggregate_builder(function, receiver_value, result),
            );
        }
        let mut values = vec![receiver];
        values.extend(
            args.iter()
                .enumerate()
                .map(|(index, arg)| self.closure_call_argument(function, row, index, arg)),
        );
        let emitted = format!("{}({})", self.prelude_symbol(call), values.join(", "));
        if row.symbol.name() == "jet_list_each_ref" {
            format!("({emitted})?")
        } else {
            emitted
        }
    }

    fn local_read(&self, function: &MirFunction, id: jet_foundation::MIR::MirLocalId) -> String {
        let local = function
            .locals
            .iter()
            .find(|local| local.id == id)
            .unwrap_or_else(|| panic!("MIR local ID {:?} has no row", id));
        self.place_read(function, local.place)
    }

    fn place_read(&self, function: &MirFunction, id: MirPlaceId) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR place ID {:?} has no row", id));
        if place.projections.is_empty() {
            match &place.base {
                MirPlaceBase::Local(local) => {
                    if self.local_uninit_fixed_type(function, *local).is_some() {
                        return format!(
                            "{}.as_ref().expect(\"MIR local\").read_array()",
                            self.local_storage(function, *local)
                        );
                    }
                    if self.local_allocator_view(function, *local) {
                        return format!(
                            "&mut **{}.as_mut().expect(\"MIR local\")",
                            self.local_storage(function, *local)
                        );
                    }
                    let local_row = function
                        .locals
                        .iter()
                        .find(|candidate| candidate.id == *local)
                        .unwrap_or_else(|| panic!("MIR local ID {:?} has no row", local));
                    if is_allocator_result_type(&local_row.ty) {
                        return format!(
                            "{}.as_ref().expect(\"MIR local\").clone()",
                            self.local_storage(function, *local)
                        );
                    }
                    if self.local_direct_move_storage(function, *local) {
                        return format!("{}.clone()", self.local_storage(function, *local));
                    }
                    return format!(
                        "{}.as_ref().expect(\"MIR local\").clone()",
                        self.local_storage(function, *local)
                    );
                }
                MirPlaceBase::Temporary(value) => return self.value_read(*value),
                MirPlaceBase::Static(name) => {
                    return format!("({}{}).get()", self.config.root_prefix, mangle_path(name));
                }
                MirPlaceBase::Parameter(_) | MirPlaceBase::Capture(_) => {}
            }
        }

        let value = self.place_base(function, &place.base, false, &place.projections);
        if matches!(
            place.projections.last(),
            Some(MirProjection::Field { field, .. }) if self.boxed_field(*field)
        ) {
            format!("({value}).as_ref().clone()")
        } else {
            format!("({value}).clone()")
        }
    }

    fn move_parameter_name(&self, function: &MirFunction, value: MirValueId) -> String {
        let parameter = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if instruction.result != Some(value) {
                    return None;
                }
                if let MirOperation::Parameter { index, .. } = &instruction.operation {
                    function.params.iter().find(|param| param.index == *index)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("MIR parameter value {:?} has no parameter row", value));
        if parameter.access != MirAccess::Move {
            panic!("MIR MovePlace parameter is not an owned move parameter");
        }
        if self.is_receiver(function, parameter) {
            return "self".to_string();
        }
        mangle(&parameter.name)
    }

    fn move_capture_name(&self, function: &MirFunction, value: MirValueId) -> String {
        let slot = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| {
                if instruction.result != Some(value) {
                    return None;
                }
                if let MirOperation::Capture { slot } = &instruction.operation {
                    Some(*slot)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("MIR capture value {:?} has no capture row", value));
        self.capture_param(function, slot);
        if !self.capture_move_required(function, slot) {
            panic!("MIR MovePlace capture has no checked move fact");
        }
        let name = self.capture_param_name(slot);
        name
    }

    fn move_place_for_capture(&self, function: &MirFunction, id: MirPlaceId) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR capture move place ID {:?} has no row", id));
        if matches!(&place.base, MirPlaceBase::Static(_)) {
            panic!("MIR capture move place {:?} is a static place", id);
        }
        if place.projections.is_empty() {
            self.move_base(function, &place.base)
        } else {
            self.move_projected_place(function, place)
        }
    }

    fn move_base(&self, function: &MirFunction, base: &MirPlaceBase) -> String {
        match base {
            MirPlaceBase::Local(local) => {
                if self.local_uninit_fixed_type(function, *local).is_some() {
                    return format!(
                        "{}.take().expect(\"MIR local\").into_array()",
                        self.local_storage(function, *local)
                    );
                }
                if self.local_direct_move_storage(function, *local) {
                    self.local_storage(function, *local)
                } else {
                    format!(
                        "{}.take().expect(\"MIR local\")",
                        self.local_storage(function, *local)
                    )
                }
            }
            MirPlaceBase::Temporary(value) => self.value_move(*value),
            MirPlaceBase::Parameter(value) => self.move_parameter_name(function, *value),
            MirPlaceBase::Capture(value) => self.move_capture_name(function, *value),
            MirPlaceBase::Static(name) => {
                format!("({}{}).take()", self.config.root_prefix, mangle_path(name))
            }
        }
    }

    fn move_index_projection(&self, kind: MirIndexKind, base: String, index: MirValueId) -> String {
        let index = self.value_read(index);
        match kind {
            MirIndexKind::List | MirIndexKind::FixedListProof => format!(
                "jet_list_remove_slot(&mut ({base}), {index}).unwrap_or_else(|_| jet_panic(\"<core.collections>\", 0, \"MIR owning list index move failed\"))"
            ),
            MirIndexKind::Map => format!(
                "jet_map_pop_kernel(&mut ({base}), &({index})).unwrap_or_else(|_| jet_panic(\"<core.collections>\", 0, \"MIR owning map index move failed\"))"
            ),
            MirIndexKind::Pool => format!(
                "({base}).remove({index}).unwrap_or_else(|_| jet_panic(\"<core.pool>\", 0, \"MIR owning pool index move failed\"))"
            ),
            MirIndexKind::Lane => {
                panic!("MIR MovePlace lane index has no canonical owning collection primitive")
            }
        }
    }
    fn move_projection_chain(
        &self,
        mut expression: String,
        projections: &[MirProjection],
    ) -> String {
        if projections.is_empty() {
            return expression;
        }
        match &projections[0] {
            MirProjection::Field { field, .. } => {
                expression = format!("({expression}).{}", self.field_name(*field));
                self.move_projection_chain(expression, &projections[1..])
            }
            MirProjection::Index { kind, index, .. } => {
                let moved = self.move_index_projection(*kind, expression, *index);
                if projections.len() == 1 {
                    moved
                } else {
                    format!(
                        "{{ let mut __jet_move_value = {moved}; {} }}",
                        self.move_projection_chain(
                            "__jet_move_value".to_string(),
                            &projections[1..]
                        )
                    )
                }
            }
            MirProjection::Deref { .. } => {
                panic!("MIR MovePlace dereference has no canonical owned representation")
            }
        }
    }

    fn move_projected_place(&self, function: &MirFunction, place: &MirPlace) -> String {
        if matches!(&place.base, MirPlaceBase::Static(_)) {
            panic!("MIR MovePlace projected static is rejected by the canonical front end");
        }
        if let Some(index) = place
            .projections
            .iter()
            .position(|projection| matches!(projection, MirProjection::Index { .. }))
        {
            // Remove from the live collection, leaving its other entries and
            // the enclosing record in place. Sema already checked the move.
            let owner = self.place_base(function, &place.base, true, &place.projections[..index]);
            return self.move_projection_chain(owner, &place.projections[index..]);
        }
        // Native bindings retain Rust's partial-move/drop state. Taking an
        // Option root here would drop siblings at the end of this expression.
        let owner = match &place.base {
            MirPlaceBase::Local(local) => self.local_storage(function, *local),
            MirPlaceBase::Parameter(value) => self.move_parameter_name(function, *value),
            MirPlaceBase::Capture(value) => self.move_capture_name(function, *value),
            MirPlaceBase::Temporary(value) => value_slot(*value),
            MirPlaceBase::Static(_) => unreachable!(),
        };
        self.move_projection_chain(owner, &place.projections)
    }

    fn move_place(&self, function: &MirFunction, id: MirPlaceId) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR move place ID {:?} has no row", id));
        if place.access != MirAccess::Move {
            panic!("MIR MovePlace {:?} does not have move access", id);
        }
        if place.projections.is_empty() {
            return self.move_base(function, &place.base);
        }
        self.move_projected_place(function, place)
    }

    fn place_lvalue(&self, function: &MirFunction, id: MirPlaceId) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR place ID {:?} has no row", id));
        if place.projections.is_empty() {
            match &place.base {
                MirPlaceBase::Local(local) if self.local_allocator_view(function, *local) => {
                    return format!(
                        "**{}.as_mut().expect(\"MIR local\")",
                        self.local_storage(function, *local)
                    );
                }
                MirPlaceBase::Local(local) => return self.local_storage(function, *local),
                MirPlaceBase::Temporary(value) => return value_slot(*value),
                MirPlaceBase::Parameter(_) | MirPlaceBase::Capture(_) => {
                    return self.place_base(function, &place.base, true, &place.projections)
                }
                MirPlaceBase::Static(name) => {
                    return format!("{}{}", self.config.root_prefix, mangle_path(name))
                }
            }
        }
        self.place_base(function, &place.base, true, &place.projections)
    }

    fn place_base(
        &self,
        function: &MirFunction,
        base: &MirPlaceBase,
        mutable: bool,
        projections: &[MirProjection],
    ) -> String {
        let mut expression = match base {
            MirPlaceBase::Local(local) => {
                if self.local_uninit_fixed_type(function, *local).is_some() {
                    if mutable {
                        format!(
                            "*{}.as_mut().expect(\"MIR local\").as_array_mut()",
                            self.local_storage(function, *local)
                        )
                    } else {
                        format!(
                            "*{}.as_ref().expect(\"MIR local\").as_array()",
                            self.local_storage(function, *local)
                        )
                    }
                } else if self.local_allocator_view(function, *local) {
                    if mutable {
                        format!(
                            "**{}.as_mut().expect(\"MIR local\")",
                            self.local_storage(function, *local)
                        )
                    } else {
                        format!(
                            "**{}.as_ref().expect(\"MIR local\")",
                            self.local_storage(function, *local)
                        )
                    }
                } else if self.local_direct_move_storage(function, *local) {
                    self.local_storage(function, *local)
                } else if mutable {
                    format!(
                        "*{}.as_mut().expect(\"MIR local\")",
                        self.local_storage(function, *local)
                    )
                } else {
                    format!(
                        "*{}.as_ref().expect(\"MIR local\")",
                        self.local_storage(function, *local)
                    )
                }
            }
            MirPlaceBase::Parameter(value) => self.parameter_place(function, *value, mutable),
            MirPlaceBase::Capture(value) => self.capture_place(function, *value, mutable),
            MirPlaceBase::Temporary(value) => {
                if self.temporary_direct_move_storage(function, *value) {
                    value_slot(*value)
                } else {
                    format!("*({})", self.value_slot_reference(*value, mutable))
                }
            }
            MirPlaceBase::Static(name) => {
                if mutable {
                    panic!("MIR mutable static projection has no canonical owning representation");
                }
                format!("({}{}).get()", self.config.root_prefix, mangle_path(name))
            }
        };

        for projection in projections {
            match projection {
                MirProjection::Field { field, .. } => {
                    expression = format!("({expression}).{}", self.field_name(*field))
                }
                MirProjection::Index {
                    kind,
                    index,
                    call,
                    location,
                    context,
                    ..
                } => {
                    let index = if *kind == MirIndexKind::Map && !mutable {
                        format!("&({})", self.value_read(*index))
                    } else if *kind == MirIndexKind::Map {
                        self.value_read(*index)
                    } else {
                        self.index_operand(function, *index, *location)
                    };
                    let symbol = match (kind, mutable) {
                        (MirIndexKind::List | MirIndexKind::FixedListProof, false) => {
                            "jet_index_vec_ref"
                        }
                        (MirIndexKind::List | MirIndexKind::FixedListProof, true) => {
                            "jet_index_vec_mut"
                        }
                        (MirIndexKind::Map, false) => "jet_index_map_ref",
                        (MirIndexKind::Map, true) => "jet_index_map_mut",
                        (MirIndexKind::Pool, false) => "jet_std::jet_pool_get_ref",
                        (MirIndexKind::Pool, true) => "jet_std::jet_pool_get_mut",
                        (MirIndexKind::Lane, false) => "jet_std::jet_native_lane_ref",
                        (MirIndexKind::Lane, true) => "jet_std::jet_native_lane_mut",
                    };
                    let borrow = if mutable { "&mut" } else { "&" };
                    let args = self.index_arguments(
                        *call,
                        format!("{borrow} ({expression})"),
                        index,
                        *location,
                        context.as_ref(),
                    );
                    expression = format!(
                        "*{}{}({})",
                        self.config.root_prefix,
                        symbol,
                        args.join(", ")
                    );
                }
                MirProjection::Deref { .. } => expression = format!("*({expression})"),
            }
        }
        expression
    }

    fn capture_place(&self, function: &MirFunction, value: MirValueId, mutable: bool) -> String {
        let slot = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| match instruction.operation {
                MirOperation::Capture { slot } if instruction.result == Some(value) => Some(slot),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "MIR capture place value {:?} has no Capture instruction",
                    value
                )
            });
        let capture = self.capture_param(function, slot);
        let name = self.capture_param_name(slot);
        match capture.access {
            MirAccess::Read | MirAccess::Write => {
                if mutable && matches!(capture.access, MirAccess::Read) {
                    panic!("MIR read capture place is not writable");
                }
                format!("*{name}")
            }
            MirAccess::Move => name,
        }
    }

    fn place_reference(&self, function: &MirFunction, id: MirPlaceId, access: MirAccess) -> String {
        let place = function
            .places
            .iter()
            .find(|place| place.id == id)
            .unwrap_or_else(|| panic!("MIR place ID {:?} has no row", id));
        let mutable = matches!(access, MirAccess::Write);
        if place.projections.is_empty() {
            match &place.base {
                MirPlaceBase::Local(local) => {
                    if self.local_uninit_fixed_type(function, *local).is_some() {
                        return if mutable {
                            format!(
                                "{}.as_mut().expect(\"MIR local\").as_array_mut()",
                                self.local_storage(function, *local)
                            )
                        } else {
                            format!(
                                "{}.as_ref().expect(\"MIR local\").as_array()",
                                self.local_storage(function, *local)
                            )
                        };
                    }
                    if self.local_direct_move_storage(function, *local) {
                        return if mutable {
                            format!("&mut {}", self.local_storage(function, *local))
                        } else {
                            format!("&{}", self.local_storage(function, *local))
                        };
                    }
                    return if mutable {
                        format!(
                            "{}.as_mut().expect(\"MIR local\")",
                            self.local_storage(function, *local)
                        )
                    } else {
                        format!(
                            "{}.as_ref().expect(\"MIR local\")",
                            self.local_storage(function, *local)
                        )
                    };
                }
                MirPlaceBase::Temporary(value) => {
                    return self.value_slot_reference(*value, mutable);
                }
                MirPlaceBase::Parameter(_) | MirPlaceBase::Capture(_) | MirPlaceBase::Static(_) => {
                }
            }
        }
        let value = self.place_base(function, &place.base, mutable, &place.projections);
        if mutable {
            format!("&mut ({value})")
        } else {
            format!("&({value})")
        }
    }
    fn parameter_place(&self, function: &MirFunction, value: MirValueId, mutable: bool) -> String {
        let parameter = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| match &instruction.operation {
                MirOperation::Parameter { index, .. } if instruction.result == Some(value) => {
                    function
                        .params
                        .iter()
                        .find(|param| param.index == *index)
                        .map(|param| {
                            if self.is_receiver(function, param) {
                                // `&self`/`&mut self` deref like any borrowed slot,
                                // whatever the owner's representation.
                                ("self".to_string(), param.access, false, false)
                            } else {
                                (
                                    mangle(&param.name),
                                    param.access,
                                    !self.parameter_borrowed(param),
                                    false,
                                )
                            }
                        })
                }
                MirOperation::Capture { slot } if instruction.result == Some(value) => {
                    let capture = self.capture_param(function, *slot);
                    Some((
                        self.capture_param_name(*slot),
                        capture.access,
                        self.is_scalar(&capture.ty),
                        true,
                    ))
                }
                MirOperation::Parameter { .. }
                | MirOperation::Capture { .. }
                | MirOperation::WritePlace { .. }
                | MirOperation::Global { .. }
                | MirOperation::Phi { .. }
                | MirOperation::ReadPlace(_)
                | MirOperation::MovePlace { .. }
                | MirOperation::InitializeUninit { .. }
                | MirOperation::Copy { .. }
                | MirOperation::Move { .. }
                | MirOperation::Constant(_)
                | MirOperation::Unary { .. }
                | MirOperation::Binary { .. }
                | MirOperation::BuildString { .. }
                | MirOperation::BuildList { .. }
                | MirOperation::BuildMap { .. }
                | MirOperation::EnumIs { .. }
                | MirOperation::EnumPayload { .. }
                | MirOperation::OptionIsSome { .. }
                | MirOperation::OptionValue { .. }
                | MirOperation::ResultIsOk { .. }
                | MirOperation::ResultValue { .. }
                | MirOperation::PatternCapture { .. }
                | MirOperation::PatternMatched { .. }
                | MirOperation::ProjectMembers { .. }
                | MirOperation::Index { .. }
                | MirOperation::Slice { .. }
                | MirOperation::Range { .. }
                | MirOperation::Field { .. }
                | MirOperation::Struct { .. }
                | MirOperation::Enum { .. }
                | MirOperation::Tuple { .. }
                | MirOperation::Present { .. }
                | MirOperation::Convert { .. }
                | MirOperation::Absent
                | MirOperation::ResultOk { .. }
                | MirOperation::ResultErr { .. }
                | MirOperation::Call { .. }
                | MirOperation::IndirectCall { .. }
                | MirOperation::Closure { .. }
                | MirOperation::PtrFromAddr { .. }
                | MirOperation::Deref { .. }
                | MirOperation::RawAddressOf { .. }
                | MirOperation::AddressOf { .. }
                | MirOperation::CoreCall { .. }
                | MirOperation::AttachTag { .. }
                | MirOperation::Todo { .. }
                | MirOperation::Never { .. }
                | MirOperation::Semantic(_)
                | MirOperation::LoopRangeInit { .. }
                | MirOperation::LoopRangeHasNext { .. }
                | MirOperation::LoopRangeValue { .. }
                | MirOperation::LoopRangeAdvance { .. }
                | MirOperation::LoopIterInit { .. }
                | MirOperation::LoopIterHasNext { .. }
                | MirOperation::LoopIterValue { .. }
                | MirOperation::LoopIterAdvance { .. }
                | MirOperation::ScopeEnter { .. }
                | MirOperation::ScopeExit { .. }
                | MirOperation::Drop { .. } => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "MIR parameter value {:?} has no Parameter or Capture instruction",
                    value
                )
            });
        let (name, access, by_value, capture) = parameter;
        if mutable {
            if access == MirAccess::Move {
                return name;
            }
            if !matches!(access, MirAccess::Write) {
                panic!("MIR place is not writable");
            }
            return format!("*{name}");
        }
        match access {
            MirAccess::Read if !capture && by_value => name,
            MirAccess::Read | MirAccess::Write => format!("*{name}"),
            MirAccess::Move => name,
        }
    }

    fn value_slot_reference(&self, value: MirValueId, mutable: bool) -> String {
        if let Some((base, _)) = self.history_binding(value) {
            let function = self.function_row(
                self.history_current_function
                    .get()
                    .expect("current native function"),
            );
            let value = self.place_base(function, &base, mutable, &[]);
            return if mutable {
                format!("&mut ({value})")
            } else {
                format!("&({value})")
            };
        }
        if self
            .history_current_function
            .get()
            .is_some_and(|id| self.temporary_direct_move_storage(self.function_row(id), value))
        {
            return if mutable {
                format!("&mut {}", value_slot(value))
            } else {
                format!("&{}", value_slot(value))
            };
        }
        if mutable {
            format!("{}.as_mut().expect(\"MIR value\")", value_slot(value))
        } else {
            format!("{}.as_ref().expect(\"MIR value\")", value_slot(value))
        }
    }

    fn value_read(&self, value: MirValueId) -> String {
        format!("({}).clone()", self.value_slot_reference(value, false))
    }

    fn value_copy(&self, value: MirValueId) -> String {
        self.value_read(value)
    }

    fn value_move(&self, value: MirValueId) -> String {
        if let Some((base, access)) = self.history_binding(value) {
            if access != MirAccess::Move {
                return self.value_read(value);
            }
            return self.move_base(
                self.function_row(
                    self.history_current_function
                        .get()
                        .expect("current native function"),
                ),
                &base,
            );
        }
        if self
            .history_current_function
            .get()
            .is_some_and(|id| {
                allocator_view_inner(self.value_type(self.function_row(id), value)).is_some()
            })
        {
            return format!("{}.take().expect(\"MIR value\")", value_slot(value));
        }

        if self
            .history_current_function
            .get()
            .is_some_and(|id| self.temporary_direct_move_storage(self.function_row(id), value))
        {
            return value_slot(value);
        }
        format!("{}.take().expect(\"MIR value\")", value_slot(value))
    }

    fn value_borrow_mut(&self, value: MirValueId) -> String {
        self.value_slot_reference(value, true)
    }

    fn typed_struct_constant(
        &self,
        nominal: &MirNominalRef,
        args: &[MirType],
        type_name: &str,
        fields: &[(String, MirConstant)],
    ) -> Option<String> {
        let definition = self.program.types.iter().find(|definition| {
            definition.id == nominal.id
                || definition.key == nominal.name
                || definition.name == nominal.name
        })?;
        if type_name != nominal.name.as_str()
            && type_name != definition.key.as_str()
            && type_name != definition.name.as_str()
        {
            return None;
        }
        let MirTypeDefKind::Struct {
            fields: declared, ..
        } = &definition.kind
        else {
            return None;
        };
        let bindings = definition
            .generic_params
            .iter()
            .zip(args)
            .map(|(param, ty)| (param.name.clone(), ty.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut rendered = Vec::new();
        for field in declared.iter().filter(|field| !field.computed) {
            let (_, value) = fields.iter().find(|(name, _)| name == &field.name)?;
            let field_ty = Self::history_capture_type(&field.ty, &bindings);
            let mut value = self.constant_for_type(value, &field_ty);
            if definition
                .boxed_edges
                .iter()
                .any(|edge| edge == &field.name)
            {
                value = format!("Box::new({value})");
            }
            rendered.push(format!("{}: {value}", self.field_name(field.id)));
        }
        let mut constructor = self.rust_apply_type(nominal, args);
        if !args.is_empty() {
            let start = constructor
                .find('<')
                .expect("checked generic struct constructor has type arguments");
            constructor.insert_str(start, "::");
        }
        Some(format!(
            "{} {{ {} }}",
            constructor,
            rendered.join(", "),
        ))
    }

    fn constant_for_type(&self, constant: &MirConstant, ty: &MirType) -> String {
        match ty.kind() {
            MirTypeKind::Int => match constant {
                MirConstant::Int { value, .. } => {
                    format!("jet_foundation::Numeric::JetInt::from_i64({value})")
                }
                MirConstant::BigInt(value) => format!(
                    "{}jet_std::jet_int_owned_from_str({value:?}).expect(\"MIR Int\")",
                    self.config.root_prefix
                ),
                _ => self.constant(constant),
            },
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => self.constant_for_type(constant, base),
            MirTypeKind::IntN { signed, bits } => {
                let value = match constant {
                    MirConstant::Int { value, .. } => value.to_string(),
                    MirConstant::BigInt(value) => value.clone(),
                    _ => return self.constant(constant),
                };
                format!("{value}{}{bits}", if *signed { "i" } else { "u" })
            }
            MirTypeKind::Option(inner) => match constant {
                MirConstant::Present(value) => {
                    format!("Ok({})", self.constant_for_type(value, inner))
                }
                _ => self.constant(constant),
            },
            MirTypeKind::Result { ok, err } => match constant {
                MirConstant::Present(value) => {
                    format!("Ok({})", self.constant_for_type(value, ok))
                }
                MirConstant::Failed(MirConstReport::Told(value)) => {
                    format!("Err({})", self.constant_for_type(value, err))
                }
                _ => self.constant(constant),
            },
            MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                let MirConstant::List(values) = constant else {
                    return self.constant(constant);
                };
                let values = values
                    .iter()
                    .map(|value| self.constant_for_type(value, inner))
                    .collect::<Vec<_>>()
                    .join(", ");
                if matches!(ty.kind(), MirTypeKind::FixedList { .. }) {
                    format!("[{values}]")
                } else {
                    format!("vec![{values}]")
                }
            }
            MirTypeKind::Map { key, value } => {
                let MirConstant::Map(entries) = constant else {
                    return self.constant(constant);
                };
                let pairs = entries
                    .iter()
                    .map(|(entry_key, entry_value)| {
                        let key = match entry_key {
                            MirConstKey::Int(literal) => self.constant_for_type(
                                &MirConstant::Int {
                                    value: *literal,
                                    width: None,
                                    spelling: None,
                                },
                                key,
                            ),
                            _ => self.const_key(entry_key),
                        };
                        format!("({key}, {})", self.constant_for_type(entry_value, value))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{pairs}].into_iter().collect::<{}>()", self.rust_type(ty))
            }
            MirTypeKind::Apply { name, args } => match constant {
                MirConstant::Struct { type_name, fields } => self
                    .typed_struct_constant(name, args, type_name, fields)
                    .unwrap_or_else(|| self.constant(constant)),
                _ => self.constant(constant),
            },
            _ => self.constant(constant),
        }
    }

    fn constant(&self, constant: &MirConstant) -> String {
        match constant {
            MirConstant::Int {
                value,
                width,
                spelling,
            } => spelling.clone().unwrap_or_else(|| match width {
                Some((signed, bits)) => {
                    format!("{}{}{}", value, if *signed { "i" } else { "u" }, bits)
                }
                None => value.to_string(),
            }),
            MirConstant::Float {
                value,
                f32,
                spelling,
            } => spelling.clone().unwrap_or_else(|| {
                if value.is_nan() {
                    if *f32 {
                        "f32::NAN".to_string()
                    } else {
                        "f64::NAN".to_string()
                    }
                } else if value.is_infinite() {
                    if value.is_sign_negative() {
                        if *f32 {
                            "f32::NEG_INFINITY".to_string()
                        } else {
                            "f64::NEG_INFINITY".to_string()
                        }
                    } else if *f32 {
                        "f32::INFINITY".to_string()
                    } else {
                        "f64::INFINITY".to_string()
                    }
                } else if *f32 {
                    format!("{value:?}f32")
                } else {
                    format!("{value:?}f64")
                }
            }),
            MirConstant::Bool(value) => value.to_string(),
            MirConstant::Char(value) => format!("{value:?}"),
            MirConstant::String(value) => {
                if self.is_core_layer() {
                    format!("{value:?}")
                } else {
                    format!("{value:?}.to_string()")
                }
            }
            MirConstant::Bytes(value) => format!(
                "vec![{}]",
                value
                    .iter()
                    .map(|byte| byte.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirConstant::Unit => "()".to_string(),
            MirConstant::BigInt(value) => format!(
                "{}jet_std::jet_int_owned_from_str({:?}).expect(\"MIR Int\")",
                self.config.root_prefix, value
            ),
            MirConstant::List(values) => format!(
                "vec![{}]",
                values
                    .iter()
                    .map(|value| self.constant(value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirConstant::Map(values) => format!(
                "[{pairs}].into_iter().collect::<{root}JetMap<_,_>>()",
                root = self.config.root_prefix,
                pairs = values
                    .iter()
                    .map(|(key, value)| {
                        format!("({}, {})", self.const_key(key), self.constant(value))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirConstant::Struct { type_name, fields }
                if type_name == jet_foundation::Syntax::TYPE_FRACTION
                    && !self.types_by_name.contains_key(type_name) =>
            {
                let part = |name: &str| {
                    let value = &fields
                        .iter()
                        .find(|(field, _)| field == name)
                        .unwrap_or_else(|| panic!("MIR Fraction has no {name}"))
                        .1;
                    if !matches!(value, MirConstant::Int { .. } | MirConstant::BigInt(_)) {
                        panic!("MIR Fraction {name} is not an Int");
                    }
                    self.constant_for_type(value, &MirType::from_kind(MirTypeKind::Int))
                };
                format!(
                    "{}jet_fraction_from_parts({}, {})",
                    self.config.root_prefix,
                    part("numerator"),
                    part("denominator"),
                )
            }
            MirConstant::Struct { type_name, fields } => {
                let native = crate::Codegen::core_rust_type_name(type_name).is_some()
                    || crate::Codegen::root_prelude_rust_type_name(type_name).is_some()
                    || crate::Codegen::compute_handle_rust_type(type_name).is_some();
                format!(
                    "{} {{ {} }}",
                    self.nominal_name(type_name),
                    fields
                        .iter()
                        .map(|(name, value)| {
                            let name = if native { name.clone() } else { mangle(name) };
                            format!("{name}: {}", self.constant(value))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            MirConstant::Enum {
                type_name,
                variant,
                args,
            } => {
                let native = crate::Codegen::core_rust_type_name(type_name).is_some()
                    || crate::Codegen::root_prelude_rust_type_name(type_name).is_some()
                    || crate::Codegen::compute_handle_rust_type(type_name).is_some();
                let variant_name = if native {
                    variant.clone()
                } else {
                    mangle(variant)
                };
                let variant = format!("{}::{variant_name}", self.nominal_name(type_name));
                if args.is_empty() {
                    variant
                } else if args.iter().all(|(field, _)| field.is_some()) {
                    let fields = args
                        .iter()
                        .map(|(field, value)| {
                            format!(
                                "{}: {}",
                                if native {
                                    field.as_deref().expect("checked enum field").to_string()
                                } else {
                                    mangle(field.as_deref().expect("checked enum field"))
                                },
                                self.constant(value)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{variant} {{ {fields} }}")
                } else if args.iter().all(|(field, _)| field.is_none()) {
                    let values = args
                        .iter()
                        .map(|(_, value)| self.constant(value))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{variant}({values})")
                } else {
                    panic!("MIR enum constant mixes named and positional payloads");
                }
            }
            MirConstant::Present(value) => format!("Ok({})", self.constant(value)),
            MirConstant::Failed(report) => match report {
                MirConstReport::Clean(_) => format!("Err({}JetAbsent)", self.config.root_prefix),
                MirConstReport::Told(value) => format!("Err({})", self.constant(value)),
            },
        }
    }

    fn const_key(&self, key: &MirConstKey) -> String {
        match key {
            MirConstKey::Int(value) => value.to_string(),
            MirConstKey::String(value) => {
                if self.is_core_layer() {
                    format!("{value:?}")
                } else {
                    format!("{value:?}.to_string()")
                }
            }
            MirConstKey::Bool(value) => value.to_string(),
            MirConstKey::Char(value) => format!("{value:?}"),
            MirConstKey::Tuple(values) => format!(
                "({})",
                values
                    .iter()
                    .map(|(_, value)| self.const_key(value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MirConstKey::Struct { type_name, fields } => {
                let native = crate::Codegen::core_rust_type_name(type_name).is_some()
                    || crate::Codegen::root_prelude_rust_type_name(type_name).is_some()
                    || crate::Codegen::compute_handle_rust_type(type_name).is_some();
                format!(
                    "{} {{ {} }}",
                    self.nominal_name(type_name),
                    fields
                        .iter()
                        .map(|(name, value)| {
                            let name = if native { name.clone() } else { mangle(name) };
                            format!("{name}: {}", self.const_key(value))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            MirConstKey::Enum { type_name, variant } => {
                let native = crate::Codegen::core_rust_type_name(type_name).is_some()
                    || crate::Codegen::root_prelude_rust_type_name(type_name).is_some()
                    || crate::Codegen::compute_handle_rust_type(type_name).is_some();
                let variant = if native {
                    variant.clone()
                } else {
                    mangle(variant)
                };
                format!("{}::{variant}", self.nominal_name(type_name))
            }
        }
    }
}

fn value_slot(value: MirValueId) -> String {
    format!("__jet_v_{}", value.0)
}

fn local_slot(local: jet_foundation::MIR::MirLocalId) -> String {
    format!("__jet_l_{}", local.0)
}

fn fixed_inline_backing_name(value: MirValueId) -> String {
    format!("__jet_fixed_backing_v{}", value.0)
}
