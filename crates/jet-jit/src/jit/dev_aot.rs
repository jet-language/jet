//! Cranelift dev builds (#3953): `jet build` without `--release` compiles the
//! checked MIR program with the resident JIT's own Cranelift lowering, and the
//! build appends the resulting image to a copy of the prebuilt `jet-aot-rt`
//! runner. Release builds stay on rustc/LLVM.
//!
//! The image is the warm tier-cache module (captured machine code, relocations
//! against the host-function table, entry rail, compile-time strings) plus the
//! remaining compile-time runtime state the code depends on: the program
//! source used by runtime reports, the checked type descriptors, and the
//! closure-target rows with their execution identity. A program whose
//! compile-time state cannot be carried this way is refused with a reason and
//! the build falls back to rustc.
//!
//! The runner loads the image through the same `define_function_bytes` path as
//! a warm `jet run`: code is written into writable pages, then those pages are
//! flipped to read+execute by `finalize_definitions`; no page is ever mapped
//! writable and executable at once.
//!
//! File layout of a built executable:
//! `runner bytes | body | sha256(body) (32) | body length (u64 LE) | TRAILER_MAGIC (8)`.

use cranelift_module::FuncId;
use jet_foundation::{
    JitBackend::RunOutcome,
    MIR::{
        MirArtifactBuildMode, MirArtifactId, MirArtifactIdentity, MirArtifactKind,
        MirArtifactTarget, MirExecutionIdentity, MirFunctionId, MirProgram, MirProgramIdentity,
        MirSourceMapIdentity,
    },
    Shape::ShapeFieldNames,
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use super::define_batch::{with_code_store, CompiledCodeStore};

use super::api_debug::{cranelift_host_supported, resident_jit_safe_program_detail};
use super::resident::{
    ensure_resident_module, fresh_runtime_with_allocator_cap, program_allocator_cap_bytes,
    resident_invoke, resident_teardown,
};
use super::runtime_host::{
    catch_jit_panic, JitRuntime, RuntimeFieldDescriptor, RuntimeIntegerWidth,
    RuntimeTypeDescriptor, RuntimeValueAbi, RuntimeValueKind, RuntimeVariantDescriptor,
};
use super::tier_cache::{
    self, read_bool, read_i64, read_str, read_u32, read_u64, write_str, WarmModule,
};
use super::types_meta::mir_fn_name;
use super::tiers::{plan_mir_tiers, Tier};
use super::RESIDENT_RUNTIME;

/// Body format version. The runner accepts only this version.
const DEV_AOT_FORMAT: u32 = 1;
/// Last eight bytes of every Cranelift dev-build executable.
const TRAILER_MAGIC: &[u8; 8] = b"JETAOT01";
const TRAILER_LEN: u64 = 32 + 8 + 8;
/// Length of the trailer that ends every image and dev-build executable:
/// `sha256(body) | body length | TRAILER_MAGIC`. It names the image by content.
pub const DEV_AOT_TRAILER_LEN: usize = TRAILER_LEN as usize;

/// Whether `trailer` (the last `DEV_AOT_TRAILER_LEN` bytes of a file) ends a
/// Cranelift dev-build image.
pub fn is_dev_aot_trailer(trailer: &[u8]) -> bool {
    trailer.len() == DEV_AOT_TRAILER_LEN && &trailer[40..48] == TRAILER_MAGIC
}

/// Compile a checked MIR executable into a dev-build image: the bytes the
/// build appends to a copy of the `jet-aot-rt` runner (body and trailer).
///
/// `runtime_id` names the exact runtime build the image was compiled against;
/// the runner refuses an image whose id differs from its own. `code_store`
/// keeps compiled functions across builds and programs. `Err` is a
/// capability refusal with the reason; the caller falls back to rustc.
pub fn compile_dev_aot_image(
    program: &MirProgram,
    artifact: MirArtifactId,
    runtime_id: &str,
    code_store: Option<Arc<dyn CompiledCodeStore>>,
) -> Result<Vec<u8>, String> {
    if !cranelift_host_supported() {
        return Err("Cranelift dev builds support only x86_64 hosts".to_string());
    }
    let unsafe_detail = resident_jit_safe_program_detail(program);
    if !unsafe_detail.is_empty() {
        return Err(format!(
            "the program leaves the resident Cranelift subset: {unsafe_detail}"
        ));
    }
    // An interpreter-tier function deopts into the checked MIR program at run
    // time; the image carries machine code only.
    let plan = plan_mir_tiers(program, artifact);
    if let Some(row) = plan.rows.iter().find(|row| row.tier != Tier::Native) {
        return Err(format!(
            "function `{}` runs on the interpreter tier",
            row.function_name
        ));
    }
    if plan.whole_program_deopt || !plan.deopt.is_empty() {
        return Err("the program deopts to the interpreter tier".to_string());
    }
    // `catch_jit_panic` serializes resident work on the run lock and turns a
    // lowering panic into a refusal.
    let mut body = crate::on_compiler_stack(|| {
        let body = catch_jit_panic("dev build compile", || {
            with_code_store(code_store, || compile_image_body(program, artifact, runtime_id))
        });
        tier_cache::abort_capture();
        resident_teardown();
        body
    })?;
    let digest = Sha256::digest(&body);
    let body_len = body.len() as u64;
    body.extend_from_slice(&digest);
    body.extend_from_slice(&body_len.to_le_bytes());
    body.extend_from_slice(TRAILER_MAGIC);
    Ok(body)
}

fn compile_image_body(
    program: &MirProgram,
    artifact: MirArtifactId,
    runtime_id: &str,
) -> Result<Vec<u8>, String> {
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    resident_teardown();
    let allocator_cap = program_allocator_cap_bytes(program);
    let policy = ReleaseDevtoolsPolicy::development();
    RESIDENT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(fresh_runtime_with_allocator_cap(policy.clone(), allocator_cap))
    });
    tier_cache::begin_capture();
    ensure_resident_module(program, artifact, &policy)?;
    if crate::CLI::cli_user_run_target().is_some() {
        return Err("the entry decodes its arguments through the CLI adapter".to_string());
    }
    let state = RESIDENT_RUNTIME.with(|slot| {
        let guard = slot.borrow();
        let runtime = guard
            .as_ref()
            .ok_or_else(|| "resident runtime missing".to_string())?;
        encode_runtime_state(runtime)
    })?;
    let warm = tier_cache::encode_capture(&[], artifact, true).map_err(str::to_string)?;

    let mut body = Vec::with_capacity(warm.len() + state.len() + 128);
    body.extend_from_slice(&DEV_AOT_FORMAT.to_le_bytes());
    write_str(&mut body, runtime_id);
    write_opt_u64(&mut body, allocator_cap);
    body.extend_from_slice(&(warm.len() as u64).to_le_bytes());
    body.extend_from_slice(&warm);
    body.extend_from_slice(&state);
    Ok(body)
}

