use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Codegen::MIRRust::{
    MirRustConfig, MirRustExecutionConfig, MirRustTarget, UNIT_BEGIN_MARKER, UNIT_END_MARKER,
};
use jet_foundation::Layout::TargetLayout;
use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget, MirOperation,
};

const BOOTSTRAP_PROJECT_RELATIVE: &str = ".cache/jet-luna/compiler-bootstrap/project";
/// Stable home of the per-artifact unit crates of a unit-split backend build.
const BOOTSTRAP_UNITS_RELATIVE: &str = ".cache/jet-luna/compiler-bootstrap/units";
/// Dev-profile override for the runtime and unit crates of a unit-split
/// backend build. Each unit crate is one very large rustc (the compiler's
/// Sema and Foundation units are over 100 MB of Rust each); full debuginfo
/// would multiply its codegen memory.
const BACKEND_UNIT_PROFILE: &str = "debug = \"line-tables-only\"\n";
/// Dev-profile rows for the repository crate whose host code dominates a
/// debug jetc0's run time: compiler-image restore and every
/// `execution_identity()` re-encode, verify and re-digest the whole compiler
/// MIR in jet-foundation (MIR image codec, `CanonicalWriter`), which at
/// opt-level 0 with debug assertions (`MIR_VERIFICATION` follows
/// `debug_assertions`) costs tens of minutes per run. Built once per change
/// of the crate; the emitted unit crates keep the fast dev profile.
const BACKEND_DEV_HOST_PROFILE: &str = "\n[profile.dev.package.jet-foundation]\nopt-level = 2\ndebug-assertions = false\n";
/// Parallel rustc jobs (and codegen tokens) of a backend build. The
/// stage-zero backend build runs inside the bootstrap test's memory cap, so
/// at most this many unit or repository crates compile at once. Measured
/// peaks: sharded unit crates <= 2.1 GB each, jet_jit 5.7 GB, so six jobs
/// stay well inside a 22 GB cap.
const BACKEND_BUILD_JOBS: &str = "6";
/// Stack for the backend build's rustc threads (`RUST_MIN_STACK`): the
/// emitted units' deeply nested blocks overflow rustc's default 8 MiB in
/// LLVM's debuginfo scope walk (`DwarfCompileUnit::createAndAddScopeChildren`).
const BACKEND_RUSTC_STACK: &str = "268435456";
/// A unit crate longer than this is sharded (`shard_unit`) into crates of
/// about this size: rustc's memory grows with the crate, and the compiler's
/// largest units alone would not fit the bootstrap's memory cap.
const BACKEND_SHARD_BYTES: usize = 12 << 20;
/// Impl methods at least this long move out of a sharded unit's facade.
const BACKEND_DELEGATE_METHOD_BYTES: usize = 2048;
/// A moved function at least this long is cold (`shard_unit`) whatever its
/// kind: LLVM's optimization time and memory grow superlinearly within one
/// function (one function is one codegen unit). Measured at opt-level 2:
/// seven Jet functions of 200-455 KB (2.3 MB) build in 55 s at 1.1 GB, while
/// a shard holding 2-3 MB decoders and tables took 773 s at 9.1 GB.
const BACKEND_COLD_FUNCTION_BYTES: usize = 1 << 20;
/// Traits whose impl methods are cold (`shard_unit`): the derived codecs run
/// only on compiler image load and save, derived orderings rarely, and their
/// bodies are among the program's largest functions.
const BACKEND_COLD_TRAITS: [&str; 3] = ["__jet_Decode", "__jet_Encode", "__jet_Comparable"];
/// Release-profile override for cold shard crates (`shard_unit`). Measured on
/// a 12 MB shard of derived decoders and constant tables: opt-level 2 peaks at
/// 9.1 GB in 773 s, opt-level 1 at 7.2 GB in 787 s, opt-level 0 at 2.2 GB in
/// 53 s. Hot shards, facades and the runtime keep the profile's opt-level 2
/// (a 12 MB hot shard: about 3 GB in 110-120 s).
const BACKEND_COLD_PROFILE: &str = "opt-level = 0\ndebug = false\n";
/// `BACKEND_BUILD_JOBS` of a release-profile backend build. Measured peaks:
/// 12 MB hot shards about 3 GB (opt-level 2), the runtime 2 GB, the largest
/// cold shard 4.6 GB (opt-level 0); with jet_jit (5.7 GB in dev) five jobs
/// stay under a 22 GB cap.
const BACKEND_RELEASE_BUILD_JOBS: &str = "5";
/// `JET_STAGE_ZERO_PROFILE=release` builds this test run's backend artifacts
/// (jetc0 above all) with this Cargo profile instead of dev: stage one is
/// jetc0 compiling the whole compiler, where a debug jetc0 is many times
/// slower. Thin-local LTO is off and debuginfo is off to keep the unit crates'
/// rustc memory and time down; overflow checks stay on (generated code may
/// rely on them). The dev profile stays the default: it links fastest.
const BACKEND_RELEASE_PROFILE_NAME: &str = "jet-stage-release";
const BACKEND_RELEASE_PROFILE: &str = "\n[profile.jet-stage-release]\ninherits = \"release\"\nopt-level = 2\ndebug = false\nlto = \"off\"\ncodegen-units = 16\nincremental = false\noverflow-checks = true\n";

/// The Cargo profile of backend builds: `None` = dev, `Some(name)` = the
/// release profile `JET_STAGE_ZERO_PROFILE=release` selects.
fn backend_release_profile() -> Option<&'static str> {
    match std::env::var("JET_STAGE_ZERO_PROFILE").as_deref() {
        Err(_) | Ok("") | Ok("dev") => None,
        Ok("release") => Some(BACKEND_RELEASE_PROFILE_NAME),
        Ok(other) => panic!("JET_STAGE_ZERO_PROFILE must be `dev` or `release`, not `{other}`"),
    }
}
const BOOTSTRAP_ENTRY_RELATIVE: &str = "src/compiler.jet";
const SMALL_ENTRY_RELATIVE: &str = "main.jet";
const TASK_ROOTS_FIXTURE_SOURCE: &str = include_str!("../JetEval/Tests/TaskRoots.jet");
const CORE_FILES_PARTIAL_MOVE_SIBLINGS_SOURCE: &str =
    include_str!("../../tests/fixtures/core_files_partial_move_siblings.jet");
const UNINIT_FIXED_PARTIAL_EXIT_SOURCE: &str =
    include_str!("../../tests/fixtures/uninit_fixed_partial_exit.jet");
const SMALL_PROGRAM_SOURCE: &str = r#"SOURCE_TEXT :: prep { "π🙂" }
SOURCE_LIST :: prep { [Int]{7, 8} }
SOURCE_BYTES :: prep { SOURCE_TEXT.bytes() }
MATERIALIZED_TEXT :: prep { ~(SOURCE_TEXT.after("")) }
MATERIALIZED_LIST :: prep { ~(SOURCE_LIST[0..<SOURCE_LIST.len()]) }
MATERIALIZED_BYTES :: prep { ~(SOURCE_BYTES[0..<SOURCE_BYTES.len()]) }
MATERIALIZED_LIST_LEN :: prep { MATERIALIZED_LIST.len() }
MATERIALIZED_BYTES_LEN :: prep { MATERIALIZED_BYTES.len() }

pub fn main() {
    print("bootstrap-small-program")
    print("{MATERIALIZED_TEXT}")
    print("{MATERIALIZED_LIST_LEN}")
    print("{MATERIALIZED_BYTES_LEN}")
}
"#;
const SMALL_PROGRAM_MANIFEST: &str = r#"name: "bootstrap_small"
version: "0.1.0"
edition: "2028"
outputs: { app: .Executable{ entry: main } }
"#;

const CARRIER_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/contracts/carrier_binding.jet");
const CARRIER_FIXTURE_EXPECTED: &str =
    include_str!("../../Examples/features/expected/contracts/carrier_binding.out");
const SOURCE_FIXTURE_MANIFEST: &str = r#"name: "bootstrap_source_fixture"
version: "0.1.0"
edition: "2028"
outputs: { app: .Executable{ entry: run } }
"#;
const USER_OPERATOR_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/operators/user_defined.jet");
const USER_OPERATOR_FIXTURE_EXPECTED: &str = "4,6 4,6 true true false\n";
const SPACESHIP_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/operators/spaceship.jet");
const SPACESHIP_FIXTURE_EXPECTED: &str =
    "int less: true\nstring less: true\nAda:10\nCal:20\nBea:30\n";
const MIXED_OPERATOR_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/operators/mixed_types.jet");
const MIXED_OPERATOR_FIXTURE_EXPECTED: &str =
    include_str!("../../Examples/features/expected/operators/mixed_types.out");
// D-ONCE-DERIVE1=A: Equatable/Comparable bodies expand from the Prelude
// derive templates in JetSema, including reflected enum payloads.
const DERIVED_ORDER_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/types/enum_derived_order.jet");
const DERIVED_ORDER_FIXTURE_EXPECTED: &str =
    include_str!("../../Examples/features/expected/types/enum_derived_order.out");
// #3740: `T Never!` / `T? Never!` functions return their success value
// directly; calls, early returns, and function values reconcile the carrier.
const NEVER_PLAIN_RETURN_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/functions/never_plain_return.jet");
const NEVER_PLAIN_RETURN_FIXTURE_EXPECTED: &str =
    include_str!("../../Examples/features/expected/functions/never_plain_return.out");
// Registry-row shapes the stage-one ladder hit: owned temporaries fill `^`
// and `&` parameters, plain enums compare through their derived equality,
// and `Val(Enum.X)` / `Ok(Enum.X)` / `??` payloads reach canonical declared
// types through tables, returns, and a `prep` row table. The unit is smaller
// than the derive template, so derived members keep offsets past its end.
const REGISTRY_ROWS_FIXTURE_SOURCE: &str =
    include_str!("../../Examples/features/types/registry_rows.jet");
const REGISTRY_ROWS_FIXTURE_EXPECTED: &str =
    include_str!("../../Examples/features/expected/types/registry_rows.out");
const HANDLE_LIFETIME_FIXTURE_SOURCE: &str = r#"use c.close as c

#Layout(c)
struct Handle {
    value: I64
}

#Import module c.close {
    #Close(release)
    fn acquire() Handle = "jet_handle_acquire"
    fn read(handle: ^Handle) I64 = "jet_handle_read"
    fn release(handle: ^Handle) = "jet_handle_release"
    fn released() I64 = "jet_handle_released"
}

fn inspect(handle: ^Handle) -> I64 {
    return c.read(handle)
}

fn make() -> Handle {
    return c.acquire()
}

fn run() {
    borrowed := c.acquire()
    print(inspect(^borrowed))
    print(c.released())
    close(^borrowed)
    print(c.released())

    returned := make()
    print(inspect(^returned))
    print(c.released())
    close(^returned)
    print(c.released())
}
"#;
const HANDLE_LIFETIME_FIXTURE_EXPECTED: &str = "41\n0\n41\n42\n41\n42\n";
const HANDLE_LIFETIME_C_PROVIDER: &str = r#"#include <stdint.h>
typedef struct { int64_t value; } JetHandle;
static int64_t next_value = 41;
static int64_t released_value = 0;
JetHandle jet_handle_acquire(void) { return (JetHandle){next_value++}; }
int64_t jet_handle_read(JetHandle handle) { return handle.value; }
void jet_handle_release(JetHandle handle) { released_value = handle.value; }
int64_t jet_handle_released(void) { return released_value; }
"#;
const EXACT_DIVISION_FIXTURE_ONE: &str = r#"fn run() {
    third :: 1 / 3
    print(third)
    print("interpolated {third}")
    print(third * 3 == 1)
}
"#;
const EXACT_DIVISION_FIXTURE_TWO: &str = r#"TEN :: prep { 10 }
THIRD :: prep { TEN / 3 }
fn run() {
    runtime :: 10 / 3
    print(THIRD)
    print(THIRD == runtime)
    print(THIRD * 3 == 10)
}
"#;
const EXACT_DIVISION_FIXTURE_ONE_EXPECTED: &str = "1/3\ninterpolated 1/3\ntrue\n";
const EXACT_DIVISION_FIXTURE_TWO_EXPECTED: &str = "10/3\ntrue\ntrue\n";
/// This is appended to every compiler artifact built by the private harness.
/// It is an executable harness entry, not a compiler callback: the generated
/// Jet factory and the packaged Runner remain the only compilation path.
/// Text between `TASK_ROOTS_BEGIN` and `TASK_ROOTS_END` calls the task-root
/// fixture functions and is kept only when that fixture is compiled in.
const GENERATED_ARTIFACT_MAIN: &str = r#"
// bootstrap:task-roots-begin
struct BootstrapTaskRootsCursorState {
    close_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl ::jet_foundation::MIR::MirNativeCursorState for BootstrapTaskRootsCursorState {
    fn has_next(&mut self) -> Result<bool, ::jet_foundation::MIR::MirNativeCursorError> {
        Ok(false)
    }
    fn value(
        &mut self,
    ) -> Result<
        ::jet_foundation::MIR::MirRuntimeValue,
        ::jet_foundation::MIR::MirNativeCursorError,
    > {
        Ok(::jet_foundation::MIR::MirRuntimeValue::Unit)
    }
    fn advance(&mut self) -> Result<(), ::jet_foundation::MIR::MirNativeCursorError> {
        Ok(())
    }
}
impl Drop for BootstrapTaskRootsCursorState {
    fn drop(&mut self) {
        self.close_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
// bootstrap:task-roots-end
fn assert_source_completion_retired(
    completion: ::jet_jit::SourceExecutionCompletion,
) {
    match completion.disposition {
        ::jet_jit::SourceExecutionCompletionDisposition::Invoked {
            retirement: Some(
                ::jet_jit::SourceExecutionCompletionRetirement::Completed(retirement),
            ),
            ..
        } => {
            assert!(
                matches!(
                    retirement.outcome.as_ref(),
                    Some(::jet_foundation::JitBackend::RunOutcome::Ran { .. })
                ),
                "bootstrap returned a failed Source completion: {retirement:?}",
            );
            for nested in retirement.completions {
                assert_source_completion_retired(nested);
            }
        }
        disposition => panic!("bootstrap returned an incomplete Source completion: {disposition:?}"),
    }
}

fn finish_source_completion_owner(
    owner: &crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner,
) {
    loop {
        match owner.finish() {
            Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Ready(completions)) => {
                for completion in completions {
                    assert_source_completion_retired(completion);
                }
            }
            Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Pending {
                retirement_requested,
                retained_roots,
                open_callback_sessions,
                pending_jobs,
            }) => panic!(
                "bootstrap retained Source work: retirement_requested={retirement_requested}, retained_roots={retained_roots}, open_callback_sessions={open_callback_sessions}, pending_jobs={pending_jobs}"
            ),
            Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Complete { failures }) => {
                assert!(failures.is_empty(), "bootstrap callback jobs failed: {failures:?}");
                break;
            }
            Err(error) => panic!("bootstrap Source completion finish failed: {error:?}"),
        }
    }
}

#[doc(hidden)]
fn assert_private_compiler_image_root(
    image: &__JetBootstrapCompilerImage,
    bindings: &crate::BootstrapBindingDescriptor,
) {
    let mut roots = bindings
        .callables
        .iter()
        .filter(|callable| callable.source_name == "jet_bootstrap_compile");
    let root = roots
        .next()
        .unwrap_or_else(|| panic!("compiler bindings have no private factory root"));
    assert!(
        roots.next().is_none(),
        "compiler bindings have multiple private factory roots"
    );
    let root_id = root.metadata.function.clone();
    let artifact = image
        .program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == image.header.artifact)
        .unwrap_or_else(|| panic!("compiler image artifact is absent from its MIR"));
    assert_eq!(
        artifact.target,
        ::jet_foundation::MIR::MirArtifactTarget::RustAot
    );
    assert!(
        artifact.entry.is_none(),
        "private compiler factory root must not become a public artifact entry"
    );
    assert_eq!(image.header.entry_function, root_id);
    let function = image
        .program
        .functions
        .iter()
        .find(|function| function.id == root_id)
        .unwrap_or_else(|| panic!("private compiler factory root is absent from checked MIR"));
    assert_eq!(function.name, "jet_bootstrap_compile");
    assert!(function.capture_params.is_empty());
    assert!(function.target_applicability.rust_aot);
    assert!(artifact.modules.contains(&function.module_id));
    crate::BootstrapEntryCodec::new(&image.program, bindings, root_id)
        .unwrap_or_else(|error| panic!("private compiler factory root ABI is invalid: {error}"));
}
fn bootstrap_fixture_function<'a>(
    program: &'a ::jet_foundation::MIR::MirProgram,
    source_file: ::jet_foundation::MIR::MirSourceFileId,
    name: &str,
) -> &'a ::jet_foundation::MIR::MirFunction {
    let mut functions = program
        .functions
        .iter()
        .filter(|function| function.source_file == source_file && function.name == name);
    let function = functions
        .next()
        .unwrap_or_else(|| panic!("fixture MIR has no exact `{name}` function"));
    assert!(
        functions.next().is_none(),
        "fixture MIR has multiple exact `{name}` functions"
    );
    function
}

fn bootstrap_mir_calls_target(
    function: &::jet_foundation::MIR::MirFunction,
    target: ::jet_foundation::MIR::MirFunctionId,
) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .any(|instruction| match &instruction.operation {
            ::jet_foundation::MIR::MirOperation::Call { callee, .. } => match callee {
                ::jet_foundation::MIR::MirCallee::User(function) => *function == target,
                ::jet_foundation::MIR::MirCallee::Associated { function, .. }
                | ::jet_foundation::MIR::MirCallee::Method { function, .. } => *function == target,
                _ => false,
            },
            _ => false,
        })
}
/// The generated compiler recurses over a whole compiler unit (parser, sema,
/// lowering, emission). The process main thread only has the `ulimit -s`
/// stack, so the harness runs on its own thread with a large reserved stack
/// (address space, committed page by page). Overflowing it hits the guard
/// page, and std reports `thread 'jet-bootstrap' has overflowed its stack`
/// instead of a bare SIGSEGV; a panic keeps its message and exit status 101.
#[doc(hidden)]
fn main() {
    const BOOTSTRAP_STACK_BYTES: usize = 512 * 1024 * 1024;
    let worker = std::thread::Builder::new()
        .name("jet-bootstrap".to_string())
        .stack_size(BOOTSTRAP_STACK_BYTES)
        .spawn(__jet_bootstrap_harness_main)
        .unwrap_or_else(|error| panic!("cannot start the bootstrap compiler thread: {error}"));
    if let Err(payload) = worker.join() {
        std::panic::resume_unwind(payload);
    }
}

#[doc(hidden)]
fn __jet_bootstrap_harness_main() {
    let mode = std::env::var("JET_BOOTSTRAP_MODE")
        .unwrap_or_else(|error| panic!("bootstrap mode is unavailable: {error}"));
    let factory_tier_name = std::env::var("JET_BOOTSTRAP_FACTORY_TIER")
        .unwrap_or_else(|_| "aot".to_string());
    let factory_tier = match factory_tier_name.as_str() {
        "aot" => crate::BootstrapFactoryTier::Aot,
        _ => panic!(
            "unknown bootstrap factory tier `{factory_tier_name}`: stage zero runs the compiler factory only as AOT"
        ),
    };
    // JET_BOOTSTRAP_BATCH names a file of `source_root<TAB>entry<TAB>output<TAB>receipt`
    // rows: one process compiles them in order, restoring the compiler image
    // once. A panicking entry is reported between its `jet-bootstrap-batch:`
    // begin/end lines on stderr and the batch goes on with the next row.
    if let Ok(batch) = std::env::var("JET_BOOTSTRAP_BATCH") {
        __jet_bootstrap_harness_batch(&mode, factory_tier, &batch);
        return;
    }
    let source_root = std::env::var("JET_BOOTSTRAP_SOURCE_ROOT")
        .unwrap_or_else(|error| panic!("bootstrap source root is unavailable: {error}"));
    let entry = std::env::var("JET_BOOTSTRAP_ENTRY")
        .unwrap_or_else(|error| panic!("bootstrap entry is unavailable: {error}"));
    let output_path = std::env::var("JET_BOOTSTRAP_OUTPUT")
        .unwrap_or_else(|error| panic!("bootstrap output path is unavailable: {error}"));
    let receipt_path = std::env::var("JET_BOOTSTRAP_RECEIPT")
        .unwrap_or_else(|error| panic!("bootstrap receipt path is unavailable: {error}"));
    __jet_bootstrap_harness_entry(&mode, factory_tier, &source_root, &entry, &output_path, &receipt_path);
}

#[doc(hidden)]
fn __jet_bootstrap_harness_batch(mode: &str, factory_tier: crate::BootstrapFactoryTier, batch: &str) {
    let rows = std::fs::read_to_string(batch)
        .unwrap_or_else(|error| panic!("cannot read bootstrap batch `{batch}`: {error}"));
    for (index, row) in rows.lines().enumerate() {
        if row.is_empty() {
            continue;
        }
        let fields: Vec<&str> = row.split('\t').collect();
        let [source_root, entry, output_path, receipt_path] = fields.as_slice() else {
            panic!("bootstrap batch row {index} is not source_root, entry, output and receipt: `{row}`");
        };
        eprintln!("jet-bootstrap-batch: begin {index} {source_root} {entry}");
        let started = std::time::Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            __jet_bootstrap_harness_entry(mode, factory_tier, source_root, entry, output_path, receipt_path)
        }));
        eprintln!(
            "jet-bootstrap-batch: end {index} {} {}ms",
            if outcome.is_ok() { "ok" } else { "panicked" },
            started.elapsed().as_millis()
        );
    }
}

