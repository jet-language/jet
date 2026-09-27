use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Codegen::MIRRust::{
    MirRustConfig, MirRustExecutionConfig, MirRustTarget,
};
use jet_foundation::Layout::TargetLayout;
use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget, MirOperation,
};

const BOOTSTRAP_PROJECT_RELATIVE: &str = ".cache/jet-luna/compiler-bootstrap/project";
const BOOTSTRAP_ENTRY_RELATIVE: &str = "src/compiler.jet";
const SMALL_ENTRY_RELATIVE: &str = "main.jet";
const SMALL_PROGRAM_SOURCE: &str = r#"@SOURCE_TEXT :: "π🙂"
@SOURCE_LIST :: [Int]{7, 8}
@SOURCE_BYTES :: @SOURCE_TEXT.bytes()
@MATERIALIZED_TEXT :: ~(@SOURCE_TEXT.after(""))
@MATERIALIZED_LIST :: ~(@SOURCE_LIST[0..<@SOURCE_LIST.len()])
@MATERIALIZED_BYTES :: ~(@SOURCE_BYTES[0..<@SOURCE_BYTES.len()])
@MATERIALIZED_LIST_LEN :: @MATERIALIZED_LIST.len()
@MATERIALIZED_BYTES_LEN :: @MATERIALIZED_BYTES.len()

pub fn main() {
    print("bootstrap-small-program")
    print("{@MATERIALIZED_TEXT}")
    print("{@MATERIALIZED_LIST_LEN}")
    print("{@MATERIALIZED_BYTES_LEN}")
}
"#;
const SMALL_PROGRAM_MANIFEST: &str = r#"name: "bootstrap_small"
version: "0.1.0"
edition: "2028"
outputs: { app: .Executable{ entry: main } }
"#;

const CARRIER_FIXTURE_SOURCE: &str =
    include_str!("../../examples/features/contracts/carrier_binding.jet");
const CARRIER_FIXTURE_EXPECTED: &str =
    include_str!("../../examples/features/expected/contracts/carrier_binding.out");
const SOURCE_FIXTURE_MANIFEST: &str = r#"name: "bootstrap_source_fixture"
version: "0.1.0"
edition: "2028"
outputs: { app: .Executable{ entry: run } }
"#;
const USER_OPERATOR_FIXTURE_SOURCE: &str =
    include_str!("../../examples/features/operators/user_defined.jet");
const USER_OPERATOR_FIXTURE_EXPECTED: &str = "4,6 4,6 true true false\n";
const SPACESHIP_FIXTURE_SOURCE: &str =
    include_str!("../../examples/features/operators/spaceship.jet");
const SPACESHIP_FIXTURE_EXPECTED: &str =
    "int less: true\nstring less: true\nAda:10\nCal:20\nBea:30\n";
const MIXED_OPERATOR_FIXTURE_SOURCE: &str =
    include_str!("../../examples/features/operators/mixed_types.jet");
const MIXED_OPERATOR_FIXTURE_EXPECTED: &str =
    include_str!("../../examples/features/expected/operators/mixed_types.out");
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

fn inspect(handle: ^Handle) I64 {
    return c.read(handle)
}