/// Compile-time runtime state that the warm module does not carry.
fn encode_runtime_state(runtime: &JitRuntime) -> Result<Vec<u8>, String> {
    if !runtime.native_interface_methods.is_empty() || !runtime.native_callable_methods.is_empty()
    {
        return Err("the program binds native interface or callable methods".to_string());
    }
    let mut out = Vec::new();
    write_str(&mut out, &runtime.source_file);
    write_str(&mut out, &runtime.source_text);
    let mut ids = runtime.type_descriptors.keys().copied().collect::<Vec<_>>();
    ids.sort_unstable();
    out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    for id in ids {
        write_descriptor(&mut out, &runtime.type_descriptors[&id])?;
    }
    match &runtime.jit_closure_execution_identity {
        None => out.push(0),
        Some(execution) => {
            out.push(1);
            write_execution(&mut out, execution);
            let mut targets = runtime.jit_closure_targets.values().collect::<Vec<_>>();
            targets.sort_unstable_by_key(|target| target.function.0);
            out.extend_from_slice(&(targets.len() as u32).to_le_bytes());
            for target in targets {
                if target.execution != *execution {
                    return Err("closure targets belong to more than one execution image".to_string());
                }
                out.extend_from_slice(&target.function.0.to_le_bytes());
                out.extend_from_slice(&(target.capture_type_ids.len() as u32).to_le_bytes());
                for (type_id, owned) in target.capture_type_ids.iter().zip(&target.capture_owned) {
                    out.extend_from_slice(&type_id.to_le_bytes());
                    out.push(u8::from(*owned));
                }
            }
        }
    }
    Ok(out)
}