#[doc(hidden)]
fn __jet_bootstrap_harness_entry(
    mode: &str,
    factory_tier: crate::BootstrapFactoryTier,
    source_root: &str,
    entry: &str,
    output_path: &str,
    receipt_path: &str,
) {
    let lease = crate::compiler_bootstrap_host::open_authorized_sources(
        std::path::Path::new(source_root),
        std::path::Path::new(entry),
    )
    .unwrap_or_else(|error| panic!("authorized bootstrap source snapshot failed: {error:?}"));
    if mode == "offset-projection" {
        let projected = crate::__jet_bootstrap_project_span(
            &::jet_foundation::Diagnostics::Span {
                start: 1200,
                end: 1204,
            },
            1200,
            0,
            "🧪",
        )
        .unwrap_or_else(|error| panic!("nonzero source-segment projection failed: {error}"));
        let projection = format!("{}..{}", projected.start, projected.end);
        std::fs::write(&output_path, &projection)
            .unwrap_or_else(|error| panic!("cannot write source-segment projection: {error}"));
        std::fs::write(&receipt_path, format!("mode=offset-projection\nspan={projection}\n"))
            .unwrap_or_else(|error| panic!("cannot write source-segment projection receipt: {error}"));
        return;
    }

    if mode == "optional-roundtrip" {
        let absent = ::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::Int,
        );
        let present = absent
            .clone()
            .with_identity(::jet_foundation::MIR::MirTypeId(41));
        let absent_carrier = crate::__jet_bootstrap_type_from_host(&absent)
            .unwrap_or_else(|error| panic!("absent type identity encoding failed: {error}"));
        let present_carrier = crate::__jet_bootstrap_type_from_host(&present)
            .unwrap_or_else(|error| panic!("present type identity encoding failed: {error}"));
        let absent_roundtrip = crate::__jet_bootstrap_type_to_host(&absent_carrier)
            .unwrap_or_else(|error| panic!("absent type identity decoding failed: {error}"));
        let present_roundtrip = crate::__jet_bootstrap_type_to_host(&present_carrier)
            .unwrap_or_else(|error| panic!("present type identity decoding failed: {error}"));
        assert_eq!(absent_roundtrip.identity, None);
        assert_eq!(present_roundtrip.identity, present.identity);
        let expected_trait =
            ::jet_foundation::MIR::MirNominalRef::from_name("JetEvalHostAdapter");
        let trait_ty = ::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::TraitObject(vec![expected_trait.clone()]),
        );
        let trait_carrier = crate::__jet_bootstrap_type_from_host(&trait_ty)
            .unwrap_or_else(|error| panic!("trait object identity encoding failed: {error}"));
        let trait_roundtrip = crate::__jet_bootstrap_type_to_host(&trait_carrier)
            .unwrap_or_else(|error| panic!("trait object identity decoding failed: {error}"));
        let trait_bounds = match trait_roundtrip.kind() {
            ::jet_foundation::MIR::MirTypeKind::TraitObject(bounds) => bounds,
            _ => panic!("trait object roundtrip changed its MIR kind"),
        };
        assert_eq!(trait_bounds, &[expected_trait]);
        let zero_trait_ty = ::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::TraitObject(vec![
                ::jet_foundation::MIR::MirNominalRef {
                    id: ::jet_foundation::MIR::MirTypeId(0),
                    name: "JetEvalHostAdapter".to_string(),
                },
            ]),
        );
        assert!(crate::__jet_bootstrap_type_from_host(&zero_trait_ty).is_err());
        std::fs::write(&output_path, "identity=absent,present\ntrait=exact-id-name\n")
            .unwrap_or_else(|error| panic!("cannot write optional codec result: {error}"));
        std::fs::write(
            &receipt_path,
            "mode=optional-roundtrip\nidentity_absent=roundtrip\nidentity_present=roundtrip\ntrait_id_name=roundtrip\ntrait_zero=rejected\n",
        )
        .unwrap_or_else(|error| panic!("cannot write optional codec receipt: {error}"));
        return;
    }

    if mode == "optimizer-evidence" {
        let compiler_image = crate::__jet_bootstrap_compiler_image_envelope()
            .unwrap_or_else(|error| panic!("optimizer-evidence compiler image envelope failed: {error}"));
        let execution = crate::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(
            compiler_image.artifact,
        );
        let completion_scope = ::jet_jit::SourceExecutionCompletionScope::new();
        let mut completion_owner =
            crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner::new(
                completion_scope.clone(),
            );
        let mut result = completion_scope
            .with_current(|| {
                crate::__jet_bootstrap_compile_from_host(
                    lease.snapshot(),
                    crate::__jet_bootstrap_native_compile_target(),
                    factory_tier,
                    &execution,
                    &mut completion_owner,
                )
            })
            .unwrap_or_else(|error| panic!("optimizer-evidence compiler factory failed: {error}"));
        assert!(result.complete, "compiler-source optimizer evidence did not compile");
        let program = result
            .mir
            .as_ref()
            .unwrap_or_else(|| panic!("optimizer-evidence result has no optimized MIR"));
        let pipeline_file = program
            .source_files
            .iter()
            .find(|file| file.path.ends_with("Compiler/JetOptimizer/Source/Pipeline.jet"))
            .unwrap_or_else(|| panic!("optimizer evidence is not bound to compiler-source Pipeline MIR"));
        let optimizer = bootstrap_fixture_function(program, pipeline_file.id, "optimize_mir_program");
        let inline_pass =
            bootstrap_fixture_function(program, pipeline_file.id, "mir_pass_expand_inline_always");
        let inline_pass_call_retained = bootstrap_mir_calls_target(optimizer, inline_pass.id);
        assert!(
            inline_pass_call_retained,
            "compiler-source optimizer entry lost its checked inline-expansion pass"
        );
        if let Some(resources) = result.resources.take() {
            completion_owner.set_resources(resources);
        }
        drop(result.runtime_config.take());
        completion_owner.allow_resource_retirement();
        finish_source_completion_owner(&completion_owner);
        let evidence = "mode=optimizer-evidence\nsource_file=Compiler/JetOptimizer/Source/Pipeline.jet\noptimizer_entry=optimize_mir_program\ninline_pass=mir_pass_expand_inline_always\ninline_pass_call_retained=true\n";
        std::fs::write(&output_path, evidence)
            .unwrap_or_else(|error| panic!("cannot write compiler-source optimizer evidence: {error}"));
        std::fs::write(&receipt_path, evidence)
            .unwrap_or_else(|error| panic!("cannot write compiler-source optimizer receipt: {error}"));
        return;
    }
    // bootstrap:task-roots-begin
    if mode == "task-roots" {
        let compiler_image = crate::__jet_bootstrap_compiler_image()
            .unwrap_or_else(|error| panic!("task-root compiler image restore failed: {error}"));
        let execution = crate::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(
            compiler_image.header.artifact,
        );
        let completion_scope = ::jet_jit::SourceExecutionCompletionScope::new();
        let mut completion_owner =
            crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner::new(
                completion_scope.clone(),
            );
        let mut result = completion_scope
            .with_current(|| {
                crate::__jet_bootstrap_compile_from_host(
                    lease.snapshot(),
                    crate::__jet_bootstrap_native_compile_target(),
                    factory_tier,
                    &execution,
                    &mut completion_owner,
                )
            })
            .unwrap_or_else(|error| panic!("task-root compiler factory failed: {error}"));
        assert!(result.complete, "task-root fixture compiler source did not compile");
        let bindings = result
            .bindings
            .as_ref()
            .unwrap_or_else(|| panic!("task-root compiler result has no binding descriptor"));
        assert_private_compiler_image_root(&compiler_image, bindings);
        let mut source_program = result
            .source_program
            .take()
            .unwrap_or_else(|| panic!("task-root compiler result has no typed Source MIR"));
        let mut runtime_config = result
            .runtime_config
            .take()
            .unwrap_or_else(|| panic!("task-root compiler result has no runtime config"));
        let resources = result
            .resources
            .take()
            .unwrap_or_else(|| panic!("task-root compiler result has no Source resource session"));
        completion_owner.set_resources(resources.clone());
        // The checked MIR is the host projection of `source_program` (same
        // function identities), so the fixture's callback is selected there.
        let checked_mir = result
            .mir
            .as_ref()
            .unwrap_or_else(|| panic!("task-root result has no checked MIR"));
        let mut owners = checked_mir.functions.iter().filter(|function| {
            function.name == "jet_eval_task_roots_fixture_escaping_callback_owner"
        });
        let owner = owners
            .next()
            .unwrap_or_else(|| panic!("task-root Source MIR has no exact callback owner"));
        assert!(owners.next().is_none(), "task-root Source MIR has multiple exact callback owners");
        assert!(owner.capture_params.is_empty(), "fixture callback owner unexpectedly captures values");
        assert_eq!(owner.params.len(), 1, "fixture callback owner lost its checked seed parameter");
        let mut closure_targets = owner
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter_map(|instruction| match &instruction.operation {
                ::jet_foundation::MIR::MirOperation::Closure { function, .. } => Some(function.clone()),
                _ => None,
            });
        let callback_id = closure_targets
            .next()
            .unwrap_or_else(|| panic!("task-root callback owner has no checked closure construction"));
        assert!(
            closure_targets.next().is_none(),
            "task-root callback owner has multiple checked closure constructions"
        );
        let callback = checked_mir
            .functions
            .iter()
            .find(|function| function.id == callback_id)
            .unwrap_or_else(|| panic!("task-root closure target is absent from checked Source MIR"));
        assert_eq!(callback.capture_params.len(), 1);
        let capture_facts = callback
            .captures
            .as_ref()
            .unwrap_or_else(|| panic!("task-root callback has no checked capture facts"));
        assert!(capture_facts.escapes, "task-root callback is not escaping");
        assert!(
            capture_facts.moved.is_empty(),
            "task-root callback unexpectedly moves its capture"
        );
        assert!(
            callback.target_applicability.interpreter,
            "task-root callback target is not enabled for Source execution"
        );

        let close_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cursor_handle = ::jet_jit::SourceResources::loop_cursor_handle_id();
        let cursor_state = BootstrapTaskRootsCursorState {
            close_count: close_count.clone(),
        };
        let cursor = ::jet_foundation::MIR::MirNativeCursor::new(cursor_state);
        let capability = resources
            .arena()
            .insert_cursor(cursor_handle, cursor)
            .unwrap_or_else(|error| panic!("cannot register task-root Source cursor: {error}"));
        let fixture_ok = crate::__jet_bootstrap_task_roots_fixture(
            &mut source_program,
            &mut runtime_config,
            &callback_id,
            &capability.handle,
            capability.raw,
        )
        .unwrap_or_else(|error| panic!("task-root fixture arguments do not encode: {error}"));
        assert!(fixture_ok, "Source task-root fixture did not complete cleanly");
        assert_eq!(
            close_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the real arena-backed cursor must close exactly once at its last alias"
        );
        let mut finalizers = checked_mir
            .functions
            .iter()
            .filter(|function| function.name == "jet_eval_shared_payload_finalize");
        let finalizer = finalizers
            .next()
            .unwrap_or_else(|| panic!("task-root MIR has no checked Shared payload finalizer"));
        assert!(
            finalizers.next().is_none(),
            "task-root MIR has multiple checked Shared payload finalizers"
        );
        let shared_finalizer = finalizer.id;
        let shared_finalizer_return_type = finalizer.return_type.clone();
        fn assert_shared_finalizer_completed(
            completion: ::jet_jit::SourceExecutionCompletion,
            helper: ::jet_foundation::MIR::MirFunctionId,
            mir: &::jet_foundation::MIR::MirProgram,
            return_type: &::jet_foundation::MIR::MirType,
        ) {
            assert_eq!(completion.function, Some(helper));
            match completion.disposition {
                ::jet_jit::SourceExecutionCompletionDisposition::Invoked {
                    retirement: Some(
                        ::jet_jit::SourceExecutionCompletionRetirement::Completed(retirement),
                    ),
                    writebacks,
                    ..
                } => {
                    assert!(writebacks.is_empty(), "Shared payload finalizer has writeback debt");
                    assert!(
                        matches!(
                            retirement.outcome.as_ref(),
                            Some(::jet_foundation::JitBackend::RunOutcome::Ran { .. })
                        ),
                        "Shared payload finalizer failed: {retirement:?}"
                    );
                    assert!(retirement.stdout.is_empty());
                    assert!(retirement.stderr.is_empty());
                    assert!(retirement.completions.is_empty());
                    let value = retirement
                        .value
                        .as_ref()
                        .unwrap_or_else(|| panic!("Shared payload finalizer omitted its checked return value"));
                    crate::compiler_bootstrap_entry_codec::validate_runtime_value(
                        mir,
                        return_type,
                        value,
                        0,
                    )
                    .unwrap_or_else(|error| panic!("Shared finalizer return value violates its checked MIR type: {error}"));
                    let ::jet_foundation::MIR::MirRuntimeValue::Struct {
                        type_name,
                        fields,
                    } = value
                    else {
                        panic!("Shared payload finalizer returned a non-record: {value:?}");
                    };
                    assert_eq!(type_name, "JetEvalOwnedRootDropResult");
                    fn field<'a>(
                        fields: &'a [(String, ::jet_foundation::MIR::MirRuntimeValue)],
                        name: &str,
                    ) -> &'a ::jet_foundation::MIR::MirRuntimeValue {
                        fields
                            .iter()
                            .find(|(field, _)| field == name)
                            .map(|(_, value)| value)
                            .unwrap_or_else(|| panic!("Shared finalizer result omitted `{name}`"))
                    }
                    assert!(matches!(
                        field(fields, "disposition"),
                        ::jet_foundation::MIR::MirRuntimeValue::Enum {
                            type_name,
                            variant,
                            args,
                        } if type_name == "JetEvalOwnedRootDropDisposition"
                            && variant == "Cleaned"
                            && args.is_empty()
                    ));
                    assert!(matches!(
                        field(fields, "failure"),
                        ::jet_foundation::MIR::MirRuntimeValue::Absent { .. }
                    ));
                    assert!(matches!(
                        field(fields, "internal_problem"),
                        ::jet_foundation::MIR::MirRuntimeValue::Absent { .. }
                    ));
                    assert!(matches!(
                        field(fields, "stdout"),
                        ::jet_foundation::MIR::MirRuntimeValue::String(value) if value.is_empty()
                    ));
                    assert!(matches!(
                        field(fields, "stderr"),
                        ::jet_foundation::MIR::MirRuntimeValue::String(value) if value.is_empty()
                    ));
                }
                disposition => panic!(
                    "Shared payload finalizer did not produce an invoked completion: {disposition:?}"
                ),
            }
        }
        let shared_parent_lease = resources
            .retain_root()
            .unwrap_or_else(|error| panic!("cannot retain shared task-root parent: {error}"));
        let shared_child_lease = shared_parent_lease.clone();
        let shared_arena = resources.arena();
        let shared_consumer_ok = completion_scope.with_current(|| {
            let shared_carrier = {
                let _activation = shared_parent_lease.activate();
                crate::__jet_bootstrap_task_roots_fixture_shared_produce(
                    &mut source_program,
                    &mut runtime_config,
                )
                .unwrap_or_else(|error| panic!("task-root shared producer arguments do not encode: {error}"))
            }
            .unwrap_or_else(|| panic!("Source shared task-root producer did not return its carrier"));
            for completion in resources
                .retire()
                .unwrap_or_else(|error| panic!("task-root Source resource retirement failed: {error:?}"))
            {
                if let Err(completion) =
                    ::jet_jit::SourceExecutionCompletionScope::record_current(completion)
                {
                    panic!("task-root parent produced an unrouted completion: {completion:?}");
                }
            }
            assert!(
                resources
                    .is_retirement_requested()
                    .unwrap_or_else(|error| panic!("cannot inspect Source retirement request: {error}")),
                "Source parent retirement request was not retained"
            );
            assert!(
                !shared_arena
                    .is_retired()
                    .unwrap_or_else(|error| panic!("cannot inspect Source arena retirement: {error}")),
                "the retained task root did not defer parent retirement"
            );
            assert!(
                resources
                    .retained_root_count()
                    .unwrap_or_else(|error| panic!("cannot inspect Source task roots: {error}"))
                    > 0,
                "the deferred Source parent lost its counted task root"
            );
            drop(shared_parent_lease);
            assert!(
                !shared_arena
                    .is_retired()
                    .unwrap_or_else(|error| panic!("cannot inspect Source arena retirement: {error}")),
                "dropping the producer alias retired the parent while B still owned its root"
            );
            assert!(
                resources
                    .retained_root_count()
                    .unwrap_or_else(|error| panic!("cannot inspect Source task roots: {error}"))
                    > 0,
                "the consumer task root did not retain parent resources"
            );
            let shared_consumer_ok = {
                let _activation = shared_child_lease.activate();
                crate::__jet_bootstrap_task_roots_fixture_shared_consume(
                    &mut source_program,
                    &mut runtime_config,
                    shared_carrier,
                )
                .unwrap_or_else(|error| panic!("task-root shared consumer arguments do not encode: {error}"))
            };
            assert!(
                shared_consumer_ok,
                "Source consumer did not finish/finalize the transferred shared root"
            );
            drop(shared_child_lease);
            drop(runtime_config);
            drop(source_program);
            shared_consumer_ok
        });
        let mut finalizer_completions = 0;
        let mut outer_b_finished = false;
        loop {
            match completion_owner
                .finish()
                .unwrap_or_else(|error| panic!("outer B Shared completion finish failed: {error:?}"))
            {
                crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Ready(completions) => {
                    for completion in completions {
                        assert_shared_finalizer_completed(
                            completion,
                            shared_finalizer,
                            result.mir.as_ref().expect("checked MIR remains owned by the compile result"),
                            &shared_finalizer_return_type,
                        );
                        finalizer_completions += 1;
                    }
                }
                crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Pending {
                    retirement_requested,
                    retained_roots,
                    open_callback_sessions,
                    pending_jobs,
                } => panic!(
                    "outer B retained Shared work after consumption: retirement_requested={retirement_requested}, retained_roots={retained_roots}, open_callback_sessions={open_callback_sessions}, pending_jobs={pending_jobs}"
                ),
                crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Complete { failures } => {
                    assert!(failures.is_empty(), "outer B callback jobs failed: {failures:?}");
                    outer_b_finished = true;
                    break;
                }
        }
        }
        assert!(shared_consumer_ok, "Source shared consumer did not complete");
        assert_eq!(
            finalizer_completions,
            1,
            "outer B did not receive exactly the late A shared-payload finalizer receipt"
        );
        assert!(outer_b_finished, "outer B did not finish after its completion was consumed");
        let shared_payload_finalizer_completed = finalizer_completions == 1 && outer_b_finished;
        assert!(
            shared_payload_finalizer_completed,
            "outer B did not complete the exact A finalizer obligation"
        );
        assert_eq!(
            resources
                .retained_root_count()
                .unwrap_or_else(|error| panic!("cannot inspect final task-root resource leases: {error}")),
            0,
            "task-root Source resource lease leaked"
        );
        assert!(
            shared_arena
                .is_retired()
                .unwrap_or_else(|error| panic!("cannot inspect final Source arena retirement: {error}")),
            "deferred parent did not retire after the consumer released its root"
        );
        std::fs::write(&output_path, "task-roots=passed\n")
            .unwrap_or_else(|error| panic!("cannot write task-root result: {error}"));
        std::fs::write(
            &receipt_path,
            format!(
                "mode=task-roots\nstatus=passed\nselected_factory_tier={:?}\nactual_factory_tier={:?}\ncallback_capture_params=1\ncallback_escapes=true\ncallback_moves=0\ncursor_close_count=1\nshared_producer_machine_retired=true\nshared_parent_retirement_requested=true\nshared_parent_retirement_deferred=true\nshared_consumer_completed={}\nshared_payload_finalizer_completed={}\nretained_roots=0\n",
                result.selected_factory_tier,
                result.actual_factory_tier,
                shared_consumer_ok,
                shared_payload_finalizer_completed,
            ),
        )
            .unwrap_or_else(|error| panic!("cannot write task-root receipt: {error}"));
        return;
    }
    // bootstrap:task-roots-end
    if mode == "task-roots" {
        panic!("this compiler artifact was built without the task-root fixture");
    }
    let (complete, source, callable_count, type_count, field_count, variant_count, reports, selected_tier, actual_tier) =
        if mode == "factory" {
            let compiler_image = crate::__jet_bootstrap_compiler_image_envelope()
                .unwrap_or_else(|error| panic!("factory compiler image envelope failed: {error}"));
            let execution = crate::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(
                compiler_image.artifact,
            );
            let completion_scope = ::jet_jit::SourceExecutionCompletionScope::new();
            let mut completion_owner =
                crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner::new(
                    completion_scope.clone(),
                );
            let mut result = completion_scope
                .with_current(|| {
                    crate::compiler_bootstrap_runner::invoke_bootstrap_entry(
                        &lease,
                        |snapshot| crate::__jet_bootstrap_compile_from_host(
                            snapshot,
                            crate::__jet_bootstrap_native_compile_target(),
                            factory_tier,
                            &execution,
                            &mut completion_owner,
                        ),
                    )
                })
                .unwrap_or_else(|error| panic!("generated Jet factory authority call failed: {error:?}"))
                .unwrap_or_else(|error| panic!("generated Jet compiler codec failed: {error:?}"));
            if let Some(resources) = result.resources.take() {
                completion_owner.set_resources(resources);
            }
            drop(result.runtime_config.take());
            completion_owner.allow_resource_retirement();
            finish_source_completion_owner(&completion_owner);
            let counts = result.bindings.as_ref().map_or((0, 0, 0, 0), |bindings| {
                (
                    bindings.callables.len(),
                    bindings.types.len(),
                    bindings.fields.len(),
                    bindings.variants.len(),
                )
            });
            (
                result.complete,
                result.emitted_source,
                counts.0,
                counts.1,
                counts.2,
                counts.3,
                result.reports,
                result.selected_factory_tier,
                result.actual_factory_tier,
            )
        } else if mode == "runner" {
            let compiler_image = crate::__jet_bootstrap_compiler_image_envelope()
                .unwrap_or_else(|error| panic!("stage-zero compiler image envelope failed: {error}"));
            let execution = crate::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(
                compiler_image.artifact,
            );
            let config = crate::Codegen::MIRRust::MirRustConfig {
                target: ::jet_foundation::Layout::TargetLayout::host(),
                target_kind: crate::Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: "crate::".to_string(),
                execution,
            };
            fn assert_source_completion_retired(
                completion: ::jet_jit::SourceExecutionCompletion,
            ) {
                match completion.disposition {
                    ::jet_jit::SourceExecutionCompletionDisposition::Invoked {
                        retirement: Some(
                            ::jet_jit::SourceExecutionCompletionRetirement::Completed(retirement),
                        ),
                        ..
                    } => {
                        assert!(
                            matches!(
                                retirement.outcome.as_ref(),
                                Some(::jet_foundation::JitBackend::RunOutcome::Ran { .. })
                            ),
                            "Runner returned a failed source execution completion: {retirement:?}",
                        );
                        for nested in retirement.completions {
                            assert_source_completion_retired(nested);
                        }
                    }
                    disposition => panic!(
                        "Runner returned an incomplete source execution completion: {disposition:?}"
                    ),
                }
            }
            let mut result = match crate::__jet_bootstrap_run_from_host(
                &lease,
                &config,
                factory_tier,
                |snapshot, tier, execution, completion_owner| crate::__jet_bootstrap_compile_from_host(
                    snapshot,
                    crate::__jet_bootstrap_native_compile_target(),
                    tier,
                    execution,
                    completion_owner,
                ),
                |artifact| match artifact {
                    crate::BootstrapBackendArtifact::NativeRust { source, compiler_image, .. } => {
                        // A compiler artifact embeds its image via `include_bytes!("compiler.image")`;
                        // the image travels beside the emitted source as `<output>.image`.
                        let image_path = std::path::Path::new(&output_path).with_extension("image");
                        match compiler_image {
                            Some(image) => std::fs::write(&image_path, image)
                                .unwrap_or_else(|error| panic!("cannot write generated compiler image: {error}")),
                            None => match std::fs::remove_file(&image_path) {
                                Ok(()) => {}
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                                Err(error) => panic!("cannot remove stale generated compiler image: {error}"),
                            },
                        }
                        source
                    }
                    crate::BootstrapBackendArtifact::Web { .. } => panic!("Runner fixture unexpectedly produced a Web artifact"),
                },
                None,
            ) {
                Ok(result) => result,
                Err(error) => {
                    loop {
                        match error.finish_source_completions() {
                            Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Ready(completions)) => {
                                for completion in completions {
                                    assert_source_completion_retired(completion);
                                }
                            }
                            Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Pending {
                                retirement_requested,
                                retained_roots,
                                open_callback_sessions,
                                pending_jobs,
                            }) => panic!(
                                "failed Runner result retained Source work: retirement_requested={retirement_requested}, retained_roots={retained_roots}, open_callback_sessions={open_callback_sessions}, pending_jobs={pending_jobs}; error={error:?}"
                            ),
                            Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Complete { failures }) => {
                                assert!(failures.is_empty(), "failed Runner callback jobs failed: {failures:?}");
                                break;
                            }
                            Err(finish_error) => {
                                panic!("failed Runner result could not finish Source work: {finish_error:?}; error={error:?}")
                            }
                        }
                    }
                    panic!("generated Jet Runner failed: {error:?}");
                }
            };
            drop(result.value.take());
            drop(result.runtime_config.take());
            loop {
                match result.finish_source_completions() {
                    Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Ready(completions)) => {
                        for completion in completions {
                            assert_source_completion_retired(completion);
                        }
                    }
                    Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Pending {
                        retirement_requested,
                        retained_roots,
                        open_callback_sessions,
                        pending_jobs,
                    }) => panic!(
                        "generated Jet Runner returned before Source owners retired: retirement_requested={retirement_requested}, retained_roots={retained_roots}, open_callback_sessions={open_callback_sessions}, pending_jobs={pending_jobs}"
                    ),
                    Ok(crate::compiler_bootstrap_runner::BootstrapRunCompletionFinish::Complete { failures }) => {
                        assert!(failures.is_empty(), "generated Jet Runner callback jobs failed: {failures:?}");
                        break;
                    }
                    Err(error) => panic!("generated Jet Runner Source completion finish failed: {error:?}"),
                }
            }
            (
                result.complete,
                result.backend,
                0,
                0,
                0,
                0,
                result.reports,
                result.selected_factory_tier,
                result.actual_factory_tier,
            )
        } else {
            panic!("unknown private bootstrap mode `{mode}`")
        };

    let source_bytes = source.as_ref().map_or(0, String::len);
    if let Some(source) = source {
        std::fs::write(&output_path, &source)
            .unwrap_or_else(|error| panic!("cannot write generated bootstrap source: {error}"));
    }
    let mut receipt = format!(
        "mode={mode}\ncomplete={complete}\nsource_bytes={source_bytes}\ncallables={callable_count}\ntypes={type_count}\nfields={field_count}\nvariants={variant_count}\nreport_count={}\n",
        reports.len(),
    );
    receipt.push_str(&format!("selected_factory_tier={selected_tier:?}\nactual_factory_tier={actual_tier:?}\n"));
    for report in reports {
        receipt.push_str("report_json=");
        receipt.push_str(&report.json());
        receipt.push('\n');
    }
    receipt.push_str("compiler_identity=");
    receipt.push_str(option_env!("JET_COMPILER_BUILD_ID").unwrap_or("unavailable"));
    receipt.push('\n');
    std::fs::write(&receipt_path, receipt)
        .unwrap_or_else(|error| panic!("cannot write generated bootstrap receipt: {error}"));

}
"#;