fn make() Handle {
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
const EXACT_DIVISION_FIXTURE_TWO: &str = r#"@TEN :: 10
@THIRD :: @TEN / 3
fn run() {
    runtime :: 10 / 3
    print(@THIRD)
    print(@THIRD == runtime)
    print(@THIRD * 3 == 10)
}
"#;
const EXACT_DIVISION_FIXTURE_ONE_EXPECTED: &str = "1/3\ninterpolated 1/3\ntrue\n";
const EXACT_DIVISION_FIXTURE_TWO_EXPECTED: &str = "10/3\ntrue\ntrue\n";
/// This is appended to every compiler artifact built by the private harness.
/// It is an executable harness entry, not a compiler callback: the generated
/// Jet factory and the packaged Runner remain the only compilation path.
const GENERATED_ARTIFACT_MAIN: &str = r#"
#[doc(hidden)]
fn main() {
    let mode = std::env::var("JET_BOOTSTRAP_MODE")
        .unwrap_or_else(|error| panic!("bootstrap mode is unavailable: {error}"));
    let source_root = std::env::var("JET_BOOTSTRAP_SOURCE_ROOT")
        .unwrap_or_else(|error| panic!("bootstrap source root is unavailable: {error}"));
    let entry = std::env::var("JET_BOOTSTRAP_ENTRY")
        .unwrap_or_else(|error| panic!("bootstrap entry is unavailable: {error}"));
    let output_path = std::env::var("JET_BOOTSTRAP_OUTPUT")
        .unwrap_or_else(|error| panic!("bootstrap output path is unavailable: {error}"));
    let receipt_path = std::env::var("JET_BOOTSTRAP_RECEIPT")
        .unwrap_or_else(|error| panic!("bootstrap receipt path is unavailable: {error}"));
    let lease = crate::compiler_bootstrap_host::open_authorized_sources(
        std::path::Path::new(&source_root),
        std::path::Path::new(&entry),
    )
    .unwrap_or_else(|error| panic!("authorized bootstrap source snapshot failed: {error:?}"));
    if mode == "offset-projection" {
        let projected = crate::__jet_bootstrap_project_span(
            &::jet_foundation::Diagnostics::Span {
                start: 1200,
                end: 1204,
            },
            1200,
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
        assert!(absent_carrier.identity.as_ref().is_err());
        assert!(present_carrier.identity.as_ref().is_ok());
        let absent_roundtrip = crate::__jet_bootstrap_type_to_host(&absent_carrier)
            .unwrap_or_else(|error| panic!("absent type identity decoding failed: {error}"));
        let present_roundtrip = crate::__jet_bootstrap_type_to_host(&present_carrier)
            .unwrap_or_else(|error| panic!("present type identity decoding failed: {error}"));
        assert_eq!(absent_roundtrip.identity, None);
        assert_eq!(present_roundtrip.identity, present.identity);
        std::fs::write(&output_path, "identity=absent,present\n")
            .unwrap_or_else(|error| panic!("cannot write optional codec result: {error}"));
        std::fs::write(
            &receipt_path,
            "mode=optional-roundtrip\nidentity_absent=roundtrip\nidentity_present=roundtrip\n",
        )
        .unwrap_or_else(|error| panic!("cannot write optional codec receipt: {error}"));
        return;
    }


    let (complete, source, callable_count, type_count, field_count, variant_count, reports) =
        if mode == "factory" {
            let result = crate::compiler_bootstrap_runner::invoke_bootstrap_entry(
                &lease,
                |snapshot| crate::__jet_bootstrap_compile_from_host(snapshot, crate::__jet_bootstrap_native_compile_target()),
            )
            .unwrap_or_else(|error| panic!("generated Jet factory authority call failed: {error:?}"))
            .unwrap_or_else(|error| panic!("generated Jet compiler codec failed: {error:?}"));
            if let Some(resources) = result.resources.as_ref() {
                resources.retire().unwrap_or_else(|error| panic!("generated Jet factory source resource retirement failed: {error}"));
            }
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
            )
        } else if mode == "runner" {
            let execution = crate::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(
                ::jet_foundation::MIR::MirArtifactId(0),
            );
            let config = crate::Codegen::MIRRust::MirRustConfig {
                target: ::jet_foundation::Layout::TargetLayout::host(),
                target_kind: crate::Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: "crate::".to_string(),
                execution,
            };
            let result = crate::__jet_bootstrap_run_from_host(
                &lease,
                &config,
                |snapshot| crate::__jet_bootstrap_compile_from_host(snapshot, crate::__jet_bootstrap_native_compile_target()),
                |artifact| match artifact {
                    crate::BootstrapBackendArtifact::NativeRust { source, .. } => source,
                    crate::BootstrapBackendArtifact::Web { .. } => panic!("Runner fixture unexpectedly produced a Web artifact"),
                },
                None,
            )
                .unwrap_or_else(|error| panic!("generated Jet Runner failed: {error:?}"));
            (
                result.complete,
                result.backend,
                0,
                0,
                0,
                0,
                result.reports,
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

#[test]
fn bootstrap_private_self_compile_harness() {
    let repo = Path::new(crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT);
    let session = bootstrap_session_root();
    fs::create_dir_all(&session).unwrap_or_else(|error| {
        panic!(
            "cannot create private bootstrap cache `{}`: {error}",
            session.display()
        )
    });

    assemble_compiler_sources(repo);
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
    let (reference_output, bundle) = crate::Driver::compile_bundle_path_opts_with_source_closure_and_runtime(
        &compiler_snapshot.entry_path,
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
        &source_closure,
        None,
    )
    .unwrap_or_else(|diagnostics| panic!("stage-zero Rust frontend rejected compiler sources: {diagnostics:?}"));
    let reference_rust = reference_output.rust;
    let reference_ffi = reference_output.ffi;
    source_lease
        .revalidate()
        .unwrap_or_else(|error| panic!("compiler source authority changed after stage zero: {error:?}"));

    let request = MirArtifactRequest::new(
        MirArtifactTarget::RustAot,
        MirArtifactKind::NativeExecutable,
        MirArtifactBuildMode::Dev,
    );
    let (mir, artifact) = crate::lower_checked_semantic_mir_program_for(&bundle, request);
    assert_evaluator_materialization_helpers_in_mir(&mir);
    let mut execution = MirRustExecutionConfig::for_artifact(artifact);
    execution.ffi = reference_ffi.as_ref();
    execution.emit_types = true;
    execution.emit_foreign = true;
    execution.emit_metadata = false;
    execution.emit_runtime = true;
    execution.release_devtools_policy =
        crate::Driver::release_devtools_policy_for_bundle(&bundle, "dev");
    let metadata_config = MirRustConfig {
        target: TargetLayout::from_build_facts(&bundle.build_facts),
        target_kind: MirRustTarget::Native,
        root_prefix: String::new(),
        execution: execution.clone(),
    };
    let prepare_config = MirRustConfig {
        target: metadata_config.target.clone(),
        target_kind: MirRustTarget::Native,
        root_prefix: "crate::".to_string(),
        execution,
    };
    let metadata = crate::Codegen::MIRRust::mir_rust_aot_metadata(&mir, &metadata_config);
    let prepared = crate::prepare_bootstrap_artifact_from_aot(
        reference_rust,
        &mir,
        &compiler_snapshot,
        &prepare_config,
        &metadata,
    )
    .unwrap_or_else(|error| panic!("stage-zero Host/Runner packaging failed: {error}"));
    let stage_zero_source = append_generated_artifact_main(prepared.source);
    let stage_zero_project = session.join("stage-zero");
    let (stage_zero_binary, stage_zero_id) = build_backend_artifact(
        repo,
        &stage_zero_project,
        "jet_bootstrap_stage_zero",
        &stage_zero_source,
    );

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
    );
    let stage_one_project = session.join("stage-one");
    let (stage_one_binary, stage_one_id) = build_backend_artifact(
        repo,
        &stage_one_project,
        "jet_bootstrap_stage_one",
        &stage_one_source,
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
    );
    fs::write(&stage_two_source, &stage_two_source_text).unwrap_or_else(|error| {
        panic!("cannot retain stage-two source `{}`: {error}", stage_two_source.display())
    });
    let stage_two_project = session.join("stage-two");
    let (stage_two_binary, stage_two_id) = build_backend_artifact(
        repo,
        &stage_two_project,
        "jet_bootstrap_stage_two",
        &stage_two_source_text,
    );
    assert_ne!(
        stage_one_id, stage_two_id,
        "distinct generated compiler artifacts must not reuse one identity"
    );
    assert_optional_codec_roundtrip(&stage_two_binary, &small_project, &session, "stage-two");

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

    let provenance = session.join("bootstrap.provenance");
    let provenance_text = format!(
        "schema=jet-private-bootstrap/v1\ncompiler_project={}\nstage_zero_id={stage_zero_id}\nstage_zero_source={}\nstage_one_id={stage_one_id}\nstage_one_source={}\nstage_two_id={stage_two_id}\nstage_two_source={}\nstage_two_backend={}\nstage_two_small_backend={}\nsmall_entry={}\nbackend=scripts/agent/jet-env cargo build --manifest-path <cache-project>/Cargo.toml --bin <cache-binary>\n",
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


fn assemble_compiler_sources(repo: &Path) {
    let output = Command::new(repo.join("scripts/agent/jet-env"))
        .current_dir(repo)
        .args(["node", "Compiler/Bootstrap/assemble.mjs"])
        .output()
        .unwrap_or_else(|error| panic!("cannot run bootstrap assembler: {error}"));
    assert_command_success("bootstrap assembler", &output);
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
        native_library,
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
                            materialize_view: true,
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

fn append_generated_artifact_main(source: String) -> String {
    let mut complete = source;
    complete.push_str(GENERATED_ARTIFACT_MAIN);
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
    format!(
        r#"use std::fs;
use std::path::Path;

fn main() {{
    let root = Path::new({:?});
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read(manifest_dir.join("src/main.rs"))
        .expect("generated artifact source must be readable");
    let manifest = fs::read(manifest_dir.join("Cargo.toml"))
        .expect("generated artifact manifest must be readable");
    let build_script = fs::read(manifest_dir.join("build.rs"))
        .expect("generated artifact build script must be readable");
    let facts = jet::BootstrapBuildIdentity::build_facts()
        .expect("Cargo-supplied backend build facts must be readable");
    let extras = vec![
        ("Compiler/Bootstrap/__generated_compiler.rs".to_string(), source),
        ("Compiler/Bootstrap/__generated_cargo_manifest.toml".to_string(), manifest),
        ("Compiler/Bootstrap/__generated_cargo_build.rs".to_string(), build_script),
    ];
    let identity = jet::BootstrapBuildIdentity::semantic_id_with_extra(
        root,
        jet::BootstrapBuildIdentity::COMPILER_DOMAIN,
        jet::BootstrapBuildIdentity::COMPILER_SOURCES,
        &facts,
        &extras,
    )
    .expect("generated compiler identity must be computable");
    fs::write(manifest_dir.join(".bootstrap-artifact-id"), &identity)
        .expect("generated compiler identity receipt must be writable");
    println!("cargo:rustc-env=JET_COMPILER_BUILD_ID={{identity}}");
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("src/main.rs").display());
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("Cargo.toml").display());
    println!("cargo:rerun-if-changed={{}}", manifest_dir.join("build.rs").display());
{native_link_directives}}}
"#,
        repo.display().to_string(),
        native_link_directives = native_link_directives,
    )
}
fn build_backend_artifact(
    repo: &Path,
    project: &Path,
    package_name: &str,
    source: &str,
) -> (PathBuf, String) {
    build_backend_artifact_with_native_library(repo, project, package_name, source, None)
}

fn build_backend_artifact_with_native_library(
    repo: &Path,
    project: &Path,
    package_name: &str,
    source: &str,
    native_library: Option<(&Path, &str)>,
) -> (PathBuf, String) {
    let source_dir = project.join("src");
    fs::create_dir_all(&source_dir).unwrap_or_else(|error| {
        panic!("cannot create backend project `{}`: {error}", project.display())
    });
    let source_path = source_dir.join("main.rs");
    let manifest_path = project.join("Cargo.toml");
    let build_script_path = project.join("build.rs");
    fs::write(&source_path, source).unwrap_or_else(|error| {
        panic!("cannot write backend source `{}`: {error}", source_path.display())
    });
    let manifest = format!(
        "[package]\nname = {:?}\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[dependencies]\njet = {{ path = {:?} }}\njet-driver = {{ path = {:?} }}\njet-foundation = {{ path = {:?} }}\njet-jit = {{ path = {:?} }}\njet-net = {{ path = {:?} }}\njet-rt = {{ path = {:?} }}\n\n[build-dependencies]\njet = {{ path = {:?} }}\n",
        package_name,
        repo.display().to_string(),
        repo.join("crates/jet-driver").display().to_string(),
        repo.join("crates/jet-foundation").display().to_string(),
        repo.join("crates/jet-jit").display().to_string(),
        repo.join("crates/jet-net").display().to_string(),
        repo.join("crates/jet-rt").display().to_string(),
        repo.display().to_string(),
    );
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

    let target = repo.join("target");
    let output = Command::new(repo.join("scripts/agent/jet-env"))
        .current_dir(repo)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
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
        ])
        .output()
        .unwrap_or_else(|error| panic!("cannot invoke the permitted Rust backend: {error}"));
    assert_command_success(
        &format!("Rust backend for `{package_name}` (compiler ICE/101 on rejection)"),
        &output,
    );
    let binary = target.join("debug").join(package_name);
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

fn assert_factory_receipt(path: &Path) {
    let receipt = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read factory receipt `{}`: {error}", path.display()));
    assert!(receipt.lines().any(|line| line == "mode=factory"));
    assert!(receipt.lines().any(|line| line == "complete=true"));
    assert_receipt_count_at_least(&receipt, "source_bytes", 1);
    assert_receipt_count_at_least(&receipt, "callables", 1);
}

fn assert_runner_receipt(path: &Path) {
    let receipt = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read Runner receipt `{}`: {error}", path.display()));
    assert!(receipt.lines().any(|line| line == "mode=runner"));
    assert!(receipt.lines().any(|line| line == "complete=true"));
    assert_receipt_count_at_least(&receipt, "source_bytes", 1);
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