/// The one argument a bare runner (no appended image) answers: its runtime
/// build identity, so `jet build` can refuse a runner from another build
/// before packaging an image the runner would reject.
pub const RUNTIME_ID_PROBE: &str = "--jet-aot-runtime-id";

/// Entry point of the `jet-aot-rt` runner: verify the image appended to this
/// executable, run it, write its output, and return the process exit code.
pub fn run_dev_aot_executable(runtime_id: &str) -> i32 {
    let args = std::env::args().collect::<Vec<_>>();
    let body = match read_appended_body() {
        Ok(Some(body)) => body,
        Ok(None) if args.len() == 2 && args[1] == RUNTIME_ID_PROBE => {
            println!("{runtime_id}");
            return jet_foundation::ExitCodes::OK;
        }
        Ok(None) => {
            eprintln!(
                "error: this runner carries no Jet dev-build image; build a program with `jet build` and run its output"
            );
            return jet_foundation::ExitCodes::USAGE;
        }
        Err(error) => {
            eprintln!("error: {error}");
            return jet_foundation::ExitCodes::ICE;
        }
    };
    crate::set_program_owns_streams();
    // Decode on the worker so no decoded runtime carrier crosses threads.
    let outcome = crate::with_program_args(&args, || {
        crate::on_compiler_stack(move || {
            let image = decode_image(&body, runtime_id)?;
            drop(body);
            crate::reset_one_shot_core_state();
            let outcome = run_image(image);
            crate::release_one_shot_core_state();
            Ok::<_, String>(outcome)
        })
    });
    match outcome {
        Ok(outcome) => report_outcome(outcome),
        Err(error) => {
            eprintln!("error: {error}");
            jet_foundation::ExitCodes::ICE
        }
    }
}

/// The verified image body appended to this executable; `None` when the file
/// carries no image at all (the bare runner).
fn read_appended_body() -> Result<Option<Vec<u8>>, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("couldn't locate this executable: {error}"))?;
    let mut file = std::fs::File::open(&exe)
        .map_err(|error| format!("couldn't open {}: {error}", exe.display()))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("couldn't read {}: {error}", exe.display()))?
        .len();
    if file_len < TRAILER_LEN {
        return Ok(None);
    }
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.seek(SeekFrom::Start(file_len - TRAILER_LEN))
        .and_then(|_| file.read_exact(&mut trailer))
        .map_err(|error| format!("couldn't read {}: {error}", exe.display()))?;
    if &trailer[40..48] != TRAILER_MAGIC {
        return Ok(None);
    }
    let body_len = u64::from_le_bytes(trailer[32..40].try_into().expect("8-byte length"));
    let corrupt = || {
        format!(
            "the Jet dev-build image in {} is corrupt; rebuild it with `jet build`",
            exe.display()
        )
    };
    if body_len > file_len - TRAILER_LEN {
        return Err(corrupt());
    }
    let mut body = vec![0u8; body_len as usize];
    file.seek(SeekFrom::Start(file_len - TRAILER_LEN - body_len))
        .and_then(|_| file.read_exact(&mut body))
        .map_err(|error| format!("couldn't read {}: {error}", exe.display()))?;
    if Sha256::digest(&body).as_slice() != &trailer[..32] {
        return Err(corrupt());
    }
    Ok(Some(body))
}

struct ClosureRow {
    function: MirFunctionId,
    capture_type_ids: Vec<u64>,
    capture_owned: Vec<bool>,
}

struct DevAotImage {
    allocator_cap: Option<u64>,
    warm: WarmModule,
    source_file: String,
    source_text: String,
    descriptors: Vec<RuntimeTypeDescriptor>,
    closures: Option<(MirExecutionIdentity, Vec<ClosureRow>)>,
}