/// Stage zero: the Rust reference frontend checks the assembled Jet compiler
/// unit once, emits it as Rust, and the Host/Runner splice makes `jetc0`, the
/// first Jet-built compiler binary.
struct StageZero {
    session: PathBuf,
    compiler_project: PathBuf,
    source_lease: crate::AuthorizedSourceLease,
    project: PathBuf,
    binary: PathBuf,
    id: String,
}

/// `task_roots` appends the task-root fixture to the compiler unit and keeps
/// the `task-roots` harness mode; hello-world proofs build without it.
fn build_stage_zero(repo: &Path, task_roots: bool) -> StageZero {
    let session = bootstrap_session_root();
    fs::create_dir_all(&session).unwrap_or_else(|error| {
        panic!(
            "cannot create private bootstrap cache `{}`: {error}",
            session.display()
        )
    });

    assemble_compiler_sources(repo, task_roots);
    let compiler_project = home_path().join(BOOTSTRAP_PROJECT_RELATIVE);
    let compiler_entry = compiler_project.join(BOOTSTRAP_ENTRY_RELATIVE);
    assert!(
        compiler_entry.is_file(),
        "the assembler did not produce the canonical compiler entry `{}`",
        compiler_entry.display()
    );

    let source_lease = crate::open_authorized_sources(
        &compiler_project,
        Path::new(BOOTSTRAP_ENTRY_RELATIVE),
    )
    .unwrap_or_else(|error| panic!("cannot authorize compiler sources: {error:?}"));
    let compiler_snapshot = source_lease.snapshot().clone();
    let input_stamp = stage_zero_input_stamp(&compiler_snapshot, task_roots);
    let keep = std::env::var_os("JET_STAGE_ZERO_KEEP").map(PathBuf::from);
    // A kept backend project survives a failed backend build, so the emitted
    // Rust can be re-checked (`cargo check --manifest-path <keep>/stage-zero/
    // Cargo.toml`) without re-running the frontend, and
    // `Tools/stage0-repack/repack.sh <keep>` reruns only the Host/Runner
    // packaging after a generator edit.
    let stage_zero_project = keep
        .as_ref()
        .map_or_else(|| session.join("stage-zero"), |keep| keep.join("stage-zero"));
    let (stage_zero_source, stage_zero_image, stage_zero_ffi) =
        match resumable_stage_zero_source(&input_stamp) {
            Some(resumed) => resumed,
            None => {
                let (source, image, ffi) = emit_stage_zero_source(&source_lease, &compiler_snapshot);
                (append_generated_artifact_main(source, task_roots), image, ffi)
            }
        };
    if let Some(keep) = &keep {
        retain_stage_zero_source(
            keep,
            &stage_zero_source,
            &stage_zero_image,
            stage_zero_ffi.as_ref(),
            &compiler_snapshot,
            &input_stamp,
        );
    }
    // Debug hook for the bootstrap loop: stop once the packaged source is
    // retained, so packaging and the emitted Rust can be checked offline
    // without spending a backend build.
    if std::env::var_os("JET_STAGE_ZERO_STOP_AFTER_EMIT").is_some() {
        panic!("stage zero stopped after emit (JET_STAGE_ZERO_STOP_AFTER_EMIT); source retained");
    }
    let (stage_zero_binary, stage_zero_id) = build_backend_artifact_with_native_library(
        repo,
        &stage_zero_project,
        "jet_bootstrap_stage_zero",
        &stage_zero_source,
        Some(stage_zero_image.as_slice()),
        None,
        stage_zero_ffi.as_ref(),
    );
    retain_stage_zero(
        repo,
        &session,
        &stage_zero_project,
        &stage_zero_binary,
        &stage_zero_id,
        task_roots,
    );
    StageZero {
        session,
        compiler_project,
        source_lease,
        project: stage_zero_project,
        binary: stage_zero_binary,
        id: stage_zero_id,
    }
}

/// Check, lower, emit and package the assembled compiler unit: the frontend
/// half of stage zero, whose product is the backend Rust source, the compiler
/// image that source embeds from `compiler.image`, and the prepared FFI bridge
/// that source names.
fn emit_stage_zero_source(
    source_lease: &crate::AuthorizedSourceLease,
    compiler_snapshot: &crate::AuthorizedSourceSnapshot,
) -> (String, Vec<u8>, Option<BackendFfiCrate>) {
    let source_closure = compiler_snapshot
        .roots
        .iter()
        .flat_map(|root| root.files.iter())
        .filter(|file| file.relative_path.ends_with(".jet"))
        .map(|file| (PathBuf::from(&file.path), file.source.clone()))
        .collect::<Vec<_>>();
    source_lease
        .revalidate()
        .unwrap_or_else(|error| panic!("compiler source authority changed before stage zero: {error:?}"));

    // Stage zero is the sole Rust-reference frontend invocation. It consumes
    // the authority-selected source bytes, then only the canonical MIR Rust
    // adapter and native Host/Runner splice are used to make the artifact.
    // The compiler unit has no `fn run`: the Host/Runner splice supplies the
    // Rust `main`, so the unit lowers as a `NativeLibrary` (D4), never as an
    // executable without an entry.
    let (bundle, _lints, reference_ffi) =
        crate::Driver::check_bundle_path_with_source_closure_for_artifact(
            &compiler_snapshot.entry_path,
            crate::Sema::CompileMode::Check,
            "dev",
            &source_closure,
        )
        .unwrap_or_else(|diagnostics| {
            panic!("stage-zero Rust frontend rejected compiler sources: {diagnostics:?}")
        });
    source_lease
        .revalidate()
        .unwrap_or_else(|error| panic!("compiler source authority changed after stage zero: {error:?}"));
    assert!(
        !bundle.build_facts.target_triple.is_empty(),
        "the stage-zero checked bundle has no selected target"
    );
    let ffi_crate = reference_ffi.as_ref().map(|link| BackendFfiCrate {
        name: link.crate_name.clone(),
        dir: crate::FFI::bridge_crate_dir(link),
    });

    // Lowering, MIR Rust emission and Host/Runner packaging of the whole
    // compiler recurse deeper than a test thread's stack; run them on the
    // compiler stack the Driver uses for the same work.
    let bundle = &bundle;
    let (source, image) = crate::with_compiler_stack(move || {
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeLibrary,
            MirArtifactBuildMode::Dev,
        );
        let mut lower_span = jet_driver::Trace::span("lower");
        let (mir, artifact) = crate::lower_checked_semantic_mir_program_for(bundle, request);
        lower_span.items(mir.functions.len());
        drop(lower_span);
        assert_evaluator_materialization_helpers_in_mir(&mir);
        let mut execution = MirRustExecutionConfig::for_artifact(artifact);
        execution.ffi = reference_ffi.as_ref();
        execution.emit_types = true;
        execution.emit_foreign = true;
        execution.emit_metadata = false;
        execution.emit_runtime = true;
        execution.prune_unreachable_codecs = true;
        execution.release_devtools_policy =
            crate::Driver::release_devtools_policy_for_bundle(bundle, "dev");
        let metadata_config = MirRustConfig {
            target: TargetLayout::from_build_facts(&bundle.build_facts),
            target_kind: MirRustTarget::Native,
            root_prefix: String::new(),
            execution: execution.clone(),
        };
        let mut emit_span = jet_driver::Trace::span("emit");
        let reference_rust = crate::Codegen::MIRRust::emit_mir_program(&mir, &metadata_config);
        emit_span.items(mir.functions.len());
        drop(emit_span);
        // A kept run also retains the unpackaged emitted Rust, so a Host/Runner
        // packaging failure still leaves the compiler's Rust to check.
        if let Some(keep) = std::env::var_os("JET_STAGE_ZERO_KEEP").map(PathBuf::from) {
            fs::create_dir_all(&keep).unwrap_or_else(|error| {
                panic!("cannot create stage-zero keep directory `{}`: {error}", keep.display())
            });
            let target = keep.join("reference.rs");
            fs::write(&target, &reference_rust).unwrap_or_else(|error| {
                panic!("cannot write retained stage-zero `{}`: {error}", target.display())
            });
        }
        let prepare_config = MirRustConfig {
            target: metadata_config.target.clone(),
            target_kind: MirRustTarget::Native,
            root_prefix: "crate::".to_string(),
            execution,
        };
        let _package_span = jet_driver::Trace::span("package");
        let metadata = crate::Codegen::MIRRust::mir_rust_aot_metadata(&mir, &metadata_config);
        crate::prepare_bootstrap_artifact_from_aot(
            reference_rust,
            &mir,
            compiler_snapshot,
            &prepare_config,
            &metadata,
        )
        .map(|artifact| (artifact.source, artifact.compiler_image))
        .unwrap_or_else(|error| panic!("stage-zero Host/Runner packaging failed: {error}"))
    });
    let image =
        image.unwrap_or_else(|| panic!("stage-zero Host/Runner packaging embedded no compiler image"));
    (source, image, ffi_crate)
}

/// What the stage-zero backend source is a function of: the authorized
/// compiler source bytes, the reference compiler doing the emission (this
/// test binary), and the harness mode.
fn stage_zero_input_stamp(snapshot: &crate::AuthorizedSourceSnapshot, task_roots: bool) -> String {
    let digest = crate::compiler_bootstrap_compiler_image::compiler_image_source_authority_digest(snapshot);
    let source = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let exe = std::env::current_exe()
        .unwrap_or_else(|error| panic!("cannot locate the stage-zero test binary: {error}"));
    let meta = fs::metadata(&exe)
        .unwrap_or_else(|error| panic!("cannot stat `{}`: {error}", exe.display()));
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_nanos());
    format!(
        "source={source}\nhost={}@{}@{modified}\ntask_roots={task_roots}\n",
        exe.display(),
        meta.len()
    )
}

/// `JET_STAGE_ZERO_RESUME=<dir>` reuses the backend source, its compiler image
/// (and the FFI bridge it names) a previous `JET_STAGE_ZERO_KEEP=<dir>` run
/// retained, skipping the frontend, but only when its input stamp matches this
/// run's.
/// `JET_STAGE_ZERO_RESUME_BACKEND_ONLY=1` also accepts a source emitted by a
/// different test binary (same compiler sources and harness mode): for
/// re-running only the backend build after a harness-only change.
fn resumable_stage_zero_source(
    input_stamp: &str,
) -> Option<(String, Vec<u8>, Option<BackendFfiCrate>)> {
    let dir = PathBuf::from(std::env::var_os("JET_STAGE_ZERO_RESUME")?);
    let backend_only = std::env::var_os("JET_STAGE_ZERO_RESUME_BACKEND_ONLY").is_some();
    let compared = |stamp: &str| {
        stamp
            .lines()
            .filter(|line| !(backend_only && line.starts_with("host=")))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let kept_stamp = fs::read_to_string(dir.join("stage-zero.stamp")).ok();
    if kept_stamp.as_deref().map(compared) != Some(compared(input_stamp)) {
        eprintln!(
            "stage zero: `{}` has no source for these inputs; running the frontend",
            dir.display()
        );
        return None;
    }
    let source = fs::read_to_string(dir.join("stage-zero.rs")).ok()?;
    let image = fs::read(dir.join("stage-zero.image")).ok()?;
    let ffi = fs::read_to_string(dir.join("stage-zero.ffi")).ok()?;
    let ffi = BackendFfiCrate::parse(&ffi);
    eprintln!("stage zero: resuming from `{}`", dir.display());
    Some((source, image, ffi))
}

/// Retain the backend source, the compiler image it embeds (`stage-zero.image`,
/// which the backend crate holds as `src/compiler.image`) and its FFI bridge
/// (`stage-zero.ffi`: crate name and directory, empty without a bridge) with its
/// input stamp; the stamp is written last so a half-written source never matches.
/// The authorized compiler sources go to `compiler-project/`, so
/// `Tools/stage0-repack` can repackage this run after the shared assembly
/// directory has moved on (the image binds their authority digest).
fn retain_stage_zero_source(
    keep: &Path,
    source: &str,
    image: &[u8],
    ffi: Option<&BackendFfiCrate>,
    compiler_snapshot: &crate::AuthorizedSourceSnapshot,
    input_stamp: &str,
) {
    fs::create_dir_all(keep).unwrap_or_else(|error| {
        panic!("cannot create stage-zero keep directory `{}`: {error}", keep.display())
    });
    let _ = fs::remove_file(keep.join("stage-zero.stamp"));
    let sources = keep.join("compiler-project");
    match fs::remove_dir_all(&sources) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("cannot clear retained compiler sources `{}`: {error}", sources.display()),
    }
    for file in compiler_snapshot
        .roots
        .iter()
        .flat_map(|root| root.files.iter().chain(&root.foreign_cache_files))
    {
        let target = sources.join(&file.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("cannot create retained source directory `{}`: {error}", parent.display())
            });
        }
        fs::write(&target, &file.source).unwrap_or_else(|error| {
            panic!("cannot write retained compiler source `{}`: {error}", target.display())
        });
    }
    let ffi = ffi.map_or_else(String::new, |ffi| format!("{}\n{}\n", ffi.name, ffi.dir.display()));
    for (name, bytes) in [
        ("stage-zero.rs", source.as_bytes()),
        ("stage-zero.image", image),
        ("stage-zero.ffi", ffi.as_bytes()),
        ("stage-zero.stamp", input_stamp.as_bytes()),
    ] {
        let target = keep.join(name);
        fs::write(&target, bytes).unwrap_or_else(|error| {
            panic!("cannot write retained stage-zero `{}`: {error}", target.display())
        });
    }
}

