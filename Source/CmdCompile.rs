//! check / build / run / test / new / fmt / fix subcommand handlers + the
//! rustc bridge.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{exit, Command, Stdio};
use std::sync::{mpsc, LazyLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::OutputAdapter::{
    write_mode_diagnostic, write_mode_machine, write_mode_progress, write_mode_renderable,
    write_mode_status,
};
use crate::{report_problems, usage, BuildProfile, OutputMode, ProfileConfig};
use jet::ExitCodes;
use jet::RecordIndex::{
    RecordCapture, RecordIdentity, RecordIndex, RecordIndexEntry, RecordKind, RecordLink,
};
use jet_foundation::DataTree::DataTree;
use jet_foundation::Report::{
    render_status, render_status_with_reports, StatusEnvelope, StatusFields, StatusValue,
};
use jet_foundation::JSON::json_escape;
use jet_foundation::JSON::parse_json;
use jet_store::{ArtifactRestore, Store};

struct BuildProgress<'profile> {
    enabled: bool,
    verbose: bool,
    started: Instant,
    mode: OutputMode,
    progress: Option<jet_cli::MultiProgress::MultiProgress<'profile>>,
    task: Option<jet_cli::MultiProgress::TaskId>,
}

impl<'profile> BuildProgress<'profile> {
    fn new(
        cmd: &str,
        emit_rust: bool,
        verbose: bool,
        mode: OutputMode,
        profile: Option<&'profile jet_cli::OutputProfile::OutputProfile>,
    ) -> Self {
        let enabled = cmd == "build" && !emit_rust && !mode.quiet && !mode.json;
        let mut progress = enabled
            .then_some(profile)
            .flatten()
            .map(jet_cli::MultiProgress::MultiProgress::new);
        let task = progress
            .as_mut()
            .and_then(|progress| progress.add_unknown("build").ok());
        Self {
            enabled,
            verbose,
            started: Instant::now(),
            mode,
            progress,
            task,
        }
    }

    fn render(&mut self, phase: &str, detail: &str) {
        let (Some(progress), Some(task)) = (self.progress.as_mut(), self.task) else {
            return;
        };
        if progress.set_custom_column(task, "phase", phase).is_err()
            || progress.set_custom_column(task, "detail", detail).is_err()
        {
            return;
        }
        if let Ok(Some(frame)) = progress.frame() {
            let text = frame.text;
            if !text.is_empty() {
                write_mode_progress(self.mode, &format!("{text}\n"));
            }
        }
    }

    fn major(&mut self, label: &str, detail: &str) {
        if self.enabled {
            self.render(label, detail);
        }
    }

    fn minor(&mut self, label: &str, detail: &str) {
        if self.enabled && self.verbose {
            self.render(label, detail);
        }
    }

    fn finish(&mut self, artifact: &str) {
        if !self.enabled {
            return;
        }
        let elapsed = self.started.elapsed();
        let duration = if elapsed.as_secs() == 0 {
            format!("{}ms", elapsed.as_millis())
        } else {
            format!("{:.1}s", elapsed.as_secs_f64())
        };
        if let (Some(progress), Some(task)) = (self.progress.as_mut(), self.task) {
            let _ = progress.set_custom_column(task, "artifact", artifact);
            let _ = progress.set_custom_column(task, "elapsed", &duration);
            let _ = progress.finish(task);
            if let Ok(Some(frame)) = progress.frame() {
                let text = frame.text;
                if !text.is_empty() {
                    write_mode_progress(self.mode, &format!("{text}\n"));
                }
            }
        }
    }
}

/// `RunOutcome` carries stdout and stderr separately. Flush the first stream
/// before handing the second to the OS so the Prelude's write order survives
/// the CLI adapter when both streams share a non-TTY sink.
fn emit_run_output(stdout: &str, stderr: &str) {
    print!("{stdout}");
    if !stderr.is_empty() {
        let _ = std::io::stdout().flush();
        eprint!("{stderr}");
    }
}
fn render_diagnostic_status(
    action: &str,
    ok: bool,
    file: &jet::Diagnostics::ReportPath,
    source: &str,
    diagnostics: &[jet::Diagnostics::Diagnostic],
) -> String {
    let clears = jet::Diagnostics::report_clear_counts(diagnostics);
    let reports = diagnostics
        .iter()
        .zip(clears)
        .map(|(diagnostic, clears)| diagnostic.to_report_with_clears(file, source, clears));
    format!(
        "{}\n",
        render_status_with_reports(action, ok, reports, StatusFields::new())
    )
}

fn index_compile_artifact(
    identity: RecordIdentity,
    kind: RecordKind,
    artifact_id: String,
    path: PathBuf,
    size: u64,
    capture: RecordCapture,
    consumed: Vec<RecordLink>,
    produced: Vec<RecordLink>,
) -> Result<(), String> {
    let mut index = RecordIndex::load_for_project(".")
        .map_err(|error| format!("could not load record index: {error}"))?;
    let (recorded_sequence, saved) = if let Some(entry) = index.find(kind, &artifact_id, true) {
        (entry.recorded_sequence, entry.saved)
    } else {
        (
            index
                .next_recorded_sequence()
                .map_err(|error| format!("could not allocate record sequence: {error}"))?,
            false,
        )
    };
    let entry = RecordIndexEntry::new(identity, kind, artifact_id.clone(), path)
        .map_err(|error| format!("could not construct {kind} record: {error}"))?
        .with_links(consumed, produced)
        .map_err(|error| format!("could not link {kind} record `{artifact_id}`: {error}"))?
        .with_capture(capture)
        .with_size(size)
        .with_recorded_sequence(recorded_sequence)
        .with_saved(saved);
    index
        .update_and_store(entry)
        .map_err(|error| format!("could not store {kind} record `{artifact_id}`: {error}"))
}

fn replay_artifact_id(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 16 {
        return Err("replay artifact is truncated before its header".into());
    }
    let header_len = u32::from_le_bytes(
        bytes[12..16]
            .try_into()
            .map_err(|_| "replay header length is invalid".to_string())?,
    ) as usize;
    let header_end = 16usize
        .checked_add(header_len)
        .ok_or_else(|| "replay header length overflows".to_string())?;
    if header_end > bytes.len() {
        return Err("replay artifact header is truncated".into());
    }
    let header = std::str::from_utf8(&bytes[16..header_end])
        .map_err(|_| "replay artifact header is not UTF-8".to_string())?;
    let DataTree::Object(fields) =
        parse_json(header).map_err(|_| "replay artifact header is not valid JSON".to_string())?
    else {
        return Err("replay artifact header is not a JSON object".into());
    };
    let Some(artifact_id) = fields
        .iter()
        .find(|(key, _)| key == "artifact_id")
        .and_then(|(_, value)| value.as_str().ok())
    else {
        return Err("replay artifact header has no artifact_id".into());
    };
    Ok(artifact_id.to_string())
}

fn project_relative_record_path(path: &Path) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let relative = if path.is_absolute() {
        path.strip_prefix(&cwd)
            .map_err(|_| format!("record artifact is outside the project: {}", path.display()))?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "record artifact path is not project-relative: {}",
            path.display()
        ));
    }
    Ok(relative)
}

fn index_named_capture(
    capture: &crate::ProveReplay::NamedCapture,
    name: &str,
    capture_mode: RecordCapture,
) -> Result<(), String> {
    let path = PathBuf::from(format!(".jet/replays/{name}.jetproof-replay"));
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "could not read replay artifact `{}`: {error}",
            path.display()
        )
    })?;
    let artifact_id = replay_artifact_id(&bytes)?;
    let identity = capture.record_identity()?;
    let size =
        u64::try_from(bytes.len()).map_err(|_| "replay artifact is too large".to_string())?;
    index_compile_artifact(
        identity,
        RecordKind::Replay,
        artifact_id,
        path,
        size,
        capture_mode,
        Vec::new(),
        Vec::new(),
    )
}

fn index_production_receipt(context: &crate::ProductionReceipt::Context) -> Result<(), String> {
    let Some(directory) =
        std::env::var_os(jet::development_receipt::JET_DEVELOPMENT_RECEIPT_DIRECTORY_ENV)
    else {
        return Ok(());
    };
    let absolute_path = PathBuf::from(directory).join("receipt");
    let metadata = match fs::symlink_metadata(&absolute_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not inspect production receipt: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "production receipt is not a regular file: {}",
            absolute_path.display()
        ));
    }
    let bytes = fs::read(&absolute_path)
        .map_err(|error| format!("could not read production receipt: {error}"))?;
    let path = project_relative_record_path(&absolute_path)?;
    let identity = context.record_identity()?;
    let artifact_id = format!("sha256-{}", jet::SHA256::sha256_hex(&bytes));
    let size =
        u64::try_from(bytes.len()).map_err(|_| "production receipt is too large".to_string())?;
    index_compile_artifact(
        identity,
        RecordKind::Receipt,
        artifact_id,
        path,
        size,
        RecordCapture::Safe,
        Vec::new(),
        Vec::new(),
    )
}

fn fail_record_index(error: String) -> ! {
    crate::cli_error!("E2105", "record index update failed: {error}");
    exit(ExitCodes::ICE);
}

/// Rebuild one indexed `jet-receipt-v2` invocation and compare its native
/// output with the artifact that invocation produced. Receipt inputs are
/// authenticated before any child build is launched; a changed or missing
/// input is reported in path order.
pub(crate) fn run_build_verify(artifact_id: &str, mode: OutputMode) -> ! {
    let fail = |message: String| -> ! {
        if mode.json {
            let diagnostic = jet::Diagnostics::Diagnostic::error(
                "E2105",
                format!("build verification failed: {message}"),
                "the indexed receipt could not be verified".to_string(),
                "repair the receipt or rerun the producing build".to_string(),
                None,
            );
            let rendered = render_diagnostic_status(
                "build.verify",
                false,
                &jet::Diagnostics::ReportPath::from_process("<cli>"),
                "",
                std::slice::from_ref(&diagnostic),
            );
            write_mode_machine(mode, &rendered);
        } else {
            crate::cli_error!("E2105", "build verification failed: {}", message);
        }
        exit(ExitCodes::USER_ERROR);
    };
    let index = RecordIndex::load_for_project(".")
        .unwrap_or_else(|error| fail(format!("could not load record index: {error}")));
    let indexed = index
        .find(RecordKind::Receipt, artifact_id, true)
        .unwrap_or_else(|| fail(format!("receipt `{artifact_id}` is not indexed")));
    let receipt_path = PathBuf::from(".").join(&indexed.path);
    let receipt = jet::ReceiptStore::read_path(&receipt_path)
        .unwrap_or_else(|error| fail(format!("could not read indexed receipt: {error}")));

    for input in &receipt.claim.inputs {
        let digest = match jet::SHA256::sha256_file_hex(&input.path) {
            Ok(digest) => digest,
            Err(error) => fail(format!(
                "first differing input `{}` is unavailable: {error}",
                input.path.display()
            )),
        };
        if digest != input.digest {
            fail(format!(
                "first differing input `{}`: expected sha256-{}, found sha256-{}",
                input.path.display(),
                input.digest,
                digest
            ));
        }
    }

    let target =
        indexed_receipt_target(&receipt, &indexed.identity).unwrap_or_else(|error| fail(error));
    let artifact = receipt_artifact_path(&receipt, &target);
    let before = fs::read(&artifact).unwrap_or_else(|error| {
        fail(format!(
            "recorded build output `{}` is unavailable: {error}",
            artifact.display()
        ))
    });
    if let Err(error) = rebuild_indexed_target(&target, mode) {
        fail(error);
    }
    let after = fs::read(&artifact).unwrap_or_else(|error| {
        fail(format!(
            "rebuilt output `{}` is unavailable: {error}",
            artifact.display()
        ))
    });

    if before != after {
        fail(format!(
            "rebuilt output `{}` differs although all {} receipt inputs matched; \
             the native toolchain/output diverged",
            artifact.display(),
            receipt.claim.inputs.len()
        ));
    }
    if mode.json {
        let rendered = render_status(
            "build.verify",
            true,
            StatusFields::new()
                .with("status", "identical")
                .with("inputs", receipt.claim.inputs.len())
                .with("outputs", 1usize)
                .with("sha256", jet::SHA256::sha256_hex(&after)),
        );
        write_mode_machine(mode, &format!("{rendered}\n"));
    } else {
        write_mode_renderable(
            mode,
            &format!(
                "identical: {} inputs, 1 output, sha256 match\n",
                receipt.claim.inputs.len()
            ),
        );
    }
    exit(ExitCodes::OK);
}

fn indexed_receipt_target(
    receipt: &jet::ReceiptStore::Receipt,
    identity: &RecordIdentity,
) -> Result<PathBuf, String> {
    receipt
        .claim
        .inputs
        .iter()
        .filter(|input| {
            input
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("jet")
        })
        .find_map(|input| {
            let path = input.path.to_string_lossy();
            crate::CmdProve::target_input_sha256_for_file(&path)
                .ok()
                .filter(|digest| digest == &identity.target_inputs_sha256)
                .map(|_| input.path.clone())
        })
        .ok_or_else(|| {
            "indexed receipt does not identify a rebuildable Jet target in its inputs".to_string()
        })
}

fn receipt_artifact_path(receipt: &jet::ReceiptStore::Receipt, target: &Path) -> PathBuf {
    let output = String::from_utf8_lossy(&receipt.stdout);
    output
        .lines()
        .find_map(|line| line.strip_prefix("built: "))
        .map(|line| {
            line.split_once(" (")
                .map(|(path, _)| path)
                .unwrap_or(line)
                .trim()
        })
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| build_artifact_path(&target.to_string_lossy(), None))
}

fn rebuild_indexed_target(target: &Path, mode: OutputMode) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate the current jet executable: {error}"))?;
    let output = Command::new(executable)
        .arg("build")
        .arg(target)
        .env("JET_RECEIPT_BYPASS", "1")
        .output()
        .map_err(|error| format!("could not rebuild `{}`: {error}", target.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            format!("child build exited with {}", output.status)
        } else {
            format!(
                "child build exited with {}: {}",
                output.status,
                stderr.trim()
            )
        };
        if !mode.quiet {
            write_mode_diagnostic(mode, &format!("{detail}\n"));
        }
        return Err(detail);
    }
    Ok(())
}

fn finish_recorded_artifacts(
    record: Option<&crate::ProveReplay::NamedCapture>,
    record_name: Option<&str>,
    production_receipt: Option<&crate::ProductionReceipt::Context>,
    exit_code: i32,
    mode: OutputMode,
) {
    if let Some(capture) = record {
        crate::ProveReplay::finish_named_capture(capture, exit_code, mode.json)
            .unwrap_or_else(|status| exit(status));
        if let Some(name) = record_name {
            index_named_capture(capture, name, RecordCapture::Safe)
                .unwrap_or_else(|error| fail_record_index(error));
        }
    }
    if let Some(context) = production_receipt {
        index_production_receipt(context).unwrap_or_else(|error| fail_record_index(error));
    }
}

fn finish_recorded_run_artifacts(
    capture: &crate::ProveReplay::NamedCapture,
    record_name: &str,
    production_receipt: Option<&crate::ProductionReceipt::Context>,
    exit_code: i32,
    mode: OutputMode,
    run: &jet::Debug::RecordedRun,
) {
    crate::ProveReplay::finish_named_capture_with_run(capture, exit_code, mode.json, run)
        .unwrap_or_else(|status| exit(status));
    index_named_capture(capture, record_name, RecordCapture::Safe)
        .unwrap_or_else(|error| fail_record_index(error));
    if let Some(context) = production_receipt {
        index_production_receipt(context).unwrap_or_else(|error| fail_record_index(error));
    }
}

fn try_recorded_run(
    file: &str,
    program_args: &[&str],
    capture: &crate::ProveReplay::NamedCapture,
    record_name: Option<&str>,
    production_receipt: Option<&crate::ProductionReceipt::Context>,
    mode: OutputMode,
) -> bool {
    let Ok(execution) = jet::Debug::record_run(file, program_args) else {
        return false;
    };
    emit_run_output(&execution.stdout, &execution.stderr);
    crate::ProveReplay::finish_named_capture_with_run(
        capture,
        execution.exit_code,
        mode.json,
        &execution.run,
    )
    .unwrap_or_else(|status| exit(status));
    if let Some(name) = record_name {
        index_named_capture(capture, name, RecordCapture::Safe)
            .unwrap_or_else(|error| fail_record_index(error));
    }
    if let Some(context) = production_receipt {
        index_production_receipt(context).unwrap_or_else(|error| fail_record_index(error));
    }
    exit(execution.exit_code);
}

fn render_internal_fault(what: &str) -> String {
    jet::Diagnostics::render_ice_report(what, "", false)
}

fn emit_internal_fault(stdout: &str, what: &str, mode: OutputMode) -> ! {
    emit_run_output(stdout, "");
    let _ = std::io::stdout().flush();
    write_mode_diagnostic(mode, &render_internal_fault(what));
    exit(ExitCodes::ICE);
}

fn exit_if_internal_fault(diagnostics: &[jet::Diagnostics::Diagnostic], mode: OutputMode) {
    if let Some((stdout, what)) = diagnostics
        .iter()
        .find_map(jet::Diagnostics::Diagnostic::runtime_host_fault_parts)
    {
        emit_internal_fault(stdout, what, mode);
    }
}

/// The one home for turning a finished child process into this process's exit
/// status. A harness child that stopped at runtime (`require`, `panic`) already
/// chose its status through `jet_runtime_boundary` — the CLI relays it instead
/// of inventing a second, weaker one. A termination by signal has no numeric
/// status, so it falls back to the driver-reported user error.
pub(crate) fn child_exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(ExitCodes::USER_ERROR)
}

fn vector_build_projection(
    file: &str,
) -> Vec<(String, usize, usize, String, String, String, String)> {
    let (diagnostics, bundle, _) = jet::Driver::check_file_with_effect_facts(file, None, false);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        return Vec::new();
    }
    let Some(bundle) = bundle else {
        return Vec::new();
    };
    let (mir, _) = jet::lower_checked_semantic_mir_program_for(
        &bundle,
        jet_foundation::MIR::MirArtifactRequest::new(
            jet_foundation::MIR::MirArtifactTarget::RustAot,
            jet_foundation::MIR::MirArtifactKind::NativeExecutable,
            jet_foundation::MIR::MirArtifactBuildMode::Dev,
        ),
    );
    mir.functions
        .iter()
        .flat_map(|function| {
            function.optimization.vector_facts.iter().map(|fact| {
                let decision = match &fact.decision {
                    jet_foundation::MIR::MirOptimizationDecision::Eligible => {
                        "accepted".to_string()
                    }
                    jet_foundation::MIR::MirOptimizationDecision::Rejected(reason) => {
                        reason.as_str().to_string()
                    }
                };
                (
                    function.key.clone(),
                    fact.span.start,
                    fact.span.end,
                    fact.rule.as_str().to_string(),
                    fact.layout.as_str().to_string(),
                    fact.lane_width
                        .map(|lane| lane.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    decision,
                )
            })
        })
        .collect()
}

fn vector_build_projection_json(
    rows: &[(String, usize, usize, String, String, String, String)],
) -> String {
    let rows = rows
        .iter()
        .map(|(function, start, end, rule, layout, lane, decision)| {
            format!(
                "{{\"function\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"rule\":\"{}\",\"layout\":\"{}\",\"lane\":\"{}\",\"decision\":\"{}\"}}",
                json_escape(function),
                start,
                end,
                json_escape(rule),
                json_escape(layout),
                json_escape(lane),
                json_escape(decision),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{rows}]")
}

fn vector_build_projection_text(
    rows: &[(String, usize, usize, String, String, String, String)],
) -> String {
    rows.iter()
        .map(|(function, start, end, rule, layout, lane, decision)| {
            format!("vector\t{function}\t{start}..{end}\t{rule}\t{layout}\t{lane}\t{decision}\n")
        })
        .collect()
}

pub(crate) fn run_build_query(command: &str, args: &[&String], mode: OutputMode) {
    let explain_nodes = command == "explain-build" && args.len() <= 1;
    let (subject, file) = match command {
        "graph" => (None, args.first().map(|s| s.as_str())),
        "query" if args.first().map(|s| s.as_str()) == Some("build") => {
            (None, args.get(1).map(|s| s.as_str()))
        }
        "explain-build" if explain_nodes => (None, args.first().map(|s| s.as_str())),
        "explain-build" => (
            args.first().map(|s| s.as_str()),
            args.get(1).map(|s| s.as_str()),
        ),
        _ => (None, None),
    };
    let Some(file) = file else {
        write_mode_diagnostic(
            mode,
            &format!(
                "usage: jet {command} {}<file.jet>\n",
                if command == "explain-build" {
                    "<target|action|file> "
                } else {
                    ""
                }
            ),
        );
        exit(ExitCodes::USAGE);
    };
    let src = fs::read_to_string(file).unwrap_or_default();
    if explain_nodes {
        let nodes = match jet::Driver::query_build_nodes(file) {
            Ok(nodes) => nodes,
            Err(diags) => {
                report_problems(mode, file, &src, &diags);
                exit(ExitCodes::USER_ERROR);
            }
        };
        print_build_nodes(file, &nodes, mode);
        return;
    }
    let plan = match if command == "query" {
        jet::Driver::evaluate_build_query(file, jet::Driver::BuildQueryExpression::Build)
    } else {
        jet::Driver::query_build_plan(file)
    } {
        Ok(plan) => plan,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let Some(plan) = plan else {
        if mode.json {
            write_mode_machine(
                mode,
                &format!(
                    "{}\n",
                    render_status(
                        "inspect.build",
                        true,
                        StatusFields::new().with("build", StatusValue::Null),
                    )
                ),
            );
        } else {
            write_mode_renderable(mode, "default pipeline: no root fn build\n");
        }
        return;
    };
    if let Some(subject) = subject {
        if let Some(explanation) = plan.explain_target_named(subject) {
            print_build_explanation(&explanation, mode);
            return;
        }
        if let Some(explanation) = plan.explain_action_named(subject) {
            print_build_explanation(&explanation, mode);
            return;
        }
        print_build_explanation(&plan.explain_file(subject), mode);
        return;
    }
    let graph = plan.graph();
    let vector_rows = vector_build_projection(file);
    if mode.json {
        let payload = jet::Driver::build_plan_json(&plan, None);
        let payload = payload
            .strip_suffix('}')
            .map(|payload| {
                format!(
                    "{payload},\"vector\":{}}}",
                    vector_build_projection_json(&vector_rows)
                )
            })
            .unwrap_or(payload);
        write_mode_machine(
            mode,
            &format!(
                "{}\n",
                render_status(
                    "inspect.build",
                    true,
                    StatusFields::new().with(
                        "build",
                        StatusValue::parse(&payload)
                            .expect("build plan projection must be valid JSON"),
                    ),
                )
            ),
        );
    } else {
        for target in graph.targets {
            write_mode_renderable(
                mode,
                &format!("target\t{}\t{:?}\n", target.name, target.kind),
            );
        }
        for action in graph.actions {
            write_mode_renderable(
                mode,
                &format!("action\t{}\t{}\n", action.name, action.outputs.join(",")),
            );
        }
        write_mode_renderable(mode, &vector_build_projection_text(&vector_rows));
    }
}
fn print_build_nodes(
    file: &str,
    static_nodes: &[jet::Comptime::Build::BuildPlanNode],
    mode: OutputMode,
) {
    let program = build_record_program(file, None);
    let records = Store::from_env()
        .ok()
        .and_then(|store| store.latest_build_record(&program).ok().flatten())
        .map(|record| record.nodes)
        .unwrap_or_else(|| {
            static_nodes
                .iter()
                .map(|node| jet_store::BuildNodeRecord {
                    kind: node.kind.as_str().to_string(),
                    key: node.key.clone(),
                    subject: node.subject.clone(),
                    duration_ms: 0.0,
                    why_ran: "first-run".to_string(),
                    inputs: node.inputs.clone(),
                })
                .collect()
        });
    if mode.json {
        let nodes = records
            .iter()
            .map(|node| {
                format!(
                    "{{\"kind\":\"{}\",\"key\":\"{}\",\"subject\":\"{}\",\"duration_ms\":{:.3},\"why_ran\":\"{}\",\"inputs\":{}}}",
                    json_escape(&node.kind),
                    json_escape(&node.key),
                    json_escape(&node.subject),
                    node.duration_ms,
                    json_escape(&node.why_ran),
                    json_strings(&node.inputs),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        write_mode_machine(
            mode,
            &format!(
                "{{\"schema\":\"jet.explain-build/v1\",\"program\":\"{}\",\"nodes\":[{}]}}\n",
                json_escape(&program),
                nodes
            ),
        );
    } else {
        for node in records {
            write_mode_renderable(
                mode,
                &format!(
                    "{}\t{}\t{}\t{:.3}\t{}\t{}\n",
                    node.kind,
                    node.key,
                    node.subject,
                    node.duration_ms,
                    node.why_ran,
                    node.inputs.join(",")
                ),
            );
        }
    }
}

/// D-FRONTENDAPI1=A / #1049: expose the Rust and Jet compiler value API
/// through one schema-versioned, deterministic JSON command. The operation
/// names map directly to the read-only `core.compiler` surface; no second
/// parser or checker is introduced here.
pub(crate) fn run_compiler_api(operation: &str, file: &str, mode: OutputMode) {
    debug_assert!(
        mode.json,
        "inspect compiler must use the shared machine-output mode"
    );
    let path = Path::new(file);
    let document = match operation {
        "lex" => match fs::read_to_string(path) {
            Ok(source) => jet::Compiler::lex_source_json(&source),
            Err(error) => jet::Compiler::compiler_api_error_json(
                operation,
                file,
                "E0956",
                format!("could not read compiler input: {error}"),
            ),
        },
        "parse" => match fs::read_to_string(path) {
            Ok(source) => jet::Compiler::parse_source_json(&source),
            Err(error) => jet::Compiler::compiler_api_error_json(
                operation,
                file,
                "E0956",
                format!("could not read compiler input: {error}"),
            ),
        },
        "check" => jet::Compiler::check_file_json(path),
        "source-map" | "source_map" => match fs::read_to_string(path) {
            Ok(source) => jet::Compiler::source_map_json(&source),
            Err(error) => jet::Compiler::compiler_api_error_json(
                operation,
                file,
                "E0956",
                format!("could not read compiler input: {error}"),
            ),
        },
        _ => jet::Compiler::compiler_api_error_json(
            operation,
            file,
            "E0956",
            "unsupported compiler operation; choose lex, parse, check, or source-map",
        ),
    };
    write_mode_machine(mode, &format!("{document}\n"));
}
fn print_build_explanation(explanation: &jet::Comptime::Build::BuildExplanation, mode: OutputMode) {
    if mode.json {
        write_mode_machine(
            mode,
            &format!(
                "{}\n",
                render_status(
                    "inspect.build",
                    true,
                    StatusFields::new()
                        .with("label", explanation.label.clone())
                        .with(
                            "provenance",
                            StatusValue::array(
                                explanation
                                    .provenance
                                    .iter()
                                    .cloned()
                                    .map(StatusValue::from),
                            ),
                        ),
                )
            ),
        );
    } else {
        write_mode_renderable(mode, &format!("{}\n", explanation.label));
        for fact in &explanation.provenance {
            write_mode_renderable(mode, &format!("  {fact}\n"));
        }
    }
}

fn json_strings(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn authority_prompt_is_interactive(mode: OutputMode) -> bool {
    !mode.json
        && !env_truthy("CI")
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
}

fn append_authority_receipt(
    transaction: &crate::Store::AuthorityTransaction,
    bytes: &[u8],
    kind: &str,
) -> Result<(), String> {
    transaction
        .append_file(
            Path::new(".jet")
                .join("receipts")
                .join("authority.jsonl")
                .as_path(),
            bytes,
        )
        .map_err(|error| format!("could not write {kind}: {error}"))
}
enum PendingAuthorityProjectUpdate {
    Inline {
        snapshot: crate::Store::AuthorityFileSnapshot,
        replacement: Vec<u8>,
        manifest: jet::Package::PackageFacts,
    },
    Manifest {
        snapshot: crate::Store::AuthorityFileSnapshot,
        replacement: Vec<u8>,
        manifest: jet::Package::PackageFacts,
    },
}
fn fail_authority_transaction(
    mode: OutputMode,
    file: &str,
    src: &str,
    title: String,
    detail: String,
    error: String,
) -> ! {
    let diagnostic = jet::Diagnostics::Diagnostic::error("E2105", title, detail, error, None);
    report_problems(mode, file, src, &[diagnostic]);
    exit(ExitCodes::USER_ERROR);
}

fn authority_relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| {
            (root == Path::new(".") || root.as_os_str().is_empty()).then(|| path.to_path_buf())
        })
}

fn write_authority_receipt(
    transaction: &crate::Store::AuthorityTransaction,
    source: &str,
    operation: &str,
    scope: &str,
    projection: &jet::EffectBudget::EffectProjection,
    policy_source: &str,
) -> Result<(), String> {
    let required = projection
        .required_effects
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let granted = projection
        .granted_effects
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let denied = projection
        .denied_effects
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    writeln!(
        bytes,
        "{{\"schema\":\"jet.authority.receipt/v1\",\"kind\":\"approval\",\"scope\":{},\"resource\":\"application\",\"operation\":{},\"source\":{},\"authority\":{},\"policy_source\":{},\"required_effects\":{},\"granted_effects\":{},\"denied_effects\":{}}}",
        json_escape(scope),
        json_escape(operation),
        json_escape(source),
        json_escape(&projection.authority),
        json_escape(policy_source),
        json_strings(&required),
        json_strings(&granted),
        json_strings(&denied),
    )
    .map_err(|error| format!("could not format authority receipt: {error}"))?;
    append_authority_receipt(transaction, &bytes, "authority receipt")
}

fn write_authority_delegation_receipts(
    transaction: &crate::Store::AuthorityTransaction,
    source: &str,
    projection: &jet::EffectBudget::EffectProjection,
    delegations: &[jet::Sema::AuthorityDelegation],
    policy_source: &str,
) -> Result<(), String> {
    if delegations.is_empty() {
        return Ok(());
    }
    let required = projection
        .required_effects
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let granted = projection
        .granted_effects
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let denied = projection
        .denied_effects
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    for delegation in delegations {
        let scope = format!(
            "{}@{}..{}",
            delegation.binding, delegation.scope_span.start, delegation.scope_span.end
        );
        let policy = format!("#FX; {policy_source}");
        writeln!(
            bytes,
            "{{\"schema\":\"jet.authority.receipt/v1\",\"kind\":\"delegation\",\"scope\":{},\"resource\":{},\"operation\":{},\"source\":{},\"source_span\":{{\"start\":{},\"end\":{}}},\"authority\":{},\"policy_source\":{},\"required_effects\":{},\"granted_effects\":{},\"denied_effects\":{}}}",
            json_escape(&scope),
            json_escape(&delegation.resource),
            json_escape(&delegation.operation),
            json_escape(source),
            delegation.span.start,
            delegation.span.end,
            json_escape(&delegation.binding),
            json_escape(&policy),
            json_strings(&required),
            json_strings(&granted),
            json_strings(&denied),
        )
        .map_err(|error| format!("could not format authority delegation receipt: {error}"))?;
    }
    append_authority_receipt(transaction, &bytes, "authority delegation receipt")
}

fn resolve_run_authority_before_execution(
    file: &str,
    src: &str,
    mode: OutputMode,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    entry_fn: Option<&str>,
    package_manifest: &mut Option<(PathBuf, jet::Package::PackageFacts)>,
    source_closure: &[(PathBuf, String)],
    source_snapshot: Option<&crate::Store::AuthorityFileSnapshot>,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<jet_foundation::Authority::ApplicationAuthority> {
    if source_closure.is_empty() {
        fail_authority_transaction(
            mode,
            file,
            src,
            "could not establish the checked source authority".to_string(),
            "authority checks must use the immutable source checked for this compile".to_string(),
            "the checked source closure is empty".to_string(),
        );
    }
    let (diagnostics, bundle, facts) = jet::run_compiler_work(|| {
        jet::Driver::check_file_with_effect_facts_for_run_and_entry_with_source_closure(
            file,
            &source_closure,
            profile,
            setting_overrides,
            entry_fn,
        )
    });
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        report_problems(mode, file, src, &diagnostics);
        exit(ExitCodes::USER_ERROR);
    }
    let Some(bundle) = bundle else {
        report_problems(mode, file, src, &diagnostics);
        exit(ExitCodes::USER_ERROR);
    };
    let (mut projection, delegations) = native_effect_projection(
        &bundle,
        &facts.summaries,
        entry_fn,
        package_manifest.as_ref().map(|(_, manifest)| manifest),
    );
    let entries =
        jet::EffectBudget::compute_package_effects(&bundle, &facts.solved, &facts.summaries);
    let lints = crate::CmdDevTools::visible_lints(&diagnostics);
    apply_native_effect_policy(
        "run",
        file,
        src,
        mode,
        false,
        &lints,
        &entries,
        &facts.fact_registry,
        &mut projection,
        package_manifest,
        &delegations,
        source_snapshot,
        invocation_authority,
    )
}

fn merge_invocation_authority(
    projection: &mut jet::EffectBudget::EffectProjection,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) {
    let Some(invocation_authority) = invocation_authority else {
        return;
    };
    projection
        .granted_effects
        .extend(invocation_authority.granted_effects.iter().cloned());
    projection
        .denied_effects
        .extend(invocation_authority.denied_effects.iter().cloned());
    if !projection.authority.is_empty() && !invocation_authority.authority.is_empty() {
        projection.authority.push_str(" + ");
    }
    projection
        .authority
        .push_str(&invocation_authority.authority);
}

fn resolve_application_authority(
    cmd: &str,
    file: &str,
    src: &str,
    mode: OutputMode,
    projection: &mut jet::EffectBudget::EffectProjection,
    package_manifest: &mut Option<(PathBuf, jet::Package::PackageFacts)>,
    delegations: &[jet::Sema::AuthorityDelegation],
    source_snapshot: Option<&crate::Store::AuthorityFileSnapshot>,
    transaction: &crate::Store::AuthorityTransaction,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<jet_foundation::Authority::ApplicationAuthority> {
    if !matches!(cmd, "build" | "run") {
        return None;
    }
    merge_invocation_authority(projection, invocation_authority);
    let denied: jet::Sema::EffectSet = projection
        .required_effects
        .iter()
        .filter(|effect| {
            jet_foundation::Authority::answer(
                &projection.granted_effects,
                &projection.denied_effects,
                effect,
            ) == jet_foundation::Authority::Verdict::Denied
        })
        .cloned()
        .collect();
    if !denied.is_empty() {
        let diagnostic = jet::EffectBudget::application_policy_diagnostic(projection, &denied);
        report_problems(mode, file, src, &[diagnostic]);
        exit(ExitCodes::USER_ERROR);
    }
    let root = package_manifest
        .as_ref()
        .map(|(root, _)| root.clone())
        .unwrap_or_else(|| {
            Path::new(file)
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        });
    let undecided = projection.undecided();
    if undecided.is_empty() {
        if let Err(error) = write_authority_delegation_receipts(
            transaction,
            file,
            projection,
            delegations,
            "declared policy",
        ) {
            fail_authority_transaction(
                mode,
                file,
                src,
                "could not record the Authority delegation receipt".to_string(),
                "every Authority delegation is source-linked before the effect runs".to_string(),
                error,
            );
        }
        return invocation_authority.map(|_| projection.application_authority());
    }
    if !authority_prompt_is_interactive(mode) {
        let diagnostic =
            jet::EffectBudget::application_policy_diagnostic(projection, &BTreeSet::new());
        report_problems(mode, file, src, &[diagnostic]);
        exit(ExitCodes::USER_ERROR);
    }

    write_mode_diagnostic(
        mode,
        &format!(
            "authority required for {} — choose once, project, or deny [{}]\n  {}\n",
            json_strings(&undecided.iter().cloned().collect::<Vec<_>>()),
            projection.authority,
            jet::EffectBudget::render_effect_projection_line(projection),
        ),
    );
    write_mode_diagnostic(mode, "authority> ");
    let _ = std::io::stderr().flush();
    let mut choice = String::new();
    let _ = std::io::stdin().read_line(&mut choice);
    let choice = choice.trim().to_ascii_lowercase();
    let mut pending = None;
    let (scope, policy_source) = match choice.as_str() {
        "once" | "1" => ("invocation", "interactive.once"),
        "project" | "2" => {
            if package_manifest.is_none() {
                fail_authority_transaction(
                    mode,
                    file,
                    src,
                    "can't persist project authority".to_string(),
                    "project approval requires a loaded canonical package manifest".to_string(),
                    "no package manifest is available".to_string(),
                );
            }
            let inline = match jet::Package::PackageFacts::parse_inline(src, file.to_string()) {
                Ok(inline) => inline,
                Err(error) => {
                    let diagnostic = jet::Diagnostics::Diagnostic::error(
                        "E1362",
                        "the inline Package body is malformed".to_string(),
                        error.to_string(),
                        "fix the inline Package fields before approving authority".to_string(),
                        None,
                    );
                    report_problems(mode, file, src, &[diagnostic]);
                    exit(ExitCodes::USER_ERROR);
                }
            };
            if let Some((_, block)) = inline {
                let Some(source_snapshot) = source_snapshot else {
                    fail_authority_transaction(
                        mode,
                        file,
                        src,
                        "can't persist project authority for this source".to_string(),
                        "project approval requires a checked source file, not a virtual overlay"
                            .to_string(),
                        "the source has no descriptor-relative identity".to_string(),
                    );
                };
                let Some(relative) = authority_relative_path(&root, Path::new(file)) else {
                    fail_authority_transaction(
                        mode,
                        file,
                        src,
                        "can't persist project authority for this source".to_string(),
                        "project approval requires the checked source beneath the project root"
                            .to_string(),
                        format!(
                            "source `{}` is outside project root `{}`",
                            source_snapshot.path().display(),
                            root.display()
                        ),
                    );
                };
                let snapshot = transaction
                    .snapshot_file(&relative)
                    .unwrap_or_else(|error| {
                        fail_authority_transaction(
                        mode,
                        file,
                        src,
                        "can't snapshot project authority".to_string(),
                        "project approval must edit the same immutable source checked for this run"
                            .to_string(),
                        format!("could not snapshot `{}`: {error}", relative.display()),
                    )
                    });
                let mut updated_body = block.body(src).to_string();
                for effect in &undecided {
                    updated_body = jet::Manifest::add_authority_hold(&updated_body, effect);
                }
                let mut updated_source = src.to_string();
                updated_source
                    .replace_range(block.body_span.start..block.body_span.end, &updated_body);
                let reparsed = match jet::Package::PackageFacts::parse(
                    &updated_body,
                    file.to_string(),
                ) {
                    Ok(manifest) => manifest,
                    Err(error) => {
                        let diagnostic = jet::Diagnostics::Diagnostic::error(
                            "E1221",
                            format!("project approval produced an invalid `{file}`"),
                            error.to_string(),
                            "edit the inline Package authority.holds with the canonical manifest editor"
                                .to_string(),
                            None,
                        );
                        report_problems(mode, file, src, &[diagnostic]);
                        exit(ExitCodes::USER_ERROR);
                    }
                };
                pending = Some(PendingAuthorityProjectUpdate::Inline {
                    snapshot,
                    replacement: updated_source.into_bytes(),
                    manifest: reparsed,
                });
                ("project", "inline Package authority.holds")
            } else {
                let Some(manifest_path) = jet::Loader::manifest_path(&root) else {
                    let diagnostic = jet::EffectBudget::application_policy_diagnostic(
                        projection,
                        &BTreeSet::new(),
                    );
                    report_problems(mode, file, src, &[diagnostic]);
                    exit(ExitCodes::USER_ERROR);
                };
                let Some(relative) = authority_relative_path(&root, &manifest_path) else {
                    fail_authority_transaction(
                        mode,
                        file,
                        src,
                        "can't snapshot project authority".to_string(),
                        "project approval must edit the canonical manifest beneath the project root"
                            .to_string(),
                        format!(
                            "manifest `{}` is outside project root `{}`",
                            manifest_path.display(),
                            root.display()
                        ),
                    );
                };
                let manifest_snapshot = transaction.snapshot_file(&relative).unwrap_or_else(|error| {
                    fail_authority_transaction(
                        mode,
                        file,
                        src,
                        format!("can't read `{}` for project approval", manifest_path.display()),
                        "project approval must edit the same immutable manifest checked for this run"
                            .to_string(),
                        format!("could not snapshot `{}`: {error}", relative.display()),
                    )
                });
                let raw = match manifest_snapshot.text() {
                    Ok(raw) => raw.to_owned(),
                    Err(error) => fail_authority_transaction(
                        mode,
                        file,
                        src,
                        format!(
                            "can't read `{}` for project approval",
                            manifest_path.display()
                        ),
                        "project approval requires a UTF-8 canonical package manifest".to_string(),
                        error.to_string(),
                    ),
                };
                let mut updated = raw;
                for effect in &undecided {
                    updated = jet::Manifest::add_authority_hold(&updated, effect);
                }
                let reparsed = match jet::Package::PackageFacts::parse(
                    &updated,
                    manifest_path.display().to_string(),
                ) {
                    Ok(manifest) => manifest,
                    Err(error) => {
                        let diagnostic = jet::Diagnostics::Diagnostic::error(
                            "E1221",
                            format!(
                                "project approval produced an invalid `{}`",
                                manifest_path.display()
                            ),
                            error.to_string(),
                            "edit `authority.holds.allow` with the canonical manifest editor"
                                .to_string(),
                            None,
                        );
                        report_problems(mode, file, src, &[diagnostic]);
                        exit(ExitCodes::USER_ERROR);
                    }
                };
                pending = Some(PendingAuthorityProjectUpdate::Manifest {
                    snapshot: manifest_snapshot,
                    replacement: updated.into_bytes(),
                    manifest: reparsed,
                });
                ("project", "package.jet authority.holds")
            }
        }
        _ => {
            let denied_now = undecided.clone();
            projection.denied_effects.extend(denied_now.iter().cloned());
            let diagnostic =
                jet::EffectBudget::application_policy_diagnostic(projection, &denied_now);
            report_problems(mode, file, src, &[diagnostic]);
            exit(ExitCodes::USER_ERROR);
        }
    };
    if let Err(error) =
        write_authority_receipt(transaction, file, cmd, scope, projection, policy_source)
    {
        fail_authority_transaction(
            mode,
            file,
            src,
            "could not record the application authority receipt".to_string(),
            "every authority approval is source-linked before the effect runs".to_string(),
            error,
        );
    }
    if let Err(error) = write_authority_delegation_receipts(
        transaction,
        file,
        projection,
        delegations,
        policy_source,
    ) {
        fail_authority_transaction(
            mode,
            file,
            src,
            "could not record the Authority delegation receipt".to_string(),
            "every Authority delegation is source-linked before the effect runs".to_string(),
            error,
        );
    }
    if let Some(pending) = pending {
        let (result, reparsed) = match pending {
            PendingAuthorityProjectUpdate::Inline {
                snapshot,
                replacement,
                manifest,
            }
            | PendingAuthorityProjectUpdate::Manifest {
                snapshot,
                replacement,
                manifest,
            } => (transaction.replace_file(&snapshot, &replacement), manifest),
        };
        if let Err(error) = result {
            let detail = if error.is_published() {
                "the authority rename committed, but post-commit verification or durability failed; the canonical state is committed or uncertain and execution is forbidden"
            } else {
                "the approval receipt is durable, but the canonical authority file was not published; execution is forbidden"
            };
            fail_authority_transaction(
                mode,
                file,
                src,
                "could not persist project authority".to_string(),
                detail.to_string(),
                error.to_string(),
            );
        }
        let Some((_, manifest)) = package_manifest.as_mut() else {
            fail_authority_transaction(
                mode,
                file,
                src,
                "could not persist project authority".to_string(),
                "project approval requires a loaded canonical package manifest".to_string(),
                "no package manifest is available".to_string(),
            );
        };
        *manifest = reparsed;
    }
    (scope == "invocation").then(|| projection.application_authority())
}

/// Apply the complete application effect policy before any native adapter can
/// execute. This is shared by AOT, JIT, and interpreter paths; the only tier
/// input is the checked semantic effect view.
fn apply_native_effect_policy(
    cmd: &str,
    file: &str,
    src: &str,
    mode: OutputMode,
    is_plugin: bool,
    lints: &[jet::Diagnostics::Diagnostic],
    entries: &[jet::EffectBudget::PackageEffects],
    fact_registry: &jet_foundation::Facts::FactRegistry,
    projection: &mut jet::EffectBudget::EffectProjection,
    package_manifest: &mut Option<(PathBuf, jet::Package::PackageFacts)>,
    delegations: &[jet::Sema::AuthorityDelegation],
    source_snapshot: Option<&crate::Store::AuthorityFileSnapshot>,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<jet_foundation::Authority::ApplicationAuthority> {
    let authority_root = package_manifest
        .as_ref()
        .map(|(root, _)| root.clone())
        .unwrap_or_else(|| {
            Path::new(file)
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        });
    let transaction = match crate::Store::begin_authority_transaction(&authority_root) {
        Ok(transaction) => transaction,
        Err(error) => fail_authority_transaction(
            mode,
            file,
            src,
            "could not establish the authority transaction".to_string(),
            "authority receipts and grants must use one locked project root".to_string(),
            format!(
                "could not lock authority root `{}`: {error}",
                authority_root.display()
            ),
        ),
    };
    // D-PLUGIN-AUTHORITY1: a plugin may use only effects explicitly declared
    // by its package authority.needs. Mem remains the implicit guest-safe
    // allocator root; every other root effect must be covered by a declared
    // canonical right before any backend writes or instantiates a component.
    if is_plugin {
        if let Some(root) = entries.iter().find(|package| package.name == "root") {
            let declared_needs = package_manifest
                .as_ref()
                .map(|(_, manifest)| manifest.authority.needs.as_slice())
                .unwrap_or(&[]);
            let mut forbidden = root.effects.clone();
            forbidden.retain(|effect| {
                jet::Sema::effect_root(effect) != "Mem"
                    && !declared_needs
                        .iter()
                        .any(|need| jet_foundation::Authority::covers(need, effect))
            });
            if !forbidden.is_empty() {
                let required = forbidden
                    .iter()
                    .map(|effect| format!("{effect} (declare authority.needs `{effect}`)"))
                    .collect::<BTreeSet<_>>();
                let diagnostic = jet::Manifest::e1258(&jet::Sema::show_set(&required));
                report_problems(mode, file, src, &[diagnostic]);
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    let authority = resolve_application_authority(
        cmd,
        file,
        src,
        mode,
        projection,
        package_manifest,
        delegations,
        source_snapshot,
        &transaction,
        invocation_authority,
    );
    if let Some((root, manifest)) = package_manifest.as_ref() {
        let lint_violations = jet::LintPolicy::enforce(lints, manifest);
        if !lint_violations.is_empty() {
            report_problems(mode, file, src, &lint_violations);
            exit(ExitCodes::USER_ERROR);
        }
        let configured_names = manifest
            .authority
            .holds
            .allow
            .iter()
            .flatten()
            .chain(manifest.authority.holds.deny.iter().flatten())
            .chain(
                manifest
                    .authority
                    .grants
                    .iter()
                    .flat_map(|(_, names)| names),
            );
        let mut violations = Vec::new();
        for name in configured_names {
            if jet::Sema::parse_effect_name(name).is_some() {
                if let Err(suggestion) = jet::Sema::resolve_effect_name(name, fact_registry) {
                    violations.push(jet::Sema::undeclared_effect(
                        name,
                        suggestion.as_deref(),
                        None,
                    ));
                }
            }
        }
        violations.extend(jet::EffectBudget::enforce(entries, manifest));
        if !violations.is_empty() {
            report_problems(mode, file, src, &violations);
            exit(ExitCodes::USER_ERROR);
        }
        // `jet fetch` owns creating the lockfile. Native execution only adds
        // the effect provenance and grants when a lock already exists.
        let lock_path = jet::PkgStore::lock_path(root);
        let lock_snapshot = {
            let Some(relative) = authority_relative_path(root, &lock_path) else {
                fail_authority_transaction(
                    mode,
                    file,
                    src,
                    format!("can't read project lock `{}`", lock_path.display()),
                    "native execution must preserve the canonical lock authority".to_string(),
                    format!(
                        "lock `{}` is outside authority root `{}`",
                        lock_path.display(),
                        root.display()
                    ),
                );
            };
            match transaction.snapshot_file(&relative) {
                Ok(snapshot) => Some(snapshot),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => fail_authority_transaction(
                    mode,
                    file,
                    src,
                    format!("can't read project lock `{}`", lock_path.display()),
                    "native execution must preserve the canonical lock authority".to_string(),
                    format!("fix the lock permissions and retry: {error}"),
                ),
            }
        };
        if let Some(lock_snapshot) = lock_snapshot {
            let raw = match lock_snapshot.text() {
                Ok(raw) => raw.to_owned(),
                Err(error) => fail_authority_transaction(
                    mode,
                    file,
                    src,
                    format!("can't read project lock `{}`", lock_path.display()),
                    "native execution requires a UTF-8 canonical lockfile".to_string(),
                    error.to_string(),
                ),
            };
            let mut lock = match jet::Lock::parse(&raw) {
                Ok(lock) => lock,
                Err(error) => fail_authority_transaction(
                    mode,
                    file,
                    src,
                    format!("can't parse project lock `{}`", lock_path.display()),
                    "native execution cannot update a corrupt lockfile".to_string(),
                    error,
                ),
            };
            jet::EffectBudget::update_lock_provenance(&mut lock, entries, manifest);
            let replacement = jet::Lock::write(&lock);
            if let Err(error) = transaction.replace_file(&lock_snapshot, replacement.as_bytes()) {
                let detail = if error.is_published() {
                    "the lockfile rename committed, but post-commit verification or durability failed; canonical lock authority is committed or uncertain and execution is forbidden"
                } else {
                    "the canonical lockfile was not published; execution is forbidden"
                };
                fail_authority_transaction(
                    mode,
                    file,
                    src,
                    format!("can't write project lock `{}`", lock_path.display()),
                    detail.to_string(),
                    error.to_string(),
                );
            }
        }
    }
    if let Err(error) = transaction.finish() {
        fail_authority_transaction(
            mode,
            file,
            src,
            "could not finish the authority transaction".to_string(),
            "authority receipts or grants may be committed, but durability state is uncertain; execution is forbidden"
                .to_string(),
            error.to_string(),
        );
    }
    authority
}

/// D-BUILDPROFILE1: load Package build profiles from the project root of
/// `source_file`, using the same canonical Package context for a leading
/// inline carrier and `package.jet`.
fn load_pkg_profiles(source_file: &str) -> Option<Vec<jet::Package::BuildProfileDef>> {
    load_pkg_manifest(source_file).map(|(_, facts)| facts.build_profiles)
}

/// Resolve the profile name through the shared contribution law. Profile
/// details remain owned by the package profile table; this function decides
/// only which writer supplies the selected name.
fn resolve_profile_name(named_profile: Option<&str>) -> Option<String> {
    let mut contributions = Vec::new();
    if let Some(name) = named_profile {
        contributions.push(jet::Policy::FactContribution::new(
            "Build.Profile",
            jet::Policy::FactValue::Text(name.to_string()),
            jet::Policy::SourceScope::Package,
            jet::Policy::ContributionLayer::CommandLine,
            "command line",
        ));
    }
    let fact = match jet::Policy::resolve(
        jet::Policy::FactKey::with_default(
            "Build.Profile",
            jet::Policy::FactValue::Text("dev".to_string()),
        ),
        contributions,
    ) {
        Ok(fact) => fact,
        Err(error) => {
            crate::cli_error!(
                @full "E3521",
                error.message(),
                "same-layer profile writers cannot disagree",
                "make the profile writers agree, or select one explicit profile"
            );
            exit(ExitCodes::USER_ERROR);
        }
    }?;
    match fact.value {
        jet::Policy::FactValue::Text(name) if name != "dev" => Some(name),
        jet::Policy::FactValue::Text(_) => None,
        _ => {
            crate::cli_error!(
                @full "E3521",
                "the selected build profile is not a text fact",
                "Build.Profile names optimization bundles",
                "select a named text profile"
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// D-BUILDPROFILE1: resolve `--profile=<name>` (or `--release` → `"release"`)
/// to a `BuildProfile`. The built-in hardened profile is reserved so a
/// package profile cannot silently remove its sentry guarantee. Manifest
/// entries override the other blessed defaults; unknown names emit E1219.
pub(crate) fn resolve_named_profile(
    name: &str,
    source_file: &str,
    mode: OutputMode,
) -> BuildProfile {
    if name == jet::Syntax::BUILD_PROFILE_HARDENED {
        return BuildProfile::Hardened;
    }
    if let Some(profiles) = load_pkg_profiles(source_file) {
        if let Some(def) = profiles.iter().find(|p| p.name == name) {
            return BuildProfile::Named {
                name: name.to_string(),
                config: ProfileConfig::from_def(def),
            };
        }
    }
    match name {
        n if n == jet::Syntax::BUILD_PROFILE_RELEASE => BuildProfile::Release,
        n if n == jet::Syntax::BUILD_PROFILE_HARDENED => BuildProfile::Hardened,
        n if n == jet::Syntax::BUILD_PROFILE_DEBUG => BuildProfile::Debug,
        n if n == jet::Syntax::BUILD_PROFILE_CI => BuildProfile::Ci,
        _ => {
            let diag = jet::Diagnostics::Diagnostic::error(
                "E1219",
                format!("`--profile={name}` is not a defined build profile."),
                "Blessed profiles have built-in defaults; other names must be declared in `package.jet`.".to_string(),
                "Use `--release`, `--profile=debug`, or `--profile=ci`, or declare the profile in `package.jet`.".to_string(),
                None,
            );
            let rendered = if mode.json {
                render_diagnostic_status(
                    "compile.profile",
                    false,
                    &jet::Diagnostics::ReportPath::from_process("<cli>"),
                    "",
                    std::slice::from_ref(&diag),
                )
            } else {
                jet::render_all_colored("<cli>", "", &[diag], mode.color_stderr())
            };
            write_mode_diagnostic(mode, &rendered);
            std::process::exit(jet::ExitCodes::USER_ERROR);
        }
    }
}
/// D-DX-PROD1: one release compiler gate carries both the release marker and
/// the closed devtools-presence form. Non-release tiers receive no release
/// inspection cfg, so ambient deployment state cannot add release code.
fn release_profile_cfg_args(profile: &BuildProfile) -> Vec<String> {
    let Some(inspect) = profile.release_inspect() else {
        return Vec::new();
    };
    let policy = jet::Package::ReleaseDevtoolsPolicy::from_manifest_profile(inspect);
    let Some(cfg_value) = policy.release_cfg_value() else {
        return Vec::new();
    };
    vec![
        "--cfg".to_string(),
        "jet_release".to_string(),
        "--cfg".to_string(),
        format!("jet_release_inspect=\"{cfg_value}\""),
    ]
}
/// Fold the selected release profile and deployment environment once at the
/// host boundary.  A non-release profile retains the ordinary development
/// host; release deployment variables can never add release code.
pub(crate) fn release_devtools_policy_for_profile(
    profile: &BuildProfile,
) -> jet::Package::ReleaseDevtoolsPolicy {
    let Some(inspect) = profile.release_inspect() else {
        return jet::Package::ReleaseDevtoolsPolicy::development();
    };
    jet::Package::ReleaseDevtoolsPolicy::from_manifest_profile_with_env(inspect)
        .unwrap_or_else(|error| {
            crate::cli_error!(
                @fix "E2104",
                format!("invalid release devtools deployment policy: {error}"),
                "set JET_INSPECT=1 only with a valid JET_INSPECT_TOKEN and optional IP/CIDR allowlist"
            );
            exit(ExitCodes::USER_ERROR);
        })
}

/// Resolve the profile spelling retained by the watch loop and fold its
/// manifest/profile/environment facts into one typed host policy.
pub(crate) fn release_devtools_policy_for_name(
    source_file: &str,
    profile_name: &str,
    mode: OutputMode,
) -> jet::Package::ReleaseDevtoolsPolicy {
    let profile = if profile_name == "dev" {
        BuildProfile::Fast
    } else {
        resolve_named_profile(profile_name, source_file, mode)
    };
    release_devtools_policy_for_profile(&profile)
}

/// D-LINTPOLICY1: load the one canonical Package context used by compile.
/// An inline carrier wins when present; the loader separately rejects an
/// inline/package.jet conflict before this helper can be used.
fn load_pkg_manifest(source_file: &str) -> Option<(PathBuf, jet::Package::PackageFacts)> {
    let source_path = Path::new(source_file);
    let search_from = source_path.parent().unwrap_or(Path::new("."));
    let facts = jet::Loader::package_facts_for_entry(source_path)
        .ok()
        .flatten()?;
    let root = jet::Loader::find_package_root_checked(search_from)
        .ok()
        .flatten()
        .unwrap_or_else(|| search_from.to_path_buf());
    Some((root, facts))
}

fn load_pkg_library_output_names(source_file: &str) -> Option<Vec<String>> {
    load_pkg_manifest(source_file).map(|(_, facts)| {
        facts
            .outputs
            .iter()
            .filter(|(_, output)| output.kind == jet::Package::PackageOutputKind::Library)
            .map(|(name, _)| name.clone())
            .collect()
    })
}

/// Find the project's declared environment without realizing or mutating it.
/// `jet` may inspect this boundary, but acquisition and activation belong to
/// `jetpack` (D-VERDICT-2188-1).
fn declared_project_environment(
    start: &Path,
    requested_environment: Option<&str>,
) -> Option<(PathBuf, String)> {
    let search_from = if start.is_dir() {
        start
    } else {
        start.parent().unwrap_or_else(|| Path::new("."))
    };
    let root = jetpack::EnvHook::find_env_root(search_from)?;
    let source = fs::read_to_string(root.join(jet::Syntax::ENV_FILE)).ok()?;

    if jet_env_model::ModuleEval::is_module_surface(&source) {
        let plan = match jet_env_model::ModuleEval::evaluate_env_with_selections(
            &source,
            &root,
            None,
            requested_environment,
        ) {
            Ok(plan) => plan,
            // A malformed typed environment still declares the boundary. Let
            // jetpack report the source diagnostic after the user enters it.
            Err(_) => return Some((root, "env".to_string())),
        };
        let name = plan
            .active_environment
            .clone()
            .or_else(|| plan.environment_names.first().cloned())?;
        return Some((root, format!("env.{name}")));
    }

    let env = jetpack::EnvFile::parse(&source);
    (env.default_source.is_some() || !env.named.is_empty() || !env.packages.is_empty())
        .then(|| (root, "env".to_string()))
}

fn same_environment_root(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Build the one application-boundary effect view used by every native tier.
/// Engines receive the resulting authority only; they do not project policy
/// or decide what an effect means.
fn native_effect_projection(
    bundle: &jet::AST::ProgramBundle,
    summaries: &HashMap<String, jet::Sema::EffectSummary>,
    entry_fn: Option<&str>,
    package_manifest: Option<&jet::Package::PackageFacts>,
) -> (
    jet::EffectBudget::EffectProjection,
    Vec<jet::Sema::AuthorityDelegation>,
) {
    let projection = jet::EffectBudget::project_program_effects(
        bundle,
        summaries,
        entry_fn.unwrap_or("run"),
        package_manifest,
    );
    let mut delegations = summaries
        .values()
        .flat_map(|summary| summary.authority_delegations.iter().cloned())
        .collect::<Vec<_>>();
    // #2515 HashMap census: summaries is unordered, so authority rows that
    // reach receipts or diagnostics must be ordered before they leave sema.
    delegations.sort_by(|left, right| {
        left.binding
            .cmp(&right.binding)
            .then_with(|| left.resource.cmp(&right.resource))
            .then_with(|| left.operation.cmp(&right.operation))
            .then_with(|| left.scope_span.start.cmp(&right.scope_span.start))
            .then_with(|| left.scope_span.end.cmp(&right.scope_span.end))
            .then_with(|| left.span.start.cmp(&right.span.start))
            .then_with(|| left.span.end.cmp(&right.span.end))
    });
    (projection, delegations)
}

/// Return the first import in the program graph that names a package declared
/// by the nearest `package.jet`. Local modules stay in Jet's source graph and
/// Core imports stay in the embedded toolchain; neither one needs an active
/// project environment.
fn package_import_name(
    import: &jet::AST::ImportDecl,
    dependencies: &BTreeSet<String>,
    allow_inline: bool,
) -> Option<String> {
    match &import.kind {
        jet::AST::ImportKind::Module(name, _) => {
            if allow_inline {
                if let Some(version) = &import.inline_version {
                    return Some(format!("{name}#{}", version.text));
                }
            }
            if import.core_module_path().is_some() {
                return None;
            }
            dependencies
                .contains(name.split('.').next().unwrap_or(name))
                .then(|| name.clone())
        }
        jet::AST::ImportKind::Unqualified {
            module_alias,
            items,
            ..
        } => {
            if jet::AST::core_list_prefix(module_alias).is_some() {
                return None;
            }
            if !dependencies.contains(module_alias.split('.').next().unwrap_or(module_alias)) {
                return None;
            }
            let item = items
                .first()
                .map(|(name, _)| format!(".{name}"))
                .unwrap_or_default();
            Some(format!("{module_alias}{item}"))
        }
        jet::AST::ImportKind::File(_, _) => None,
    }
}

/// Find one source file behind a local module or file import so the gate sees
/// a package dependency reached through a local module too. Ambiguous or
/// malformed imports are left to the normal loader diagnostics.
fn local_import_target(
    importing: &Path,
    import: &jet::AST::ImportDecl,
    project_root: &Path,
    source_files: &[PathBuf],
) -> Option<PathBuf> {
    match &import.kind {
        jet::AST::ImportKind::File(path, _) => {
            if path.contains("..") {
                return None;
            }
            let mut target = importing.parent().unwrap_or(Path::new(".")).to_path_buf();
            for part in path.split('/') {
                if !part.is_empty() && part != "." {
                    target.push(part);
                }
            }
            target.set_extension(jet::Syntax::FILE_EXT);
            let target = fs::canonicalize(target).ok()?;
            let root =
                fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
            target.starts_with(root).then_some(target)
        }
        jet::AST::ImportKind::Module(name, _) => {
            if import.core_module_path().is_some() {
                return None;
            }
            let direct_name = format!("{name}.{}", jet::Syntax::FILE_EXT);
            let mut matches = source_files
                .iter()
                .filter(|file| {
                    let Some(file_name) = file.file_name().and_then(|name| name.to_str()) else {
                        return false;
                    };
                    file_name == direct_name
                        || (file_name == jet::Syntax::DEFAULT_ENTRY_FILE
                            && file
                                .parent()
                                .and_then(|parent| parent.file_name())
                                .and_then(|name| name.to_str())
                                == Some(name))
                })
                .cloned()
                .collect::<Vec<_>>();
            (matches.len() == 1).then(|| matches.remove(0))
        }
        jet::AST::ImportKind::Unqualified { .. } => None,
    }
}

/// Inspect only source and package facts. This never evaluates `env.jet`,
/// resolves a package, or creates project state.
fn project_package_import(start: &Path, environment_root: &Path) -> Option<String> {
    let entry = if start.is_dir() {
        let project_root = jet::Loader::find_manifest_root(start)
            .unwrap_or_else(|| environment_root.to_path_buf());
        crate::find_project_entry(&project_root)
    } else {
        start.to_path_buf()
    };

    let manifest = load_pkg_manifest(&entry.to_string_lossy());
    let (project_root, dependencies, allow_inline) = match manifest {
        Some((root, facts)) => (
            root,
            facts.deps.keys().cloned().collect::<BTreeSet<_>>(),
            false,
        ),
        None => (environment_root.to_path_buf(), BTreeSet::new(), true),
    };
    let source_files = jet::ProjectParts::source_files(&project_root);
    let mut pending = vec![entry];
    if start.is_dir() {
        pending.extend(source_files.iter().cloned());
        let test_override = project_root.join(jet::Syntax::COMMAND_FILE_TEST);
        if test_override.is_file() {
            pending.push(test_override);
        }
    }
    let mut visited = BTreeSet::new();

    while let Some(file) = pending.pop() {
        let identity = fs::canonicalize(&file).unwrap_or_else(|_| file.clone());
        if !visited.insert(identity) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let source_for_parse = jet::Package::mask_inline_package_source(&source)
            .map(|(masked, _)| masked)
            .unwrap_or(source);
        let (tokens, lex_diagnostics) = jet::Lexer::lex(&source_for_parse);
        if !lex_diagnostics.is_empty() {
            continue;
        }
        let Ok(program) = jet::Parser::parse_with_source(&tokens, &source_for_parse) else {
            continue;
        };
        for import in &program.imports {
            if let Some(name) = package_import_name(import, &dependencies, allow_inline) {
                return Some(name);
            }
        }
        for import in &program.imports {
            if let Some(target) = local_import_target(&file, import, &project_root, &source_files) {
                pending.push(target);
            }
        }
    }
    None
}

/// Return the inactive environment requirement and its authoritative project
/// root for a program that actually consumes a declared package. Core-only and
/// local-module programs return `None`, even when an `env.jet` is nearby.
fn project_environment_requirement_with_root(start: &Path) -> Option<(PathBuf, String, String)> {
    let raw = current_jet_args();
    let (requested_preset, requested_environment) = environment_selections(&raw);
    let (root, environment) =
        declared_project_environment(start, requested_environment.as_deref())?;
    let import = project_package_import(start, &root)?;
    let active_root = std::env::var_os(jet::Syntax::JETPACK_ENV_MARKER)
        .is_some_and(|value| !value.is_empty() && value != "0")
        && std::env::var_os(jet::Syntax::ENV_HOOK_ACTIVE_DIR_VAR)
            .is_some_and(|active| same_environment_root(&root, Path::new(&active)));
    let active = active_root
        && std::env::var(jet::Syntax::ENV_HOOK_ACTIVE_HASH_VAR)
            .ok()
            .filter(|value| !value.is_empty())
            .is_some_and(|active_hash| {
                jetpack::EnvHook::definition_fingerprint_with_selections(
                    &root,
                    requested_preset.as_deref(),
                    requested_environment.as_deref(),
                )
                .is_some_and(|expected_hash| expected_hash == active_hash)
            });
    (!active).then_some((root, environment, import))
}

/// Return the inactive environment requirement for callers that only need its
/// display values. Preparation uses the root-carrying helper above so its
/// delegation cannot silently fall back to the caller's working directory.
pub(crate) fn project_environment_requirement(start: &Path) -> Option<(String, String)> {
    project_environment_requirement_with_root(start)
        .map(|(_, environment, import)| (environment, import))
}
fn current_jet_args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

fn environment_selections(raw: &[String]) -> (Option<String>, Option<String>) {
    let mut requested_preset = None;
    let mut requested_environment = None;
    let mut index = 0;
    while index < raw.len() {
        let argument = &raw[index];
        if argument == "--" {
            break;
        }
        if let Some(value) = argument.strip_prefix("--preset=") {
            requested_preset = Some(value.to_string());
        } else if argument == "--preset" {
            if let Some(value) = raw.get(index + 1) {
                requested_preset = Some(value.clone());
                index += 1;
            }
        } else if let Some(value) = argument.strip_prefix("--env=") {
            requested_environment = Some(value.to_string());
        } else if argument == "--env" {
            if let Some(value) = raw.get(index + 1) {
                requested_environment = Some(value.clone());
                index += 1;
            }
        }
        index += 1;
    }
    (requested_preset, requested_environment)
}

fn current_jet_invocation() -> (Vec<String>, String) {
    let raw = current_jet_args();
    let command = format!("{} {}", jet::Syntax::BINARY_NAME, raw.join(" "));
    (raw, command)
}

/// Build the one canonical Jetpack delegation used by `jet run`.
///
/// Jet owns only the source/target request. Jetpack owns project resolution,
/// lock replay, trust, realization, concurrency, and the child environment.
/// The child preserves the original argv shape; only the selected source
/// target is made absolute so the project-root cwd cannot change its identity.
pub(crate) fn prepare_project_environment(cmd: &str, start: &Path, mode: OutputMode) {
    let Some((project_root, environment, import)) =
        project_environment_requirement_with_root(start)
    else {
        return;
    };
    if cmd != "run" {
        let (_, command) = current_jet_invocation();
        crate::emit_cli_row_with_detail(
            "E1355",
            &[
                ("environment", environment.as_str()),
                ("command", command.as_str()),
            ],
            format!(
                " Import: `use {import}` is the package import that requires this environment.\n"
            ),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    }

    let (raw, command) = current_jet_invocation();
    let mut before_separator = raw.iter().take_while(|argument| argument.as_str() != "--");
    if before_separator.any(|argument| argument.as_str() == jet::Syntax::RUN_FLAG_NO_PREPARE) {
        crate::emit_cli_row_with_detail(
            "E1355",
            &[
                ("environment", environment.as_str()),
                ("command", command.as_str()),
            ],
            format!(
                " Import: `use {import}` is the package import that requires this environment.\n"
            ),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    }

    let jet_binary = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            crate::emit_cli_row_with_detail(
                "E1356",
                &[
                    ("command", command.as_str()),
                    ("reason", "the running Jet executable could not be located"),
                ],
                format!(" Import: `use {import}` is the package import that requires this environment: {error}.\n"),
                mode.json,
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    if crate::EngineDispatch::find_engine_binary(jet::Syntax::JETPACK_BINARY_NAME).is_none() {
        crate::emit_cli_row_with_detail(
            "E1356",
            &[
                ("command", command.as_str()),
                ("reason", "the matching Jetpack engine is not installed"),
            ],
            format!(
                " Import: `use {import}` is the package import that requires this environment.\n"
            ),
            mode.json,
        );
        exit(ExitCodes::USER_ERROR);
    }

    let mut forwarded = vec!["env".to_string()];
    forwarded.extend(jetpack_environment_flags(&raw));
    forwarded.push("--".to_string());
    forwarded.push(jet_binary.to_string_lossy().into_owned());
    let child_raw = delegated_run_arguments(&raw, start);
    forwarded.extend(child_raw);
    exit(crate::EngineDispatch::dispatch_in(
        jet::Syntax::JETPACK_BINARY_NAME,
        "env",
        &forwarded,
        Some(&project_root),
    ));
}

/// Copy only flags understood by `jetpack env` before its command separator.
/// Compiler-only flags remain in the original argv and are parsed by the
/// delegated Jet child; forwarding them to Jetpack would turn them into
/// environment positional arguments.
fn jetpack_environment_flags(raw: &[String]) -> Vec<String> {
    let mut forwarded = Vec::new();
    let mut index = 0;
    while index < raw.len() {
        let argument = &raw[index];
        if argument == "--" {
            break;
        }
        match argument.as_str() {
            "--offline" | "--online" | "--trust" | "--flake" | "--pure" | "--yes" | "--json"
            | "--no-color" => forwarded.push(argument.clone()),
            "--fixtures" | "--env" | "--preset" => {
                forwarded.push(argument.clone());
                if let Some(value) = raw.get(index + 1).filter(|value| *value != "--") {
                    let value = if argument == "--fixtures" {
                        absolute_caller_path(value)
                    } else {
                        value.clone()
                    };
                    forwarded.push(value);
                    index += 1;
                }
            }
            value if value.starts_with("--fixtures=") => {
                forwarded.push("--fixtures".to_string());
                forwarded.push(absolute_caller_path(
                    value.trim_start_matches("--fixtures="),
                ));
            }
            value if value.starts_with("--env=") => {
                forwarded.push("--env".to_string());
                forwarded.push(value.trim_start_matches("--env=").to_string());
            }
            value if value.starts_with("--preset=") => {
                forwarded.push("--preset".to_string());
                forwarded.push(value.trim_start_matches("--preset=").to_string());
            }
            _ => {}
        }
        index += 1;
    }
    forwarded
}

fn absolute_caller_path(value: &str) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        return value.to_string();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
        .to_string_lossy()
        .into_owned()
}

/// Keep the delegated source target bound to the path the user selected even
/// though Jetpack resolves the project from its authoritative root. The child
/// runs with that root as its cwd; an absolute target avoids turning a caller
/// relative path into a different source identity. Flags and the `--` program
/// argument boundary remain byte-for-byte unchanged.
fn delegated_run_arguments(raw: &[String], start: &Path) -> Vec<String> {
    let mut child = Vec::with_capacity(raw.len());
    let Some(command) = raw.first() else {
        return child;
    };
    child.push(command.clone());

    let caller_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let source_path = if start.is_absolute() {
        start.to_path_buf()
    } else {
        caller_cwd.join(start)
    };
    let source_path = fs::canonicalize(&source_path).unwrap_or(source_path);
    let source_path = source_path.to_string_lossy().into_owned();
    let mut target_seen = false;
    let mut separate_value = false;
    let mut index = 1;
    while index < raw.len() {
        let argument = &raw[index];
        if argument == "--" {
            child.extend(raw[index..].iter().cloned());
            break;
        }
        if separate_value {
            child.push(argument.clone());
            separate_value = false;
            index += 1;
            continue;
        }
        if takes_separate_run_value(argument) {
            child.push(argument.clone());
            separate_value = !argument.contains('=');
            index += 1;
            continue;
        }
        if argument.starts_with("--") {
            child.push(argument.clone());
            index += 1;
            continue;
        }
        if !target_seen {
            child.push(source_path.clone());
            target_seen = true;
        } else {
            child.push(argument.clone());
        }
        index += 1;
    }
    child
}

fn takes_separate_run_value(argument: &str) -> bool {
    matches!(
        argument,
        "-p" | "--fixtures"
            | "--env"
            | "--preset"
            | "--target"
            | "--profile"
            | "--output"
            | "--record"
            | "--set"
            | "--builder"
            | "--endpoint"
            | "--channel"
            | "--platform"
            | "--trust-key"
            | "--gate"
            | "--allow"
            | "--deny"
            | "--scope"
            | "--kind"
            | "--live"
            | "--replay"
            | "--project"
            | "--sandbox"
            | "--console-ttl"
            | "--app"
            | "--share"
            | "--token"
            | "--base-receipt"
            | "--receipt"
            | "--head-receipt"
            | "--after-receipt"
            | "--canvas-host"
            | "--canvas-port"
            | "--canvas-transport"
            | "--canvas-authority"
            | "--canvas-audit"
            | "--where"
            | "--capture"
            | "--browser"
            | "--browser-retries"
            | "--browser-reporter"
            | "--filter"
            | "--shuffle"
            | "--verify"
            | "--max"
            | "--remote-builder"
            | "--build-root"
            | "--project-root"
            | "--cross"
            | "--arch"
            | "--cpu"
            | "--memory"
            | "--jobs"
            | "--timeout"
            | "--name"
            | "--entry"
            | "--package"
            | "--member"
    )
}

/// Refuse an env-backed `jet` verb unless the caller is already inside the
/// realized project environment. `run` is the sole verb that delegates the
/// existing project boundary to Jetpack; all other verbs stay refusal-only.
pub(crate) fn require_project_environment(cmd: &str, start: &Path, mode: OutputMode) {
    if cmd == "run" {
        prepare_project_environment(cmd, start, mode);
        return;
    }
    let Some((environment, import)) = project_environment_requirement(start) else {
        return;
    };
    let (_, command) = current_jet_invocation();
    crate::emit_cli_row_with_detail(
        "E1355",
        &[
            ("environment", environment.as_str()),
            ("command", command.as_str()),
        ],
        format!(" Import: `use {import}` is the package import that requires this environment.\n"),
        mode.json,
    );
    exit(ExitCodes::USER_ERROR);
}

/// All command-side inputs for one native compile/run request. The CLI parses
/// argv; this seam owns compatibility, profile, tier selection, and the
/// execution lifecycle. Engines receive only the selected execution payload.
pub(crate) struct NativeExecutionRequest<'a> {
    pub(crate) command: &'a str,
    pub(crate) file: &'a str,
    pub(crate) emit_rust: bool,
    pub(crate) emit_generated: bool,
    pub(crate) library: bool,
    pub(crate) small: bool,
    pub(crate) no_os: bool,
    pub(crate) gates: jet::Policy::GateSet,
    pub(crate) build_grants: &'a [String],
    pub(crate) invocation_authority: Option<&'a jet_foundation::Authority::ApplicationAuthority>,
    pub(crate) sbom: bool,
    pub(crate) remote_builder: Option<&'a str>,
    pub(crate) locked: bool,
    pub(crate) target: Option<&'a str>,
    pub(crate) explain_partition: bool,
    pub(crate) verbose: bool,
    pub(crate) target_machine: Option<&'a jet::TargetMachine::TargetMachine>,
    pub(crate) release: bool,
    pub(crate) profile: Option<&'a str>,
    pub(crate) setting_overrides: &'a BTreeMap<String, String>,
    pub(crate) output: Option<&'a str>,
    pub(crate) program_args: &'a [&'a String],
    pub(crate) mode: OutputMode,
    pub(crate) output_profile: Option<&'a jet_cli::OutputProfile::OutputProfile>,
    pub(crate) record: Option<&'a str>,
    pub(crate) interpret: bool,
    pub(crate) entry_fn: Option<&'a str>,
    pub(crate) check_project_scope: bool,
    pub(crate) package_scope: bool,
    pub(crate) build_override: bool,
    pub(crate) source_overlay: Option<(&'a Path, &'a str)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeEngine {
    Interpreter,
    Jit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeTier {
    /// The canonical execution-tier fact. Engine choice is independent from
    /// the build profile and is kept only as a dispatch adapter below.
    execution: jet::TargetMachine::ExecutionTier,
    engine: Option<NativeEngine>,
}

impl NativeTier {
    fn aot() -> Self {
        Self {
            execution: jet::TargetMachine::ExecutionTier::Aot,
            engine: None,
        }
    }

    fn dev(engine: NativeEngine) -> Self {
        Self {
            execution: jet::TargetMachine::ExecutionTier::Dev,
            engine: Some(engine),
        }
    }

    fn jit() -> Self {
        Self {
            execution: jet::TargetMachine::ExecutionTier::Jit,
            engine: Some(NativeEngine::Jit),
        }
    }

    fn is_engine(self) -> bool {
        self.engine.is_some()
    }

    /// Project the selected tier and independent profile into the one typed
    /// build-fact snapshot consumed by artifact/cache/query projections.
    fn project(self, facts: &mut jet_foundation::Facts::BuildFactSnapshot, profile: &BuildProfile) {
        facts.profile = profile.budget_name().to_string();
        facts.target_dossier.tier_identity = self.execution.as_str().to_string();
    }
}

enum NativeRunResult {
    Engine(jet::Interpreter::RunOutcome),
    Child(std::process::ExitStatus),
    Exit(i32),
    LaunchError(String),
}

fn select_native_profile(
    command: &str,
    file: &str,
    no_os: bool,
    small: bool,
    release: bool,
    profile_name: Option<&str>,
    mode: OutputMode,
) -> BuildProfile {
    // D-BUILD-DEFAULT1/D-BUILDPROFILE1: profile selection. An explicit
    // --profile name is authoritative, including when combined with target
    // conveniences such as --small or --no-os; --release supplies the named
    // release profile only when no explicit profile was requested.
    let named_profile = if profile_name == Some(jet::Syntax::BUILD_PROFILE_HARDENED) {
        profile_name
    } else if release {
        Some(jet::Syntax::BUILD_PROFILE_RELEASE)
    } else {
        profile_name
    };
    if let Some(name) = resolve_profile_name(named_profile) {
        resolve_named_profile(&name, file, mode)
    } else if no_os {
        BuildProfile::NoOs
    } else if small {
        BuildProfile::Small
    } else {
        BuildProfile::default_for_command(command)
    }
}

fn select_native_tier(
    command: &str,
    file: &str,
    mode: OutputMode,
    interpret: bool,
    target: Option<&str>,
    output: Option<&str>,
    remote_builder: Option<&str>,
    emit_rust: bool,
    emit_generated: bool,
    small: bool,
    no_os: bool,
    build_grants: &[String],
    sbom: bool,
    profile: &BuildProfile,
    selects_build_entry: bool,
    is_web: bool,
    is_plugin: bool,
    src: &str,
) -> NativeTier {
    if command != "run" {
        return NativeTier::aot();
    }
    if interpret {
        let incompatible = target.is_some()
            || output.is_some()
            || remote_builder.is_some()
            || emit_rust
            || emit_generated
            || small
            || no_os
            || !build_grants.is_empty()
            || sbom;
        if incompatible {
            let diagnostic = jet::Diagnostics::Diagnostic::error(
                "E2102",
                "`--interpret` cannot be combined with build or artifact flags".to_string(),
                "`--interpret` selects the interpreter engine; the build profile remains an independent fact"
                    .to_string(),
                "run `jet run --interpret [--release|--profile <name>] <file.jet>` without artifact flags"
                    .to_string(),
                None,
            );
            report_problems(mode, file, src, &[diagnostic]);
            exit(ExitCodes::USAGE);
        }
        return NativeTier::dev(NativeEngine::Interpreter);
    }
    if matches!(profile, BuildProfile::Fast)
        && target.is_none()
        && output.is_none()
        && !emit_rust
        && !small
        && !no_os
        && build_grants.is_empty()
        && !sbom
        && !is_web
        && !is_plugin
        && !selects_build_entry
    {
        NativeTier::jit()
    } else {
        NativeTier::aot()
    }
}

fn render_native_lints(
    file: &str,
    src: &str,
    mode: OutputMode,
    lints: &[jet::Diagnostics::Diagnostic],
) {
    let lints = crate::CmdDevTools::visible_lints(lints);
    if !lints.is_empty() {
        report_problems(mode, file, src, &lints);
    }
}

fn finalize_tier_trace_sidecar(mode: OutputMode) {
    let aggregate = jet_jit::take_trace_aggregate();
    let Some(path) = std::env::var_os("JET_TRACE_TIERS_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    if let Err(error) = jet_jit::write_trace_sidecar(&path, &aggregate) {
        write_mode_diagnostic(
            mode,
            &format!(
                "couldn't write compiler-owned tier trace `{}`: {}\n",
                path.display(),
                error
            ),
        );
    }
}

fn finish_native_run(
    file: &str,
    src: &str,
    mode: OutputMode,
    record: Option<&crate::ProveReplay::NamedCapture>,
    record_name: Option<&str>,
    production_receipt: Option<&crate::ProductionReceipt::Context>,
    lints: &[jet::Diagnostics::Diagnostic],
    result: NativeRunResult,
) -> ! {
    finalize_tier_trace_sidecar(mode);
    render_native_lints(file, src, mode, lints);
    match result {
        NativeRunResult::Engine(jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        }) => {
            emit_run_output(&stdout, &stderr);
            finish_recorded_artifacts(record, record_name, production_receipt, exit_code, mode);
            exit(exit_code);
        }
        NativeRunResult::Engine(jet::Interpreter::RunOutcome::Problems(diags)) => {
            exit_if_internal_fault(&diags, mode);
            report_problems(mode, file, src, &diags);
            finish_recorded_artifacts(
                record,
                record_name,
                production_receipt,
                ExitCodes::USER_ERROR,
                mode,
            );
            exit(ExitCodes::USER_ERROR);
        }
        NativeRunResult::Child(status) => {
            let exit_code = child_exit_code(status);
            finish_recorded_artifacts(record, record_name, production_receipt, exit_code, mode);
            exit(exit_code);
        }
        NativeRunResult::Exit(exit_code) => {
            finish_recorded_artifacts(record, record_name, production_receipt, exit_code, mode);
            exit(exit_code);
        }
        NativeRunResult::LaunchError(error) => {
            finish_recorded_artifacts(
                record,
                record_name,
                production_receipt,
                ExitCodes::USER_ERROR,
                mode,
            );
            crate::cli_error!("E2105", "couldn't run the built program: {}", error);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn run_native_lens(
    tier: NativeTier,
    file: &str,
    src: &str,
    gates: jet::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
    profile: &BuildProfile,
    entry_fn: Option<&str>,
    program_args: &[&String],
    mode: OutputMode,
    record: Option<&crate::ProveReplay::NamedCapture>,
    record_name: Option<&str>,
    production_receipt: Option<&crate::ProductionReceipt::Context>,
    package_manifest: &mut Option<(PathBuf, jet::Package::PackageFacts)>,
    source_closure: &[(PathBuf, String)],
    source_snapshot: Option<&crate::Store::AuthorityFileSnapshot>,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> ! {
    let application_authority = resolve_run_authority_before_execution(
        file,
        src,
        mode,
        profile.budget_name(),
        setting_overrides,
        entry_fn,
        package_manifest,
        source_closure,
        source_snapshot,
        invocation_authority,
    );

    let args = program_args
        .iter()
        .map(|arg| arg.as_str())
        .collect::<Vec<_>>();
    if let Some(capture) = record {
        try_recorded_run(file, &args, capture, record_name, production_receipt, mode);
    }

    // The adapters marshal checked facts and runtime arguments. The workflow
    // above owns authority; the completion seam below owns all output, lint,
    // diagnostic, and capture handling.
    jet_jit::set_program_owns_streams();
    let run = match tier.engine {
        Some(NativeEngine::Interpreter) => {
            jet::Interpreter::run_interpreter_once_with_source_closure(
                file,
                source_closure,
                &args,
                gates,
                profile.budget_name(),
                setting_overrides,
                application_authority.as_ref(),
                entry_fn,
                jet::Interpreter::InterpreterInvocation::RunInterpret,
            )
        }
        Some(NativeEngine::Jit) => jet::Interpreter::run_jit_once_with_source_closure(
            file,
            source_closure,
            &args,
            gates,
            profile.budget_name(),
            setting_overrides,
            application_authority.as_ref(),
            entry_fn,
        ),
        None => unreachable!("AOT does not use the engine lens"),
    };
    let lints = run.lints;
    finish_native_run(
        file,
        src,
        mode,
        record,
        record_name,
        production_receipt,
        &lints,
        NativeRunResult::Engine(run.outcome),
    )
}
/// Private child entry used by the REPL. The parent sends the exact session
/// snapshot on stdin; this path never admits a source pathname.
pub(crate) fn run_native_source_from_stdin() -> ! {
    let mut source = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut source) {
        crate::cli_error!("E2105", "couldn't read REPL source: {}", error);
        exit(ExitCodes::USER_ERROR);
    }
    run_native_source_execution(&source)
}
fn run_native_source_execution(source: &str) -> ! {
    let file_path = std::env::temp_dir().join("__jet_repl_run_stdin.jet");
    let file = file_path.to_string_lossy().into_owned();
    let empty_strings: Vec<String> = Vec::new();
    let empty_args: Vec<&String> = Vec::new();
    let empty_settings = BTreeMap::new();
    run_native_execution(NativeExecutionRequest {
        command: "run",
        file: &file,
        emit_rust: false,
        emit_generated: false,
        library: false,
        small: false,
        no_os: false,
        gates: jet::Policy::GateSet::default(),
        build_grants: &empty_strings,
        invocation_authority: None,
        remote_builder: None,
        locked: false,
        target: None,
        target_machine: None,
        explain_partition: false,
        verbose: false,
        sbom: false,
        release: false,
        profile: None,
        setting_overrides: &empty_settings,
        output: None,
        program_args: &empty_args,
        mode: OutputMode {
            json: false,
            color: jet::Diagnostics::ColorChoice::Auto,
            quiet: false,
        },
        output_profile: None,
        record: None,
        interpret: false,
        entry_fn: None,
        check_project_scope: false,
        package_scope: true,
        build_override: true,
        source_overlay: Some((&file_path, source)),
    });
    unreachable!("native REPL child execution should exit from the run pipeline")
}

/// Keep the internal web-run guard aligned with the CLI target dispatch:
/// App-returning entries are served by the native runtime edge, while a
/// non-App web entry still requires `jet dev` or a web artifact build.
fn source_entry_returns_app(source: &str) -> bool {
    let source = match jet::Package::mask_inline_package_source(source) {
        Ok((masked, _)) => masked,
        Err(_) => return false,
    };
    let (tokens, diagnostics) = jet::Lexer::lex(&source);
    if !diagnostics.is_empty() {
        return false;
    }
    let program = match jet::Parser::parse_with_source(&tokens, &source) {
        Ok(program) => program,
        Err(_) => return false,
    };
    jet::AST::app_entry_run_fn(&program.items).is_some()
}

pub(crate) fn run_native_execution(request: NativeExecutionRequest<'_>) {
    // Native compilation and the runtime lenses share the compiler-facing
    // `core.compiler` package views. Install the one ambient bridge before
    // entering any Driver path; otherwise `jet run` bypasses the public
    // frontend wrapper and the canonical evaluator rejects those calls.
    let trace_tiers = jet_jit::trace_tiers_enabled();
    jet::with_compiler_stack(|| {
        // `--trace-tiers` is parsed on the CLI thread, while the compiler
        // stack may run the request on its worker. Carry this thread-local
        // execution setting across that boundary before the nested runtime
        // lens captures and publishes its trace.
        jet_jit::set_trace_tiers(trace_tiers);
        run_native_execution_inner(request)
    })
}
fn run_native_execution_inner(request: NativeExecutionRequest<'_>) {
    let NativeExecutionRequest {
        command: cmd,
        file,
        emit_rust,
        emit_generated,
        library: library_flag,
        small,
        no_os,
        gates,
        build_grants,
        invocation_authority,
        remote_builder,
        locked,
        target: cross_target,
        target_machine,
        explain_partition,
        verbose,
        sbom,
        release,
        profile: profile_name,
        setting_overrides,
        output: output_name,
        program_args,
        mode,
        output_profile,
        record: record_name,
        interpret: force_interpreter,
        entry_fn,
        check_project_scope,
        package_scope,
        build_override,
        source_overlay,
    } = request;
    require_project_environment(cmd, Path::new(file), mode);
    let profile = select_native_profile(cmd, file, no_os, small, release, profile_name, mode);
    let release_profile = profile.is_release();
    let mut progress = BuildProgress::new(cmd, emit_rust, verbose, mode, output_profile);
    progress.major("Reading", file);
    progress.minor("profile", profile.budget_name());
    let (src, source_snapshot) = match source_overlay {
        Some((_, source)) => (source.to_owned(), None),
        None => {
            let snapshot = match crate::Store::read_authority_file(Path::new(file)) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    let path = Path::new(file);
                    let fix = if error.kind() == std::io::ErrorKind::NotFound {
                        let default_entry_fix = path.file_name().and_then(|name| name.to_str())
                            == Some(jet::Syntax::DEFAULT_ENTRY_FILE)
                            && path.parent().is_some_and(|parent| {
                                jet::Loader::find_manifest_root(parent).is_some()
                            });
                        if default_entry_fix {
                            format!(
                                "create `{}` in the project, or run `{} {} <file.{}>`",
                                jet::Syntax::DEFAULT_ENTRY_FILE,
                                jet::Syntax::BINARY_NAME,
                                cmd,
                                jet::Syntax::FILE_EXT
                            )
                        } else {
                            format!(
                                "check the spelling, or run {} from the folder that contains it",
                                jet::Syntax::BINARY_NAME
                            )
                        }
                    } else {
                        format!("fix the source authority and retry: {error}")
                    };
                    crate::cli_error!(
                        @fix "E2105",
                        format!("can't securely read the file `{}`", file),
                        fix
                    );
                    exit(ExitCodes::USER_ERROR);
                }
            };
            let source = match snapshot.text() {
                Ok(source) => source.to_owned(),
                Err(error) => {
                    crate::cli_error!(
                        @fix "E2105",
                        format!("can't read the source `{}`", file),
                        format!("fix the source encoding and retry: {error}")
                    );
                    exit(ExitCodes::USER_ERROR);
                }
            };
            (source, Some(snapshot))
        }
    };
    progress.minor("source", &format!("{} bytes", src.len()));
    // Explicit `jet check <file>` must reject project imports before the
    // immutable source-closure loader attempts to resolve them.
    if cmd == "check" && !check_project_scope {
        if let Some(diagnostic) =
            crate::CmdInspect::missing_project_context_diagnostic(Path::new(file))
        {
            report_problems(mode, file, &src, &[diagnostic]);
            exit(ExitCodes::USER_ERROR);
        }
    }
    let source_identity = source_overlay
        .map(|(path, _)| path)
        .or_else(|| source_snapshot.as_ref().map(|snapshot| snapshot.path()))
        .unwrap_or_else(|| Path::new(file));
    let source_closure = match jet::Driver::load_immutable_source_closure(
        file,
        &[(source_identity, src.as_str())],
    ) {
        Ok(source_closure) => source_closure,
        Err(diagnostics) => {
            report_problems(mode, file, &src, &diagnostics);
            exit(ExitCodes::USER_ERROR);
        }
    };

    if cmd == "run"
        && cross_target == Some(jet::Syntax::BUILD_TARGET_WEB)
        && !source_entry_returns_app(&src)
    {
        let diagnostic = jet::Diagnostics::Diagnostic::error(
            "E2102",
            "`jet run` cannot execute a web-targeted program natively".to_string(),
            "the web target produces browser artifacts, not a native console executable".to_string(),
            "use `jet dev <file.jet>` for the browser development loop, or `jet build --target=web <file.jet>` for web artifacts".to_string(),
            None,
        );
        report_problems(mode, file, &src, &[diagnostic]);
        exit(ExitCodes::USER_ERROR);
    }

    let record = if cmd == "run" {
        record_name.map(|name| {
            crate::ProveReplay::begin_named_capture(
                file,
                name,
                profile.budget_name(),
                setting_overrides,
                mode.json,
            )
            .unwrap_or_else(|status| exit(status))
        })
    } else {
        None
    };

    if remote_builder.is_some() && cmd != "build" {
        let diagnostic = jet::Diagnostics::Diagnostic::error(
            "E2102",
            "`--builder` is only valid with `jet build`".to_string(),
            "remote execution is selected at the programmable build boundary".to_string(),
            "run `jet build --builder=<name> <file.jet>`".to_string(),
            None,
        );
        report_problems(mode, file, &src, &[diagnostic]);
        exit(ExitCodes::USAGE);
    }

    if cmd == "check" {
        let mut checked = match jet::with_compiler_stack(|| {
            crate::CmdInspect::check_projection_for_command(
                Path::new(file),
                gates,
                profile.budget_name(),
                setting_overrides,
                if check_project_scope {
                    crate::CmdInspect::CheckScope::Project
                } else {
                    crate::CmdInspect::CheckScope::ExplicitFile
                },
                entry_fn,
                cross_target,
            )
        }) {
            Ok(checked) => Some(checked),
            Err(diagnostics) => {
                let errors: Vec<_> = diagnostics
                    .iter()
                    .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
                    .cloned()
                    .collect();
                if !errors.is_empty() {
                    report_problems(mode, file, &src, &errors);
                    exit(ExitCodes::USER_ERROR);
                }
                None
            }
        };
        let mut all_diags = checked
            .as_ref()
            .map(|projection| projection.diagnostics.clone())
            .unwrap_or_default();
        if let Some(projection) = checked.as_ref() {
            if let Err(error) =
                crate::CmdDevTools::merge_cost_diagnostics(&projection.bundle, &mut all_diags)
            {
                crate::CmdDevTools::exit_cost_projection_error(file, &error);
            }
        }
        let errors: Vec<_> = all_diags
            .iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .cloned()
            .collect();
        if !errors.is_empty() {
            report_problems(mode, file, &src, &errors);
            exit(ExitCodes::USER_ERROR);
        }
        let lints = crate::CmdDevTools::visible_lints(&all_diags);
        if !lints.is_empty() {
            report_problems(mode, file, &src, &lints);
        }
        if !mode.json && !mode.quiet {
            if let Some(projection) = checked.as_ref() {
                write_mode_renderable(
                    mode,
                    &crate::CmdInspect::check_result_text(&projection.check),
                );
            }
            if let Some(checked) = checked.as_mut() {
                if let Some(report) =
                    crate::CmdFill::render_goal_report(checked, None, None, false, false)
                {
                    write_mode_renderable(mode, &report);
                }
            }
        }
        if mode.json && lints.is_empty() {
            if let Some(projection) = checked.as_ref() {
                write_mode_machine(
                    mode,
                    &crate::CmdInspect::check_result_json(&projection.check),
                );
            } else {
                let machine_file = crate::machine_report_path_for_process(file);
                write_mode_machine(mode, &jet::Diagnostics::render_success_json(&machine_file));
            }
        } else if !mode.json && lints.is_empty() && !mode.quiet {
            write_mode_status(mode, &format!("ok: `{file}` has no problems\n"));
        }
        return;
    }

    // E2-M15: validate cross-compilation target before invoking rustc.
    if let Some(triple) = cross_target {
        validate_target(triple, mode);
    }

    let is_web = cross_target == Some(jet::Syntax::BUILD_TARGET_WEB);
    // D-PLUGIN1=B / D-DEP-WASM1=A (c81): `--target=sandbox` routes to the
    // sandboxed WASM Component Model guest build instead of a native binary.
    let is_plugin = cross_target == Some(jet::Syntax::TARGET_SANDBOX);
    // D-DEVR-PROD1=A: native `jet run` gives every execution tier one shared,
    // redacted receipt context. `jet dev` installs the same context at its
    // watcher seam. The Prelude writer owns receipt bytes; this CLI boundary
    // supplies only closure/input digests and a local destination.
    let production_receipt = (cmd == "run" && !is_web && !is_plugin && cross_target.is_none())
        .then(|| crate::ProductionReceipt::prepare(file, &src, program_args));
    if let Some(context) = production_receipt.as_ref() {
        context.install();
    }
    if explain_partition && !is_web {
        let diag = jet::Diagnostics::Diagnostic::error(
            "E2102",
            "`--explain-partition` requires `--target=web`".to_string(),
            "the partition report is only meaningful for the web backend".to_string(),
            format!(
                "run `jet build --target={} --explain-partition <file>`",
                jet::Syntax::BUILD_TARGET_WEB
            ),
            None,
        );
        report_problems(mode, file, &src, &[diag]);
        exit(ExitCodes::USAGE);
    }

    // D-BUILDENTRY1 + I9: a program that selects a `fn build` has one meaning
    // in every tier, and only the build pipeline can produce it — the entry
    // computes fact contributions (D-CONF-SPLIT1), may generate modules, and
    // records both in `.jet/lock`, all before the runtime program is checked.
    // The fact-blind lanes must therefore stand aside so `jet run` stages the
    // build exactly as `jet build` does: otherwise `jet run` folds
    // `@build.settings.*` from manifest declarations while `jet build` and
    // `jet explain` fold the contributed value.
    let mut package_manifest = load_pkg_manifest(file);
    // `jet build` already stages the entry, so only `jet run` has to ask.
    let selects_build_entry = cmd == "run"
        && jet::Driver::selects_build_entry(
            &src,
            package_manifest.as_ref().map(|(root, _)| root.as_path()),
        );

    // D-ONCE-TIER1=A / D-LENS-RUN1: native runs choose one adapter here. The
    // interpreter and strict Cranelift adapters share authority and completion
    // through `run_native_lens`; all other requests continue to the AOT path.
    let tier = select_native_tier(
        cmd,
        file,
        mode,
        force_interpreter,
        cross_target,
        output_name,
        remote_builder,
        emit_rust,
        emit_generated,
        small,
        no_os,
        build_grants,
        sbom,
        &profile,
        selects_build_entry,
        is_web,
        is_plugin,
        &src,
    );
    if tier.is_engine() {
        run_native_lens(
            tier,
            file,
            &src,
            gates,
            setting_overrides,
            &profile,
            entry_fn,
            program_args,
            mode,
            record.as_ref(),
            record_name,
            production_receipt.as_ref(),
            &mut package_manifest,
            &source_closure,
            source_snapshot.as_ref(),
            invocation_authority,
        );
    }

    // D-BUILDNORM1=A (Tower #85): compute the content-cache key from the
    // program's canonical *pre-sema* AST, up front. `mode_tag` keeps the native
    // codegen shapes (plain, no-OS, and audited gates) in separate key spaces.
    // `None` for web/cross builds (they never cache) or an `embed_file` build
    // (external bytes not in the AST) or a parse failure.
    let profile_tag = profile.cache_tag();
    let mode_tag = if no_os {
        "no-os"
    } else if !gates.is_empty() {
        "gated"
    } else {
        "run"
    };
    let cache_profile_tag = format!(
        "{profile_tag};{};aot-target=native",
        setting_overrides_tag(setting_overrides)
    );
    // `jet run` needs its key *before* the front end: a hit below replays the
    // cached binary without loading the program at all. `jet build` stays on
    // the full path (its effect + authority summaries must print), so #2083
    // computes its key further down, from the one front end a build actually
    // runs, instead of from a second independently reloaded copy of the same
    // program.
    let mut native_key = if output_name.is_none()
        && !is_web
        && cross_target.is_none()
        && cmd == "run"
        && !selects_build_entry
    {
        native_cache_key_with_source_closure(
            file,
            &source_closure,
            profile.budget_name(),
            &cache_profile_tag,
            mode_tag,
            invocation_authority,
        )
    } else {
        None
    };
    let native_store = Store::from_env().ok();
    debug_native_cache_event(format!(
        "compile pid={} cmd={} file={} profile={} mode={} key={:?} cwd={} store_dir={:?}",
        std::process::id(),
        cmd,
        file,
        cache_profile_tag,
        mode_tag,
        native_key,
        std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        native_store.as_ref().map(|store| store.root()),
    ));

    // `jet run` short-circuits the whole front end (only the parse inside the
    // key computation ran) when this exact program is already in the content
    // cache. A hit means this program was type-checked, codegen'd, compiled and
    // run once before under this toolchain + profile + manifest — replaying its
    // binary replays a validated result, it never bypasses a check (I2/I3). The
    // toolchain-version + `package.jet` salts guarantee a compiler or policy change
    // invalidates the entry, so effect-budget enforcement can't be masked.
    // `jet build` deliberately stays on the full path below so its effect +
    // authority summaries always print; it still skips rustc via `native_key`.
    // A selected `fn build` also stays on the full path: replaying a binary
    // would skip staging the build entry, so nothing would execute the action
    // graph or record the computed writers in this project's `.jet/lock`.
    if cmd == "run"
        && mode_tag == "run"
        && !emit_rust
        && !selects_build_entry
        && entry_fn.is_none()
        && !src.contains("core.game")
    {
        if let Some(ref key) = native_key {
            let out = bin_path(file);
            if native_store.as_ref().is_some_and(|store| {
                matches!(
                    store.restore_file(key, &out),
                    Ok(ArtifactRestore::Hit { .. })
                )
            }) {
                let _application_authority = resolve_run_authority_before_execution(
                    file,
                    &src,
                    mode,
                    profile.budget_name(),
                    setting_overrides,
                    entry_fn,
                    &mut package_manifest,
                    &source_closure,
                    source_snapshot.as_ref(),
                    invocation_authority,
                );
                if verbose {
                    write_mode_status(
                        mode,
                        "[build] cache hit -> reused cached binary (front end skipped)\n",
                    );
                }
                let mut run_cmd = Command::new(&out);
                for arg in program_args {
                    run_cmd.arg(arg.as_str());
                }
                let status = match run_cmd.status() {
                    Ok(status) => status,
                    Err(error) => finish_native_run(
                        file,
                        &src,
                        mode,
                        record.as_ref(),
                        record_name,
                        production_receipt.as_ref(),
                        &[],
                        NativeRunResult::LaunchError(error.to_string()),
                    ),
                };
                finish_native_run(
                    file,
                    &src,
                    mode,
                    record.as_ref(),
                    record_name,
                    production_receipt.as_ref(),
                    &[],
                    NativeRunResult::Child(status),
                );
            }
        }
    }

    let library_output_names = if cmd == "build" && !is_web && !is_plugin {
        load_pkg_library_output_names(file)
    } else {
        None
    };
    let library_output =
        if cmd == "build" && !is_web && !is_plugin && (library_flag || output_name.is_none()) {
            output_name.map(str::to_owned).or_else(|| {
                library_output_names
                    .as_ref()
                    .and_then(|names| (names.len() == 1).then(|| names[0].clone()))
            })
        } else {
            None
        };
    let is_library = library_flag || library_output.is_some();
    if is_library {
        if let Some(target) = cross_target {
            let diagnostic = jet::Diagnostics::Diagnostic::error(
                "E1341",
                format!("a Library output cannot select target `{target}`"),
                "native Library artifacts use the host ABI and do not have a cross-target projection".to_string(),
                "remove `--target`; build the Library for the host ABI".to_string(),
                None,
            );
            report_problems(mode, file, &src, &[diagnostic]);
            exit(ExitCodes::USER_ERROR);
        }
    }
    progress.major("Checking", "program and build plan");

    // #2083: one front end per build. A `jet build` used to load and
    // type-check its program three times — once inside `native_cache_key`,
    // once inside the compile, and once more for the effect summary below —
    // and throw the first two bundles away. Run the build pipeline's own first
    // stage here, exactly once: the native cache key is hashed from the bundle
    // this compile then emits, the compile resumes from that bundle instead of
    //
    // A failure here is deliberately dropped: the compile below runs the same
    // stage and reports the real diagnostic through the one problem reporter.
    let mut build_front_end =
        if !is_library && output_name.is_none() && (cmd == "build" || selects_build_entry) {
            match jet::prepare_programmable_build_front_end_scoped_with_entry_with_source_closure(
                file,
                locked,
                is_web,
                is_plugin,
                cross_target,
                profile.budget_name(),
                setting_overrides,
                package_scope,
                build_override,
                entry_fn,
                &source_closure,
            ) {
                Ok(prepared) => Some(prepared),
                Err(diagnostics) => {
                    report_problems(mode, file, &src, &diagnostics);
                    exit(ExitCodes::USER_ERROR);
                }
            }
        } else {
            None
        };
    progress.minor(
        "front end",
        if build_front_end.is_some() {
            "checked"
        } else {
            "using direct compilation"
        },
    );
    if cmd == "build" && output_name.is_none() && !is_library && !is_web && cross_target.is_none() {
        native_key = native_cache_key_for_prepared_build(
            file,
            build_front_end.as_ref(),
            &cache_profile_tag,
            mode_tag,
            invocation_authority,
        );
    }
    // D-EFFBUDGET1's summary is a projection of the checked program, so project
    // it from the front end above rather than running a third one for it. Taken
    // before the compile consumes the bundle; printed in its original place.
    //
    // `emitted_program`, not `runtime_program`: when a `fn build` is selected
    // the emitted program is the planned bundle (with generated modules), and
    // the historic check-only pass ran after the build, so only a build that
    // never planned may reuse this bundle and still print the same summary.
    let reused_package_effects = build_front_end.as_ref().and_then(|prepared| {
        let program = prepared.emitted_program()?;
        let facts = prepared.effect_facts();
        let (projection, delegations) = native_effect_projection(
            program,
            &facts.summaries,
            entry_fn,
            package_manifest.as_ref().map(|(_, manifest)| manifest),
        );
        Some((
            jet::EffectBudget::compute_package_effects(program, &facts.solved, &facts.summaries),
            facts.fact_registry.clone(),
            jet::EffectBudget::summary_line_for_program_with_authority(
                program,
                &facts.summaries,
                entry_fn.unwrap_or("run"),
                package_manifest.as_ref().map(|(_, manifest)| manifest),
            ),
            jet::EffectBudget::summary_json_for_program_with_authority(
                program,
                &facts.summaries,
                entry_fn.unwrap_or("run"),
                package_manifest.as_ref().map(|(_, manifest)| manifest),
            ),
            projection,
            delegations,
        ))
    });
    // S59: same story for the C link flags — resolve them from the one loaded
    // program instead of reopening and re-parsing it. E3201 still surfaces from
    // its original place, after the compile succeeds.
    let reused_clinks = build_front_end
        .as_ref()
        .and_then(|prepared| prepared.emitted_program())
        .map(|program| jet::resolve_c_links_for_bundle(program, cross_target));

    // Restore a validated native artifact before code generation. A workspace
    // index and any foreign-linked/bridge-backed program stay on the ordinary
    // path because their final artifact has inputs outside this key.
    let native_cache_hit = if cmd == "build"
        && !emit_rust
        && !emit_generated
        && !is_library
        && output_name.is_none()
        && !is_web
        && !is_plugin
        && cross_target.is_none()
        && Path::new(file).file_name().and_then(|name| name.to_str())
            != Some(jet::Syntax::WORKSPACE_FILE)
        && native_key.is_some()
        && reused_clinks
            .as_ref()
            .is_some_and(|result| matches!(result, Ok(args) if args.is_empty()))
        && !src.contains("core.game")
        && !build_front_end
            .as_ref()
            .and_then(|prepared| prepared.emitted_program())
            .is_some_and(program_uses_game_runtime)
        && build_front_end
            .as_ref()
            .and_then(|prepared| prepared.emitted_program())
            .is_some_and(native_cacheable_program)
    {
        native_key.as_ref().is_some_and(|key| {
            native_store.as_ref().is_some_and(|store| {
                matches!(
                    store.restore_file(key, &bin_path(file)),
                    Ok(ArtifactRestore::Hit { .. })
                )
            })
        })
    } else {
        false
    };

    // D-LINTPOLICY1=A (the override law): visible lints from this compile,
    // captured here (out of the `match` arm's scope) so the `policy.lints`
    // deny-path enforcement below can see them alongside the already-loaded
    // `package.jet` manifest.
    #[allow(unused_assignments)]
    let mut visible_lints: Vec<jet::Diagnostics::Diagnostic> = Vec::new();
    let execution_lints: Vec<jet::Diagnostics::Diagnostic>;
    let mut checked_runtime: Option<jet::AST::ProgramBundle> = None;
    let mut programmable_build_target: Option<String> = None;
    progress.major("Generating", "native code");

    let compile_result = if target_machine.is_some()
        && output_name.is_none()
        && !is_library
        && !selects_build_entry
    {
        let machine = target_machine.expect("target machine checked above");
        match jet::Driver::compile_bundle_path_with_target_machine_and_profile_and_settings_with_source_closure_and_runtime(
            file,
            jet::Sema::CompileMode::Run,
            machine,
            gates,
            profile.budget_name(),
            locked,
            setting_overrides,
            &source_closure,
            invocation_authority,
        ) {
            Ok((output, runtime)) => {
                checked_runtime = (!emit_rust).then_some(runtime);
                Ok(output)
            }
            Err(jet::Driver::TargetMachineCompileError::Diagnostics(diags)) => Err(diags),
            Err(jet::Driver::TargetMachineCompileError::Machine(errors)) => {
                Err(vec![jet::Diagnostics::Diagnostic::error(
                    "E3302",
                    "target machine validation failed".to_string(),
                    format!("typed target facts rejected: {errors:?}"),
                    "select a target with matching providers and memory facts".to_string(),
                    None,
                )])
            }
        }
    } else if is_library {
        jet::Driver::compile_bundle_path_opts_with_source_closure(
            file,
            jet::Sema::CompileMode::Check,
            false,
            gates,
            false,
            false,
            true,
            false,
            None,
            library_output.as_deref(),
            profile.budget_name(),
            setting_overrides,
            locked,
            None,
            &source_closure,
        )
    } else if let Some(output) = output_name {
        if !is_web && !is_plugin {
            match jet::Driver::compile_bundle_path_opts_with_source_closure_and_runtime(
                file,
                jet::Sema::CompileMode::Run,
                no_os,
                gates,
                false,
                false,
                false,
                false,
                cross_target,
                Some(output),
                profile.budget_name(),
                setting_overrides,
                false,
                None,
                &source_closure,
                invocation_authority,
            ) {
                Ok((output, runtime)) => {
                    checked_runtime = (!emit_rust).then_some(runtime);
                    Ok(output)
                }
                Err(diags) => Err(diags),
            }
        } else {
            jet::Driver::compile_bundle_path_opts_with_source_closure(
                file,
                jet::Sema::CompileMode::Run,
                no_os,
                gates,
                is_web,
                is_plugin,
                false,
                false,
                cross_target,
                Some(output),
                profile.budget_name(),
                setting_overrides,
                false,
                None,
                &source_closure,
            )
        }
    } else if cmd == "build" && emit_generated {
        match jet::compile_programmable_build_output_with_builder_and_profile_and_settings_scoped_with_entry_and_authority(
            file,
            build_grants,
            no_os,
            gates,
            locked,
            is_web,
            is_plugin,
            cross_target,
            true,
            remote_builder,
            profile.budget_name(),
            setting_overrides,
            build_front_end.take(),
            package_scope,
            build_override,
            entry_fn,
            false,
            invocation_authority,
        ) {
            Ok(output) => {
                programmable_build_target = programmable_build_target_name(&output);
                checked_runtime = (!emit_rust).then_some(output.runtime).flatten();
                Ok(output.compile)
            }
            Err(diags) => Err(diags),
        }
    } else if native_cache_hit {
        match jet::compile_programmable_build_output_with_builder_and_profile_and_settings_scoped_with_entry_and_authority(
            file,
            build_grants,
            no_os,
            gates,
            locked,
            is_web,
            is_plugin,
            cross_target,
            false,
            remote_builder,
            profile.budget_name(),
            setting_overrides,
            build_front_end.take(),
            package_scope,
            build_override,
            entry_fn,
            true,
            invocation_authority,
        ) {
            Ok(output) => {
                programmable_build_target = programmable_build_target_name(&output);
                checked_runtime = (!emit_rust).then_some(output.runtime).flatten();
                Ok(output.compile)
            }
            Err(diags) => Err(diags),
        }
    } else if cmd == "build" || selects_build_entry {
        match jet::compile_programmable_build_output_with_builder_and_profile_and_settings_scoped_with_entry_and_authority(
            file,
            build_grants,
            no_os,
            gates,
            locked,
            is_web,
            is_plugin,
            cross_target,
            false,
            remote_builder,
            profile.budget_name(),
            setting_overrides,
            build_front_end.take(),
            package_scope,
            build_override,
            entry_fn,
            false,
            invocation_authority,
        ) {
            Ok(output) => {
                programmable_build_target = programmable_build_target_name(&output);
                checked_runtime = (!emit_rust).then_some(output.runtime).flatten();
                Ok(output.compile)
            }
            Err(diags) => Err(diags),
        }
    } else if cmd == "run" && !is_web && !is_plugin {
        match jet::Driver::compile_bundle_path_opts_with_source_closure_and_runtime(
            file,
            jet::Sema::CompileMode::Run,
            no_os,
            gates,
            false,
            false,
            false,
            false,
            cross_target,
            None,
            profile.budget_name(),
            setting_overrides,
            false,
            entry_fn,
            &source_closure,
            invocation_authority,
        ) {
            Ok((output, runtime)) => {
                checked_runtime = (!emit_rust).then_some(runtime);
                Ok(output)
            }
            Err(diags) => Err(diags),
        }
    } else {
        let compile_mode = if is_plugin {
            jet::Sema::CompileMode::Check
        } else {
            jet::Sema::CompileMode::Run
        };
        let compile_target = if is_web {
            None
        } else if is_plugin {
            Some(jet::Syntax::TARGET_SANDBOX)
        } else {
            cross_target
        };
        jet::Driver::compile_bundle_path_opts_with_source_closure(
            file,
            compile_mode,
            no_os,
            gates,
            is_web,
            is_plugin,
            false,
            false,
            compile_target,
            None,
            profile.budget_name(),
            setting_overrides,
            false,
            entry_fn,
            &source_closure,
        )
    };
    let (
        mut rust_code,
        ffi_link,
        clinks,
        web_out,
        web_partition_report,
        plugin_out,
        library_out,
        library_config,
    ) = match compile_result {
        Ok(out) => {
            // D-A11YGATE1=B (c134 Phase 6): a11y lints (E2930/E2931) are opt-in
            // via `jet lint --a11y`; ordinary build/run never surfaces them.
            let lints = crate::CmdDevTools::visible_lints(&out.lints);
            visible_lints = lints.clone();
            let warning_lints = package_manifest
                .as_ref()
                .map(|(_, manifest)| jet::LintPolicy::non_denied(&lints, manifest))
                .unwrap_or_else(|| lints.clone());
            if cmd == "build" {
                render_native_lints(file, &src, mode, &warning_lints);
            }
            execution_lints = warning_lints;
            // S59 (E2-M14): resolve native C link flags at build time; E3201
            // (unresolved C lib) surfaces here, not during front-end checking.
            let clinks = match reused_clinks.unwrap_or_else(|| {
                jet::resolve_c_links_for_target_with_source_closure(
                    file,
                    cross_target,
                    &source_closure,
                )
            }) {
                Ok(args) => args,
                Err(diags) => {
                    report_problems(mode, file, &src, &diags);
                    exit(ExitCodes::USER_ERROR);
                }
            };
            let library_rust = out.library.as_ref().map(|library| library.rust.clone());
            (
                library_rust.unwrap_or(out.rust),
                out.ffi,
                clinks,
                out.web,
                out.web_partition_report,
                out.plugin,
                out.library,
                out.library_config,
            )
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let game_runtime = checked_runtime
        .as_ref()
        .is_some_and(program_uses_game_runtime)
        || src.contains("core.game");
    if let Some(bundle) = checked_runtime.as_mut() {
        tier.project(&mut bundle.build_facts, &profile);
    }
    inject_game_crash_reporter_bootstrap(&mut rust_code, game_runtime);
    progress.minor("generated Rust", &format!("{} bytes", rust_code.len()));

    if emit_rust {
        write_mode_renderable(mode, &rust_code);
    }

    // D-EFFBUDGET1: zero-config effect summary on every build, plus opt-in
    // whole-graph enforcement when `package.jet` declares `authority.holds`.
    // summary is the whole-program effect fixpoint (`Sema::solve`) that
    // ordinary compilation doesn't need to return.
    let mut build_effect_json = None;
    if cmd == "build" || cmd == "run" {
        // #2083: reuse the fixpoint the build front end already solved. Only
        // the paths that never ran it — a plain `jet run`, a library or named
        // output, a package build entry — still pay a check-only pass here.
        let effect_view = match reused_package_effects {
            Some(view) => Some(view),
            None => {
                let checked = if is_library {
                    jet::Driver::check_file_with_effect_facts_profile_and_settings_with_source_closure(
                        file,
                        &source_closure,
                        profile.budget_name(),
                        setting_overrides,
                    )
                } else {
                    match output_name {
                        Some(output) => {
                            jet::Driver::check_file_with_effect_facts_for_output_with_source_closure(
                                file,
                                output,
                                &source_closure,
                                profile.budget_name(),
                                setting_overrides,
                            )
                        }
                        None => jet::Driver::check_file_with_effect_facts_for_run_and_entry_with_source_closure(
                            file,
                            &source_closure,
                            profile.budget_name(),
                            setting_overrides,
                            entry_fn,
                        ),
                    }
                };
                let (diagnostics, bundle, facts) = checked;
                if diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
                {
                    report_problems(mode, file, &src, &diagnostics);
                    exit(ExitCodes::USER_ERROR);
                }
                let Some(bundle) = bundle else {
                    let diagnostic = jet::Diagnostics::Diagnostic::error(
                        "E2105",
                        "could not obtain the checked effect view".to_string(),
                        "effect enforcement must use the same immutable source checked for compilation"
                            .to_string(),
                        "retry after fixing the source authority".to_string(),
                        None,
                    );
                    report_problems(mode, file, &src, &[diagnostic]);
                    exit(ExitCodes::USER_ERROR);
                };
                let (projection, delegations) = native_effect_projection(
                    &bundle,
                    &facts.summaries,
                    entry_fn,
                    package_manifest.as_ref().map(|(_, manifest)| manifest),
                );
                Some((
                    jet::EffectBudget::compute_package_effects(
                        &bundle,
                        &facts.solved,
                        &facts.summaries,
                    ),
                    facts.fact_registry.clone(),
                    jet::EffectBudget::summary_line_for_program_with_authority(
                        &bundle,
                        &facts.summaries,
                        entry_fn.unwrap_or("run"),
                        package_manifest.as_ref().map(|(_, manifest)| manifest),
                    ),
                    jet::EffectBudget::summary_json_for_program_with_authority(
                        &bundle,
                        &facts.summaries,
                        entry_fn.unwrap_or("run"),
                        package_manifest.as_ref().map(|(_, manifest)| manifest),
                    ),
                    projection,
                    delegations,
                ))
            }
        };
        if let Some((
            entries,
            fact_registry,
            _effect_summary,
            _effect_json,
            mut projection,
            delegations,
        )) = effect_view
        {
            let application_authority = apply_native_effect_policy(
                cmd,
                file,
                &src,
                mode,
                is_plugin,
                &visible_lints,
                &entries,
                &fact_registry,
                &mut projection,
                &mut package_manifest,
                &delegations,
                source_snapshot.as_ref(),
                invocation_authority,
            );
            if let (Some(bundle), Some(application_authority)) =
                (checked_runtime.as_mut(), application_authority.as_ref())
            {
                let required_effects = bundle
                    .package_guarantees
                    .application_authority
                    .required_effects
                    .clone();
                let mut applied = application_authority.clone();
                applied.required_effects = required_effects;
                bundle.package_guarantees.application_authority = applied;
            }
            let effect_summary = jet::EffectBudget::render_effect_projection_line(&projection);
            // Program stdout stays the program's (U7 / D-DEVMODE1). The
            // effect summary is build-time tool output, not runtime stderr.
            if cmd == "build" {
                if mode.json {
                    build_effect_json = Some(jet::EffectBudget::render_effect_projection_json(
                        &projection,
                    ));
                } else {
                    write_mode_status(mode, &effect_summary);
                }
            }
        }
    }

    match cmd {
        "build" => {
            progress.major("Building", "backend artifacts");
            if is_library {
                let library = library_out.as_ref().unwrap_or_else(|| {
                    write_mode_diagnostic(
                        mode,
                        &jet::Diagnostics::render_ice_report(
                            "missing Library codegen output",
                            "",
                            false,
                        ),
                    );
                    exit(ExitCodes::ICE);
                });
                let config = library_config.as_ref().unwrap_or_else(|| {
                    write_mode_diagnostic(
                        mode,
                        &jet::Diagnostics::render_ice_report(
                            "missing Library output configuration",
                            "",
                            false,
                        ),
                    );
                    exit(ExitCodes::ICE);
                });
                let paths = match build_library(
                    &rust_code,
                    library,
                    config,
                    profile,
                    ffi_link.as_ref(),
                    verbose,
                    mode,
                ) {
                    Ok(paths) => paths,
                    Err(LibraryBuildError::GeneratedCode(stderr)) => {
                        write_mode_diagnostic(
                            mode,
                            &jet::Diagnostics::render_ice_report(
                                "rustc rejected generated Library code",
                                &stderr,
                                true,
                            ),
                        );
                        exit(ExitCodes::ICE);
                    }
                    Err(error) => {
                        crate::cli_error!(
                            "E2105",
                            "couldn't publish the Library output: {}",
                            error
                        );
                        exit(ExitCodes::USER_ERROR);
                    }
                };
                if !mode.quiet && !mode.json {
                    if let Some(shared) = &paths.shared {
                        write_mode_renderable(mode, &format!("built: {}\n", shared.display()));
                    }
                    if let Some(staticlib) = &paths.staticlib {
                        write_mode_renderable(mode, &format!("built: {}\n", staticlib.display()));
                    }
                    if let Some(header) = &paths.header {
                        write_mode_renderable(mode, &format!("built: {}\n", header.display()));
                    }
                    if let Some(loadable) = &paths.loadable {
                        write_mode_renderable(mode, &format!("built: {}\n", loadable.display()));
                    }
                    for binding in &paths.bindings {
                        write_mode_renderable(mode, &format!("built: {}\n", binding.display()));
                    }
                }
                progress.finish("library artifacts");
                if mode.json {
                    write_mode_machine(
                        mode,
                        &format!(
                            "{}\n",
                            build_effect_json.as_deref().unwrap_or("{\"effects\":[]}")
                        ),
                    );
                }
                return;
            }
            let artifact_path = if is_web || is_plugin {
                bin_path(file)
            } else {
                build_artifact_path(file, programmable_build_target.as_deref())
            };
            let budget_profile = profile.budget_name().to_string();
            let hardened_profile = matches!(profile, BuildProfile::Hardened);
            build_target_machine(
                file,
                &rust_code,
                checked_runtime.as_ref(),
                artifact_path.clone(),
                profile,
                ffi_link.as_ref(),
                &clinks,
                verbose,
                cross_target,
                web_out.as_ref(),
                plugin_out.as_ref(),
                mode,
                native_cache_hit,
                native_key.clone(),
                target_machine,
            );
            print_release_job_summary(&src, release_profile, mode);
            progress.major("Verifying", "build budgets");
            // D-PERFBUDGET-INTEGRATION1: every build enforces applicable
            // deterministic Fail budgets through CmdBudget's one canonical
            // evaluator/report path. Cross backends use their semantic target
            // class; native remains the default current target.
            let budget_target = if is_web {
                "web"
            } else if is_plugin {
                "sandbox"
            } else {
                "native"
            };
            if crate::CmdBudget::run_build_gates(
                file,
                &artifact_path,
                budget_target,
                &budget_profile,
            ) != 0
            {
                exit(ExitCodes::USER_ERROR);
            }
            if !mode.quiet && !mode.json {
                let artifact = if is_web {
                    "build/app.wasm + build/app.js".to_string()
                } else if is_plugin {
                    format!("build/{}.wasm (sandbox)", stem(file))
                } else {
                    artifact_path.display().to_string()
                };
                let artifact = if hardened_profile {
                    format!("{artifact} (hardened; foreign dependencies fenced)")
                } else {
                    artifact
                };
                if progress.enabled {
                    progress.finish(&artifact);
                } else {
                    write_mode_renderable(mode, &format!("built: {artifact}\n"));
                }
            }
            if explain_partition && !mode.json {
                if let Some(report) = &web_partition_report {
                    write_mode_renderable(mode, &format!("{report}\n"));
                }
            }
            if let Some(triple) = cross_target {
                if !mode.quiet && !mode.json {
                    write_mode_status(mode, &format!("target: {triple}\n"));
                }
            }
            // D-SUPPLY1: `--sbom` writes an SPDX SBOM next to the binary.
            if sbom {
                write_sbom_for_build(file, &artifact_path, mode);
            }
            if mode.json {
                write_mode_machine(
                    mode,
                    &format!(
                        "{}\n",
                        build_effect_json.as_deref().unwrap_or("{\"effects\":[]}")
                    ),
                );
            }
        }
        "run" => {
            let out = if is_web || is_plugin {
                bin_path(file)
            } else {
                build_artifact_path(file, programmable_build_target.as_deref())
            };
            // AOT children inherit stdout/stderr. Render their lints before
            // building/spawning so diagnostics keep the same order as the
            // program's streams; the completion seam receives no lints for
            // this branch and therefore does not print them twice.
            build_target_machine(
                file,
                &rust_code,
                checked_runtime.as_ref(),
                out.clone(),
                profile,
                ffi_link.as_ref(),
                &clinks,
                verbose,
                cross_target,
                web_out.as_ref(),
                plugin_out.as_ref(),
                mode,
                false,
                native_key.clone(),
                target_machine,
            );
            print_release_job_summary(&src, release_profile, mode);
            if cross_target.is_some() {
                write_mode_status(
                    mode,
                    "note: cross-compiled binary cannot run on this host — use emulation (see docs/embedded.md)\n",
                );
                finish_native_run(
                    file,
                    &src,
                    mode,
                    record.as_ref(),
                    record_name,
                    production_receipt.as_ref(),
                    &[],
                    NativeRunResult::Exit(ExitCodes::OK),
                );
            }
            let mut run_cmd = Command::new(&out);
            for arg in program_args {
                run_cmd.arg(arg.as_str());
            }
            let status = match run_cmd.status() {
                Ok(status) => status,
                Err(error) => finish_native_run(
                    file,
                    &src,
                    mode,
                    record.as_ref(),
                    record_name,
                    production_receipt.as_ref(),
                    &[],
                    NativeRunResult::LaunchError(error.to_string()),
                ),
            };
            finish_native_run(
                file,
                &src,
                mode,
                record.as_ref(),
                record_name,
                production_receipt.as_ref(),
                &[],
                NativeRunResult::Child(status),
            );
        }
        other => {
            crate::cli_error!(
                "E2101",
                "`{}` isn't a {} command",
                other,
                jet::Syntax::BINARY_NAME
            );
            write_mode_diagnostic(mode, &usage());
            exit(ExitCodes::USAGE);
        }
    }
}

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when `file`
/// defines a top-level `fn dev()` — compile NATIVELY with `dev()` swapped in
/// as the program's real entry point (`jet::compile_with_entry`), then run the
/// resulting binary exactly like `jet run` does. `dev()`'s own body decides
/// what happens next — normally configuring and starting a `core.web.devserver`
/// value, but it's just an ordinary function; this call site owns none of
/// that behavior (I3: codegen/the driver stay dumb about what `dev()` does).
pub(crate) fn run_dev_entry(
    file: &str,
    profile: BuildProfile,
    mode: OutputMode,
    setting_overrides: &BTreeMap<String, String>,
    program_args: &[&String],
    record_name: Option<&str>,
) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    // D-DEVR-PROD1=A / I9: the native `fn dev()` entry uses the same receipt
    // context as `jet run`, while the compiled program remains the adapter.
    let production_receipt = crate::ProductionReceipt::prepare(file, &src, program_args);
    production_receipt.install();
    let record = record_name.map(|name| {
        crate::ProveReplay::begin_named_capture(
            file,
            name,
            profile.budget_name(),
            setting_overrides,
            mode.json,
        )
        .unwrap_or_else(|status| exit(status))
    });
    let out = match jet::compile_with_entry_and_settings(
        file,
        "dev",
        profile.budget_name(),
        setting_overrides,
    ) {
        Ok(out) => out,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    crate::CmdDevTools::render_lints(file, mode, &out.lints);
    let clinks = match jet::resolve_c_links(file) {
        Ok(args) => args,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = bin_path(file);
    build(
        file,
        &out.rust,
        None,
        bin.clone(),
        profile,
        out.ffi.as_ref(),
        &clinks,
        false,
        None,
        None,
        None,
        mode,
        false,
        // `jet dev` is an interactive live-reload loop (entry-swapped codegen);
        // not worth a content-cache entry. Still race-safe via `build`'s
        // per-process temp path.
        None,
    );
    // `devserver.app()` support: hand the running `dev()` program the
    // canonical absolute path of the file `jet dev` was pointed at, so "the
    // file being run is the file to watch" needs no path spelled out in the
    // Jet source — and works from any invocation directory (a relative
    // string literal in `for_app(...)` only resolves from one cwd).
    let dev_file = fs::canonicalize(file)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| file.to_string());
    // JET_BIN: the devserver's rebuild subprocess must use THIS `jet`, not
    // whatever a bare PATH lookup finds — a different `jet` on PATH could be
    // a different version, and cwd-sensitive wrappers (the repo's own nix
    // devshell `jet` resolves target/debug/jet relative to cwd) break
    // outright when the rebuild runs from a staging directory.
    let jet_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| jet::Syntax::BINARY_NAME.to_string());
    let status = Command::new(&bin)
        .args(program_args)
        .env("JET_DEV_FILE", &dev_file)
        .env("JET_BIN", &jet_bin)
        .status()
        .unwrap_or_else(|e| {
            crate::cli_error!("E2105", "couldn't run the built program: {}", e);
            exit(ExitCodes::USER_ERROR);
        });
    let exit_code = child_exit_code(status);
    finish_recorded_artifacts(
        record.as_ref(),
        record_name,
        Some(&production_receipt),
        exit_code,
        mode,
    );
    exit(exit_code);
}

/// D-WEBAPP-SERVE1=D: `jet dev` serves an App returned by `fn run`
/// through the same native app entry as `jet run`, adding only the reload
/// response flag. A user-authored `fn dev()` is selected before this helper.
pub(crate) fn run_web_app_dev_entry(
    file: &str,
    mode: OutputMode,
    port: Option<u16>,
    profile: Option<&str>,
    setting_overrides: &BTreeMap<String, String>,
    record_name: Option<&str>,
    passthrough: &[&String],
) {
    let _src = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(_) => {
            crate::cli_error!("E2105", "can't find the file `{file}`");
            exit(ExitCodes::USER_ERROR);
        }
    };
    let record = record_name.map(|name| {
        crate::ProveReplay::begin_named_capture(
            file,
            name,
            profile.unwrap_or("dev"),
            setting_overrides,
            mode.json,
        )
        .unwrap_or_else(|status| exit(status))
    });
    let dev_file = fs::canonicalize(file)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| file.to_string());
    let jet_bin = std::env::current_exe().unwrap_or_else(|_| {
        crate::cli_error!("E2105", "couldn't locate the running Jet executable");
        exit(ExitCodes::USER_ERROR);
    });
    let mut command = Command::new(jet_bin);
    command.arg("run").arg(file);
    if let Some(profile) = profile {
        command.arg(format!("--profile={profile}"));
    }
    for (key, value) in setting_overrides {
        command.arg(format!("--set={key}={value}"));
    }
    if !passthrough.is_empty() {
        command.arg("--").args(passthrough);
    }
    command.env("JET_APP_DEV", "1");
    command.env("JET_DEV_FILE", dev_file);
    if let Some(port) = port {
        command.env("JET_APP_PORT", port.to_string());
    }
    let status = command.status().unwrap_or_else(|error| {
        crate::cli_error!("E2105", "couldn't run the web app: {error}");
        exit(ExitCodes::USER_ERROR);
    });
    let exit_code = child_exit_code(status);
    finish_recorded_artifacts(record.as_ref(), record_name, None, exit_code, mode);
    exit(exit_code);
}

#[derive(Debug)]
struct JobListing {
    name: String,
    doc: Option<String>,
    schedule: Option<String>,
    metadata: Option<jet::AST::JobMetadata>,
}

fn marker_string(marker: &jet::AST::Marker) -> Option<String> {
    match marker.expr_arg(0) {
        Some(jet::AST::Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            jet::AST::StrPart::Lit(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn schedule_text(marker: &jet::AST::EveryMarker) -> Option<String> {
    match &marker.arg {
        jet::AST::EveryArg::Duration {
            int, float, suffix, ..
        } => Some(format!(
            "{}{suffix}",
            int.map(|value| value.to_string())
                .or_else(|| float.map(|value| value.to_string()))
                .unwrap_or_default()
        )),
        jet::AST::EveryArg::WallClock { text, .. } => Some(format!("day {text}")),
        jet::AST::EveryArg::Expression(_) => None,
    }
}

/// D-TASKS-LIST1=A / D-JOB-NAME1=A: one lex+parse source for E1294 and `jet jobs`.
/// Source order, `#Doc`, and `#Every` metadata all come from this program.
/// Listing is reached via `jet jobs`; the former tasks subcommand is retired.
fn list_job_names(src: &str) -> Result<Vec<JobListing>, Vec<jet::Diagnostics::Diagnostic>> {
    let source = match jet::Package::mask_inline_package_source(src) {
        Ok((masked, _)) => masked,
        Err(error) => return Err(vec![error.diagnostic()]),
    };
    let (toks, lex_diags) = jet::Lexer::lex(&source);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let prog = jet::Parser::parse_with_source(&toks, &source)?;
    Ok(prog
        .items
        .iter()
        .filter_map(|i| match i {
            jet::AST::Item::Func(f) if f.is_job => {
                let doc = prog
                    .applied_rules
                    .iter()
                    .find(|application| {
                        application.target == Some(f.span)
                            && application.marker.name == jet::Syntax::MARKER_DOC
                    })
                    .and_then(|application| marker_string(&application.marker));
                Some(JobListing {
                    name: f.name.clone(),
                    doc,
                    schedule: f.every.as_ref().and_then(schedule_text),
                    metadata: f.job_metadata.clone(),
                })
            }
            _ => None,
        })
        .collect())
}

/// D-JOB-SUBCMD1=C: release binaries intentionally expose only `.Ship` jobs.
/// Keep the dropped development surface visible at the build boundary so a
/// release cannot silently lose a command the author expected to ship.
fn print_release_job_summary(src: &str, release: bool, mode: OutputMode) {
    if !release || mode.quiet || mode.json {
        return;
    }
    let Ok(jobs) = list_job_names(src) else {
        return;
    };
    let stripped = jobs
        .iter()
        .filter(|job| {
            job.metadata
                .as_ref()
                .map(|metadata| metadata.scope != jet::AST::JobScope::Ship)
                .unwrap_or(true)
        })
        .map(|job| job.name.as_str())
        .collect::<Vec<_>>();
    if !stripped.is_empty() {
        write_mode_status(
            mode,
            &format!(
                "stripped {} dev job(s): {} (mark #Job(.Ship) to include)\n",
                stripped.len(),
                stripped.join(", ")
            ),
        );
    }
}

/// D-SUPPLY1 — write an SPDX SBOM next to the freshly built binary.
///
/// Best-effort: an SBOM describes the *dependency* graph, so a single-file
/// program with no project is emitted with just the root component. When a
/// Package root and lockfile exist, the SBOM lists every locked dependency with
/// its tree-hash checksum.
fn write_sbom_for_build(file: &str, bin: &Path, mode: OutputMode) {
    let file_path = Path::new(file);
    let search_from = file_path.parent().unwrap_or(Path::new("."));

    let (name, version, lock) = match jet::Loader::package_facts_for_entry(file_path) {
        Ok(Some(facts)) => {
            let root = jet::Loader::find_manifest_root(search_from)
                .unwrap_or_else(|| search_from.to_path_buf());
            (
                facts.name,
                facts.version.unwrap_or_else(|| "0.0.0".to_string()),
                jet::Lock::load(&root),
            )
        }
        Ok(None) | Err(_) => (stem(file), "0.0.0".to_string(), None),
    };

    let lock = lock.unwrap_or_else(|| jet::Lock::LockFile {
        version: 1,
        packages: Vec::new(),
        root_dependencies: Vec::new(),
        authority: None,
        workspace_members: Vec::new(),
        workspace_source_digest: None,
        workspace_overlay_policy: Default::default(),
        comptime_inputs: Vec::new(),
        toolchains: Vec::new(),
        browsers: Vec::new(),
        source_channels: Vec::new(),
        build_stamp: None,
        build_contributions: Vec::new(),
    });

    let sbom = jet::Publish::emit_spdx(&lock, &name, &version);
    let out = bin.with_extension("spdx");
    match fs::write(&out, sbom) {
        // #1659 criterion 3: the SBOM was still written; `--quiet` only mutes
        // this confirmation line, never the warning below.
        Ok(()) => {
            if !mode.quiet && !mode.json {
                write_mode_status(mode, &format!("sbom: {}\n", out.display()));
            }
        }
        Err(e) => write_mode_diagnostic(
            mode,
            &format!("warning: couldn't write SBOM to {}: {}\n", out.display(), e),
        ),
    }
}

struct FixPlan {
    before: String,
    staged: String,
    edits: usize,
    skipped_suggestions: usize,
}

/// Apply all auto-fixable diagnostics in a source file in place (D-LSP7 / M13).
/// Goes through `jet::LSP::collect_fixes` / `apply_all` — the SAME unified fix
/// engine the LSP code-action layer uses — so a fix on the command line and a
/// fix in the editor are byte-identical. `--dry-run` shows the diff without
/// writing. With `--edition=2027`, apply encoding-surface migrations first
/// (D-JSONCANON1 / D-ENC-CBOR-SURFACE1 / D-ENCBASE-STRICT1).
pub(crate) fn run_fix(
    file: &str,
    dry_run: bool,
    edition: Option<&str>,
    all: bool,
    mode: OutputMode,
) {
    macro_rules! status {
        ($($args:tt)*) => {{
            let text = format!($($args)*);
            write_mode_status(mode, &format!("{text}\n"));
        }};
    }
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!("E2105", "can't find the file `{}`", file);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let edition_migrated = if edition == Some("2027") {
        apply_edition_2027_encoding_fixes(&src)
    } else {
        src.clone()
    };
    let (migrated, retired_target_count) = rewrite_retired_package_targets(&edition_migrated, file);
    if edition == Some("2027") {
        for note in edition_2027_encoding_audit(&src, &edition_migrated) {
            status!("{file}: edition 2027 migration: {note}");
        }
    }
    let retired_selector_count =
        jet::Formatter::retired_interpolation_selector_edits(&migrated).len();
    let retired_print_count = jet::Formatter::retired_print_family_edits(&migrated).len();
    let retired_type_count = jet::Formatter::retired_type_edits(&migrated).len();
    let fixes = jet::LSP::collect_fixes(file, &migrated);
    let selected_fixes = if all {
        fixes.clone()
    } else {
        jet::LSP::safe_fixes(&fixes)
    };
    let skipped_suggestions = fixes.len().saturating_sub(selected_fixes.len());
    let fixed = if selected_fixes.is_empty() {
        migrated
    } else {
        jet::LSP::apply_all(&migrated, &selected_fixes)
    };
    let plan = FixPlan {
        before: src.clone(),
        staged: fixed,
        edits: selected_fixes.len(),
        skipped_suggestions,
    };
    if plan.staged == plan.before {
        if plan.skipped_suggestions == 0 {
            status!("{}: no changes made", file);
        } else {
            status!(
                "{}: no changes made ({} suggestion{} need review)",
                file,
                plan.skipped_suggestions,
                if plan.skipped_suggestions == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }
        return;
    }
    let n = plan.edits;
    if dry_run {
        write_mode_renderable(
            mode,
            &jet::Formatter::unified_diff(file, &plan.before, &plan.staged),
        );
        if n == 0
            && retired_target_count == 0
            && retired_selector_count == 0
            && retired_print_count == 0
            && retired_type_count == 0
        {
            status!(
                "{}: would apply edition migration (dry run; nothing written)",
                file
            );
        } else if n > 0 {
            status!(
                "{}: would apply {} fix{} (dry run; nothing written)",
                file,
                n,
                if n == 1 { "" } else { "es" }
            );
        }
        if retired_target_count > 0 {
            status!(
                "{}: rewrote {} retired target spelling{} from `plugin` to `sandbox` (D-ONCE-SANDBOX1=A)",
                file,
                retired_target_count,
                if retired_target_count == 1 { "" } else { "s" }
            );
        }
        if retired_selector_count > 0 {
            status!(
                "{}: rewrote {} retired interpolation selector{} from `#` to `:` (D-ONCE-HASH1)",
                file,
                retired_selector_count,
                if retired_selector_count == 1 { "" } else { "s" }
            );
        }
        if retired_print_count > 0 {
            status!(
                "{}: rewrote {} retired print-family spelling{} (D-ONCE-PRINT1=A)",
                file,
                retired_print_count,
                if retired_print_count == 1 { "" } else { "s" }
            );
        }
        if retired_type_count > 0 {
            status!(
                "{}: rewrote {} retired Core container name{} (D-COLLNAME1=A)",
                file,
                retired_type_count,
                if retired_type_count == 1 { "" } else { "s" }
            );
        }
        if plan.skipped_suggestions > 0 {
            status!(
                "{}: skipped {} suggestion{} for review (dry run)",
                file,
                plan.skipped_suggestions,
                if plan.skipped_suggestions == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }
        return;
    }
    let log = crate::CmdCodemod::commit_fix(
        Path::new(file),
        plan.before.into_bytes(),
        plan.staged.into_bytes(),
    );
    if n == 0
        && retired_target_count == 0
        && retired_selector_count == 0
        && retired_print_count == 0
        && retired_type_count == 0
    {
        status!("{}: applied edition migration", file);
    } else if n > 0 {
        status!(
            "{}: applied {} fix{}",
            file,
            n,
            if n == 1 { "" } else { "es" }
        );
    }
    if plan.skipped_suggestions > 0 {
        status!(
            "{}: skipped {} suggestion{} for review",
            file,
            plan.skipped_suggestions,
            if plan.skipped_suggestions == 1 {
                ""
            } else {
                "s"
            }
        );
    }
    status!("  log: {}", log.display());
    if retired_target_count > 0 {
        status!(
            "{}: rewrote {} retired target spelling{} from `plugin` to `sandbox` (D-ONCE-SANDBOX1=A)",
            file,
            retired_target_count,
            if retired_target_count == 1 { "" } else { "s" }
        );
    }
    if retired_selector_count > 0 {
        status!(
            "{}: rewrote {} retired interpolation selector{} from `#` to `:` (D-ONCE-HASH1)",
            file,
            retired_selector_count,
            if retired_selector_count == 1 { "" } else { "s" }
        );
    }
    if retired_print_count > 0 {
        status!(
            "{}: rewrote {} retired print-family spelling{} (D-ONCE-PRINT1=A)",
            file,
            retired_print_count,
            if retired_print_count == 1 { "" } else { "s" }
        );
    }
    if retired_type_count > 0 {
        status!(
            "{}: rewrote {} retired Core container name{} (D-COLLNAME1=A)",
            file,
            retired_type_count,
            if retired_type_count == 1 { "" } else { "s" }
        );
    }
}

fn apply_edition_2027_encoding_fixes(src: &str) -> String {
    let mut out = src.to_string();
    out = replace_untyped_call(&out, "cbor.encode(", "cbor.to_bytes(");
    out = replace_untyped_call(&out, "cbor.decode(", "cbor.parse(");
    out = rewrite_json_canonical_calls(&out);
    out
}

fn rewrite_json_canonical_calls(src: &str) -> String {
    let needle = "json.canonical(";
    let mut out = String::with_capacity(src.len());
    let mut base = 0usize; // byte offset into `src` of the start of `rest`
    let mut rest = src;
    while let Some(rel) = rest.find(needle) {
        let index = base + rel;
        out.push_str(&rest[..rel]);
        let after = &rest[rel + needle.len()..];
        let Some(end) = find_matching_paren(after) else {
            out.push_str(needle);
            base = index + needle.len();
            rest = after;
            continue;
        };
        let call = format!("{needle}{}", &after[..=end]);
        let trailing = &after[end + 1..];
        // D-JSONCANON1 idempotency: skip whitespace before testing for an
        // already-migrated `?` (fallible propagation) or `??` (panic
        // fallback) — a space before `??` used to defeat `starts_with('?')`
        // and cause `jet fix` to double-append the fallback on a re-run.
        let already = trailing.trim_start().starts_with('?');
        if already {
            out.push_str(&call);
        } else if enclosing_fn_is_fallible(src, index) {
            // D-JSONCANON1: inside a fallible function, propagate with `?`.
            out.push_str(&format!("{call}?"));
        } else {
            // D-JSONCANON1: otherwise, the ratified panic fallback.
            out.push_str(&format!("{call} ?? panic(\"value is not canonical JSON\")"));
        }
        base = index + call.len();
        rest = trailing;
    }
    out.push_str(rest);
    out
}

/// D-JSONCANON1 migration: is the function enclosing byte offset `at` in
/// `src` fallible (S34 `-> T !E` / omitted contract)? Walks outward through
/// nested `{ }` scopes — `if`/`for`/`match`/struct-literal bodies aren't
/// functions — until a scope's header text parses as a `fn` signature, or
/// there is no enclosing scope (top-level, e.g. a `$` initializer:
/// not fallible, since `?` propagation requires an enclosing fallible fn).
fn enclosing_fn_is_fallible(src: &str, at: usize) -> bool {
    let mut scan_from = at;
    while let Some(open) = innermost_open_brace(src, scan_from) {
        if let Some(sig_start) = fn_header_before(src, open) {
            return src[sig_start..open].contains('?');
        }
        scan_from = open;
    }
    false
}

/// Byte offset of the `{` that opens the innermost scope containing `before`
/// (naive brace balancing; the same string/comment fidelity as the rest of
/// this migration pass).
fn innermost_open_brace(src: &str, before: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut idx = before;
    while idx > 0 {
        idx -= 1;
        match bytes[idx] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    return Some(idx);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// If the `{` at `open` is a function body's opening brace, the byte offset
/// where its `fn` header starts; otherwise `None` (the scope belongs to an
/// `if`/`for`/`match`/struct-literal/other non-function block).
fn fn_header_before(src: &str, open: usize) -> Option<usize> {
    let prefix = &src[..open];
    let mut search_end = prefix.len();
    loop {
        let fn_at = prefix[..search_end].rfind("fn ")?;
        let boundary_ok = fn_at == 0 || {
            let b = prefix.as_bytes()[fn_at - 1];
            !(b.is_ascii_alphanumeric() || b == b'_')
        };
        if boundary_ok {
            let after_fn = &prefix[fn_at + "fn ".len()..];
            if let Some(paren) = after_fn.find('(') {
                if let Some(close_rel) = find_matching_paren(&after_fn[paren + 1..]) {
                    let close_abs = fn_at + "fn ".len() + paren + 1 + close_rel + 1;
                    let tail = prefix[close_abs..].trim();
                    // Only a return-type/effects clause between the params
                    // and `open` — no stray braces or statement separators —
                    // means this `fn` truly owns the `open` scope.
                    if !tail.contains(['{', '}', ';']) {
                        return Some(fn_at);
                    }
                }
            }
        }
        search_end = fn_at;
    }
}

fn find_matching_paren(after_open: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut in_string = false;
    let mut escape = false;
    for (index, ch) in after_open.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn replace_untyped_call(src: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(index) = rest.find(from) {
        out.push_str(&rest[..index]);
        let after = &rest[index + from.len()..];
        if after.starts_with('<') {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn edition_2027_encoding_audit(before: &str, after: &str) -> Vec<String> {
    let mut notes = Vec::new();
    if before.contains("json.canonical(") {
        if after.contains("?? panic(\"value is not canonical JSON\")")
            || after.contains("json.canonical(") && after != before
        {
            notes.push(
                "rewrote `json.canonical(...)` to fallible form; review hashing/signing fixtures"
                    .to_string(),
            );
        } else {
            notes.push(
                "review every `json.canonical(...)` call — edition 2027 requires fallible `json.canonical(data, limits)?` (or an explicit panic fallback)".to_string(),
            );
        }
    }
    if before != after && (before.contains("cbor.encode(") || before.contains("cbor.decode(")) {
        notes.push("rewrote deprecated CBOR forwarding calls (`encode`/`decode`)".to_string());
    }
    if notes.is_empty() {
        notes.push("no encoding forwarding calls found".to_string());
    }
    notes
}

/// D-DX-BROWSERTEST1=A: create one isolated native-BiDi test project with a
/// runnable first test and explicit cross-browser/server configuration.
pub(crate) fn scaffold_browser_tests(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err("browser test project name must be a simple folder name".to_string());
    }
    let project = root.join(name);
    if project.exists() {
        return Err(format!(
            "browser test project `{}` already exists",
            project.display()
        ));
    }
    let tests = project.join("tests");
    fs::create_dir_all(&tests)
        .map_err(|error| format!("couldn't create `{}`: {error}", tests.display()))?;
    fs::write(
        project.join("package.jet"),
        format!(
            "name: \"{name}_browser_tests\"\nversion: \"0.1.0\"\nedition: \"2026\"\ndescription: \"Native cross-browser tests\"\n"
        ),
    )
    .map_err(|error| format!("couldn't write browser test package: {error}"))?;
    fs::write(project.join("run.jet"), "#Target(Web)\n\nfn run() {}\n")
        .map_err(|error| format!("couldn't write browser app entry: {error}"))?;
    fs::write(
        project.join("browser-tests.conf"),
        "browsers=chromium,firefox,webkit\nserver=reuse\nwebServer=jet dev\nserver-url=http://127.0.0.1:8080\nreporter=json\n",
    )
    .map_err(|error| format!("couldn't write browser test configuration: {error}"))?;
    fs::write(
        tests.join("browser_suite.jet"),
        "use core.web.browser\n\n#Test fn first_page(page: BrowserPage) {\n  page.goto(\"/\")\n}\n",
    )
    .map_err(|error| format!("couldn't write first browser test: {error}"))?;
    Ok(project)
}

pub(crate) fn run_new(name: &str, annotated: bool, web: bool, mode: OutputMode) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        crate::cli_error!(@fix "E2104", "project name must be a simple folder name", format!("try: {} new my_app", jet::Syntax::BINARY_NAME));
        exit(ExitCodes::USER_ERROR);
    }
    let dir = Path::new(name);
    if dir.exists() {
        crate::cli_error!("E2104", "`{}` already exists", name);
        exit(ExitCodes::USER_ERROR);
    }
    // Create: <name>/package.jet, <name>/run.jet, the four optional command
    // homes, and <name>/.gitignore. The homes contain comments only, so a new
    // project keeps the stock command behavior until the owner opts in.
    let jet_dir = dir.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`/.jet: {}", name, e);
        exit(ExitCodes::USER_ERROR);
    });
    let mut manifest_text = jet::Manifest::new_template(name, annotated);
    if web {
        // The web starter uses `core.ui`, whose Browser effect must be visible
        // in the generated package authority before `jet dev` builds it.
        manifest_text = jet::Manifest::add_authority_hold(&manifest_text, "Browser");
    } else {
        // The native starter reads argv, so its package authority must carry
        // the same Exec effect as the generated source.
        manifest_text = jet::Manifest::add_authority_hold(&manifest_text, "Exec");
    }
    fs::write(dir.join(jet::Syntax::PACKAGE_FILE), manifest_text).unwrap_or_else(|e| {
        crate::cli_error!(
            "E2105",
            "couldn't write {}: {}",
            jet::Syntax::PACKAGE_FILE,
            e
        );
        exit(ExitCodes::USER_ERROR);
    });
    let running_version = env!("CARGO_PKG_VERSION");
    let channel = jetpack::JetPin::channel_of(running_version);
    if let Err(error) = jet::Lock::record_toolchain(
        dir,
        jetpack::JetPin::toolchain_record(&channel, running_version),
    ) {
        crate::cli_error!(
            @fix "E1206",
            format!("couldn't write {}", jet::Syntax::UNIFIED_LOCK_FILE),
            format!("check the project lock permissions: {error}")
        );
        exit(ExitCodes::USER_ERROR);
    }

    let run_src = if web {
        "// Start the live browser app: `jet dev`\n// Run the scaffold test: `jet test`\n// Build static browser files: `jet build --target web`\nuse core.ui as ui\nuse core.reactive as reactive\n#Target(Web)\n\nfn run() {\n    count :: reactive.signal(0)\n    ui.reactive_render(() -> {\n        n := count.get()\n        tree :: ui.box([\n            ui.node_color(\"Clicks: {n}\", 240.0, 40.0, \"#3366ff\"),\n            ui.button(\"Add one\", on_click: () -> {\n                count.set(count.get() + 1)\n            })\n        ])\n        backend :: ui.null_backend()\n        ui.mount(backend, tree, ui.constraint(0.0, 0.0, 320.0, 120.0))\n    })\n}\n\n#Test(\"the counter increments\") {\n    count :: reactive.signal(0)\n    count.set(count.get() + 1)\n    assert_eq(count.get(), 1)\n}\n"
    } else {
        "#CLI\nstruct GreetingArgs {\n    #Doc(\"name to greet\") name: String{\"world\"}\n}\n\nfn greeting(name: String) String -> \"hello, {name}\"\n\nfn run(args: GreetingArgs) { print(greeting(args.name)) }\n\n#Test(\"the greeting stays stable\") {\n    assert_eq(greeting(\"world\"), \"hello, world\")\n}\n"
    };
    fs::write(dir.join(jet::Syntax::DEFAULT_ENTRY_FILE), run_src).unwrap_or_else(|e| {
        crate::cli_error!(
            "E2105",
            "couldn't write {}: {}",
            jet::Syntax::DEFAULT_ENTRY_FILE,
            e
        );
        exit(ExitCodes::USER_ERROR);
    });
    let command_files = [
        (
            jet::Syntax::COMMAND_FILE_RUN,
            "// Optional `jet run` override. Uncomment one `fn run` to replace the stock default.\n// fn run() {\n//     print(\"run override\");\n// }\n// Inspect the stock behavior with: `jet run --show-default`\n",
        ),
        (
            jet::Syntax::COMMAND_FILE_BUILD,
            "// Optional `jet build` override. Uncomment one `fn build` to replace the stock default.\n// fn build(b: BuildContext) BuildPlan -> { return b.plan() }\n// Inspect the stock behavior with: `jet build --show-default`\n",
        ),
        (
            jet::Syntax::COMMAND_FILE_DEV,
            "// Optional `jet dev` override. Uncomment one `fn dev` to replace the stock default.\n// fn dev() {\n//     print(\"dev override\");\n// }\n// Inspect the stock behavior with: `jet dev --show-default`\n",
        ),
        (
            jet::Syntax::COMMAND_FILE_TEST,
            "// Optional `jet test` override. Uncomment one `fn test` to replace the stock default.\n// fn test(suite: TestSuite) { suite.run() }\n// Inspect the stock behavior with: `jet test --show-default`\n",
        ),
    ];
    for &(file, source) in &command_files {
        fs::write(dir.join(file), source).unwrap_or_else(|e| {
            crate::cli_error!("E2105", "couldn't write {}: {}", file, e);
            exit(ExitCodes::USER_ERROR);
        });
    }
    fs::write(
        dir.join(".gitignore"),
        "build/\n.jet-build/\n.jet/lock\n.jet/cache/\n",
    )
    .unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write .gitignore: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    // #1659 criterion 3: `--quiet` suppresses this confirmation; the project
    // itself was still created — only the non-error status narration mutes.
    if !mode.quiet {
        write_mode_status(mode, &format!("created {}/\n", name));
        write_mode_status(mode, &format!("  {}\n", jet::Syntax::PACKAGE_FILE));
        write_mode_status(mode, &format!("  {}\n", jet::Syntax::DEFAULT_ENTRY_FILE));
        for &(file, _) in &command_files {
            write_mode_status(mode, &format!("  {file}\n"));
        }
        write_mode_status(mode, "  .gitignore\n");
        let next = if web { "dev" } else { "run" };
        write_mode_status(
            mode,
            &format!(
                "next: cd {} && {} {}\n",
                name,
                jet::Syntax::BINARY_NAME,
                next
            ),
        );
    }
}

/// How much stdout/stderr a test run keeps visible. The default is intentionally
/// failure-only so a passing package remains quiet without hiding diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TestCapturePolicy {
    #[default]
    Failed,
    All,
    None,
}

impl TestCapturePolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::All => "all",
            Self::None => "none",
        }
    }
}

pub(crate) fn parse_test_capture(value: &str, _mode: OutputMode) -> TestCapturePolicy {
    match value.trim().to_ascii_lowercase().as_str() {
        "failed" | "failure" | "failures" => TestCapturePolicy::Failed,
        "all" => TestCapturePolicy::All,
        "none" | "off" => TestCapturePolicy::None,
        _ => {
            crate::cli_error!(
                "E2104",
                "invalid --capture value `{}` (expected failed, all, or none)",
                value
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// `--where` is deliberately a fact predicate, not another test-name
/// substring. Keep the grammar small until the recorded fact vocabulary grows:
/// conjunctions of exact `package`, `dep`, `path`, `tag`, and `status` values.
#[derive(Clone, Debug)]
struct TestWhereClause {
    field: String,
    value: String,
}

fn split_test_where_terms(expression: &str) -> Result<Vec<&str>, String> {
    let bytes = expression.as_bytes();
    let mut terms = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(mark) = quote {
            if byte == b'\\' {
                index = index.saturating_add(2);
                continue;
            }
            if byte == mark {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if index + 3 <= bytes.len()
            && (bytes[index] == b'a' || bytes[index] == b'A')
            && (bytes[index + 1] == b'n' || bytes[index + 1] == b'N')
            && (bytes[index + 2] == b'd' || bytes[index + 2] == b'D')
            && (index == 0 || bytes[index - 1].is_ascii_whitespace())
            && (index + 3 == bytes.len() || bytes[index + 3].is_ascii_whitespace())
        {
            let term = expression[start..index].trim();
            if term.is_empty() {
                return Err("empty predicate before `and`".to_string());
            }
            terms.push(term);
            index += 3;
            start = index;
            continue;
        }
        index += 1;
    }
    if quote.is_some() {
        return Err("unterminated quote in --where expression".to_string());
    }
    let term = expression[start..].trim();
    if term.is_empty() {
        return Err("empty predicate in --where expression".to_string());
    }
    terms.push(term);
    Ok(terms)
}

fn parse_test_where(expression: &str) -> Result<Vec<TestWhereClause>, String> {
    let mut clauses = Vec::new();
    for term in split_test_where_terms(expression.trim())? {
        let (field, raw_value) = term
            .split_once('=')
            .ok_or_else(|| format!("predicate `{term}` needs `field=value`"))?;
        let field = field.trim().to_ascii_lowercase();
        if !matches!(
            field.as_str(),
            "package" | "dep" | "dependency" | "path" | "tag" | "status"
        ) {
            return Err(format!(
                "unknown --where field `{field}` (expected package, dep, path, tag, or status)"
            ));
        }
        let raw_value = raw_value.trim();
        if raw_value.is_empty() {
            return Err(format!("empty value for --where field `{field}`"));
        }
        let value = if let Some(first) = raw_value.as_bytes().first().copied() {
            if first == b'\'' || first == b'"' {
                if raw_value.len() < 2 || raw_value.as_bytes().last().copied() != Some(first) {
                    return Err(format!("unterminated quote for --where field `{field}`"));
                }
                raw_value[1..raw_value.len() - 1].to_string()
            } else if raw_value.bytes().any(|byte| byte.is_ascii_whitespace()) {
                return Err(format!(
                    "unquoted --where value `{raw_value}` contains whitespace"
                ));
            } else {
                raw_value.to_string()
            }
        } else {
            return Err(format!("empty value for --where field `{field}`"));
        };
        clauses.push(TestWhereClause {
            field: if field == "dependency" {
                "dep".to_string()
            } else {
                field
            },
            value,
        });
    }
    Ok(clauses)
}

#[derive(Default)]
struct TestTargetFacts {
    package: String,
    deps: BTreeSet<String>,
    path: String,
    tags: BTreeSet<String>,
    status: Option<&'static str>,
}

fn source_hash_tags(source: &str) -> BTreeSet<String> {
    let mut tags = BTreeSet::new();
    for line in source.lines() {
        let marker = line.trim_start();
        let Some(marker) = marker.strip_prefix('#') else {
            continue;
        };
        let end = marker
            .char_indices()
            .find_map(|(index, ch)| (!ch.is_ascii_alphanumeric() && ch != '_').then_some(index))
            .unwrap_or(marker.len());
        if end != 0 {
            tags.insert(marker[..end].to_string());
        }
    }
    tags
}

fn test_target_facts(
    path: &Path,
    source: &str,
    opts: &TestRunOpts,
    package: bool,
) -> TestTargetFacts {
    let shown = path.to_string_lossy().into_owned();
    let mut facts = TestTargetFacts {
        package: path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("package")
            .to_string(),
        path: shown,
        tags: source_hash_tags(source),
        ..TestTargetFacts::default()
    };
    if let Ok(Some(package_facts)) = jet::Loader::package_facts_for_entry(path) {
        facts.package = package_facts.name.clone();
        facts.deps.extend(package_facts.deps.keys().cloned());
    }
    let entry = path.to_string_lossy();
    let (bundle, _) = jet::Loader::load_entry_with_overlays_and_dependencies(&entry, &[], false);
    if let Ok(bundle) = bundle {
        facts
            .deps
            .extend(bundle.package_guarantees.dependency_names.iter().cloned());
        facts.deps.extend(bundle.dep_roots.keys().cloned());
        facts
            .deps
            .extend(bundle.dep_roots.values().filter_map(|root| {
                root.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            }));
    }
    if !opts.fresh {
        if let Some(cached) = test_result_cache_read(path, opts, package) {
            facts.status = Some(if cached.ok { "pass" } else { "fail" });
        }
    }
    facts
}

fn test_where_matches(expression: &str, facts: &TestTargetFacts) -> bool {
    let Ok(clauses) = parse_test_where(expression) else {
        return false;
    };
    clauses.iter().all(|clause| match clause.field.as_str() {
        "package" => facts.package == clause.value,
        "dep" => facts.deps.iter().any(|dep| {
            dep == &clause.value
                || dep
                    .strip_prefix(&clause.value)
                    .is_some_and(|tail| tail.starts_with('.'))
        }),
        "path" => {
            facts.path == clause.value
                || facts.path.ends_with(&format!("/{}", clause.value))
                || facts.path.contains(&clause.value)
        }
        "tag" => facts.tags.contains(&clause.value),
        "status" => {
            let lowered = clause.value.to_ascii_lowercase();
            let wanted = match lowered.as_str() {
                "passed" | "success" | "ok" => "pass",
                "failed" | "failure" | "error" => "fail",
                value => value,
            };
            facts.status == Some(wanted)
        }
        _ => false,
    })
}

#[derive(Clone, Debug)]
struct TestResultCache {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn append_test_cache_field(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

fn test_result_cache_key(path: &Path, opts: &TestRunOpts, package: bool) -> Option<String> {
    let mut identity = Vec::new();
    append_test_cache_field(&mut identity, b"jet-test-result-v2");
    append_test_cache_field(
        &mut identity,
        fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .as_bytes(),
    );
    append_test_cache_field(&mut identity, if package { b"package" } else { b"file" });
    let entry = path.to_string_lossy();
    let (bundle, dependencies) =
        jet::Loader::load_entry_with_overlays_and_dependencies(&entry, &[], false);
    let mut inputs = BTreeSet::new();
    if let Some(root) = path.parent().and_then(jet::Loader::find_manifest_root) {
        inputs.insert(root.join(jet::Syntax::PACKAGE_FILE));
    }
    inputs.insert(path.to_path_buf());
    inputs.extend(dependencies);
    if let Ok(bundle) = bundle {
        for input in &bundle.comptime_inputs {
            append_test_cache_field(&mut identity, b"comptime-input");
            append_test_cache_field(&mut identity, input.path.as_bytes());
            append_test_cache_field(&mut identity, input.hash.as_bytes());
        }
        inputs.extend(bundle.modules.into_iter().map(|module| module.path));
    }
    for input in inputs {
        append_test_cache_field(&mut identity, input.to_string_lossy().as_bytes());
        match fs::read(&input) {
            Ok(contents) => append_test_cache_field(&mut identity, &contents),
            Err(_) => append_test_cache_field(&mut identity, b"<missing>"),
        }
    }

    // A test result is executable output too. Reuse the native key's complete
    // identity so compile-time inputs, compiler/toolchain, runtime, and corelib
    // changes cannot serve an older result. `None` is the safe no-cache answer
    // for an uncacheable or invalid program (for example, embed_file).
    let profile = opts
        .profile
        .as_deref()
        .unwrap_or(if opts.release || opts.measure {
            jet::Syntax::BUILD_PROFILE_RELEASE
        } else {
            "dev"
        });
    let profile_tag = format!(
        "{profile};{};aot-target=native",
        setting_overrides_tag(&opts.setting_overrides)
    );
    let mode_tag = if opts.coverage { "testcov" } else { "test" };
    let native_identity = native_cache_key(entry.as_ref(), profile, &profile_tag, mode_tag, None)?;
    append_test_cache_field(&mut identity, b"native-identity");
    append_test_cache_field(&mut identity, native_identity.as_bytes());

    let options = format!(
        "show_default={};coverage={};release={};profile={:?};trace_tiers={};filter={:?};shuffle={:?};serial={};measure={};docs={};browser={:?};browser_retries={:?};browser_reporter={:?};browser_ui={};browser_visual={};browser_trace={};",
        opts.show_default,
        opts.coverage,
        opts.release,
        opts.profile,
        opts.trace_tiers,
        opts.filter,
        opts.shuffle_seed,
        opts.serial,
        opts.measure,
        opts.docs,
        opts.browser_engines,
        opts.browser_retries,
        opts.browser_reporter,
        opts.browser_ui,
        opts.browser_visual,
        opts.browser_trace,
    );
    append_test_cache_field(&mut identity, options.as_bytes());
    Some(jet::SHA256::sha256_hex(&identity))
}

fn test_result_cache_path(path: &Path, opts: &TestRunOpts, package: bool) -> Option<PathBuf> {
    let root = path
        .parent()
        .and_then(jet::Loader::find_manifest_root)
        .unwrap_or_else(|| {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        });
    Some(root.join(".jet").join("test-results").join(format!(
        "{}.cache",
        test_result_cache_key(path, opts, package)?
    )))
}

fn read_test_cache_line<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let start = *cursor;
    let end = bytes.get(start..)?.iter().position(|byte| *byte == b'\n')? + start;
    *cursor = end + 1;
    Some(&bytes[start..end])
}

fn test_result_cache_read(
    path: &Path,
    opts: &TestRunOpts,
    package: bool,
) -> Option<TestResultCache> {
    let bytes = fs::read(test_result_cache_path(path, opts, package)?).ok()?;
    let mut cursor = 0usize;
    if read_test_cache_line(&bytes, &mut cursor)? != b"JET_TEST_RESULT_V2" {
        return None;
    }
    let ok = read_test_cache_line(&bytes, &mut cursor)? == b"1";
    let stdout_len = std::str::from_utf8(read_test_cache_line(&bytes, &mut cursor)?)
        .ok()?
        .parse::<usize>()
        .ok()?;
    let stderr_len = std::str::from_utf8(read_test_cache_line(&bytes, &mut cursor)?)
        .ok()?
        .parse::<usize>()
        .ok()?;
    let stdout_end = cursor.checked_add(stdout_len)?;
    let stderr_end = stdout_end.checked_add(stderr_len)?;
    let stdout = String::from_utf8(bytes.get(cursor..stdout_end)?.to_vec()).ok()?;
    let stderr = String::from_utf8(bytes.get(stdout_end..stderr_end)?.to_vec()).ok()?;
    Some(TestResultCache { ok, stdout, stderr })
}

fn test_result_cache_write(
    path: &Path,
    opts: &TestRunOpts,
    package: bool,
    result: &TestResultCache,
) {
    let Some(destination) = test_result_cache_path(path, opts, package) else {
        return;
    };
    if let Some(parent) = destination.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut bytes = b"JET_TEST_RESULT_V2\n".to_vec();
    bytes.extend_from_slice(if result.ok { b"1\n" } else { b"0\n" });
    bytes.extend_from_slice(result.stdout.len().to_string().as_bytes());
    bytes.extend_from_slice(result.stderr.len().to_string().as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(result.stdout.as_bytes());
    bytes.extend_from_slice(result.stderr.as_bytes());
    let _ = fs::write(destination, bytes);
}

fn test_result_cache_write_allowed(opts: &TestRunOpts) -> bool {
    !opts.docs && !opts.coverage && !opts.measure && !opts.update_snapshots && opts.record.is_none()
}
fn emit_test_capture(result: &TestResultCache, policy: TestCapturePolicy, mode: OutputMode) {
    if mode.json {
        if let Some(document) =
            canonical_test_json(&result.stdout).or_else(|| canonical_status_json(&result.stdout))
        {
            write_mode_machine(mode, &format!("{document}\n"));
        } else {
            // Machine stdout is reserved for the canonical report. Preserve
            // an unexpected child stream as a diagnostic rather than corrupting
            // the JSON channel with human output.
            if !result.stdout.is_empty() {
                eprint!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
        }
        return;
    }
    let show = matches!(policy, TestCapturePolicy::All)
        || (!result.ok && matches!(policy, TestCapturePolicy::Failed));
    if !show {
        return;
    }
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }
}
#[derive(Clone, Debug)]
struct HarnessTestCase {
    name: String,
    ok: bool,
    expected_failure: bool,
    skipped: bool,
    message: String,
}

fn host_target_triple() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

fn with_test_evidence_environment<R>(
    report_path: &Path,
    report_id: &str,
    toolchain: &str,
    target: &str,
    profile: &str,
    source_revision: &str,
    build_revision: &str,
    revision: &str,
    operation: impl FnOnce() -> R,
) -> R {
    let values = [
        (
            jet_foundation::Evidence::EVIDENCE_REPORT_ENV,
            report_path.to_string_lossy().into_owned(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_REPORT_ID_ENV,
            report_id.to_string(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_TOOLCHAIN_ENV,
            toolchain.to_string(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_TARGET_ENV,
            target.to_string(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_PROFILE_ENV,
            profile.to_string(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_SOURCE_REVISION_ENV,
            source_revision.to_string(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_BUILD_REVISION_ENV,
            build_revision.to_string(),
        ),
        (
            jet_foundation::Evidence::EVIDENCE_REVISION_ENV,
            revision.to_string(),
        ),
    ];
    let previous = values
        .iter()
        .map(|(name, _)| (*name, std::env::var_os(name)))
        .collect::<Vec<_>>();
    for (name, value) in &values {
        std::env::set_var(name, value);
    }
    let result = operation();
    for (name, value) in previous {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }
    result
}

fn harness_test_cases(stdout: &str) -> Result<Option<Vec<HarnessTestCase>>, String> {
    if let Some(document) = canonical_test_json(stdout) {
        let value = parse_json(&document)
            .map_err(|_| "the test harness emitted an invalid canonical JSON report".to_string())?;
        let tests = value
            .get("tests")
            .map_err(|error| format!("the test harness report is missing `tests`: {error}"))?
            .as_array()
            .map_err(|error| format!("the test harness report has invalid `tests`: {error}"))?;
        let mut cases = Vec::with_capacity(tests.len());
        for test in tests {
            let name = test
                .get("name")
                .and_then(|value| value.as_str())
                .map_err(|error| format!("the test harness report has an invalid name: {error}"))?
                .to_string();
            let ok = match test
                .get("ok")
                .map_err(|error| format!("test `{name}` has no result: {error}"))?
            {
                DataTree::Bool(ok) => *ok,
                _ => return Err(format!("test `{name}` has a non-boolean result")),
            };
            let expected_failure = match test.get_opt("expectedFailure") {
                Some(DataTree::Bool(expected_failure)) => *expected_failure,
                None => false,
                _ => return Err(format!("test `{name}` has a non-boolean expectation")),
            };
            let skipped = match test.get_opt("skipped") {
                Some(DataTree::Bool(skipped)) => *skipped,
                None => false,
                _ => return Err(format!("test `{name}` has a non-boolean skip flag")),
            };
            let stderr = test
                .get_opt("stderr")
                .and_then(|value| value.as_str().ok())
                .unwrap_or("");
            let message = if stderr.is_empty() && !ok {
                "test failed".to_string()
            } else {
                stderr.to_string()
            };
            cases.push(HarnessTestCase {
                name,
                ok,
                expected_failure,
                skipped,
                message,
            });
        }
        return Ok(Some(cases));
    }
    let cases = stdout
        .lines()
        .filter_map(|line| {
            let (name, status) = line.rsplit_once(": ")?;
            let (ok, expected_failure, skipped) = match status {
                "pass" => (true, false, false),
                "expected-fail" => (true, true, false),
                "skip" => (true, false, true),
                "FAIL" => (false, false, false),
                "UNEXPECTED-PASS (remove expected_fail: true)" => (false, true, false),
                _ => return None,
            };
            Some(HarnessTestCase {
                name: name.trim().to_string(),
                ok,
                expected_failure,
                skipped,
                message: if ok { String::new() } else { status.to_string() },
            })
        })
        .filter(|case| !case.name.is_empty())
        .collect::<Vec<_>>();
    if cases.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cases))
    }
}

fn append_harness_test_evidence(
    report: &mut jet_foundation::Evidence::EvidenceReport,
    stdout: &str,
    file: &str,
    report_id: &str,
    build: &jet_foundation::Evidence::EvidenceBuild,
    revision: &jet_foundation::Evidence::EvidenceRevision,
    child_ok: bool,
) -> Result<(), String> {
    let cases = harness_test_cases(stdout)?;
    let Some(cases) = cases else {
        if child_ok {
            return Ok(());
        }
        let name = format!("{file}:harness");
        let mut record = jet_foundation::Evidence::EvidenceRecord::from_test_codes(
            0,
            3,
            &name,
            "producer unavailable",
            file,
            0,
        )?
        .with_report_id(report_id);
        record.build = build.clone();
        record.revision = revision.clone();
        let derivation = record.checked_derivation();
        report
            .add_record_with_derivation(record, derivation)
            .map_err(|error| error.to_string())?;
        return Ok(());
    };
    for case in cases {
        if report
            .records
            .iter()
            .any(|record| record.identity.claim_id == case.name)
        {
            continue;
        }
        let mut record = jet_foundation::Evidence::EvidenceRecord::from_test_codes(
            0,
            if case.skipped {
                2
            } else if case.ok != case.expected_failure {
                0
            } else {
                1
            },
            &case.name,
            &case.message,
            file,
            0,
        )?
        .with_report_id(report_id);
        if case.expected_failure && !case.skipped {
            record
                .set_expectation(
                    jet_foundation::Evidence::EvidenceExpectation::ExpectedFailure,
                )
                .map_err(|error| error.to_string())?;
        }
        record.build = build.clone();
        record.revision = revision.clone();
        let derivation = record.checked_derivation();
        report
            .add_record_with_derivation(record, derivation)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_or_create_test_evidence_report(
    path: &Path,
    report_id: &str,
    file: &str,
    toolchain: &str,
    target: &str,
    profile: &str,
    source_revision: &str,
    build_revision: &str,
    revision: &str,
) -> Result<jet_foundation::Evidence::EvidenceReport, String> {
    match jet_foundation::Evidence::EvidenceReport::read(path) {
        Ok(report) => Ok(report),
        Err(_error) if !path.exists() => Ok(jet_foundation::Evidence::EvidenceReport::new(
            report_id,
            jet_foundation::Evidence::EvidenceProducerKind::Test,
            jet_foundation::Evidence::EvidenceSource::new(file, 0, 1),
            jet_foundation::Evidence::EvidenceBuild::new(toolchain, target, profile),
            jet_foundation::Evidence::EvidenceRevision::new(
                source_revision,
                build_revision,
                revision,
            ),
        )),
        Err(error) => Err(format!(
            "could not read test evidence report `{}`: {error}",
            path.display()
        )),
    }
}

fn write_test_evidence_bytes(path: &Path, bytes: &[u8]) -> Result<u64, String> {
    let Some(parent) = path.parent() else {
        return Err("test evidence report has no parent directory".to_string());
    };
    let mut current = PathBuf::from(".");
    for component in parent.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "test evidence parent is a symlink: {}",
                    current.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "test evidence parent is not a directory: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| error.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
                        .map_err(|error| error.to_string())?;
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "final test evidence path is not a regular file: {}",
                path.display()
            ));
        }
        let existing = fs::read(path).map_err(|error| error.to_string())?;
        if existing == bytes {
            return u64::try_from(existing.len())
                .map_err(|_| "test evidence report is too large".to_string());
        }
        return Err(format!(
            "refusing to overwrite differing test evidence report at {}",
            path.display()
        ));
    }
    let temporary = path.with_extension(format!(
        "json.tmp.{}.{}",
        std::process::id(),
        jet::SHA256::sha256_hex(bytes)
            .get(..8)
            .unwrap_or("00000000")
    ));
    let write_result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
        }
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::hard_link(&temporary, path).map_err(|error| error.to_string())?;
        fs::remove_file(&temporary).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        fs::File::open(parent)
            .map_err(|error| error.to_string())?
            .sync_all()
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    u64::try_from(bytes.len()).map_err(|_| "test evidence report is too large".to_string())
}

pub(crate) fn persist_evidence_report(
    report: &jet_foundation::Evidence::EvidenceReport,
) -> Result<(), String> {
    persist_evidence_report_for_inputs(report, &report.revision.source)
}

pub(crate) fn persist_evidence_report_for_inputs(
    report: &jet_foundation::Evidence::EvidenceReport,
    target_inputs_sha256: &str,
) -> Result<(), String> {
    let report_id = report.identity.report_id.as_str();
    if report_id.is_empty()
        || report_id.contains('/')
        || report_id.contains('\\')
        || report_id == "."
        || report_id == ".."
    {
        return Err(format!("evidence report has an unsafe id `{report_id}`"));
    }
    let path = PathBuf::from(".jet/evidence").join(format!("{report_id}.json"));
    let bytes = report.encode()?;
    let size = write_test_evidence_bytes(&path, &bytes)?;
    let identity = RecordIdentity::new(
        target_inputs_sha256.to_string(),
        report.build.toolchain.clone(),
        report.producer.as_str(),
    )?;
    index_compile_artifact(
        identity,
        RecordKind::Evidence,
        report_id.to_string(),
        path,
        size,
        RecordCapture::Safe,
        Vec::new(),
        Vec::new(),
    )?;
    Ok(())
}

fn finish_test_evidence(
    report_path: &Path,
    report_id: &str,
    file: &str,
    toolchain: &str,
    target: &str,
    profile: &str,
    source_revision: &str,
    build_revision: &str,
    revision: &str,
    harness_stdout: Option<&str>,
    child_ok: bool,
    preserve_report: bool,
    mode: OutputMode,
) -> Result<Option<bool>, String> {
    let mut report = read_or_create_test_evidence_report(
        report_path,
        report_id,
        file,
        toolchain,
        target,
        profile,
        source_revision,
        build_revision,
        revision,
    )?;
    let build = jet_foundation::Evidence::EvidenceBuild::new(toolchain, target, profile);
    let evidence_revision =
        jet_foundation::Evidence::EvidenceRevision::new(source_revision, build_revision, revision);
    if let Some(stdout) = harness_stdout {
        append_harness_test_evidence(
            &mut report,
            stdout,
            file,
            report_id,
            &build,
            &evidence_revision,
            child_ok,
        )?;
    }
    if report.records.is_empty() {
        if !preserve_report {
            let _ = fs::remove_file(report_path);
        }
        return Ok(None);
    }
    persist_evidence_report(&report)?;
    let projection = jet::Package::ClaimsProjection::from_records(&report.records);
    let floor = jet::Loader::package_facts_for_entry(Path::new(file))
        .ok()
        .flatten()
        .and_then(|facts| facts.policy.claims_min);
    let floor_met = projection.floor_met(floor);
    let grade = projection.grade.render();
    let claims = StatusValue::object(
        StatusFields::new()
            .with("grade", grade.clone())
            .with(
                "floor",
                floor.map_or(StatusValue::Null, |floor| {
                    StatusValue::String(floor.render())
                }),
            )
            .with("floorMet", floor_met)
            .with("generatedAttempts", projection.generated_attempts)
            .with("generated", projection.generated_successes)
            .with("examples", projection.examples_successes),
    );
    let summaries = StatusValue::object(StatusFields::new().with(
        "claims",
        StatusValue::object(StatusFields::new().with("grade", grade)),
    ));
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut expected_failures = 0usize;
    let mut unexpected_passes = 0usize;
    for record in &report.records {
        if !matches!(
            record.kind,
            jet_foundation::Evidence::EvidenceKind::Unit
                | jet_foundation::Evidence::EvidenceKind::Property
                | jet_foundation::Evidence::EvidenceKind::Doctest
        ) {
            continue;
        }
        if record.is_unexpected_pass() {
            unexpected_passes += 1;
            continue;
        }
        if record.is_expected_failure() {
            expected_failures += 1;
            continue;
        }
        match record.outcome {
            jet_foundation::Evidence::EvidenceOutcome::Passed => passed += 1,
            jet_foundation::Evidence::EvidenceOutcome::Failed
            | jet_foundation::Evidence::EvidenceOutcome::Error => failed += 1,
            jet_foundation::Evidence::EvidenceOutcome::Skipped => skipped += 1,
            _ => {}
        }
    }
    let test = StatusValue::object(
        StatusFields::new()
            .with("failed", failed)
            .with("passed", passed)
            .with("skipped", skipped)
            .with(
                "selected",
                passed + failed + skipped + expected_failures + unexpected_passes,
            )
            .with("expectedFailures", expected_failures)
            .with("unexpectedPasses", unexpected_passes),
    );
    let evidence = StatusValue::parse(&report.json()).map_err(|error| error.to_string())?;
    let mut status = StatusEnvelope::new("test", failed == 0 && unexpected_passes == 0)
        .with_field("test", test)
        .with_field("evidence", evidence)
        .with_field("grade", projection.grade.render())
        .with_field("claims", claims)
        .with_field("summaries", summaries);
    status.ok = status.ok && child_ok && floor_met;
    if mode.json {
        write_mode_machine(mode, &format!("{}\n", status.json()));
    }
    let ok = status.ok;
    if !preserve_report {
        let _ = fs::remove_file(report_path);
    }
    Ok(Some(ok))
}

fn canonical_test_json(stdout: &str) -> Option<String> {
    let whole = stdout.trim();
    if is_canonical_test_report(whole) {
        return Some(whole.to_string());
    }
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| is_canonical_test_report(line))
        .map(str::to_string)
}

fn object_field<'a>(object: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn is_canonical_test_report(candidate: &str) -> bool {
    let Ok(DataTree::Object(object)) = parse_json(candidate) else {
        return false;
    };
    let schema = object_field(&object, "schema").and_then(|value| value.as_str().ok());
    matches!(schema, Some("jet.test.v1" | "jet-test-v1"))
        && matches!(object_field(&object, "tests"), Some(DataTree::Array(_)))
}
fn canonical_status_json(stdout: &str) -> Option<String> {
    let whole = stdout.trim();
    if is_canonical_status_report(whole) {
        return Some(whole.to_string());
    }
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| is_canonical_status_report(line))
        .map(str::to_string)
}

fn is_canonical_status_report(candidate: &str) -> bool {
    let Ok(DataTree::Object(object)) = parse_json(candidate) else {
        return false;
    };
    matches!(
        object_field(&object, "schema").and_then(|value| value.as_str().ok()),
        Some("jet.status/v1")
    )
}
fn emit_doctest_result(
    label: &str,
    ok: bool,
    stdout: &str,
    stderr: &str,
    policy: TestCapturePolicy,
    mode: OutputMode,
) {
    if mode.json {
        return;
    }
    let show = matches!(policy, TestCapturePolicy::All)
        || (!ok && matches!(policy, TestCapturePolicy::Failed));
    if !show {
        return;
    }
    write_mode_status(
        mode,
        &format!("{}: {}\n", label, if ok { "pass" } else { "FAIL" }),
    );
    if !stdout.is_empty() {
        print!("{}", stdout);
    }
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }
}

/// `jet test` flags beyond the file/dir target (D-TESTKIT1=A gaps #2-#4).
/// Grouped so new flags don't keep growing every `run_test*` signature.
#[derive(Clone, Default)]
pub(crate) struct TestRunOpts {
    /// `--show-default` forces the stock harness when the entry defines `fn test`.
    pub(crate) show_default: bool,
    /// `--watch` keeps the test command alive and retains its failed cursor.
    pub(crate) watch: bool,
    /// `--fresh` bypasses only the persistent test-result cache.
    pub(crate) fresh: bool,
    /// `--docs` selects checked documentation examples instead of `#Test` blocks.
    pub(crate) docs: bool,
    /// Structured package/dependency/path/tag/status fact predicate.
    pub(crate) where_expr: Option<String>,
    /// Captured child output policy. `failed` is the quiet default.
    pub(crate) capture: TestCapturePolicy,
    pub(crate) update_snapshots: bool,
    pub(crate) coverage: bool,
    /// `--release`: build the test harness with the release AOT profile.
    pub(crate) release: bool,
    /// `--profile=<name>`: build the test harness with the selected profile.
    pub(crate) profile: Option<String>,
    /// CLI build-setting contributions shared with the checked test target.
    pub(crate) setting_overrides: BTreeMap<String, String>,
    /// `--trace-tiers`: print the harness execution tier marker.
    pub(crate) trace_tiers: bool,
    /// `--filter=<substr>`: only run tests whose name contains it.
    pub(crate) filter: Option<String>,
    /// `--shuffle` / `--shuffle=<seed>`: reorder tests before running (order-
    /// dependence detection). `None` = source order (the default).
    pub(crate) shuffle_seed: Option<u64>,
    /// `--serial`: run one test at a time instead of the parallel default.
    pub(crate) serial: bool,
    /// `--measure`: run only `.measure` claims through the measurement harness.
    pub(crate) measure: bool,
    /// `--record=NAME`: write the shared safe replay envelope for this target.
    pub(crate) record: Option<String>,
    /// `--browser=<engines>` selects the locked native BiDi engines.
    pub(crate) browser_engines: Option<String>,
    /// `--browser-retries=<n>` bounds fresh-context retries per engine.
    pub(crate) browser_retries: Option<i64>,
    /// `--browser-reporter=<text|json|html>` selects the deterministic report.
    pub(crate) browser_reporter: Option<String>,
    /// `--browser-ui` opens the local report viewer when the report is written.
    pub(crate) browser_ui: bool,
    /// `--browser-visual` captures screenshots for passing attempts too.
    pub(crate) browser_visual: bool,
    /// `--browser-trace` records the redacted BiDi trace for every attempt.
    pub(crate) browser_trace: bool,
}
impl TestRunOpts {
    /// Parse the complete `jet test` argv once for both the bare-package and
    /// explicit-target routes. The first positional after `test` is the
    /// target; the optional second positional is the promised name filter.
    pub(crate) fn parse(
        argv: &[String],
        mode: OutputMode,
        setting_overrides: &BTreeMap<String, String>,
    ) -> Self {
        let mut opts = Self::default();
        opts.setting_overrides = setting_overrides.clone();
        let mut positionals = Vec::new();
        let mut index = 0usize;
        while index < argv.len() {
            let arg = argv[index].as_str();
            if index == 0 && arg == "test" {
                index += 1;
                continue;
            }
            if arg == "--" {
                break;
            }
            let (name, inline) = arg
                .split_once('=')
                .map_or((arg, None), |(name, value)| (name, Some(value)));
            match name {
                "--show-default" => opts.show_default = true,
                "--watch" => opts.watch = !matches!(inline, Some("off" | "false" | "0")),
                "--fresh" => opts.fresh = true,
                "--docs" => opts.docs = true,
                "--update-snapshots" | "-u" => opts.update_snapshots = true,
                "--coverage" => opts.coverage = true,
                "--release" => opts.release = true,
                "--trace-tiers" => opts.trace_tiers = true,
                "--serial" => opts.serial = true,
                "--measure" => opts.measure = true,
                "--browser-ui" => opts.browser_ui = !matches!(inline, Some("off" | "false" | "0")),
                "--browser-visual" => {
                    opts.browser_visual = !matches!(inline, Some("off" | "false" | "0"))
                }
                "--browser-trace" => {
                    opts.browser_trace = !matches!(inline, Some("off" | "false" | "0"))
                }
                "--where" => {
                    opts.where_expr = Some(test_run_option_value(argv, &mut index, name, inline));
                }
                "--capture" => {
                    let value = test_run_option_value(argv, &mut index, name, inline);
                    opts.capture = parse_test_capture(&value, mode);
                }
                "--filter" => {
                    let value = test_run_option_value(argv, &mut index, name, inline);
                    merge_test_filter(&mut opts.filter, value);
                }
                "--profile" => {
                    opts.profile = Some(test_run_option_value(argv, &mut index, name, inline));
                }
                "--set" => {
                    if inline.is_none() {
                        let _ = test_run_option_value(argv, &mut index, name, inline);
                    }
                }
                "--shuffle" => {
                    opts.shuffle_seed = Some(match inline {
                        Some(value) => value.parse::<u64>().unwrap_or_else(|_| {
                            invalid_test_run_option(format!("`--shuffle={value}` isn't a number"))
                        }),
                        None => test_shuffle_seed(),
                    });
                }
                "--record" => {
                    if inline.is_none() {
                        invalid_test_run_option(
                            "`--record` needs a closed `=NAME` value".to_string(),
                        );
                    }
                    let flag = format!("--record={}", inline.unwrap_or_default());
                    match crate::ProveReplay::parse_record_flag(&flag) {
                        Some(Ok(value)) => opts.record = Some(value),
                        Some(Err(error)) => invalid_test_run_option(error),
                        None => invalid_test_run_option("invalid --record flag".to_string()),
                    }
                }
                "--browser" => {
                    opts.browser_engines =
                        Some(test_run_option_value(argv, &mut index, name, inline));
                }
                "--browser-retries" => {
                    let value = test_run_option_value(argv, &mut index, name, inline);
                    opts.browser_retries = Some(
                        value
                            .parse::<i64>()
                            .ok()
                            .filter(|n| (0..=5).contains(n))
                            .unwrap_or_else(|| {
                                invalid_test_run_option(format!(
                                    "`--browser-retries={value}` must be between 0 and 5"
                                ))
                            }),
                    );
                }
                "--browser-reporter" => {
                    let value = test_run_option_value(argv, &mut index, name, inline);
                    if !matches!(value.as_str(), "text" | "json" | "html") {
                        invalid_test_run_option(format!(
                            "`--browser-reporter={value}` is not supported"
                        ));
                    }
                    opts.browser_reporter = Some(value);
                }
                // Output flags are consumed by `OutputAdapterHost`; they are
                // not test execution inputs and must not become positionals.
                "--json" | "--machine" | "--quiet" | "--verbose" | "--no-color" => {}
                "--color" => {
                    if inline.is_none() {
                        let _ = test_run_option_value(argv, &mut index, name, inline);
                    }
                }
                _ if arg.starts_with('-') => {}
                _ => positionals.push(arg.to_string()),
            }
            index += 1;
        }
        if positionals.len() > 2 {
            invalid_test_run_option(format!(
                "`jet test` accepts one target and one positional filter, got {} positionals",
                positionals.len()
            ));
        }
        if let Some(filter) = positionals.get(1) {
            merge_test_filter(&mut opts.filter, filter.clone());
        }
        // `--release` is sugar for the named release profile and therefore
        // wins deterministically over a simultaneous `--profile` spelling.
        if opts.release {
            opts.profile = Some(jet::Syntax::BUILD_PROFILE_RELEASE.to_string());
        }
        opts
    }
}

fn test_run_option_value(
    argv: &[String],
    index: &mut usize,
    name: &str,
    inline: Option<&str>,
) -> String {
    if let Some(value) = inline {
        return value.to_string();
    }
    let Some(value) = argv.get(*index + 1) else {
        invalid_test_run_option(format!("`{name}` needs a value"));
    };
    *index += 1;
    value.clone()
}

fn merge_test_filter(filter: &mut Option<String>, value: String) {
    if let Some(previous) = filter.as_deref() {
        if previous != value.as_str() {
            invalid_test_run_option(format!(
                "conflicting test filters `{previous}` and `{value}`"
            ));
        }
    } else {
        *filter = Some(value);
    }
}

fn test_shuffle_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default()
}

fn invalid_test_run_option(message: String) -> ! {
    crate::cli_error!(
        @fix "E2104",
        message,
        "use the canonical `jet test` flag spelling"
    );
    exit(ExitCodes::USAGE);
}

/// `jet test [--watch] [--fresh] [--docs] [--where=<expr>]
/// [--capture=<failed|all|none>] [--release] [--trace-tiers] [--coverage]
/// [--filter=<substr>] [--shuffle[=<seed>]] [--serial]`.
/// A finite run is quiet for passing tests and stores only test results in the
/// result cache; native build artifacts remain in the ordinary build cache.
/// `--watch` keeps the failed cursor and reuses that build graph.
pub(crate) fn run_test_opts(path: &str, opts: TestRunOpts, mode: OutputMode) {
    let p = Path::new(path);
    require_project_environment("test", p, mode);
    if !p.exists() {
        crate::cli_error!("E2105", "can't find `{}`", path);
        exit(ExitCodes::USER_ERROR);
    }
    if let Some(expression) = opts.where_expr.as_deref() {
        if let Err(error) = parse_test_where(expression) {
            crate::cli_error!("E2104", "invalid --where expression: {}", error);
            exit(ExitCodes::USER_ERROR);
        }
    }
    if opts.watch {
        run_test_watch(path, opts, mode);
        return;
    }
    if opts.fresh && !mode.quiet {
        write_mode_status(
            mode,
            "jet test: fresh result run (test-result cache bypassed; build cache reusable)\n",
        );
    }
    if p.is_dir() {
        let root = jet::Loader::find_manifest_root(p).unwrap_or_else(|| p.to_path_buf());
        if p.join(jet::Syntax::PACKAGE_FILE).is_file() {
            run_test_package(&root, opts, mode);
            return;
        }
        if !opts.show_default {
            if let Some(override_file) =
                crate::resolve_package_command_override(&root, "test", mode)
            {
                let ok = matches!(
                    run_test_target(&override_file, &opts, mode, false),
                    TestTargetOutcome::Ran(true)
                        | TestTargetOutcome::Override(true)
                        | TestTargetOutcome::NoTests
                );
                exit(if ok {
                    ExitCodes::OK
                } else {
                    ExitCodes::USER_ERROR
                });
            }
        }
        let ext = jet::Syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = Vec::new();
        collect_source_files_recursive(p, ext, &mut files);
        files.sort();
        if files.is_empty() {
            crate::cli_error!(
                "E2104",
                "no .{} files in `{}` (searched subdirectories too)",
                ext,
                path
            );
            exit(ExitCodes::USER_ERROR);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f, &opts, mode) {
                any_fail = true;
            }
        }
        exit(if any_fail {
            ExitCodes::USER_ERROR
        } else {
            ExitCodes::OK
        });
    }
    exit(if run_test_file(p, &opts, mode) {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    });
}

/// Bare `jet test` inside a package (spec S43): every source file the package
/// compiles is a test target, not only the resolved entry file. The member set
/// comes from the driver's project scan — the same authority-checked
/// enumeration `jet project parts` reports — so discovery covers exactly what
/// the package compiles instead of walking the filesystem a second time.
/// Members with neither `#Test` blocks nor doctests are skipped instead of
/// failing; a member that fails to parse still reports its real compile error.
/// When nothing in the package was testable, the resolved entry runs on the
/// ordinary file path so its own report (E0601, or E2105 for a missing entry)
/// is the one the user sees, exactly once.
pub(crate) fn run_test_package(root: &Path, opts: TestRunOpts, mode: OutputMode) {
    require_project_environment("test", root, mode);
    let entry = crate::find_project_entry(root);
    let mut ran = 0usize;
    let mut any_fail = false;
    // D-CMDOVERRIDE1=A: one package-scoped `fn test` owns the whole command.
    // The package authority reports duplicate command functions before this
    // branch, and the selected file runs in its own file scope.
    if !opts.show_default {
        if let Some(override_file) = crate::resolve_package_command_override(root, "test", mode) {
            let ok = matches!(
                run_test_target(&override_file, &opts, mode, false),
                TestTargetOutcome::Ran(true)
                    | TestTargetOutcome::Override(true)
                    | TestTargetOutcome::NoTests
            );
            exit(if ok {
                ExitCodes::OK
            } else {
                ExitCodes::USER_ERROR
            });
        }
    }
    if entry.is_file() {
        match run_test_target(&entry, &opts, mode, true) {
            TestTargetOutcome::Override(ok) => exit(if ok {
                ExitCodes::OK
            } else {
                ExitCodes::USER_ERROR
            }),
            TestTargetOutcome::Ran(ok) => {
                ran += 1;
                any_fail = !ok;
            }
            TestTargetOutcome::NoTests => {}
        }
    }
    for target in jet::ProjectParts::source_files(root) {
        if target == entry || is_reserved_target_file(&target) {
            continue;
        }
        match run_test_target(&target, &opts, mode, true) {
            // A member's own `fn test` override is that file's harness choice;
            // only the entry's override speaks for the whole package.
            TestTargetOutcome::Ran(ok) | TestTargetOutcome::Override(ok) => {
                ran += 1;
                any_fail |= !ok;
            }
            TestTargetOutcome::NoTests => {}
        }
    }
    if ran == 0 {
        run_test_opts(&entry.to_string_lossy(), opts, mode);
    }
    exit(if any_fail {
        ExitCodes::USER_ERROR
    } else {
        ExitCodes::OK
    });
}

/// D-TESTKIT1=A gap #2: walk every subdirectory under
/// `dir`, collecting `.ext` files. `build/` and dotdirs (`.git`, `.jet`'s own
/// cache, etc.) are skipped, as are reserved package/env/workspace/config
/// files. The test runner owns this target walk.
pub(crate) fn collect_source_files_recursive(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "build" || name.starts_with('.') {
                continue;
            }
            collect_source_files_recursive(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext)
            && !is_reserved_target_file(&path)
        {
            out.push(path);
        }
    }
}

/// Reserved package surfaces are never test targets: they declare
/// package, env, workspace, and config facts instead of runnable code. One
/// list for the recursive target walk and for the package member set.
fn is_reserved_target_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name == jet::Syntax::PACKAGE_FILE
                || name == jet::Syntax::ENV_FILE
                || name == jet::Syntax::WORKSPACE_FILE
                || name == jet::Syntax::CONFIG_FILE
                || jet::Syntax::COMMAND_ROLE_FILES.contains(&name)
        })
}

/// What one test target contributed. Package mode needs "no tests here" as a
/// result distinct from failure; a named file or directory target keeps falling
/// through to the harness so it reports E0601.
enum TestTargetOutcome {
    Ran(bool),
    NoTests,
    /// The file defines `fn test` (D-CMD-OVERRIDE1=C) and ran as that override.
    Override(bool),
}

fn run_test_file(path: &Path, opts: &TestRunOpts, mode: OutputMode) -> bool {
    matches!(
        run_test_target(path, opts, mode, false),
        TestTargetOutcome::Ran(true)
            | TestTargetOutcome::Override(true)
            | TestTargetOutcome::NoTests
    )
}

/// Run one test target. `package` marks bare-`jet test` package members, whose
/// empty files are skipped rather than reported.
fn run_test_target(
    path: &Path,
    opts: &TestRunOpts,
    mode: OutputMode,
    package: bool,
) -> TestTargetOutcome {
    let update_snapshots = opts.update_snapshots;
    let coverage = opts.coverage;
    let shown = path.to_string_lossy();
    let profile = if let Some(name) = opts.profile.as_deref() {
        resolve_named_profile(name, &shown, mode)
    } else if opts.release || opts.measure {
        BuildProfile::Release
    } else {
        BuildProfile::Default
    };
    let profile_tag = format!(
        "{};{};aot-target=native",
        profile.cache_tag(),
        setting_overrides_tag(&opts.setting_overrides)
    );
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            crate::cli_error!("E2105", "couldn't read `{}`: {}", shown, e);
            return TestTargetOutcome::Ran(false);
        }
    };
    // `jet prove` supplies a report path and identity for this child. Preserve
    // that protocol so the parent can consume the same typed report; ordinary
    // `jet test` runs create and clean up their own private report.
    let computed_source_revision = jet::SHA256::sha256_hex(src.as_bytes());
    let source_revision = std::env::var(jet_foundation::Evidence::EVIDENCE_SOURCE_REVISION_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or(computed_source_revision);
    let report_id = std::env::var(jet_foundation::Evidence::EVIDENCE_REPORT_ID_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let run = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test execution starts after the Unix epoch")
                .as_nanos();
            format!(
                "test:{}:{}:{run}",
                source_revision.get(..16).unwrap_or(source_revision.as_str()),
                std::process::id(),
            )
        });
    let build_revision = std::env::var(jet_foundation::Evidence::EVIDENCE_BUILD_REVISION_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            jet::SHA256::sha256_hex(
                format!("jet-test-build-v1:{source_revision}:{profile_tag}").as_bytes(),
            )
        });
    let revision = std::env::var(jet_foundation::Evidence::EVIDENCE_REVISION_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            jet::SHA256::sha256_hex(
                format!("jet-test-mir-v1:{source_revision}:{profile_tag}").as_bytes(),
            )
        });
    let inherited_report_path = std::env::var_os(jet_foundation::Evidence::EVIDENCE_REPORT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let preserve_report = inherited_report_path.is_some();
    let report_path = inherited_report_path.unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "jet_test_{}_{}.bin",
            std::process::id(),
            source_revision
                .get(..16)
                .unwrap_or(source_revision.as_str())
        ))
    });
    let evidence_toolchain = std::env::var(jet_foundation::Evidence::EVIDENCE_TOOLCHAIN_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let evidence_target = std::env::var(jet_foundation::Evidence::EVIDENCE_TARGET_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(host_target_triple);
    let evidence_profile = std::env::var(jet_foundation::Evidence::EVIDENCE_PROFILE_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| profile.budget_name().to_string());
    let _ = fs::remove_file(&report_path);
    if let Some(expression) = opts.where_expr.as_deref() {
        let facts = test_target_facts(path, &src, opts, package);
        if !test_where_matches(expression, &facts) {
            return TestTargetOutcome::NoTests;
        }
    }
    // D-TEST4: discover and run any `///` doctest examples first. They are
    // independent of `#Test` blocks, so a file with only doctests is testable.
    let has_doctests = !jet::Doctest::discover(&src).is_empty();
    let override_entry = !opts.show_default && jet::has_entry_fn(&shown, "test");
    let doctests_ok = with_test_evidence_environment(
        &report_path,
        &report_id,
        &evidence_toolchain,
        &evidence_target,
        &evidence_profile,
        &source_revision,
        &build_revision,
        &revision,
        || {
            run_doctests(
                path,
                &shown,
                &src,
                update_snapshots,
                &profile,
                &opts.setting_overrides,
                opts.capture,
                mode,
            )
        },
    );
    let finish_doctest_report = || -> bool {
        match finish_test_evidence(
            &report_path,
            &report_id,
            &shown,
            &evidence_toolchain,
            &evidence_target,
            &evidence_profile,
            &source_revision,
            &build_revision,
            &revision,
            None,
            doctests_ok,
            preserve_report,
            mode,
        ) {
            Ok(Some(ok)) => ok,
            Ok(None) => doctests_ok,
            Err(error) => {
                if !preserve_report {
                    let _ = fs::remove_file(&report_path);
                }
                crate::cli_error!(
                    "E2105",
                    "couldn't finalize test evidence for `{}`: {}",
                    shown,
                    error
                );
                false
            }
        }
    };
    if opts.docs {
        if !has_doctests {
            let _ = fs::remove_file(&report_path);
            return TestTargetOutcome::NoTests;
        }
        return TestTargetOutcome::Ran(finish_doctest_report());
    }

    if !override_entry {
        if package {
            // Package mode walks every member: one with no `#Test` blocks or
            // contract-bearing callables contributes nothing to the run,
            // which is not the error an empty named target is. A member that
            // fails to load answers `true` here, so its real compile error still
            // surfaces below instead of being silently skipped.
            if !jet::has_test_blocks(&shown) && !jet::has_test_contracts(&shown) {
                return if has_doctests {
                    TestTargetOutcome::Ran(finish_doctest_report())
                } else {
                    let _ = fs::remove_file(&report_path);
                    TestTargetOutcome::NoTests
                };
            }
        } else if has_doctests && !jet::has_test_blocks(&shown) && !jet::has_test_contracts(&shown)
        {
            // A file with doctests but no `#Test` blocks or contracts is
            // testable on its doctests alone — skip the test harness (which
            // would otherwise error E0601). A file with NEITHER falls through
            // so the harness reports E0601.
            return TestTargetOutcome::Ran(finish_doctest_report());
        }
    }
    let cacheable = test_result_cache_write_allowed(opts) && !has_doctests && !override_entry;
    if cacheable && !opts.fresh {
        if let Some(cached) = test_result_cache_read(path, opts, package) {
            let evidence_ok = match finish_test_evidence(
                &report_path,
                &report_id,
                &shown,
                &evidence_toolchain,
                &evidence_target,
                &evidence_profile,
                &source_revision,
                &build_revision,
                &revision,
                Some(&cached.stdout),
                cached.ok,
                preserve_report,
                mode,
            ) {
                Ok(Some(ok)) => ok,
                Ok(None) => cached.ok,
                Err(error) => {
                    if !preserve_report {
                        let _ = fs::remove_file(&report_path);
                    }
                    crate::cli_error!(
                        "E2105",
                        "couldn't finalize test evidence for `{}`: {}",
                        shown,
                        error
                    );
                    false
                }
            };
            if !mode.json {
                emit_test_capture(&cached, opts.capture, mode);
            }
            return TestTargetOutcome::Ran(cached.ok && evidence_ok);
        }
    }

    let (rust_code, ffi_link) = match if override_entry {
        if opts.capture == TestCapturePolicy::All {
            write_mode_status(mode, "jet test: using fn test override\n");
        }
        jet::compile_test_override_with_path_and_profile(
            &src,
            &shown,
            coverage,
            profile.budget_name(),
            &opts.setting_overrides,
        )
    } else {
        jet::compile_tests_with_path_cov_and_profile(
            &src,
            &shown,
            coverage,
            profile.budget_name(),
            &opts.setting_overrides,
        )
    } {
        Ok(r) => r,
        Err(diags) => {
            report_problems(mode, &shown, &src, &diags);
            if let Err(error) = finish_test_evidence(
                &report_path,
                &report_id,
                &shown,
                &evidence_toolchain,
                &evidence_target,
                &evidence_profile,
                &source_revision,
                &build_revision,
                &revision,
                None,
                false,
                preserve_report,
                mode,
            ) {
                crate::cli_error!(
                    "E2105",
                    "couldn't finalize test evidence for `{}`: {}",
                    shown,
                    error
                );
            }
            if !preserve_report {
                let _ = fs::remove_file(&report_path);
            }
            return if override_entry {
                TestTargetOutcome::Override(false)
            } else {
                TestTargetOutcome::Ran(false)
            };
        }
    };
    // Test harnesses are one-shot process-private executables. Concurrent
    // `jet test` invocations may target the same file; sharing
    // `build/test_<stem>` lets one process replace an executable while another
    // is launching it (ETXTBSY on Linux, sharing violations on Windows).
    let bin = test_bin_path(path);
    let cache_key = native_cache_key(
        shown.as_ref(),
        profile.budget_name(),
        &profile_tag,
        if override_entry {
            if coverage {
                "testcov-override"
            } else {
                "test-override"
            }
        } else if coverage {
            "testcov"
        } else {
            "test"
        },
        None,
    );
    let record_request = opts
        .record
        .as_deref()
        .map(|name| (name, profile.budget_name().to_owned()));
    let coverage_profile = coverage.then(|| profile.budget_name().to_owned());
    build(
        &shown,
        &rust_code,
        None,
        bin.clone(),
        profile,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        false,
        // `jet test` caches on the same canonical-AST key, but in its own mode
        // space (the test-harness binary must never be served for a `jet run`);
        // profile and `--coverage` instrumentation are distinct binaries again.
        cache_key,
    );
    // D-COV1: run with `JET_COV_OUT` pointing at a temp file; the harness writes
    // function and branch records there for the coverage report.
    let cov_out = if coverage {
        Some(bin.with_extension("cov"))
    } else {
        None
    };
    let mut cmd = Command::new(&bin);
    if let Some(root) = path.parent().and_then(jet::Loader::find_manifest_root) {
        cmd.env("JET_PROJECT_ROOT", root);
    }
    // D-TOOL4: `-u`/`--update-snapshots` must reach the harness. Both
    // `expect(…).snapshot()` (any value) and `testing.snap` (`=1`) honor this.
    if update_snapshots {
        cmd.env("JET_UPDATE_SNAPSHOTS", "1");
    }
    if let Some(co) = &cov_out {
        let _ = fs::remove_file(co);
        cmd.env("JET_COV_OUT", co);
    }
    if let Some(engines) = &opts.browser_engines {
        cmd.env("JET_TEST_BROWSERS", engines);
    }
    if let Some(retries) = opts.browser_retries {
        cmd.env("JET_TEST_RETRIES", retries.to_string());
    }
    if let Some(reporter) = &opts.browser_reporter {
        cmd.env("JET_TEST_REPORTER", reporter);
    }
    if opts.browser_ui {
        cmd.env("JET_TEST_BROWSER_UI", "1");
    }
    if opts.browser_visual {
        cmd.env("JET_TEST_BROWSER_VISUAL", "1");
    }
    if opts.browser_trace {
        // Explicit browser tracing captures the first passing attempt too;
        // retry tracing remains enabled for the same run.
        cmd.env("JET_TEST_TRACE", "1");
        cmd.env("JET_TEST_TRACE_ON_RETRY", "1");
    }
    // D-TESTKIT1=A gaps #3/#4: filter/shuffle/serial reach the harness the same
    // way `--coverage`/`-u` do — an env var the generated `main` reads (see
    // `emit_test_main_cov` in jet-codegen).
    if let Some(filter) = &opts.filter {
        cmd.env("JET_TEST_FILTER", filter);
    }
    if let Some(seed) = opts.shuffle_seed {
        cmd.env("JET_TEST_SHUFFLE_SEED", seed.to_string());
    }
    if opts.serial {
        cmd.env("JET_TEST_SERIAL", "1");
    }
    if opts.trace_tiers {
        cmd.env("JET_TEST_TRACE_TIERS", "1");
    }
    if opts.measure {
        cmd.env("JET_TEST_MEASURE", "1");
    }
    cmd.env("JET_TEST_CAPTURE", opts.capture.as_str());
    if mode.json {
        cmd.env("JET_TEST_JSON", "1");
    }
    cmd.env(jet_foundation::Evidence::EVIDENCE_REPORT_ENV, &report_path)
        .env(jet_foundation::Evidence::EVIDENCE_REPORT_ID_ENV, &report_id)
        .env(
            jet_foundation::Evidence::EVIDENCE_TOOLCHAIN_ENV,
            &evidence_toolchain,
        )
        .env(
            jet_foundation::Evidence::EVIDENCE_TARGET_ENV,
            &evidence_target,
        )
        .env(
            jet_foundation::Evidence::EVIDENCE_PROFILE_ENV,
            &evidence_profile,
        )
        .env(
            jet_foundation::Evidence::EVIDENCE_SOURCE_REVISION_ENV,
            &source_revision,
        )
        .env(
            jet_foundation::Evidence::EVIDENCE_BUILD_REVISION_ENV,
            &build_revision,
        )
        .env(jet_foundation::Evidence::EVIDENCE_REVISION_ENV, &revision);
    let record = record_request.as_ref().map(|(name, profile)| {
        crate::ProveReplay::begin_named_capture(
            &shown,
            name,
            profile,
            &opts.setting_overrides,
            mode.json,
        )
        .unwrap_or_else(|status| exit(status))
    });
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) => {
            let _ = fs::remove_file(&bin);
            if let Some(co) = &cov_out {
                let _ = fs::remove_file(co);
            }
            if let Err(error) = finish_test_evidence(
                &report_path,
                &report_id,
                &shown,
                &evidence_toolchain,
                &evidence_target,
                &evidence_profile,
                &source_revision,
                &build_revision,
                &revision,
                None,
                false,
                preserve_report,
                mode,
            ) {
                crate::cli_error!(
                    "E2105",
                    "couldn't finalize test evidence for `{}`: {}",
                    shown,
                    error
                );
            }
            if !preserve_report {
                let _ = fs::remove_file(&report_path);
            }
            crate::cli_error!("E2105", "couldn't run tests in `{}`: {}", shown, e);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let child_ok = out.status.success();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let evidence_ok = match finish_test_evidence(
        &report_path,
        &report_id,
        &shown,
        &evidence_toolchain,
        &evidence_target,
        &evidence_profile,
        &source_revision,
        &build_revision,
        &revision,
        Some(&stdout),
        child_ok,
        preserve_report,
        mode,
    ) {
        Ok(Some(ok)) => ok,
        Ok(None) => child_ok,
        Err(error) => {
            if !preserve_report {
                let _ = fs::remove_file(&report_path);
            }
            crate::cli_error!(
                "E2105",
                "couldn't finalize test evidence for `{}`: {}",
                shown,
                error
            );
            false
        }
    };
    let ok = child_ok && doctests_ok && evidence_ok;
    let result = TestResultCache { ok, stdout, stderr };
    if !mode.json {
        emit_test_capture(&result, opts.capture, mode);
    }
    if cacheable {
        test_result_cache_write(path, opts, package, &result);
    }
    if let Some(co) = &cov_out {
        report_coverage(
            &shown,
            co,
            mode,
            coverage_profile.as_deref().expect("coverage profile retained before build"),
            &opts.setting_overrides,
        );
        let _ = fs::remove_file(co);
    }
    let _ = fs::remove_file(&bin);
    let status = if ok {
        ExitCodes::OK
    } else if child_ok {
        ExitCodes::USER_ERROR
    } else {
        child_exit_code(out.status)
    };
    finish_recorded_artifacts(record.as_ref(), opts.record.as_deref(), None, status, mode);
    if override_entry {
        TestTargetOutcome::Override(ok)
    } else {
        TestTargetOutcome::Ran(ok)
    }
}

/// Read one-byte controls for the persistent `jet test --watch` loop.
fn spawn_test_watch_input() -> mpsc::Receiver<u8> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut byte = [0u8; 1];
        while stdin.read(&mut byte).ok().is_some_and(|count| count != 0) {
            if sender.send(byte[0]).is_err() {
                break;
            }
        }
    });
    receiver
}

fn test_watch_failure_names(stdout: &str, stderr: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for stream in [stdout, stderr] {
        for line in stream.lines() {
            let candidate = line.trim();
            let Ok(value) = parse_json(candidate) else {
                continue;
            };
            if is_canonical_test_value(&value) {
                append_test_report_failures(&value, &mut names, &mut seen);
            } else if is_canonical_status_value(&value) {
                append_status_report_failures(&value, &mut names, &mut seen);
            }
        }
    }
    // Human-mode children still provide the stable marker when JSON mode is
    // unavailable (and older generated harnesses remain watchable).
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        let name = line
            .strip_prefix("FAIL ")
            .or_else(|| line.strip_suffix(": FAIL"))
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(name) = name {
            let name = name.to_string();
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    names
}

fn is_canonical_test_value(value: &DataTree) -> bool {
    let DataTree::Object(object) = value else {
        return false;
    };
    let schema = object_field(object, "schema").and_then(|value| value.as_str().ok());
    matches!(schema, Some("jet.test.v1" | "jet-test-v1"))
        && matches!(object_field(object, "tests"), Some(DataTree::Array(_)))
}

fn is_canonical_status_value(value: &DataTree) -> bool {
    let DataTree::Object(object) = value else {
        return false;
    };
    matches!(
        object_field(object, "schema").and_then(|value| value.as_str().ok()),
        Some("jet.status/v1")
    )
}

fn append_status_report_failures(
    value: &DataTree,
    names: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
) {
    let DataTree::Object(object) = value else {
        return;
    };
    let Some(DataTree::Object(evidence)) = object_field(object, "evidence") else {
        return;
    };
    let Some(DataTree::Array(records)) = object_field(evidence, "evidence") else {
        return;
    };
    for record in records {
        let DataTree::Object(record) = record else {
            continue;
        };
        let Some(name) = object_field(record, "claim").and_then(|value| value.as_str().ok()) else {
            continue;
        };
        let failed = object_field(record, "outcome")
            .and_then(|value| value.as_str().ok())
            .map(|outcome| matches!(outcome, "failed" | "error" | "unavailable"))
            .unwrap_or(false);
        if failed {
            let name = name.to_string();
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
}

fn append_test_report_failures(
    value: &DataTree,
    names: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
) {
    let DataTree::Object(object) = value else {
        return;
    };
    let Some(DataTree::Array(tests)) = object_field(object, "tests") else {
        return;
    };
    for test in tests {
        let DataTree::Object(test) = test else {
            continue;
        };
        let Some(name) = object_field(test, "name").and_then(|value| value.as_str().ok()) else {
            continue;
        };
        let failed = object_field(test, "ok")
            .and_then(|value| match value {
                DataTree::Bool(ok) => Some(!ok),
                _ => None,
            })
            .or_else(|| {
                object_field(test, "passed").and_then(|value| match value {
                    DataTree::Bool(passed) => Some(!passed),
                    _ => None,
                })
            })
            .or_else(|| {
                object_field(test, "status")
                    .and_then(|value| value.as_str().ok())
                    .map(|status| {
                        matches!(
                            status.to_ascii_lowercase().as_str(),
                            "fail" | "failed" | "error" | "panic"
                        )
                    })
            })
            .unwrap_or(false);
        if failed {
            let name = name.to_string();
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
}

fn append_test_watch_flags(command: &mut Command, opts: &TestRunOpts, filter: Option<&str>) {
    command.arg("--quiet").arg("--capture=all");
    if opts.show_default {
        command.arg("--show-default");
    }
    if opts.update_snapshots {
        command.arg("--update-snapshots");
    }
    if opts.coverage {
        command.arg("--coverage");
    }
    if opts.release {
        command.arg("--release");
    }
    if let Some(profile) = opts.profile.as_deref() {
        command.arg(format!("--profile={profile}"));
    }
    for (key, value) in &opts.setting_overrides {
        command.arg(format!("--set={key}={value}"));
    }
    if opts.trace_tiers {
        command.arg("--trace-tiers");
    }
    if let Some(engines) = opts.browser_engines.as_deref() {
        command.arg(format!("--browser={engines}"));
    }
    if let Some(retries) = opts.browser_retries {
        command.arg(format!("--browser-retries={retries}"));
    }
    if let Some(reporter) = opts.browser_reporter.as_deref() {
        command.arg(format!("--browser-reporter={reporter}"));
    }
    if opts.browser_ui {
        command.arg("--browser-ui");
    }
    if opts.browser_visual {
        command.arg("--browser-visual");
    }
    if opts.browser_trace {
        command.arg("--browser-trace");
    }
    if let Some(filter) = filter.or(opts.filter.as_deref()) {
        command.arg(format!("--filter={filter}"));
    }
    if let Some(seed) = opts.shuffle_seed {
        command.arg(format!("--shuffle={seed}"));
    }
    if opts.serial {
        command.arg("--serial");
    }
    if opts.measure {
        command.arg("--measure");
    }
    if let Some(expression) = opts.where_expr.as_deref() {
        command.arg(format!("--where={expression}"));
    }
    if opts.docs {
        command.arg("--docs");
    }
    if opts.fresh {
        command.arg("--fresh");
    }
}

fn run_test_watch_child(
    path: &str,
    opts: &TestRunOpts,
    filter: Option<&str>,
    mode: OutputMode,
) -> (i32, TestResultCache) {
    let executable =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from(jet::Syntax::BINARY_NAME));
    let mut command = Command::new(executable);
    command
        .arg("test")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if mode.json {
        command.arg("--json");
    }
    append_test_watch_flags(&mut command, opts, filter);
    // The parent applies the selected capture policy after collecting the
    // complete child stream, so watch reruns always ask the canonical runner
    // for all captures. Machine mode still receives one JSON report.
    command.env("JET_TEST_CAPTURE", TestCapturePolicy::All.as_str());
    if mode.json {
        command.env("JET_TEST_JSON", "1");
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return (
                ExitCodes::USER_ERROR,
                TestResultCache {
                    ok: false,
                    stdout: String::new(),
                    stderr: format!("jet test watcher: couldn't run child: {error}\n"),
                },
            );
        }
    };
    let status = child_exit_code(output.status);
    (
        status,
        TestResultCache {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
    )
}

fn append_test_watch_failures(
    failures: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    found: impl IntoIterator<Item = String>,
) {
    for name in found {
        if seen.insert(name.clone()) {
            failures.push(name);
        }
    }
}

fn run_test_watch_selection(
    path: &str,
    opts: &TestRunOpts,
    policy: TestCapturePolicy,
    failed: Option<&[String]>,
    mode: OutputMode,
) -> (i32, Vec<String>) {
    let mut status = ExitCodes::OK;
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    match failed {
        Some(names) => {
            for name in names {
                let (child_status, result) = run_test_watch_child(path, opts, Some(name), mode);
                emit_test_capture(&result, policy, mode);
                if child_status != ExitCodes::OK {
                    status = child_status;
                    let found = test_watch_failure_names(&result.stdout, &result.stderr);
                    if found.is_empty() {
                        append_test_watch_failures(
                            &mut failures,
                            &mut seen,
                            std::iter::once(name.clone()),
                        );
                    } else {
                        append_test_watch_failures(&mut failures, &mut seen, found);
                    }
                }
            }
        }
        None => {
            let (child_status, result) = run_test_watch_child(path, opts, None, mode);
            emit_test_capture(&result, policy, mode);
            status = child_status;
            append_test_watch_failures(
                &mut failures,
                &mut seen,
                test_watch_failure_names(&result.stdout, &result.stderr),
            );
        }
    }
    (status, failures)
}

fn run_test_watch(path: &str, opts: TestRunOpts, mode: OutputMode) -> ! {
    let target = Path::new(path);
    let mut watch = match jet_devserver::WatchSession::open(target) {
        Ok(watch) => watch,
        Err(diagnostic) => {
            write_mode_diagnostic(
                mode,
                &jet::render_all_colored(path, "", &[diagnostic], mode.color_stderr()),
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    let mut files = Vec::new();
    if target.is_dir() {
        collect_source_files_recursive(target, jet::Syntax::FILE_EXT, &mut files);
    } else {
        files.push(target.to_path_buf());
    }
    for file in files {
        watch
            .graph_mut()
            .upsert(file, jet_devserver::WatchService::RootKind::BuildInput);
    }
    watch.graph_mut().scan_runtime_inputs();
    let manifest_search = if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or_else(|| Path::new("."))
    };
    if let Some(root) = jet::Loader::find_manifest_root(manifest_search) {
        watch.graph_mut().upsert(
            root.join(jet::Syntax::PACKAGE_FILE),
            jet_devserver::WatchService::RootKind::Manifest,
        );
    }
    // A watch session owns one capture authority for its lifetime. Do not pass
    // `--record` to each child: the ordinary named artifact is intentionally
    // create-once and a rerun must not collide with it or overwrite another
    // explicit capture.
    let watch_record = opts.record.as_deref().map(|name| {
        let record_path = if target.is_dir() {
            crate::find_project_entry(target)
        } else {
            target.to_path_buf()
        };
        let record_path = record_path.to_string_lossy().into_owned();
        let profile = opts
            .profile
            .as_deref()
            .unwrap_or(if opts.release || opts.measure {
                jet::Syntax::BUILD_PROFILE_RELEASE
            } else {
                "dev"
            });
        crate::ProveReplay::begin_named_capture(
            &record_path,
            name,
            profile,
            &opts.setting_overrides,
            mode.json,
        )
        .unwrap_or_else(|status| exit(status))
    });
    if !mode.json && !mode.quiet {
        let cache_mode = if opts.fresh { "fresh" } else { "cached" };
        write_mode_status(
            mode,
            &format!(
                "jet test --watch ({cache_mode} results): r rerun failed, a run all, o cycle output, q quit\n"
            ),
        );
    }
    let input = spawn_test_watch_input();
    let mut output = opts.capture;
    let mut failed = Vec::new();
    let mut last_status = ExitCodes::OK;
    let mut last_good = false;
    let (status, current_failures) = run_test_watch_selection(path, &opts, output, None, mode);
    last_status = status;
    failed = current_failures;
    if status == ExitCodes::OK {
        last_good = true;
    } else if !mode.json && !mode.quiet {
        write_mode_status(
            mode,
            "jet test: failure; last-good build artifact retained\n",
        );
    }
    loop {
        let mut action = None;
        while let Ok(byte) = input.try_recv() {
            action = Some(byte.to_ascii_lowercase());
        }
        if let Some(byte) = action {
            match byte {
                b'q' | 3 => {
                    if let Some(capture) = watch_record.as_ref() {
                        if let Err(status) = crate::ProveReplay::finish_named_capture(
                            capture,
                            last_status,
                            mode.json,
                        ) {
                            exit(status);
                        }
                    }
                    exit(last_status);
                }
                b'r' => {
                    if failed.is_empty() {
                        if !mode.json && !mode.quiet {
                            write_mode_status(mode, "jet test: no failed tests to rerun\n");
                        }
                    } else {
                        let (status, current_failures) =
                            run_test_watch_selection(path, &opts, output, Some(&failed), mode);
                        last_status = status;
                        failed = current_failures;
                        if status == ExitCodes::OK {
                            last_good = true;
                        } else if last_good && !mode.json && !mode.quiet {
                            write_mode_status(
                                mode,
                                "jet test: failure; last-good build artifact retained\n",
                            );
                        }
                    }
                }
                b'a' => {
                    let (status, current_failures) =
                        run_test_watch_selection(path, &opts, output, None, mode);
                    last_status = status;
                    failed = current_failures;
                    if status == ExitCodes::OK {
                        last_good = true;
                    } else if last_good && !mode.json && !mode.quiet {
                        write_mode_status(
                            mode,
                            "jet test: failure; last-good build artifact retained\n",
                        );
                    }
                }
                b'o' => {
                    output = match output {
                        TestCapturePolicy::Failed => TestCapturePolicy::All,
                        TestCapturePolicy::All => TestCapturePolicy::None,
                        TestCapturePolicy::None => TestCapturePolicy::Failed,
                    };
                    if !mode.json && !mode.quiet {
                        write_mode_status(mode, &format!("jet test: output {}\n", output.as_str()));
                    }
                }
                _ => {}
            }
        }
        if let Some(receipt) = watch.poll() {
            let _ = watch.acknowledge(&receipt);
            let selection = (!failed.is_empty()).then_some(failed.as_slice());
            let (status, current_failures) =
                run_test_watch_selection(path, &opts, output, selection, mode);
            last_status = status;
            failed = current_failures;
            if status == ExitCodes::OK {
                last_good = true;
            } else if last_good && !mode.json && !mode.quiet {
                write_mode_status(
                    mode,
                    "jet test: failure; last-good build artifact retained\n",
                );
            }
        }
        thread::sleep(Duration::from_millis(
            jet_devserver::WATCH_POLL_INTERVAL_MS.max(25),
        ));
    }
}

/// D-TEST4: compile and run each ```` ```jet ```` doctest block found in the file's
/// `///` doc comments, comparing each `// =>` line to the produced value. A
/// mismatch fires E2901 (or rewrites the claimed output when `update_snapshots`).
/// Returns true when every block (if any) passed. A file with no doctests passes
/// trivially and prints nothing.
fn run_doctests(
    path: &Path,
    shown: &str,
    src: &str,
    update_snapshots: bool,
    profile: &BuildProfile,
    setting_overrides: &BTreeMap<String, String>,
    capture: TestCapturePolicy,
    mode: OutputMode,
) -> bool {
    let blocks = jet::Doctest::discover(src);
    if blocks.is_empty() {
        return true;
    }
    let mut all_ok = true;
    let mut rewritten = src.to_string();
    let mut did_rewrite = false;
    for (n, block) in blocks.iter().enumerate() {
        let label = format!("doctest at {}:{}", shown, block.fence_line);
        let program = jet::Doctest::synth_program(block);
        // Write the synthetic program to a temp file next to the build dir so the
        // normal compile+build pipeline can consume it.
        let _ = fs::create_dir_all("build");
        let tmp = PathBuf::from("build").join(format!(
            "{}__doctest_{}.{}.jet",
            stem(shown),
            n,
            std::process::id()
        ));
        if fs::write(&tmp, &program).is_err() {
            emit_doctest_result(&label, false, "", "", capture, mode);
            crate::cli_error!("E2105", "couldn't stage doctest from `{}`", shown);
            all_ok = false;
            write_doctest_proof_record(
                &label,
                shown,
                block.fence_line,
                false,
                "producer_start_failed",
            );
            continue;
        }
        let tmp_shown = tmp.to_string_lossy().into_owned();
        let compiled = jet::compile_with_target_and_gates_and_profile_and_settings(
            &program,
            &tmp_shown,
            jet::Policy::GateSet::default(),
            None,
            profile.budget_name(),
            setting_overrides,
        );
        let (rust_code, ffi_link) = match compiled {
            Ok(out) => (out.rust, out.ffi),
            Err(diags) => {
                // The doctest source is wrong; surface its diagnostics against the
                // synthetic program so the author sees the exact problem.
                emit_doctest_result(&label, false, "", "", capture, mode);
                report_problems(mode, &tmp_shown, &program, &diags);
                all_ok = false;
                write_doctest_proof_record(
                    &label,
                    shown,
                    block.fence_line,
                    false,
                    "does not compile",
                );
                let _ = fs::remove_file(&tmp);
                continue;
            }
        };
        let bin = tmp.with_extension("");
        let generated_rs = PathBuf::from("build").join(format!("{}.rs", stem(&tmp_shown)));
        build(
            &tmp_shown,
            &rust_code,
            None,
            bin.clone(),
            profile.clone(),
            ffi_link.as_ref(),
            &[],
            false,
            None,
            None,
            None,
            mode,
            false,
            // Doctest binaries are one-shot synthetic programs; not cached.
            None,
        );
        let mut command = Command::new(&bin);
        command.env("JET_TEST_CAPTURE", capture.as_str());
        if mode.json {
            command.env("JET_TEST_JSON", "1");
        }
        let out = match command.output() {
            Ok(o) => o,
            Err(e) => {
                emit_doctest_result(&label, false, "", "", capture, mode);
                crate::cli_error!("E2105", "couldn't run {}: {}", label, e);
                all_ok = false;
                write_doctest_proof_record(
                    &label,
                    shown,
                    block.fence_line,
                    false,
                    "producer_start_failed",
                );
                let _ = fs::remove_file(&tmp);
                let _ = fs::remove_file(&bin);
                let _ = fs::remove_file(&generated_rs);
                continue;
            }
        };
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(&bin);
        let _ = fs::remove_file(&generated_rs);
        if !out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            emit_doctest_result(&label, false, &stdout, &stderr, capture, mode);
            all_ok = false;
            write_doctest_proof_record(&label, shown, block.fence_line, false, "runtime error");
            continue;
        }
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let produced: Vec<&str> = stdout.lines().collect();
        let mut block_ok = true;
        for (i, e) in block.expects.iter().enumerate() {
            let actual = produced.get(i).copied().unwrap_or("");
            if actual == e.expected {
                continue;
            }
            if update_snapshots {
                // D-TOOL4 / E2901 fix: rewrite the claimed `// =>` value in place.
                if rewrite_doctest_expect(&mut rewritten, e.line, actual) {
                    did_rewrite = true;
                    continue;
                }
            }
            block_ok = false;
            all_ok = false;
            let span = doc_line_span(src, e.line);
            let diag = jet::Doctest::mismatch_diag(shown, e, actual, span);
            // Render against the original file so the line points at the doc
            // comment's producing line.
            report_problems(mode, shown, src, &[diag]);
        }
        emit_doctest_result(&label, block_ok, &stdout, &stderr, capture, mode);
        write_doctest_proof_record(
            &label,
            shown,
            block.fence_line,
            block_ok,
            if block_ok { "" } else { "output mismatch" },
        );
    }
    if did_rewrite {
        if let Err(e) = fs::write(path, &rewritten) {
            crate::cli_error!(
                "E2105",
                "couldn't update doctest snapshots in `{}`: {}",
                shown,
                e
            );
            return false;
        }
    }
    all_ok
}

fn write_doctest_proof_record(name: &str, file: &str, line: usize, passed: bool, message: &str) {
    let Ok(path) = std::env::var(jet_foundation::Evidence::EVIDENCE_REPORT_ENV) else {
        return;
    };
    let _ = jet_foundation::Evidence::write_record_from_codes(
        Path::new(&path),
        4,
        if passed { 0 } else { 1 },
        name,
        message,
        file,
        line as u32,
    );
}

/// Replace the `// => …` claim on 1-based `line` with `actual`. Returns false when
/// the line has no expect marker (caller falls through to E2901).
fn rewrite_doctest_expect(src: &mut String, line: usize, actual: &str) -> bool {
    let mut out = String::with_capacity(src.len() + actual.len());
    let mut found = false;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 != line {
            out.push_str(l);
            continue;
        }
        let (nl, body) = if let Some(b) = l.strip_suffix('\n') {
            ("\n", b)
        } else {
            ("", l)
        };
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim_start();
        if !trimmed.starts_with("///") {
            out.push_str(l);
            continue;
        }
        let indent_len = body.len() - trimmed.len();
        let indent = &body[..indent_len];
        let after_slashes = &trimmed[3..];
        let doc_space = if after_slashes.starts_with(' ') {
            " "
        } else {
            ""
        };
        let inner = after_slashes.strip_prefix(' ').unwrap_or(after_slashes);
        let Some(idx) = find_doctest_expect_marker(inner) else {
            out.push_str(l);
            continue;
        };
        let expr = inner[..idx].trim_end();
        out.push_str(indent);
        out.push_str("///");
        out.push_str(doc_space);
        out.push_str(expr);
        if !expr.is_empty() {
            out.push(' ');
        }
        out.push_str("// => ");
        out.push_str(actual);
        out.push_str(nl);
        found = true;
    }
    if found {
        *src = out;
    }
    found
}

/// Same marker scan as `Doctest::find_expect_marker` — keep local so CmdCompile
/// does not depend on a private helper.
fn find_doctest_expect_marker(s: &str) -> Option<usize> {
    const MARKER: &str = "// =>";
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i + MARKER.len() <= bytes.len() {
        let c = bytes[i];
        if c == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        }
        if !in_str && &s[i..i + MARKER.len()] == MARKER {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// D-TEST4: the byte span of the (1-based) `line` in `src`, for an E2901 report.
fn doc_line_span(src: &str, line: usize) -> Option<jet::Diagnostics::Span> {
    let mut start = 0usize;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let trimmed_len = l.trim_end_matches(['\n', '\r']).len();
            return Some(jet::Diagnostics::Span::new(start, start + trimmed_len));
        }
        start += l.len();
    }
    None
}

/// D-COV1: read function and branch records and render the human or JSON report.
/// Each branch has one row for each outcome so an uncovered side remains visible.
struct CoverageBranchRow {
    id: String,
    function: String,
    taken: u64,
    not_taken: u64,
}

fn report_coverage(
    file: &str,
    cov_out: &Path,
    mode: OutputMode,
    profile: &str,
    setting_overrides: &std::collections::BTreeMap<String, String>,
) {
    macro_rules! render {
        ($($args:tt)*) => {{
            let text = format!($($args)*);
            write_mode_renderable(mode, &format!("{text}\n"));
        }};
    }
    let mut function_hits = std::collections::BTreeSet::new();
    let mut branches = Vec::new();
    for record in fs::read_to_string(cov_out).unwrap_or_default().lines() {
        let fields: Vec<&str> = record.split('\t').collect();
        match fields.first().copied() {
            Some("f") if fields.len() == 2 => {
                if let Ok(line) = fields[1].parse::<usize>() {
                    function_hits.insert(line);
                }
            }
            Some("b") if fields.len() == 5 => {
                if let (Ok(taken), Ok(not_taken)) =
                    (fields[3].parse::<u64>(), fields[4].parse::<u64>())
                {
                    branches.push(CoverageBranchRow {
                        id: fields[1].to_string(),
                        function: fields[2].to_string(),
                        taken,
                        not_taken,
                    });
                }
            }
            _ => {}
        }
    }
    let _ = fs::remove_file(cov_out);
    let funcs = jet::coverable_functions(file, profile, setting_overrides);
    let mut by_line = funcs.clone();
    by_line.sort_by_key(|(_, line)| *line);
    branches.sort_by(|left, right| left.id.cmp(&right.id));

    if mode.json {
        let functions = by_line
            .iter()
            .map(|(name, line)| {
                format!(
                    "{{\"name\":\"{}\",\"line\":{},\"covered\":{}}}",
                    json_escape(name),
                    line,
                    function_hits.contains(line)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let branch_rows = branches
            .iter()
            .flat_map(|branch| {
                [
                    format!(
                        "{{\"id\":\"{}\",\"function\":\"{}\",\"outcome\":\"taken\",\"hits\":{}}}",
                        json_escape(&branch.id),
                        json_escape(&branch.function),
                        branch.taken
                    ),
                    format!(
                        "{{\"id\":\"{}\",\"function\":\"{}\",\"outcome\":\"not-taken\",\"hits\":{}}}",
                        json_escape(&branch.id),
                        json_escape(&branch.function),
                        branch.not_taken
                    ),
                ]
            })
            .collect::<Vec<_>>()
            .join(",");
        let payload = format!(
            "{{\"file\":\"{}\",\"functions\":[{}],\"branches\":[{}]}}",
            json_escape(file),
            functions,
            branch_rows
        );
        write_mode_machine(
            mode,
            &format!(
                "{}\n",
                render_status(
                    "coverage",
                    true,
                    StatusFields::new().with(
                        "coverage",
                        StatusValue::parse(&payload)
                            .expect("coverage projection must be valid JSON"),
                    ),
                )
            ),
        );
        return;
    }

    if funcs.is_empty() && branches.is_empty() {
        render!("\ncoverage: no functions to measure");
        return;
    }

    if funcs.is_empty() {
        render!("\ncoverage: no functions to measure");
    } else {
        render!("\ncoverage for {}", file);
        let mut covered_functions = 0usize;
        for (name, line) in &by_line {
            let hit = function_hits.contains(line);
            if hit {
                covered_functions += 1;
            }
            render!(
                "  {:4}  {}  {}:{}",
                if hit { "HIT " } else { "MISS" },
                name,
                file,
                line
            );
        }
        let total = funcs.len();
        let function_pct = (covered_functions as f64 / total as f64) * 100.0;
        render!(
            "  {}/{} functions covered ({:.0}%)",
            covered_functions,
            total,
            function_pct
        );
    }

    let mut covered_branches = 0usize;
    for branch in &branches {
        for (outcome, hits) in [("taken", branch.taken), ("not-taken", branch.not_taken)] {
            let hit = hits > 0;
            if hit {
                covered_branches += 1;
            }
            render!(
                "  BRANCH {} {:9} {:4} hits={}  {}",
                branch.id,
                outcome,
                if hit { "HIT" } else { "MISS" },
                hits,
                branch.function
            );
        }
    }
    let total_branches = branches.len() * 2;
    let branch_pct = if total_branches == 0 {
        100
    } else {
        (covered_branches * 100) / total_branches
    };
    render!(
        "  {}/{} branches covered ({}%)",
        covered_branches,
        total_branches,
        branch_pct
    );
}

// ─── D-FMTPROJECT1=D: project-level formatter ───────────────────────────────

/// Directories skipped during recursive discovery. Explicit file paths and
/// stdin are NEVER subject to these ignore rules.
const IGNORED_DIRS: &[&str] = &["vendor", "target", "build", ".git", "node_modules", ".jet"];

/// Diagnostic snapshot trees and the syntax catalog are not valid Jet by
/// construction. Directory walks skip them so `jet fmt --check tests` can
/// cover the parseable corpus. Explicit file paths still format (D-FMTPROJECT1=D).
fn skip_fmt_walk_path(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    let s = raw.replace('\\', "/");
    let rel = if let Some((_, rest)) = s.rsplit_once("/tests/") {
        format!("tests/{rest}")
    } else if let Some((_, rest)) = s.rsplit_once("/docs/") {
        format!("docs/{rest}")
    } else {
        s.clone()
    };
    if rel == "docs/spec/reference/syntax-surface.jet"
        || rel.ends_with("/docs/spec/reference/syntax-surface.jet")
        || rel == "syntax-surface.jet"
    {
        return true;
    }
    if rel.starts_with("tests/ui_lint/") || rel == "tests/ui_lint" {
        return true;
    }
    if rel.starts_with("tests/fuzz/sema/invalid/") || rel == "tests/fuzz/sema/invalid" {
        return true;
    }
    if rel.starts_with("tests/ui/") || rel == "tests/ui" {
        // Walk nested directories so `*.fixed.jet` is still formatted.
        if !rel.ends_with(".jet") {
            return false;
        }
        return !rel.ends_with(".fixed.jet");
    }
    if rel.starts_with("tests/fixtures/jetpack-config/")
        || rel.starts_with("tests/fixtures/jetpack-config-real/")
        || rel == "tests/fixtures/jetpack-project/functional-env.jet"
        || rel.starts_with("tests/fixtures/policy_package_unsafe_allow/")
        || rel == "tests/agent_workloads/inputs/build-test-recovery/invalid.jet"
        || rel.ends_with("/ex_concurrency_detached_task.jet")
    {
        return true;
    }
    false
}

/// Recursively collect source `.jet` files under `dir`, skipping IGNORED_DIRS
/// and the retired package manifest. The canonical Package and Config files use
/// the typed package formatter in the preflight path below.
/// Entries are sorted deterministically.
fn walk_jet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !IGNORED_DIRS.contains(&name) && !skip_fmt_walk_path(&path) {
                walk_jet_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(jet::Syntax::FILE_EXT)
            && path.file_name().and_then(|name| name.to_str()) != Some(jet::Syntax::PAYLOAD_FILE)
            && !skip_fmt_walk_path(&path)
        {
            out.push(path);
        }
    }
}

/// Discover the project/workspace root via `package.jet` and collect all `.jet`
/// files under it. Falls back to cwd when no manifest is found above.
fn discover_project_files() -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd);
    let mut files = Vec::new();
    walk_jet_files(&root, &mut files);
    files
}

/// Collect `.jet` files from an explicit list of paths. Directories are walked
/// recursively (IGNORED_DIRS still apply to directory traversal); individual
/// file paths are included as-is (no ignore — explicit path = intentional).
/// Non-existent paths are pushed so the read phase produces a proper I/O
/// error that gets collected by the preflight loop.
fn collect_explicit_files(paths: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for raw in paths {
        let p = PathBuf::from(raw);
        if p.is_dir() {
            walk_jet_files(&p, &mut files);
        } else {
            files.push(p);
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Collect VCS-changed `.jet` files using git. Exits with USAGE (exit 2) when
/// not inside a git repository, with a diagnostic naming the fix.
fn collect_changed_files() -> Vec<PathBuf> {
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !is_git {
        crate::cli_error!(@full "E2104", "`--changed` requires a git repository", "`jet fmt --changed` uses git to find modified .jet files", "run from inside a git repository, or format specific files with `jet fmt <path>`");
        exit(ExitCodes::USAGE);
    }

    let ext = jet::Syntax::FILE_EXT;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut seen: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

    // Modified tracked files vs HEAD (covers staged + unstaged changes to tracked files).
    // Also grab staged-only diffs (new files added to index but not yet committed).
    for git_args in [
        &["diff", "--name-only", "HEAD"][..],
        &["diff", "--name-only", "--cached"][..],
    ] {
        if let Ok(out) = Command::new("git").args(git_args).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    if line.ends_with(&format!(".{}", ext)) {
                        let p = cwd.join(line);
                        if p.is_file() {
                            seen.insert(p);
                        }
                    }
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// JSON-escape a string (backslash, double-quote, newlines).
/// `json_escape` at line 87 covers the same set — this reuses that function.

/// Emit a JSON "ok" result for `--json --check` or successful format with no changes.
fn fmt_json_ok() -> String {
    render_status(
        "fmt",
        true,
        StatusFields::new().with(
            "fmt",
            StatusValue::object(StatusFields::new().with("status", "ok")),
        ),
    )
}

/// Emit a JSON "dirty" result (--check found changes, no --diff).
fn fmt_json_dirty_paths(paths: &[&str]) -> String {
    let files = StatusValue::array(
        paths
            .iter()
            .map(|path| StatusValue::from((*path).to_string())),
    );
    render_status(
        "fmt",
        false,
        StatusFields::new().with(
            "fmt",
            StatusValue::object(
                StatusFields::new()
                    .with("status", "dirty")
                    .with("files", files),
            ),
        ),
    )
}

/// Emit a JSON "dirty" result with unified diffs (--check --diff).
fn fmt_json_dirty_diffs(entries: &[(&str, &str)]) -> String {
    let files = StatusValue::array(entries.iter().map(|(path, diff)| {
        StatusValue::object(
            StatusFields::new()
                .with("path", (*path).to_string())
                .with("diff", (*diff).to_string()),
        )
    }));
    render_status(
        "fmt",
        false,
        StatusFields::new().with(
            "fmt",
            StatusValue::object(
                StatusFields::new()
                    .with("status", "dirty")
                    .with("files", files),
            ),
        ),
    )
}

/// `jet fmt - [--stdin-path=<label>]`: read from stdin, format, write to stdout.
/// Ignore rules do NOT apply. Exit 2 on parse error.
fn run_fmt_stdin(
    stdin_path: Option<&str>,
    explicit_copies: bool,
    simplify: bool,
    mode: OutputMode,
) {
    use std::io::Read;
    let mut src = String::new();
    if std::io::stdin().read_to_string(&mut src).is_err() {
        crate::cli_error!("E2105", "failed to read from stdin");
        exit(ExitCodes::USAGE);
    }
    let label = stdin_path.unwrap_or("<stdin>");
    let (_, retired_target_count) = rewrite_retired_package_targets(&src, label);
    let retired_selector_count = jet::Formatter::retired_interpolation_selector_edits(&src).len();
    let retired_print_count = jet::Formatter::retired_print_family_edits(&src).len();
    let retired_type_count = jet::Formatter::retired_type_edits(&src).len();
    match format_source_for_fmt(
        &src,
        stdin_path.unwrap_or("<stdin>"),
        explicit_copies,
        simplify,
    ) {
        Ok(formatted) => {
            write_mode_renderable(mode, &formatted);
            if retired_target_count > 0 {
                write_mode_status(
                    mode,
                    &format!(
                        "{}: rewrote {} retired target spelling{} from `plugin` to `sandbox` (D-ONCE-SANDBOX1=A)\n",
                        label,
                        retired_target_count,
                        if retired_target_count == 1 { "" } else { "s" }
                    ),
                );
            }
            if retired_selector_count > 0 {
                write_mode_status(
                    mode,
                    &format!(
                        "{}: rewrote {} retired interpolation selector{} from `#` to `:` (D-ONCE-HASH1)\n",
                        label,
                        retired_selector_count,
                        if retired_selector_count == 1 { "" } else { "s" }
                    ),
                );
            }
            if retired_print_count > 0 {
                write_mode_status(
                    mode,
                    &format!(
                        "{}: rewrote {} retired print-family spelling{} (D-ONCE-PRINT1=A)\n",
                        label,
                        retired_print_count,
                        if retired_print_count == 1 { "" } else { "s" }
                    ),
                );
            }
            if retired_type_count > 0 {
                write_mode_status(
                    mode,
                    &format!(
                        "{}: rewrote {} retired Core container name{} (D-COLLNAME1=A)\n",
                        label,
                        retired_type_count,
                        if retired_type_count == 1 { "" } else { "s" }
                    ),
                );
            }
        }
        Err(diags) => {
            let rendered = if mode.json {
                let machine_file = crate::machine_report_path_for_process(label);
                render_diagnostic_status("fmt", false, &machine_file, &src, &diags)
            } else {
                jet::render_diagnostics(label, &src, &diags)
            };
            write_mode_diagnostic(mode, &rendered);
            exit(ExitCodes::USAGE);
        }
    }
}

fn format_source_for_fmt(
    src: &str,
    origin: &str,
    explicit_copies: bool,
    simplify: bool,
) -> Result<String, Vec<jet::Diagnostics::Diagnostic>> {
    // D-ECO-FILEROOT1=A: `package.jet` is the reserved Package root — a typed
    // record (`name: "demo"`), never a module in the compiler's grammar. The
    // module-grammar formatter can only ever reject it (E0003 at the first
    // `field:`), so it is never asked. The file name is the whole fact, and it
    // decides this on every route into the formatter: the directory walk, an
    // explicit path, and `--changed` alike.
    if is_package_manifest_file(origin) {
        return format_package_manifest_for_fmt(src, origin);
    }
    let materialized = if explicit_copies
        && Path::new(origin).is_file()
        && !is_typed_package_source(src, origin)
    {
        // Sema spans point into the source as read. Insert copy verbs before
        // any package-target migration can change those offsets.
        let mut source = src.to_string();
        let mut bundle = jet::Loader::load_entry(origin)?;
        let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        {
            return Err(diagnostics);
        }
        for span in implicit_copy_spans(&bundle, origin).into_iter().rev() {
            if span.start <= source.len() {
                source.insert_str(span.start, jet::Syntax::SIGIL_COPY);
            }
        }
        rewrite_retired_package_targets(&source, origin).0
    } else {
        rewrite_retired_package_targets(src, origin).0
    };
    if is_typed_package_source(&materialized, origin) {
        // A Config binding is valid ordinary Jet syntax too, but its record
        // fields are owned by the Package model. Prefer that formatter before
        // the ordinary AST printer so a semantically valid `name :: Config`
        // file cannot be reshaped into a source form the typed loader rejects.
        if let Ok(formatted) = jet::Package::format_source(&materialized, origin) {
            return Ok(formatted);
        }
    }
    match jet::format_source_with_options(&materialized, jet::Formatter::FormatOptions { simplify })
    {
        Ok(formatted) => Ok(formatted),
        Err(diagnostics) => Err(diagnostics),
    }
}

pub(crate) fn implicit_copy_spans(
    bundle: &jet::AST::ProgramBundle,
    origin: &str,
) -> Vec<jet::Diagnostics::Span> {
    let requested = fs::canonicalize(origin).ok();
    let module = bundle
        .modules
        .iter()
        .find(|module| {
            requested
                .as_ref()
                .is_some_and(|path| fs::canonicalize(&module.path).ok().as_ref() == Some(path))
                || module.display == origin
        })
        .or_else(|| bundle.modules.get(bundle.entry));
    let Some(module) = module else {
        return Vec::new();
    };
    implicit_copy_spans_in_module(module)
}

pub(crate) fn implicit_copy_spans_in_module(
    module: &jet::AST::LoadedModule,
) -> Vec<jet::Diagnostics::Span> {
    let mut spans = Vec::new();
    collect_stmts_implicit_copy_spans(&module.script_body, &mut spans);
    for item in &module.items {
        collect_item_implicit_copy_spans(item, &mut spans);
    }
    spans.sort_by_key(|span| std::cmp::Reverse(span.start));
    spans.dedup_by_key(|span| span.start);
    spans
}

fn collect_stmts_implicit_copy_spans(
    statements: &[jet::AST::Stmt],
    spans: &mut Vec<jet::Diagnostics::Span>,
) {
    for statement in statements {
        let mut statement = statement.clone();
        statement.for_each_expr_mut(|expr| collect_copy_span(expr, spans));
    }
}

fn collect_expr_implicit_copy_spans(
    expression: &jet::AST::Expr,
    spans: &mut Vec<jet::Diagnostics::Span>,
) {
    let mut expression = expression.clone();
    expression.for_each_expr_mut(|expr| collect_copy_span(expr, spans));
}

fn collect_copy_span(expression: &mut jet::AST::Expr, spans: &mut Vec<jet::Diagnostics::Span>) {
    if let jet::AST::Expr::Copy(inner, copy_span) = expression {
        if *copy_span == inner.span() {
            spans.push(*copy_span);
        }
    }
}

fn collect_derive_body_implicit_copy_spans(
    body: &[jet::AST::DeriveBodyItem],
    spans: &mut Vec<jet::Diagnostics::Span>,
) {
    for body_item in body {
        match body_item {
            jet::AST::DeriveBodyItem::Item(item) => {
                collect_item_implicit_copy_spans(item, spans);
            }
            jet::AST::DeriveBodyItem::Stmt(statement) => {
                collect_stmts_implicit_copy_spans(std::slice::from_ref(statement), spans);
            }
            jet::AST::DeriveBodyItem::Loop { source, body, .. } => {
                collect_expr_implicit_copy_spans(source, spans);
                collect_derive_body_implicit_copy_spans(body, spans);
            }
        }
    }
}

fn collect_func_implicit_copy_spans(
    function: &jet::AST::Func,
    spans: &mut Vec<jet::Diagnostics::Span>,
) {
    collect_stmts_implicit_copy_spans(&function.body, spans);
}

fn collect_item_implicit_copy_spans(
    item: &jet::AST::Item,
    spans: &mut Vec<jet::Diagnostics::Span>,
) {
    use jet::AST::Item;
    match item {
        Item::Func(function) => collect_func_implicit_copy_spans(function, spans),
        Item::Struct(definition) => {
            for function in &definition.methods {
                collect_func_implicit_copy_spans(function, spans);
            }
            for implementation in &definition.trait_impls {
                for function in &implementation.methods {
                    collect_func_implicit_copy_spans(function, spans);
                }
            }
        }
        Item::Enum(definition) => {
            for function in &definition.methods {
                collect_func_implicit_copy_spans(function, spans);
            }
            for implementation in &definition.trait_impls {
                for function in &implementation.methods {
                    collect_func_implicit_copy_spans(function, spans);
                }
            }
        }
        Item::Impl(definition) => {
            for function in &definition.methods {
                collect_func_implicit_copy_spans(function, spans);
            }
        }
        Item::Test(definition) => collect_stmts_implicit_copy_spans(&definition.body, spans),
        Item::CodeModule(definition) => {
            if let Some(body) = &definition.body {
                for item in body {
                    collect_item_implicit_copy_spans(item, spans);
                }
            }
        }
        Item::GenericModule(definition) => {
            for item in &definition.body {
                collect_item_implicit_copy_spans(item, spans);
            }
        }
        Item::UserDerive(definition) => {
            collect_derive_body_implicit_copy_spans(&definition.body, spans);
        }
        Item::MarkerDecl(definition) => {
            if let Some(body) = &definition.body {
                collect_derive_body_implicit_copy_spans(body, spans);
            }
            // A marker carries no checked-text clause yet; card #2185 owns
            // adding that field, and this walk returns with it.
        }
        _ => {}
    }
}

fn rewrite_retired_package_targets(src: &str, origin: &str) -> (String, usize) {
    if is_typed_package_source(src, origin) {
        jet::Package::rewrite_retired_targets(src)
    } else {
        (src.to_string(), 0)
    }
}

fn is_typed_package_source(src: &str, origin: &str) -> bool {
    if is_package_manifest_file(origin) {
        return true;
    }
    let trimmed = src.trim_start();
    let value = trimmed
        .strip_prefix("pub ")
        .unwrap_or(trimmed)
        .split_once("::")
        .map(|(_, value)| value.trim_start())
        .or_else(|| trimmed.strip_prefix("Config").map(str::trim_start));
    let Some(value) = value else { return false };
    let Some(value) = value.strip_prefix("Config") else {
        return false;
    };
    let value = value.trim_start();
    value.starts_with(".{") || value.starts_with('{')
}

/// D-ECO-FILEROOT1=A: the one reserved Package root filename. Its `config.jet`,
/// `env.jet` and `workspace.jet` siblings are deliberately *not* in this family:
/// those are `module name { … }` declarations that the ordinary grammar parses
/// (`Parser::Modules::module_decl`) and the ordinary formatter owns.
fn is_package_manifest_file(origin: &str) -> bool {
    Path::new(origin).file_name().and_then(|name| name.to_str()) == Some(jet::Syntax::PACKAGE_FILE)
}

/// Format `package.jet` with the typed Package model — the only formatter that
/// owns manifest text.
///
/// That model fails closed on authored comments instead of dropping them
/// (`tests/fmt.rs`), so a declined layout says this model has no canonical form
/// to offer for this file, not that the file is wrong: `jet fmt` then leaves the
/// manifest exactly as written and `--check` calls it clean. A manifest the
/// Package model cannot read at all still stops the whole run, now with the
/// registered manifest diagnostic (E1206 and its family) that `jet build` gives
/// the same file, instead of module-grammar noise about `:`.
fn format_package_manifest_for_fmt(
    src: &str,
    origin: &str,
) -> Result<String, Vec<jet::Diagnostics::Diagnostic>> {
    let (materialized, _) = jet::Package::rewrite_retired_targets(src);
    let (materialized, _) = jet::Package::rewrite_retired_record_heads(&materialized);
    if let Err(error) = jet::Package::PackageFacts::parse(&materialized, origin) {
        return Err(vec![jet::Manifest::manifest_parse_diagnostic(
            Path::new(origin),
            &error,
        )]);
    }
    match jet::Package::format_source(&materialized, origin) {
        Ok(formatted) => Ok(formatted),
        Err(_declined) => Ok(materialized),
    }
}

/// D-FMTPROJECT1=D: the full project formatter.
///
/// `explicit_paths` — zero or more file/directory paths given on the CLI.
/// `stdin_mode`     — true when `-` was among the path arguments.
/// `stdin_path`     — optional `--stdin-path=<label>` for diagnostics.
/// `check_only`     — `--check`: exit 1 if any file would change, list paths.
/// `show_diff`      — `--diff`: with `--check`, also print unified diffs.
/// `changed_only`   — `--changed`: limit to VCS-changed `.jet` files (git).
/// `simplify`       — `--simplify`: enable ratified simplest-spelling rewrites.
/// `mode`           — output mode (`--json`, color).
struct ExternalFmtTempDir(PathBuf);

impl ExternalFmtTempDir {
    fn new() -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        for attempt in 0..32u32 {
            let path = std::env::temp_dir()
                .join(format!("jet-fmt-{stamp}-{}-{attempt}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("could not allocate a formatter staging directory".to_string())
    }
}

impl Drop for ExternalFmtTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ExternalFmtStagedFile {
    source: PathBuf,
    staged: PathBuf,
    before: Vec<u8>,
}

/// D-ECO12=A: Jet owns the `fmt` verb. For a non-Jet language, it discovers
/// the formatter fact locally, stages the inputs, and asks the Jetpack engine
/// only to realize the environment and execute that formatter.
pub(crate) fn run_external_fmt(raw: &[String], mode: OutputMode) -> i32 {
    let mut language = None;
    let mut explicit_paths = Vec::new();
    let mut child_flags = Vec::new();
    let mut preset = None;
    let mut environment = None;
    let mut check_only = false;
    let mut show_diff = false;
    let mut changed_only = false;
    let mut index = 1;
    while index < raw.len() {
        let arg = &raw[index];
        match arg.as_str() {
            "--lang" => {
                index += 1;
                language = raw.get(index).cloned();
            }
            a if a.starts_with("--lang=") => {
                language = Some(a.trim_start_matches("--lang=").to_string());
            }
            "--check" => check_only = true,
            "--diff" => {
                check_only = true;
                show_diff = true;
            }
            "--dry-run" => {
                check_only = true;
                show_diff = true;
            }
            "--changed" => changed_only = true,
            "--fixtures" | "--preset" | "--env" => {
                index += 1;
                let Some(value) = raw.get(index).cloned() else {
                    crate::cli_error!(
                        @full "E2104",
                        format!("{arg} needs a value"),
                        "these options select the realized environment",
                        "pass a value after the option"
                    );
                    return ExitCodes::USAGE;
                };
                match arg.as_str() {
                    "--fixtures" => {
                        child_flags.push(arg.clone());
                        child_flags.push(value);
                    }
                    "--preset" => {
                        preset = Some(value.clone());
                        child_flags.push(arg.clone());
                        child_flags.push(value);
                    }
                    "--env" => {
                        environment = Some(value.clone());
                        child_flags.push(arg.clone());
                        child_flags.push(value);
                    }
                    _ => unreachable!(),
                }
            }
            a if a.starts_with("--fixtures=") => {
                let value = a.trim_start_matches("--fixtures=").to_string();
                child_flags.extend(["--fixtures".to_string(), value]);
            }
            a if a.starts_with("--preset=") => {
                let value = a.trim_start_matches("--preset=").to_string();
                preset = Some(value.clone());
                child_flags.extend(["--preset".to_string(), value]);
            }
            a if a.starts_with("--env=") => {
                let value = a.trim_start_matches("--env=").to_string();
                environment = Some(value.clone());
                child_flags.extend(["--env".to_string(), value]);
            }
            "--trust" | "--offline" | "--online" | "--no-color" | "--json" | "--flake"
            | "--pure" | "--yes" | "-y" => child_flags.push(arg.clone()),
            a if a.starts_with("--color=") => child_flags.push(arg.clone()),
            "--" => {
                crate::cli_error!(
                    @full "E2104",
                    "jet fmt --lang does not accept a command separator",
                    "Jet supplies the staged files to the environment formatter",
                    "pass formatter input paths before `--`"
                );
                return ExitCodes::USAGE;
            }
            a if a.starts_with('-') => {
                crate::cli_error!(
                    @full "E2102",
                    format!("unknown external formatter option `{a}`"),
                    "Jet forwards only environment selection and formatter mode flags",
                    "run `jet fmt --help` or remove the unknown option"
                );
                return ExitCodes::USAGE;
            }
            _ => explicit_paths.push(arg.clone()),
        }
        index += 1;
    }

    let Some(language) = language
        .as_deref()
        .map(str::trim)
        .map(|value| value.trim_start_matches('.'))
        .filter(|value| {
            !value.is_empty()
                && !value.chars().any(char::is_whitespace)
                && !value.contains('/')
                && !value.contains('\\')
        })
        .map(str::to_string)
    else {
        crate::cli_error!(
            @full "E2104",
            "jet fmt needs --lang <language>",
            "non-Jet formatting is selected by a file-language name",
            "run `jet fmt --lang nix` or omit `--lang` for Jet files"
        );
        return ExitCodes::USAGE;
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(project_dir) = jetpack::EnvHook::find_env_root(&cwd) else {
        crate::cli_error!(
            @full "E1340",
            "jet fmt --lang needs an env.jet",
            "the external formatter is an environment package fact",
            "run from a realized environment project containing env.jet"
        );
        return ExitCodes::USER_ERROR;
    };
    let env_path = project_dir.join(jet::Syntax::ENV_FILE);
    let source = match fs::read_to_string(&env_path) {
        Ok(source) => source,
        Err(error) => {
            crate::cli_error!(
                @full "E1340",
                format!("could not read `{}`", env_path.display()),
                error.to_string(),
                "fix the environment file and retry"
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let plan = match jet_env_model::ModuleEval::evaluate_env_with_selections(
        &source,
        &project_dir,
        preset.as_deref(),
        environment.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            write_mode_diagnostic(
                mode,
                &jet::Diagnostics::render_all(
                    jet::Syntax::ENV_FILE,
                    &source,
                    std::slice::from_ref(&diagnostic),
                ),
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let Some(formatter) = plan.lifecycle.formatter.as_ref() else {
        crate::cli_error!(
            @full "E1340",
            format!("no formatter is declared for language {language}"),
            "jet fmt --lang delegates through the selected environment formatter fact",
            "declare a formatter package in the selected env module"
        );
        return ExitCodes::USER_ERROR;
    };
    let Some(formatter_program) = external_formatter_program(&formatter.package) else {
        crate::cli_error!(
            @full "E1340",
            "the environment formatter package has no executable name",
            "Jetpack realizes formatter packages, and Jet invokes their package name",
            "use a formatter package such as `pkgs.nixfmt`"
        );
        return ExitCodes::USER_ERROR;
    };

    let mut files = match collect_external_fmt_files(&cwd, &project_dir, &explicit_paths, &language)
    {
        Ok(files) => files,
        Err(error) => {
            crate::cli_error!(
                @full "E1340",
                "could not discover formatter input files",
                error,
                "fix the named path or run the formatter from the environment root"
            );
            return ExitCodes::USAGE;
        }
    };
    if changed_only {
        let changed = match external_fmt_changed_files(&project_dir) {
            Ok(changed) => changed,
            Err(error) => {
                crate::cli_error!(
                    @full "E1340",
                    "--changed needs a readable Git worktree",
                    error,
                    "run inside a Git worktree or remove --changed"
                );
                return ExitCodes::USAGE;
            }
        };
        files.retain(|path| {
            path.strip_prefix(&project_dir)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .is_some_and(|relative| changed.contains(&relative))
        });
    }
    if files.is_empty() {
        if mode.json {
            write_mode_machine(mode, &format!("{}\n", fmt_json_ok()));
        }
        return ExitCodes::OK;
    }

    let temp = match ExternalFmtTempDir::new() {
        Ok(temp) => temp,
        Err(error) => {
            crate::cli_error!(
                @full "E1340",
                "could not stage formatter inputs",
                error,
                "fix the temporary directory and retry"
            );
            return ExitCodes::USAGE;
        }
    };
    let mut staged = Vec::with_capacity(files.len());
    let mut command = vec!["env".to_string()];
    command.extend(child_flags);
    command.push("--".to_string());
    command.push(formatter_program);
    for (index, source_path) in files.iter().enumerate() {
        let before = match fs::read(source_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                crate::cli_error!(
                    @full "E1340",
                    format!("could not read `{}`", source_path.display()),
                    error.to_string(),
                    "fix the file permissions and retry"
                );
                return ExitCodes::USAGE;
            }
        };
        let name = source_path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed("input"));
        let staged_path = temp.0.join(format!("{index:04}-{name}"));
        if let Err(error) = fs::write(&staged_path, &before) {
            crate::cli_error!(
                @full "E1340",
                "could not stage formatter inputs",
                error.to_string(),
                "fix the temporary directory and retry"
            );
            return ExitCodes::USAGE;
        }
        command.push(staged_path.to_string_lossy().into_owned());
        staged.push(ExternalFmtStagedFile {
            source: source_path.clone(),
            staged: staged_path,
            before,
        });
    }

    let code = crate::EngineDispatch::dispatch(jet::Syntax::JETPACK_BINARY_NAME, "env", &command);
    if code != ExitCodes::OK {
        return code;
    }

    let mut changed = Vec::new();
    for file in staged {
        let after = match fs::read(&file.staged) {
            Ok(bytes) => bytes,
            Err(error) => {
                crate::cli_error!(
                    @full "E1340",
                    "environment formatter did not produce readable output",
                    error.to_string(),
                    "check the formatter package and retry"
                );
                return ExitCodes::USER_ERROR;
            }
        };
        if file.before != after {
            changed.push((file.source, file.before, after));
        }
    }
    if changed.is_empty() {
        if mode.json {
            write_mode_machine(mode, &format!("{}\n", fmt_json_ok()));
        }
        return ExitCodes::OK;
    }
    if check_only {
        let paths = changed
            .iter()
            .map(|(path, _, _)| external_fmt_display_path(&project_dir, path))
            .collect::<Vec<_>>();
        if mode.json {
            if show_diff {
                let diff_strings = changed
                    .iter()
                    .zip(paths.iter())
                    .map(|((_, before, after), path)| {
                        let before = String::from_utf8_lossy(before);
                        let after = String::from_utf8_lossy(after);
                        jet::Formatter::unified_diff(path, &before, &after)
                    })
                    .collect::<Vec<_>>();
                let diffs = paths
                    .iter()
                    .zip(diff_strings.iter())
                    .map(|(path, diff)| (path.as_str(), diff.as_str()))
                    .collect::<Vec<_>>();
                write_mode_machine(mode, &format!("{}\n", fmt_json_dirty_diffs(&diffs)));
            } else {
                let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
                write_mode_machine(mode, &format!("{}\n", fmt_json_dirty_paths(&paths)));
            }
        } else {
            for ((_, before, after), path) in changed.iter().zip(paths.iter()) {
                write_mode_renderable(mode, &format!("{path}\n"));
                if show_diff {
                    let before = String::from_utf8_lossy(before);
                    let after = String::from_utf8_lossy(after);
                    write_mode_renderable(
                        mode,
                        &jet::Formatter::unified_diff(path, &before, &after),
                    );
                }
            }
        }
        return ExitCodes::USER_ERROR;
    }
    for (path, _, after) in changed {
        if let Err(error) = fs::write(&path, after) {
            crate::cli_error!(
                @full "E1340",
                format!("could not write `{}`", path.display()),
                error.to_string(),
                "fix the file permissions and rerun the formatter"
            );
            return ExitCodes::USAGE;
        }
    }
    if mode.json {
        write_mode_machine(mode, &format!("{}\n", fmt_json_ok()));
    }
    ExitCodes::OK
}

fn external_formatter_program(package: &str) -> Option<String> {
    let package = package.split('@').next().unwrap_or(package);
    let package = package.split('#').next().unwrap_or(package);
    let program = package.rsplit(['/', ':']).next().unwrap_or(package).trim();
    (!program.is_empty() && !program.chars().any(char::is_whitespace)).then(|| program.to_string())
}

fn collect_external_fmt_files(
    cwd: &Path,
    project_dir: &Path,
    explicit_paths: &[String],
    language: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut files = BTreeSet::new();
    if explicit_paths.is_empty() {
        walk_external_fmt_files(project_dir, language, &mut files);
    } else {
        for raw in explicit_paths {
            let path = Path::new(raw);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if metadata.is_dir() {
                walk_external_fmt_files(&path, language, &mut files);
            } else if metadata.is_file() && matches_external_language(&path, language) {
                files.insert(path);
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn walk_external_fmt_files(dir: &Path, language: &str, files: &mut BTreeSet<PathBuf>) {
    const IGNORED: &[&str] = &[".git", ".jet", "target", "build", "vendor", "node_modules"];
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let ignored = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| IGNORED.contains(&name));
            if !ignored {
                walk_external_fmt_files(&path, language, files);
            }
        } else if kind.is_file() && matches_external_language(&path, language) {
            files.insert(path);
        }
    }
}

fn matches_external_language(path: &Path, language: &str) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some(language)
}

fn external_fmt_changed_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut changed = BTreeSet::new();
    for args in [
        vec!["diff", "--name-only", "HEAD"],
        vec!["diff", "--name-only", "--cached"],
        vec!["ls-files", "--others", "--exclude-standard"],
    ] {
        let output = Command::new("git")
            .args(&args)
            .current_dir(root)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() && args.first() == Some(&"diff") {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let relative = line.trim().replace('\\', "/");
            if !relative.is_empty() && root.join(&relative).is_file() {
                changed.insert(relative);
            }
        }
    }
    Ok(changed)
}

fn external_fmt_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

pub(crate) fn run_fmt(
    explicit_paths: &[String],
    stdin_mode: bool,
    stdin_path: Option<&str>,
    check_only: bool,
    show_diff: bool,
    changed_only: bool,
    explicit_copies: bool,
    simplify: bool,
    mode: OutputMode,
) {
    if stdin_mode {
        run_fmt_stdin(stdin_path, explicit_copies, simplify, mode);
        return;
    }

    // Collect the set of files to operate on.
    let files: Vec<PathBuf> = if changed_only {
        collect_changed_files()
    } else if explicit_paths.is_empty() {
        discover_project_files()
    } else {
        collect_explicit_files(explicit_paths)
    };

    if files.is_empty() {
        if mode.json {
            write_mode_machine(mode, &format!("{}\n", fmt_json_ok()));
        }
        return;
    }

    // Preflight: format ALL files before writing ANY. Collect results so that
    // a single parse or I/O failure aborts the whole batch with no writes.
    struct FileResult {
        path: PathBuf,
        original: String,
        formatted: String,
        changed: bool,
        retired_interpolation_selectors: usize,
        retired_print_family_spellings: usize,
        retired_type_names: usize,
        retired_target_spellings: usize,
        io_error: Option<String>,
        parse_diags: Vec<jet::Diagnostics::Diagnostic>,
    }

    let mut results: Vec<FileResult> = Vec::with_capacity(files.len());
    for path in &files {
        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                results.push(FileResult {
                    path: path.clone(),
                    original: String::new(),
                    formatted: String::new(),
                    changed: false,
                    retired_interpolation_selectors: 0,
                    retired_print_family_spellings: 0,
                    retired_type_names: 0,
                    retired_target_spellings: 0,
                    io_error: Some(format!("can't read `{}`: {}", path.display(), e)),
                    parse_diags: Vec::new(),
                });
                continue;
            }
        };
        match format_source_for_fmt(&src, &path.display().to_string(), explicit_copies, simplify) {
            Ok(formatted) => {
                let changed = formatted != src;
                let retired_interpolation_selectors =
                    jet::Formatter::retired_interpolation_selector_edits(&src).len();
                let retired_print_family_spellings =
                    jet::Formatter::retired_print_family_edits(&src).len();
                let retired_type_names = jet::Formatter::retired_type_edits(&src).len();
                let (_, retired_target_spellings) =
                    rewrite_retired_package_targets(&src, &path.display().to_string());
                results.push(FileResult {
                    path: path.clone(),
                    original: src,
                    formatted,
                    changed,
                    retired_interpolation_selectors,
                    retired_print_family_spellings,
                    retired_type_names,
                    retired_target_spellings,
                    io_error: None,
                    parse_diags: Vec::new(),
                });
            }
            Err(diags) => {
                results.push(FileResult {
                    path: path.clone(),
                    original: src,
                    formatted: String::new(),
                    changed: false,
                    retired_interpolation_selectors: 0,
                    retired_print_family_spellings: 0,
                    retired_type_names: 0,
                    retired_target_spellings: 0,
                    io_error: None,
                    parse_diags: diags,
                });
            }
        }
    }

    // If ANY file failed, report every failure and exit 2 — nothing is written.
    let has_errors = results
        .iter()
        .any(|r| r.io_error.is_some() || !r.parse_diags.is_empty());
    if has_errors {
        for r in &results {
            if let Some(ref io_err) = r.io_error {
                if mode.json {
                    let path_s = r.path.to_str().unwrap_or("?");
                    write_mode_diagnostic(
                        mode,
                        &format!(
                            "{}\n",
                            render_status(
                                "fmt",
                                false,
                                StatusFields::new().with(
                                    "fmt",
                                    StatusValue::object(
                                        StatusFields::new().with("status", "error").with(
                                            "errors",
                                            StatusValue::array([StatusValue::object(
                                                StatusFields::new()
                                                    .with("path", path_s.to_string())
                                                    .with("message", io_err.to_string()),
                                            )]),
                                        ),
                                    ),
                                ),
                            )
                        ),
                    );
                } else {
                    crate::cli_error!("E2105", "{}", io_err);
                }
            }
            if !r.parse_diags.is_empty() {
                let path_s = r.path.to_str().unwrap_or("?");
                if mode.json {
                    let machine_file = jet::Diagnostics::ReportPath::from_process(path_s);
                    write_mode_diagnostic(
                        mode,
                        &render_diagnostic_status(
                            "fmt",
                            false,
                            &machine_file,
                            &r.original,
                            &r.parse_diags,
                        ),
                    );
                } else {
                    write_mode_diagnostic(
                        mode,
                        &jet::render_diagnostics(path_s, &r.original, &r.parse_diags),
                    );
                }
            }
        }
        exit(if check_only {
            ExitCodes::USER_ERROR
        } else {
            ExitCodes::USAGE
        });
    }

    // Root for root-relative path display in --check output.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let display_root = jet::Loader::find_manifest_root(&cwd).unwrap_or_else(|| cwd.clone());
    let make_rel = |p: &Path| -> String {
        p.strip_prefix(&display_root)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| p.display().to_string())
    };

    if check_only {
        // --check: report which files would change (sorted root-relative), exit 1 if any.
        let mut dirty: Vec<&FileResult> = results.iter().filter(|r| r.changed).collect();
        dirty.sort_by(|a, b| make_rel(&a.path).cmp(&make_rel(&b.path)));

        if dirty.is_empty() {
            if mode.json {
                write_mode_machine(mode, &format!("{}\n", fmt_json_ok()));
            }
            return; // exit 0
        }

        if mode.json {
            if show_diff {
                let entries: Vec<(String, String)> = dirty
                    .iter()
                    .map(|r| {
                        let rel = make_rel(&r.path);
                        let diff = jet::Formatter::unified_diff(&rel, &r.original, &r.formatted);
                        (rel, diff)
                    })
                    .collect();
                let refs: Vec<(&str, &str)> = entries
                    .iter()
                    .map(|(p, d)| (p.as_str(), d.as_str()))
                    .collect();
                write_mode_machine(mode, &format!("{}\n", fmt_json_dirty_diffs(&refs)));
            } else {
                let paths: Vec<&str> = dirty
                    .iter()
                    .map(|r| r.path.to_str().unwrap_or("?"))
                    .collect();
                write_mode_machine(mode, &format!("{}\n", fmt_json_dirty_paths(&paths)));
            }
        } else {
            for r in &dirty {
                write_mode_renderable(mode, &format!("{}\n", make_rel(&r.path)));
                if show_diff {
                    let rel = make_rel(&r.path);
                    write_mode_renderable(
                        mode,
                        &jet::Formatter::unified_diff(&rel, &r.original, &r.formatted),
                    );
                }
            }
        }
        exit(ExitCodes::USER_ERROR); // exit 1 — --check found changes
    }

    // Format mode: write all changed files (preflight passed, so all are valid).
    for r in results.iter().filter(|r| r.changed) {
        if let Err(e) = fs::write(&r.path, &r.formatted) {
            crate::cli_error!("E2105", "couldn't write `{}`: {}", r.path.display(), e);
            exit(ExitCodes::USAGE);
        }
        if r.retired_interpolation_selectors > 0 {
            write_mode_status(
                mode,
                &format!(
                    "{}: rewrote {} retired interpolation selector{} from `#` to `:` (D-ONCE-HASH1)\n",
                    r.path.display(),
                    r.retired_interpolation_selectors,
                    if r.retired_interpolation_selectors == 1 { "" } else { "s" }
                ),
            );
        }
        if r.retired_print_family_spellings > 0 {
            write_mode_status(
                mode,
                &format!(
                    "{}: rewrote {} retired print-family spelling{} from `#` to `:` (D-ONCE-PRINT1=A)\n",
                    r.path.display(),
                    r.retired_print_family_spellings,
                    if r.retired_print_family_spellings == 1 { "" } else { "s" }
                ),
            );
        }
        if r.retired_target_spellings > 0 {
            write_mode_status(
                mode,
                &format!(
                    "{}: rewrote {} retired target spelling{} from `plugin` to `sandbox` (D-ONCE-SANDBOX1=A)\n",
                    r.path.display(),
                    r.retired_target_spellings,
                    if r.retired_target_spellings == 1 { "" } else { "s" }
                ),
            );
        }
        if r.retired_type_names > 0 {
            write_mode_status(
                mode,
                &format!(
                    "{}: rewrote {} retired Core container name{} (D-COLLNAME1=A)\n",
                    r.path.display(),
                    r.retired_type_names,
                    if r.retired_type_names == 1 { "" } else { "s" }
                ),
            );
        }
    }
    // exit 0 implicitly
}

pub(crate) fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string())
        .replace('.', "_")
}

fn bin_path(file: &str) -> PathBuf {
    PathBuf::from("build").join(stem(file))
}

fn programmable_build_target_name(output: &jet::Driver::BuildCompileOutput) -> Option<String> {
    let build = output.build.as_ref()?;
    let target_id = build.plan.default_target()?.id().0;
    let target = build.plan.targets().get(target_id)?;
    (target.kind == jet::Comptime::Build::TargetKind::Executable).then(|| target.name.clone())
}

fn build_artifact_path(file: &str, target_name: Option<&str>) -> PathBuf {
    let Some(target_name) = target_name else {
        return bin_path(file);
    };
    let path = Path::new(target_name);
    let mut components = path.components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return bin_path(file);
    }
    PathBuf::from("build").join(target_name)
}

fn test_bin_path(path: &Path) -> PathBuf {
    PathBuf::from("build").join(format!(
        ".test_{}.{}",
        stem(&path.to_string_lossy()),
        std::process::id()
    ))
}

/// `jet fuzz` options are retained only so older dispatch code can emit the
/// migration diagnostic. The harness itself is retired; `jet test` owns
/// generated-assertion execution.
#[derive(Clone, Default)]
pub(crate) struct FuzzRunOpts {
    pub(crate) iterations: Option<u64>,
    pub(crate) time_budget_ms: Option<u64>,
    pub(crate) seed: Option<u64>,
    pub(crate) corpus: Option<String>,
}

/// Retire the pre-ratification standalone fuzz harness.
///
/// Generated property evidence is now part of the ordinary `jet test`
/// contract. Keep this entry point until the top-level dispatcher migrates so
/// old invocations fail with an actionable replacement rather than compiling
/// or executing a second harness.
pub(crate) fn run_fuzz(file: &str, test_name: Option<&str>, opts: FuzzRunOpts, mode: OutputMode) {
    let mut replacement = format!("{} test --grade=generated", jet::Syntax::BINARY_NAME);
    if let Some(iterations) = opts.iterations {
        replacement.push_str(&format!(" --iterations={iterations}"));
    }
    replacement.push(' ');
    replacement.push_str(file);
    if let Some(test_name) = test_name {
        replacement.push(' ');
        replacement.push_str(test_name);
    }
    let _ = (opts.time_budget_ms, opts.seed, opts.corpus, mode);
    crate::cli_error!("E2104", "`jet fuzz` is retired; use `{}`", replacement);
    exit(ExitCodes::USAGE);
}

/// D-BUILDNORM1=A (Tower #85): a semantic SHA-256 of the enclosing Package root's typed facts and selected build entry.
///
/// Comments and formatting are not Package meaning, so they do not change this identity. The selected `fn build` is loaded as its own AST when it lives outside the runtime bundle; its parsed import closure is included so compiler-host edits cannot reuse a stale native artifact.
/// An unreadable or invalid manifest/build entry returns `None`, disabling cache reuse rather than guessing. A manifest-less file uses an empty identity.
fn package_build_fingerprint(file: &str) -> Result<Option<String>, ()> {
    let Some((root, facts)) = load_pkg_manifest(file) else {
        return Ok(None);
    };
    let resolver = jet::Authority::AuthorityResolver::open(&root).map_err(|_| ())?;
    let entry = facts
        .resolve_build_entry_checked(&resolver)
        .map_err(|_| ())?;
    let Some(entry) = entry else {
        return Ok(None);
    };
    let raw = entry.text().map_err(|_| ())?;
    let source = if entry.path.file_name().and_then(|name| name.to_str())
        == Some(jet::Syntax::PACKAGE_FILE)
    {
        jet::Package::build_entry_source(&raw).unwrap_or(raw)
    } else {
        raw
    };
    let entry_path = entry.path.to_string_lossy().into_owned();
    let bundle =
        jet::Loader::load_entry_with_overlay(&entry_path, Some((&entry.path, &source)), false)
            .map_err(|_| ())?;
    Ok(Some(jet::SHA256::sha256_hex(
        &jet::CanonicalAST::canonical_bytes(&bundle),
    )))
}

fn manifest_fingerprint(file: &str) -> Option<String> {
    let search_from = Path::new(file).parent().unwrap_or(Path::new("."));
    let package_root = match jet::Loader::find_package_root_checked(search_from) {
        Ok(root) => root,
        Err(_) => return None,
    };
    let Some((_, facts)) = load_pkg_manifest(file) else {
        return package_root.is_none().then(String::new);
    };
    let build = package_build_fingerprint(file).ok()?;
    let mut bytes = b"jet.package-semantic.v2\0".to_vec();
    append_cache_field(&mut bytes, &facts.semantic_digest());
    append_cache_field(&mut bytes, build.as_deref().unwrap_or_default());
    Some(jet::SHA256::sha256_hex(&bytes))
}
fn debug_native_cache_event(event: impl AsRef<str>) {
    let Ok(path) = std::env::var("JET_DEBUG_NATIVE_CACHE_LOG") else {
        return;
    };
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = std::io::Write::write_all(&mut log, format!("{}\n", event.as_ref()).as_bytes());
    }
}

fn append_cache_field(bytes: &mut Vec<u8>, value: &str) {
    let value = value.as_bytes();
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}
fn append_link_identity(bytes: &mut Vec<u8>, links: &[String], root: Option<&Path>) {
    for link in links {
        append_cache_field(bytes, &normalize_link_argument(link, root));
    }

    let mut artifacts = Vec::new();
    let mut index = 0;
    while index < links.len() {
        let argument = &links[index];
        let raw_path = if argument == "-L" {
            links
                .get(index + 1)
                .and_then(|value| value.strip_prefix("native="))
        } else {
            argument.strip_prefix("native=")
        };
        if let Some(raw_path) = raw_path {
            collect_link_artifacts(&resolve_link_path(raw_path, root), root, &mut artifacts);
        }
        if argument == "-L" {
            index += 2;
        } else {
            index += 1;
        }
    }

    for link in links {
        let Some(raw_path) = link
            .strip_prefix("link-arg=")
            .filter(|path| !path.starts_with("-Wl,"))
        else {
            continue;
        };
        collect_link_artifacts(&resolve_link_path(raw_path, root), root, &mut artifacts);
    }

    let mut identities = artifacts
        .into_iter()
        .map(|path| {
            let path = fs::canonicalize(&path).unwrap_or(path);
            let logical = stable_link_path(&path, root);
            let digest = fs::read(&path)
                .map(|contents| jet::SHA256::sha256_hex(&contents))
                .unwrap_or_else(|_| "unreadable".to_string());
            (logical, digest)
        })
        .collect::<Vec<_>>();
    identities.sort();
    identities.dedup();
    append_cache_field(bytes, "jet-linked-artifacts.v1");
    for (path, digest) in identities {
        append_cache_field(bytes, &path);
        append_cache_field(bytes, &digest);
    }
}

fn collect_link_artifacts(path: &Path, root: Option<&Path>, artifacts: &mut Vec<PathBuf>) {
    let Some(root) = root else {
        return;
    };
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !path.starts_with(root) {
        return;
    }
    if path.is_file() {
        if is_link_artifact(&path) {
            artifacts.push(path);
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let candidate = entry.path();
        if candidate.is_file() && is_link_artifact(&candidate) {
            artifacts.push(candidate);
        }
    }
}

fn is_link_artifact(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        matches!(
            extension.to_str(),
            Some("a" | "so" | "dylib" | "dll" | "lib")
        )
    })
}

fn resolve_link_path(raw: &str, root: Option<&Path>) -> PathBuf {
    let path = Path::new(raw);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(root) = root {
        root.join(path)
    } else {
        path.to_path_buf()
    };
    fs::canonicalize(&path).unwrap_or(path)
}

fn stable_link_path(path: &Path, root: Option<&Path>) -> String {
    let rendered = root
        .and_then(|root| path.strip_prefix(root).ok())
        .map(|relative| format!("project/{}", relative.to_string_lossy()))
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    rendered.replace('\\', "/")
}

fn normalize_link_argument(argument: &str, root: Option<&Path>) -> String {
    for marker in ["native=", "dependency=", "link-arg=-Wl,-rpath,"] {
        if let Some(raw_path) = argument.strip_prefix(marker) {
            let path = resolve_link_path(raw_path, root);
            return format!("{marker}{}", stable_link_path(&path, root));
        }
    }
    if let Some(raw_path) = argument
        .strip_prefix("link-arg=")
        .filter(|path| !path.starts_with("-Wl,"))
    {
        let path = resolve_link_path(raw_path, root);
        return format!("link-arg={}", stable_link_path(&path, root));
    }
    argument.to_string()
}

fn native_cache_salt(
    toolchain: &str,
    dependency_fingerprint: &str,
    runtime_fingerprint: &str,
    corelib_fingerprint: &str,
    mode: &str,
    target: &str,
    instance_fingerprints: &[String],
    bridge_identity: Option<&str>,
    comptime_inputs: &[jet::AST::ComptimeInput],
    authority_identity: Option<&str>,
) -> String {
    native_cache_salt_with_schema(
        NATIVE_CACHE_SALT_SCHEMA,
        toolchain,
        dependency_fingerprint,
        runtime_fingerprint,
        corelib_fingerprint,
        mode,
        target,
        instance_fingerprints,
        bridge_identity,
        comptime_inputs,
        authority_identity,
    )
}
fn native_cache_salt_with_schema(
    schema: &[u8],
    toolchain: &str,
    dependency_fingerprint: &str,
    runtime_fingerprint: &str,
    corelib_fingerprint: &str,
    mode: &str,
    target: &str,
    instance_fingerprints: &[String],
    bridge_identity: Option<&str>,
    comptime_inputs: &[jet::AST::ComptimeInput],
    authority_identity: Option<&str>,
) -> String {
    let mut instances = instance_fingerprints.to_vec();
    instances.sort();
    let mut inputs = comptime_inputs
        .iter()
        .map(|input| (&input.path, &input.hash))
        .collect::<Vec<_>>();
    inputs.sort_unstable();
    // Package-view reads carry a package-relative path and content hash. Sort
    // them so query order cannot change identity, while either field can.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(schema);
    for value in [
        toolchain,
        dependency_fingerprint,
        runtime_fingerprint,
        corelib_fingerprint,
        mode,
        target,
        bridge_identity.unwrap_or("none"),
    ] {
        append_cache_field(&mut bytes, value);
    }
    bytes.extend_from_slice(&(instances.len() as u64).to_be_bytes());
    for instance in instances {
        append_cache_field(&mut bytes, &instance);
    }
    bytes.extend_from_slice(&(inputs.len() as u64).to_be_bytes());
    for (path, hash) in inputs {
        append_cache_field(&mut bytes, path);
        append_cache_field(&mut bytes, hash);
    }
    if let Some(authority_identity) = authority_identity {
        append_cache_field(&mut bytes, "application-authority");
        append_cache_field(&mut bytes, authority_identity);
    }
    jet::SHA256::sha256_hex(&bytes)
}

const APPLICATION_AUTHORITY_CACHE_SCHEMA: &[u8] = b"jet-application-authority-cache-v1";
const NATIVE_CACHE_SALT_SCHEMA: &[u8] = b"jet-native-cache-salt-v7";
const NATIVE_CACHE_COMPILER_ABI: &str = "jet.native-cache-abi.v5";

fn application_authority_cache_identity(
    authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    let authority = authority?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(APPLICATION_AUTHORITY_CACHE_SCHEMA);
    append_cache_field(&mut bytes, &authority.authority);
    for (name, holds) in [
        ("required", &authority.required_effects),
        ("granted", &authority.granted_effects),
        ("denied", &authority.denied_effects),
    ] {
        append_cache_field(&mut bytes, name);
        bytes.extend_from_slice(&(holds.len() as u64).to_be_bytes());
        for hold in holds {
            append_cache_field(&mut bytes, hold);
        }
    }
    Some(jet::SHA256::sha256_hex(&bytes))
}

fn command_identity(program: &str, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) => {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(output.status.code().unwrap_or(-1) as i64).to_be_bytes());
            bytes.extend_from_slice(&output.stdout);
            bytes.extend_from_slice(&output.stderr);
            jet::SHA256::sha256_hex(&bytes)
        }
        Err(error) => format!("unavailable:{:?}", error.kind()),
    }
}

fn rustc_identity() -> (String, String) {
    match Command::new("rustc").arg("-vV").output() {
        Ok(output) => {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(output.status.code().unwrap_or(-1) as i64).to_be_bytes());
            bytes.extend_from_slice(&output.stdout);
            bytes.extend_from_slice(&output.stderr);
            let verbose = String::from_utf8_lossy(&output.stdout);
            let backend = verbose
                .lines()
                .find(|line| line.starts_with("LLVM version:"))
                .unwrap_or("LLVM version: unavailable")
                .to_string();
            (jet::SHA256::sha256_hex(&bytes), backend)
        }
        Err(error) => {
            let unavailable = format!("unavailable:{:?}", error.kind());
            (unavailable.clone(), unavailable)
        }
    }
}

/// Identity of every executable/backend that can change emitted native code.
///
/// `build=` is this compiler's compile-time semantic identity — `build.rs`'s
/// `semantic_id("jet.compiler.v2", COMPILER_SOURCES, build_facts())`, a SHA-256
/// over every non-test source file under `Source/` and `crates/jet-*` (the
/// Prelude and the Core closure included), `Cargo.toml`/`Cargo.lock`, and the
/// `rustc -vV` / TARGET / HOST / PROFILE / OPT_LEVEL / DEBUG / feature /
/// `RUSTFLAGS` / custom-target-spec facts of the build that produced this
/// binary. It moves for exactly the edits that can change emitted code, it
/// distinguishes local/dev compilers whose package SemVer is unchanged, and
/// reading it costs nothing: it is a `&'static str` baked into this binary.
/// `env!` (not `option_env!`) on purpose — a key input must never silently
/// degrade to a constant placeholder.
///
/// #2085: this used to be `sha256(fs::read("/proc/self/exe"))`. On a dev
/// compiler that is an 835 MB read, a second 835 MB copy inside `sha256`, and
/// 13 M scalar compression blocks in an unoptimized build: 33.4 s of the 35.1 s
/// a `jet build` took, paid on every invocation to re-derive a value that is
/// fixed for the life of the binary. `rustc -vV` still rides along because the
/// rustc that compiles the *emitted* Rust need not be the one that built this
/// compiler; it carries its own LLVM backend revision.
static NATIVE_TOOLCHAIN_IDENTITY: LazyLock<String> = LazyLock::new(|| {
    let compiler_build = env!("JET_COMPILER_BUILD_ID");
    let (rustc, backend) = rustc_identity();
    let linker_selection = crate::NativeLinker::for_target(None);
    let (linker_driver, linker_backend_name, linker_backend_program) = linker_selection.identity();
    let linker_name = linker_driver.unwrap_or("system");
    let linker = linker_driver
        .map(|name| command_identity(name, &["--version"]))
        .unwrap_or_else(|| "system".into());
    let linker_backend_name = linker_backend_name.unwrap_or("system");
    let linker_backend_program_name = linker_backend_program.unwrap_or("system");
    let linker_backend = linker_backend_program
        .map(|name| command_identity(name, &["--version"]))
        .unwrap_or_else(|| "system".into());
    format!(
        "abi={NATIVE_CACHE_COMPILER_ABI}\u{1}build={compiler_build}\u{1}version={}\u{1}semindex={}\u{1}rustc-pin={}\u{1}rustc={rustc}\u{1}backend={backend}\u{1}linker-selection={}\u{1}linker-name={linker_name}\u{1}linker={linker}\u{1}linker-backend={}\u{1}linker-backend-name={linker_backend_program_name}\u{1}linker-backend-identity={linker_backend}",
        jet::Manifest::COMPILER_VERSION,
        jet_semindex::SCHEMA_VERSION,
        jet::Doctor::RUSTC_VERSION_PIN,
        linker_selection.label(),
        linker_backend_name,
    )
});

fn native_toolchain_identity() -> &'static str {
    NATIVE_TOOLCHAIN_IDENTITY.as_str()
}

fn dependency_interface_fingerprint(bundle: &jet::AST::ProgramBundle) -> String {
    let mut interfaces = Vec::new();
    for (dependency, root) in &bundle.dep_roots {
        for (module_idx, module) in bundle.modules.iter().enumerate() {
            if !module.path.starts_with(root) {
                continue;
            }
            for item in &module.items {
                let name = match item {
                    jet::AST::Item::Func(def) => Some(def.name.as_str()),
                    jet::AST::Item::Struct(def) => Some(def.name.as_str()),
                    jet::AST::Item::Enum(def) => Some(def.name.as_str()),
                    jet::AST::Item::Trait(def) => Some(def.name.as_str()),
                    jet::AST::Item::Tag(def) => Some(def.name.as_str()),
                    jet::AST::Item::CodeModule(def) => Some(def.name.as_str()),
                    _ => None,
                };
                let public = name.is_some_and(|name| bundle.name_ledger.exported(module_idx, name));
                if public {
                    interfaces.push((
                        dependency.clone(),
                        module.display.clone(),
                        jet::CanonicalAST::canonical_fragment(item),
                    ));
                }
            }
        }
    }
    interfaces.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    let mut bytes = Vec::new();
    for (dependency, module, interface) in interfaces {
        for value in [
            dependency.as_bytes(),
            module.as_bytes(),
            interface.as_slice(),
        ] {
            bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
            bytes.extend_from_slice(value);
        }
    }
    jet::SHA256::sha256_hex(&bytes)
}

/// D-BUILDNORM1=A (Tower #85): the content-cache key for building `file` under
/// `mode_tag` (`"run"`, `"test"`, `"testcov"`, `"dev"`, …), computed
/// from the *pre-sema* canonical AST of the whole program (entry + every module
/// the loader resolved). Returns `None` when:
///
/// - the program can't be loaded/parsed — the caller falls through to the
///   normal compile, which reports the real diagnostic; or
/// - the program uses `embed_file`/`embed_bytes` — its output depends on
///   external file bytes not captured by the AST, so it must never be served
///   from (or stored into) a content cache keyed on the AST alone. Detected by a
///   conservative source-substring scan: a false positive only forgoes caching
///   for that build (a safe perf cost), never serves a stale binary.
///
/// `mode_tag` keeps binaries built from the same AST under different pipelines in
/// separate key spaces (a `jet test` harness binary can never be served for a
/// `jet run`). The toolchain version, `package.jet` fingerprint, and any FFI
/// bridge identity ride the salt.
fn setting_overrides_tag(settings: &BTreeMap<String, String>) -> String {
    if settings.is_empty() {
        return "settings=default".to_string();
    }
    format!(
        "settings={}",
        settings
            .iter()
            .map(|(key, value)| format!("{}:{key}{}:{value}", key.len(), value.len()))
            .collect::<Vec<_>>()
            .join("")
    )
}

fn native_cache_key(
    file: &str,
    profile: &str,
    profile_tag: &str,
    mode_tag: &str,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    native_cache_key_with_toolchain(
        file,
        profile,
        profile_tag,
        mode_tag,
        native_toolchain_identity(),
        invocation_authority,
    )
}
fn native_cache_key_with_source_closure(
    file: &str,
    source_closure: &[(PathBuf, String)],
    profile: &str,
    profile_tag: &str,
    mode_tag: &str,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    let overlays = source_closure
        .iter()
        .map(|(path, source)| (path.as_path(), source.as_str()))
        .collect::<Vec<_>>();
    native_cache_key_with_toolchain_and_overlays(
        file,
        profile,
        profile_tag,
        mode_tag,
        native_toolchain_identity(),
        &overlays,
        invocation_authority,
    )
}
fn native_cache_key_with_toolchain(
    file: &str,
    profile: &str,
    profile_tag: &str,
    mode_tag: &str,
    toolchain_identity: &str,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    native_cache_key_with_toolchain_and_overlays(
        file,
        profile,
        profile_tag,
        mode_tag,
        toolchain_identity,
        &[],
        invocation_authority,
    )
}

fn native_cache_key_with_toolchain_and_overlays(
    file: &str,
    profile: &str,
    profile_tag: &str,
    mode_tag: &str,
    toolchain_identity: &str,
    overlays: &[(&Path, &str)],
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    debug_native_cache_event(format!(
        "key-entry pid={} file={} profile={} mode={}",
        std::process::id(),
        file,
        profile_tag,
        mode_tag,
    ));
    // `jet prove` consumes a compiler-private structured harness protocol. A
    // dirty/development compiler must never receive an older cached harness
    // that predates or mismatches that protocol.
    if std::env::var_os("JET_PROVE_FRESH_TEST").is_some() {
        debug_native_cache_event("key-none prove-fresh");
        return None;
    }
    let Ok(mut bundle) = jet::Loader::load_entry_with_overlays(file, overlays, false) else {
        debug_native_cache_event("key-none load");
        return None;
    };
    // Checked before the front end runs: an `embed_file` program is never
    // cacheable, so it must not pay a sema pass to find that out.
    if program_uses_embed(&bundle) {
        debug_native_cache_event("key-none embed");
        return None;
    }
    // #91: instance identity is a sema product. Run the front end before a
    // cache lookup so a hit is keyed by resolved template identity rather than
    // consumer spelling. A hit still skips codegen/rustc, never validation.
    if jet::Driver::seed_build_facts(&mut bundle, profile, false, &BTreeMap::new()).is_err() {
        debug_native_cache_event("key-none seed-facts");
        return None;
    }
    if jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check)
        .iter()
        .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        debug_native_cache_event("key-none sema");
        return None;
    }
    native_cache_key_for_program(
        file,
        &bundle,
        profile_tag,
        mode_tag,
        toolchain_identity,
        invocation_authority,
    )
}

fn native_cache_key_for_prepared_build(
    file: &str,
    prepared: Option<&jet::Driver::PreparedBuildFrontEnd>,
    profile_tag: &str,
    mode_tag: &str,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    let Some(program) = prepared.and_then(|prepared| prepared.emitted_program()) else {
        debug_native_cache_event("program-key-none no-emitted-program");
        return None;
    };
    native_cache_key_for_program(
        file,
        program,
        profile_tag,
        mode_tag,
        native_toolchain_identity(),
        invocation_authority,
    )
}

/// The one native-key formula, over a program the front end already checked.
///
/// #2083: `jet build` calls this with the bundle its own front end produced, so
/// the key is hashed from the exact program that is then emitted. That is
/// strictly safer than the previous shape, which loaded and checked the program
/// a second time and trusted the two passes to agree — a divergence there (a
/// source edit racing the build, a fact snapshot taken twice, a differing sema
/// mode) would have stored a binary under a key describing a different program.
/// The inputs are canonical AST, instance identities, dependency interfaces,
/// runtime/Core fingerprints, bridge identity, manifest, toolchain, profile,
/// and recorded compile-time package inputs.

fn native_cache_key_for_program(
    file: &str,
    bundle: &jet::AST::ProgramBundle,
    profile_tag: &str,
    mode_tag: &str,
    toolchain_identity: &str,
    invocation_authority: Option<&jet_foundation::Authority::ApplicationAuthority>,
) -> Option<String> {
    debug_native_cache_event(format!(
        "program-key-entry pid={} file={} profile={} mode={} modules={}",
        std::process::id(),
        file,
        profile_tag,
        mode_tag,
        bundle.modules.len(),
    ));
    if std::env::var_os("JET_PROVE_FRESH_TEST").is_some() {
        debug_native_cache_event("program-key-none prove-fresh");
        return None;
    }
    let devtools_policy = jet::Driver::release_devtools_policy_for_bundle(bundle, profile_tag);
    let runtime_fingerprint =
        jet::Codegen::cached_runtime_fingerprint_with_policy(&devtools_policy);
    let corelib_fingerprint = jet::Codegen::corelib_emission_fingerprint_with_policy(
        bundle,
        mode_tag.starts_with("test"),
        &devtools_policy,
    );
    if !native_cacheable_program(bundle) {
        return None;
    }
    // The bridge is an input to the final executable, not merely a codegen
    // detail. Its content-addressed identity covers foreign declarations,
    // bridge dependencies, target tools, and link inputs.
    let bridge_identity = match jet::FFI::prepare(bundle) {
        Ok(Some(link)) => Some(link.cache_identity),
        Ok(None) => None,
        Err(_) => return None,
    };
    let instances: Vec<String> = bundle
        .modules
        .iter()
        .flat_map(|module| {
            module.items.iter().filter_map(|item| {
                let jet::AST::Item::CodeModule(cm) = item else {
                    return None;
                };
                cm.instance_identity
                    .as_ref()
                    .map(|identity| identity.fingerprint.clone())
            })
        })
        .collect();
    let dependency_interfaces = dependency_interface_fingerprint(bundle);
    let Some(manifest) = manifest_fingerprint(file) else {
        debug_native_cache_event("program-key-none manifest");
        return None;
    };
    // The native cache salt must carry the selected target boundary, not the
    // host process's OS/architecture. The canonical artifact key also frames
    // these exact facts; keeping the dossier digest here makes the lower-level
    // native salt independently target-aware before that key is assembled.
    let target_identity = jet::SHA256::sha256_hex(
        &bundle
            .build_facts
            .target_dossier
            .cache_bytes(&bundle.build_facts.target_triple),
    );
    let authority_identity = application_authority_cache_identity(invocation_authority);
    let salt = native_cache_salt(
        toolchain_identity,
        &format!("{manifest}:{dependency_interfaces}"),
        &runtime_fingerprint,
        &corelib_fingerprint,
        mode_tag,
        &target_identity,
        &instances,
        bridge_identity.as_deref(),
        &bundle.comptime_inputs,
        authority_identity.as_deref(),
    );
    let canonical = jet::CanonicalAST::canonical_bytes(bundle);
    let canonical_fingerprint = jet::SHA256::sha256_hex(&canonical);
    let key = jet::CanonicalAST::ast_cache_key(bundle, profile_tag, &salt, &bundle.build_facts);
    if let Ok(path) = std::env::var("JET_DEBUG_NATIVE_CACHE_LOG") {
        let line = format!(
            "pid={} file={} profile={} mode={} canonical={} toolchain={} dependency={} runtime={} corelib={} manifest={} salt={} key={} instances={:?} comptime_inputs={:?}\n",
            std::process::id(),
            file,
            profile_tag,
            mode_tag,
            canonical_fingerprint,
            jet::SHA256::sha256_hex(toolchain_identity.as_bytes()),
            dependency_interfaces,
            runtime_fingerprint,
            corelib_fingerprint,
            manifest,
            salt,
            key,
            instances,
            bundle.comptime_inputs,
        );
        if let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = std::io::Write::write_all(&mut log, line.as_bytes());
        }
    }
    debug_native_cache_event(format!(
        "program-key-ok pid={} file={} key={}",
        std::process::id(),
        file,
        key,
    ));
    Some(key)
}

/// `embed_file`/`embed_bytes` detection: a conservative source-substring scan.
/// A false positive only forgoes caching for that build (a safe perf cost); it
/// never serves a stale binary.
fn program_uses_embed(bundle: &jet::AST::ProgramBundle) -> bool {
    bundle.modules.iter().any(|module| {
        module.source.contains(jet::Syntax::BUILTIN_EMBED_FILE)
            || module.source.contains(jet::Syntax::BUILTIN_EMBED_BYTES)
    })
}

/// Game package builds need a fresh code-generation pass so the startup
/// bootstrap cannot be hidden behind an older native artifact cache entry.
fn program_uses_game_runtime(bundle: &jet::AST::ProgramBundle) -> bool {
    bundle
        .modules
        .iter()
        .any(|module| module.source.contains("core.game"))
}
/// Rust FFI bridges are cacheable once their content-addressed identity is in
/// the native key. Explicit C ABI links remain uncached because system library
/// contents are not represented by the Jet AST or the resolved argument list.
fn native_cacheable_program(bundle: &jet::AST::ProgramBundle) -> bool {
    !bundle.cffi.links_c()
        && !jet::FFI::collect_externs(bundle)
            .iter()
            .any(|entry| entry.c_abi)
}
/// Game package manifests are runtime configuration, so every generated game
/// binary carries the same startup seam.  Ordinary game runs have no manifest
/// beside them and the generated helper is therefore inert.
fn inject_game_crash_reporter_bootstrap(rust_code: &mut String, game_runtime: bool) {
    if !game_runtime {
        return;
    }
    const MAIN: &str = "fn main() {\n";
    let Some(main_start) = rust_code.find(MAIN) else {
        return;
    };
    const MARKER_DECL: &str =
        "\n#[used]\nstatic JET_GAME_CRASH_REPORTER_BOOTSTRAP_MARKER: &[u8] = \
         b\"jet-game-crash-reporter-v1\";\n";
    if !rust_code.contains("JET_GAME_CRASH_REPORTER_BOOTSTRAP_MARKER") {
        rust_code.insert_str(main_start, MARKER_DECL);
    }
    let main_start = rust_code.find(MAIN).unwrap_or(main_start);
    let insertion = "    jet_game_crash_reporter_install_from_manifest();\n";
    let body_start = main_start + MAIN.len();
    if rust_code[body_start..].starts_with(insertion) {
        return;
    }
    rust_code.insert_str(body_start, insertion);
}

/// E2-M15 / E3302: prove that rustc knows the requested cross-compilation
/// target and has its actual standard-library component installed.
///
/// `web` and `sandbox` are Jet backend aliases. The shared Doctor probe maps
/// both to the `wasm32-unknown-unknown` component used by their rustc step.
pub(crate) fn validate_target(triple: &str, mode: OutputMode) {
    let probe = jet::Doctor::probe_target_component(triple);
    let Err(error) = probe else {
        return;
    };

    let needs_install_hint = matches!(
        &error,
        jet::Doctor::TargetComponentProbeError::SysrootUnavailable
            | jet::Doctor::TargetComponentProbeError::LibraryUnavailable(_)
    );
    let fix = error.fix(triple);
    let diag = jet::Sema::e3302(triple);
    let src = format!("// cross-build for {}", triple);
    report_problems(mode, "<target>", &src, &[diag]);
    if needs_install_hint {
        write_mode_diagnostic(mode, &format!(" why: {fix}\n"));
    }
    exit(ExitCodes::USER_ERROR);
}

/// `jet dev <file>.jet --target=web`: the root retains only the R5 build
/// executor and process watch loop. HTTP, Canvas routes, terminal/browser
/// status, client leases, and last-good swapping live in `jet-devserver`.
pub(crate) fn run_dev_web(
    file: &str,
    profile: &BuildProfile,
    mode: OutputMode,
    verbose: bool,
    port: Option<u16>,
    canvas: bool,
    canvas_options: Option<jet_devserver::WebHost::CanvasHostOptions>,
    setting_overrides: &BTreeMap<String, String>,
) {
    let release_policy = release_devtools_policy_for_profile(profile);
    let path = Path::new(file);
    if !path.exists() {
        crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
        exit(ExitCodes::USER_ERROR);
    }

    let host = match if canvas {
        let options = canvas_options.as_ref().cloned().unwrap_or_default();
        jet_devserver::WebHost::WebHost::bind_web_with_canvas_options_and_policy(
            file,
            verbose,
            port,
            &options,
            release_policy.clone(),
        )
    } else {
        jet_devserver::WebHost::WebHost::bind_with_policy(file, verbose, port, release_policy)
    } {
        Ok(host) => host,
        Err(message) => {
            if canvas {
                crate::emit_cli_diagnostic_with_fix(
                    "E2105",
                    message,
                    "close the existing Canvas session or choose another `--canvas-port`"
                        .to_string(),
                );
            } else {
                write_mode_diagnostic(mode, &format!("{message}\n"));
            }
            exit(ExitCodes::USER_ERROR);
        }
    };
    let resident_session = host.resident_session();
    let _project_rebuild_executor = match resident_session.register_project_rebuild_executor() {
        Ok(guard) => guard,
        Err(error) => {
            write_mode_diagnostic(mode, &format!("{error}\n"));
            exit(ExitCodes::USER_ERROR);
        }
    };
    if rebuild_dev_web(file, profile, mode, verbose, false, &host, setting_overrides).is_err() {
        exit(ExitCodes::USER_ERROR);
    }
    if canvas {
        host.start_canvas();
        crate::CmdDevTools::open_canvas_browser(&host.canvas_url());
    } else {
        host.start();
    }

    // #439 / E3-UL6: same WatchSession engine as native `jet dev` / `jet run --watch`.
    let mut watch = match jet_devserver::WatchSession::open(path) {
        Ok(watch) => watch,
        Err(diagnostic) => {
            write_mode_diagnostic(
                mode,
                &jet::render_all_colored(file, "", &[diagnostic], mode.color_stderr()),
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    loop {
        thread::sleep(Duration::from_millis(jet_devserver::WATCH_POLL_INTERVAL_MS));
        if let Some(code) = host.exit_code() {
            exit(code);
        }
        if let Some(receipt) = watch.poll() {
            if receipt.change_kinds.iter().all(|k| *k == "stale") {
                continue;
            }
            let _ = rebuild_dev_web(file, profile, mode, verbose, true, &host, setting_overrides);
            if let Err(diagnostic) = watch.acknowledge(&receipt) {
                write_mode_diagnostic(
                    mode,
                    &jet::render_all_colored(file, "", &[diagnostic], mode.color_stderr()),
                );
                exit(ExitCodes::USER_ERROR);
            }
        }
        match resident_session.take_project_rebuild() {
            Ok(Some(request)) => {
                let result =
                    rebuild_dev_web(file, profile, mode, verbose, true, &host, setting_overrides);
                if let Err(error) = resident_session.finish_project_rebuild(&request, result) {
                    write_mode_diagnostic(mode, &format!("{error}\n"));
                }
            }
            Ok(None) => {}
            Err(error) => {
                write_mode_diagnostic(mode, &format!("{error}\n"));
            }
        }
    }
}

fn rebuild_dev_web(
    file: &str,
    profile: &BuildProfile,
    mode: OutputMode,
    verbose: bool,
    is_rebuild: bool,
    host: &jet_devserver::WebHost::WebHost,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<(), String> {
    let _source_transaction = host.lock_source_transaction();
    let started = Instant::now();
    host.mark_building();
    let src = fs::read_to_string(file).unwrap_or_default();
    let out = match jet::compile_web_with_gates_and_profile_and_settings(
        file,
        jet::Policy::GateSet::default(),
        profile.budget_name(),
        setting_overrides,
    ) {
        Ok(out) => out,
        Err(diags) => {
            if !is_rebuild {
                report_problems(mode, file, &src, &diags);
            }
            let code = diags
                .first()
                .map(|diagnostic| diagnostic.code.to_string())
                .unwrap_or_default();
            let message = jet::render_diagnostics(file, &src, &diags);
            host.mark_error(code, message.clone(), is_rebuild);
            return Err(message);
        }
    };
    let Some(web) = &out.web else {
        let message = jet::Diagnostics::render_ice_report("missing web codegen output", "", false);
        write_mode_diagnostic(mode, &format!("{message}\n"));
        host.mark_error("ICE".to_string(), message.clone(), is_rebuild);
        return Err(message);
    };

    let staging = PathBuf::from("build").join(".jet-dev-staging");
    let staging_authority =
        match jet_devserver::WebHost::WebOutputAuthority::open_or_create(&staging) {
            Ok(authority) => authority,
            Err(error) => {
                let message = format!("error: couldn't open web staging output: {error}");
                write_mode_diagnostic(mode, &format!("{message}\n"));
                host.mark_error("ICE".to_string(), message.clone(), is_rebuild);
                return Err(message);
            }
        };
    let model_runtime = web.wasm_rust.contains("extern crate jet_rt;");
    if let Err(message) = write_web_artifacts(
        file,
        web,
        out.ffi.as_ref(),
        model_runtime,
        verbose,
        &staging_authority,
        true,
        mode,
    ) {
        write_mode_diagnostic(mode, &format!("{message}\n"));
        host.mark_error("ICE".to_string(), message.clone(), is_rebuild);
        return Err(message);
    }
    if let Err(error) = jet_devserver::WebHost::stage_and_swap(&staging, Path::new("build")) {
        let message = format!("couldn't finalize web build: {error}");
        write_mode_diagnostic(mode, &format!("{message}\n"));
        host.mark_error("ICE".to_string(), message.clone(), is_rebuild);
        return Err(message);
    }

    host.mark_ready(started.elapsed().as_millis(), is_rebuild);
    Ok(())
}

/// Where `write_web_artifacts` put each `build/*` file it wrote — returned so
/// a caller can print/report the exact locations without recomputing the
/// path-join logic a second time (I8: the join logic lives in exactly one
/// place).
pub(crate) struct WebBuildPaths {
    pub(crate) manifest: PathBuf,
    pub(crate) dom: PathBuf,
    pub(crate) js: PathBuf,
    pub(crate) js_map: Option<PathBuf>,
    pub(crate) wasm: PathBuf,
    pub(crate) wasm_map: Option<PathBuf>,
    pub(crate) html: PathBuf,
}

/// D-WEBKIND1=A (c123 M2), extended by c134 Phase 7 (`jet dev --target=web`):
/// the ONE place that knows how to turn a compiled `WebArtifacts` bundle into
/// files on disk under `out_dir` (I8 — `jet build --target=web` and `jet dev
/// --target=web`'s rebuild-on-save loop both call this, never duplicating the
/// write/rustc-invoke logic).
///
/// `file` is the `.jet` source path — used only to look for a companion
/// `<stem>.html` next to it, which wins over the generic `index_html` codegen
/// emits (an example wiring a button to an exported `#JS` function ships its
/// own page; see the canonical `Codegen::MIRWeb::WebArtifacts` type for
/// how web artifacts are typed during compilation.
///
/// Returns the paths written on success. On failure to run/pass rustc for the
/// wasm half, returns `Err` with an already-formatted message instead of
/// exiting — I2: rustc rejecting generated code is always an internal
/// compiler error, but only the caller knows whether that should abort the
/// process (`jet build`) or just be reported while the previous good build
/// keeps serving (`jet dev --target=web`).
fn jet_rt_rlib(target: Option<&str>, release: bool) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("JET_RT_RLIB").map(PathBuf::from) {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("JET_RT_RLIB `{}` is unavailable: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "JET_RT_RLIB `{}` is not a regular file",
                path.display()
            ));
        }
        return Ok(path);
    }
    let profile = if release { "release" } else { "debug" };
    let mut roots = Vec::new();
    let target_root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    if let Some(target) = target {
        roots.push(target_root.join(target).join(profile).join("deps"));
    }
    roots.push(target_root.join(profile).join("deps"));
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            roots.push(parent.join("deps"));
        }
    }
    let mut candidates = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if name.starts_with("libjet_rt-")
                && path.extension().and_then(|extension| extension.to_str()) == Some("rlib")
                && metadata.is_file()
                && !metadata.file_type().is_symlink()
            {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates.pop().ok_or_else(|| {
        "could not locate the runtime model crate; build `jet-rt` or set JET_RT_RLIB".to_string()
    })
}

/// Write the web artifacts and, when needed, link the neutral model runtime
/// crate into the generated wasm module.
pub(crate) fn write_web_artifacts(
    file: &str,
    web: &jet::Codegen::MIRWeb::WebArtifacts,
    ffi: Option<&jet::FFI::FfiLink>,
    model_runtime: bool,
    verbose: bool,
    output: &jet_devserver::WebHost::WebOutputAuthority,
    emit_maps: bool,
    mode: OutputMode,
) -> Result<WebBuildPaths, String> {
    let step = |msg: String| {
        if verbose {
            write_mode_status(mode, &format!("[build] {msg}\n"));
        }
    };
    let output_path = |name: &str| {
        output
            .path_for(name)
            .map_err(|e| format!("error: couldn't resolve web output `{name}`: {e}"))
    };

    let manifest_path = output_path("web.manifest.json")?;
    let dom_path = output_path("jet_dom_runtime.js")?;
    let onnx_runtime_path = output_path("jet_onnx_runtime.js")?;
    let onnx_runtime_worker_path = output_path("OnnxRuntimeWebWorker.js")?;
    let js_path = output_path("app.js")?;
    let js_map_path = output_path("app.js.map")?;
    let wasm_rs_path = output_path("app_wasm.rs")?;
    let wasm_path = output_path("app.wasm")?;
    let wasm_map_path = output_path("app.wasm.map")?;
    let html_path = output_path("index.html")?;

    // D-HTMLPAIR1 (ratified 2026-07-01, c134): precedence for the served HTML source —
    // (1) an explicit `#HTML("path.html")` marker, relative to the source
    //     file's own directory; a path that doesn't resolve is a hard error
    //     naming the missing file, never a silent fallback;
    // (2) the legacy `<stem>.html` sibling-filename convention, kept for
    //     backward-compat with existing examples that predate the marker;
    // (3) the generic `jet_main()`-only page from `emit_index_html`.
    let html_contents = if let Some(rel) = &web.explicit_html_path {
        read_web_html_source(file, Path::new(rel), true)?
            .ok_or_else(|| format!("error: `#HTML(\"{rel}\")` source is missing"))?
    } else {
        let sibling_html = PathBuf::from(file).with_extension("html");
        match sibling_html.file_name() {
            Some(name) => read_web_html_source(file, Path::new(name), false)?
                .unwrap_or_else(|| web.index_html.clone()),
            None => web.index_html.clone(),
        }
    };

    let manifest_json = if emit_maps {
        web.manifest_json.clone()
    } else {
        strip_manifest_source_map(&web.manifest_json)
    };
    let mut js_app = web.js_app.clone();
    if emit_maps {
        if !js_app.ends_with('\n') {
            js_app.push('\n');
        }
        js_app.push_str("//# sourceMappingURL=app.js.map\n");
    }

    output
        .replace_file("web.manifest.json", manifest_json.as_bytes())
        .map_err(|e| format!("error: couldn't write {}: {}", manifest_path.display(), e))?;
    output
        .replace_file("jet_dom_runtime.js", web.dom_runtime.as_bytes())
        .map_err(|e| format!("error: couldn't write {}: {}", dom_path.display(), e))?;
    if model_runtime {
        output
            .replace_file("jet_onnx_runtime.js", web.onnx_runtime_js.as_bytes())
            .map_err(|e| {
                format!(
                    "error: couldn't write {}: {}",
                    onnx_runtime_path.display(),
                    e
                )
            })?;
        output
            .replace_file(
                "OnnxRuntimeWebWorker.js",
                web.onnx_runtime_worker_js.as_bytes(),
            )
            .map_err(|e| {
                format!(
                    "error: couldn't write {}: {}",
                    onnx_runtime_worker_path.display(),
                    e
                )
            })?;
    } else {
        output
            .remove_file_if_exists("jet_onnx_runtime.js")
            .map_err(|e| {
                format!(
                    "error: couldn't remove {}: {}",
                    onnx_runtime_path.display(),
                    e
                )
            })?;
        output
            .remove_file_if_exists("OnnxRuntimeWebWorker.js")
            .map_err(|e| {
                format!(
                    "error: couldn't remove {}: {}",
                    onnx_runtime_worker_path.display(),
                    e
                )
            })?;
    }
    output
        .replace_file("app.js", js_app.as_bytes())
        .map_err(|e| format!("error: couldn't write {}: {}", js_path.display(), e))?;
    output
        .replace_file("app_wasm.rs", web.wasm_rust.as_bytes())
        .map_err(|e| format!("error: couldn't write {}: {}", wasm_rs_path.display(), e))?;
    output
        .replace_file("index.html", html_contents.as_bytes())
        .map_err(|e| format!("error: couldn't write {}: {}", html_path.display(), e))?;

    let mut js_map_written = None;
    if emit_maps {
        output
            .replace_file("app.js.map", web.js_source_map.as_bytes())
            .map_err(|e| format!("error: couldn't write {}: {}", js_map_path.display(), e))?;
        js_map_written = Some(js_map_path.clone());
        step(format!("js map     -> {}", js_map_path.display()));
    } else {
        output
            .remove_file_if_exists("app.js.map")
            .map_err(|e| format!("error: couldn't remove {}: {}", js_map_path.display(), e))?;
        output
            .remove_file_if_exists("app.wasm.map")
            .map_err(|e| format!("error: couldn't remove {}: {}", wasm_map_path.display(), e))?;
    }

    step(format!("web manifest -> {}", manifest_path.display()));
    step(format!("dom shim    -> {}", dom_path.display()));
    step(format!("js entry    -> {}", js_path.display()));
    step(format!("wasm emit   -> {}", wasm_rs_path.display()));
    step(format!("index.html  -> {}", html_path.display()));
    step(format!(
        "rustc wasm  {} -> {}",
        wasm_rs_path.display(),
        wasm_path.display()
    ));

    let wasm_temp = output
        .create_temp_file("jet-web-rustc", ".wasm")
        .map_err(|e| format!("error: couldn't create a wasm temporary: {e}"))?;
    let wasm_temp_path = wasm_temp.path().to_path_buf();
    let wasm_source = wasm_rs_path.to_str().ok_or_else(|| {
        format!(
            "error: web source path is not valid UTF-8: {}",
            wasm_rs_path.display()
        )
    })?;
    let wasm_destination = wasm_temp_path.to_str().ok_or_else(|| {
        format!(
            "error: web wasm temporary path is not valid UTF-8: {}",
            wasm_temp_path.display()
        )
    })?;

    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        "--target",
        "wasm32-unknown-unknown",
        "--crate-type",
        "cdylib",
        "--crate-name",
    ]);
    rustc.arg(jet::Syntax::sanitize_crate_name(
        wasm_rs_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("out"),
    ));
    if emit_maps {
        // Exact Jet statement lines need rustc's Wasm line table.
        rustc.args(["-C", "opt-level=0", "-C", "debuginfo=2"]);
    } else {
        rustc.arg("-O");
    }
    if model_runtime {
        let rlib = jet_rt_rlib(Some("wasm32-unknown-unknown"), !emit_maps)?;
        let dependencies = rlib.parent().ok_or_else(|| {
            format!(
                "runtime rlib `{}` has no dependency directory",
                rlib.display()
            )
        })?;
        rustc
            .arg("--extern")
            .arg(format!("jet_rt={}", rlib.display()))
            .arg("-L")
            .arg(format!("dependency={}", dependencies.display()));
    }
    rustc.args([wasm_source, "-o", wasm_destination]);
    if let Some(link) = ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc
        .output()
        .map_err(|e| format!("error: couldn't run rustc for wasm: {}", e))?;

    if !rustc.status.success() {
        return Err(jet::Diagnostics::render_ice_report(
            "rustc rejected generated wasm module",
            &String::from_utf8_lossy(&rustc.stderr),
            true,
        ));
    }

    // rustc may atomically replace its output path. Reopen the path instead of
    // reading the pre-existing temporary-file handle, which can still point at
    // the empty inode after that rename.
    let mut wasm = fs::read(&wasm_temp_path)
        .map_err(|e| format!("error: couldn't read {}: {}", wasm_temp_path.display(), e))?;
    jet_foundation::CLISchema::embed_wasm_record(&mut wasm, &web.command_record)
        .map_err(|e| format!("error: couldn't embed JetCommandSchema metadata: {e}"))?;

    let mut wasm_map_written = None;
    if emit_maps {
        let wasm_map = jet::Codegen::build_wasm_jet_source_map(
            &wasm,
            &web.wasm_rust,
            &web.source_names,
            &web.source_contents,
        )
        .map_err(|e| format!("error: couldn't build app.wasm.map: {e}"))?;
        output
            .replace_file("app.wasm.map", wasm_map.as_bytes())
            .map_err(|e| format!("error: couldn't write {}: {}", wasm_map_path.display(), e))?;
        jet_foundation::WasmDebug::embed_source_mapping_url(&mut wasm, "app.wasm.map")
            .map_err(|e| format!("error: couldn't embed wasm sourceMappingURL: {e:?}"))?;
        wasm_map_written = Some(wasm_map_path.clone());
        step(format!("wasm map   -> {}", wasm_map_path.display()));
    }

    output
        .replace_file("app.wasm", &wasm)
        .map_err(|e| format!("error: couldn't write {}: {}", wasm_path.display(), e))?;

    Ok(WebBuildPaths {
        manifest: manifest_path,
        dom: dom_path,
        js: js_path,
        js_map: js_map_written,
        wasm: wasm_path,
        wasm_map: wasm_map_written,
        html: html_path,
    })
}

fn read_web_html_source(
    file: &str,
    relative: &Path,
    required: bool,
) -> Result<Option<String>, String> {
    let source_dir = Path::new(file)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let source_path = source_dir.join(relative);
    let resolver = jet::Authority::AuthorityResolver::open(source_dir).map_err(|error| {
        format!(
            "error: couldn't open the pinned HTML source directory `{}`: {}",
            source_dir.display(),
            error
        )
    })?;
    let checked = match resolver.checked_file(relative) {
        Ok(checked) => checked,
        Err(error) if !required && error.is_missing() => return Ok(None),
        Err(error) if required && error.is_missing() => {
            return Err(format!(
                "error: `#HTML(\"{}\")` names a file that doesn't exist: {} ({})",
                relative.display(),
                source_path.display(),
                error
            ));
        }
        Err(error) => {
            return Err(format!(
                "error: HTML source `{}` is not a safe source-relative regular file: {}",
                source_path.display(),
                error
            ));
        }
    };
    resolver.revalidate_file(&checked).map_err(|error| {
        format!(
            "error: HTML source `{}` changed during its pinned read: {}",
            source_path.display(),
            error
        )
    })?;
    let text = checked.text().map_err(|error| {
        format!(
            "error: HTML source `{}` is not valid UTF-8: {}",
            source_path.display(),
            error
        )
    })?;
    Ok(Some(text))
}

mod foreign_build_import {
    use super::*;

    const CANDIDATE_SCHEMA: &str = "jet-ffi-build-import-v1";
    const SELECTION_PACKAGE: &str = "__jet_foreign_build__";

    #[derive(Clone, Debug)]
    enum Operation {
        Preview,
        Accept(String),
    }

    #[derive(Clone, Debug)]
    struct Request {
        kind: String,
        build_dir: String,
        operation: Operation,
    }

    #[derive(Clone, Debug)]
    struct Action {
        id: String,
        owner: String,
        inner_owner: Option<String>,
        command: Vec<String>,
        inputs: Vec<String>,
        outputs: Vec<String>,
        environment: Vec<(String, String)>,
        workdir: String,
        custom: bool,
    }

    #[derive(Clone, Debug)]
    struct Candidate {
        key: String,
        build_dir: String,
        argv: Vec<String>,
        inputs: Vec<(String, String)>,
        outputs: Vec<String>,
        toolchains: Vec<(String, String)>,
        environment: Vec<(String, String)>,
        workdirs: Vec<String>,
        actions: Vec<Action>,
        unsupported: Vec<String>,
        ownership: String,
    }

    #[derive(Debug)]
    enum Failure {
        User(String),
        Internal(String),
    }

    impl Failure {
        fn user(message: impl Into<String>) -> Self {
            Self::User(message.into())
        }

        fn internal(message: impl Into<String>) -> Self {
            Self::Internal(message.into())
        }
    }

    fn relative_path(value: &str) -> Result<PathBuf, Failure> {
        let path = Path::new(value);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(Failure::user(format!(
                "foreign build path `{value}` must be relative to the project"
            )));
        }
        Ok(path.to_path_buf())
    }

    fn display_relative(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn ensure_directory(path: &Path) -> Result<(), Failure> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(Failure::internal(format!(
                "refusing symlinked directory `{}`",
                path.display()
            ))),
            Ok(metadata) if metadata.is_dir() => Ok(()),
            Ok(_) => Err(Failure::internal(format!(
                "foreign build cache path `{}` is not a directory",
                path.display()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    ensure_directory(parent)?;
                }
                match fs::create_dir(path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        ensure_directory(path)
                    }
                    Err(error) => Err(Failure::internal(format!(
                        "could not create `{}`: {error}",
                        path.display()
                    ))),
                }
            }
            Err(error) => Err(Failure::internal(format!(
                "could not inspect `{}`: {error}",
                path.display()
            ))),
        }
    }

    fn read_regular(root: &Path, relative: &str) -> Result<Vec<u8>, Failure> {
        let relative_path = relative_path(relative)?;
        let path = root.join(&relative_path);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            Failure::user(format!(
                "foreign build input `{relative}` is unavailable: {error}"
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Failure::user(format!(
                "foreign build input `{relative}` must be a regular non-symlink file"
            )));
        }
        fs::read(&path).map_err(|error| {
            Failure::user(format!(
                "could not read foreign build input `{relative}`: {error}"
            ))
        })
    }

    fn add_input(
        root: &Path,
        relative: &str,
        inputs: &mut Vec<(String, String)>,
        unsupported: &mut Vec<String>,
    ) -> Result<(), Failure> {
        let relative = display_relative(&relative_path(relative)?);
        if inputs.iter().any(|(path, _)| path == &relative) {
            return Ok(());
        }
        match read_regular(root, &relative) {
            Ok(bytes) => {
                inputs.push((
                    relative,
                    format!("sha256-{}", jet::SHA256::sha256_hex(&bytes)),
                ));
                Ok(())
            }
            Err(Failure::User(error)) => {
                unsupported.push(error);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn cmake_calls(source: &str, command: &str) -> Vec<Vec<String>> {
        let lower = source.to_ascii_lowercase();
        let needle = command.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let mut cursor = 0usize;
        let mut calls = Vec::new();
        while cursor < bytes.len() {
            let Some(found) = lower[cursor..].find(&needle) else {
                break;
            };
            let start = cursor + found;
            let before = start
                .checked_sub(1)
                .and_then(|index| bytes.get(index).copied());
            let after_name = start + needle.len();
            if before.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || bytes
                    .get(after_name)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                cursor = after_name;
                continue;
            }
            let Some(open_offset) = lower[after_name..].find('(') else {
                break;
            };
            let open = after_name + open_offset;
            let Some(close_offset) = source[open + 1..].find(')') else {
                break;
            };
            let close = open + 1 + close_offset;
            let args = source[open + 1..close]
                .split_whitespace()
                .map(|value| {
                    value
                        .trim_matches(|character| matches!(character, '"' | '\''))
                        .trim_end_matches(';')
                        .to_string()
                })
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            calls.push(args);
            cursor = close + 1;
        }
        calls
    }

    fn cmake_source_tokens(
        calls: &[Vec<String>],
        root: &Path,
        inputs: &mut Vec<(String, String)>,
        unsupported: &mut Vec<String>,
    ) -> Result<(), Failure> {
        const FLAGS: &[&str] = &[
            "WIN32",
            "MACOSX_BUNDLE",
            "EXCLUDE_FROM_ALL",
            "STATIC",
            "SHARED",
            "MODULE",
            "OBJECT",
            "INTERFACE",
            "ALL",
        ];
        for args in calls {
            for token in args.iter().skip(1) {
                if FLAGS.contains(&token.as_str())
                    || token.starts_with('$')
                    || token.contains('=')
                    || !token.contains('.')
                {
                    continue;
                }
                let path = token.trim_matches(|character| matches!(character, '"' | '\''));
                let extension = Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !matches!(
                    extension.as_str(),
                    "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" | "m" | "mm"
                ) {
                    continue;
                }
                if root.join(path).is_file() {
                    add_input(root, path, inputs, unsupported)?;
                } else {
                    unsupported.push(format!("declared CMake source `{path}` is unavailable"));
                }
            }
        }
        Ok(())
    }
    fn cmake_custom_dependencies(
        calls: &[Vec<String>],
        root: &Path,
        inputs: &mut Vec<(String, String)>,
        unsupported: &mut Vec<String>,
    ) -> Result<(), Failure> {
        const FLAGS: &[&str] = &[
            "COMMAND",
            "DEPENDS",
            "OUTPUT",
            "BYPRODUCTS",
            "VERBATIM",
            "WORKING_DIRECTORY",
            "COMMENT",
            "USES_TERMINAL",
            "COMMAND_EXPAND_LISTS",
        ];
        for args in calls {
            let Some(depends) = args.iter().position(|value| value == "DEPENDS") else {
                continue;
            };
            for token in args.iter().skip(depends + 1) {
                let token = token.trim_matches(|character| matches!(character, '"' | '\''));
                if token.is_empty()
                    || FLAGS.iter().any(|flag| *flag == token)
                    || token.starts_with('$')
                    || token.contains('=')
                    || token.starts_with('-')
                {
                    continue;
                }
                if root.join(token).is_file() {
                    add_input(root, token, inputs, unsupported)?;
                } else {
                    unsupported.push(format!(
                        "declared CMake custom dependency `{token}` is unavailable"
                    ));
                }
            }
        }
        Ok(())
    }

    fn cache_value(cache: &str, key: &str) -> Option<String> {
        cache.lines().find_map(|line| {
            let line = line.trim();
            let (name, value) = line.split_once('=')?;
            let name = name.split(':').next().unwrap_or(name);
            (name == key).then(|| value.trim().to_string())
        })
    }

    fn cmake_target_outputs(build_dir: &str, command: &str, name: &str) -> Vec<String> {
        let prefix = format!("{build_dir}/");
        match command {
            "add_library" => vec![format!("{prefix}lib{name}.a")],
            "add_executable" => vec![format!("{prefix}{name}")],
            _ => Vec::new(),
        }
    }

    fn custom_outputs(build_dir: &str, calls: &[Vec<String>]) -> Vec<String> {
        let mut outputs = Vec::new();
        for args in calls {
            let mut key = None;
            for value in args {
                if matches!(key, Some("OUTPUT" | "BYPRODUCTS")) {
                    if !value.starts_with('$') {
                        if !Path::new(value).is_absolute() {
                            outputs.push(format!("{build_dir}/{value}"));
                        }
                    }
                    key = None;
                } else if matches!(value.as_str(), "OUTPUT" | "BYPRODUCTS") {
                    key = Some(value.as_str());
                }
            }
        }
        outputs
    }

    fn candidate_path(root: &Path, key: &str) -> PathBuf {
        let digest = jet::SHA256::sha256_hex(key.as_bytes());
        root.join(".jet")
            .join("cache")
            .join("foreign-build")
            .join(format!(
                "{}-{}.json",
                key.replace(':', "-").replace('/', "_"),
                digest.get(..16).unwrap_or(&digest)
            ))
    }

    fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
        let parent = path.parent().ok_or_else(|| {
            Failure::internal(format!("candidate `{}` has no parent", path.display()))
        })?;
        ensure_directory(parent)?;
        let temporary = parent.join(format!(
            ".foreign-build-{}-{}.tmp",
            std::process::id(),
            jet::SHA256::sha256_hex(bytes)
                .get(..16)
                .unwrap_or("candidate")
        ));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| {
                    Failure::internal(format!("could not stage candidate: {error}"))
                })?;
            file.write_all(bytes).map_err(|error| {
                Failure::internal(format!("could not write candidate: {error}"))
            })?;
            file.sync_all()
                .map_err(|error| Failure::internal(format!("could not sync candidate: {error}")))?;
            if let Ok(metadata) = fs::symlink_metadata(path) {
                if metadata.file_type().is_symlink() || metadata.is_dir() {
                    return Err(Failure::internal(format!(
                        "candidate destination `{}` is not replaceable",
                        path.display()
                    )));
                }
            }
            fs::rename(&temporary, path).map_err(|error| {
                Failure::internal(format!("could not publish candidate: {error}"))
            })?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn json_strings(values: &[String]) -> String {
        values
            .iter()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn json_pairs(values: &[(String, String)]) -> String {
        values
            .iter()
            .map(|(key, value)| {
                format!(
                    "{{\"name\":\"{}\",\"value\":\"{}\"}}",
                    json_escape(key),
                    json_escape(value)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn action_json(action: &Action) -> String {
        let inner = action
            .inner_owner
            .as_deref()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"id\":\"{}\",\"owner\":\"{}\",\"inner_owner\":{},\"command\":[{}],\"inputs\":[{}],\"outputs\":[{}],\"environment\":[{}],\"workdir\":\"{}\",\"custom_action\":{}}}",
            json_escape(&action.id),
            json_escape(&action.owner),
            inner,
            json_strings(&action.command),
            json_strings(&action.inputs),
            json_strings(&action.outputs),
            json_pairs(&action.environment),
            json_escape(&action.workdir),
            action.custom
        )
    }

    fn candidate_json(candidate: &Candidate) -> String {
        let actions = candidate
            .actions
            .iter()
            .map(action_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\n  \"schema\":\"{CANDIDATE_SCHEMA}\",\n  \"selection_key\":\"{}\",\n  \"kind\":\"cmake\",\n  \"build_dir\":\"{}\",\n  \"ownership\":\"{}\",\n  \"argv\":[{}],\n  \"dependencies\":[{}],\n  \"inputs\":[{}],\n  \"outputs\":[{}],\n  \"toolchains\":[{}],\n  \"environment\":[{}],\n  \"workdirs\":[{}],\n  \"actions\":[{}],\n  \"unsupported_edges\":[{}]\n}}\n",
            json_escape(&candidate.key),
            json_escape(&candidate.build_dir),
            json_escape(&candidate.ownership),
            json_strings(&candidate.argv),
            json_strings(
                &candidate
                    .inputs
                    .iter()
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>()
            ),
            candidate
                .inputs
                .iter()
                .map(|(path, digest)| {
                    format!(
                        "{{\"path\":\"{}\",\"digest\":\"{}\"}}",
                        json_escape(path),
                        json_escape(digest)
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
            json_strings(&candidate.outputs),
            json_pairs(&candidate.toolchains),
            json_pairs(&candidate.environment),
            json_strings(&candidate.workdirs),
            actions,
            json_strings(&candidate.unsupported)
        )
    }

    fn build_candidate(
        root: &Path,
        request: &Request,
    ) -> Result<(PathBuf, Candidate, String), Failure> {
        if request.kind != "cmake" {
            return Err(Failure::user(format!(
                "unsupported foreign build importer `{}`; only `cmake:build` is currently supported",
                request.kind
            )));
        }
        let cmake_source = read_regular(root, "CMakeLists.txt")?;
        let cmake_source = std::str::from_utf8(&cmake_source)
            .map_err(|_| Failure::user("CMakeLists.txt must be UTF-8"))?
            .to_string();
        let cache_relative = display_relative(&relative_path(&format!(
            "{}/CMakeCache.txt",
            request.build_dir
        ))?);
        let cache_bytes = read_regular(root, &cache_relative)?;
        let cache = std::str::from_utf8(&cache_bytes)
            .map_err(|_| Failure::user("CMakeCache.txt must be UTF-8"))?
            .to_string();
        let compile_commands_relative = display_relative(&relative_path(&format!(
            "{}/compile_commands.json",
            request.build_dir
        ))?);
        let compile_commands_present = root.join(&compile_commands_relative).is_file();

        let mut inputs = Vec::new();
        let mut unsupported = Vec::new();
        add_input(root, "CMakeLists.txt", &mut inputs, &mut unsupported)?;
        add_input(root, &cache_relative, &mut inputs, &mut unsupported)?;
        if compile_commands_present {
            add_input(
                root,
                &compile_commands_relative,
                &mut inputs,
                &mut unsupported,
            )?;
            unsupported.push(
                "compilation database is evidence only; it cannot establish the complete graph"
                    .to_string(),
            );
        }
        if root.join("package.jet").is_file() {
            add_input(root, "package.jet", &mut inputs, &mut unsupported)?;
        }
        let target_calls = ["add_executable", "add_library", "add_custom_target"]
            .iter()
            .flat_map(|command| {
                cmake_calls(&cmake_source, command)
                    .into_iter()
                    .map(move |args| ((*command).to_string(), args))
            })
            .collect::<Vec<_>>();
        cmake_source_tokens(
            &target_calls
                .iter()
                .map(|(_, args)| args.clone())
                .collect::<Vec<_>>(),
            root,
            &mut inputs,
            &mut unsupported,
        )?;
        let custom_calls = cmake_calls(&cmake_source, "add_custom_command");
        cmake_custom_dependencies(&custom_calls, root, &mut inputs, &mut unsupported)?;
        if !custom_calls.is_empty() {
            unsupported.push(
                "CMake custom actions retain their inner foreign owner until explicitly modeled"
                    .to_string(),
            );
        }
        for dynamic in ["add_dependencies(", "include(", "execute_process(", "$<"] {
            if cmake_source.contains(dynamic) {
                unsupported.push(format!("unsupported dynamic CMake edge `{dynamic}`"));
            }
        }
        let mut argv = vec![
            "cmake".to_string(),
            "--build".to_string(),
            request.build_dir.clone(),
        ];
        match jet::Comptime::Build::LegacyWrapperSpec::from_project_file(
            root,
            jet::Comptime::Build::LegacyWrapperKind::CMake,
        ) {
            Ok(spec) => argv = spec.argv,
            Err(error) => unsupported.push(format!(
                "typed CMake importer retained this edge: {error:?}"
            )),
        }

        let mut outputs = Vec::new();
        let mut actions = Vec::new();
        for (index, (command, args)) in target_calls.iter().enumerate() {
            let Some(name) = args.first() else {
                unsupported.push(format!("{command} has no literal target name"));
                continue;
            };
            let target_outputs = cmake_target_outputs(&request.build_dir, command, name);
            outputs.extend(target_outputs.iter().cloned());
            let action_command = vec![
                "cmake".to_string(),
                "--build".to_string(),
                request.build_dir.clone(),
                "--target".to_string(),
                name.clone(),
            ];
            let action_inputs = inputs
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            actions.push(Action {
                id: format!("cmake.target.{index}.{name}"),
                owner: "cmake".to_string(),
                inner_owner: None,
                command: action_command,
                inputs: action_inputs,
                outputs: target_outputs,
                environment: Vec::new(),
                workdir: request.build_dir.clone(),
                custom: false,
            });
        }
        for (index, args) in custom_calls.iter().enumerate() {
            let inner_owner = args
                .iter()
                .position(|value| value == "COMMAND")
                .and_then(|position| args.get(position + 1))
                .cloned()
                .or_else(|| Some("cmake custom command".to_string()));
            let action_outputs = custom_outputs(&request.build_dir, std::slice::from_ref(args));
            outputs.extend(action_outputs.iter().cloned());
            actions.push(Action {
                id: format!("cmake.custom.{index}"),
                owner: "cmake".to_string(),
                inner_owner,
                command: vec![
                    "cmake".to_string(),
                    "--build".to_string(),
                    request.build_dir.clone(),
                ],
                inputs: inputs
                    .iter()
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>(),
                outputs: action_outputs,
                environment: Vec::new(),
                workdir: ".".to_string(),
                custom: true,
            });
        }
        if actions.is_empty() {
            actions.push(Action {
                id: "cmake.build".to_string(),
                owner: "cmake".to_string(),
                inner_owner: Some("cmake generator".to_string()),
                command: argv.clone(),
                inputs: inputs
                    .iter()
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>(),
                outputs: Vec::new(),
                environment: Vec::new(),
                workdir: ".".to_string(),
                custom: true,
            });
        }
        outputs.sort();
        outputs.dedup();
        if outputs.is_empty() {
            unsupported.push("CMake did not expose a declared artifact output".to_string());
        }
        let mut toolchains = Vec::new();
        for key in [
            "CMAKE_C_COMPILER",
            "CMAKE_CXX_COMPILER",
            "CMAKE_LINKER",
            "CMAKE_GENERATOR",
            "CMAKE_BUILD_TYPE",
        ] {
            if let Some(value) = cache_value(&cache, key) {
                toolchains.push((key.to_string(), value));
            }
        }
        let mut environment = Vec::new();
        for key in [
            "CMAKE_BUILD_TYPE",
            "CMAKE_GENERATOR",
            "CMAKE_TOOLCHAIN_FILE",
        ] {
            if let Some(value) = cache_value(&cache, key) {
                environment.push((key.to_string(), value));
            }
        }
        let mut unsupported = unsupported
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let ownership = "partial".to_string();
        let mut candidate = Candidate {
            key: format!("{}:{}", request.kind, request.build_dir),
            build_dir: request.build_dir.clone(),
            argv,
            inputs,
            outputs,
            toolchains,
            environment,
            workdirs: vec![".".to_string()],
            actions,
            unsupported: std::mem::take(&mut unsupported),
            ownership,
        };
        candidate.inputs.sort();
        candidate.outputs.sort();
        candidate.toolchains.sort();
        candidate.environment.sort();
        candidate.workdirs.sort();
        candidate
            .actions
            .sort_by(|left, right| left.id.cmp(&right.id));
        let bytes = candidate_json(&candidate).into_bytes();
        let digest = format!("sha256-{}", jet::SHA256::sha256_hex(&bytes));
        let path = candidate_path(root, &candidate.key);
        write_atomic(&path, &bytes)?;
        Ok((path, candidate, digest))
    }

    fn tree_field<'a>(tree: &'a DataTree, key: &str) -> Result<&'a DataTree, Failure> {
        tree.get(key)
            .map_err(|error| Failure::internal(format!("candidate {error}")))
    }

    fn tree_string(tree: &DataTree, key: &str) -> Result<String, Failure> {
        tree_field(tree, key)?
            .as_str()
            .map(str::to_string)
            .map_err(|error| Failure::internal(format!("candidate `{key}` {error}")))
    }

    fn tree_array<'a>(tree: &'a DataTree, key: &str) -> Result<&'a [DataTree], Failure> {
        tree_field(tree, key)?
            .as_array()
            .map(Vec::as_slice)
            .map_err(|error| Failure::internal(format!("candidate `{key}` {error}")))
    }

    fn tree_object_field<'a>(tree: &'a DataTree, key: &str) -> Result<&'a DataTree, Failure> {
        let fields = tree
            .as_object()
            .map_err(|error| Failure::internal(format!("candidate object {error}")))?;
        fields
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
            .ok_or_else(|| Failure::internal(format!("candidate missing `{key}`")))
    }

    fn candidate_inputs(tree: &DataTree) -> Result<Vec<(String, String)>, Failure> {
        tree_array(tree, "inputs")?
            .iter()
            .map(|entry| {
                Ok((
                    tree_object_field(entry, "path")?
                        .as_str()
                        .map(str::to_string)
                        .map_err(|error| {
                            Failure::internal(format!("candidate input path {error}"))
                        })?,
                    tree_object_field(entry, "digest")?
                        .as_str()
                        .map(str::to_string)
                        .map_err(|error| {
                            Failure::internal(format!("candidate input digest {error}"))
                        })?,
                ))
            })
            .collect()
    }

    fn candidate_strings(tree: &DataTree, key: &str) -> Result<Vec<String>, Failure> {
        tree_array(tree, key)?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .map_err(|error| Failure::internal(format!("candidate `{key}` {error}")))
            })
            .collect()
    }
    fn candidate_pairs(tree: &DataTree, key: &str) -> Result<Vec<(String, String)>, Failure> {
        tree_array(tree, key)?
            .iter()
            .map(|entry| {
                Ok((
                    tree_object_field(entry, "name")?
                        .as_str()
                        .map(str::to_string)
                        .map_err(|error| {
                            Failure::internal(format!("candidate `{key}` name {error}"))
                        })?,
                    tree_object_field(entry, "value")?
                        .as_str()
                        .map(str::to_string)
                        .map_err(|error| {
                            Failure::internal(format!("candidate `{key}` value {error}"))
                        })?,
                ))
            })
            .collect()
    }

    fn load_candidate(
        root: &Path,
        request: &Request,
        expected_digest: Option<&str>,
    ) -> Result<(DataTree, String, PathBuf), Failure> {
        let path = candidate_path(root, &format!("{}:{}", request.kind, request.build_dir));
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            Failure::user(format!(
                "no generated candidate for `{}:{}`; run preview first: {error}",
                request.kind, request.build_dir
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Failure::internal(format!(
                "generated candidate `{}` is not a regular non-symlink file",
                path.display()
            )));
        }
        let bytes = fs::read(&path).map_err(|error| {
            Failure::internal(format!(
                "could not read generated candidate `{}`: {error}",
                path.display()
            ))
        })?;
        let actual = format!("sha256-{}", jet::SHA256::sha256_hex(&bytes));
        if let Some(expected) = expected_digest {
            if expected != actual {
                return Err(Failure::user(format!(
                    "stale plan digest: expected `{expected}`, found `{actual}`; previous selection was preserved"
                )));
            }
        }
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| Failure::internal("generated candidate is not UTF-8"))?;
        let tree = parse_json(source).map_err(|error| {
            Failure::internal(format!("generated candidate is not valid JSON: {error:?}"))
        })?;
        if tree_string(&tree, "schema")? != CANDIDATE_SCHEMA {
            return Err(Failure::internal(
                "generated candidate has an unsupported schema",
            ));
        }
        if tree_string(&tree, "selection_key")? != format!("{}:{}", request.kind, request.build_dir)
        {
            return Err(Failure::user(
                "generated candidate belongs to a different foreign build; previous selection was preserved",
            ));
        }
        Ok((tree, actual, path))
    }

    fn verify_inputs(root: &Path, tree: &DataTree) -> Result<(), Failure> {
        for (relative, expected) in candidate_inputs(tree)? {
            let bytes = read_regular(root, &relative).map_err(|error| match error {
                Failure::User(message) => Failure::user(format!(
                    "candidate input drifted: `{relative}` is unavailable ({message}); previous selection was preserved"
                )),
                Failure::Internal(message) => Failure::Internal(message),
            })?;
            let actual = format!("sha256-{}", jet::SHA256::sha256_hex(&bytes));
            if actual != expected {
                return Err(Failure::user(format!(
                    "candidate input drifted: `{relative}` expected `{expected}`, found `{actual}`; previous selection was preserved"
                )));
            }
        }
        Ok(())
    }

    fn lock_path(root: &Path) -> PathBuf {
        root.join(".jet").join("lock")
    }

    fn selected_digest(root: &Path, key: &str) -> Result<Option<String>, Failure> {
        let path = lock_path(root);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(Failure::internal(format!(
                    "could not inspect `{}`: {error}",
                    path.display()
                )))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Failure::internal(format!(
                "lock `{}` is not a regular non-symlink file",
                path.display()
            )));
        }
        let raw = fs::read_to_string(&path).map_err(|error| {
            Failure::internal(format!("could not read `{}`: {error}", path.display()))
        })?;
        let lock = jet::Lock::parse(&raw).map_err(|error| {
            Failure::internal(format!("could not parse `{}`: {error}", path.display()))
        })?;
        Ok(lock
            .build_contributions
            .iter()
            .find(|row| row.package == SELECTION_PACKAGE && row.key == key)
            .map(|row| row.value.clone()))
    }

    fn select_plan(root: &Path, key: &str, digest: &str) -> Result<(), Failure> {
        let path = lock_path(root);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            Failure::user(format!(
                "cannot activate a foreign build without the existing `.jet/lock`: {error}"
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Failure::user(format!(
                "lock `{}` is not a regular non-symlink file",
                path.display()
            )));
        }
        let raw = fs::read_to_string(&path).map_err(|error| {
            Failure::internal(format!("could not read `{}`: {error}", path.display()))
        })?;
        let mut lock = jet::Lock::parse(&raw).map_err(|error| {
            Failure::internal(format!("could not parse `{}`: {error}", path.display()))
        })?;
        lock.build_contributions
            .retain(|row| !(row.package == SELECTION_PACKAGE && row.key == key));
        lock.build_contributions
            .push(jet::Lock::LockedBuildContribution {
                package: SELECTION_PACKAGE.to_string(),
                key: key.to_string(),
                value: digest.to_string(),
                scope: "project".to_string(),
                layer: "explicit-accept".to_string(),
                source: "jet build --import".to_string(),
                reason: "reviewed foreign build plan selected explicitly".to_string(),
            });
        let replacement = jet::Lock::write(&lock);
        jet::Lock::write_lock_atomically(root, replacement.as_bytes()).map_err(|error| {
            Failure::internal(format!("could not atomically select foreign plan: {error}"))
        })
    }

    fn parse_request(args: &[String]) -> Result<Request, Failure> {
        let position = args
            .iter()
            .position(|argument| argument == "--import")
            .ok_or_else(|| Failure::user("foreign build import requires `--import cmake:build`"))?;
        let spec = args.get(position + 1).ok_or_else(|| {
            Failure::user("foreign build import requires a kind and build directory")
        })?;
        let (kind, build_dir) = spec
            .split_once(':')
            .ok_or_else(|| Failure::user("foreign build import must use `kind:build-directory`"))?;
        if kind.is_empty() || build_dir.is_empty() {
            return Err(Failure::user(
                "foreign build import must use `kind:build-directory`",
            ));
        }
        relative_path(build_dir)?;
        let preview = args.iter().any(|argument| argument == "--preview");
        let accept_position = args.iter().position(|argument| argument == "--accept");
        if preview && accept_position.is_some() {
            return Err(Failure::user(
                "foreign build import accepts either `--preview` or `--accept`, not both",
            ));
        }
        let operation = if preview {
            Operation::Preview
        } else if let Some(position) = accept_position {
            let digest = args.get(position + 1).ok_or_else(|| {
                Failure::user("foreign build import `--accept` requires a plan digest")
            })?;
            if digest.starts_with('-') {
                return Err(Failure::user(
                    "foreign build import `--accept` requires a plan digest",
                ));
            }
            Operation::Accept(digest.clone())
        } else {
            return Err(Failure::user(
                "foreign build import requires `--preview` or `--accept <plan-digest>`",
            ));
        };
        Ok(Request {
            kind: kind.to_string(),
            build_dir: display_relative(&relative_path(build_dir)?),
            operation,
        })
    }

    fn import_banned() -> bool {
        super::env_truthy("CI") && !super::env_truthy("JET_ALLOW_FOREIGN_BUILD_IMPORT")
    }

    fn report_failure(error: Failure, mode: OutputMode) -> ! {
        match error {
            Failure::User(message) => {
                crate::cli_error!(
                    @fix "E2104",
                    message,
                    "run the preview, review every action and input, then accept the exact plan digest"
                );
                exit(ExitCodes::USER_ERROR);
            }
            Failure::Internal(message) => {
                write_mode_diagnostic(
                    mode,
                    &jet::Diagnostics::render_ice_report(
                        "foreign build import failed",
                        &message,
                        false,
                    ),
                );
                exit(ExitCodes::ICE);
            }
        }
    }

    pub(super) fn requested(args: &[String]) -> bool {
        args.iter().any(|argument| argument == "--import")
    }

    pub(super) fn run(args: &[String], mode: OutputMode) -> ! {
        if import_banned() {
            report_failure(
                Failure::user(
                    "foreign build import is disabled by the CI build policy; set `JET_ALLOW_FOREIGN_BUILD_IMPORT=1` only for an explicitly reviewed job",
                ),
                mode,
            );
        }
        let request = parse_request(args).unwrap_or_else(|error| report_failure(error, mode));
        match &request.operation {
            Operation::Preview => {
                let root = std::env::current_dir()
                    .map_err(|error| {
                        Failure::internal(format!("could not resolve project root: {error}"))
                    })
                    .unwrap_or_else(|error| report_failure(error, mode));
                let (path, candidate, digest) = build_candidate(&root, &request)
                    .unwrap_or_else(|error| report_failure(error, mode));
                let rendered = candidate_json(&candidate);
                if mode.json {
                    write_mode_machine(mode, &rendered);
                } else {
                    let custom = candidate
                        .actions
                        .iter()
                        .filter(|action| action.custom)
                        .map(|action| action.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let unsupported = if candidate.unsupported.is_empty() {
                        "none".to_string()
                    } else {
                        candidate.unsupported.join(" | ")
                    };
                    write_mode_renderable(
                        mode,
                        &format!(
                            "foreign build import preview\nplan-digest: {digest}\nselection: {}\nownership: {}\ndependencies: {}\ncommands: {}\ntoolchain identities: {}\ninputs: {}\noutputs: {}\nenvironment: {}\nworkdirs: {}\ncustom actions: {}\nunsupported edges: {}\ncandidate: {}\n",
                            candidate.key,
                            candidate.ownership,
                            candidate
                                .inputs
                                .iter()
                                .map(|(path, _)| path.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate
                                .actions
                                .iter()
                                .map(|action| action.command.join(" "))
                                .collect::<Vec<_>>()
                                .join(" | "),
                            candidate
                                .toolchains
                                .iter()
                                .map(|(key, value)| format!("{key}={value}"))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate
                                .inputs
                                .iter()
                                .map(|(path, digest)| format!("{path} ({digest})"))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate.outputs.join(", "),
                            candidate
                                .environment
                                .iter()
                                .map(|(key, value)| format!("{key}={value}"))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate.workdirs.join(", "),
                            if custom.is_empty() { "none" } else { &custom },
                            unsupported,
                            path.display()
                        ),
                    );
                }
            }
            Operation::Accept(digest) => {
                let root = std::env::current_dir()
                    .map_err(|error| {
                        Failure::internal(format!("could not resolve project root: {error}"))
                    })
                    .unwrap_or_else(|error| report_failure(error, mode));
                let (tree, actual, _) = load_candidate(&root, &request, Some(digest))
                    .unwrap_or_else(|error| report_failure(error, mode));
                verify_inputs(&root, &tree).unwrap_or_else(|error| report_failure(error, mode));
                if tree_string(&tree, "ownership").unwrap_or_default() != "partial" {
                    report_failure(
                        Failure::user(
                            "foreign build candidate did not disclose partial ownership; full ownership requires every action to be modeled",
                        ),
                        mode,
                    );
                }
                let key = format!("{}:{}", request.kind, request.build_dir);
                select_plan(&root, &key, &actual)
                    .unwrap_or_else(|error| report_failure(error, mode));
                if mode.json {
                    write_mode_machine(
                        mode,
                        &format!(
                            "{{\"schema\":\"jet-ffi-build-activation-v1\",\"status\":\"accepted\",\"selection_key\":\"{}\",\"plan_digest\":\"{}\",\"ownership\":\"partial\"}}\n",
                            json_escape(&key),
                            json_escape(&actual)
                        ),
                    );
                } else {
                    write_mode_renderable(
                        mode,
                        &format!("foreign build plan accepted: {key} ({actual})\n"),
                    );
                }
            }
        }
        exit(ExitCodes::OK);
    }

    pub(super) fn run_selected(mode: OutputMode) -> Option<i32> {
        let root = match std::env::current_dir() {
            Ok(root) => root,
            Err(error) => {
                write_mode_diagnostic(
                    mode,
                    &jet::Diagnostics::render_ice_report(
                        "selected foreign build failed",
                        &format!("could not resolve project root: {error}"),
                        false,
                    ),
                );
                return Some(ExitCodes::ICE);
            }
        };
        let key = match selected_digest(&root, "cmake:build") {
            Ok(Some(key)) => key,
            Ok(None) => return None,
            Err(Failure::User(message)) => {
                crate::cli_error!("E2104", "{}", message);
                return Some(ExitCodes::USER_ERROR);
            }
            Err(Failure::Internal(message)) => {
                write_mode_diagnostic(
                    mode,
                    &jet::Diagnostics::render_ice_report(
                        "selected foreign build failed",
                        &message,
                        false,
                    ),
                );
                return Some(ExitCodes::ICE);
            }
        };
        if import_banned() {
            crate::cli_error!(
                @fix "E2104",
                "the selected foreign build is disabled by the CI build policy",
                "set JET_ALLOW_FOREIGN_BUILD_IMPORT=1 only for an explicitly reviewed job"
            );
            return Some(ExitCodes::USER_ERROR);
        }
        let request = Request {
            kind: "cmake".to_string(),
            build_dir: "build".to_string(),
            operation: Operation::Accept(key.clone()),
        };
        let result = (|| {
            let (tree, actual, _) = load_candidate(&root, &request, Some(&key))?;
            verify_inputs(&root, &tree)?;
            let argv = candidate_strings(&tree, "argv")?;
            let outputs = candidate_strings(&tree, "outputs")?;
            let environment = candidate_pairs(&tree, "environment")?;
            let workdir = candidate_strings(&tree, "workdirs")?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    Failure::internal("selected foreign build has no working directory")
                })?;
            let workdir = relative_path(&workdir)?;
            let workdir = root.join(workdir);
            let metadata = fs::symlink_metadata(&workdir).map_err(|error| {
                Failure::user(format!(
                    "selected foreign build working directory is unavailable: {error}"
                ))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(Failure::user(
                    "selected foreign build working directory is not a regular directory"
                        .to_string(),
                ));
            }
            if argv.is_empty() {
                return Err(Failure::internal("selected foreign build has no command"));
            }
            let mut command = Command::new(&argv[0]);
            command.args(argv.iter().skip(1));
            command.current_dir(&workdir);
            for (name, value) in environment {
                command.env(name, value);
            }
            let status = command.status().map_err(|error| {
                Failure::user(format!("could not run selected foreign build: {error}"))
            })?;
            if !status.success() {
                return Ok(Some(status.code().unwrap_or(ExitCodes::USER_ERROR)));
            }
            for output in outputs {
                let relative = relative_path(&output)?;
                let path = root.join(relative);
                let metadata = fs::symlink_metadata(&path).map_err(|error| {
                    Failure::user(format!(
                        "selected foreign build completed without declared output `{output}`: {error}"
                    ))
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(Failure::user(format!(
                        "selected foreign build output `{output}` is not a regular file"
                    )));
                }
            }
            if mode.json {
                write_mode_machine(
                    mode,
                    &format!(
                        "{{\"schema\":\"jet-ffi-build-execution-v1\",\"status\":\"built\",\"plan_digest\":\"{}\",\"ownership\":\"partial\"}}\n",
                        json_escape(&actual)
                    ),
                );
            }
            Ok(Some(ExitCodes::OK))
        })();
        match result {
            Ok(status) => status,
            Err(Failure::User(message)) => {
                crate::cli_error!("E2104", "{}", message);
                Some(ExitCodes::USER_ERROR)
            }
            Err(Failure::Internal(message)) => {
                write_mode_diagnostic(
                    mode,
                    &jet::Diagnostics::render_ice_report(
                        "selected foreign build failed",
                        &message,
                        false,
                    ),
                );
                Some(ExitCodes::ICE)
            }
        }
    }
}

pub(crate) fn foreign_build_import_requested(args: &[String]) -> bool {
    foreign_build_import::requested(args)
}

pub(crate) fn run_foreign_build_import(args: &[String], mode: OutputMode) -> ! {
    foreign_build_import::run(args, mode)
}

pub(crate) fn run_selected_foreign_build(mode: OutputMode) -> Option<i32> {
    foreign_build_import::run_selected(mode)
}
#[cfg(test)]

fn ensure_web_output_dir(path: &Path) -> Result<PathBuf, String> {
    let cwd = fs::canonicalize(".")
        .map_err(|e| format!("error: couldn't resolve the web output root: {e}"))?;
    ensure_web_directory(path).map_err(|e| {
        format!(
            "error: couldn't create the {} folder safely: {}",
            path.display(),
            e
        )
    })?;
    let real = fs::canonicalize(path)
        .map_err(|e| format!("error: couldn't resolve {}: {e}", path.display()))?;
    if !real.starts_with(&cwd) {
        return Err(format!(
            "error: web output directory `{}` escapes the working directory",
            path.display()
        ));
    }
    Ok(real)
}

#[cfg(test)]

fn validate_web_output_file(path: &Path, root: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "error: web output `{}` must not be a symlink",
                path.display()
            ));
        }
        if web_output_has_multiple_links(&metadata) {
            return Err(format!(
                "error: web output `{}` must not be a hard link",
                path.display()
            ));
        }
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let real_parent = fs::canonicalize(parent)
        .map_err(|e| format!("error: couldn't resolve web output parent: {e}"))?;
    if !real_parent.starts_with(root) {
        return Err(format!(
            "error: web output `{}` escapes the output directory",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]

fn web_output_has_multiple_links(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return metadata.is_file() && metadata.nlink() > 1;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.is_file() && metadata.number_of_links() > 1;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        false
    }
}

#[cfg(test)]

fn ensure_web_directory(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "web output directory must not be a symlink",
        )),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "web output path is not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_web_directory(parent)?;
            }
            fs::create_dir(path)
        }
        Err(error) => Err(error),
    }
}

fn strip_manifest_source_map(manifest: &str) -> String {
    let mut out = String::with_capacity(manifest.len());
    for line in manifest.lines() {
        if line.trim_start().starts_with("\"sourceMap\":") {
            continue;
        }
        // Drop a trailing comma left on the previous field when sourceMap was last-but-one.
        out.push_str(line);
        out.push('\n');
    }
    // Repair ",\n  \"partitions\"" style when sourceMap line removed mid-object:
    // the previous non-empty line may end with a comma already; if we removed
    // sourceMap, the prior line (traceMap) still has its comma — fine.
    // If sourceMap was alone after a non-comma line, JSON stays valid because
    // emit_manifest always puts sourceMap before partitions with commas on each
    // field line except the structural closers.
    out
}

/// Where `write_plugin_artifacts` put the final `.wasm` Component (and the
/// intermediate `.wit`/guest-Rust files, kept for `-v` reporting).
pub(crate) struct PluginBuildPaths {
    pub(crate) wit: PathBuf,
    pub(crate) guest_rust: PathBuf,
    pub(crate) core_wasm: PathBuf,
    pub(crate) component_wasm: PathBuf,
}

/// D-PLUGIN1=B / D-DEP-WASM1=A (c81): whether a plugin build step failed
/// because a required external tool is missing/failed to run (a clean E1259,
/// never an internal-compiler-error exit — I2 reserves ICE for our own
/// generated code being rejected) or because `rustc` rejected the
/// jet-generated guest Rust (a genuine internal compiler error — sema should
/// have caught this before codegen ever ran).
pub(crate) enum PluginBuildError {
    ToolFailure(String),
    GeneratedCodeRejected(String),
}

/// The ONE place that turns a compiled `PluginArtifacts` bundle into an actual
/// `.wasm` Component Model file on disk (I8, mirrors `write_web_artifacts`):
/// `rustc --target wasm32-unknown-unknown --crate-type cdylib` builds the core
/// module from the guest Rust, then `wasm-tools component embed` + `component
/// new` lift it into a typed component using the generated `.wit` world — no
/// wit-bindgen crate, no adapter shims; scalar exports use the canonical
/// Component Model ABI at the core-wasm level (including `cabi_realloc` for
/// Text; see `Codegen::Plugin` module doc).
pub(crate) fn write_plugin_artifacts(
    file: &str,
    plugin: &jet::Codegen::PluginArtifacts,
    verbose: bool,
    out_dir: &Path,
    mode: OutputMode,
) -> Result<PluginBuildPaths, PluginBuildError> {
    let step = |msg: String| {
        if verbose {
            write_mode_status(mode, &format!("[build] {msg}\n"));
        }
    };
    fs::create_dir_all(out_dir).map_err(|e| {
        PluginBuildError::ToolFailure(format!(
            "couldn't create the {} folder: {}",
            out_dir.display(),
            e
        ))
    })?;
    let name = stem(file);
    let wit_dir = out_dir.join(format!("{name}_wit"));
    fs::create_dir_all(&wit_dir).map_err(|e| {
        PluginBuildError::ToolFailure(format!("couldn't create {}: {}", wit_dir.display(), e))
    })?;
    let wit_path = wit_dir.join("world.wit");
    let guest_rust_path = out_dir.join(format!("{name}_plugin.rs"));
    let core_wasm_path = out_dir.join(format!("{name}_core.wasm"));
    let embedded_wasm_path = out_dir.join(format!("{name}_embedded.wasm"));
    let component_wasm_path = out_dir.join(format!("{name}.wasm"));

    fs::write(&wit_path, &plugin.wit).map_err(|e| {
        PluginBuildError::ToolFailure(format!("couldn't write {}: {}", wit_path.display(), e))
    })?;
    fs::write(&guest_rust_path, &plugin.guest_rust).map_err(|e| {
        PluginBuildError::ToolFailure(format!(
            "couldn't write {}: {}",
            guest_rust_path.display(),
            e
        ))
    })?;

    step(format!("wit emit    -> {}", wit_path.display()));
    step(format!("guest rust  -> {}", guest_rust_path.display()));
    step(format!(
        "rustc wasm  {} -> {}",
        guest_rust_path.display(),
        core_wasm_path.display()
    ));
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "--crate-name",
        ])
        .arg(jet::Syntax::sanitize_crate_name(
            guest_rust_path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("out"),
        ))
        .args([
            guest_rust_path.to_str().unwrap(),
            "-o",
            core_wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| {
            PluginBuildError::ToolFailure(format!(
                "couldn't run `rustc --target wasm32-unknown-unknown` ({e}) — is the wasm32-unknown-unknown target installed?"
            ))
        })?;
    if !rustc.status.success() {
        return Err(PluginBuildError::GeneratedCodeRejected(format!(
            "rustc rejected the generated plugin guest module\n{}",
            String::from_utf8_lossy(&rustc.stderr)
        )));
    }

    step(format!(
        "wit embed   {} -> {}",
        core_wasm_path.display(),
        embedded_wasm_path.display()
    ));
    let embed = Command::new("wasm-tools")
        .args([
            "component",
            "embed",
            wit_dir.to_str().unwrap(),
            "--world",
            &plugin.world_name,
            core_wasm_path.to_str().unwrap(),
            "-o",
            embedded_wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| {
            PluginBuildError::ToolFailure(format!(
                "couldn't run `wasm-tools` ({e}) — install it (ships in the project's `nix develop` shell) or add it to PATH"
            ))
        })?;
    if !embed.status.success() {
        return Err(PluginBuildError::ToolFailure(format!(
            "`wasm-tools component embed` failed\n{}",
            String::from_utf8_lossy(&embed.stderr)
        )));
    }

    step(format!(
        "wit lift    {} -> {}",
        embedded_wasm_path.display(),
        component_wasm_path.display()
    ));
    let new = Command::new("wasm-tools")
        .args([
            "component",
            "new",
            embedded_wasm_path.to_str().unwrap(),
            "-o",
            component_wasm_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| PluginBuildError::ToolFailure(format!("couldn't run `wasm-tools` ({e})")))?;
    if !new.status.success() {
        return Err(PluginBuildError::ToolFailure(format!(
            "`wasm-tools component new` failed\n{}",
            String::from_utf8_lossy(&new.stderr)
        )));
    }

    Ok(PluginBuildPaths {
        wit: wit_path,
        guest_rust: guest_rust_path,
        core_wasm: core_wasm_path,
        component_wasm: component_wasm_path,
    })
}

struct LibraryBuildPaths {
    shared: Option<PathBuf>,
    staticlib: Option<PathBuf>,
    header: Option<PathBuf>,
    loadable: Option<PathBuf>,
    bindings: Vec<PathBuf>,
}

#[derive(Debug)]
enum LibraryBuildError {
    Io(String),
    Tool(String),
    GeneratedCode(String),
}

impl std::fmt::Display for LibraryBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) | Self::Tool(error) => formatter.write_str(error),
            Self::GeneratedCode(stderr) => formatter.write_str(stderr),
        }
    }
}

impl From<std::io::Error> for LibraryBuildError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

struct LibraryStage {
    path: PathBuf,
}

impl Drop for LibraryStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn library_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validate_library_output_root(root: &Path) -> Result<(), LibraryBuildError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => {
            if library_reparse_point(&metadata) || !metadata.is_dir() {
                return Err(LibraryBuildError::Io(format!(
                    "Library output root `{}` is not a real directory",
                    root.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(root)?;
        }
        Err(error) => return Err(error.into()),
    }
    let first = fs::canonicalize(root)?;
    let second = fs::canonicalize(root)?;
    if first != second {
        return Err(LibraryBuildError::Io(format!(
            "Library output root `{}` changed while it was being resolved",
            root.display()
        )));
    }
    let metadata = fs::symlink_metadata(root)?;
    if library_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(LibraryBuildError::Io(format!(
            "Library output root `{}` changed to a symlink or reparse point",
            root.display()
        )));
    }
    Ok(())
}

fn library_owned_paths(target: &Path, stem: &str) -> Vec<PathBuf> {
    [
        format!("lib{stem}.a"),
        format!("lib{stem}.so"),
        format!("lib{stem}.dylib"),
        format!("lib{stem}.dll"),
        format!("{stem}.h"),
        format!("{stem}.jetlib"),
        format!("{stem}.rs"),
        format!(".{stem}.jet-library.complete"),
    ]
    .into_iter()
    .map(|name| target.join(name))
    .chain(
        ["h", "hpp", "rs", "zig", "go", "py", "mjs", "swift"]
            .into_iter()
            .map(|extension| target.join("bindings").join(format!("{stem}.{extension}"))),
    )
    .collect()
}

fn remove_library_owned_path(path: &Path) -> Result<(), LibraryBuildError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if library_reparse_point(&metadata) {
        return Err(LibraryBuildError::Io(format!(
            "refusing to remove symlink or reparse point `{}`",
            path.display()
        )));
    }
    if metadata.is_dir() {
        return Err(LibraryBuildError::Io(format!(
            "refusing to remove directory where a Library file belongs: `{}`",
            path.display()
        )));
    }
    fs::remove_file(path)?;
    Ok(())
}

fn validate_library_bindings_root(target: &Path) -> Result<(), LibraryBuildError> {
    let root = target.join("bindings");
    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if library_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(LibraryBuildError::Io(format!(
            "Library bindings root `{}` is not a real directory",
            root.display()
        )));
    }
    let first = fs::canonicalize(&root)?;
    let second = fs::canonicalize(&root)?;
    if first != second {
        return Err(LibraryBuildError::Io(format!(
            "Library bindings root `{}` changed while it was being resolved",
            root.display()
        )));
    }
    let metadata = fs::symlink_metadata(&root)?;
    if library_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(LibraryBuildError::Io(format!(
            "Library bindings root `{}` changed to a symlink or reparse point",
            root.display()
        )));
    }
    Ok(())
}

fn clean_library_outputs(target: &Path, stem: &str) -> Result<(), LibraryBuildError> {
    // Validate both parents before inspecting any owned output. A swapped or
    // symlinked root would otherwise make stale-output cleanup leave the
    // target tree and remove an unrelated path.
    validate_library_output_root(target)?;
    validate_library_bindings_root(target)?;
    for path in library_owned_paths(target, stem) {
        remove_library_owned_path(&path)?;
    }
    let stage_prefix = format!(".jet-library-stage-{stem}-{}-", std::process::id());
    for entry in fs::read_dir(target)? {
        let entry = entry?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(&stage_prefix) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if library_reparse_point(&metadata) || !metadata.is_dir() {
            return Err(LibraryBuildError::Io(format!(
                "refusing to remove invalid Library staging path `{}`",
                entry.path().display()
            )));
        }
        fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}

fn new_library_stage(target: &Path, stem: &str) -> Result<LibraryStage, LibraryBuildError> {
    for serial in 0..64u32 {
        let path = target.join(format!(
            ".jet-library-stage-{stem}-{}-{serial}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(LibraryStage { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(LibraryBuildError::Io(
        "could not allocate a unique Library staging directory".to_string(),
    ))
}

fn write_library_staged(path: &Path, bytes: &[u8]) -> Result<(), LibraryBuildError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn publish_library_file(staged: &Path, final_path: &Path) -> Result<(), LibraryBuildError> {
    if let Ok(metadata) = fs::symlink_metadata(final_path) {
        if library_reparse_point(&metadata) {
            return Err(LibraryBuildError::Io(format!(
                "refusing to replace symlink or reparse point `{}`",
                final_path.display()
            )));
        }
        if metadata.is_dir() {
            return Err(LibraryBuildError::Io(format!(
                "refusing to replace directory `{}`",
                final_path.display()
            )));
        }
        #[cfg(windows)]
        fs::remove_file(final_path)?;
    }
    fs::rename(staged, final_path)?;
    Ok(())
}

fn library_stem(name: &str) -> String {
    let mut stem = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if stem.is_empty() {
        stem.push_str("library");
    }
    if stem.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        stem.insert(0, '_');
    }
    stem
}

fn library_rustc(
    source: &Path,
    crate_type: &str,
    output: &Path,
    profile: BuildProfile,
    verbose: bool,
    linker: &crate::NativeLinker::Selection,
    ffi: Option<&jet::FFI::FfiLink>,
    mode: OutputMode,
) -> Result<(), LibraryBuildError> {
    let mut command = Command::new("rustc");
    command
        .arg("--edition")
        .arg("2021")
        .arg("--crate-type")
        .arg(crate_type)
        .arg("--crate-name")
        .arg(jet::Syntax::sanitize_crate_name(
            source
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("out"),
        ))
        .arg(source)
        .arg("-o")
        .arg(output);
    let config = profile.config();
    command.args(release_profile_cfg_args(&profile));
    command.args(config.rustc_args_for_target(ffi.is_some(), true));
    command.args(linker.rustc_args());
    if let Some(link) = ffi {
        command
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            command
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    if verbose {
        write_mode_status(
            mode,
            &format!(
                "[build] rustc {} -> {} (linker {})\n",
                source.display(),
                output.display(),
                linker.label()
            ),
        );
    }
    let result = command
        .output()
        .map_err(|error| LibraryBuildError::Tool(format!("couldn't start rustc: {error}")))?;
    if !result.status.success() {
        return Err(LibraryBuildError::GeneratedCode(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    Ok(())
}

/// Canonicalize the metadata fields in a rustc-produced Unix archive.
///
/// Rustc's static-library member order and payloads are already content
/// addressed by the generated source.  The archive container still records
/// process metadata, so normalize those fixed-width fields before publishing
/// the artifact.  This keeps repeated library builds byte-identical without
/// depending on an external `ar` invocation.
fn normalize_static_archive(path: &Path) -> Result<(), LibraryBuildError> {
    const GLOBAL_HEADER: &[u8] = b"!<arch>\n";
    const MEMBER_HEADER: usize = 60;
    let mut archive = fs::read(path)?;
    if !archive.starts_with(GLOBAL_HEADER) {
        return Err(LibraryBuildError::GeneratedCode(format!(
            "rustc produced an invalid static archive `{}`",
            path.display()
        )));
    }
    let mut offset = GLOBAL_HEADER.len();
    while offset < archive.len() {
        let end = offset.checked_add(MEMBER_HEADER).ok_or_else(|| {
            LibraryBuildError::GeneratedCode(format!(
                "static archive `{}` has an overflowing member header",
                path.display()
            ))
        })?;
        if end > archive.len() || &archive[offset + 58..end] != b"`\n" {
            return Err(LibraryBuildError::GeneratedCode(format!(
                "rustc produced a malformed static archive `{}`",
                path.display()
            )));
        }
        // GNU archive fields: mtime [16..28], uid [28..34], gid [34..40],
        // mode [40..48].  Keep the member name and size untouched.
        for (start, width, value) in [(16, 12, b"0"), (28, 6, b"0"), (34, 6, b"0")] {
            archive[offset + start..offset + start + width].fill(b' ');
            archive[offset + start] = value[0];
        }
        archive[offset + 40..offset + 48].fill(b' ');
        archive[offset + 40..offset + 46].copy_from_slice(b"100644");
        let size = std::str::from_utf8(&archive[offset + 48..offset + 58])
            .ok()
            .and_then(|field| field.trim().parse::<usize>().ok())
            .ok_or_else(|| {
                LibraryBuildError::GeneratedCode(format!(
                    "static archive `{}` has an invalid member size",
                    path.display()
                ))
            })?;
        let data_end = end.checked_add(size).ok_or_else(|| {
            LibraryBuildError::GeneratedCode(format!(
                "static archive `{}` has an overflowing member payload",
                path.display()
            ))
        })?;
        if data_end > archive.len() {
            return Err(LibraryBuildError::GeneratedCode(format!(
                "static archive `{}` has a truncated member payload",
                path.display()
            )));
        }
        offset = data_end + (size & 1);
    }
    fs::write(path, archive)?;
    Ok(())
}

fn build_library(
    rust_code: &str,
    artifacts: &jet::Codegen::LibraryArtifacts,
    config: &jet::LibraryExport::LibraryConfig,
    profile: BuildProfile,
    ffi: Option<&jet::FFI::FfiLink>,
    verbose: bool,
    mode: OutputMode,
) -> Result<LibraryBuildPaths, LibraryBuildError> {
    let stem = library_stem(&config.name);
    let target = PathBuf::from("target");
    validate_library_output_root(&target)?;
    clean_library_outputs(&target, &stem)?;
    let stage = new_library_stage(&target, &stem)?;
    let needs_shared = config.native || config.loadable;
    let shared_name = format!(
        "lib{stem}.{}",
        if cfg!(target_os = "macos") {
            "dylib"
        } else if cfg!(target_os = "windows") {
            "dll"
        } else {
            "so"
        }
    );
    let shared = needs_shared.then(|| target.join(&shared_name));
    let staged_shared = needs_shared.then(|| stage.path.join(&shared_name));
    let staticlib = config.native.then(|| target.join(format!("lib{stem}.a")));
    let staged_staticlib = config
        .native
        .then(|| stage.path.join(format!("lib{stem}.a")));
    let header = config.native.then(|| target.join(format!("{stem}.h")));
    let staged_header = config.native.then(|| stage.path.join(format!("{stem}.h")));
    let bindings_dir = config.native.then(|| target.join("bindings"));
    let staged_bindings_dir = config.native.then(|| stage.path.join("bindings"));
    let linker = crate::NativeLinker::for_target(None);

    if needs_shared {
        let source = stage.path.join(format!("{stem}.rs"));
        write_library_staged(&source, rust_code.as_bytes())?;
        let shared_path = staged_shared
            .as_ref()
            .expect("shared path for native Library");
        library_rustc(
            &source,
            "cdylib",
            shared_path,
            profile.clone(),
            verbose,
            &linker,
            ffi,
            mode,
        )?;
        if let Some(staticlib_path) = &staged_staticlib {
            library_rustc(
                &source,
                "staticlib",
                staticlib_path,
                profile.clone(),
                verbose,
                &linker,
                ffi,
                mode,
            )?;
            normalize_static_archive(staticlib_path)?;
        }
    }

    if let Some(staged_header) = &staged_header {
        write_library_staged(staged_header, artifacts.header.as_bytes())?;
    }

    let mut binding_paths = Vec::new();
    let mut staged_binding_paths = Vec::new();
    if let Some(staged_bindings_dir) = &staged_bindings_dir {
        fs::create_dir(staged_bindings_dir)?;
        for (language, text) in &artifacts.bindings {
            let extension = match language.as_str() {
                "c" => "h",
                "cpp" => "hpp",
                "rust" => "rs",
                "zig" => "zig",
                "go" => "go",
                "python" => "py",
                "javascript" => "mjs",
                "swift" => "swift",
                _ => continue,
            };
            let path = target.join("bindings").join(format!("{stem}.{extension}"));
            let staged_path = staged_bindings_dir.join(format!("{stem}.{extension}"));
            write_library_staged(&staged_path, text.as_bytes())?;
            binding_paths.push(path);
            staged_binding_paths.push(staged_path);
        }
    }

    let loadable = config
        .loadable
        .then(|| target.join(format!("{stem}.jetlib")));
    let staged_loadable = if config.loadable {
        let path = stage.path.join(format!("{stem}.jetlib"));
        let shared_path = staged_shared
            .as_ref()
            .expect("loadable Library has shared payload");
        let payload = fs::read(shared_path)?;
        let exports = artifacts
            .exports
            .iter()
            .map(|export| {
                let scalar = match export.scalar {
                    jet::Codegen::ExportScalar::Int => jet::JetLibScalar::Int,
                    jet::Codegen::ExportScalar::Float => jet::JetLibScalar::Float,
                    jet::Codegen::ExportScalar::Bool => jet::JetLibScalar::Bool,
                    jet::Codegen::ExportScalar::Text => jet::JetLibScalar::Text,
                };
                let conventions = export
                    .conventions
                    .iter()
                    .map(|convention| match convention {
                        jet_foundation::MIR::MirAccess::Read => jet::JetLibAccess::Read,
                        jet_foundation::MIR::MirAccess::Write => jet::JetLibAccess::Write,
                        jet_foundation::MIR::MirAccess::Move => jet::JetLibAccess::Move,
                    })
                    .collect();
                jet::JetLibExport::with_conventions(export.name.clone(), scalar, conventions)
            })
            .collect();
        let mut stamp = jet::JetLibStamp::for_library_with_identity(
            config.name.clone(),
            exports,
            config.declared_effects.clone(),
            env!("JET_COMPILER_BUILD_ID"),
            env!("JET_BUILD_TARGET"),
            native_toolchain_identity(),
        );
        stamp.entry = config.entry.clone();
        stamp.seal_payload(&payload);
        let artifact = jet::JetLibArtifact { stamp, payload };
        write_library_staged(&path, &artifact.encode())?;
        Some(path)
    } else {
        None
    };

    validate_library_output_root(&target)?;
    if let Some(bindings_dir) = &bindings_dir {
        validate_library_bindings_root(&target)?;
        match fs::symlink_metadata(bindings_dir) {
            Ok(metadata) if library_reparse_point(&metadata) || !metadata.is_dir() => {
                return Err(LibraryBuildError::Io(format!(
                    "Library bindings root `{}` is not a real directory",
                    bindings_dir.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(bindings_dir)?;
            }
            Err(error) => return Err(error.into()),
        }
    }

    let mut publications = Vec::new();
    if let (Some(shared), Some(staged_shared)) = (&shared, &staged_shared) {
        publications.push((staged_shared.clone(), shared.clone()));
    }
    if let (Some(staticlib), Some(staged_staticlib)) = (&staticlib, &staged_staticlib) {
        publications.push((staged_staticlib.clone(), staticlib.clone()));
    }
    if let (Some(header), Some(staged_header)) = (&header, &staged_header) {
        publications.push((staged_header.clone(), header.clone()));
    }
    publications.extend(
        staged_binding_paths
            .into_iter()
            .zip(binding_paths.iter().cloned()),
    );
    if let (Some(loadable), Some(staged_loadable)) = (&loadable, &staged_loadable) {
        publications.push((staged_loadable.clone(), loadable.clone()));
    }
    let completion = target.join(format!(".{stem}.jet-library.complete"));
    let staged_completion = stage.path.join(format!(".{stem}.jet-library.complete"));
    let mut completion_text = String::from("jet-library-set-v1\n");
    for (staged_path, final_path) in &publications {
        let relative = final_path.strip_prefix(&target).map_err(|_| {
            LibraryBuildError::Io(format!(
                "Library publication path `{}` escaped target root",
                final_path.display()
            ))
        })?;
        let digest = jet::SHA256::sha256_hex(&fs::read(staged_path)?);
        completion_text.push_str(&format!("{}\tsha256-{digest}\n", relative.display()));
    }
    write_library_staged(&staged_completion, completion_text.as_bytes())?;
    publications.push((staged_completion, completion));
    validate_library_output_root(&target)?;
    validate_library_bindings_root(&target)?;
    for (staged_path, final_path) in publications {
        if let Err(error) = publish_library_file(&staged_path, &final_path) {
            let _ = clean_library_outputs(&target, &stem);
            return Err(error);
        }
    }

    if let Err(error) = validate_library_output_root(&target) {
        let _ = clean_library_outputs(&target, &stem);
        return Err(error);
    }
    Ok(LibraryBuildPaths {
        shared,
        staticlib,
        header,
        loadable,
        bindings: binding_paths,
    })
}

fn build_inner(
    file: &str,
    rust_code: &str,
    runtime_bundle: Option<&jet::AST::ProgramBundle>,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::FFI::FfiLink>,
    clinks: &[String],
    verbose: bool,
    cross_target: Option<&str>,
    web: Option<&jet::Codegen::MIRWeb::WebArtifacts>,
    plugin: Option<&jet::Codegen::PluginArtifacts>,
    mode: OutputMode,
    _restored_cache: bool,
    // D-BUILDNORM1=A (Tower #85): the content-addressed cache key, computed by
    // the caller from the checked canonical AST, profile, toolchain, manifest,
    // runtime/Core, dependency-interface, instance, and optional Rust FFI bridge
    // identities. `None` when external bytes are not represented by the key,
    // such as `embed_file` or explicit C links. `build` also rejects cross-target
    cache_key: Option<String>,
    target_machine: Option<&jet::TargetMachine::TargetMachine>,
) {
    // D-BUILD2: `jet build -v` makes the hidden Jet→Rust→native bridge honest.
    // Step labels are deterministic so they can be golden-tested.
    let step = |msg: String| {
        if verbose {
            write_mode_status(mode, &format!("[build] {msg}\n"));
        }
    };
    let native_store = Store::from_env().ok();
    let output_names = bin
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| vec![name.to_string()])
        .unwrap_or_default();
    let compiler_nodes = runtime_bundle
        .map(|bundle| {
            jet::Driver::compiler_nodes_for_bundle_with_outputs(bundle, None, &output_names)
        })
        .unwrap_or_default();
    let record_program = build_record_program(file, runtime_bundle);
    let previous_record = native_store
        .as_ref()
        .and_then(|store| store.latest_build_record(&record_program).ok().flatten());

    let output_authority =
        jet_devserver::WebHost::WebOutputAuthority::open_or_create(Path::new("build"))
            .unwrap_or_else(|error| {
                let message = format!("error: couldn't create the build folder safely: {error}");
                write_mode_diagnostic(mode, &format!("{message}\n"));
                exit(ExitCodes::USER_ERROR);
            });
    let rs_name = format!("{}.rs", stem(file));
    let rs_path = output_authority.path_for(&rs_name).unwrap_or_else(|error| {
        let message = format!("error: invalid web Rust output `{rs_name}`: {error}");
        write_mode_diagnostic(mode, &format!("{message}\n"));
        exit(ExitCodes::USER_ERROR);
    });
    step(format!("emit Rust  -> {}", rs_path.display()));
    output_authority
        .replace_file(&rs_name, rust_code.as_bytes())
        .unwrap_or_else(|error| {
            write_mode_diagnostic(
                mode,
                &format!("error: couldn't write {}: {}\n", rs_path.display(), error),
            );
        });
    // D-WEBKIND1=A (c123 M2): `web` is a Jet backend target — emit WASM + JS.
    if cross_target == Some(jet::Syntax::BUILD_TARGET_WEB) {
        let web = web.unwrap_or_else(|| {
            write_mode_diagnostic(
                mode,
                &jet::Diagnostics::render_ice_report("missing web codegen output", "", false),
            );
            exit(ExitCodes::ICE);
        });
        let emit_maps = !profile.is_release();
        let model_runtime = runtime_bundle.is_some_and(|bundle| !bundle.model_outputs().is_empty());
        let paths = match write_web_artifacts(
            file,
            web,
            ffi,
            model_runtime,
            verbose,
            &output_authority,
            emit_maps,
            mode,
        ) {
            Ok(p) => p,
            Err(msg) => {
                write_mode_diagnostic(mode, &format!("{msg}\n"));
                exit(ExitCodes::ICE);
            }
        };
        let _ = rust_code;
        let _ = bin;
        let _ = ffi;
        let _ = clinks;
        if !mode.json {
            let mut note = format!(
                "note: `--target=web` wrote `{}`, `{}`, `{}`, `{}`, `{}`",
                paths.manifest.display(),
                paths.dom.display(),
                paths.js.display(),
                paths.wasm.display(),
                paths.html.display(),
            );
            if let Some(js_map) = &paths.js_map {
                note.push_str(&format!(", `{}`", js_map.display()));
            }
            if let Some(wasm_map) = &paths.wasm_map {
                note.push_str(&format!(", `{}`", wasm_map.display()));
            }
            write_mode_status(mode, &format!("{note}\n"));
        }
        return;
    }

    // D-PLUGIN1=B (c81): `sandbox` is another Jet backend target — build the
    // sandboxed wasm32 Component Model module instead of a native binary.
    if cross_target == Some(jet::Syntax::TARGET_SANDBOX) {
        let plugin = plugin.unwrap_or_else(|| {
            write_mode_diagnostic(
                mode,
                &jet::Diagnostics::render_ice_report("missing sandbox codegen output", "", false),
            );
            exit(ExitCodes::ICE);
        });
        let paths = match write_plugin_artifacts(file, plugin, verbose, Path::new("build"), mode) {
            Ok(p) => p,
            Err(PluginBuildError::GeneratedCodeRejected(msg)) => {
                write_mode_diagnostic(
                    mode,
                    &jet::Diagnostics::render_ice_report(
                        "rustc rejected generated code",
                        &msg,
                        true,
                    ),
                );
                exit(ExitCodes::ICE);
            }
            Err(PluginBuildError::ToolFailure(msg)) => {
                let diag = jet::Manifest::e1259(&msg);
                let src = format!("// sandbox build for {}", file);
                report_problems(mode, "<sandbox>", &src, &[diag]);
                exit(ExitCodes::USER_ERROR);
            }
        };
        let _ = profile;
        let _ = ffi;
        let _ = clinks;
        write_mode_status(
            mode,
            &format!(
                "note: `--target=sandbox` wrote `{}`, `{}`, `{}`, `{}`\n",
                paths.wit.display(),
                paths.guest_rust.display(),
                paths.core_wasm.display(),
                paths.component_wasm.display(),
            ),
        );
        return;
    }

    if let Some(machine) = target_machine.filter(|machine| {
        machine.no_os
            && (machine.triple.contains("aarch64")
                || machine.triple.contains("thumb")
                || machine.triple.starts_with("arm"))
    }) {
        // Link a Rust static archive so the checked program brings the target
        // core/alloc and compiler-builtins it uses. Startup alone is not the
        // program, and a bare --emit=obj omits its Rust runtime dependencies.
        let usage = runtime_bundle
            .map(|bundle| {
                jet::TargetMachine::TargetMachineUse::from_core_apis(bundle.used_core.iter())
            })
            .unwrap_or_default();
        let dossier = machine.target_dossier(
            &usage,
            jet::TargetMachine::ExecutionTier::Aot,
            env!("CARGO_PKG_VERSION"),
            "none",
        );
        let mut identity = b"jet-firmware-build-v1\0".to_vec();
        append_cache_field(&mut identity, rust_code);
        append_cache_field(&mut identity, &profile.cache_tag());
        append_cache_field(&mut identity, native_toolchain_identity());
        identity.extend_from_slice(&dossier.cache_bytes(&machine.triple));
        let target_digest = jet::SHA256::sha256_hex(&identity);
        let target_dir = PathBuf::from(".jet").join("target").join(format!(
            "{}-{target_digest}",
            jet::Syntax::sanitize_crate_name(&machine.name)
        ));
        let program_object = target_dir.join(format!(
            "{}.program.a",
            jet::Syntax::sanitize_crate_name(&stem(file))
        ));
        if let Err(error) = fs::create_dir_all(&target_dir) {
            write_mode_diagnostic(
                mode,
                &format!(
                    "error: couldn't create firmware work directory {}: {error}\n",
                    target_dir.display()
                ),
            );
            exit(ExitCodes::USER_ERROR);
        }
        let rustc_started = Instant::now();
        step(format!("rustc object -> {}", program_object.display()));
        let mut rustc = Command::new("rustc");
        rustc
            .arg("--edition")
            .arg("2021")
            .arg("--crate-type")
            .arg("staticlib")
            .arg("--emit")
            .arg("link")
            .arg("--target")
            .arg(&machine.triple)
            .arg("--crate-name")
            .arg(jet::Syntax::sanitize_crate_name(&stem(file)))
            .args(release_profile_cfg_args(&profile))
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("relocation-model=static")
            .arg(&rs_path)
            .arg("-o")
            .arg(&program_object);
        let rustc_output = match rustc.output() {
            Ok(output) => output,
            Err(error) => {
                write_mode_diagnostic(
                    mode,
                    &format!("error: couldn't run rustc for firmware object: {error}\n"),
                );
                exit(ExitCodes::USER_ERROR);
            }
        };
        if !rustc_output.status.success() {
            let detail = String::from_utf8_lossy(&rustc_output.stderr);
            write_mode_diagnostic(
                mode,
                &jet::Diagnostics::render_ice_report(
                    "the generated Rust did not compile for the selected target.",
                    &detail,
                    true,
                ),
            );
            exit(ExitCodes::ICE);
        }
        let artifacts = match jet::Driver::build_target_machine_firmware(
            machine,
            &usage,
            &program_object,
            &target_dir,
        ) {
            Ok(artifacts) => artifacts,
            Err(jet::Driver::TargetMachineCompileError::Diagnostics(diags)) => {
                report_problems(mode, file, rust_code, &diags);
                exit(ExitCodes::USER_ERROR);
            }
            Err(jet::Driver::TargetMachineCompileError::Machine(errors)) => {
                write_mode_diagnostic(
                    mode,
                    &format!("error: target machine firmware link failed: {errors:?}\n"),
                );
                exit(ExitCodes::USER_ERROR);
            }
        };
        if let Err(error) = fs::copy(&artifacts.elf, &bin) {
            write_mode_diagnostic(
                mode,
                &format!(
                    "error: couldn't publish firmware {}: {error}\n",
                    bin.display()
                ),
            );
            exit(ExitCodes::USER_ERROR);
        }
        persist_build_record(
            native_store.as_ref(),
            &record_program,
            &compiler_nodes,
            previous_record.as_ref(),
            false,
            rustc_started.elapsed().as_secs_f64() * 1000.0,
            0.0,
        );
        step(format!("firmware link -> {}", artifacts.elf.display()));
        return;
    }

    // Cross-compiled and explicit C-linked builds bypass the host binary cache.
    // Rust FFI bridges are cacheable because their content-addressed identity
    // is folded into the caller's native key.
    // D-BUILDNORM1=A: the key is the caller's pre-sema canonical-AST key
    // (D-BUILDPROFILE1's profile tag is already folded into it). Kept only when
    // this build is cacheable.
    if let Some(key) = &cache_key {
        if native_store.as_ref().is_some_and(|store| {
            matches!(
                store.restore_file(key, &bin),
                Ok(ArtifactRestore::Hit { .. })
            )
        }) {
            step("cache hit -> reused cached binary".to_string());
            persist_build_record(
                native_store.as_ref(),
                &record_program,
                &compiler_nodes,
                previous_record.as_ref(),
                true,
                0.0,
                0.0,
            );
            return;
        }
    }
    if verbose {
        if cache_key.is_some() {
            step("cache miss -> compiling".to_string());
        } else if cross_target.is_some() {
            step("cache bypassed (cross-compiled build)".to_string());
        } else {
            step("cache bypassed (C-linked build)".to_string());
        }
    }

    step(format!(
        "rustc      {} -> {}",
        rs_path.display(),
        bin.display()
    ));
    let mut rustc_flags = Vec::new();
    // A no-OS Wasm artifact is a reactor, not a hosted executable.  Keep the
    // selected entry export explicit so wasm-ld never asks for `main` or
    // synthesizes `_start`; the ordinary Web target still takes the separate
    // MIRWeb artifact path above.
    let no_os_wasm =
        target_machine.is_some_and(|machine| machine.no_os && machine.triple.starts_with("wasm32"));
    if no_os_wasm {
        rustc_flags.extend([
            "--crate-type".to_string(),
            "cdylib".to_string(),
            "-C".to_string(),
            "link-arg=--no-entry".to_string(),
            "-C".to_string(),
            "link-arg=--export=__jet_program_entry".to_string(),
        ]);
    }
    // E2-M15: cross-compilation target triple.
    let ffi_present = ffi.is_some();
    let config = profile.config();
    rustc_flags.extend(release_profile_cfg_args(&profile));
    rustc_flags.extend(config.rustc_args_for_target(ffi_present, cross_target.is_none()));
    let linker = crate::NativeLinker::for_target(cross_target);
    rustc_flags.extend(linker.rustc_args());
    if verbose {
        step(format!("linker     -> {}", linker.label()));
    }
    let cache_flags = rustc_flags
        .iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    let cache_env: Vec<(std::ffi::OsString, std::ffi::OsString)> = Vec::new();
    // Rust FFI glue can implement runtime traits for foreign types. Keep that
    let prepared_runtime = if ffi_present {
        if verbose {
            step("runtime store bypassed (Rust FFI glue)".to_string());
        }
        jet_store::runtime::PreparedRuntime::inline(rust_code)
    } else if let Some(store) = native_store.as_ref() {
        match jet_store::runtime::prepare(
            store,
            std::ffi::OsStr::new("rustc"),
            rust_code,
            &cache_flags,
            &cache_env,
        ) {
            Ok(prepared) => {
                if verbose {
                    let status = if prepared.cache_hit() {
                        "cache hit"
                    } else if prepared.is_split() {
                        "cache store"
                    } else {
                        "bypassed (inline fallback)"
                    };
                    step(format!("runtime   -> {status}"));
                }
                prepared
            }
            Err(jet_store::runtime::RuntimeError::Cache(error)) => {
                if verbose {
                    step(format!("runtime store bypassed ({error})"));
                }
                jet_store::runtime::PreparedRuntime::inline(rust_code)
            }
            Err(jet_store::runtime::RuntimeError::Tool(_)) => {
                crate::cli_error!(@full "E2105", "couldn't find `rustc` on this machine", "v1 of this language uses Rust as its backend (docs/spec/architecture.md)", "install Rust from https://rustup.rs, then try again");
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        if verbose {
            step("runtime store bypassed (store unavailable)".to_string());
        }
        jet_store::runtime::PreparedRuntime::inline(rust_code)
    };
    let model_runtime = runtime_bundle.is_some_and(|bundle| !bundle.model_outputs().is_empty());
    let model_rlib = if model_runtime {
        match jet_rt_rlib(cross_target, profile.is_release()) {
            Ok(path) => Some(path),
            Err(message) => {
                write_mode_diagnostic(mode, &format!("error: {message}\n"));
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        None
    };
    // Cache-integrity fix (Tower #85 §0): compile to a *private per-process*
    // path, never straight onto the shared `build/<stem>` display path. Two
    // concurrent `jet` processes compiling different source that happens to
    // share a file stem would otherwise race — process A could `store_cached`
    // its hash against process B's freshly-overwritten `build/<stem>`, mapping
    // A's key to B's binary in the shared content cache. `process::id()`
    // disambiguates the processes; we `store_cached` from this private path
    // (safe — only ever racing another process computing the *same* key, i.e.
    // the same content) and only then rename into the shared display path.
    let bin_name = bin
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("out")
        .to_string();
    // Private per-process working directory: rustc writes the output binary,
    // the generated `.rs`, AND all its intermediate codegen-unit object files
    // (`*.rcgu.o`) here. Two concurrent builds that share a file stem would
    // otherwise collide in the shared `build/` dir — on the binary, on the
    // source file mid-compile, and on the crate-name-derived intermediates —
    // corrupting each other's compile and (per §0) the shared content cache.
    // The results are published to the shared display paths only after a clean
    // compile; `store_cached` reads from this private path.
    let work = PathBuf::from("build").join(format!(".work.{}.{}", bin_name, std::process::id()));
    if let Err(e) = fs::create_dir_all(&work) {
        crate::cli_error!("E2105", "couldn't create the build work dir: {}", e);
        exit(ExitCodes::USER_ERROR);
    }
    let tmp_bin = work.join(&bin_name);
    let tmp_rs = work.join(format!("{}.rs", stem(file)));
    let entry_parent = Path::new(file)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let project_root = runtime_bundle
        .map(|bundle| bundle.project_root.clone())
        .or_else(|| {
            jet::Loader::find_package_root_checked(&entry_parent)
                .ok()
                .flatten()
        })
        .unwrap_or(entry_parent);
    let project_root_prefix = fs::canonicalize(&project_root).ok();
    let crate_name = jet::Syntax::sanitize_crate_name(&stem(file));
    let metadata_key = cache_key.clone().unwrap_or_else(|| {
        let mut bytes = b"jet.rustc-metadata.v1\0".to_vec();
        append_cache_field(&mut bytes, &crate_name);
        append_cache_field(&mut bytes, rust_code);
        for flag in &rustc_flags {
            append_cache_field(&mut bytes, flag);
        }
        if let Some(link) = ffi {
            append_cache_field(&mut bytes, &link.cache_identity);
        }
        if let Some(path) = &model_rlib {
            append_cache_field(&mut bytes, &path.display().to_string());
            if let Ok(metadata) = fs::metadata(path) {
                append_cache_field(&mut bytes, &metadata.len().to_string());
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                        append_cache_field(&mut bytes, &duration.as_nanos().to_string());
                    }
                }
            }
        }
        append_link_identity(&mut bytes, clinks, project_root_prefix.as_deref());
        jet::SHA256::sha256_hex(&bytes)
    });
    // Pin the crate name to the file stem — the name rustc used to infer from
    // `build/<stem>.rs` — so the private working-dir source name doesn't leak
    // into codegen. Everything here is decided once and replayed per attempt:
    // one rustc invocation over one prepared program.
    let run_rustc = |prepared: &jet_store::runtime::PreparedRuntime| -> std::process::Output {
        #[cfg(debug_assertions)]
        let rust = if std::env::var_os("JET_ICE_RUSTC_REJECTION_SELF_TEST").is_some() {
            format!(
                "{}\ncompile_error!(\"JET_RUSTC_REJECTION_SENTINEL\");\ncompile_error!(\"JET_RUSTC_REJECTION_SECOND_SENTINEL\");\n",
                prepared.rust()
            )
        } else {
            prepared.rust().to_string()
        };
        #[cfg(not(debug_assertions))]
        let rust = prepared.rust().to_string();
        if let Err(e) = fs::write(&tmp_rs, rust) {
            crate::cli_error!("E2105", "couldn't write {}: {}", tmp_rs.display(), e);
            exit(ExitCodes::USER_ERROR);
        }
        let mut cmd = Command::new("rustc");
        cmd.arg("--edition").arg("2021").args(&rustc_flags);
        cmd.arg("--crate-name").arg(&crate_name);
        cmd.arg("-C").arg(format!("metadata={metadata_key}"));
        // Keep project and per-process work paths out of generated DWARF and
        // ThinLTO records. Both prefixes have stable targets across checkouts.
        if let Some(project_prefix) = &project_root_prefix {
            cmd.arg("--remap-path-prefix")
                .arg(format!("{}=/jet/project", project_prefix.display()));
        }
        cmd.arg("--remap-path-prefix")
            .arg(format!("{}=/jet/build", work.display()));
        if let Ok(work_prefix) = fs::canonicalize(&work) {
            cmd.arg("--remap-path-prefix")
                .arg(format!("{}=/jet/build", work_prefix.display()));
        }
        cmd.arg(&tmp_rs).arg("-o").arg(&tmp_bin);
        prepared.add_rustc_args(&mut cmd);
        if let Some(rlib) = &model_rlib {
            let dependencies = rlib.parent().unwrap_or_else(|| Path::new("."));
            cmd.arg("--extern")
                .arg(format!("jet_rt={}", rlib.display()))
                .arg("-L")
                .arg(format!("dependency={}", dependencies.display()));
        }
        if let Some(link) = ffi {
            cmd.arg("--extern")
                .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
            for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
                cmd.arg("-L")
                    .arg(format!("dependency={}", deps_dir.display()));
            }
        }
        // S59 (E2-M14): native C library link flags (`-L native=…`, `-l <name>`).
        for arg in clinks {
            cmd.arg(arg);
        }
        match cmd.output() {
            Ok(output) => output,
            Err(_) => {
                crate::cli_error!(@full "E2105", "couldn't find `rustc` on this machine", "v1 of this language uses Rust as its backend (docs/spec/architecture.md)", "install Rust from https://rustup.rs, then try again");
                exit(ExitCodes::USER_ERROR);
            }
        }
    };

    // rustc owns the backend and linker in one production invocation. Keep the
    // receipt explicit: Jet-side backend preparation is separate from the
    // rustc invocation that performs backend code generation and linking.
    let rustc_started = Instant::now();
    let mut out = run_rustc(&prepared_runtime);
    // I5 fail-open: linking the cached runtime rlib is an optimization, so it may
    // cost a compile but must never cost a working build. If the thin crate is
    // rejected — a runtime item the exporter left private, an impl the split
    // moved away from its type — rebuild the exact inline monolith and report on
    // that instead. `jet build` of correct Jet cannot regress into an I2 banner.
    if !out.status.success() && prepared_runtime.is_split() {
        if verbose {
            step("runtime cache bypassed (split crate rejected — inline retry)".to_string());
        }
        out = run_rustc(&jet_store::runtime::PreparedRuntime::inline(rust_code));
    }

    if !out.status.success() {
        // Preserve the shared `build/<stem>.rs` artifact (written above) for the
        // ICE bug report, but drop this process's private working dir.
        let _ = fs::remove_dir_all(&work);
        let stderr = String::from_utf8_lossy(&out.stderr);
        // I2: a *missing C library* is a user/system problem, not generated-code
        // rejection. Detect a linker "cannot find -l<name>" and print a clean
        // E3209 diagnostic naming the lib + fix — NOT the bug-in-jet banner. The
        // ICE banner stays only for genuine codegen-rejected-by-rustc.
        if let Some(missing) = missing_c_lib(&stderr) {
            let diag = jet::CFFI::e3209(&missing);
            report_problems(mode, file, "", &[diag]);
            exit(ExitCodes::USER_ERROR);
        }
        if let Some(linker) = missing_linker(&stderr) {
            write_mode_diagnostic(
                mode,
                &format!(
                    "Error [L2101]: rustc could not find linker `{linker}`.\n Why: Jet uses rustc as its backend, and rustc needs a C linker to produce a native binary.\n Fix: Run from `nix develop`, or install a C toolchain (`gcc`/`clang`; on Debian/Ubuntu: `build-essential`, on Arch: `base-devel`).\nMore: jet-lang.dev/e/L2101\n"
                ),
            );
            exit(ExitCodes::USER_ERROR);
        }
        let log_name = rustc_log_name(file);
        let rustc_log_path = output_authority.path().join(&log_name);
        let log_detail = match output_authority.replace_file(&log_name, &out.stderr) {
            Ok(()) => format!("  rustc log: {}", rustc_log_path.display()),
            Err(error) => format!(
                "  rustc log: {} (could not write: {})",
                rustc_log_path.display(),
                error
            ),
        };
        let rustc_error = first_rustc_error_block(&stderr)
            .map(|block| format!("  rustc error:\n{block}"))
            .unwrap_or_else(|| {
                let stderr = stderr.trim();
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!("  rustc stderr:\n{stderr}")
                }
            });
        if verbose {
            if let Some(block) = first_rustc_error_block(&stderr) {
                write_mode_status(mode, &format!("[build] rustc error:\n{block}\n"));
            }
        }
        let detail = if rustc_error.is_empty() {
            format!("  generated: {}\n{log_detail}", rs_path.display())
        } else {
            format!(
                "  generated: {}\n{log_detail}\n{rustc_error}",
                rs_path.display()
            )
        };
        write_mode_diagnostic(
            mode,
            &jet::Diagnostics::render_ice_report(
                "the generated Rust did not compile.",
                &detail,
                true,
            ),
        );
        exit(ExitCodes::ICE);
    }

    step(format!("link       -> {}", bin.display()));

    // Store into the content cache from the private path first, so the cache
    // entry is written from exactly the binary this process just built.
    if let Some(key) = cache_key {
        if let Some(store) = native_store.as_ref() {
            if let Err(error) = store.publish_file(&key, &tmp_bin) {
                let _ = fs::remove_dir_all(&work);
                crate::cli_error!("E2105", "couldn't store build cache artifact: {error}");
                exit(ExitCodes::USER_ERROR);
            }
            step("cache store -> saved binary for next time".to_string());
        }
    }
    // Then publish the private binary onto the shared, human-readable display
    // path (`build/<stem>`) that `jet run`/`jet build` hand back. A same-dir
    // rename is atomic; last writer wins the convenience slot, which was never
    // a content identity. Fall back to copy if rename crosses a filesystem.
    if fs::rename(&tmp_bin, &bin).is_err() {
        if let Err(e) = fs::copy(&tmp_bin, &bin) {
            let _ = fs::remove_file(&tmp_bin);
            crate::cli_error!("E2105", "couldn't finish writing {}: {}", bin.display(), e);
            exit(ExitCodes::USER_ERROR);
        }
        let _ = fs::remove_file(&tmp_bin);
    }
    persist_build_record(
        native_store.as_ref(),
        &record_program,
        &compiler_nodes,
        previous_record.as_ref(),
        false,
        rustc_started.elapsed().as_secs_f64() * 1000.0,
        0.0,
    );
    // Drop the private working dir (generated `.rs` + rustc intermediates).
    let _ = fs::remove_dir_all(&work);
}

pub(crate) fn build(
    file: &str,
    rust_code: &str,
    runtime_bundle: Option<&jet::AST::ProgramBundle>,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::FFI::FfiLink>,
    clinks: &[String],
    verbose: bool,
    cross_target: Option<&str>,
    web: Option<&jet::Codegen::MIRWeb::WebArtifacts>,
    plugin: Option<&jet::Codegen::PluginArtifacts>,
    mode: OutputMode,
    restored_cache: bool,
    cache_key: Option<String>,
) {
    build_inner(
        file,
        rust_code,
        runtime_bundle,
        bin,
        profile,
        ffi,
        clinks,
        verbose,
        cross_target,
        web,
        plugin,
        mode,
        restored_cache,
        cache_key,
        None,
    );
}

pub(crate) fn build_target_machine(
    file: &str,
    rust_code: &str,
    runtime_bundle: Option<&jet::AST::ProgramBundle>,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::FFI::FfiLink>,
    clinks: &[String],
    verbose: bool,
    cross_target: Option<&str>,
    web: Option<&jet::Codegen::MIRWeb::WebArtifacts>,
    plugin: Option<&jet::Codegen::PluginArtifacts>,
    mode: OutputMode,
    restored_cache: bool,
    cache_key: Option<String>,
    target_machine: Option<&jet::TargetMachine::TargetMachine>,
) {
    build_inner(
        file,
        rust_code,
        runtime_bundle,
        bin,
        profile,
        ffi,
        clinks,
        verbose,
        cross_target,
        web,
        plugin,
        mode,
        restored_cache,
        cache_key,
        target_machine,
    );
}

fn build_record_program(file: &str, runtime_bundle: Option<&jet::AST::ProgramBundle>) -> String {
    let input = Path::new(file);
    let root = runtime_bundle
        .map(|bundle| bundle.project_root.clone())
        .or_else(|| {
            jet::Loader::find_manifest_root(input.parent().unwrap_or_else(|| Path::new(".")))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let path = if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    };
    path.strip_prefix(&root)
        .unwrap_or(input)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn persist_build_record(
    store: Option<&Store>,
    program: &str,
    nodes: &[jet::Comptime::Build::BuildPlanNode],
    previous: Option<&jet_store::BuildRecord>,
    cache_hit: bool,
    compile_duration_ms: f64,
    link_duration_ms: f64,
) {
    if nodes.is_empty() {
        return;
    }
    let nodes = nodes
        .iter()
        .map(|node| {
            let why_ran = if cache_hit {
                "cached".to_string()
            } else if previous.is_none() {
                "first-run".to_string()
            } else if let Some(input) = node.inputs.first() {
                format!("input:{input}")
            } else {
                "first-run".to_string()
            };
            let duration_ms = match node.kind {
                jet::Comptime::Build::BuildNodeKind::Check => 0.0,
                jet::Comptime::Build::BuildNodeKind::Compile => compile_duration_ms,
                jet::Comptime::Build::BuildNodeKind::Link => link_duration_ms,
            };
            jet_store::BuildNodeRecord {
                kind: node.kind.as_str().to_string(),
                key: node.key.clone(),
                subject: node.subject.clone(),
                duration_ms,
                why_ran,
                inputs: node.inputs.clone(),
            }
        })
        .collect::<Vec<_>>();
    let Some(store) = store else {
        return;
    };
    let record = jet_store::BuildRecord::new(program.to_string(), nodes);
    let record_key = jet::SHA256::sha256_hex(record.to_json().as_bytes());
    let _ = store.publish_build_record(&record_key, &record);
}

/// D-DBG3 step 2 (dap-debugger): build + launch the native lldb-backed `jet
/// debug` backend — a debug-profile build whose checked MIR source carries
/// the line-map metadata consumed by `crates/jet-debug/src/Inferior.rs`, then
/// either the `(jet)` terminal session or the DAP server (`--dap`) drives it.
/// Returns the process exit code.
pub(crate) fn run_debug_native(file: &str, raw_frames: bool, dap: bool, mode: OutputMode) -> i32 {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!("E2105", "can't find the file `{}`", file);
            return ExitCodes::USER_ERROR;
        }
    };
    let out = match jet::compile_for_debug(file) {
        Ok(o) => o,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            return ExitCodes::USER_ERROR;
        }
    };
    let clinks = match jet::resolve_c_links(file) {
        Ok(a) => a,
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            return ExitCodes::USER_ERROR;
        }
    };
    let bin = PathBuf::from("build").join(format!("{}_dbg", stem(file)));
    build(
        file,
        &out.rust,
        None,
        bin.clone(),
        BuildProfile::Debug,
        out.ffi.as_ref(),
        &clinks,
        false,
        None,
        None,
        None,
        mode,
        false,
        // `jet debug` builds carry a line-map and launch interactively; not cached.
        None,
    );
    // `build()` always writes the generated Rust to `build/<stem>.rs` (the
    // debug binary path is the only caller-chosen path) — lldb's `-f` flag
    // matches by basename, so this is what `Inferior::set_breakpoint` needs.
    let rust_file = format!("{}.rs", stem(file));
    if dap {
        jet::Debug::run_dap(&bin, &rust_file, &out.rust, file, &src)
    } else {
        jet::Debug::run_native(&bin, &rust_file, &out.rust, file, &src, raw_frames)
    }
}

/// Scan a failed rustc/linker stderr for a missing C library and return its
/// link name. Matches the GNU ld / lld phrasing `cannot find -l<name>` (and the
/// `-l<name>` form some linkers print). Used to keep a missing *system library*
/// off the I2 ICE path — it's a user/system problem, not generated-code being
/// rejected.
fn missing_c_lib(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        // e.g. "cannot find -lraylib" / "cannot find -lraylib: No such file"
        if let Some(rest) = line.split("cannot find -l").nth(1) {
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ':' && *c != '\'' && *c != '"')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Scan a failed rustc stderr for a missing system linker. This is an
/// environment/toolchain problem (L2101), not a generated-Rust ICE.
fn missing_linker(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        if let Some(rest) = line.split("linker `").nth(1) {
            if let Some((name, tail)) = rest.split_once('`') {
                if !name.is_empty() && tail.contains("not found") {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}
fn rustc_log_name(file: &str) -> String {
    format!("{}.rustc.log", stem(file))
}

fn is_rustc_error_header(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("error:") || line.starts_with("error[")
}
fn is_rustc_diagnostic_header(line: &str) -> bool {
    let line = line.trim_start();
    is_rustc_error_header(line) || line.starts_with("warning:")
}

fn first_rustc_error_block(stderr: &str) -> Option<&str> {
    let mut block_start = None;
    let mut block_end = stderr.len();
    let mut offset = 0;
    for line in stderr.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if block_start.is_some() && is_rustc_diagnostic_header(content) {
            block_end = offset;
            break;
        }
        if block_start.is_none() && is_rustc_error_header(content) {
            block_start = Some(offset);
        }
        offset += line.len();
    }
    let start = block_start?;
    let block = stderr.get(start..block_end)?.trim_end();
    (!block.is_empty()).then_some(block)
}

#[cfg(test)]
mod rustc_ice_tests {
    use super::first_rustc_error_block;

    #[test]
    fn first_rustc_error_block_keeps_the_first_error() {
        let stderr = concat!(
            "warning: unused import\n",
            "error[E0308]: mismatched types\n",
            "  --> build/out.rs:1:1\n",
            "  |\n",
            "warning: another unused import\n",
            "  --> build/out.rs:2:1\n",
            "error[E0425]: cannot find type\n",
        );
        let block = first_rustc_error_block(stderr).expect("first rustc error");
        assert_eq!(
            block,
            "error[E0308]: mismatched types\n  --> build/out.rs:1:1\n  |"
        );
    }

    #[test]
    fn first_rustc_error_block_accepts_unnumbered_error() {
        let stderr = "error: aborting due to previous error\n";
        assert_eq!(
            first_rustc_error_block(stderr),
            Some("error: aborting due to previous error")
        );
    }
}

#[cfg(test)]
mod profile_tests {
    use super::BuildProfile;

    #[test]
    fn command_defaults_route_dev_commands_to_fast_profile() {
        assert!(matches!(
            BuildProfile::default_for_command("run"),
            BuildProfile::Fast
        ));
        assert!(matches!(
            BuildProfile::default_for_command("dev"),
            BuildProfile::Fast
        ));
        assert!(matches!(
            BuildProfile::default_for_command("build"),
            BuildProfile::Default
        ));
    }

    #[test]
    fn fast_profile_is_unoptimized_parallel_and_without_lto() {
        let args = BuildProfile::Fast.config().rustc_args(false);
        assert_eq!(
            args,
            vec![
                "-C",
                "codegen-units=256",
                "-C",
                "opt-level=0",
                "-C",
                "lto=off",
            ]
        );
    }

    #[test]
    fn optimized_default_and_debug_profiles_keep_distinct_flags() {
        let optimized = BuildProfile::Default.config().rustc_args(false);
        assert!(optimized.contains(&"opt-level=2".to_string()));
        assert!(!optimized.contains(&"-O".to_string()));
        assert!(optimized.contains(&"lto=thin".to_string()));
        assert!(optimized.contains(&"strip=symbols".to_string()));
        assert!(!optimized
            .iter()
            .any(|arg| arg.starts_with("codegen-units=")));

        let debug = BuildProfile::Debug.config().rustc_args(false);
        assert!(debug.contains(&"codegen-units=256".to_string()));
        assert!(debug.contains(&"debuginfo=2".to_string()));
        assert!(!debug.contains(&"-O".to_string()));
        assert!(!debug.contains(&"lto=thin".to_string()));
    }

    #[test]
    fn fast_and_optimized_defaults_have_distinct_cache_identity() {
        assert_ne!(
            BuildProfile::Fast.cache_tag(),
            BuildProfile::Default.cache_tag()
        );
        assert!(BuildProfile::Fast
            .config()
            .settings_tag()
            .contains("codegen-units=256"));
    }
}

#[cfg(test)]
mod missing_c_lib_tests {
    use super::{
        child_exit_code, missing_c_lib, missing_linker, native_cache_key,
        native_cache_key_for_prepared_build, native_cache_key_for_program,
        native_cache_key_with_toolchain, native_cache_salt, native_cache_salt_with_schema,
        render_internal_fault,
    };

    struct ScratchProject(std::path::PathBuf);

    impl ScratchProject {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "jet_genmod_cache_{}_{}",
                std::process::id(),
                nonce
            ));
            std::fs::create_dir_all(&root).expect("create generic-module cache fixture");
            Self(root)
        }

        fn write(&self, name: &str, source: &str) {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create cache fixture input directory");
            }
            std::fs::write(path, source).expect("write generic-module cache fixture");
        }

        fn main(&self) -> String {
            self.0.join("main.jet").to_string_lossy().into_owned()
        }
        fn native_cache_key_with_input(&self, relative: &str) -> String {
            // Production cache keys require a canonical package manifest. Keep
            // this fixture package-shaped so consumed-input edits exercise the
            // actual manifest-salted key path rather than the no-key fallback.
            self.write(
                "package.jet",
                "name: \"consumed-input-cache\"\nversion: \"1.0.0\"\n",
            );
            let mut bundle = jet::Loader::load_entry_with_overlay(&self.main(), None, false)
                .expect("load consumed-input cache fixture");
            jet::Driver::seed_build_facts(
                &mut bundle,
                "dev",
                false,
                &std::collections::BTreeMap::new(),
            )
            .expect("seed consumed-input cache fixture");
            let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
                "consumed-input cache fixture must check"
            );
            let bytes =
                std::fs::read(self.0.join(relative)).expect("read consumed-input cache fixture");
            bundle.comptime_inputs = vec![jet::AST::ComptimeInput {
                path: relative.replace(std::path::MAIN_SEPARATOR, "/"),
                hash: jet::SHA256::sha256_hex(&bytes),
            }];
            native_cache_key_for_program(
                &self.main(),
                &bundle,
                "default",
                "run",
                "comptime-input-test-toolchain",
                None,
            )
            .expect("consumed-input cache key")
        }
    }

    impl Drop for ScratchProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn crate_name_sanitizes_user_facing_file_stems() {
        assert_eq!(
            jet::Syntax::sanitize_crate_name("renderable-varargs"),
            "renderable_varargs"
        );
        assert_eq!(jet::Syntax::sanitize_crate_name("3d.demo"), "_3d_demo");
    }

    #[cfg(unix)]
    #[test]
    fn signaled_program_is_not_reported_as_success() {
        let status = std::process::Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .expect("launch signal fixture");
        assert_eq!(child_exit_code(status), jet::ExitCodes::USER_ERROR);
    }
    #[test]
    fn native_cache_ignores_comments_and_tracks_bridge_identity() {
        let project = ScratchProject::new();
        project.write("main.jet", "fn run() { print(1) }\n");
        let base = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("base cache key");
        project.write("main.jet", "// cache-only comment\nfn run() { print(1) }\n");
        let comment = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("comment cache key");
        assert_eq!(base, comment, "comments must not invalidate native cache");

        let instances = Vec::new();
        let no_bridge = native_cache_salt(
            "tool",
            "dependencies",
            "runtime",
            "core",
            "run",
            "linux-x86_64",
            &instances,
            None,
            &[],
            None,
        );
        let bridge_a = native_cache_salt(
            "tool",
            "dependencies",
            "runtime",
            "core",
            "run",
            "linux-x86_64",
            &instances,
            Some("bridge-a"),
            &[],
            None,
        );
        let bridge_b = native_cache_salt(
            "tool",
            "dependencies",
            "runtime",
            "core",
            "run",
            "linux-x86_64",
            &instances,
            Some("bridge-b"),
            &[],
            None,
        );
        assert_ne!(
            no_bridge, bridge_a,
            "a bridge must have its own cache identity"
        );
        assert_ne!(
            bridge_a, bridge_b,
            "bridge input changes must invalidate cache"
        );
    }

    #[test]
    fn generic_instance_cache_salt_tracks_every_downstream_input() {
        let instances = vec!["instance-a".to_string(), "instance-b".to_string()];
        let salt = |tool, deps, runtime, core, mode, target, instances: &[String]| {
            native_cache_salt(
                tool,
                deps,
                runtime,
                core,
                mode,
                target,
                instances,
                None,
                &[],
                None,
            )
        };
        let base = salt(
            "tool-a",
            "deps-a",
            "runtime-a",
            "core-a",
            "run",
            "linux-x86_64",
            &instances,
        );
        assert_ne!(
            base,
            salt(
                "tool-b",
                "deps-a",
                "runtime-a",
                "core-a",
                "run",
                "linux-x86_64",
                &instances
            )
        );
        assert_ne!(
            base,
            salt(
                "tool-a",
                "deps-b",
                "runtime-a",
                "core-a",
                "run",
                "linux-x86_64",
                &instances
            )
        );
        assert_ne!(
            base,
            salt(
                "tool-a",
                "deps-a",
                "runtime-b",
                "core-a",
                "run",
                "linux-x86_64",
                &instances
            )
        );
        assert_ne!(
            base,
            salt(
                "tool-a",
                "deps-a",
                "runtime-a",
                "core-b",
                "run",
                "linux-x86_64",
                &instances
            )
        );
        assert_ne!(
            base,
            salt(
                "tool-a",
                "deps-a",
                "runtime-a",
                "core-a",
                "test",
                "linux-x86_64",
                &instances
            )
        );
        assert_ne!(
            base,
            salt(
                "tool-a",
                "deps-a",
                "runtime-a",
                "core-a",
                "run",
                "macos-aarch64",
                &instances
            )
        );
        assert_ne!(
            base,
            salt(
                "tool-a",
                "deps-a",
                "runtime-a",
                "core-a",
                "run",
                "linux-x86_64",
                &["instance-c".into()]
            )
        );
        assert_eq!(
            base,
            salt(
                "tool-a",
                "deps-a",
                "runtime-a",
                "core-a",
                "run",
                "linux-x86_64",
                &["instance-b".into(), "instance-a".into()]
            )
        );
    }

    #[test]
    fn hostile_cache_invalidation_matrix() {
        let project = ScratchProject::new();
        project.write("main.jet", "fn run() { print(1) }\n");
        let identity = |compiler, schema, backend, flags, linker| {
            format!(
                "compiler={compiler};schema={schema};backend={backend};flags={flags};linker={linker}"
            )
        };
        let base_identity = identity("compiler-a", "schema-a", "backend-a", "flags-a", "linker-a");
        let key = |toolchain: &str, profile: &str| {
            native_cache_key_with_toolchain(
                &project.main(),
                "dev",
                profile,
                "run",
                toolchain,
                None,
            )
            .expect("hostile cache fixture key");
        };
        let base = key(&base_identity, "default");
        let identities = [
            (
                "compiler",
                identity("compiler-b", "schema-a", "backend-a", "flags-a", "linker-a"),
            ),
            (
                "schema",
                identity("compiler-a", "schema-b", "backend-a", "flags-a", "linker-a"),
            ),
            (
                "backend",
                identity("compiler-a", "schema-a", "backend-b", "flags-a", "linker-a"),
            ),
            (
                "flags",
                identity("compiler-a", "schema-a", "backend-a", "flags-b", "linker-a"),
            ),
            (
                "linker",
                identity("compiler-a", "schema-a", "backend-a", "flags-a", "linker-b"),
            ),
        ];
        for (input, changed) in identities {
            assert_ne!(
                base,
                key(&changed, "default"),
                "{input} must miss final cache"
            );
        }
        assert_ne!(
            base,
            key(&base_identity, "profile:debug;opt:none"),
            "profile must miss final cache"
        );

        let instances = vec!["instance-a".to_string()];
        let salt = |dependencies, runtime, core, target| {
            native_cache_salt(
                &base_identity,
                dependencies,
                runtime,
                core,
                "run",
                target,
                &instances,
                None,
                &[],
                None,
            )
        };
        let base_salt = salt(
            "dependency-a",
            "generated-runtime-a",
            "core-a",
            "linux-x86_64",
        );
        assert_ne!(
            base_salt,
            native_cache_salt_with_schema(
                b"jet-native-cache-salt-v4",
                &base_identity,
                "dependency-a",
                "generated-runtime-a",
                "core-a",
                "run",
                "linux-x86_64",
                &instances,
                None,
                &[],
                None,
            ),
            "cache-schema change must miss final work"
        );
        for (input, changed) in [
            (
                "dependency",
                salt(
                    "dependency-b",
                    "generated-runtime-a",
                    "core-a",
                    "linux-x86_64",
                ),
            ),
            (
                "generated code",
                salt(
                    "dependency-a",
                    "generated-runtime-b",
                    "core-a",
                    "linux-x86_64",
                ),
            ),
            (
                "Core artifact",
                salt(
                    "dependency-a",
                    "generated-runtime-a",
                    "core-b",
                    "linux-x86_64",
                ),
            ),
            (
                "target",
                salt("dependency-a", "generated-runtime-a", "core-a", "wasm32"),
            ),
        ] {
            assert_ne!(base_salt, changed, "{input} must miss affected final work");
        }
    }

    #[test]
    fn native_cache_key_requires_an_exact_emitted_program() {
        let project = ScratchProject::new();
        project.write(
            "package.jet",
            "name: \"prepared-cache\"\nversion: \"1.0.0\"\n",
        );
        project.write("main.jet", "fn run() {}\n");
        let mut options = jet::Driver::BuildRunOptions::default();
        options.package_scope = true;
        options.build_override = true;

        let exact_inputs = jet::Driver::FrontEndInputs::for_build(&project.main(), &options);
        let exact = jet::Driver::prepare_build_front_end(exact_inputs)
            .expect("prepare exact runtime bundle");
        assert!(exact.emitted_program().is_some());
        assert!(
            native_cache_key_for_prepared_build(&project.main(), Some(&exact), "dev", "run", None,)
                .is_some(),
            "an exact checked runtime bundle keeps native caching enabled",
        );

        project.write(
            "tools/build.jet",
            "fn build(b: BuildContext) BuildPlan -> { return b.plan() }\n",
        );
        let package_inputs = jet::Driver::FrontEndInputs::for_build(&project.main(), &options);
        let package = jet::Driver::prepare_build_front_end(package_inputs)
            .expect("prepare external package build entry");
        assert!(
            package.emitted_program().is_none(),
            "a package build entry does not itself equal the emitted runtime bundle",
        );
        assert_eq!(
            native_cache_key_for_prepared_build(
                &project.main(),
                Some(&package),
                "dev",
                "run",
                None,
            ),
            None,
            "without an exact emitted bundle, native cache lookup and store stay disabled",
        );
    }

    #[test]
    fn generic_instance_native_cache_key_invalidates_on_program_dependency_arg_package_and_profile_edits(
    ) {
        let project = ScratchProject::new();
        let main = "use defs.box\nmodule defs\n\nmodule selected :: box<Int>(3)\nfn run() { print(selected.value()) }\n";
        let dependency = "pub module box<T>(n: Int) { pub fn value() Int -> { return n } }\n";
        let manifest_v1 = "name: \"cache-proof\"\nversion: \"1.0.0\"\n";
        project.write("main.jet", main);
        project.write("defs.jet", dependency);
        project.write("package.jet", manifest_v1);

        let base = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("base cache key");

        project.write(
            "package.jet",
            "// semantic no-op\nname: \"cache-proof\"\nversion: \"1.0.0\"\n",
        );
        let manifest_comment = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("comment-only manifest cache key");
        assert_eq!(
            base, manifest_comment,
            "comment-only package manifest edit must reuse the native cache",
        );
        project.write("package.jet", manifest_v1);

        let toolchain_a = native_cache_key_with_toolchain(
            &project.main(),
            "dev",
            "default",
            "run",
            "compiler-build-a/rustc-a/linker-a/backend-a",
            None,
        )
        .expect("toolchain A cache key");
        let toolchain_b = native_cache_key_with_toolchain(
            &project.main(),
            "dev",
            "default",
            "run",
            "compiler-build-b/rustc-a/linker-a/backend-a",
            None,
        )
        .expect("toolchain B cache key");
        assert_ne!(
            toolchain_a, toolchain_b,
            "production native-cache key seam must include compiler/toolchain identity"
        );

        project.write(
            "main.jet",
            "use defs.box\nmodule defs\n\nmodule selected :: box<Int>(3)\nfn run() { print(selected.value() + 1) }\n",
        );
        let program_body = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("program body cache key");
        assert_ne!(
            base, program_body,
            "entry-body edit must invalidate native cache"
        );

        project.write("main.jet", main);
        project.write(
            "defs.jet",
            "pub module box<T>(n: Int) { pub fn value() Int -> { return n + 1 } }\n",
        );
        let dependency_body = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("dependency cache key");
        assert_ne!(
            base, dependency_body,
            "imported template-body edit must invalidate native cache"
        );

        project.write("defs.jet", dependency);
        project.write(
            "main.jet",
            "use defs.box\nmodule defs\n\nmodule selected :: box<Int>(4)\nfn run() { print(selected.value()) }\n",
        );
        let argument = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("argument cache key");
        assert_ne!(
            base, argument,
            "normalized instance-argument edit must invalidate native cache"
        );

        project.write("main.jet", main);
        project.write("package.jet", "name: \"cache-proof\"\nversion: \"2.0.0\"\n");
        let package = native_cache_key(&project.main(), "dev", "default", "run", None)
            .expect("package cache key");
        assert_ne!(
            base, package,
            "package manifest edit must invalidate native cache"
        );

        project.write("package.jet", manifest_v1);
        let profile = native_cache_key(&project.main(), "small", "small", "run", None)
            .expect("profile cache key");
        assert_ne!(
            base, profile,
            "build-profile edit must invalidate native cache"
        );
    }

    #[test]
    fn native_cache_key_invalidates_when_consumed_lock_changes() {
        let project = ScratchProject::new();
        project.write("main.jet", "fn run() { print(1) }\n");
        project.write(".jet/lock", "version = 1\n");
        let base = project.native_cache_key_with_input(".jet/lock");

        project.write(".jet/lock", "version = 2\n");
        let changed = project.native_cache_key_with_input(".jet/lock");

        assert_ne!(
            base, changed,
            "a consumed package lock edit must invalidate native cache"
        );
    }

    #[test]
    fn native_cache_key_invalidates_when_consumed_profile_changes() {
        let project = ScratchProject::new();
        project.write("main.jet", "fn run() { print(1) }\n");
        project.write("env.jet", "module dev {}\n");
        let base = project.native_cache_key_with_input("env.jet");

        project.write("env.jet", "// changed profile\nmodule dev {}\n");
        let changed = project.native_cache_key_with_input("env.jet");

        assert_ne!(
            base, changed,
            "a consumed profile edit must invalidate native cache"
        );
    }

    #[test]
    fn native_cache_key_invalidates_when_consumed_config_changes() {
        let project = ScratchProject::new();
        project.write("main.jet", "fn run() { print(1) }\n");
        project.write(
            "config/release.jet",
            "pub release :: Config.{ version: \"1\" }\n",
        );
        let base = project.native_cache_key_with_input("config/release.jet");

        project.write(
            "config/release.jet",
            "// changed config\npub release :: Config.{ version: \"1\" }\n",
        );
        let changed = project.native_cache_key_with_input("config/release.jet");

        assert_ne!(
            base, changed,
            "a consumed package config edit must invalidate native cache"
        );
    }

    #[test]
    fn detects_ld_cannot_find() {
        // GNU ld / lld phrasing — must be routed to E3209, not the I2 ICE banner.
        let stderr = "  = note: /usr/bin/ld: cannot find -lraylib: No such file or directory\n  collect2: error: ld returned 1 exit status\n";
        assert_eq!(missing_c_lib(stderr).as_deref(), Some("raylib"));
    }

    #[test]
    fn detects_bare_form() {
        assert_eq!(
            missing_c_lib("ld: cannot find -lsqlite3\n").as_deref(),
            Some("sqlite3")
        );
    }

    #[test]
    fn genuine_codegen_error_is_not_a_missing_lib() {
        // A real rustc type error must keep the ICE path (returns None here).
        let stderr = "error[E0308]: mismatched types\n --> build/main.rs:3:5\n";
        assert_eq!(missing_c_lib(stderr), None);
    }

    #[test]
    fn detects_missing_system_linker() {
        let stderr =
            "error: linker `cc` not found\n  |\n  = note: No such file or directory (os error 2)\n";
        assert_eq!(missing_linker(stderr).as_deref(), Some("cc"));
    }

    #[test]
    fn genuine_codegen_error_is_not_a_missing_linker() {
        let stderr = "error[E0425]: cannot find type `RaylibWindow` in module `jet_std`\n";
        assert_eq!(missing_linker(stderr), None);
    }

    #[test]
    fn driver_brands_runtime_host_fault_without_rust_text() {
        let diagnostic = jet::Diagnostics::Diagnostic::runtime_host_fault(
            "before\n".to_string(),
            "the JIT runtime helper failed".to_string(),
        );
        let (stdout, what) = diagnostic
            .runtime_host_fault_parts()
            .expect("typed runtime host fault");
        let report = render_internal_fault(what);
        assert_eq!(stdout, "before\n");
        assert!(report.starts_with("internal compiler error: the JIT runtime helper failed\n"));
        assert!(report.contains("This is a bug in jet, NOT in your program."));
        assert!(!report.contains("thread 'main' panicked"));
        assert!(!report.contains("runtime_host.rs"));
    }
}

#[cfg(test)]
mod web_output_boundary_tests {
    use super::{ensure_web_output_dir, validate_web_output_file};

    #[cfg(unix)]
    #[test]
    fn web_artifact_writer_rejects_symlink_output() {
        use std::os::unix::fs::symlink;

        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-web-output-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.join("outside.js");
        let output = root.join("app.js");
        std::fs::write(&outside, "must survive").unwrap();
        symlink(&outside, &output).unwrap();

        let error = validate_web_output_file(&output, &root)
            .expect_err("web artifact writes must not follow symlinks");
        assert!(error.contains("must not be a symlink"), "{error}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn web_artifact_writer_rejects_hardlink_output() {
        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".jet-web-output-hardlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.with_file_name(format!(
            ".jet-web-output-hardlink-target-{}",
            std::process::id()
        ));
        let output = root.join("app.js");
        std::fs::write(&outside, "must survive").unwrap();
        std::fs::hard_link(&outside, &output).unwrap();

        let error = validate_web_output_file(&output, &root)
            .expect_err("web artifact writes must not follow hardlinks");
        assert!(error.contains("must not be a hard link"), "{error}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "must survive");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn web_build_rejects_symlinked_output_root_before_creation() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("jet-web-build-root-symlink-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("jet-web-build-root-outside-{}", std::process::id()));
        let _ = std::fs::remove_file(&root);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, &root).unwrap();

        let error = ensure_web_output_dir(&root)
            .expect_err("web builds must reject a symlinked output root");
        assert!(error.contains("must not be a symlink"), "{error}");
        assert!(!outside.join("created-by-build").exists());

        let _ = std::fs::remove_file(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