fn decode_image(body: &[u8], runtime_id: &str) -> Result<DevAotImage, String> {
    let corrupt = || "the Jet dev-build image is corrupt; rebuild it with `jet build`".to_string();
    let mut i = 0usize;
    let format = read_u32(body, &mut i).ok_or_else(corrupt)?;
    if format != DEV_AOT_FORMAT {
        return Err(format!(
            "the Jet dev-build image has format {format}, this runner reads {DEV_AOT_FORMAT}; rebuild it with `jet build`"
        ));
    }
    let image_runtime = read_str(body, &mut i).ok_or_else(corrupt)?;
    if image_runtime != runtime_id {
        return Err(format!(
            "the Jet dev-build image was compiled for runtime {image_runtime}, but this runner is runtime {runtime_id}; install `jet` and `jet-aot-rt` from the same build, then rebuild with `jet build`"
        ));
    }
    decode_image_parts(body, &mut i).ok_or_else(corrupt)
}

fn decode_image_parts(body: &[u8], i: &mut usize) -> Option<DevAotImage> {
    let allocator_cap = read_opt_u64(body, i)?;
    let warm_len = usize::try_from(read_u64(body, i)?).ok()?;
    let warm_bytes = body.get(*i..i.checked_add(warm_len)?)?;
    *i += warm_len;
    let artifact = tier_cache::cached_artifact_id(warm_bytes)?;
    let warm = tier_cache::decode_module(warm_bytes, artifact).ok()?;
    let source_file = read_str(body, i)?;
    let source_text = read_str(body, i)?;
    let descriptor_count = read_u32(body, i)? as usize;
    let mut descriptors = Vec::with_capacity(descriptor_count.min(body.len()));
    for _ in 0..descriptor_count {
        descriptors.push(read_descriptor(body, i)?);
    }
    let closures = if read_bool(body, i)? {
        let execution = read_execution(body, i)?;
        let row_count = read_u32(body, i)? as usize;
        let mut rows = Vec::with_capacity(row_count.min(body.len()));
        for _ in 0..row_count {
            let function = MirFunctionId(read_u64(body, i)?);
            let capture_count = read_u32(body, i)? as usize;
            let mut capture_type_ids = Vec::with_capacity(capture_count.min(body.len()));
            let mut capture_owned = Vec::with_capacity(capture_count.min(body.len()));
            for _ in 0..capture_count {
                capture_type_ids.push(read_u64(body, i)?);
                capture_owned.push(read_bool(body, i)?);
            }
            rows.push(ClosureRow {
                function,
                capture_type_ids,
                capture_owned,
            });
        }
        Some((execution, rows))
    } else {
        None
    };
    (*i == body.len()).then_some(DevAotImage {
        allocator_cap,
        warm,
        source_file,
        source_text,
        descriptors,
        closures,
    })
}

fn run_image(image: DevAotImage) -> Result<RunOutcome, String> {
    let _loaded_mod_scope = crate::Mod::LoadScope;
    let DevAotImage {
        allocator_cap,
        warm,
        source_file,
        source_text,
        descriptors,
        closures,
    } = image;
    jet_rt::__gc::initialize_trace().map_err(|error| error.to_string())?;
    let mut runtime =
        fresh_runtime_with_allocator_cap(ReleaseDevtoolsPolicy::development(), allocator_cap);
    runtime.source_file = source_file;
    runtime.source_text = source_text;
    runtime.install_type_descriptors(descriptors);
    // `catch_jit_panic` holds the resident run lock for install and invoke.
    catch_jit_panic("dev build run", || {
        tier_cache::install_warm_module(warm, runtime, |module, ids, runtime| {
            let Some((execution, rows)) = closures else {
                return Ok(());
            };
            install_closure_targets(module, ids, runtime, execution, rows)
        })?;
        resident_invoke()
    })
}