/// `JET_STAGE_ZERO_KEEP=<dir>` retains `jetc0` outside the timestamped
/// session and the shared backend target, which later builds overwrite, so
/// stage-one scripts can drive it without rebuilding stage zero. Beside the
/// binary it keeps the backend Cargo manifest and build script (the template
/// for building later stages), the generated-artifact `main` matching this
/// build, and a `jetc0.env` manifest.
fn retain_stage_zero(
    repo: &Path,
    session: &Path,
    project: &Path,
    binary: &Path,
    id: &str,
    task_roots: bool,
) {
    let Some(keep) = std::env::var_os("JET_STAGE_ZERO_KEEP").map(PathBuf::from) else {
        return;
    };
    fs::create_dir_all(&keep).unwrap_or_else(|error| {
        panic!("cannot create stage-zero keep directory `{}`: {error}", keep.display())
    });
    let staged = keep.join("jetc0.partial");
    fs::copy(binary, &staged).unwrap_or_else(|error| {
        panic!("cannot copy `{}` to `{}`: {error}", binary.display(), staged.display())
    });
    fs::rename(&staged, keep.join("jetc0")).unwrap_or_else(|error| {
        panic!("cannot publish retained jetc0 in `{}`: {error}", keep.display())
    });
    for name in ["Cargo.toml", "build.rs"] {
        let target = keep.join(format!("backend-{name}"));
        fs::copy(project.join(name), &target).unwrap_or_else(|error| {
            panic!("cannot retain stage-zero `{name}` as `{}`: {error}", target.display())
        });
    }
    let retained = [
        (
            "artifact-main.rs",
            append_generated_artifact_main(String::new(), task_roots),
        ),
        (
            "jetc0.env",
            format!(
                "repo={}\nsession={}\nid={id}\ntask_roots={task_roots}\npackage=jet_bootstrap_stage_zero\n",
                repo.display(),
                session.display(),
            ),
        ),
    ];
    for (name, text) in retained {
        let target = keep.join(name);
        fs::write(&target, text).unwrap_or_else(|error| {
            panic!("cannot write retained stage-zero `{}`: {error}", target.display())
        });
    }
}

/// The shortest self-hosting proof: `jetc0` compiles hello world, the Rust
/// backend builds the emitted source, and the program prints.
#[test]
fn bootstrap_stage_zero_hello() {
    let repo = Path::new(crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT);
    let stage_zero = build_stage_zero(repo, false);
    let hello_project = stage_zero.session.join("hello");
    write_source_fixture_project(
        &hello_project,
        SOURCE_FIXTURE_MANIFEST,
        "fn run() {\n    print(\"hello\")\n}\n",
    );
    compile_and_run_source_fixture(
        &stage_zero.binary,
        repo,
        &stage_zero.session,
        "jetc0_hello",
        &hello_project,
        "hello\n",
    );
}

/// The backend project `build_backend_artifact_with_native_library` builds,
/// written without building it, so later stages built outside `cargo test`
/// (the stage-one scripts) use exactly the harness layout: unit-split
/// workspace, compiler image, manifest and identity build script.
/// `JET_BACKEND_PROJECT_REPO` is the tree the manifest and build script name;
/// `JET_BACKEND_PROJECT_DIR`, `_PACKAGE` and `_SOURCE` name the project, its
/// package and the emitted source. `<source>.image` is the compiler image the
/// source embeds and `<source>.ffi` the FFI bridge it names (a retained
/// `stage-zero.ffi`), each when present: generated compilers emit with no
/// bridge, as the self-compile harness builds stage one and two.
#[test]
#[ignore = "writes the backend project named by JET_BACKEND_PROJECT_*"]
fn bootstrap_backend_project_from_env() {
    let var = |name: &str| {
        std::env::var(name).unwrap_or_else(|error| panic!("`{name}` is unavailable: {error}"))
    };
    let source_path = PathBuf::from(var("JET_BACKEND_PROJECT_SOURCE"));
    let source = fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!("cannot read backend source `{}`: {error}", source_path.display())
    });
    let image = fs::read(source_path.with_extension("image")).ok();
    let ffi = fs::read_to_string(source_path.with_extension("ffi"))
        .ok()
        .and_then(|record| BackendFfiCrate::parse(&record));
    write_backend_project(
        Path::new(&var("JET_BACKEND_PROJECT_REPO")),
        Path::new(&var("JET_BACKEND_PROJECT_DIR")),
        &var("JET_BACKEND_PROJECT_PACKAGE"),
        &source,
        image.as_deref(),
        None,
        ffi.as_ref(),
    );
}

#[test]
fn bootstrap_private_self_compile_harness() {
    let repo = Path::new(crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT);
    let StageZero {
        session,
        compiler_project,
        source_lease,
        project: stage_zero_project,
        binary: stage_zero_binary,
        id: stage_zero_id,
    } = build_stage_zero(repo, true);
    let task_roots_output = session.join("task-roots.out");
    let task_roots_receipt = session.join("task-roots.receipt");
    run_generated_artifact(
        &stage_zero_binary,
        "task-roots",
        &compiler_project,
        BOOTSTRAP_ENTRY_RELATIVE,
        &task_roots_output,
        &task_roots_receipt,
    );
    assert_task_roots_receipt(&task_roots_output, &task_roots_receipt);

    let small_project = session.join("small-program");
    let small_entry = write_small_program(&small_project);
    assert_optional_codec_roundtrip(&stage_zero_binary, &small_project, &session, "stage-zero");
    let stage_zero_small_source = session.join("stage-zero-small.rs");
    let stage_zero_small_receipt = session.join("stage-zero-small.receipt");
    run_generated_artifact(
        &stage_zero_binary,
        "factory",
        &small_project,
        SMALL_ENTRY_RELATIVE,
        &stage_zero_small_source,
        &stage_zero_small_receipt,
    );
    assert_factory_receipt(&stage_zero_small_receipt);
    let stage_zero_small_rust = fs::read_to_string(&stage_zero_small_source).unwrap_or_else(|error| {
        panic!(
            "stage-zero factory did not produce `{}`: {error}",
            stage_zero_small_source.display()
        )
    });
    let stage_zero_small_project = session.join("stage-zero-small-backend");
    let (stage_zero_small_binary, _) = build_backend_artifact(
        repo,
        &stage_zero_small_project,
        "jet_bootstrap_small_stage_zero",
        &stage_zero_small_rust,
    );
    let image_reuse_output = session.join("user-image-reuse.rs");
    let image_reuse_receipt = session.join("user-image-reuse.receipt");
    run_generated_artifact(
        &stage_zero_small_binary,
        "factory",
        &small_project,
        SMALL_ENTRY_RELATIVE,
        &image_reuse_output,
        &image_reuse_receipt,
    );
    assert_factory_receipt(&image_reuse_receipt);
    assert_small_program_runs(&stage_zero_small_binary, "stage zero");

    source_lease.revalidate().unwrap_or_else(|error| {
        panic!("compiler source authority changed before stage-zero self-source: {error:?}")
    });
    let stage_one_raw = session.join("stage-one.rs");
    let stage_one_receipt = session.join("stage-one.receipt");
    run_generated_artifact(
        &stage_zero_binary,
        "runner",
        &compiler_project,
        BOOTSTRAP_ENTRY_RELATIVE,
        &stage_one_raw,
        &stage_one_receipt,
    );
    source_lease.revalidate().unwrap_or_else(|error| {
        panic!("compiler source authority changed after stage-zero self-source: {error:?}")
    });
    assert_runner_receipt(&stage_one_receipt);
    let stage_one_source = append_generated_artifact_main(
        fs::read_to_string(&stage_one_raw).unwrap_or_else(|error| {
            panic!("stage-zero Runner did not produce stage one source: {error}")
        }),
        true,
    );
    let stage_one_image = generated_compiler_image(&stage_one_raw);
    let stage_one_project = session.join("stage-one");
    let (stage_one_binary, stage_one_id) = build_backend_artifact_with_native_library(
        repo,
        &stage_one_project,
        "jet_bootstrap_stage_one",
        &stage_one_source,
        Some(stage_one_image.as_slice()),
        None,
        None,
    );
    assert_ne!(
        stage_zero_id, stage_one_id,
        "distinct generated compiler artifacts must not reuse one identity"
    );
    assert_optional_codec_roundtrip(&stage_one_binary, &small_project, &session, "stage-one");

    let stage_one_small_source = session.join("stage-one-small.rs");
    let stage_one_small_receipt = session.join("stage-one-small.receipt");
    run_generated_artifact(
        &stage_one_binary,
        "factory",
        &small_project,
        SMALL_ENTRY_RELATIVE,
        &stage_one_small_source,
        &stage_one_small_receipt,
    );
    assert_factory_receipt(&stage_one_small_receipt);
    let stage_one_small_rust = fs::read_to_string(&stage_one_small_source).unwrap_or_else(|error| {
        panic!(
            "stage-one factory did not produce `{}`: {error}",
            stage_one_small_source.display()
        )
    });
    let stage_one_small_project = session.join("stage-one-small-backend");
    let (stage_one_small_binary, _) = build_backend_artifact(
        repo,
        &stage_one_small_project,
        "jet_bootstrap_small_stage_one",
        &stage_one_small_rust,
    );
    assert_small_program_runs(&stage_one_small_binary, "stage one");

    source_lease.revalidate().unwrap_or_else(|error| {
        panic!("compiler source authority changed before stage-one self-source: {error:?}")
    });
    let stage_two_raw = session.join("stage-two.raw.rs");
    let stage_two_receipt = session.join("stage-two.receipt");
    run_generated_artifact(
        &stage_one_binary,
        "runner",
        &compiler_project,
        BOOTSTRAP_ENTRY_RELATIVE,
        &stage_two_raw,
        &stage_two_receipt,
    );
    source_lease.revalidate().unwrap_or_else(|error| {
        panic!("compiler source authority changed after stage-one self-source: {error:?}")
    });
    assert_runner_receipt(&stage_two_receipt);
    let stage_two_source = session.join("stage-two.rs");
    let stage_two_source_text = append_generated_artifact_main(
        fs::read_to_string(&stage_two_raw).unwrap_or_else(|error| {
            panic!("stage-one Runner did not produce stage-two source: {error}")
        }),
        true,
    );
    fs::write(&stage_two_source, &stage_two_source_text).unwrap_or_else(|error| {
        panic!("cannot retain stage-two source `{}`: {error}", stage_two_source.display())
    });
    let stage_two_image = generated_compiler_image(&stage_two_raw);
    fs::write(stage_two_source.with_extension("image"), &stage_two_image).unwrap_or_else(|error| {
        panic!("cannot retain stage-two compiler image beside `{}`: {error}", stage_two_source.display())
    });
    let stage_two_project = session.join("stage-two");
    let (stage_two_binary, stage_two_id) = build_backend_artifact_with_native_library(
        repo,
        &stage_two_project,
        "jet_bootstrap_stage_two",
        &stage_two_source_text,
        Some(stage_two_image.as_slice()),
        None,
        None,
    );
    assert_ne!(
        stage_one_id, stage_two_id,
        "distinct generated compiler artifacts must not reuse one identity"
    );
    assert_optional_codec_roundtrip(&stage_two_binary, &small_project, &session, "stage-two");
    assert_mir_optimizer_compiler_source(&stage_two_binary, &compiler_project, &session);
    assert_mir_optimizer_fixtures(&stage_two_binary, repo, &session);
    // D-PATTERN-HOLE-NAME1=A: both checkers publish the same hole refusal,
    // source span, and contextual repair, before choosing an execution tier.
    for position in ["if", "value", "route", "bytes", "or", "and", "optional", "constant", "typed"] {
        let source = fs::read_to_string(repo.join(format!(
            "tests/ui/pattern_hole_reuse_{position}.jet"
        ))).unwrap();
        let project = session.join(format!("pattern-hole-reuse-{position}"));
        let entry = write_source_fixture_project(&project, SOURCE_FIXTURE_MANIFEST, &source);
        let output = session.join(format!("pattern-hole-reuse-{position}.rs"));
        let receipt = session.join(format!("pattern-hole-reuse-{position}.receipt"));
        run_generated_artifact(
            &stage_two_binary,
            "runner",
            &project,
            SMALL_ENTRY_RELATIVE,
            &output,
            &receipt,
        );
        assert!(!output.exists(), "a reused hole must not produce backend source");
        let generated = receipt_reports(&receipt)
            .into_iter()
            .filter(|report| report.contains("\"code\":\"E0118\""))
            .collect::<Vec<_>>();
        let reference = rust_reference_reports(&entry, &source, &[(entry.clone(), source.clone())])
            .into_iter()
            .filter(|report| report.contains("\"code\":\"E0118\""))
            .collect::<Vec<_>>();
        assert_eq!(reference.len(), 1, "{position}: {reference:?}");
        assert_eq!(generated, reference, "{position}: checker report parity");
    }
    let pattern_project = session.join("pattern-hole-new-names");
    write_source_fixture_project(
        &pattern_project,
        SOURCE_FIXTURE_MANIFEST,
        include_str!("../../Examples/features/basics/pattern_matching.jet"),
    );
    compile_and_run_source_fixture(
        &stage_two_binary,
        repo,
        &session,
        "pattern_hole_new_names",
        &pattern_project,
        include_str!("../../Examples/features/expected/basics/pattern_matching.out"),
    );

    let invalid_project = session.join("invalid-imported-source");
    let (invalid_entry, invalid_entry_source, imported_source_path, imported_source) =
        write_invalid_import_project(&invalid_project);
    let invalid_output = session.join("invalid-compile.rs");
    let invalid_receipt = session.join("invalid-compile.receipt");
    run_generated_artifact(
        &stage_two_binary,
        "runner",
        &invalid_project,
        SMALL_ENTRY_RELATIVE,
        &invalid_output,
        &invalid_receipt,
    );
    let invalid_receipt_text = fs::read_to_string(&invalid_receipt).unwrap_or_else(|error| {
        panic!("cannot read generated invalid-compile receipt `{}`: {error}", invalid_receipt.display())
    });
    assert!(invalid_receipt_text.lines().any(|line| line == "complete=false"));
    assert_receipt_count_at_least(&invalid_receipt_text, "report_count", 1);
    assert!(
        !invalid_output.exists(),
        "an incomplete generated Jet compile must not produce backend Rust source"
    );
    assert!(
        imported_source.len() > imported_source.chars().count(),
        "invalid source fixture must exercise UTF-8 byte coordinates"
    );
    let end = imported_source.len();
    let generated_report = report_at_imported_eof(
        &receipt_reports(&invalid_receipt),
        &imported_source_path,
        end,
    );
    let source_closure = vec![
        (invalid_entry.clone(), invalid_entry_source.clone()),
        (imported_source_path.clone(), imported_source.clone()),
    ];
    let reference_reports =
        rust_reference_reports(&invalid_entry, &invalid_entry_source, &source_closure);
    let reference_report = report_at_imported_eof(
        &reference_reports,
        &imported_source_path,
        end,
    );
    assert_eq!(
        generated_report, reference_report,
        "generated Jet report for a non-entry Unicode EOF error must match the Rust reference"
    );

    let casing_project = session.join("identifier-casing");
    let (casing_entry, casing_entry_source, casing_closure) =
        write_identifier_casing_project(&casing_project);
    let casing_output = session.join("identifier-casing.rs");
    let casing_receipt = session.join("identifier-casing.receipt");
    run_generated_artifact(
        &stage_two_binary,
        "runner",
        &casing_project,
        SMALL_ENTRY_RELATIVE,
        &casing_output,
        &casing_receipt,
    );
    let generated_casing_reports = receipt_reports(&casing_receipt)
        .into_iter()
        .filter(|report| report.contains("\"code\":\"E0357\""))
        .collect::<Vec<_>>();
    let reference_casing_reports =
        rust_reference_reports(&casing_entry, &casing_entry_source, &casing_closure)
            .into_iter()
            .filter(|report| report.contains("\"code\":\"E0357\""))
            .collect::<Vec<_>>();
    assert_eq!(
        reference_casing_reports.len(),
        8,
        "casing parity fixture must exercise every checked category: {reference_casing_reports:?}"
    );
    assert_eq!(
        generated_casing_reports, reference_casing_reports,
        "generated Jet E0357 reports must match the Rust reference (D-SHAPE-CASE1)"
    );

    let valid_report_project = session.join("valid-report");
    let (valid_entry, valid_entry_source) = write_valid_report_project(&valid_report_project);
    let valid_report_output = session.join("valid-report.rs");
    let valid_report_receipt = session.join("valid-report.receipt");
    run_generated_artifact(
        &stage_two_binary,
        "runner",
        &valid_report_project,
        SMALL_ENTRY_RELATIVE,
        &valid_report_output,
        &valid_report_receipt,
    );
    assert_runner_receipt(&valid_report_receipt);
    assert!(
        valid_report_output.is_file(),
        "a complete generated Jet compile must produce backend Rust source"
    );
    let valid_source_closure = vec![(valid_entry, valid_entry_source)];
    let (valid_entry_path, valid_entry_text) = &valid_source_closure[0];
    let generated_valid_reports = receipt_reports(&valid_report_receipt);
    let reference_valid_reports =
        rust_reference_success_reports(valid_entry_path, valid_entry_text, &valid_source_closure);
    assert!(
        reference_valid_reports.iter().any(|report| report.contains("\"code\":\"L0104\"")),
        "valid parity fixture must exercise a successful compile with a lint report: {reference_valid_reports:?}"
    );
    assert!(
        reference_valid_reports.iter().any(|report| report.contains("\"code\":\"L0619\"")),
        "valid parity fixture must exercise a report with absent source metadata: {reference_valid_reports:?}"
    );
    assert_eq!(
        generated_valid_reports, reference_valid_reports,
        "generated Jet reports for a complete compile must match the Rust reference"
    );

    let closure_project = session.join("nested-closure-callback");
    write_nested_closure_callback_project(&closure_project);
    let closure_output = session.join("nested-closure-callback.rs");
    let closure_receipt = session.join("nested-closure-callback.receipt");
    run_generated_artifact(
        &stage_two_binary,
        "factory",
        &closure_project,
        SMALL_ENTRY_RELATIVE,
        &closure_output,
        &closure_receipt,
    );
    assert_factory_receipt(&closure_receipt);
    assert!(
        closure_output.is_file(),
        "captured lambdas and C callback adapters must survive Pipeline MIR assembly and reach emission"
    );

    let projection_output = session.join("offset-projection.span");
    let projection_receipt = session.join("offset-projection.receipt");
    run_generated_artifact(
        &stage_two_binary,
        "offset-projection",
        &small_project,
        SMALL_ENTRY_RELATIVE,
        &projection_output,
        &projection_receipt,
    );
    assert_eq!(
        fs::read_to_string(&projection_output).unwrap_or_else(|error| {
            panic!("cannot read nonzero-offset projection `{}`: {error}", projection_output.display())
        }),
        "0..4",
        "aggregate offset 1200 must rebase to UTF-8 physical byte span 0..4 in a four-byte file"
    );
    let projection_receipt_text = fs::read_to_string(&projection_receipt).unwrap_or_else(|error| {
        panic!("cannot read nonzero-offset projection receipt `{}`: {error}", projection_receipt.display())
    });
    assert!(projection_receipt_text.lines().any(|line| line == "span=0..4"));

    let stage_two_small_source = session.join("stage-two-small.rs");
    let stage_two_small_receipt = session.join("stage-two-small.receipt");
    run_generated_artifact(
        &stage_two_binary,
        "factory",
        &small_project,
        SMALL_ENTRY_RELATIVE,
        &stage_two_small_source,
        &stage_two_small_receipt,
    );
    assert_factory_receipt(&stage_two_small_receipt);
    let stage_two_small_rust = fs::read_to_string(&stage_two_small_source).unwrap_or_else(|error| {
        panic!(
            "stage-two factory did not produce `{}`: {error}",
            stage_two_small_source.display()
        )
    });
    let stage_two_small_project = session.join("stage-two-small-backend");
    let (stage_two_small_binary, _) = build_backend_artifact(
        repo,
        &stage_two_small_project,
        "jet_bootstrap_small_stage_two",
        &stage_two_small_rust,
    );
    assert_small_program_runs(&stage_two_small_binary, "stage two");

    let carrier_fixture_project = session.join("source-contract-carrier");
    write_source_fixture_project(
        &carrier_fixture_project,
        SOURCE_FIXTURE_MANIFEST,
        CARRIER_FIXTURE_SOURCE,
    );
    compile_and_run_source_fixture(
        &stage_two_binary,
        repo,
        &session,
        "carrier_binding",
        &carrier_fixture_project,
        CARRIER_FIXTURE_EXPECTED,
    );

    for (label, source, expected) in [
        (
            "operator_user_defined",
            USER_OPERATOR_FIXTURE_SOURCE,
            USER_OPERATOR_FIXTURE_EXPECTED,
        ),
        (
            "operator_spaceship",
            SPACESHIP_FIXTURE_SOURCE,
            SPACESHIP_FIXTURE_EXPECTED,
        ),
        (
            "exact_division_runtime",
            EXACT_DIVISION_FIXTURE_ONE,
            EXACT_DIVISION_FIXTURE_ONE_EXPECTED,
        ),
        (
            "exact_division_comptime",
            EXACT_DIVISION_FIXTURE_TWO,
            EXACT_DIVISION_FIXTURE_TWO_EXPECTED,
        ),
        (
            "operator_mixed_types",
            MIXED_OPERATOR_FIXTURE_SOURCE,
            MIXED_OPERATOR_FIXTURE_EXPECTED,
        ),
        (
            "enum_derived_order",
            DERIVED_ORDER_FIXTURE_SOURCE,
            DERIVED_ORDER_FIXTURE_EXPECTED,
        ),
        (
            "display_read_receiver",
            include_str!("../../Examples/features/traits/display_read_receiver.jet"),
            include_str!("../../Examples/features/expected/traits/display_read_receiver.out"),
        ),
        (
            "never_plain_return",
            NEVER_PLAIN_RETURN_FIXTURE_SOURCE,
            NEVER_PLAIN_RETURN_FIXTURE_EXPECTED,
        ),
        (
            "registry_rows",
            REGISTRY_ROWS_FIXTURE_SOURCE,
            REGISTRY_ROWS_FIXTURE_EXPECTED,
        ),
        (
            "annotated_tuple_empty_list",
            include_str!("../../Examples/features/basics/tuple_empty_list.jet"),
            include_str!("../../Examples/features/expected/basics/tuple_empty_list.out"),
        ),
        (
            "prelude_assert_eq_shadow",
            include_str!("../../Examples/features/traits/prelude_assert_eq_shadow.jet"),
            include_str!("../../Examples/features/expected/traits/prelude_assert_eq_shadow.out"),
        ),
        (
            "user_read_dir",
            include_str!("../../Examples/features/traits/user_read_dir.jet"),
            include_str!("../../Examples/features/expected/traits/user_read_dir.out"),
        ),
        (
            "math_copy",
            include_str!("../../Examples/features/math/copy.jet"),
            include_str!("../../Examples/features/expected/math/copy.out"),
        ),
    ] {
        let project = session.join(label);
        write_source_fixture_project(&project, SOURCE_FIXTURE_MANIFEST, source);
        compile_and_run_source_fixture(
            &stage_two_binary,
            repo,
            &session,
            label,
            &project,
            expected,
        );
    }
    let scoped_files_project = session.join("scoped-mapped-files");
    let scoped_files_path = scoped_files_project.join("inside.txt");
    let scoped_files_source = format!(r#"
use core.files as files
fn scoped_read(scope: files.FileScope) -> String {{
    scope.read("inside.txt") ?? panic("scope")
}}
fn run() {{
    scope :: files.scope(Authority.from_rights(["FS.Read:{}"]))
    print(scoped_read(scope))
    mapped :: files.map(Path.from("{}")) ?? panic("map")
    window :: mapped.window_len(0, 1) ?? panic("window")
    print(Int.from_u8(window[0]))
    loop line in mapped.lines() {{
        print(Int.from_u8(line[0]))
        print(String.from_bytes(~line) ?? panic("utf8"))
    }}
}}
"#, scoped_files_project.display(), scoped_files_path.display());
    write_source_fixture_project(&scoped_files_project, SOURCE_FIXTURE_MANIFEST, &scoped_files_source);
    fs::write(&scoped_files_path, "mapped\n").expect("write scoped mapped fixture");
    compile_and_run_source_fixture(
        &stage_two_binary,
        repo,
        &session,
        "scoped-mapped-files",
        &scoped_files_project,
        "mapped\n\n109\n109\nmapped\n",
    );
    for (label, source, code) in [
        (
            "generic-concrete-bound-report",
            "trait Shape { fn area(self) -> Int }\nstruct Token {}\nfn area<T: Shape>(value: T) -> Int { value.area() }\nfn run() { _ :: area<Token>(Token{}) }\n",
            "E0905",
        ),
        (
            "generic-forward-bound-report",
            include_str!("../../tests/ui/generic_bound_forward_missing.jet"),
            "E0905",
        ),
        (
            "retired-raw-directory-report",
            include_str!("../../tests/ui/path_raw_string_error.jet"),
            "E0340",
        ),
    ] {
        let code_field = format!("\"code\":\"{code}\"");
        let project = session.join(label);
        let entry = write_source_fixture_project(&project, SOURCE_FIXTURE_MANIFEST, source);
        let output = session.join(format!("{label}.rs"));
        let receipt = session.join(format!("{label}.receipt"));
        run_generated_artifact(
            &stage_two_binary,
            "runner",
            &project,
            SMALL_ENTRY_RELATIVE,
            &output,
            &receipt,
        );
        assert!(!output.exists(), "invalid fixture reached Rust emission");
        let generated = receipt_reports(&receipt)
            .into_iter()
            .filter(|report| report.contains(&code_field))
            .collect::<Vec<_>>();
        let source_closure = vec![(entry.clone(), source.to_string())];
        let reference = rust_reference_reports(&entry, source, &source_closure)
            .into_iter()
            .filter(|report| report.contains(&code_field))
            .collect::<Vec<_>>();
        assert_eq!(reference.len(), 1, "{code} fixture must fail in sema");
        assert_eq!(generated, reference, "{code} registry reports must agree");
    }
    for receiver in ["^self", "&self"] {
        for inline in [false, true] {
            let method = format!("fn display({receiver}) -> String {{ \"label\" }}");
            let source = if inline {
                format!("struct Label {{\n    text: String\n    impl Display {{ {method} }}\n}}\nfn run() {{}}\n")
            } else {
                format!("struct Label {{ text: String }}\nimpl Label.Display {{ {method} }}\nfn run() {{}}\n")
            };
            let label = format!("display-receiver-{}-{inline}", if receiver == "^self" { "take" } else { "edit" });
            let project = session.join(&label);
            let entry = write_source_fixture_project(&project, SOURCE_FIXTURE_MANIFEST, &source);
            let output = session.join(format!("{label}.rs"));
            let receipt = session.join(format!("{label}.receipt"));
            run_generated_artifact(
                &stage_two_binary,
                "runner",
                &project,
                SMALL_ENTRY_RELATIVE,
                &output,
                &receipt,
            );
            assert!(!output.exists(), "invalid Display receiver reached Rust emission");
            let generated = receipt_reports(&receipt)
                .into_iter()
                .filter(|report| report.contains("\"code\":\"E0907\""))
                .collect::<Vec<_>>();
            let source_closure = vec![(entry.clone(), source.clone())];
            let reference = rust_reference_reports(&entry, &source, &source_closure)
                .into_iter()
                .filter(|report| report.contains("\"code\":\"E0907\""))
                .collect::<Vec<_>>();
            assert_eq!(reference.len(), 1, "Display receiver must fail in sema");
            assert_eq!(generated, reference, "Display receiver reports must agree");
        }
    }
    let partial_move_project = session.join("core-files-partial-move-siblings");
    write_source_fixture_project(
        &partial_move_project,
        SOURCE_FIXTURE_MANIFEST,
        CORE_FILES_PARTIAL_MOVE_SIBLINGS_SOURCE,
    );
    let partial_move_expected_stdout = "true\n".repeat(25);
    compile_and_run_source_fixture(
        &stage_two_binary,
        repo,
        &session,
        "core_files_partial_move_siblings",
        &partial_move_project,
        &partial_move_expected_stdout,
    );
    let uninit_partial_exit_project = session.join("uninit-fixed-partial-exit");
    write_source_fixture_project(
        &uninit_partial_exit_project,
        SOURCE_FIXTURE_MANIFEST,
        UNINIT_FIXED_PARTIAL_EXIT_SOURCE,
    );
    let uninit_partial_exit_expected_stdout = "7\n";
    compile_and_run_source_fixture(
        &stage_two_binary,
        repo,
        &session,
        "uninit_fixed_partial_exit",
        &uninit_partial_exit_project,
        uninit_partial_exit_expected_stdout,
    );



    let handle_lifetime_project = session.join("native-handle-lifetimes");
    let handle_lifetime_manifest = format!(
        "name: \"bootstrap_handle_lifetimes\"\nversion: \"0.1.0\"\nedition: \"2028\"\ndeps: {{ close: c@{:?} }}\noutputs: {{ app: .Executable{{ entry: run }} }}\n",
        handle_lifetime_project.display().to_string(),
    );
    write_source_fixture_project(
        &handle_lifetime_project,
        &handle_lifetime_manifest,
        HANDLE_LIFETIME_FIXTURE_SOURCE,
    );
    build_local_c_provider(
        &handle_lifetime_project,
        "close",
        HANDLE_LIFETIME_C_PROVIDER,
    );
    compile_and_run_source_fixture_with_native_library(
        &stage_two_binary,
        repo,
        &session,
        "native_handle_lifetimes",
        &handle_lifetime_project,
        HANDLE_LIFETIME_FIXTURE_EXPECTED,
        Some((handle_lifetime_project.as_path(), "close")),
    );

    let struct_word_project = session.join("struct_call_word");
    write_source_fixture_project(
        &struct_word_project,
        SOURCE_FIXTURE_MANIFEST,
        include_str!("../../Examples/features/basics/struct_call_word.jet"),
    );
    compile_and_run_source_fixture(
        &stage_two_binary,
        repo,
        &session,
        "struct_call_word",
        &struct_word_project,
        include_str!("../../Examples/features/expected/basics/struct_call_word.out"),
    );

    let provenance = session.join("bootstrap.provenance");
    let provenance_text = format!(
        "schema=jet-private-bootstrap/v1\ncompiler_project={}\nstage_zero_id={stage_zero_id}\nstage_zero_source={}\nstage_one_id={stage_one_id}\nstage_one_source={}\nstage_two_id={stage_two_id}\nstage_two_source={}\nstage_two_backend={}\nstage_two_small_backend={}\nsmall_entry={}\nbackend=Tools/agent/jet-env cargo build --manifest-path <cache-project>/Cargo.toml --bin <cache-binary>\n",
        compiler_project.display(),
        stage_zero_project.join("src/main.rs").display(),
        stage_one_project.join("src/main.rs").display(),
        stage_two_source.display(),
        stage_two_project.join("src/main.rs").display(),
        stage_two_small_project.join("src/main.rs").display(),
        small_entry.display(),
    );
    fs::write(&provenance, provenance_text).unwrap_or_else(|error| {
        panic!("cannot write bootstrap provenance `{}`: {error}", provenance.display())
    });
    assert!(provenance.is_file(), "bootstrap provenance was not retained");
}

fn home_path() -> PathBuf {
    PathBuf::from(
        std::env::var_os("HOME")
            .unwrap_or_else(|| panic!("HOME is required for the private bootstrap cache")),
    )
}

fn bootstrap_session_root() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("system clock is before the Unix epoch: {error}"))
        .as_nanos();
    home_path()
        .join(".cache/jet-test-scratch")
        .join(format!("bootstrap-self-{}-{timestamp}", std::process::id()))
}