fn install_closure_targets(
    module: &cranelift_jit::JITModule,
    ids: &HashMap<String, FuncId>,
    runtime: &mut JitRuntime,
    execution: MirExecutionIdentity,
    rows: Vec<ClosureRow>,
) -> Result<(), String> {
    runtime.set_jit_closure_execution_identity(execution.clone());
    for row in rows {
        let id = ids.get(&mir_fn_name(row.function)).ok_or_else(|| {
            format!("dev-build image has no code for closure target {:?}", row.function)
        })?;
        let pointer = module.get_finalized_function(*id);
        if pointer.is_null() {
            return Err(format!(
                "dev-build closure target {:?} has no finalized address",
                row.function
            ));
        }
        runtime.install_jit_closure_target(
            row.function,
            execution.clone(),
            pointer as i64,
            row.capture_type_ids,
            row.capture_owned,
        );
    }
    Ok(())
}

/// Write the run's output the way `jet run` does and pick the exit code.
fn report_outcome(outcome: Result<RunOutcome, String>) -> i32 {
    match outcome {
        Ok(RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        }) => {
            emit_output(&stdout, &stderr);
            exit_code
        }
        Ok(RunOutcome::Problems(diagnostics)) => {
            if let Some((stdout, what)) = diagnostics
                .iter()
                .find_map(jet_foundation::Diagnostics::Diagnostic::runtime_host_fault_parts)
            {
                emit_output(stdout, "");
                eprintln!(
                    "{}",
                    jet_foundation::Diagnostics::render_ice_report(what, "", false)
                );
                return jet_foundation::ExitCodes::ICE;
            }
            for diagnostic in &diagnostics {
                eprintln!("error[{}]: {}", diagnostic.code, diagnostic.what);
            }
            jet_foundation::ExitCodes::USER_ERROR
        }
        Err(error) => {
            eprintln!(
                "{}",
                jet_foundation::Diagnostics::render_ice_report(
                    &format!("the dev build could not run its compiled image: {error}"),
                    "",
                    false,
                )
            );
            jet_foundation::ExitCodes::ICE
        }
    }
}

fn emit_output(stdout: &str, stderr: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(stdout.as_bytes());
    let _ = out.flush();
    if !stderr.is_empty() {
        let _ = std::io::stderr().write_all(stderr.as_bytes());
    }
}

fn write_opt_u64(out: &mut Vec<u8>, value: Option<u64>) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn read_opt_u64(data: &[u8], i: &mut usize) -> Option<Option<u64>> {
    Some(if read_bool(data, i)? {
        Some(read_u64(data, i)?)
    } else {
        None
    })
}

fn write_opt_str(out: &mut Vec<u8>, value: Option<&str>) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        write_str(out, value);
    }
}

fn read_opt_str(data: &[u8], i: &mut usize) -> Option<Option<String>> {
    Some(if read_bool(data, i)? {
        Some(read_str(data, i)?)
    } else {
        None
    })
}

fn read_u8(data: &[u8], i: &mut usize) -> Option<u8> {
    let byte = *data.get(*i)?;
    *i += 1;
    Some(byte)
}

fn read_u16(data: &[u8], i: &mut usize) -> Option<u16> {
    let slice = data.get(*i..*i + 2)?;
    *i += 2;
    Some(u16::from_le_bytes(slice.try_into().ok()?))
}

fn read_i128(data: &[u8], i: &mut usize) -> Option<i128> {
    let slice = data.get(*i..*i + 16)?;
    *i += 16;
    Some(i128::from_le_bytes(slice.try_into().ok()?))
}

const VALUE_KINDS: [RuntimeValueKind; 18] = [
    RuntimeValueKind::Unit,
    RuntimeValueKind::Int,
    RuntimeValueKind::Float,
    RuntimeValueKind::Bool,
    RuntimeValueKind::Char,
    RuntimeValueKind::String,
    RuntimeValueKind::List,
    RuntimeValueKind::Map,
    RuntimeValueKind::Shared,
    RuntimeValueKind::Option,
    RuntimeValueKind::Result,
    RuntimeValueKind::Record,
    RuntimeValueKind::Enum,
    RuntimeValueKind::Closure,
    RuntimeValueKind::View,
    RuntimeValueKind::Iterator,
    RuntimeValueKind::Named,
    RuntimeValueKind::Handle,
];

const VALUE_ABIS: [RuntimeValueAbi; 7] = [
    RuntimeValueAbi::Unit,
    RuntimeValueAbi::Int,
    RuntimeValueAbi::Float,
    RuntimeValueAbi::Float32,
    RuntimeValueAbi::Bool,
    RuntimeValueAbi::Char,
    RuntimeValueAbi::Handle,
];

fn tag_of<T: PartialEq>(table: &[T], value: &T) -> u8 {
    table
        .iter()
        .position(|candidate| candidate == value)
        .expect("every variant is listed in its tag table") as u8
}

fn from_tag<T: Copy>(table: &[T], tag: u8) -> Option<T> {
    table.get(usize::from(tag)).copied()
}

fn write_descriptor(out: &mut Vec<u8>, descriptor: &RuntimeTypeDescriptor) -> Result<(), String> {
    if descriptor.cli.is_some() || descriptor.migration.is_some() {
        return Err(format!(
            "type `{}` carries CLI or schema-migration metadata",
            descriptor.name
        ));
    }
    out.extend_from_slice(&descriptor.id.to_le_bytes());
    write_str(out, &descriptor.name);
    write_str(out, &descriptor.canonical);
    out.push(tag_of(&VALUE_KINDS, &descriptor.kind));
    out.push(tag_of(&VALUE_ABIS, &descriptor.abi));
    out.push(u8::from(descriptor.integer_width.is_some()));
    if let Some(width) = descriptor.integer_width {
        out.push(u8::from(width.signed));
        out.push(width.bits);
    }
    out.push(u8::from(descriptor.integer_range.is_some()));
    if let Some((low, high)) = descriptor.integer_range {
        out.extend_from_slice(&low.to_le_bytes());
        out.extend_from_slice(&high.to_le_bytes());
    }
    for child in [
        descriptor.element,
        descriptor.key,
        descriptor.value,
        descriptor.ok,
        descriptor.err,
    ] {
        write_opt_u64(out, child);
    }
    write_opt_str(out, descriptor.serde_tag.as_deref());
    out.push(u8::from(descriptor.serde_untagged));
    out.push(u8::from(descriptor.serde_deny_unknown));
    write_fields(out, &descriptor.fields);
    out.extend_from_slice(&(descriptor.variants.len() as u32).to_le_bytes());
    for variant in &descriptor.variants {
        write_str(out, &variant.name);
        write_str(out, &variant.wire_name);
        out.extend_from_slice(&variant.discriminant.to_le_bytes());
        write_fields(out, &variant.fields);
    }
    Ok(())
}

fn read_descriptor(data: &[u8], i: &mut usize) -> Option<RuntimeTypeDescriptor> {
    let id = read_u64(data, i)?;
    let name = read_str(data, i)?;
    let canonical = read_str(data, i)?;
    let kind = from_tag(&VALUE_KINDS, read_u8(data, i)?)?;
    let abi = from_tag(&VALUE_ABIS, read_u8(data, i)?)?;
    let integer_width = if read_bool(data, i)? {
        Some(RuntimeIntegerWidth {
            signed: read_bool(data, i)?,
            bits: read_u8(data, i)?,
        })
    } else {
        None
    };
    let integer_range = if read_bool(data, i)? {
        Some((read_i128(data, i)?, read_i128(data, i)?))
    } else {
        None
    };
    let element = read_opt_u64(data, i)?;
    let key = read_opt_u64(data, i)?;
    let value = read_opt_u64(data, i)?;
    let ok = read_opt_u64(data, i)?;
    let err = read_opt_u64(data, i)?;
    let serde_tag = read_opt_str(data, i)?;
    let serde_untagged = read_bool(data, i)?;
    let serde_deny_unknown = read_bool(data, i)?;
    let fields = read_fields(data, i)?;
    let variant_count = read_u32(data, i)? as usize;
    let mut variants = Vec::with_capacity(variant_count.min(data.len()));
    for _ in 0..variant_count {
        variants.push(RuntimeVariantDescriptor {
            name: read_str(data, i)?,
            wire_name: read_str(data, i)?,
            discriminant: read_i64(data, i)?,
            fields: read_fields(data, i)?,
        });
    }
    Some(RuntimeTypeDescriptor {
        id,
        name,
        canonical,
        kind,
        abi,
        integer_width,
        integer_range,
        element,
        key,
        value,
        ok,
        err,
        serde_tag,
        serde_untagged,
        serde_deny_unknown,
        cli: None,
        fields,
        migration: None,
        variants,
    })
}