fn assemble_compiler_sources(repo: &Path, task_roots: bool) {
    // The assembler runs only node; the test's own CARGO_TARGET_DIR may lie
    // outside `repo`, which jet-env rejects for cargo commands.
    let output = Command::new(repo.join("Tools/agent/jet-env"))
        .current_dir(repo)
        .env_remove("CARGO_TARGET_DIR")
        .args(["node", "Compiler/Bootstrap/assemble.mjs"])
        .output()
        .unwrap_or_else(|error| panic!("cannot run bootstrap assembler: {error}"));
    assert_command_success("bootstrap assembler", &output);
    if !task_roots {
        return;
    }
    let compiler_entry = home_path()
        .join(BOOTSTRAP_PROJECT_RELATIVE)
        .join(BOOTSTRAP_ENTRY_RELATIVE);
    let mut compiler_source = fs::read_to_string(&compiler_entry).unwrap_or_else(|error| {
        panic!(
            "cannot read assembled compiler fixture `{}`: {error}",
            compiler_entry.display()
        )
    });
    compiler_source.push_str("\n\n");
    compiler_source.push_str(TASK_ROOTS_FIXTURE_SOURCE);
    fs::write(&compiler_entry, compiler_source).unwrap_or_else(|error| {
        panic!(
            "cannot add task-root fixture to the private compiler unit `{}`: {error}",
            compiler_entry.display()
        )
    });
}

fn write_small_program(project: &Path) -> PathBuf {
    fs::create_dir_all(project).unwrap_or_else(|error| {
        panic!("cannot create small Jet project `{}`: {error}", project.display())
    });
    let manifest = project.join("package.jet");
    let entry = project.join(SMALL_ENTRY_RELATIVE);
    fs::write(&manifest, SMALL_PROGRAM_MANIFEST).unwrap_or_else(|error| {
        panic!("cannot write small Jet manifest `{}`: {error}", manifest.display())
    });
    fs::write(&entry, SMALL_PROGRAM_SOURCE).unwrap_or_else(|error| {
        panic!("cannot write small Jet source `{}`: {error}", entry.display())
    });
    entry
}

fn write_source_fixture_project(project: &Path, manifest: &str, source: &str) -> PathBuf {
    fs::create_dir_all(project).unwrap_or_else(|error| {
        panic!("cannot create source fixture project `{}`: {error}", project.display())
    });
    let manifest_path = project.join("package.jet");
    let entry = project.join(SMALL_ENTRY_RELATIVE);
    fs::write(&manifest_path, manifest).unwrap_or_else(|error| {
        panic!("cannot write source fixture manifest `{}`: {error}", manifest_path.display())
    });
    fs::write(&entry, source).unwrap_or_else(|error| {
        panic!("cannot write source fixture `{}`: {error}", entry.display())
    });
    entry
}
fn assert_mir_optimizer_fixtures(binary: &Path, repo: &Path, session: &Path) {
    let fixtures = [
        (
            "inline",
            "tests/fixtures/mir_optimizer_inline.jet",
            "inline:11\nfailure:negative\n",
        ),
        (
            "loops",
            "tests/fixtures/mir_optimizer_loops.jet",
            "true true true true false\n",
        ),
        (
            "source_regressions",
            "tests/fixtures/mir_optimizer_source_regressions.jet",
            "unit:body\ntrue true true true true true true\n",
        ),
    ];
    for (name, fixture_path, expected_stdout) in fixtures {
        let fixture_source = fs::read_to_string(repo.join(fixture_path)).unwrap_or_else(|error| {
            panic!("cannot read MIR optimizer fixture `{fixture_path}`: {error}")
        });
        let project = session.join(format!("optimizer-{name}"));
        write_source_fixture_project(&project, SOURCE_FIXTURE_MANIFEST, &fixture_source);
        let generated_source = session.join(format!("optimizer-{name}.rs"));
        let receipt = session.join(format!("optimizer-{name}.receipt"));
        run_generated_artifact(
            binary,
            "factory",
            &project,
            SMALL_ENTRY_RELATIVE,
            &generated_source,
            &receipt,
        );
        assert_factory_receipt(&receipt);
        let generated_source = fs::read_to_string(&generated_source).unwrap_or_else(|error| {
            panic!("cannot read generated MIR optimizer backend for `{name}`: {error}")
        });
        let backend_project = session.join(format!("optimizer-{name}-backend"));
        let (backend_binary, _) = build_backend_artifact(
            repo,
            &backend_project,
            &format!("jet_bootstrap_optimizer_{name}"),
            &generated_source,
        );
        let output = Command::new(&backend_binary)
            .output()
            .unwrap_or_else(|error| panic!("cannot execute MIR optimizer fixture `{name}`: {error}"));
        assert_command_success(&format!("MIR optimizer fixture `{name}`"), &output);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_stdout,
            "generated compiler changed MIR optimizer fixture `{name}` behavior"
        );
    }
}

fn build_local_c_provider(root: &Path, library: &str, source: &str) {
    let source_path = root.join(format!("{library}.c"));
    let object_path = root.join(format!("{library}.o"));
    let archive_path = root.join(format!("lib{library}.a"));
    fs::write(&source_path, source).unwrap_or_else(|error| {
        panic!("cannot write C provider `{}`: {error}", source_path.display())
    });
    let compiler = ["cc", "gcc", "clang"]
        .into_iter()
        .find(|compiler| {
            Command::new(compiler)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .unwrap_or_else(|| panic!("C provider fixture requires cc, gcc, or clang"));
    let compile = Command::new(compiler)
        .arg("-c")
        .arg(&source_path)
        .arg("-o")
        .arg(&object_path)
        .output()
        .unwrap_or_else(|error| panic!("cannot compile C provider `{}`: {error}", source_path.display()));
    assert_command_success("native C provider compile", &compile);
    let archive = Command::new("ar")
        .arg("rcs")
        .arg(&archive_path)
        .arg(&object_path)
        .output()
        .unwrap_or_else(|error| panic!("cannot archive C provider `{}`: {error}", archive_path.display()));
    assert_command_success("native C provider archive", &archive);
}

fn compile_and_run_source_fixture(
    compiler: &Path,
    repo: &Path,
    session: &Path,
    label: &str,
    project: &Path,
    expected_stdout: &str,
) {
    let raw_source = session.join(format!("{label}.raw.rs"));
    let receipt = session.join(format!("{label}.receipt"));
    run_generated_artifact(
        compiler,
        "runner",
        project,
        SMALL_ENTRY_RELATIVE,
        &raw_source,
        &receipt,
    );
    assert_runner_receipt(&receipt);
    let source = fs::read_to_string(&raw_source).unwrap_or_else(|error| {
        panic!("generated compiler did not emit `{}`: {error}", raw_source.display())
    });
    let backend_project = session.join(format!("{label}-backend"));
    let (binary, _) = build_backend_artifact(repo, &backend_project, label, &source);
    let output = Command::new(&binary)
        .output()
        .unwrap_or_else(|error| panic!("cannot execute generated `{label}` fixture: {error}"));
    assert_command_success(&format!("generated `{label}` fixture"), &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout,
        "generated `{label}` fixture output differs from its executable source contract"
    );
}

fn compile_and_run_source_fixture_with_native_library(
    compiler: &Path,
    repo: &Path,
    session: &Path,
    label: &str,
    project: &Path,
    expected_stdout: &str,
    native_library: Option<(&Path, &str)>,
) {
    let raw_source = session.join(format!("{label}.raw.rs"));
    let receipt = session.join(format!("{label}.receipt"));
    run_generated_artifact(
        compiler,
        "runner",
        project,
        SMALL_ENTRY_RELATIVE,
        &raw_source,
        &receipt,
    );
    assert_runner_receipt(&receipt);
    let source = fs::read_to_string(&raw_source).unwrap_or_else(|error| {
        panic!("generated compiler did not emit `{}`: {error}", raw_source.display())
    });
    let backend_project = session.join(format!("{label}-backend"));
    let (binary, _) = build_backend_artifact_with_native_library(
        repo,
        &backend_project,
        label,
        &source,
        None,
        native_library,
        None,
    );
    let output = Command::new(&binary)
        .output()
        .unwrap_or_else(|error| panic!("cannot execute generated `{label}` fixture: {error}"));
    assert_command_success(&format!("generated `{label}` fixture"), &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout,
        "generated `{label}` fixture output differs from its executable source contract"
    );
}

fn assert_evaluator_materialization_helpers_in_mir(mir: &jet_foundation::MIR::MirProgram) {
    for helper_name in [
        "jet_eval_materialize_string_window",
        "jet_eval_materialize_list_window",
        "jet_eval_materialize_bytes_window",
    ] {
        let mut by_name = mir.functions.iter().filter(|function| function.name == helper_name);
        let helper = by_name
            .next()
            .unwrap_or_else(|| panic!("full-manifest MIR lacks evaluator helper `{helper_name}`"));
        assert!(
            by_name.next().is_none(),
            "full-manifest MIR has ambiguous evaluator helper `{helper_name}`"
        );
        let function_id = helper.id;
        let mut by_identity = mir.functions.iter().filter(|function| function.id == function_id);
        let checked_helper = by_identity
            .next()
            .unwrap_or_else(|| panic!("checked evaluator helper `{helper_name}` lost its function ID"));
        assert!(
            by_identity.next().is_none(),
            "checked evaluator helper `{helper_name}` has a duplicate MIR function ID"
        );
        assert!(
            checked_helper.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.operation,
                        MirOperation::Copy {
                            fact: jet_foundation::MIR::MirCopyFact::ViewMaterialize,
                            ..
                        }
                    )
                })
            }),
            "checked evaluator helper `{helper_name}` ({function_id:?}) must contain a materializing Copy"
        );
    }
}
fn write_invalid_import_project(project: &Path) -> (PathBuf, String, PathBuf, String) {
    fs::create_dir_all(project).unwrap_or_else(|error| {
        panic!("cannot create invalid Jet project `{}`: {error}", project.display())
    });
    let manifest = project.join("package.jet");
    let entry = project.join(SMALL_ENTRY_RELATIVE);
    let entry_source = "use \"broken\" as broken\npub fn main() {\n    print(\"entry\")\n}\n".to_string();
    let imported_source = "pub fn broken() { print(\"🧪\")".to_string();
    let imported = project.join("broken.jet");
    fs::write(&manifest, SMALL_PROGRAM_MANIFEST).unwrap_or_else(|error| {
        panic!("cannot write invalid Jet manifest `{}`: {error}", manifest.display())
    });
    fs::write(&entry, &entry_source).unwrap_or_else(|error| {
        panic!("cannot write invalid Jet entry `{}`: {error}", entry.display())
    });
    fs::write(&imported, &imported_source).unwrap_or_else(|error| {
        panic!("cannot write invalid imported Jet source `{}`: {error}", imported.display())
    });
    (entry, entry_source, imported, imported_source)
}

/// D-SHAPE-CASE1 parity fixture: one E0357 per checked category, including a
/// module alias and an `@` constant whose span starts at its mark.
fn write_identifier_casing_project(project: &Path) -> (PathBuf, String, Vec<(PathBuf, String)>) {
    fs::create_dir_all(project).unwrap_or_else(|error| {
        panic!("cannot create identifier-casing Jet project `{}`: {error}", project.display())
    });
    let manifest = project.join("package.jet");
    let entry = project.join(SMALL_ENTRY_RELATIVE);
    let helper = project.join("helper.jet");
    let entry_source = r#"use "helper" as BadAlias

struct bad_type {
    BadField: Int
}

MAX_RETRIES :: prep { 3 }
BAD_CONSTANT :: prep { MAX_RETRIES }

fn BadFunction(BadParam: Int) -> Int {
    BadLocal :: BadParam
    callback :: (BadLambda: Int) -> BadLambda
    return callback(BadLocal)
}

pub fn main() {
    print(BadFunction(BAD_CONSTANT))
}
"#
    .to_string();
    let helper_source = "pub fn helper() -> Int {\n    return 1\n}\n".to_string();
    fs::write(&manifest, SMALL_PROGRAM_MANIFEST).unwrap_or_else(|error| {
        panic!("cannot write identifier-casing manifest `{}`: {error}", manifest.display())
    });
    fs::write(&entry, &entry_source).unwrap_or_else(|error| {
        panic!("cannot write identifier-casing entry `{}`: {error}", entry.display())
    });
    fs::write(&helper, &helper_source).unwrap_or_else(|error| {
        panic!("cannot write identifier-casing helper `{}`: {error}", helper.display())
    });
    let closure = vec![(entry.clone(), entry_source.clone()), (helper, helper_source)];
    (entry, entry_source, closure)
}

fn write_valid_report_project(project: &Path) -> (PathBuf, String) {
    fs::create_dir_all(project).unwrap_or_else(|error| {
        panic!("cannot create valid-report Jet project `{}`: {error}", project.display())
    });
    let manifest = project.join("package.jet");
    let entry = project.join(SMALL_ENTRY_RELATIVE);
    let entry_source = "fn unused() Int -> 1\n\npub fn main() {}\n".to_string();
    let manifest_source = r#"name: "bootstrap_small"
version: "0.1.0"
edition: "2028"
outputs: { app: .Executable{ entry: main } }
boundaries: {
    deny: [{ from: "bootstrap_small.run", to: "bootstrap_small.db" }]
}
"#;
    fs::write(&manifest, manifest_source).unwrap_or_else(|error| {
        panic!("cannot write valid-report Jet manifest `{}`: {error}", manifest.display())
    });
    fs::write(&entry, &entry_source).unwrap_or_else(|error| {
        panic!("cannot write valid-report Jet entry `{}`: {error}", entry.display())
    });
    (entry, entry_source)
}

fn write_nested_closure_callback_project(project: &Path) {
    fs::create_dir_all(project).unwrap_or_else(|error| {
        panic!("cannot create nested-lambda project `{}`: {error}", project.display())
    });
    let manifest = project.join("package.jet");
    let entry = project.join(SMALL_ENTRY_RELATIVE);
    let source = r#"use c.cb as c

#Import module c.cb {
    fn call(cb: fn(I32) I32 -[]>, x: I32) I32 = "call"
}

pub fn main() {
    prefix :: "nested"
    captured :: () -> prefix
    print(captured())
    print(c.call((x) -> x, 40))
}
"#;
    fs::write(&manifest, SMALL_PROGRAM_MANIFEST).unwrap_or_else(|error| {
        panic!("cannot write nested-lambda manifest `{}`: {error}", manifest.display())
    });
    fs::write(&entry, source).unwrap_or_else(|error| {
        panic!("cannot write nested-lambda source `{}`: {error}", entry.display())
    });
}

fn rust_reference_reports(
    entry: &Path,
    entry_source: &str,
    source_closure: &[(PathBuf, String)],
) -> Vec<String> {
    let entry_path = entry
        .to_str()
        .unwrap_or_else(|| panic!("invalid Jet test entry path `{}`", entry.display()));
    let diagnostics =
        match crate::Driver::compile_bundle_path_opts_with_source_closure_and_runtime(
            entry_path,
            crate::Sema::CompileMode::Check,
            false,
            crate::Policy::GateSet::default(),
            false,
            false,
            false,
            false,
            None,
            None,
            "dev",
            &BTreeMap::new(),
            false,
            None,
            source_closure,
            None,
        ) {
            Err(diagnostics) => diagnostics,
            Ok(_) => panic!("Rust reference accepted the intentionally invalid imported source"),
        };
    let report_path = jet_foundation::Report::ReportPath::from_process(entry_path);
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.to_report(&report_path, entry_source).json())
        .collect()
}

fn rust_reference_success_reports(
    entry: &Path,
    entry_source: &str,
    source_closure: &[(PathBuf, String)],
) -> Vec<String> {
    let entry_path = entry
        .to_str()
        .unwrap_or_else(|| panic!("invalid Jet test entry path `{}`", entry.display()));
    let (output, _) = crate::Driver::compile_bundle_path_opts_with_source_closure_and_runtime(
        entry_path,
        crate::Sema::CompileMode::Check,
        false,
        crate::Policy::GateSet::default(),
        false,
        false,
        false,
        false,
        None,
        None,
        "dev",
        &BTreeMap::new(),
        false,
        None,
        source_closure,
        None,
    )
    .unwrap_or_else(|diagnostics| panic!("Rust reference rejected valid report fixture: {diagnostics:?}"));
    let report_path = jet_foundation::Report::ReportPath::from_process(entry_path);
    output
        .lints
        .iter()
        .map(|diagnostic| diagnostic.to_report(&report_path, entry_source).json())
        .collect()
}

fn report_at_imported_eof(reports: &[String], path: &Path, end: usize) -> String {
    let file = format!("\"file\":\"{}\"", path.display());
    let span = format!("\"span\":{{\"start\":{end},\"end\":{end}}}");
    reports
        .iter()
        .find(|report| report.contains("\"code\":\"E0003\"") && report.contains(&file) && report.contains(&span))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no imported-source EOF E0003 report for `{}` at byte {end}: {reports:?}",
                path.display()
            )
        })
}

const TASK_ROOTS_BEGIN: &str = "// bootstrap:task-roots-begin";
const TASK_ROOTS_END: &str = "// bootstrap:task-roots-end";

fn append_generated_artifact_main(source: String, task_roots: bool) -> String {
    let mut complete = source;
    if task_roots {
        complete.push_str(GENERATED_ARTIFACT_MAIN);
        return complete;
    }
    let mut rest = GENERATED_ARTIFACT_MAIN;
    while let Some(begin) = rest.find(TASK_ROOTS_BEGIN) {
        complete.push_str(&rest[..begin]);
        let end = rest[begin..]
            .find(TASK_ROOTS_END)
            .unwrap_or_else(|| panic!("generated artifact main has an unterminated task-root region"));
        rest = &rest[begin + end + TASK_ROOTS_END.len()..];
    }
    complete.push_str(rest);
    complete
}

fn backend_identity_build_script(
    repo: &Path,
    native_library: Option<(&Path, &str)>,
) -> String {
    let native_link_directives = native_library.map_or_else(String::new, |(directory, library)| {
        format!(
            "    println!(\"cargo:rustc-link-search=native={{}}\", {:?});\n    println!(\"cargo:rustc-link-lib=static={library}\");\n",
            directory.display().to_string(),
        )
    });
    // The identity code is inlined (the root build.rs `#[path]`-includes the
    // same two files) rather than taken from a `jet` build-dependency: that
    // dependency compiled the `jet` and `jet-jit` crates a second time for the
    // host, doubling the largest repository crates' share of the build.
    let inline_module = |relative: &str| {
        let path = repo.join(relative);
        fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("cannot read backend build-script module `{}`: {error}", path.display())
        })
    };
    format!(
        r#"use std::fs;
use std::path::Path;

#[allow(dead_code, non_snake_case)]
mod SHA256 {{
{sha256}
}}

#[allow(dead_code, non_snake_case)]
mod BuildIdentity {{
{build_identity}
}}

fn main() {{
    let root = Path::new({:?});
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read(manifest_dir.join("generated.rs"))
        .expect("generated artifact source must be readable");
    let manifest = fs::read(manifest_dir.join("Cargo.toml"))
        .expect("generated artifact manifest must be readable");
    let build_script = fs::read(manifest_dir.join("build.rs"))
        .expect("generated artifact build script must be readable");
    let facts = BuildIdentity::build_facts()
        .expect("Cargo-supplied backend build facts must be readable");
    let mut extras = vec![
        ("Compiler/Bootstrap/__generated_compiler.rs".to_string(), source),
        ("Compiler/Bootstrap/__generated_cargo_manifest.toml".to_string(), manifest),
        ("Compiler/Bootstrap/__generated_cargo_build.rs".to_string(), build_script),
    ];
    // A compiler artifact embeds its image with `include_bytes!` from here.
    let image_path = manifest_dir.join("src/compiler.image");
    if let Ok(image) = fs::read(&image_path) {{
        extras.push(("Compiler/Bootstrap/__generated_compiler.image".to_string(), image));
        println!("cargo:rerun-if-changed={{}}", image_path.display());
    }}
    let identity = BuildIdentity::semantic_id_with_extra(
        root,
        BuildIdentity::COMPILER_DOMAIN,
        BuildIdentity::COMPILER_SOURCES,
        &facts,
        &extras,
    )
    .expect("generated compiler identity must be computable");
    fs::write(manifest_dir.join(".bootstrap-artifact-id"), &identity)
        .expect("generated compiler identity receipt must be writable");
    println!("cargo:rustc-env=JET_COMPILER_BUILD_ID={{identity}}");
    // Compiler-authored MIR records the source identity, not this binary's
    // backend profile, rustc flags, or generated extras. BUILD_ID above keeps
    // all of those facts for artifact receipts and build/cache invalidation.
    let source_identity = BuildIdentity::compiler_source_id(root)
        .expect("generated compiler source identity must be computable");
    println!("cargo:rustc-env=JET_COMPILER_SOURCE_ID={{source_identity}}");
    println!("cargo::rustc-check-cfg=cfg(jet_bootstrap_compiler_artifact)");
    println!("cargo:rustc-cfg=jet_bootstrap_compiler_artifact");
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("generated.rs").display());
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("Cargo.toml").display());
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("build.rs").display());
    // The root build.rs watch set: SOURCE_ID hashes these roots and facts, so
    // an incremental backend build must rerun this script when any changes.
    for input in BuildIdentity::COMPILER_SOURCES {{
        println!("cargo:rerun-if-changed={{}}", root.join(input).display());
    }}
    for key in [
        "RUSTC",
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {{
        println!("cargo:rerun-if-env-changed={{key}}");
    }}
    for key in BuildIdentity::profile_override_keys() {{
        println!("cargo:rerun-if-env-changed={{key}}");
    }}
    if let Some(spec) = BuildIdentity::target_spec_path() {{
        println!("cargo:rerun-if-changed={{}}", spec.display());
    }}
{native_link_directives}}}
"#,
        repo.display().to_string(),
        native_link_directives = native_link_directives,
        sha256 = inline_module("crates/jet-foundation/src/SHA256.rs"),
        build_identity = inline_module("Compiler/Bootstrap/BuildIdentity.rs"),
    )
}
/// The prepared FFI bridge an emitted program names (`jet_ffi_<key>`). The
/// backend build depends on its generated source package by path, Cargo's
/// equivalent of `jet build`'s `--extern` of the bridge rlib.
struct BackendFfiCrate {
    name: String,
    dir: PathBuf,
}

impl BackendFfiCrate {
    /// The manifest dependency row naming the bridge.
    fn dependency(&self) -> String {
        format!("{} = {{ path = {:?} }}\n", self.name, self.dir.display().to_string())
    }

    /// A retained bridge record (`stage-zero.ffi`): crate name and directory
    /// lines; empty when the program names no bridge.
    fn parse(record: &str) -> Option<Self> {
        record.split_once('\n').map(|(name, path)| BackendFfiCrate {
            name: name.to_string(),
            dir: PathBuf::from(path.trim_end_matches('\n')),
        })
    }
}

/// One build unit of an emitted program, built as its own crate.
struct BackendUnit {
    crate_name: String,
    /// Crate names of the units this one names; each is built before it.
    deps: Vec<String>,
    /// The exported crate source (the facade of a sharded unit).
    source: String,
    /// The shard crates of a unit too large for one rustc (`shard_unit`);
    /// empty otherwise.
    shards: Vec<BackendShard>,
}

/// One shard crate of a sharded unit: `<unit>_s<index>` for hot code,
/// `<unit>_cold<index>` for cold code (`shard_unit`), which the release
/// profile builds unoptimized (`backend_unit_profiles`).
struct BackendShard {
    name: String,
    source: String,
    cold: bool,
}

/// An emitted program split into crates: the exported `jet_runtime` crate
/// (its runtime/Core block), the crates of its unit sections (MIRRust
/// `EmissionUnits`) in dependency order, and the main crate — everything
/// outside the sections (Host/Runner glue, the embedded compiler image, the
/// artifact main).
struct BackendWorkspace {
    runtime_lib: String,
    units: Vec<BackendUnit>,
    main: String,
}

/// Split a backend source into crates, as `jet_store::runtime::prepare`
/// splits a real build: the runtime/Core block always builds as its own
/// crate, so programs with the same block share one built runtime. Unit
/// dependencies come from the names each unit's text actually uses (every
/// program item is a unique `__jet_` symbol); units that name each other
/// cyclically merge into one crate. A source without unit sections has no
/// unit crates. `None` when the source has no runtime block.
fn split_backend_workspace(source: &str) -> Option<BackendWorkspace> {
    let Some(split) = jet_store::runtime::split_runtime_crate(source)
        .unwrap_or_else(|error| panic!("backend source: {error}"))
    else {
        assert!(!source.contains(UNIT_BEGIN_MARKER), "unit-split backend source has no runtime block");
        return None;
    };
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut main = String::new();
    let mut open: Option<usize> = None;
    for line in split.program.split_inclusive('\n') {
        if let Some(name) = line.strip_prefix(UNIT_BEGIN_MARKER) {
            let name = name.trim_end();
            assert!(open.is_none(), "backend unit section `{name}` opens inside another section");
            let existing = sections.iter().position(|(existing, _)| existing == name);
            let index = existing.unwrap_or_else(|| {
                sections.push((name.to_string(), String::new()));
                sections.len() - 1
            });
            open = Some(index);
        } else if let Some(name) = line.strip_prefix(UNIT_END_MARKER) {
            let index = open
                .take()
                .unwrap_or_else(|| panic!("backend unit section `{}` closes without opening", name.trim_end()));
            assert_eq!(sections[index].0, name.trim_end(), "backend unit sections interleave");
        } else if let Some(index) = open {
            sections[index].1.push_str(line);
        } else {
            main.push_str(line);
        }
    }
    assert!(open.is_none(), "backend unit section is unterminated");

    let mut owners: HashMap<&str, usize> = HashMap::new();
    for (index, (_, text)) in sections.iter().enumerate() {
        for name in unit_definitions(text) {
            owners.entry(name).or_insert(index);
        }
    }
    let edges = sections
        .iter()
        .enumerate()
        .map(|(index, (_, text))| {
            unit_symbols(text)
                .filter_map(|symbol| owners.get(symbol).copied())
                .filter(|owner| *owner != index)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();

    // Tarjan emits each strongly connected component after every component
    // it reaches: dependencies first, the build order.
    let components = strongly_connected_units(&edges);
    let mut component_of = vec![0; sections.len()];
    for (component, members) in components.iter().enumerate() {
        for member in members {
            component_of[*member] = component;
        }
    }
    let crate_names = components
        .iter()
        .map(|members| {
            let mut names = members.iter().map(|member| sections[*member].0.as_str()).collect::<Vec<_>>();
            names.sort_unstable();
            let name = names.join("_").to_ascii_lowercase();
            format!(
                "jetc_{}",
                name.chars()
                    .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>();
    let units = components
        .iter()
        .enumerate()
        .map(|(component, members)| {
            let mut members = members.clone();
            members.sort_unstable();
            let deps = members
                .iter()
                .flat_map(|member| edges[*member].iter().map(|target| component_of[*target]))
                .filter(|target| *target != component)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|target| crate_names[target].clone())
                .collect::<Vec<_>>();
            let crate_name = crate_names[component].clone();
            let mut prelude = String::from("#![allow(warnings)]\nuse jet_runtime::*;\n");
            for dep in &deps {
                prelude.push_str(&format!("use {dep}::*;\n"));
            }
            let text = members.iter().map(|member| sections[*member].1.as_str()).collect::<String>();
            let (source, shards) = match shard_unit(&text, BACKEND_SHARD_BYTES) {
                Some(sharded) => {
                    let source = format!(
                        "{prelude}{}unsafe extern \"Rust\" {{\n{}}}\n",
                        jet_store::runtime::export_crate_source(&sharded.facade),
                        sharded.declarations,
                    );
                    let shard = |(index, text): (usize, &String), cold: bool| BackendShard {
                        name: format!("{crate_name}_{}{index}", if cold { "cold" } else { "s" }),
                        source: format!(
                            "{prelude}use {crate_name}::*;\n{}",
                            jet_store::runtime::export_crate_source(text)
                        ),
                        cold,
                    };
                    let hot = sharded.shards.iter().enumerate().map(|indexed| shard(indexed, false));
                    let cold = sharded.cold_shards.iter().enumerate().map(|indexed| shard(indexed, true));
                    let shards = hot.chain(cold).collect();
                    (source, shards)
                }
                None => (format!("{prelude}{}", jet_store::runtime::export_crate_source(&text)), Vec::new()),
            };
            BackendUnit {
                crate_name,
                deps,
                source,
                shards,
            }
        })
        .collect::<Vec<_>>();

    let anchor = "use jet_runtime::*;\n";
    let at = main
        .find(anchor)
        .unwrap_or_else(|| panic!("split backend main has no `jet_runtime` import"))
        + anchor.len();
    // Shards are named by nothing (their functions are reached through the
    // facade's declarations), so the main crate links each explicitly.
    let uses = units
        .iter()
        .map(|unit| {
            let shards = unit.shards.iter().map(|shard| format!("use {} as _;\n", shard.name));
            std::iter::once(format!("use {}::*;\n", unit.crate_name)).chain(shards).collect::<String>()
        })
        .collect::<String>();
    main.insert_str(at, &uses);
    Some(BackendWorkspace {
        runtime_lib: split.runtime_lib,
        units,
        main,
    })
}

/// The `__jet_` symbols a unit section defines: its top-level items (emitted
/// at column 0) and the functions of its top-level foreign blocks. Indented
/// trait methods and body-local names are not unit-level definitions.
fn unit_definitions(text: &str) -> impl Iterator<Item = &str> {
    let mut in_foreign = false;
    text.lines().filter_map(move |line| {
        if in_foreign {
            in_foreign = !line.starts_with('}');
            return if in_foreign { defined_unit_item(line) } else { None };
        }
        if line.starts_with("extern ") || line.starts_with("unsafe extern ") {
            in_foreign = !line.trim_end().ends_with('}');
            return None;
        }
        if line.starts_with(char::is_whitespace) {
            return None;
        }
        defined_unit_item(line)
    })
}

/// The `__jet_` symbol a unit line defines at item level (`fn`, `struct`,
/// `enum`, `trait`, `type`, `static`, `const`, `union`, after visibility and
/// qualifiers), if any.
fn defined_unit_item(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    for visibility in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(stripped) = rest.strip_prefix(visibility) {
            rest = stripped;
            break;
        }
    }
    loop {
        if let Some(stripped) = rest.strip_prefix("const fn ") {
            return unit_symbol_at(stripped);
        }
        let current = rest;
        match ["unsafe ", "async ", "extern \"C\" ", "extern \"Rust\" "]
            .iter()
            .find_map(|qualifier| current.strip_prefix(qualifier))
        {
            Some(stripped) => rest = stripped,
            None => break,
        }
    }
    ["fn ", "struct ", "enum ", "trait ", "type ", "static mut ", "static ", "const ", "union "]
        .iter()
        .find_map(|keyword| rest.strip_prefix(keyword))
        .and_then(unit_symbol_at)
}

/// The `__jet_` identifier at the start of `text`.
fn unit_symbol_at(text: &str) -> Option<&str> {
    let end = text
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(text.len());
    let symbol = &text[..end];
    symbol.starts_with("__jet_").then_some(symbol)
}

/// Every whole `__jet_` identifier in `text`.
fn unit_symbols(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("__jet_").filter_map(move |(at, _)| {
        let starts_identifier = text[..at]
            .chars()
            .next_back()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'));
        if starts_identifier { unit_symbol_at(&text[at..]) } else { None }
    })
}

/// Strongly connected components of the unit graph, each after every
/// component it reaches.
fn strongly_connected_units(edges: &[BTreeSet<usize>]) -> Vec<Vec<usize>> {
    struct Tarjan<'e> {
        edges: &'e [BTreeSet<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, node: usize) {
            self.index[node] = Some(self.next);
            self.low[node] = self.next;
            self.next += 1;
            self.stack.push(node);
            self.on_stack[node] = true;
            for target in self.edges[node].clone() {
                match self.index[target] {
                    None => {
                        self.visit(target);
                        self.low[node] = self.low[node].min(self.low[target]);
                    }
                    Some(index) if self.on_stack[target] => {
                        self.low[node] = self.low[node].min(index);
                    }
                    Some(_) => {}
                }
            }
            if Some(self.low[node]) == self.index[node] {
                let mut component = Vec::new();
                while let Some(member) = self.stack.pop() {
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        edges,
        index: vec![None; edges.len()],
        low: vec![0; edges.len()],
        on_stack: vec![false; edges.len()],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for node in 0..edges.len() {
        if tarjan.index[node].is_none() {
            tarjan.visit(node);
        }
    }
    tarjan.components
}

/// Write the runtime and unit crates under `units_root`; returns the main
/// manifest's dependency rows for them. Every crate may name the FFI bridge,
/// so each depends on it. The runtime crate's directory is named by the
/// digest of its source and bridge row, so programs that alternate between
/// runtime/Core blocks (their runtime parts differ) each keep their built
/// runtime instead of rewriting one directory. Files are rewritten only when
/// their bytes change, so cargo keeps unchanged crates built.
fn write_backend_workspace(
    units_root: &Path,
    workspace: &BackendWorkspace,
    ffi: Option<&BackendFfiCrate>,
) -> String {
    let ffi_dependency = ffi.map_or_else(String::new, BackendFfiCrate::dependency);
    let runtime_digest = jet_foundation::SHA256::sha256_hex(
        format!("{ffi_dependency}\0{}", workspace.runtime_lib).as_bytes(),
    );
    let runtime_dir_name = format!("jet_runtime_{}", &runtime_digest[..16]);
    let runtime_dir = units_root.join(&runtime_dir_name);
    write_backend_crate(&runtime_dir, "jet_runtime", &[], &ffi_dependency, &workspace.runtime_lib);
    let mut dependencies = format!("jet_runtime = {{ path = {:?} }}\n", runtime_dir.display().to_string());
    // Unit and shard crates name the runtime by its digest directory.
    let unit_extra_dependencies = format!("jet_runtime = {{ path = \"../{runtime_dir_name}\" }}\n{ffi_dependency}");
    for unit in &workspace.units {
        let dir = units_root.join(&unit.crate_name);
        let mut deps = unit.deps.clone();
        write_backend_crate(&dir, &unit.crate_name, &deps, &unit_extra_dependencies, &unit.source);
        dependencies.push_str(&format!(
            "{} = {{ path = {:?} }}\n",
            unit.crate_name,
            dir.display().to_string()
        ));
        deps.push(unit.crate_name.clone());
        for shard in &unit.shards {
            let dir = units_root.join(&shard.name);
            write_backend_crate(&dir, &shard.name, &deps, &unit_extra_dependencies, &shard.source);
            dependencies.push_str(&format!("{} = {{ path = {:?} }}\n", shard.name, dir.display().to_string()));
        }
    }
    dependencies
}

/// The main manifest's profile overrides for the runtime and unit crates:
/// under dev `BACKEND_UNIT_PROFILE` for each (repository crates keep the
/// default dev profile); under the release profile `release` (which already
/// builds everything at opt-level 2 without debuginfo or LTO)
/// `BACKEND_COLD_PROFILE` for each cold shard.
fn backend_unit_profiles(workspace: &BackendWorkspace, release: Option<&str>) -> String {
    let shards = || workspace.units.iter().flat_map(|unit| &unit.shards);
    match release {
        None => std::iter::once("jet_runtime")
            .chain(workspace.units.iter().map(|unit| unit.crate_name.as_str()))
            .chain(shards().map(|shard| shard.name.as_str()))
            .map(|name| format!("\n[profile.dev.package.{name}]\n{BACKEND_UNIT_PROFILE}"))
            .collect(),
        Some(profile) => shards()
            .filter(|shard| shard.cold)
            .map(|shard| format!("\n[profile.{profile}.package.{}]\n{BACKEND_COLD_PROFILE}", shard.name))
            .collect(),
    }
}

/// One library crate of the backend workspace; `deps` are sibling crates,
/// `extra_dependencies` further manifest dependency rows.
fn write_backend_crate(dir: &Path, name: &str, deps: &[String], extra_dependencies: &str, lib: &str) {
    let mut manifest = format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n");
    for dep in deps {
        manifest.push_str(&format!("{dep} = {{ path = \"../{dep}\" }}\n"));
    }
    manifest.push_str(extra_dependencies);
    for (path, text) in [(dir.join("Cargo.toml"), manifest.as_str()), (dir.join("src/lib.rs"), lib)] {
        if fs::read(&path).is_ok_and(|existing| existing == text.as_bytes()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("cannot create backend crate directory `{}`: {error}", parent.display())
            });
        }
        fs::write(&path, text).unwrap_or_else(|error| {
            panic!("cannot write backend crate file `{}`: {error}", path.display())
        });
    }
}

/// A unit too large for one rustc, split by `shard_unit` (texts before
/// export).
struct ShardedUnit {
    /// Every type, trait, impl and unmovable item: the unit crate itself.
    facade: String,
    /// `unsafe extern "Rust"` block members declaring each moved function.
    declarations: String,
    /// Moved hot function definitions, at most about `shard_bytes` each.
    shards: Vec<String>,
    /// Moved cold function definitions (`MovedFunction::cold`), packed the
    /// same way into their own crates.
    cold_shards: Vec<String>,
}

/// One function moved out of a facade: its declaration and its definition
/// under its own symbol.
struct MovedFunction {
    declaration: String,
    definition: String,
    /// Code an optimized build should not optimize: a cold-trait impl
    /// method (`BACKEND_COLD_TRAITS`), constant table code (a parameterless
    /// function whose body is a `vec!` literal, or one chunk of a cached
    /// table's row fill, whose body starts `rows.push(`), or any function of
    /// at least `BACKEND_COLD_FUNCTION_BYTES`.
    cold: bool,
}

/// Shard a unit's text when it is longer than `shard_bytes`. Free
/// non-generic `__jet_` functions move to shard crates, which import the
/// facade and define each function `#[no_mangle]`; the facade declares them
/// in an `unsafe extern "Rust"` block, so every caller — facade, sibling
/// shards, dependent units — reaches a moved function through its symbol
/// and no shard depends on another. Impls stay beside their types (orphan
/// rule), but a large method body moves as a free function the method
/// calls. Cold and hot functions pack into separate shards, so the release
/// profile can leave the cold ones unoptimized. `None` for a small unit or
/// one with macros, which shards could not see.
fn shard_unit(text: &str, shard_bytes: usize) -> Option<ShardedUnit> {
    if text.len() <= shard_bytes {
        return None;
    }
    let mask = jet_foundation::RustSource::rust_code_mask(text);
    let (items, end) = masked_items(&mask);
    let mut facade = String::new();
    let mut uses = String::new();
    let mut declarations = String::new();
    let mut moved = Vec::new();
    for (start, end) in items {
        let (item, code) = (&text[start..end], &mask[start..end]);
        let head = &code[item_head(code)..];
        if head.starts_with("macro_rules!") {
            return None;
        }
        if head.starts_with("use ") || head.starts_with("pub use ") {
            uses.push_str(item);
        }
        if let Some(function) = movable_function(item, code) {
            declarations.push_str(&function.declaration);
            moved.push(function);
        } else if let Some((rewritten, functions)) = delegate_impl_methods(item, code) {
            facade.push_str(&rewritten);
            for function in functions {
                declarations.push_str(&function.declaration);
                moved.push(function);
            }
        } else {
            facade.push_str(item);
        }
    }
    facade.push_str(&text[end..]);
    let (mut shards, mut cold_shards): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for function in moved {
        let packed = if function.cold { &mut cold_shards } else { &mut shards };
        match packed.last_mut() {
            Some(shard) if shard.len() + function.definition.len() <= shard_bytes => {
                shard.push_str(&function.definition)
            }
            _ => packed.push(format!("{uses}{}", function.definition)),
        }
    }
    Some(ShardedUnit {
        facade,
        declarations,
        shards,
        cold_shards,
    })
}

/// Byte ranges of the top-level items of masked Rust `code`, each starting
/// where the previous one ended (leading whitespace and comments travel with
/// their item), and the end of the last one.
fn masked_items(code: &str) -> (Vec<(usize, usize)>, usize) {
    let mut items = Vec::new();
    let (mut start, mut depth) = (0usize, 0usize);
    for (at, byte) in code.bytes().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth
                    .checked_sub(1)
                    .unwrap_or_else(|| panic!("backend unit source closes an unopened bracket at byte {at}"));
                if depth == 0 && byte == b'}' {
                    items.push((start, at + 1));
                    start = at + 1;
                }
            }
            b';' if depth == 0 => {
                items.push((start, at + 1));
                start = at + 1;
            }
            _ => {}
        }
    }
    (items, start)
}