fn write_fields(out: &mut Vec<u8>, fields: &[RuntimeFieldDescriptor]) {
    out.extend_from_slice(&(fields.len() as u32).to_le_bytes());
    for field in fields {
        out.extend_from_slice(&(field.index as u64).to_le_bytes());
        write_str(out, &field.source_name);
        let names = &field.shape_names;
        write_str(out, &names.text);
        for optional in [
            &names.json,
            &names.cbor,
            &names.csv,
            &names.toml,
            &names.yaml,
            &names.xml,
        ] {
            write_opt_str(out, optional.as_deref());
        }
        write_str(out, &names.args);
        write_str(out, &names.env);
        write_opt_str(out, names.db.as_deref());
        write_opt_str(out, names.layout.as_deref());
        for flag in [field.skip, field.computed, field.has_default, field.redacted] {
            out.push(u8::from(flag));
        }
        out.extend_from_slice(&field.type_id.to_le_bytes());
    }
}

fn read_fields(data: &[u8], i: &mut usize) -> Option<Vec<RuntimeFieldDescriptor>> {
    let count = read_u32(data, i)? as usize;
    let mut fields = Vec::with_capacity(count.min(data.len()));
    for _ in 0..count {
        let index = usize::try_from(read_u64(data, i)?).ok()?;
        let source_name = read_str(data, i)?;
        let shape_names = ShapeFieldNames {
            text: read_str(data, i)?,
            json: read_opt_str(data, i)?,
            cbor: read_opt_str(data, i)?,
            csv: read_opt_str(data, i)?,
            toml: read_opt_str(data, i)?,
            yaml: read_opt_str(data, i)?,
            xml: read_opt_str(data, i)?,
            args: read_str(data, i)?,
            env: read_str(data, i)?,
            db: read_opt_str(data, i)?,
            layout: read_opt_str(data, i)?,
        };
        fields.push(RuntimeFieldDescriptor {
            index,
            source_name,
            shape_names,
            skip: read_bool(data, i)?,
            computed: read_bool(data, i)?,
            has_default: read_bool(data, i)?,
            redacted: read_bool(data, i)?,
            type_id: read_u64(data, i)?,
        });
    }
    Some(fields)
}

const ARTIFACT_KINDS: [MirArtifactKind; 7] = [
    MirArtifactKind::NativeExecutable,
    MirArtifactKind::NativeLibrary,
    MirArtifactKind::SandboxPlugin,
    MirArtifactKind::WebApplication,
    MirArtifactKind::TestExecutable,
    MirArtifactKind::FuzzExecutable,
    MirArtifactKind::TestOverride,
];

const ARTIFACT_TARGETS: [MirArtifactTarget; 4] = [
    MirArtifactTarget::RustAot,
    MirArtifactTarget::Cranelift,
    MirArtifactTarget::Interpreter,
    MirArtifactTarget::Web,
];

const BUILD_MODES: [MirArtifactBuildMode; 5] = [
    MirArtifactBuildMode::Dev,
    MirArtifactBuildMode::Release,
    MirArtifactBuildMode::Test,
    MirArtifactBuildMode::Fuzz,
    MirArtifactBuildMode::Coverage,
];