/// Index of the bracket closing the one at `open` in masked `code`.
fn matching_bracket(code: &str, open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, byte) in code[open..].bytes().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return open + offset;
                }
            }
            _ => {}
        }
    }
    panic!("backend unit source leaves a bracket open at byte {open}")
}

/// Offset of an item's keyword in masked `code`, past leading whitespace
/// and attributes.
fn item_head(code: &str) -> usize {
    let mut at = code.len() - code.trim_start().len();
    while code[at..].starts_with("#[") {
        at = matching_bracket(code, at + 1) + 1;
        at += code[at..].len() - code[at..].trim_start().len();
    }
    at
}

/// Whether every attribute in masked `attributes` is one a moved function
/// may keep (none changes its symbol, ABI or presence).
fn movable_attributes(attributes: &str) -> bool {
    attributes
        .split("#[")
        .skip(1)
        .all(|attribute| ["inline", "allow(", "cold]"].iter().any(|name| attribute.starts_with(name)))
}

/// Ranges of masked `code` between its depth-0 `separator`s (angle brackets
/// count as depth; the `>` of `->` does not).
fn split_top_level(code: &str, separator: u8) -> Vec<(usize, usize)> {
    let bytes = code.as_bytes();
    let (mut parts, mut start, mut depth) = (Vec::new(), 0usize, 0isize);
    for (at, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'{' | b'(' | b'[' | b'<' => depth += 1,
            b'>' if at > 0 && bytes[at - 1] == b'-' => {}
            b'}' | b')' | b']' | b'>' => depth -= 1,
            _ if byte == separator && depth == 0 => {
                parts.push((start, at));
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push((start, code.len()));
    parts
}

/// Offset of the `:` between a parameter's pattern and its type in masked
/// `code` (not a `::` path separator).
fn parameter_colon(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    split_top_level(code, b':').into_iter().map(|(_, end)| end).find(|&at| {
        at < bytes.len() && bytes.get(at + 1) != Some(&b':') && (at == 0 || bytes[at - 1] != b':')
    })
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A free, non-generic, non-const `__jet_` function item: its declaration
/// and its `#[no_mangle]` definition.
fn movable_function(item: &str, code: &str) -> Option<MovedFunction> {
    let lead = code.len() - code.trim_start().len();
    let head = item_head(code);
    if !movable_attributes(&code[lead..head]) || !code.ends_with('}') {
        return None;
    }
    let rest = &code[head..];
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let unsafe_function = rest.starts_with("unsafe ");
    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest).strip_prefix("fn ")?;
    let name_end = rest.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))?;
    let name = &rest[..name_end];
    if !name.starts_with("__jet_") || !rest[name_end..].starts_with('(') {
        return None;
    }
    let open = code.len() - rest.len() + name_end;
    let close = matching_bracket(code, open);
    let body = close + code[close..].find('{')?;
    if code[open..body].contains("impl ") || code[close..body].contains("where") {
        return None;
    }
    let mut parameters = Vec::new();
    for (start, end) in split_top_level(&code[open + 1..close], b',') {
        let (start, end) = (open + 1 + start, open + 1 + end);
        if code[start..end].trim().is_empty() {
            continue;
        }
        let colon = start + parameter_colon(&code[start..end])?;
        parameters.push(format!("_: {}", item[colon + 1..end].trim()));
    }
    // Emitted Jet code names its locals `__jet_*`: a bare `rows` is the row
    // vector of a cached table's fill chunk.
    let body_code = code[body + 1..].trim_start();
    let table = (parameters.is_empty() && body_code.starts_with("vec![")) || body_code.starts_with("rows.push(");
    Some(MovedFunction {
        declaration: format!(
            "    pub {} fn {name}({}) {};\n",
            if unsafe_function { "unsafe" } else { "safe" },
            parameters.join(", "),
            item[close + 1..body].trim()
        ),
        definition: format!("\n#[no_mangle]\n{}", &item[lead..]),
        cold: table || code.len() >= BACKEND_COLD_FUNCTION_BYTES,
    })
}

/// An inherent or trait impl of a plain named type whose large method
/// bodies move: each such method's body becomes a moved free function the
/// method calls. `None` when no method moves.
fn delegate_impl_methods(item: &str, code: &str) -> Option<(String, Vec<MovedFunction>)> {
    let head = item_head(code);
    if !code[head..].starts_with("impl ") || !code.ends_with('}') {
        return None;
    }
    let brace = head + code[head..].find('{')?;
    let header = code[head + "impl ".len()..brace].trim();
    let (trait_path, self_type) = header.split_once(" for ").unwrap_or(("inherent", header));
    let self_type = self_type.trim();
    if !is_identifier(self_type) {
        return None;
    }
    let trait_tag = trait_path
        .trim()
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
        .collect::<String>();
    let prefix = format!("__jet_impl_{self_type}_{trait_tag}_");
    let cold_trait = BACKEND_COLD_TRAITS.contains(&trait_path.trim());
    let close = code.len() - 1;
    let (methods, tail) = masked_items(&code[brace + 1..close]);
    let mut rewritten = item[..brace + 1].to_string();
    let mut moved = Vec::new();
    for (start, end) in methods {
        let (start, end) = (brace + 1 + start, brace + 1 + end);
        match delegated_method(&item[start..end], &code[start..end], self_type, &prefix) {
            Some((method, mut function)) => {
                rewritten.push_str(&method);
                function.cold |= cold_trait;
                moved.push(function);
            }
            None => rewritten.push_str(&item[start..end]),
        }
    }
    if moved.is_empty() {
        return None;
    }
    rewritten.push_str(&item[brace + 1 + tail..]);
    Some((rewritten, moved))
}

/// A method of at least `BACKEND_DELEGATE_METHOD_BYTES` whose body can
/// stand alone as a free function (`self` becomes `__jet_self`, `Self` the
/// impl's type): the method calling it, and the moved function.
fn delegated_method(
    method: &str,
    code: &str,
    self_type: &str,
    prefix: &str,
) -> Option<(String, MovedFunction)> {
    // Nested impls and module-relative paths do not survive the move
    // unchanged; such methods stay.
    if code.len() < BACKEND_DELEGATE_METHOD_BYTES || code.contains("impl ") || code.contains("self::") {
        return None;
    }
    let lead = code.len() - code.trim_start().len();
    let head = item_head(code);
    if !movable_attributes(&code[lead..head]) {
        return None;
    }
    let rest = &code[head..];
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let unsafe_method = rest.starts_with("unsafe ");
    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest).strip_prefix("fn ")?;
    let name_end = rest.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))?;
    let name = &rest[..name_end];
    if !rest[name_end..].starts_with('(') {
        return None;
    }
    let open = code.len() - rest.len() + name_end;
    let close = matching_bracket(code, open);
    let body = close + code[close..].find('{')?;
    // Signature lifetimes (named, or elided into a returned reference) would
    // resolve differently once the receiver is an ordinary parameter.
    if code[..body].contains('\'') || code[close..body].contains('&') || code[close..body].contains("where") {
        return None;
    }
    let mut parameters = Vec::new();
    let mut arguments = Vec::new();
    for (start, end) in split_top_level(&code[open + 1..close], b',') {
        let (start, end) = (open + 1 + start, open + 1 + end);
        let parameter = code[start..end].trim();
        if parameter.is_empty() {
            continue;
        }
        let receiver = match parameter {
            "&self" => Some(format!("__jet_self: &{self_type}")),
            "&mut self" => Some(format!("__jet_self: &mut {self_type}")),
            "self" => Some(format!("__jet_self: {self_type}")),
            "mut self" => Some(format!("mut __jet_self: {self_type}")),
            _ => None,
        };
        if let Some(receiver) = receiver {
            parameters.push(receiver);
            arguments.push("self");
            continue;
        }
        let colon = parameter_colon(parameter)?;
        let pattern = parameter[..colon].trim();
        let binding = pattern.strip_prefix("mut ").unwrap_or(pattern).trim();
        if !is_identifier(binding) || binding == "_" || binding == "self" {
            return None;
        }
        parameters.push(method[start..end].trim().to_string());
        arguments.push(binding);
    }
    let function_name = format!("{prefix}{name}");
    let function = format!(
        "pub {}fn {function_name}({}) {} {}",
        if unsafe_method { "unsafe " } else { "" },
        parameters.join(", "),
        method[close + 1..body].trim(),
        &method[body..]
    );
    let function = replace_self_tokens(&function, self_type);
    let moved = movable_function(&function, &jet_foundation::RustSource::rust_code_mask(&function))?;
    let call = format!("{{ {function_name}({}) }}", arguments.join(", "));
    Some((format!("{}{call}", &method[..body]), moved))
}

/// `text` with its code tokens `self` renamed `__jet_self` and `Self`
/// replaced by `self_type`.
fn replace_self_tokens(text: &str, self_type: &str) -> String {
    let code = jet_foundation::RustSource::rust_code_mask(text);
    let bytes = code.as_bytes();
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at + 4 <= bytes.len() {
        let token = &bytes[at..at + 4];
        if (token == b"self" || token == b"Self")
            && (at == 0 || !identifier(bytes[at - 1]))
            && bytes.get(at + 4).is_none_or(|byte| !identifier(*byte))
        {
            out.push_str(&text[copied..at]);
            out.push_str(if token == b"self" { "__jet_self" } else { self_type });
            copied = at + 4;
            at += 4;
        } else {
            at += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// The compiler-image envelope reader checks the archive prefix exactly as
/// full restore does, without decoding the MIR payload or hashing the
/// archive: it accepts a well-formed prefix in front of a payload and
/// checksum that full restore rejects.
#[test]
fn compiler_image_envelope_reads_only_the_checked_prefix() {
    use crate::compiler_bootstrap_compiler_image::{
        read_compiler_image_envelope, restore_compiler_image, CompilerImageMetadataWriter,
        FORMAT_VERSION, MAGIC,
    };
    use ::jet_foundation::MIR::{MirArtifactId, MirFunctionId, MIR_SCHEMA_VERSION};
    let digest = [7u8; 32];
    let archive = |schema: u16| {
        let mut writer = CompilerImageMetadataWriter::new();
        writer.write_raw(MAGIC);
        writer.write_u16(FORMAT_VERSION);
        writer.write_u16(schema);
        writer.write_u64(11);
        writer.write_u64(13);
        writer.write_raw(&digest);
        writer.write_bytes(b"identity").unwrap();
        writer.write_bytes(b"not a MIR payload").unwrap();
        writer.write_raw(&[0u8; 32]);
        writer.finish()
    };
    let bytes = archive(MIR_SCHEMA_VERSION);
    let envelope =
        read_compiler_image_envelope(&bytes, digest, MirArtifactId(11), MirFunctionId(13))
            .unwrap_or_else(|error| panic!("envelope rejected a well-formed prefix: {error}"));
    assert_eq!(envelope.source_authority_digest, digest);
    assert_eq!(envelope.artifact, MirArtifactId(11));
    assert_eq!(envelope.entry_function, MirFunctionId(13));
    let restored = restore_compiler_image(&bytes, digest, MirArtifactId(11), MirFunctionId(13));
    assert_eq!(
        restored.err().map(|error| error.0).as_deref(),
        Some("compiler-image archive checksum mismatch"),
    );
    let envelope_error = |bytes: &[u8], expected_digest: [u8; 32], artifact: u64, entry: u64| {
        read_compiler_image_envelope(
            bytes,
            expected_digest,
            MirArtifactId(artifact),
            MirFunctionId(entry),
        )
        .err()
        .map(|error| error.0)
    };
    for (expected_digest, artifact, entry, message) in [
        ([0u8; 32], 11, 13, "compiler-image source authority does not match the authorized source snapshot"),
        (digest, 12, 13, "compiler-image artifact identity does not match the requested artifact"),
        (digest, 11, 14, "compiler-image entry function does not match the requested entry"),
    ] {
        assert_eq!(
            envelope_error(&bytes, expected_digest, artifact, entry).as_deref(),
            Some(message),
        );
    }
    let stale_schema = archive(MIR_SCHEMA_VERSION.wrapping_add(1));
    assert!(envelope_error(&stale_schema, digest, 11, 13)
        .is_some_and(|error| error.starts_with("compiler-image MIR schema ")));
    let mut bad_magic = bytes.clone();
    bad_magic[0] ^= 1;
    assert_eq!(
        envelope_error(&bad_magic, digest, 11, 13).as_deref(),
        Some("invalid compiler-image magic"),
    );
    assert_eq!(
        envelope_error(&bytes[..40], digest, 11, 13).as_deref(),
        Some("truncated compiler-image archive"),
    );
}

/// Unit sections become crates in dependency order: dependencies come from
/// the top-level `__jet_` items a section names (not from indented trait
/// methods), and units naming each other cyclically merge into one crate.
#[test]
fn backend_workspace_orders_units_and_merges_cycles() {
    let source = format!(
        "#![allow(warnings)]\n\
         // jet:cached-runtime-begin\n\
         pub(crate) fn runtime_item() {{}}\n\
         // jet:cached-runtime-end\n\
         {UNIT_BEGIN_MARKER}base\n\
         fn __jet_base_item() {{}}\n\
         trait __jet_BaseStore {{\n    fn __jet_gamma_field(&self);\n}}\n\
         {UNIT_END_MARKER}base\n\
         {UNIT_BEGIN_MARKER}Alpha\n\
         fn __jet_alpha() {{ __jet_base_item(); __jet_beta(); }}\n\
         {UNIT_END_MARKER}Alpha\n\
         {UNIT_BEGIN_MARKER}Beta\n\
         fn __jet_beta() {{ __jet_alpha(); }}\n\
         {UNIT_END_MARKER}Beta\n\
         {UNIT_BEGIN_MARKER}Gamma\n\
         struct __jet_Gamma {{ __jet_gamma_field: i64 }}\n\
         fn __jet_gamma() {{ __jet_beta(); }}\n\
         {UNIT_END_MARKER}Gamma\n\
         fn main() {{ __jet_gamma(); }}\n"
    );
    let workspace = split_backend_workspace(&source).expect("unit sections split");
    let rows = workspace
        .units
        .iter()
        .map(|unit| (unit.crate_name.as_str(), unit.deps.join(",")))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("jetc_base", String::new()),
            ("jetc_alpha_beta", "jetc_base".to_string()),
            ("jetc_gamma", "jetc_alpha_beta".to_string()),
        ]
    );
    let merged = &workspace.units[1].source;
    assert!(merged.starts_with("#![allow(warnings)]\nuse jet_runtime::*;\nuse jetc_base::*;\n"));
    assert!(merged.contains("pub fn __jet_alpha()") && merged.contains("pub fn __jet_beta()"));
    assert!(workspace.runtime_lib.contains("pub fn runtime_item()"));
    assert!(workspace.main.contains(
        "use jet_runtime::*;\nuse jetc_base::*;\nuse jetc_alpha_beta::*;\nuse jetc_gamma::*;\n"
    ));
    assert!(workspace.main.ends_with("fn main() { __jet_gamma(); }\n"));
    assert!(!workspace.main.contains("__jet_alpha"));
}

/// A program without unit sections still links its runtime/Core block as
/// the separate `jet_runtime` crate (shared by every program with the same
/// block); a source without a runtime block stays one crate.
#[test]
fn backend_workspace_splits_runtime_without_unit_sections() {
    let workspace = split_backend_workspace("// jet:cached-runtime-begin\nfn f() {}\n// jet:cached-runtime-end\nfn main() { f(); }\n")
        .expect("runtime block split");
    assert!(workspace.units.is_empty());
    assert!(workspace.runtime_lib.contains("pub fn f()"));
    assert!(workspace.main.contains("use jet_runtime::*;\n"));
    assert!(workspace.main.ends_with("fn main() { f(); }\n"));
    assert!(!workspace.main.contains("fn f()"));
    assert!(split_backend_workspace("fn main() {}\n").is_none());
}

/// `shard_unit` packs cold code (cold-trait impl methods, constant tables,
/// giant functions) into cold shards apart from hot code, so the release
/// profile can leave only the cold code unoptimized.
#[test]
fn backend_shards_separate_cold_code() {
    let steps = "    __jet_n += 1;\n".repeat(200);
    let giant = "    __jet_n += 1;\n".repeat(BACKEND_COLD_FUNCTION_BYTES / 16);
    let text = format!(
        "pub struct __jet_T {{ __jet_n: i64 }}\n\
         pub fn __jet_hot(__jet_x: i64) -> i64 {{ __jet_x + 1 }}\n\
         pub fn __jet_list(__jet_x: i64) -> Vec<i64> {{ vec![__jet_x] }}\n\
         #[inline] pub fn __jet_TABLE() -> Vec<i64> {{ vec![1, 2, 3] }}\n\
         pub fn __jet_ROWS_rows_0(rows: &mut Vec<i64>) {{\n    rows.push(1);\n    rows.push(2);\n}}\n\
         impl __jet_Decode for __jet_T {{\n    fn jet_decode(__jet_tree: &i64) -> i64 {{\n        let mut __jet_n = *__jet_tree;\n{steps}        __jet_n\n    }}\n}}\n\
         impl __jet_Equatable for __jet_T {{\n    fn equals(&self, __jet_other: &__jet_T) -> bool {{\n        let mut __jet_n = self.__jet_n;\n{steps}        __jet_n == __jet_other.__jet_n\n    }}\n}}\n\
         pub fn __jet_giant(mut __jet_n: i64) -> i64 {{\n{giant}    __jet_n\n}}\n"
    );
    let sharded = shard_unit(&text, 64 << 10).expect("a unit over the shard size shards");
    let (hot, cold) = (sharded.shards.concat(), sharded.cold_shards.concat());
    for name in ["fn __jet_hot(", "fn __jet_list(", "fn __jet_impl___jet_T___jet_Equatable_equals("] {
        assert!(hot.contains(name) && !cold.contains(name), "`{name}` is hot");
    }
    for name in [
        "fn __jet_TABLE(",
        "fn __jet_ROWS_rows_0(",
        "fn __jet_giant(",
        "fn __jet_impl___jet_T___jet_Decode_jet_decode(",
    ] {
        assert!(cold.contains(name) && !hot.contains(name), "`{name}` is cold");
    }
    // The tables and the decoder share a cold shard; the giant fills its own.
    assert_eq!(sharded.cold_shards.len(), 2);
}

/// The release profile leaves only cold shards unoptimized; the dev profile
/// overrides every runtime and unit crate alike.
#[test]
fn backend_unit_profiles_leave_only_cold_shards_unoptimized() {
    let shard = |name: &str, cold| BackendShard {
        name: name.to_string(),
        source: String::new(),
        cold,
    };
    let workspace = BackendWorkspace {
        runtime_lib: String::new(),
        units: vec![BackendUnit {
            crate_name: "jetc_sema".to_string(),
            deps: Vec::new(),
            source: String::new(),
            shards: vec![shard("jetc_sema_s0", false), shard("jetc_sema_cold0", true)],
        }],
        main: String::new(),
    };
    let tables = |profiles: &str| {
        profiles.lines().filter(|line| line.starts_with('[')).map(str::to_string).collect::<Vec<_>>()
    };
    let release = backend_unit_profiles(&workspace, Some(BACKEND_RELEASE_PROFILE_NAME));
    assert_eq!(tables(&release), ["[profile.jet-stage-release.package.jetc_sema_cold0]"]);
    assert!(release.contains("\nopt-level = 0\n"));
    let dev = backend_unit_profiles(&workspace, None);
    assert_eq!(
        tables(&dev),
        ["jet_runtime", "jetc_sema", "jetc_sema_s0", "jetc_sema_cold0"].map(|name| format!("[profile.dev.package.{name}]"))
    );
    assert!(!dev.contains("opt-level"));
}

/// The compiler image a `runner` run wrote beside its emitted source `output`
/// (`<output>.image`); the emitted compiler source embeds it from
/// `compiler.image`.
fn generated_compiler_image(output: &Path) -> Vec<u8> {
    let path = output.with_extension("image");
    fs::read(&path).unwrap_or_else(|error| {
        panic!("generated compiler did not write its image `{}`: {error}", path.display())
    })
}

fn build_backend_artifact(
    repo: &Path,
    project: &Path,
    package_name: &str,
    source: &str,
) -> (PathBuf, String) {
    build_backend_artifact_with_native_library(repo, project, package_name, source, None, None, None)
}

/// Write the backend Cargo project `build_backend_artifact_with_native_library`
/// builds (main crate, unit-split workspace, compiler image, manifest, identity
/// build script) and return its manifest path. `compiler_image` is the image a
/// compiler artifact's source embeds with `include_bytes!("compiler.image")`:
/// it is written beside `src/main.rs`.
fn write_backend_project(
    repo: &Path,
    project: &Path,
    package_name: &str,
    source: &str,
    compiler_image: Option<&[u8]>,
    native_library: Option<(&Path, &str)>,
    ffi: Option<&BackendFfiCrate>,
) -> PathBuf {
    let source_dir = project.join("src");
    fs::create_dir_all(&source_dir).unwrap_or_else(|error| {
        panic!("cannot create backend project `{}`: {error}", project.display())
    });
    let source_path = source_dir.join("main.rs");
    let manifest_path = project.join("Cargo.toml");
    let build_script_path = project.join("build.rs");
    let profile = backend_release_profile();
    // The build identity hashes the complete emitted text (`generated.rs`):
    // every unit crate below is a pure function of it.
    let generated_path = project.join("generated.rs");
    fs::write(&generated_path, source).unwrap_or_else(|error| {
        panic!("cannot write backend source `{}`: {error}", generated_path.display())
    });
    // A program with a runtime block builds as a workspace: the runtime
    // crate and one crate per unit live in a stable directory per artifact
    // (outside the repository's cargo workspace), rewritten only when their
    // bytes change, so unchanged crates stay built across runs and programs
    // that share a runtime/Core block share its built crate.
    let (main_source, unit_dependencies, unit_profiles) = match split_backend_workspace(source) {
        Some(workspace) => {
            let units_root = home_path()
                .join(BOOTSTRAP_UNITS_RELATIVE)
                .join(package_name);
            let dependencies = write_backend_workspace(&units_root, &workspace, ffi);
            let profiles = backend_unit_profiles(&workspace, profile);
            (workspace.main, dependencies, profiles)
        }
        None => (source.to_string(), String::new(), String::new()),
    };
    let ffi_dependency = ffi.map_or_else(String::new, BackendFfiCrate::dependency);
    fs::write(&source_path, main_source).unwrap_or_else(|error| {
        panic!("cannot write backend source `{}`: {error}", source_path.display())
    });
    let image_path = source_dir.join("compiler.image");
    match compiler_image {
        Some(image) => fs::write(&image_path, image).unwrap_or_else(|error| {
            panic!("cannot write backend compiler image `{}`: {error}", image_path.display())
        }),
        None => match fs::remove_file(&image_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot remove stale backend compiler image `{}`: {error}", image_path.display()),
        },
    }
    // The backend project is its own workspace root, so it repeats the
    // repository's `[patch.crates-io]` (the vendored cranelift-jit that
    // jet-jit needs) and starts from the repository lockfile, keeping every
    // dependency at the version the repository builds with.
    let manifest = format!(
        "[package]\nname = {:?}\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[dependencies]\njet = {{ path = {:?} }}\njet-codegen = {{ path = {:?} }}\njet-driver = {{ path = {:?} }}\njet-foundation = {{ path = {:?} }}\njet-jit = {{ path = {:?} }}\njet-net = {{ path = {:?} }}\njet-pkg-model = {{ path = {:?} }}\njet-rt = {{ path = {:?} }}\njet-store = {{ path = {:?} }}\n{ffi_dependency}{unit_dependencies}\n[patch.crates-io]\ncranelift-jit = {{ path = {:?} }}\n{unit_profiles}{build_profile}",
        package_name,
        repo.display().to_string(),
        // The linked Host modules name these crates directly (codegen's native
        // loop cursor key, the package model's devtools policy, the record store).
        repo.join("crates/jet-codegen").display().to_string(),
        repo.join("crates/jet-driver").display().to_string(),
        repo.join("crates/jet-foundation").display().to_string(),
        repo.join("crates/jet-jit").display().to_string(),
        repo.join("crates/jet-net").display().to_string(),
        repo.join("crates/jet-pkg-model").display().to_string(),
        repo.join("crates/jet-rt").display().to_string(),
        repo.join("crates/jet-store").display().to_string(),
        repo.join("crates/vendor/cranelift-jit-0.112.3").display().to_string(),
        ffi_dependency = ffi_dependency,
        unit_dependencies = unit_dependencies,
        unit_profiles = unit_profiles,
        build_profile = if profile.is_some() { BACKEND_RELEASE_PROFILE } else { BACKEND_DEV_HOST_PROFILE },
    );
    fs::copy(repo.join("Cargo.lock"), project.join("Cargo.lock")).unwrap_or_else(|error| {
        panic!("cannot seed backend lockfile in `{}`: {error}", project.display())
    });
    fs::write(&manifest_path, manifest).unwrap_or_else(|error| {
        panic!("cannot write backend manifest `{}`: {error}", manifest_path.display())
    });
    fs::write(
        &build_script_path,
        backend_identity_build_script(repo, native_library),
    )
    .unwrap_or_else(|error| {
        panic!(
            "cannot write backend identity build script `{}`: {error}",
            build_script_path.display()
        )
    });
    manifest_path
}

fn build_backend_artifact_with_native_library(
    repo: &Path,
    project: &Path,
    package_name: &str,
    source: &str,
    compiler_image: Option<&[u8]>,
    native_library: Option<(&Path, &str)>,
    ffi: Option<&BackendFfiCrate>,
) -> (PathBuf, String) {
    let manifest_path =
        write_backend_project(repo, project, package_name, source, compiler_image, native_library, ffi);
    let target = repo.join("target");
    let profile = backend_release_profile();

    let mut command = Command::new(repo.join("Tools/agent/jet-env"));
    command
        .current_dir(repo)
        .env("CARGO_TARGET_DIR", &target)
        // jet-env sets CARGO_INCREMENTAL from JET_CARGO_INCREMENTAL. Dev
        // builds are incremental: a unit crate rebuilt only because a crate it
        // depends on changed reuses its cached queries (measured on a 12.5 MB
        // shard: 14 s instead of 48 s; a one-literal edit inside it 34 s; peak
        // memory unchanged at about 2.8 GB). The release profile stays
        // non-incremental, as its manifest profile says.
        .env("JET_CARGO_INCREMENTAL", if profile.is_some() { "0" } else { "1" })
        .env("CARGO_BUILD_JOBS", if profile.is_some() { BACKEND_RELEASE_BUILD_JOBS } else { BACKEND_BUILD_JOBS })
        .env("RUST_MIN_STACK", BACKEND_RUSTC_STACK)
        .env("JET_NO_SCCACHE", "1")
        .env_remove("RUSTC_WRAPPER")
        .args([
            "cargo",
            "build",
            "--manifest-path",
            manifest_path.to_str().unwrap_or_else(|| {
                panic!("backend manifest path is not valid UTF-8: {}", manifest_path.display())
            }),
            "--bin",
            package_name,
        ]);
    if let Some(name) = profile {
        // Frame pointers keep `perf record -g` call graphs of the optimized
        // compiler usable (stage1/lib.sh stall profiles).
        let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
        command
            .args(["--profile", name])
            .env("RUSTFLAGS", format!("{flags} -C force-frame-pointers=yes").trim_start().to_string());
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("cannot invoke the permitted Rust backend: {error}"));
    assert_command_success(
        &format!("Rust backend for `{package_name}` (compiler ICE/101 on rejection)"),
        &output,
    );
    let binary = target.join(profile.unwrap_or("debug")).join(package_name);
    assert!(
        binary.is_file(),
        "permitted Rust backend reported success without producing `{}`",
        binary.display()
    );
    let identity_path = project.join(".bootstrap-artifact-id");
    let identity = fs::read_to_string(&identity_path).unwrap_or_else(|error| {
        panic!(
            "backend build did not produce canonical identity receipt `{}`: {error}",
            identity_path.display()
        )
    });
    let identity = identity.trim().to_string();
    assert!(
        !identity.is_empty(),
        "backend identity receipt `{}` is empty",
        identity_path.display()
    );
    (binary, identity)
}

fn run_generated_artifact(
    binary: &Path,
    mode: &str,
    source_root: &Path,
    entry: &str,
    output_path: &Path,
    receipt_path: &Path,
) {
    let result = Command::new(binary)
        .env("JET_BOOTSTRAP_MODE", mode)
        .env("JET_BOOTSTRAP_SOURCE_ROOT", source_root)
        .env("JET_BOOTSTRAP_ENTRY", entry)
        .env("JET_BOOTSTRAP_OUTPUT", output_path)
        .env("JET_BOOTSTRAP_RECEIPT", receipt_path)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "cannot execute generated Jet {mode} artifact `{}`: {error}",
                binary.display()
            )
        });
    assert_command_success(
        &format!("generated Jet {mode} artifact `{}`", binary.display()),
        &result,
    );
    assert!(
        receipt_path.is_file(),
        "generated Jet {mode} artifact did not produce receipt `{}`",
        receipt_path.display()
    );
}

fn assert_aot_provenance(receipt: &str) {
    assert!(
        receipt.lines().any(|line| line == "selected_factory_tier=Aot"),
        "factory receipt selected a tier other than Aot"
    );
    assert!(
        receipt.lines().any(|line| line == "actual_factory_tier=Aot"),
        "factory receipt ran a tier other than Aot"
    );
}

fn assert_factory_receipt(path: &Path) {
    let receipt = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read factory receipt `{}`: {error}", path.display()));
    assert!(receipt.lines().any(|line| line == "mode=factory"));
    assert!(receipt.lines().any(|line| line == "complete=true"));
    assert_aot_provenance(&receipt);
    assert_receipt_count_at_least(&receipt, "source_bytes", 1);
    assert_receipt_count_at_least(&receipt, "callables", 1);
}

fn assert_runner_receipt(path: &Path) {
    let receipt = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read Runner receipt `{}`: {error}", path.display()));
    assert!(receipt.lines().any(|line| line == "mode=runner"));
    assert!(receipt.lines().any(|line| line == "complete=true"));
    assert_aot_provenance(&receipt);
    assert_receipt_count_at_least(&receipt, "source_bytes", 1);
}

fn assert_task_roots_receipt(output_path: &Path, receipt_path: &Path) {
    let output = fs::read_to_string(output_path).unwrap_or_else(|error| {
        panic!(
            "cannot read task-root fixture output `{}`: {error}",
            output_path.display()
        )
    });
    assert_eq!(output, "task-roots=passed\n");
    let receipt = fs::read_to_string(receipt_path).unwrap_or_else(|error| {
        panic!(
            "cannot read task-root fixture receipt `{}`: {error}",
            receipt_path.display()
        )
    });
    assert_eq!(
        receipt,
        "mode=task-roots\nstatus=passed\nselected_factory_tier=Aot\nactual_factory_tier=Aot\ncallback_capture_params=1\ncallback_escapes=true\ncallback_moves=0\ncursor_close_count=1\nshared_producer_machine_retired=true\nshared_parent_retirement_requested=true\nshared_parent_retirement_deferred=true\nshared_consumer_completed=true\nshared_payload_finalizer_completed=true\nretained_roots=0\n"
    );
}


fn assert_optional_codec_roundtrip(binary: &Path, project: &Path, session: &Path, stage: &str) {
    let output = session.join(format!("{stage}-optional-roundtrip.txt"));
    let receipt = session.join(format!("{stage}-optional-roundtrip.receipt"));
    run_generated_artifact(
        binary,
        "optional-roundtrip",
        project,
        SMALL_ENTRY_RELATIVE,
        &output,
        &receipt,
    );
    let receipt_text = fs::read_to_string(&receipt)
        .unwrap_or_else(|error| panic!("cannot read {stage} optional codec receipt: {error}"));
    assert!(receipt_text.lines().any(|line| line == "mode=optional-roundtrip"));
    assert!(receipt_text.lines().any(|line| line == "identity_absent=roundtrip"));
    assert!(receipt_text.lines().any(|line| line == "identity_present=roundtrip"));
    assert_eq!(
        fs::read_to_string(&output)
            .unwrap_or_else(|error| panic!("cannot read {stage} optional codec output: {error}")),
        "identity=absent,present\n"
    );
}
fn assert_mir_optimizer_compiler_source(binary: &Path, compiler_project: &Path, session: &Path) {
    let output = session.join("compiler-source-optimizer-evidence.out");
    let receipt = session.join("compiler-source-optimizer-evidence.receipt");
    run_generated_artifact(
        binary,
        "optimizer-evidence",
        compiler_project,
        BOOTSTRAP_ENTRY_RELATIVE,
        &output,
        &receipt,
    );
    let evidence = fs::read_to_string(&receipt)
        .unwrap_or_else(|error| panic!("cannot read compiler-source optimizer evidence: {error}"));
    assert_eq!(
        evidence,
        "mode=optimizer-evidence\nsource_file=Compiler/JetOptimizer/Source/Pipeline.jet\noptimizer_entry=optimize_mir_program\ninline_pass=mir_pass_expand_inline_always\ninline_pass_call_retained=true\n",
        "stage-two MIR did not retain the compiler-source optimizer entry and inline pass"
    );
}


fn receipt_reports(path: &Path) -> Vec<String> {
    let receipt = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read bootstrap receipt `{}`: {error}", path.display()));
    receipt
        .lines()
        .filter_map(|line| line.strip_prefix("report_json="))
        .map(str::to_string)
        .collect()
}

fn assert_receipt_count_at_least(receipt: &str, key: &str, minimum: usize) {
    let value = receipt
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("bootstrap receipt is missing `{key}`"))
        .parse::<usize>()
        .unwrap_or_else(|error| panic!("bootstrap receipt `{key}` is not numeric: {error}"));
    assert!(
        value >= minimum,
        "bootstrap receipt `{key}` was {value}, expected at least {minimum}"
    );
}

fn assert_small_program_runs(binary: &Path, stage: &str) {
    let output = Command::new(binary)
        .output()
        .unwrap_or_else(|error| panic!("cannot execute {stage} small Jet program: {error}"));
    assert_command_success(&format!("{stage} small Jet program"), &output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["bootstrap-small-program", "π🙂", "2", "6"],
        "{stage} generated compiler must execute String/List/Bytes materialization copies: {stdout:?}"
    );
}

fn assert_command_success(label: &str, output: &Output) {
    if output.status.success() {
        return;
    }
    panic!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