fn write_u64_list(out: &mut Vec<u8>, values: &[u64]) {
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn read_u64_list(data: &[u8], i: &mut usize) -> Option<Vec<u64>> {
    let count = read_u32(data, i)? as usize;
    let mut values = Vec::with_capacity(count.min(data.len()));
    for _ in 0..count {
        values.push(read_u64(data, i)?);
    }
    Some(values)
}

fn write_execution(out: &mut Vec<u8>, execution: &MirExecutionIdentity) {
    out.extend_from_slice(&execution.schema_version.to_le_bytes());
    let artifact = &execution.artifact;
    out.extend_from_slice(&artifact.schema_version.to_le_bytes());
    out.extend_from_slice(&artifact.mir_schema_version.to_le_bytes());
    out.extend_from_slice(&artifact.program_digest);
    write_str(out, &artifact.package_identity);
    out.extend_from_slice(&artifact.artifact.0.to_le_bytes());
    write_str(out, &artifact.name);
    out.push(tag_of(&ARTIFACT_KINDS, &artifact.kind));
    out.push(tag_of(&ARTIFACT_TARGETS, &artifact.target));
    out.push(tag_of(&BUILD_MODES, &artifact.mode));
    write_str(out, &artifact.provider_identity);
    write_str(out, &artifact.closure_identity);
    write_str(out, &artifact.artifact_identity);
    let program = &artifact.program_identity;
    write_str(out, &program.semantic_hash);
    write_str(out, &program.optimized_hash);
    write_u64_list(out, &program.function_ids);
    write_u64_list(out, &program.core_ids);
    out.extend_from_slice(&(program.target_facts.len() as u32).to_le_bytes());
    for (key, value) in &program.target_facts {
        write_str(out, key);
        write_str(out, value);
    }
    out.extend_from_slice(&(program.source_map.len() as u32).to_le_bytes());
    for source in &program.source_map {
        out.extend_from_slice(&source.id.to_le_bytes());
        write_str(out, &source.path);
        write_str(out, &source.digest);
    }
}

fn read_execution(data: &[u8], i: &mut usize) -> Option<MirExecutionIdentity> {
    let schema_version = read_u16(data, i)?;
    let artifact_schema_version = read_u16(data, i)?;
    let mir_schema_version = read_u16(data, i)?;
    let program_digest: [u8; 32] = data.get(*i..*i + 32)?.try_into().ok()?;
    *i += 32;
    let package_identity = read_str(data, i)?;
    let artifact_id = MirArtifactId(read_u64(data, i)?);
    let name = read_str(data, i)?;
    let kind = from_tag(&ARTIFACT_KINDS, read_u8(data, i)?)?;
    let target = from_tag(&ARTIFACT_TARGETS, read_u8(data, i)?)?;
    let mode = from_tag(&BUILD_MODES, read_u8(data, i)?)?;
    let provider_identity = read_str(data, i)?;
    let closure_identity = read_str(data, i)?;
    let artifact_identity = read_str(data, i)?;
    let semantic_hash = read_str(data, i)?;
    let optimized_hash = read_str(data, i)?;
    let function_ids = read_u64_list(data, i)?;
    let core_ids = read_u64_list(data, i)?;
    let fact_count = read_u32(data, i)? as usize;
    let mut target_facts = Vec::with_capacity(fact_count.min(data.len()));
    for _ in 0..fact_count {
        target_facts.push((read_str(data, i)?, read_str(data, i)?));
    }
    let source_count = read_u32(data, i)? as usize;
    let mut source_map = Vec::with_capacity(source_count.min(data.len()));
    for _ in 0..source_count {
        source_map.push(MirSourceMapIdentity {
            id: read_u64(data, i)?,
            path: read_str(data, i)?,
            digest: read_str(data, i)?,
        });
    }
    Some(MirExecutionIdentity {
        schema_version,
        artifact: MirArtifactIdentity {
            schema_version: artifact_schema_version,
            mir_schema_version,
            program_digest,
            package_identity,
            artifact: artifact_id,
            name,
            kind,
            target,
            mode,
            provider_identity,
            closure_identity,
            artifact_identity,
            program_identity: Arc::new(MirProgramIdentity {
                semantic_hash,
                optimized_hash,
                function_ids,
                core_ids,
                target_facts,
                source_map,
            }),
        },
    })
}
