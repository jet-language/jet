//! Pipeline composition — the compiler's execution stages assembled in one place.
//!
//! `lib.rs` public functions are thin facades over these. `LSP/Check.rs` calls
//! `check_file` directly for document checking.

use crate::Diagnostics::{Diagnostic, Severity};
use jet_pkg_model::Authority::AuthorityResolver;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn package_lints_deny(
    bundle: &crate::AST::ProgramBundle,
) -> Result<Vec<String>, Diagnostic> {
    let Some(entry) = bundle.modules.get(bundle.entry) else {
        return Ok(Vec::new());
    };
    let resolver = match AuthorityResolver::open(&bundle.project_root) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return Ok(Vec::new()),
        Err(error) => return Err(error.diagnostic()),
    };
    let relative = entry
        .path
        .strip_prefix(resolver.root())
        .or_else(|_| entry.path.strip_prefix(&bundle.project_root))
        .map_err(|_| {
            jet_pkg_model::Authority::AuthorityError::Escapes(entry.path.clone()).diagnostic()
        })?;
    let entry_file = match resolver.checked_file(relative) {
        Ok(file) => file,
        Err(error) if error.is_missing() => return Ok(Vec::new()),
        Err(error) => return Err(error.diagnostic()),
    };
    resolver.revalidate_file(&entry_file).map_err(|error| error.diagnostic())?;
    let manifest = match resolver.checked_manifest(std::path::Path::new(".")) {
        Ok(manifest) => manifest,
        Err(error) if error.is_missing() => return Ok(Vec::new()),
        Err(error) => return Err(error.diagnostic()),
    };
    let source = manifest.file.text().map_err(|error| error.diagnostic())?;
    let deny = jet_foundation::LintPolicy::parse_package_source(&source).map_err(|detail| {
        let fix = jet_foundation::LintPolicy::policy_error_fix(&detail);
        Diagnostic::error(
            "E1206",
            "invalid package manifest".to_string(),
            detail,
            fix,
            None,
        )
    })?.unwrap_or_default();
    resolver
        .revalidate_file(&manifest.file)
        .map_err(|error| error.diagnostic())?;
    Ok(deny)
}

fn apply_package_lint_policy(
    bundle: &crate::AST::ProgramBundle,
    diagnostics: Vec<Diagnostic>,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let deny = package_lints_deny(bundle).map_err(|diagnostic| vec![diagnostic])?;
    Ok(jet_foundation::LintPolicy::apply(&deny, diagnostics))
}

fn classify_diagnostics(
    bundle: &crate::AST::ProgramBundle,
    diagnostics: Vec<Diagnostic>,
    suppress_build_e0102: bool,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for diagnostic in apply_package_lint_policy(bundle, diagnostics)? {
        match diagnostic.severity {
            // Generated declarations do not exist during the pre-build
            // reflection pass. Defer unknown-name errors to the fresh
            // selected-program sema pass after generation.
            Severity::Error if suppress_build_e0102 && diagnostic.code == "E0102" => {}
            Severity::Error => errors.push(diagnostic),
            Severity::Lint => lints.push(diagnostic),
        }
    }
    if errors.is_empty() {
        Ok(lints)
    } else {
        Err(errors)
    }
}

/// One diagnostic gate shared by `jet build`, `jet run`, `jet dev`, and check.
///
/// Surfaces what parser recovery (`parse_teaching`) and sema (+ optional
/// extension hooks) already produced. Does not re-check anything (I3).
/// Returns lints on success; errors on failure.
pub fn gate_diagnostics(
    bundle: &crate::AST::ProgramBundle,
    parse_teaching: Vec<Diagnostic>,
    sema: Vec<Diagnostic>,
    extension: Vec<Diagnostic>,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    classify_diagnostics(
        bundle,
        parse_teaching
            .into_iter()
            .chain(sema)
            .chain(extension)
            .collect(),
        false,
    )
}

/// Main pipeline: load from file path → sema → ffi → codegen.
///
/// D-OSTARGET1=A (ratified 2026-07-01, c134): `cross_target` is the raw
/// `--target=<triple>` string (or `None`) — reused as-is from the existing
/// E2-M15 cross-compile flag, resolved to a native OS bucket in
/// `compile_bundle_path_opts_dbg` (host OS when `None` or unrecognized).
pub fn compile_bundle_path_opts(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        gates,
        web_target,
        false,
        false,
        false,
        cross_target,
        None,
        "dev",
        &BTreeMap::new(),
    )
}

/// Profile-aware native front-end entry used by the CLI. Existing callers
/// keep the `dev` default; the selected profile is a build fact, not an engine
/// branch.
pub fn compile_bundle_path_opts_with_profile(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    cross_target: Option<&str>,
    profile: &str,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_with_profile_and_settings(
        file,
        mode,
        freestanding,
        gates,
        web_target,
        cross_target,
        profile,
        &BTreeMap::new(),
    )
}

pub fn compile_bundle_path_opts_with_profile_and_settings(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    cross_target: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        gates,
        web_target,
        false,
        false,
        false,
        cross_target,
        None,
        profile,
        setting_overrides,
    )
}

#[derive(Debug)]
pub enum TargetMachineCompileError {
    Diagnostics(Vec<Diagnostic>),
    Machine(Vec<crate::TargetMachine::TargetMachineError>),
}

/// D-TARGET-* production hook: validate a selected typed target machine from
/// sema facts before codegen. CLI/UI wording remains future work; this returns
/// machine errors as data.
pub fn compile_bundle_path_with_target_machine(
    file: &str,
    mode: crate::Sema::CompileMode,
    machine: &crate::TargetMachine::TargetMachine,
) -> Result<crate::CompileOutput, TargetMachineCompileError> {
    let usage = target_machine_usage_for_file(file, mode)
        .map_err(TargetMachineCompileError::Diagnostics)?;
    let machine_errors = machine.validate(&usage);
    if !machine_errors.is_empty() {
        return Err(TargetMachineCompileError::Machine(machine_errors));
    }
    compile_bundle_path_opts_full(
        file,
        mode,
        machine.no_os,
        crate::Policy::GateSet::default(),
        false,
        false,
        false,
        false,
        Some(machine.triple.as_str()),
        None,
        "dev",
        &BTreeMap::new(),
    )
    .map_err(TargetMachineCompileError::Diagnostics)
}

/// Artifacts from a typed no-OS machine build (linker, map, audit, size, ELF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFirmwareArtifacts {
    pub out_dir: std::path::PathBuf,
    pub linker_script: std::path::PathBuf,
    pub startup: std::path::PathBuf,
    pub map: std::path::PathBuf,
    pub elf: std::path::PathBuf,
    pub audit_json: std::path::PathBuf,
    pub size_budget: crate::TargetMachine::SizeBudgetReport,
    pub audit: String,
}

/// D-TARGET-*: validate machine, generate linker/startup, link firmware ELF,
/// write audit + size budget under `out_dir` (typically `.jet/target/<name>/`).
pub fn build_target_machine_firmware(
    machine: &crate::TargetMachine::TargetMachine,
    usage: &crate::TargetMachine::TargetMachineUse,
    out_dir: &std::path::Path,
) -> Result<TargetFirmwareArtifacts, TargetMachineCompileError> {
    use crate::TargetMachine::{ExecutionTier, TargetMachineError};
    use std::fs;
    use std::process::Command;

    if let Err(err) = machine.supports_execution_tier(ExecutionTier::Aot) {
        return Err(TargetMachineCompileError::Machine(vec![err]));
    }
    // Explicit honesty: Dev/JIT are rejected for no-OS before any artifact work.
    if let Err(err) = machine.supports_execution_tier(ExecutionTier::Dev) {
        // expected for no-os; record in audit via execution field
        let _ = err;
    }
    let mut errors = machine.validate(usage);
    if !machine.no_os {
        errors.push(TargetMachineError::HostedHasNoLinkerScript);
    }
    if !errors.is_empty() {
        return Err(TargetMachineCompileError::Machine(errors));
    }

    let linker = machine
        .generate_linker_script()
        .map_err(|e| TargetMachineCompileError::Machine(vec![e]))?;
    let startup = machine
        .generate_startup_source()
        .map_err(|e| TargetMachineCompileError::Machine(vec![e]))?;

    fs::create_dir_all(out_dir).map_err(|e| {
        TargetMachineCompileError::Machine(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!("create out dir: {e}"),
        }])
    })?;

    let linker_path = out_dir.join("memory.ld");
    let startup_path = out_dir.join(&startup.filename);
    let obj_path = out_dir.join("startup.o");
    let map_path = out_dir.join("firmware.map");
    let elf_path = out_dir.join("firmware.elf");
    let audit_path = out_dir.join(format!("{}.target.json", sanitize_name(&machine.name)));

    fs::write(&linker_path, &linker).map_err(|e| {
        TargetMachineCompileError::Machine(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!("write linker: {e}"),
        }])
    })?;
    fs::write(&startup_path, &startup.contents).map_err(|e| {
        TargetMachineCompileError::Machine(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!("write startup: {e}"),
        }])
    })?;

    let clang = resolve_target_clang().map_err(TargetMachineCompileError::Machine)?;
    let lld = resolve_tool("ld.lld").map_err(TargetMachineCompileError::Machine)?;

    let mut clang_cmd = Command::new(&clang);
    clang_cmd
        .arg(format!("--target={}", machine.triple))
        .arg("-nostdlib")
        .arg("-ffreestanding")
        .arg("-fno-builtin")
        .arg("-c")
        .arg(&startup_path)
        .arg("-o")
        .arg(&obj_path);
    let clang_out = clang_cmd.output().map_err(|e| {
        TargetMachineCompileError::Machine(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!("spawn clang: {e}"),
        }])
    })?;
    if !clang_out.status.success() {
        return Err(TargetMachineCompileError::Machine(vec![
            TargetMachineError::FirmwareBuildFailed {
                detail: format!(
                    "clang failed: {}",
                    String::from_utf8_lossy(&clang_out.stderr).trim()
                ),
            },
        ]));
    }

    let mut link_cmd = Command::new(&lld);
    link_cmd
        .arg(format!("-T{}", linker_path.display()))
        .arg(format!("-Map={}", map_path.display()))
        .arg("-o")
        .arg(&elf_path)
        .arg(&obj_path);
    if machine.triple.contains("aarch64") {
        link_cmd.arg("--image-base=0x40000000");
    }
    let link_out = link_cmd.output().map_err(|e| {
        TargetMachineCompileError::Machine(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!("spawn ld.lld: {e}"),
        }])
    })?;
    if !link_out.status.success() {
        return Err(TargetMachineCompileError::Machine(vec![
            TargetMachineError::FirmwareBuildFailed {
                detail: format!(
                    "ld.lld failed: {}",
                    String::from_utf8_lossy(&link_out.stderr).trim()
                ),
            },
        ]));
    }

    let artifact_bytes = fs::metadata(&elf_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let size_budget = machine.size_budget(usage, artifact_bytes);
    if !size_budget.ok() {
        return Err(TargetMachineCompileError::Machine(vec![
            TargetMachineError::SizeBudgetExceeded {
                report: size_budget,
            },
        ]));
    }
    let audit = machine.audit_json_with_budget(usage, Some(&size_budget));
    fs::write(&audit_path, &audit).map_err(|e| {
        TargetMachineCompileError::Machine(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!("write audit: {e}"),
        }])
    })?;

    Ok(TargetFirmwareArtifacts {
        out_dir: out_dir.to_path_buf(),
        linker_script: linker_path,
        startup: startup_path,
        map: map_path,
        elf: elf_path,
        audit_json: audit_path,
        size_budget,
        audit,
    })
}

/// Run QEMU virt smoke for an aarch64 no-OS ELF; returns serial output.
pub fn qemu_virt_aarch64_smoke(
    elf: &std::path::Path,
) -> Result<String, Vec<crate::TargetMachine::TargetMachineError>> {
    use crate::TargetMachine::TargetMachineError;
    use std::process::Command;

    let qemu = resolve_tool("qemu-system-aarch64")?;
    // `timeout` keeps the smoke bounded; virt UART prints "OK\n" from startup.
    let output = Command::new("timeout")
        .arg("3")
        .arg(&qemu)
        .arg("-machine")
        .arg("virt")
        .arg("-cpu")
        .arg("cortex-a57")
        .arg("-nographic")
        .arg("-kernel")
        .arg(elf)
        .output()
        .map_err(|e| {
            vec![TargetMachineError::FirmwareBuildFailed {
                detail: format!("spawn qemu: {e}"),
            }]
        })?;
    let serial = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if serial.contains("OK") {
        Ok(serial)
    } else {
        Err(vec![TargetMachineError::FirmwareBuildFailed {
            detail: format!(
                "qemu smoke missing OK marker (status={:?}); serial={serial:?}",
                output.status.code()
            ),
        }])
    }
}

/// D-CONF-WORD1=A: the machine axis. `--target` answers what machine this
/// build is for, whether the name is a rustc triple or a declared board.
pub fn target_machine_by_name(name: &str) -> Option<crate::TargetMachine::TargetMachine> {
    match name {
        "board.sensor_v1" | "firmware" | "sensor" => {
            Some(crate::TargetMachine::TargetMachine::board_sensor_v1())
        }
        "board.virt_aarch64" | "virt" => {
            Some(crate::TargetMachine::TargetMachine::board_virt_aarch64())
        }
        "hosted" => Some(crate::TargetMachine::TargetMachine::hosted(
            "x86_64-unknown-linux-gnu",
        )),
        _ => None,
    }
}

/// The machine names `--target` accepts beside a rustc triple.
pub const TARGET_MACHINE_NAMES: &[&str] = &["board.sensor_v1", "board.virt_aarch64", "hosted"];

/// D-TARGET-AUDIT1: machine audit JSON for a named board machine.
pub fn target_machine_dossier_json(machine_name: &str) -> Result<String, String> {
    let Some(machine) = target_machine_by_name(machine_name) else {
        return Err(format!(
            "unknown target machine `{machine_name}` (try {})",
            TARGET_MACHINE_NAMES.join(", ")
        ));
    };
    let usage = crate::TargetMachine::TargetMachineUse::default();
    Ok(machine.audit_json(&usage))
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn resolve_target_clang() -> Result<std::path::PathBuf, Vec<crate::TargetMachine::TargetMachineError>> {
    if let Ok(path) = std::env::var("JET_TARGET_CLANG") {
        let p = std::path::PathBuf::from(path);
        if p.is_file() {
            return Ok(p);
        }
    }
    // Prefer unwrapped clang: `clang -print-prog-name=clang` often resolves past the nix wrapper.
    if let Ok(out) = std::process::Command::new("clang")
        .arg("-print-prog-name=clang")
        .output()
    {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let p = std::path::PathBuf::from(&path);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    resolve_tool("clang")
}

fn resolve_tool(
    name: &str,
) -> Result<std::path::PathBuf, Vec<crate::TargetMachine::TargetMachineError>> {
    if let Ok(path) = which_tool(name) {
        return Ok(path);
    }
    Err(vec![crate::TargetMachine::TargetMachineError::FirmwareToolchainMissing {
        tool: name.to_string(),
    }])
}

fn which_tool(name: &str) -> Result<std::path::PathBuf, ()> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(());
    }
    Ok(std::path::PathBuf::from(path))
}

/// Like `compile_bundle_path_opts`, but for `jet build --target=sandbox`
/// (D-PLUGIN1=B / D-DEP-WASM1=A, c81): also emits the guest `.wit` + wasm32
/// Rust artifacts (`Codegen::emit_plugin`).
pub fn compile_bundle_path_opts_plugin(
    file: &str,
    mode: crate::Sema::CompileMode,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_plugin_with_gates(file, mode, crate::Policy::GateSet::default(), cross_target)
}

pub fn compile_bundle_path_opts_plugin_with_gates(
    file: &str,
    mode: crate::Sema::CompileMode,
    gates: crate::Policy::GateSet,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_plugin_with_gates_and_settings(
        file,
        mode,
        gates,
        cross_target,
        &BTreeMap::new(),
    )
}

pub fn compile_bundle_path_opts_plugin_with_gates_and_settings(
    file: &str,
    mode: crate::Sema::CompileMode,
    gates: crate::Policy::GateSet,
    cross_target: Option<&str>,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(file, mode, false, gates, false, true, false, false, cross_target, None, "dev", setting_overrides)
}

/// Like `compile_bundle_path_opts_plugin`, but for a checked `Library` output
/// (D-LIB-EXPORT1=C). Library packages do not need an executable `fn run`; the
/// selected public surface is validated and projected into native artifacts by
/// the caller after this front-end stage.
pub fn compile_bundle_path_opts_library(
    file: &str,
    mode: crate::Sema::CompileMode,
    explicit_output: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_library_with_gates(file, mode, crate::Policy::GateSet::default(), explicit_output)
}

pub fn compile_bundle_path_opts_library_with_gates(
    file: &str,
    mode: crate::Sema::CompileMode,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_library_with_gates_and_settings(
        file,
        mode,
        gates,
        explicit_output,
        &BTreeMap::new(),
    )
}

pub fn compile_bundle_path_opts_library_with_gates_and_settings(
    file: &str,
    mode: crate::Sema::CompileMode,
    gates: crate::Policy::GateSet,
    explicit_output: Option<&str>,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        false,
        gates,
        false,
        false,
        true,
        false,
        None,
        explicit_output,
        "dev",
        setting_overrides,
    )
}

/// Like `compile_bundle_path_opts`, but `debug_linemap = true` routes codegen
/// through `emit_bundle_dbg` (D-DBG3 step 2 / dap-debugger): every generated
/// statement gets a `// jet:line N` marker the native `jet debug` backend reads
/// back into a rust-line -> jet-line table. Used ONLY by the native debug build
/// path — every other caller keeps `debug_linemap = false` (byte-identical output).
pub fn compile_bundle_path_opts_dbg(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    debug_linemap: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        gates,
        web_target,
        false,
        false,
        debug_linemap,
        cross_target,
        None,
        "dev",
        &BTreeMap::new(),
    )
}

/// Compile one explicitly addressed runnable Output. Selection is resolved in
/// sema and carried into every lower tier as one checked callable fact.
pub fn compile_bundle_path_output(
    file: &str,
    output: &str,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_output_opts(
        file,
        output,
        false,
        crate::Policy::GateSet::default(),
        false,
        false,
        None,
    )
}

pub fn compile_bundle_path_output_opts(
    file: &str,
    output: &str,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_output_opts_with_settings(
        file,
        output,
        freestanding,
        gates,
        web_target,
        plugin_target,
        cross_target,
        &BTreeMap::new(),
    )
}

pub fn compile_bundle_path_output_opts_with_settings(
    file: &str,
    output: &str,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        crate::Sema::CompileMode::Run,
        freestanding,
        gates,
        web_target,
        plugin_target,
        false,
        false,
        cross_target,
        Some(output),
        "dev",
        setting_overrides,
    )
}

fn target_machine_usage_for_file(
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::TargetMachine::TargetMachineUse, Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    seed_build_facts(&mut bundle, "dev", false, &BTreeMap::new())?;
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let _lints = gate_diagnostics(
        &bundle,
        parse_teaching,
        diags,
        Vec::new(),
    )?;
    let mmio = collect_mmio_usage(&bundle);
    let mut core_apis: Vec<String> = bundle.used_core.into_iter().collect();
    core_apis.sort();
    Ok(crate::TargetMachine::TargetMachineUse {
        core_apis,
        mmio,
        ..crate::TargetMachine::TargetMachineUse::default()
    })
}

#[derive(Clone, Copy)]
struct PtrFact {
    address: u64,
    size: crate::TargetMachine::ByteSize,
}

fn collect_mmio_usage(bundle: &crate::AST::ProgramBundle) -> Vec<crate::TargetMachine::MmioAccess> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        let core_aliases = core_aliases(module);
        for item in &module.items {
            match item {
                crate::AST::Item::Func(f) => collect_mmio_func(f, &core_aliases, &mut out),
                crate::AST::Item::Struct(s) => {
                    for m in &s.methods {
                        collect_mmio_func(m, &core_aliases, &mut out);
                    }
                }
                crate::AST::Item::Enum(e) => {
                    for m in &e.methods {
                        collect_mmio_func(m, &core_aliases, &mut out);
                    }
                }
                crate::AST::Item::Impl(i) => {
                    for m in &i.methods {
                        collect_mmio_func(m, &core_aliases, &mut out);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn core_aliases(module: &crate::AST::LoadedModule) -> std::collections::HashMap<String, String> {
    let mut aliases = std::collections::HashMap::new();
    for import in &module.imports {
        if let crate::AST::ImportKind::Module(name, _) = &import.kind {
            if crate::Syntax::is_known_core_module(name) {
                let alias = if import.alias.is_empty() {
                    name.rsplit('.').next().unwrap_or(name).to_string()
                } else {
                    import.alias.clone()
                };
                aliases.insert(alias, name.clone());
            }
        } else if let Some(prefix) = match &import.kind {
            crate::AST::ImportKind::Unqualified { module_alias, .. } => {
                crate::AST::core_list_prefix(module_alias)
            }
            _ => None,
        } {
            for binding in import.walk_bindings() {
                let Some(original) = binding.original else {
                    continue;
                };
                let full = format!("{prefix}.{original}");
                if crate::Syntax::is_known_core_module(&full) {
                    aliases.insert(binding.local, full);
                }
            }
        }
    }
    aliases
}

fn collect_mmio_func(
    f: &crate::AST::Func,
    core_aliases: &std::collections::HashMap<String, String>,
    out: &mut Vec<crate::TargetMachine::MmioAccess>,
) {
    let mut ptrs = std::collections::HashMap::new();
    let reason = if f.is_unsafe {
        f.unsafe_reason.as_deref()
    } else {
        None
    };
    collect_mmio_stmts(&f.body, core_aliases, &mut ptrs, reason, out);
}

fn collect_mmio_stmts(
    stmts: &[crate::AST::Stmt],
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &mut std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetMachine::MmioAccess>,
) {
    for stmt in stmts {
        match stmt {
            crate::AST::Stmt::Val(b) => {
                collect_mmio_expr(&b.init, core_aliases, ptrs, unsafe_reason, out);
                if let Some(fact) = ptr_fact_from_expr(&b.init) {
                    ptrs.insert(b.name.clone(), fact);
                }
            }
            crate::AST::Stmt::Expr(e)
            | crate::AST::Stmt::DeferClose { close: e, .. }
            | crate::AST::Stmt::Return(Some(e), _) => {
                collect_mmio_expr(e, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Assign { value, .. } => {
                collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::While { cond, body, .. } => {
                collect_mmio_expr(cond, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::For { kind, body, .. } => {
                collect_mmio_for_kind(kind, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                collect_mmio_expr(subject, core_aliases, ptrs, unsafe_reason, out);
                for arm in arms {
                    collect_mmio_stmts(&arm.body, core_aliases, ptrs, unsafe_reason, out);
                }
                if let Some(body) = else_body {
                    collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
                }
            }
            crate::AST::Stmt::Loop { body, .. }
            | crate::AST::Stmt::Impure { body, .. }
            | crate::AST::Stmt::Reactive { body, .. }
            | crate::AST::Stmt::Region { body, .. }
            | crate::AST::Stmt::Policy { body, .. }
            | crate::AST::Stmt::TaskGroup { body, .. } => {
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Unsafe { audit, body, .. } => {
                collect_mmio_stmts(body, core_aliases, ptrs, audit.as_deref(), out);
            }
            crate::AST::Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                collect_mmio_expr(&init.init, core_aliases, ptrs, unsafe_reason, out);
                if let Some(fact) = ptr_fact_from_expr(&init.init) {
                    ptrs.insert(init.name.clone(), fact);
                }
                collect_mmio_expr(cond, core_aliases, ptrs, unsafe_reason, out);
                if let Some(step) = step {
                    collect_mmio_stmts(
                        std::slice::from_ref(step),
                        core_aliases,
                        ptrs,
                        unsafe_reason,
                        out,
                    );
                }
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Return(None, _)
            | crate::AST::Stmt::Break(_)
            | crate::AST::Stmt::Continue(_)
            | crate::AST::Stmt::BreakLabel(_, _)
            | crate::AST::Stmt::ContinueLabel(_, _) => {}
            _ => {}
        }
    }
}

fn collect_mmio_for_kind(
    kind: &crate::AST::ForKind,
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &mut std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetMachine::MmioAccess>,
) {
    match kind {
        crate::AST::ForKind::Range { start, end, step, exclusive: _ } => {
            collect_mmio_expr(start, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_expr(end, core_aliases, ptrs, unsafe_reason, out);
            if let Some(step) = step {
                collect_mmio_expr(step, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::ForKind::In { collection, step } => {
            collect_mmio_expr(collection, core_aliases, ptrs, unsafe_reason, out);
            if let Some(step) = step {
                collect_mmio_expr(step, core_aliases, ptrs, unsafe_reason, out);
            }
        }
    }
}

fn collect_mmio_expr(
    expr: &crate::AST::Expr,
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetMachine::MmioAccess>,
) {
    match expr {
        crate::AST::Expr::PtrFromAddr { addr, .. } => {
            collect_mmio_expr(addr, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if is_core_mem_receiver(receiver, core_aliases)
                && matches!(method.as_str(), "volatile_read" | "volatile_write")
            {
                if let Some(first) = args.first() {
                    if let crate::AST::Expr::Ident(name, _) = &first.expr {
                        if let Some(fact) = ptrs.get(name) {
                            out.push(crate::TargetMachine::MmioAccess {
                                address: fact.address,
                                size: fact.size,
                                unsafe_gate: unsafe_reason.map(|reason| {
                                    crate::TargetMachine::UnsafeGate {
                                        reason: reason.to_string(),
                                    }
                                }),
                            });
                        }
                    }
                }
            }
            collect_mmio_expr(receiver, core_aliases, ptrs, unsafe_reason, out);
            for arg in args {
                collect_mmio_expr(&arg.expr, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::Call(c) => {
            for arg in &c.args {
                collect_mmio_expr(&arg.expr, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::Binary(_, a, b, _) => {
            collect_mmio_expr(a, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_expr(b, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::Index { base, index, .. } => {
            collect_mmio_expr(base, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_expr(index, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            if let crate::AST::OrFallback::Value(v) = fallback {
                collect_mmio_expr(v, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::Unary(_, e, _)
        | crate::AST::Expr::Copy(e, _)
        | crate::AST::Expr::Place(e, _, _)
        | crate::AST::Expr::Deref(e, _)
        | crate::AST::Expr::RawOf(e, _)
        | crate::AST::Expr::Field(e, _, _)
        | crate::AST::Expr::OptField { base: e, .. } => {
            collect_mmio_expr(e, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::ListLit(elems, _) => {
            for elem in elems {
                collect_mmio_expr(elem, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                collect_mmio_expr(k, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_expr(v, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::TupleLit(fields, _, _) => {
            for (_, value) in fields {
                collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::CallValue { callee, args, .. } => {
            collect_mmio_expr(callee, core_aliases, ptrs, unsafe_reason, out);
            for arg in args {
                collect_mmio_expr(&arg.expr, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        _ => {}
    }
}

fn is_core_mem_receiver(
    receiver: &crate::AST::Expr,
    core_aliases: &std::collections::HashMap<String, String>,
) -> bool {
    matches!(receiver, crate::AST::Expr::Ident(alias, _) if core_aliases.get(alias).is_some_and(|m| m == "core.mem"))
}

fn ptr_fact_from_expr(expr: &crate::AST::Expr) -> Option<PtrFact> {
    let crate::AST::Expr::PtrFromAddr { elem, addr, .. } = expr else {
        return None;
    };
    let crate::AST::Expr::Int(address, _, _, _) = addr.as_ref() else {
        return None;
    };
    if *address < 0 {
        return None;
    }
    Some(PtrFact {
        address: *address as u64,
        size: byte_size_for_type(elem)?,
    })
}

fn byte_size_for_type(ty: &crate::AST::Type) -> Option<crate::TargetMachine::ByteSize> {
    match ty {
        crate::AST::Type::Bool => Some(crate::TargetMachine::ByteSize::bytes(1)),
        crate::AST::Type::Char | crate::AST::Type::Float32 => {
            Some(crate::TargetMachine::ByteSize::bytes(4))
        }
        crate::AST::Type::Int | crate::AST::Type::Float | crate::AST::Type::String => {
            Some(crate::TargetMachine::ByteSize::bytes(8))
        }
        crate::AST::Type::IntN { bits, .. } => {
            Some(crate::TargetMachine::ByteSize::bytes((*bits as u64) / 8))
        }
        crate::AST::Type::Tagged { inner, .. } => byte_size_for_type(inner),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct BuildRunOptions {
    pub grants: std::collections::BTreeSet<crate::Comptime::Build::BuildCapability>,
    /// Policy for typed legacy/plugin bridges. Capability grants and bridge
    /// policy are separate: an explicit `Exec` grant does not implicitly
    /// enable a legacy wrapper or a plugin.
    pub policy: crate::Comptime::Build::BuildPolicy,
    pub execute: bool,
    pub gates: crate::Policy::GateSet,
    /// Validate and expose declared graph authority without granting ambient
    /// comptime authority. Used only by read-only CLI/LSP inspection.
    pub inspect_only: bool,
    /// Materialize every registered generated source for the explicit
    /// `--emit-generated` inspection surface, even when the selected runtime
    /// target does not list the source explicitly.
    pub emit_generated: bool,
    pub locked: bool,
    pub freestanding: bool,
    pub web_target: bool,
    pub plugin_target: bool,
    pub cross_target: Option<String>,
    /// D-CONF-WORD1=A: the selected optimization bundle, exposed as the
    /// compile-time `@build.profile` fact.
    pub profile: String,
    /// D-CONF-KEY1: command-line contributions to declared package settings.
    pub setting_overrides: BTreeMap<String, String>,
    /// Optional host-owned remote builder binding. Source and CLI input cannot
    /// construct an endpoint or credential; `None` is always local.
    pub remote: Option<crate::Comptime::Build::RemoteBuildBinding>,
}

impl Default for BuildRunOptions {
    fn default() -> Self {
        BuildRunOptions {
            grants: std::collections::BTreeSet::new(),
            policy: crate::Comptime::Build::BuildPolicy::local_default(),
            execute: true,
            gates: crate::Policy::GateSet::default(),
            inspect_only: false,
            emit_generated: false,
            locked: false,
            freestanding: false,
            web_target: false,
            plugin_target: false,
            cross_target: None,
            profile: "dev".to_string(),
            setting_overrides: BTreeMap::new(),
            remote: None,
        }
    }
}

/// Seed the one build-fact snapshot before sema. Package identity comes from
/// `PackageFacts`; a manifest-less entry keeps the loader's filename fallback.
/// Provenance comes from the lock model, so no JIT/interpreter engine probes
/// the host and a locked build never consults the wall clock.
pub fn seed_build_facts(
    bundle: &mut crate::AST::ProgramBundle,
    profile: &str,
    locked: bool,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<(), Vec<Diagnostic>> {
    let manifest = match crate::Package::PackageFacts::load(&bundle.project_root) {
        None => None,
        Some(Ok(facts)) => Some(facts),
        Some(Err(error)) => {
            return Err(vec![Diagnostic::error(
                "E1206",
                "package manifest is not valid".to_string(),
                error.to_string(),
                "fix `package.jet` before compiling the package".to_string(),
                None,
            )]);
        }
    };
    let package_name = manifest
        .as_ref()
        .map(|facts| facts.name.clone())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            bundle.modules.get(bundle.entry).and_then(|module| {
                module
                    .path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| "script".to_string());
    let package_version = manifest
        .as_ref()
        .and_then(|facts| facts.version.clone())
        .unwrap_or_else(|| "0.0.0".to_string());
    let stamp = crate::Lock::build_stamp(&bundle.project_root, locked).map_err(|error| {
        vec![Diagnostic::error(
            "E3512",
            "the build provenance stamp is unavailable".to_string(),
            error,
            "run an unlocked build to create the lock stamp, then rerun with `--locked`"
                .to_string(),
            None,
        )]
    })?;
    let profile_key = jet_foundation::Policy::FactKey::new("Build.Profile");
    let mut profile_contributions = vec![jet_foundation::Policy::FactContribution::new(
        "Build.Profile",
        jet_foundation::Policy::FactValue::Text("dev".to_string()),
        jet_foundation::Policy::SourceScope::Package,
        jet_foundation::Policy::ContributionLayer::Declaration,
        manifest
            .as_ref()
            .map(|facts| facts.origin.clone())
            .unwrap_or_else(|| "<default>".to_string()),
    )];
    if profile != "dev" {
        profile_contributions.push(jet_foundation::Policy::FactContribution::new(
            "Build.Profile",
            jet_foundation::Policy::FactValue::Text(profile.to_string()),
            jet_foundation::Policy::SourceScope::Package,
            jet_foundation::Policy::ContributionLayer::CommandLine,
            "command line",
        ));
    }
    let profile_fact = jet_foundation::Policy::resolve(profile_key, profile_contributions)
        .map_err(|error| {
            vec![Diagnostic::error(
                "E3521",
                "the selected build profile has conflicting contributions".to_string(),
                error.message(),
                "make the profile writers agree, or select one explicit profile".to_string(),
                None,
            )]
        })?;
    let contributions = profile_fact
        .into_iter()
        .map(|fact| (fact.key.name.clone(), fact))
        .collect();
    let enum_types = fieldless_setting_enums(bundle);
    let mut settings = BTreeMap::new();
    let mut setting_provenance = BTreeMap::new();
    if let Some(facts) = manifest.as_ref() {
        for (key, declaration) in &facts.settings {
            let value = parse_setting_value(&declaration.ty, &declaration.default, &enum_types).map_err(|detail| {
                vec![setting_value_diagnostic(key, &declaration.ty, &declaration.default, detail)]
            })?;
            settings.insert(
                key.clone(),
                jet_foundation::Facts::BuildSettingFact {
                    ty: declaration.ty.clone(),
                    value,
                },
            );
            setting_provenance.insert(
                key.clone(),
                vec![format!("{}:settings.{key} (default)", facts.origin)],
            );
        }
        if let Some(profile_def) = facts.build_profiles.iter().find(|candidate| candidate.name == profile) {
            for (key, raw) in &profile_def.settings {
                apply_setting(
                    &mut settings,
                    facts,
                    key,
                    raw,
                    "profile",
                    &enum_types,
                )?;
                setting_provenance
                    .entry(key.clone())
                    .or_default()
                    .push(format!("{}:build.{profile}.settings.{key}", facts.origin));
            }
        }
    }
    for (key, raw) in setting_overrides {
        let Some(facts) = manifest.as_ref() else {
            let declaration_site = bundle
                .project_root
                .join(crate::Syntax::PACKAGE_FILE)
                .display()
                .to_string();
            return Err(vec![undeclared_setting_diagnostic(
                key,
                "the package has no `settings:` declaration",
                &declaration_site,
            )]);
        };
        apply_setting(&mut settings, facts, key, raw, "CLI", &enum_types)?;
        setting_provenance
            .entry(key.clone())
            .or_default()
            .push(format!("command line:--set {key}={raw}"));
    }
    bundle.build_facts = jet_foundation::Facts::BuildFactSnapshot {
        package_name,
        package_version,
        os: bundle.active_os,
        profile: profile.to_string(),
        stamp,
        contributions,
        settings,
        setting_provenance,
    };
    Ok(())
}

fn apply_setting(
    settings: &mut BTreeMap<String, jet_foundation::Facts::BuildSettingFact>,
    facts: &crate::Package::PackageFacts,
    key: &str,
    raw: &str,
    source: &str,
    enum_types: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), Vec<Diagnostic>> {
    let Some(declaration) = facts.settings.get(key) else {
        return Err(vec![undeclared_setting_diagnostic(
            key,
            &format!("the {source} contribution names no declaration in `package.jet`"),
            &facts.origin,
        )]);
    };
    let value = parse_setting_value(&declaration.ty, raw, enum_types).map_err(|detail| {
        vec![setting_value_diagnostic(key, &declaration.ty, raw, detail)]
    })?;
    settings.insert(
        key.to_string(),
        jet_foundation::Facts::BuildSettingFact {
            ty: declaration.ty.clone(),
            value,
        },
    );
    Ok(())
}

fn undeclared_setting_diagnostic(key: &str, why: &str, declaration_site: &str) -> Diagnostic {
    Diagnostic::error(
        "E0302",
        format!("`@build.settings.{key}` is undeclared"),
        why.to_string(),
        format!(
            "add `{key}: Type = default` to the `settings: .{{ … }}` block in `{declaration_site}`"
        ),
        None,
    )
}

fn setting_value_diagnostic(key: &str, ty: &str, raw: &str, detail: String) -> Diagnostic {
    Diagnostic::error(
        "E0302",
        format!("setting `{key}` cannot use `{raw}` as `{ty}`"),
        detail,
        format!("use a `{ty}` value for the declared setting `{key}`"),
        None,
    )
}

fn parse_setting_value(
    ty: &str,
    raw: &str,
    enum_types: &BTreeMap<String, BTreeSet<String>>,
) -> Result<jet_foundation::Facts::BuildFactValue, String> {
    let raw = raw.trim();
    match ty.trim() {
        "Bool" => match raw {
            "true" => Ok(jet_foundation::Facts::BuildFactValue::Bool(true)),
            "false" => Ok(jet_foundation::Facts::BuildFactValue::Bool(false)),
            _ => Err("Bool settings use `true` or `false`".to_string()),
        },
        "Int" => raw
            .parse::<i64>()
            .map(jet_foundation::Facts::BuildFactValue::Int)
            .map_err(|_| "Int settings use a signed 64-bit whole number".to_string()),
        "Char" => parse_setting_char(raw).map(jet_foundation::Facts::BuildFactValue::Char),
        "String" => Ok(jet_foundation::Facts::BuildFactValue::Text(unquote_setting(raw))),
        type_name => {
            if !valid_setting_type_name(type_name) {
                return Err("settings use Bool, Int, Char, String, or a fieldless enum type".to_string());
            }
            let variant = raw.strip_prefix('.').unwrap_or(raw).trim();
            if variant.is_empty() || !valid_setting_variant(variant) {
                return Err("fieldless enum settings use a named variant such as `.On`".to_string());
            }
            let Some(variants) = enum_types.get(type_name) else {
                return Err(format!("`{type_name}` is not a declared fieldless enum"));
            };
            if !variants.contains(variant) {
                return Err(format!("`{type_name}` has no fieldless variant `{variant}`"));
            }
            Ok(jet_foundation::Facts::BuildFactValue::Enum {
                type_name: type_name.to_string(),
                variant: variant.to_string(),
            })
        }
    }
}

fn fieldless_setting_enums(
    bundle: &crate::AST::ProgramBundle,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for module in &bundle.modules {
        collect_fieldless_setting_enums(&module.items, &mut out);
    }
    out
}

fn collect_fieldless_setting_enums(
    items: &[crate::AST::Item],
    out: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for item in items {
        match item {
            crate::AST::Item::Enum(def) => {
                let variants = def
                    .variants
                    .iter()
                    .filter_map(|variant| {
                        matches!(&variant.payload, crate::AST::VariantPayload::Unit)
                            .then_some(variant.name.clone())
                    })
                    .collect::<BTreeSet<_>>();
                if variants.len() == def.variants.len() {
                    out.insert(def.name.clone(), variants);
                }
            }
            crate::AST::Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_fieldless_setting_enums(body, out);
                }
            }
            crate::AST::Item::GenericModule(module) => {
                collect_fieldless_setting_enums(&module.body, out);
            }
            _ => {}
        }
    }
}

fn valid_setting_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn valid_setting_variant(name: &str) -> bool {
    name.split('.').all(|segment| {
        let mut chars = segment.chars();
        matches!(chars.next(), Some(first) if first.is_ascii_uppercase())
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn unquote_setting(raw: &str) -> String {
    let raw = raw.trim();
    let body = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::new();
    let mut escaped = false;
    for ch in body.chars() {
        if escaped {
            out.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

fn parse_setting_char(raw: &str) -> Result<char, String> {
    let body = match raw
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        Some(body) => body,
        None if raw.chars().count() == 1 => return Ok(raw.chars().next().unwrap()),
        None => return Err("Char settings use one quoted character".to_string()),
    };
    let value = unquote_setting(body);
    let mut chars = value.chars();
    let Some(ch) = chars.next() else {
        return Err("Char settings need one character".to_string());
    };
    if chars.next().is_some() {
        return Err("Char settings need one character".to_string());
    }
    Ok(ch)
}

#[derive(Debug, Clone)]
pub struct GeneratedSourceProvenance {
    pub name: String,
    pub path: std::path::PathBuf,
    pub digest: crate::Comptime::Build::ContentDigest,
}

#[derive(Debug, Clone)]
pub struct BuildRun {
    pub plan: crate::Comptime::Build::BuildPlan,
    pub execution: crate::Comptime::Build::BuildExecutionReport,
    pub probes: Vec<crate::Comptime::Build::BuildProbeFact>,
    pub generated: Vec<GeneratedSourceProvenance>,
}

#[derive(Debug)]
pub struct BuildCompileOutput {
    pub compile: crate::CompileOutput,
    pub build: Option<BuildRun>,
}

struct BuildFilesystemTransaction {
    files: Vec<(std::path::PathBuf, Option<Vec<u8>>)>,
    committed: bool,
}

impl BuildFilesystemTransaction {
    fn new(paths: impl IntoIterator<Item = std::path::PathBuf>) -> std::io::Result<Self> {
        let mut seen = std::collections::BTreeSet::new();
        let mut files = Vec::new();
        for path in paths.into_iter().filter(|path| seen.insert(path.clone())) {
            if path_has_symlinked_component(&path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("build transaction path `{}` contains a symlink", path.display()),
                ));
            }
            let before = match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            files.push((path, before));
        }
        Ok(Self {
            files,
            committed: false,
        })
    }
    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for BuildFilesystemTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, before) in self.files.iter().rev() {
            match before {
                Some(bytes) => {
                    let _ = safe_atomic_write(path, bytes);
                }
                None => {
                    if !std::fs::symlink_metadata(path)
                        .is_ok_and(|metadata| metadata.file_type().is_symlink())
                    {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    }
}

fn path_has_symlinked_component(path: &std::path::Path) -> bool {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => {
                current.push(std::path::MAIN_SEPARATOR.to_string());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => return true,
            std::path::Component::Normal(part) => current.push(part),
        }
        if std::fs::symlink_metadata(&current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return true;
        }
    }
    false
}

fn ensure_real_parent(path: &std::path::Path) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut current = std::path::PathBuf::new();
    for component in parent.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => {
                current.push(std::path::MAIN_SEPARATOR.to_string());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "build path contains a parent component",
                ));
            }
            std::path::Component::Normal(part) => current.push(part),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("build directory `{}` is not a real directory", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn safe_atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if path_has_symlinked_component(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("build path `{}` contains a symlink", path.display()),
        ));
    }
    ensure_real_parent(path)?;
    if std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("build output `{}` is a symlink", path.display()),
        ));
    }
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    for attempt in 0..100u32 {
        let temporary = parent.join(format!(
            ".jet-atomic-{}-{}",
            std::process::id(),
            attempt
        ));
        let mut file = match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        use std::io::Write;
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        std::fs::rename(&temporary, path)?;
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique build temporary file",
    ))
}

fn read_build_file(
    root: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<Vec<u8>> {
    if path.strip_prefix(root).is_err()
        || path_has_symlinked_component(path)
        || std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("build file `{}` contains a symlink or is outside the project", path.display()),
        ));
    }
    std::fs::read(path)
}

/// Static graph facts for CLI and LSP consumers. Build evaluation is pure and
/// generated files/actions are not materialized or executed.
pub fn query_build_plan(
    file: &str,
) -> Result<Option<crate::Comptime::Build::BuildPlan>, Vec<Diagnostic>> {
    compile_bundle_path_build(file, build_query_options())
    .map(|output| output.build.map(|build| build.plan))
}

fn build_query_options() -> BuildRunOptions {
    BuildRunOptions {
        // Inspection verifies source declarations and #Impure gates, but it
        // must not require execution grants merely to display the graph.
        grants: crate::Comptime::Build::BuildCapability::ALL.into_iter().collect(),
        policy: crate::Comptime::Build::BuildPolicy::local_default(),
        execute: false,
        // Graph inspection may describe effectful actions, but it has no
        // authority to perform ambient comptime I/O. A user-written #Impure
        // gate therefore still reaches E3411 instead of touching the host.
        gates: crate::Policy::GateSet::default(),
        inspect_only: true,
        emit_generated: false,
        locked: false,
        freestanding: false,
        web_target: false,
        plugin_target: false,
        cross_target: None,
        profile: "dev".to_string(),
        setting_overrides: BTreeMap::new(),
        remote: None,
    }
}

/// Ratified D-BUILDQUERY1 query expressions. `build` is deliberately the only
/// expression until another query spelling is owner-ratified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildQueryExpression {
    Build,
}

pub fn evaluate_build_query(
    file: &str,
    expression: BuildQueryExpression,
) -> Result<Option<crate::Comptime::Build::BuildPlan>, Vec<Diagnostic>> {
    match expression {
        BuildQueryExpression::Build => query_build_plan(file),
    }
}

/// LSP variant: the open document is authoritative even before save.
pub fn query_build_plan_with_overlay(
    file: &str,
    source: &str,
) -> Result<Option<crate::Comptime::Build::BuildPlan>, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(
        file,
        build_query_options(),
        Some((std::path::Path::new(file), source)),
        None,
    )
    .map(|output| output.build.map(|build| build.plan))
}

/// One canonical graph representation shared by CLI and LSP.
pub fn build_plan_json(plan: &crate::Comptime::Build::BuildPlan) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
    fn strings(values: &[String]) -> String {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| format!("\"{}\"", escape(value)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
    let graph = plan.graph();
    format!("{{\"schema_version\":1,\"default\":{},\"targets\":[{}],\"actions\":[{}],\"files\":[{}],\"toolchains\":[{}],\"probes\":[{}],\"generated\":[{}]}}",
        plan.default_target().map(|target| target.id().0.to_string()).unwrap_or_else(|| "null".to_string()),
        graph.targets.iter().map(|target| format!("{{\"id\":{},\"name\":\"{}\",\"kind\":\"{:?}\",\"deps\":[{}],\"actions\":[{}],\"files\":{}}}", target.id.0, escape(&target.name), target.kind, target.deps.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","), target.actions.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","), strings(&target.files))).collect::<Vec<_>>().join(","),
        graph.actions.iter().map(|action| { let real = &plan.actions()[action.id.0]; format!("{{\"id\":{},\"name\":\"{}\",\"inputs\":{},\"outputs\":{},\"caps\":{},\"pools\":{},\"toolchain\":\"{}\",\"probes\":{},\"cache\":\"{:?}\",\"compiler_owned\":{},\"provenance\":{}}}", action.id.0, escape(&action.name), strings(&action.inputs), strings(&action.outputs), strings(&action.caps.iter().map(|cap| cap.name().to_string()).collect::<Vec<_>>()), strings(&action.pools.iter().map(|pool| pool.as_str().to_string()).collect::<Vec<_>>()), escape(&plan.toolchains()[real.toolchain.id().0].name), strings(&real.probes.iter().map(|probe| plan.probes()[probe.id().0].name.clone()).collect::<Vec<_>>()), real.cache, real.compiler_owned, strings(&plan.explain_action_named(&action.name).map(|fact| fact.provenance).unwrap_or_default())) }).collect::<Vec<_>>().join(","),
        graph.files.iter().map(|file| format!("{{\"path\":\"{}\",\"owner\":{},\"consumers\":[{}],\"targets\":[{}]}}", escape(&file.path), file.owner.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string()), file.consumers.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","), file.targets.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","))).collect::<Vec<_>>().join(","),
        plan.toolchains().iter().map(|tool| format!("{{\"name\":\"{}\",\"target\":\"{}\"}}", escape(&tool.name), escape(&tool.target_triple))).collect::<Vec<_>>().join(","),
        plan.probes().iter().map(|probe| format!("{{\"name\":\"{}\",\"kind\":\"{:?}\",\"reproducibility\":\"{:?}\"}}", escape(&probe.name), probe.kind, probe.reproducibility)).collect::<Vec<_>>().join(","),
        plan.generated_modules().iter().map(|module| format!("{{\"name\":\"{}\",\"path\":\"{}\",\"digest\":\"{}\"}}", escape(&module.name), escape(module.path.as_str()), module.source_digest.as_str())).collect::<Vec<_>>().join(",")
    )
}

/// D-BUILDENTRY1 complete driver staging: check root bundle, evaluate selected
/// root `fn build`, materialize/re-check generated Jet, execute canonical graph,
/// remove build-only entry, then codegen runtime program.
pub fn compile_bundle_path_build(
    file: &str,
    options: BuildRunOptions,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(file, options, None, None)
}

/// Compile a build entry from the checked source snapshot selected by an
/// authority resolver. The loader consumes `source` as its entry overlay, so
/// it does not reopen the validated entry pathname.
pub fn compile_bundle_path_build_with_overlay(
    file: &str,
    source_path: &std::path::Path,
    source: &str,
    options: BuildRunOptions,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(file, options, Some((source_path, source)), None)
}

/// Compile one workspace member as a dependency authority boundary. Its
/// source declaration and local gate are checked normally, but a missing
/// effective grant is reported as dependency denial (`E3504`), not as a root
/// build policy denial (`E3503`).
pub fn compile_bundle_path_build_as_dependency(
    file: &str,
    options: BuildRunOptions,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(file, options, None, Some(file))
}

/// Dependency variant of `compile_bundle_path_build_with_overlay`.
pub fn compile_bundle_path_build_as_dependency_with_overlay(
    file: &str,
    source_path: &std::path::Path,
    source: &str,
    options: BuildRunOptions,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(file, options, Some((source_path, source)), Some(file))
}

fn compile_bundle_path_build_inner(
    file: &str,
    options: BuildRunOptions,
    overlay: Option<(&std::path::Path, &str)>,
    dependency_boundary: Option<&str>,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    let direct_package_overlay = if overlay.is_none() {
        package_manifest_build_overlay(file)?
    } else {
        None
    };
    let mut bundle = match (overlay, direct_package_overlay.as_ref()) {
        (Some(overlay), _) => crate::Loader::load_entry_with_overlay(file, Some(overlay), false)?,
        (None, Some((path, source))) => {
            crate::Loader::load_entry_with_overlay(file, Some((path, source)), false)?
        }
        (None, None) => crate::Loader::load_entry_with_overlay(file, None, false)?,
    };
    let mut runtime_bundle_for_package = None;
    // D-BUILDSCOPE1: resolve one package build entry through PackageFacts. The
    // checked resolver owns package-wide discovery; this Driver only loads the
    // checked winner and keeps the ordinary runtime bundle for post-build
    // compilation. Imported build functions remain inert.
    let runtime_source_paths = bundle
        .modules
        .iter()
        .map(|module| module.path.clone())
        .collect::<Vec<_>>();
    let package_entry = package_build_entry_source(&bundle.project_root)?;
    let package_manifest_entry = direct_package_overlay.is_some();
    let has_package_entry = package_entry.is_some() || package_manifest_entry;
    let active_os = crate::Syntax::OSTarget::active(options.cross_target.as_deref());
    let entry_path = std::path::Path::new(file);
    let entry_dir = entry_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let is_workspace_entry = match AuthorityResolver::open(entry_dir) {
        Ok(resolver) => match resolver.resolve_workspace_source() {
            Ok(Some(source)) => {
                let entry_name = entry_path.file_name().ok_or_else(|| {
                    vec![Diagnostic::error(
                        "E1334",
                        "workspace entry cannot be resolved".to_string(),
                        "the workspace entry must name one regular file".to_string(),
                        "use an existing regular run.jet entry".to_string(),
                        None,
                    )]
                })?;
                let checked_entry = resolver
                    .checked_file(std::path::Path::new(entry_name))
                    .map_err(|error| vec![error.diagnostic()])?;
                resolver
                    .revalidate_file(&checked_entry)
                    .map_err(|error| vec![error.diagnostic()])?;
                resolver
                    .revalidate_source(&source)
                    .map_err(|error| vec![error.diagnostic()])?;
                if source.checked.path != checked_entry.path {
                    false
                } else if source.role
                    != jet_pkg_model::WorkspacePlan::WorkspaceSourceRole::Index
                {
                    return Err(vec![Diagnostic::error(
                        "E1334",
                        "workspace build entry is selected by an authority source".to_string(),
                        "workspace build execution requires the matching source to have the Index role; an authority source cannot select a workspace entry".to_string(),
                        "declare the workspace index in `workspace.jet`, or build the member entry directly".to_string(),
                        None,
                    )]);
                } else {
                    true
                }
            }
            Ok(None) => false,
            Err(error) => return Err(vec![error.workspace_diagnostic()]),
        },
        Err(error) if error.is_missing() => false,
        Err(error) => return Err(vec![error.diagnostic()]),
    };
    let compile_mode = if options.plugin_target || has_package_entry || is_workspace_entry {
        crate::Sema::CompileMode::Check
    } else {
        crate::Sema::CompileMode::Run
    };
    bundle.active_os = active_os;
    seed_build_facts(
        &mut bundle,
        &options.profile,
        options.locked,
        &options.setting_overrides,
    )?;
    bundle.web_partition_enforced = options.web_target;
    let local_build_indices = bundle.modules[bundle.entry]
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, crate::AST::Item::Func(func) if func.name == "build")
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if local_build_indices.len() > 1 {
        return Err(vec![duplicate_build_entries(&bundle, &local_build_indices)]);
    }
    let entry_module_path = bundle.modules[bundle.entry].path.clone();
    if let Some((package_path, package_source)) = package_entry.as_ref() {
        if !local_build_indices.is_empty()
            && normalize_project_path(&bundle.project_root, package_path)
                != normalize_project_path(&bundle.project_root, &entry_module_path)
        {
            let source_span = local_build_indices
                .first()
                .and_then(|index| match &bundle.modules[bundle.entry].items[*index] {
                    crate::AST::Item::Func(func) => Some(func.name_span),
                    _ => None,
                });
            let local_location = local_build_indices
                .first()
                .map(|index| build_function_location(&bundle.modules[bundle.entry], *index))
                .unwrap_or_else(|| entry_module_path.display().to_string());
            let package_location = build_source_location(package_path, package_source);
            return Err(vec![build_entry_conflict(
                "the package",
                &local_location,
                &package_location,
                source_span,
            )]);
        }
    }
    if let Some((package_path, package_source)) = package_entry {
        if normalize_project_path(&bundle.project_root, &package_path)
            == normalize_project_path(&bundle.project_root, &entry_module_path)
        {
            // The selected runtime file already is the checked package build
            // entry. Keep its loader snapshot and continue through one path.
        } else {
            runtime_bundle_for_package = Some(bundle);
            let package_path_string = package_path.to_string_lossy().into_owned();
            bundle = crate::Loader::load_entry_with_overlay(
                &package_path_string,
                Some((&package_path, &package_source)),
                false,
            )?;
            bundle.active_os = active_os;
            seed_build_facts(
                &mut bundle,
                &options.profile,
                options.locked,
                &options.setting_overrides,
            )?;
            bundle.web_partition_enforced = options.web_target;
        }
    }
    let local_build_indices = bundle.modules[bundle.entry]
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, crate::AST::Item::Func(func) if func.name == "build")
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if local_build_indices.len() > 1 {
        return Err(vec![duplicate_build_entries(&bundle, &local_build_indices)]);
    }
    let build_index = local_build_indices.into_iter().next();
    let build_span = build_index.and_then(|index| {
        match &bundle.modules[bundle.entry].items[index] {
            crate::AST::Item::Func(build) => Some(build.span),
            _ => None,
        }
    });
    if let Some(index) = build_index {
        let crate::AST::Item::Func(build) = &bundle.modules[bundle.entry].items[index] else {
            unreachable!()
        };
        if !valid_build_signature(build) {
            return Err(vec![bad_build_signature(build.name_span)]);
        }
    }

    // Build code is compiler-host code. Target restrictions apply only after
    // the selected runtime program replaces it.
    let (diags, effect_facts) =
        crate::Sema::check_bundle_with_effect_facts_for_build(&mut bundle, compile_mode);
    let extension_diags = crate::CompilerExtensionHook::post_sema_diagnostics(
        &bundle,
        Some(&effect_facts),
        &diags,
    );
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let mut lints = classify_diagnostics(
        &bundle,
        parse_teaching
            .into_iter()
            .chain(diags)
            .chain(extension_diags)
            .collect(),
        build_span.is_some(),
    )?;

    let mut build_run = None;
    let mut filesystem_transaction = None;
    let mut generated_lock_provenance = None;
    if let Some(index) = build_index {
        let build = match &bundle.modules[bundle.entry].items[index] {
            crate::AST::Item::Func(func) => func,
            _ => unreachable!(),
        };
        let declared_build_effects = build
            .declared_effects
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|(name, _)| crate::Comptime::Build::BuildCapability::parse(name))
            .collect::<std::collections::BTreeSet<_>>();
        let has_impure_gate = contains_impure_gate(&build.body);
        let mut funcs = std::collections::HashMap::new();
        let mut methods = std::collections::HashMap::new();
        let mut structs = std::collections::HashMap::new();
        let mut enums = std::collections::HashMap::new();
        let mut migrations: std::collections::HashMap<String, Vec<&crate::AST::MigrationDecl>> =
            std::collections::HashMap::new();
        let mut computed_fields = std::collections::HashMap::new();
        let mut distinct_ranges = std::collections::HashMap::new();
        let mut distinct_bases = std::collections::HashMap::new();
        let mut function_name_counts = std::collections::HashMap::<String, usize>::new();
        let mut type_name_counts = std::collections::HashMap::<String, usize>::new();
        let mut const_name_counts = std::collections::HashMap::<String, usize>::new();
        for module in &bundle.modules {
            for item in &module.items {
                match item {
                    crate::AST::Item::Func(func) => {
                        *function_name_counts.entry(func.name.clone()).or_default() += 1
                    }
                    crate::AST::Item::Struct(def) => {
                        *type_name_counts.entry(def.name.clone()).or_default() += 1
                    }
                    crate::AST::Item::Enum(def) => {
                        *type_name_counts.entry(def.name.clone()).or_default() += 1
                    }
                    crate::AST::Item::Const(def) if def.is_comptime && def.ct.is_some() => {
                        *const_name_counts.entry(def.name.clone()).or_default() += 1
                    }
                    _ => {}
                }
            }
        }
        // D-FIELDPOL1: the build interpreter consumes the same source
        // expressions that sema rewrote and checked as computed getters. Keep
        // both the qualified and unique short type keys; this mirrors the
        // method/type lookup tables above and avoids a build-only reflection
        // gap for sibling-field reads.
        for module in &bundle.modules {
            for item in &module.items {
                if let crate::AST::Item::Struct(def) = item {
                    let owner = format!("{}::{}", module.alias, def.name);
                    for field in &def.fields {
                        if let Some(expression) = &field.computed {
                            computed_fields.insert(
                                (owner.clone(), field.name.clone()),
                                expression.as_ref(),
                            );
                            if type_name_counts.get(&def.name) == Some(&1) {
                                computed_fields.insert(
                                    (def.name.clone(), field.name.clone()),
                                    expression.as_ref(),
                                );
                            }
                        }
                    }
                }
            }
        }
        for module in &bundle.modules {
            for item in &module.items {
                match item {
                    crate::AST::Item::Func(func) => {
                        funcs.insert(format!("{}::{}", module.alias, func.name), func);
                        if function_name_counts.get(&func.name) == Some(&1) {
                            funcs.insert(func.name.clone(), func);
                        }
                    }
                    crate::AST::Item::Struct(def) => {
                        let owner = format!("{}::{}", module.alias, def.name);
                        structs.insert(owner.clone(), def);
                        if type_name_counts.get(&def.name) == Some(&1) {
                            structs.insert(def.name.clone(), def);
                        }
                        for method in &def.methods {
                            methods.insert((owner.clone(), method.name.clone()), method);
                            if type_name_counts.get(&def.name) == Some(&1) {
                                methods.insert((def.name.clone(), method.name.clone()), method);
                            }
                        }
                    }
                    crate::AST::Item::Enum(def) => {
                        let owner = format!("{}::{}", module.alias, def.name);
                        enums.insert(owner.clone(), def);
                        if type_name_counts.get(&def.name) == Some(&1) {
                            enums.insert(def.name.clone(), def);
                        }
                        for method in &def.methods {
                            methods.insert((owner.clone(), method.name.clone()), method);
                            if type_name_counts.get(&def.name) == Some(&1) {
                                methods.insert((def.name.clone(), method.name.clone()), method);
                            }
                        }
                    }
                    crate::AST::Item::Impl(imp) => {
                        let owner = format!("{}::{}", module.alias, imp.type_name);
                        for method in &imp.methods {
                            methods.insert((owner.clone(), method.name.clone()), method);
                            if type_name_counts.get(&imp.type_name) == Some(&1) {
                                methods
                                    .insert((imp.type_name.clone(), method.name.clone()), method);
                            }
                        }
                    }
                    crate::AST::Item::Migration(m) => {
                        migrations.entry(m.type_name.clone()).or_default().push(m);
                    }
                    crate::AST::Item::Distinct(def) => {
                        distinct_ranges.insert(
                            def.name.clone(),
                            def.range.map(|(lo, hi, _)| (lo, hi)),
                        );
                        distinct_bases.insert(def.name.clone(), def.base.clone());
                    }
                    crate::AST::Item::UnitFamily(family) => {
                        for def in family.distinct_defs() {
                            distinct_ranges.insert(
                                def.name.clone(),
                                def.range.map(|(lo, hi, _)| (lo, hi)),
                            );
                            distinct_bases.insert(def.name, def.base);
                        }
                    }
                    _ => {}
                }
            }
        }
        // D-FIELDPOL1/D-MODCOMPUTE1: top-level immutable comptime constants
        // are real inputs to the same pure build interpreter as computed
        // fields. Keep qualified names always and expose a short name only
        // when the program has one unambiguous declaration.
        let mut globals = std::collections::HashMap::new();
        for module in &bundle.modules {
            for item in &module.items {
                let crate::AST::Item::Const(def) = item else { continue };
                if !def.is_comptime { continue; }
                let Some(value) = &def.ct else { continue };
                globals.insert(format!("{}::{}", module.alias, def.name), value.clone());
                if const_name_counts.get(&def.name) == Some(&1) {
                    globals.insert(def.name.clone(), value.clone());
                }
            }
        }
        let core_imports = core_aliases(&bundle.modules[bundle.entry]);
        let info = crate::Comptime::ProgramInfo {
            globals,
            methods,
            structs,
            enums,
            computed_fields,
            distinct_ranges,
            distinct_bases,
            core_imports,
            build_facts: bundle.build_facts.clone(),
            migrations,
        };
        let semantic_facts = program_semantic_facts(&bundle, &effect_facts);
        let program_value = crate::Comptime::build_program_info(&bundle, &semantic_facts);
        // Package and workspace entries may be selected from `src/` or from
        // a named source file. Build-relative inputs always belong to the
        // owning project root, which is also the root used by generated
        // output, action execution, and lock provenance.
        let base_dir = &bundle.project_root;
        let package = build_package_name(file)?;
        let mut evaluated = crate::Comptime::Build::with_packaged_plugin_runner(
            crate::BuildPluginHook::run_packaged_build_plugin,
            || crate::Comptime::run_build_entry_with_policy(
                build,
                &funcs,
                base_dir,
                &info,
                program_value,
                &package,
                options.gates,
                options.policy.clone(),
            ),
        )
        .map_err(|diag| vec![diag])?;

        if !evaluated.diagnostics.is_empty() {
            return Err(evaluated.diagnostics);
        }

        let dependency_name = dependency_boundary.map(build_package_name).transpose()?;
        validate_build_authority(
            &evaluated.plan,
            &declared_build_effects,
            has_impure_gate,
            &options,
            dependency_name,
            build.name_span,
        )?;
        validate_legacy_project_imports(&evaluated.plan, &bundle.project_root, build.name_span)?;

        let package_spec_bundle = runtime_bundle_for_package.as_ref().unwrap_or(&bundle);
        let package_specs = if package_spec_bundle.dep_roots.is_empty() {
            Vec::new()
        } else {
            compiler_package_specs(package_spec_bundle, &package)
        };
        let compiler_identity = format!(
            "{}@{}#{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            option_env!("JET_COMPILER_BUILD_ID").unwrap_or(env!("CARGO_PKG_VERSION")),
        );
        let compiler_target = options
            .cross_target
            .clone()
            .unwrap_or_else(|| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH));
        let compiler_profile = evaluated
            .plan
            .default_profile
            .clone()
            .unwrap_or_else(|| "default".to_string());
        if !package_specs.is_empty() && !evaluated.plan.targets().is_empty() {
            evaluated
                .plan
                .add_compiler_package_actions(
                    &package_specs,
                    compiler_identity,
                    &compiler_target,
                    &compiler_profile,
                )
                .map_err(|error| vec![build_plan_diagnostic(&error)])?;
        }
        let compiler_artifacts = package_specs
            .iter()
            .map(|spec| (spec.name.clone(), spec.clone()))
            .collect::<BTreeMap<_, _>>();
        let compiler_runner = move |
            action: &crate::Comptime::Build::BuildAction,
            snapshots: &[crate::Comptime::Build::ActionInputSnapshot],
        | {
            let package_name = action
                .labels
                .get("compiler.package")
                .ok_or_else(|| "compiler action has no package identity".to_string())?;
            let spec = compiler_artifacts
                .get(package_name)
                .ok_or_else(|| format!("compiler package `{package_name}` was not prepared"))?;
            let mut artifact = format!(
                "jet.sealed-package.v1\npackage={}\nsource={}\ncompiler={}\ntarget={}\nprofile={}\n",
                package_name,
                spec.source_digest.as_str(),
                action.labels.get("compiler.identity").map(String::as_str).unwrap_or_default(),
                action.labels.get("compiler.target").map(String::as_str).unwrap_or_default(),
                action.labels.get("compiler.profile").map(String::as_str).unwrap_or_default(),
            );
            for snapshot in snapshots {
                artifact.push_str("dependency=");
                artifact.push_str(snapshot.path.as_str());
                artifact.push('=');
                artifact.push_str(snapshot.digest.as_str());
                artifact.push('\n');
            }
            Ok(vec![artifact.into_bytes()])
        };

        let selected_actions = evaluated
            .plan
            .selected_action_ids()
            .map_err(|error| vec![build_plan_diagnostic(&error)])?;
        let selected_generated = if options.emit_generated {
            evaluated.plan.generated_modules().iter().collect()
        } else {
            evaluated
                .plan
                .selected_generated_modules()
                .map_err(|error| vec![build_plan_diagnostic(&error)])?
        };
        let mut existing_source_paths = runtime_source_paths.clone();
        existing_source_paths.extend(bundle.modules.iter().map(|module| module.path.clone()));
        let selected_action_outputs = evaluated
            .plan
            .actions()
            .iter()
            .filter(|action| selected_actions.contains(&action.id))
            .flat_map(|action| action.outputs.iter().map(|output| output.as_str().to_string()))
            .collect::<Vec<_>>();
        validate_selected_action_outputs(
            &evaluated.plan,
            &selected_actions,
            &selected_generated,
            &bundle.project_root,
            &existing_source_paths,
            build_span,
        )?;
        let transaction_paths = selected_generated
            .iter()
            .map(|module| bundle.project_root.join(module.path.as_str()))
            .chain(
                evaluated
                    .plan
                    .actions()
                    .iter()
                    .filter(|action| selected_actions.contains(&action.id))
                    .flat_map(|action| {
                        action
                            .outputs
                            .iter()
                            .map(|output| bundle.project_root.join(output.as_str()))
                    }),
            )
            .chain(std::iter::once(bundle.project_root.join(".jet/lock")))
            .collect::<Vec<_>>();
        filesystem_transaction = Some(
            BuildFilesystemTransaction::new(transaction_paths)
                .map_err(|error| vec![generated_io_diag("build filesystem transaction", &error)])?,
        );
        if options.locked {
            let planned_generated = selected_generated
                .iter()
                .map(|module| crate::AST::ComptimeInput {
                    path: module.path.as_str().to_string(),
                    hash: module.source_digest.as_str().to_string(),
                })
                .collect::<Vec<_>>();
            crate::Lock::verify_locked_generated_inputs(&bundle.project_root, &planned_generated)
                .map_err(|diagnostic| vec![diagnostic])?;
        }
        let mut generated = if options.execute {
            materialize_and_check_generated(
                &selected_generated,
                &bundle.project_root,
                &existing_source_paths,
                &selected_action_outputs,
                build_span,
                &bundle.build_facts,
            )?
        } else {
            selected_generated
                .iter()
                .map(|module| GeneratedSourceProvenance {
                    name: module.name.clone(),
                    path: bundle.project_root.join(module.path.as_str()),
                    digest: module.source_digest.clone(),
                })
                .collect()
        };
        let executed = if options.execute {
            let execution_grants = effective_grants(&options, &evaluated.plan);
            crate::Comptime::Build::execute_build_plan_with_front_end_and_remote_and_compiler(
                &evaluated.plan,
                &bundle.project_root,
                &execution_grants,
                crate::Comptime::Build::FrontEndCompletion::all_complete(),
                options.remote.as_ref(),
                Some(&compiler_runner),
            )
            .map_err(|error| vec![build_execution_diagnostic(error)])?
        } else {
            crate::Comptime::Build::BuildExecutionResult {
                report: evaluated
                    .plan
                    .execution_report(&[])
                    .map_err(|error| vec![build_plan_diagnostic(&error)])?,
                probes: Vec::new(),
            }
        };
        if options.execute {
            generated.extend(check_action_generated_sources(
                &evaluated.plan,
                &bundle.project_root,
                &existing_source_paths,
                build_span,
                &bundle.build_facts,
            )?);
            let mut locked_provenance = generated
                .iter()
                .map(|item| crate::AST::ComptimeInput {
                    path: item
                        .path
                        .strip_prefix(&bundle.project_root)
                        .unwrap_or(&item.path)
                        .display()
                        .to_string(),
                    hash: item.digest.as_str().to_string(),
                })
                .collect::<Vec<_>>();
            locked_provenance.extend(evaluated.comptime_inputs.iter().cloned());
            for action in evaluated
                .plan
                .actions()
                .iter()
                .filter(|action| {
                    selected_actions.contains(&action.id) && !action.is_compiler_owned()
                })
            {
                for input in &action.inputs {
                    let path = normalize_project_path(&bundle.project_root, std::path::Path::new(input.as_str()));
                    let bytes = read_build_file(&bundle.project_root, &path)
                        .map_err(|error| vec![generated_io_diag(&action.name, &error)])?;
                    locked_provenance.push(crate::AST::ComptimeInput {
                        path: input.as_str().to_string(),
                        hash: crate::Comptime::Build::ContentDigest::from_bytes(&bytes)
                            .as_str()
                            .to_string(),
                    });
                }
                for output in &action.outputs {
                    let path = normalize_project_path(&bundle.project_root, std::path::Path::new(output.as_str()));
                    let bytes = read_build_file(&bundle.project_root, &path)
                        .map_err(|error| vec![generated_io_diag(&action.name, &error)])?;
                    locked_provenance.push(crate::AST::ComptimeInput {
                        path: output.as_str().to_string(),
                        hash: crate::Comptime::Build::ContentDigest::from_bytes(&bytes)
                            .as_str()
                            .to_string(),
                    });
                }
            }
            locked_provenance.sort_by(|a, b| a.path.cmp(&b.path));
            locked_provenance.dedup_by(|a, b| a.path == b.path && a.hash == b.hash);
            // Keep the lock write until the fresh selected runtime bundle has
            // passed its complete sema check.  A package dependency loader
            // may run during that reload; writing a new generated-only lock
            // here would make that loader see an incomplete dependency lock.
            generated_lock_provenance = Some(locked_provenance);
        }
        let mut planned_bundle = if options.execute {
            load_planned_runtime_bundle(file, &evaluated.plan, &generated, &bundle.project_root)?
        } else {
            bundle
        };
        planned_bundle
            .comptime_inputs
            .extend(evaluated.comptime_inputs);
        build_run = Some(BuildRun {
            plan: evaluated.plan,
            execution: executed.report,
            probes: executed.probes,
            generated,
        });
        bundle = planned_bundle;
        bundle.active_os = active_os;
        seed_build_facts(
            &mut bundle,
            &options.profile,
            options.locked,
            &options.setting_overrides,
        )?;
        bundle.web_partition_enforced = options.web_target;
    }

    // Imported build entries are checked but never run. They are build-only
    // values and must not leak into runtime codegen (root was removed above).
    for module in &mut bundle.modules {
        module
            .items
            .retain(|item| !matches!(item, crate::AST::Item::Func(func) if func.name == "build"));
    }

    // The selected target source closure and generated modules are a fresh
    // program, not syntax checked in isolation. Re-run the complete front end
    // before any runtime codegen.
    if build_run.is_some() && options.execute {
        let (planned_diags, planned_facts) = if options.freestanding && !options.gates.is_empty() {
            (
                crate::Sema::check_bundle_freestanding_with_gates(&mut bundle, compile_mode, options.gates),
                None,
            )
        } else if options.freestanding {
            (
                crate::Sema::check_bundle_freestanding(&mut bundle, compile_mode),
                None,
            )
        } else if !options.gates.is_empty() {
            (
                crate::Sema::check_bundle_gates(&mut bundle, compile_mode, options.gates),
                None,
            )
        } else {
            let (diags, facts) =
                crate::Sema::check_bundle_with_effect_facts(&mut bundle, compile_mode);
            (diags, Some(facts))
        };
        let extension_diags = crate::CompilerExtensionHook::post_sema_diagnostics(
            &bundle,
            planned_facts.as_ref(),
            &planned_diags,
        );
        let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
        let planned_lints = classify_diagnostics(
            &bundle,
            parse_teaching
                .into_iter()
                .chain(planned_diags)
                .chain(extension_diags)
                .collect(),
            false,
        )?;
        lints.extend(planned_lints);
    }

    if let Some(provenance) = generated_lock_provenance.take() {
        crate::Lock::record_generated_inputs(
            &bundle.project_root,
            &provenance,
            options.locked,
            &bundle.build_facts.stamp,
        )
            .map_err(|diagnostic| vec![diagnostic])?;
    }

    // Static graph/query/explain (`execute: false`) must not codegen the
    // pre-build entry: `fn run` may call generated symbols that only exist
    // after materialization. CLI/LSP consumers only need `build.plan`.
    if !options.execute {
        if let Some(transaction) = filesystem_transaction.as_mut() {
            transaction.commit();
        }
        return Ok(BuildCompileOutput {
            compile: crate::CompileOutput {
                rust: String::new(),
                lints,
                ffi: None,
                clinks: Vec::new(),
                capabilities: crate::Capabilities::default(),
                comptime_inputs: std::mem::take(&mut bundle.comptime_inputs),
                web: None,
                web_partition_report: None,
                plugin: None,
                library: None,
                library_config: None,
                inferred_layer: bundle.inferred_layer,
                layer_ceiling: bundle.layer_ceiling,
            },
            build: build_run,
        });
    }

    let ffi = match options.cross_target.as_deref() {
        Some(target) => crate::FFI::prepare_for_target(&bundle, target),
        None => crate::FFI::prepare(&bundle),
    }
    .map_err(|diags| diags)?;
    if options.web_target {
        let misses = crate::Codegen::validate_web_tir_support(&bundle, ffi.as_ref());
        if !misses.is_empty() {
            return Err(misses.into_iter().map(|miss| Diagnostic::error(
                "E-WEB-TIR-UNSUPPORTED",
                format!("web output cannot compile `{}` yet", miss.func_name),
                "the selected BuildPlan program uses a construct unavailable to web lowering".to_string(),
                "select web-covered sources or simplify the named function".to_string(),
                Some(miss.span),
            )).collect());
        }
    }
    let rust = crate::Codegen::emit_bundle_dbg(&bundle, ffi.as_ref(), false, active_os);
    let web = if options.web_target {
        Some(
            crate::Codegen::emit_web(&bundle, compile_mode, ffi.as_ref()).map_err(|miss| {
                vec![Diagnostic::error(
                    "E-WEB-TIR-UNSUPPORTED",
                    format!("web output cannot compile `{}` yet", miss.func_name),
                    "web emitter capability facts drifted after validation".to_string(),
                    "report this compiler bug with the named function".to_string(),
                    Some(miss.span),
                )]
            })?,
        )
    } else {
        None
    };
    let plugin = if options.plugin_target {
        let errors = crate::PluginExport::validate_export_surface(&bundle);
        if !errors.is_empty() {
            return Err(errors);
        }
        let name = crate::PluginExport::resolve_export_name(&bundle);
        crate::PluginExport::check_and_freeze_version(&bundle, &name)?;
        Some(crate::Codegen::emit_plugin(&bundle, &rust, &name))
    } else {
        None
    };
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let compile = crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs: std::mem::take(&mut bundle.comptime_inputs),
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin,
        library: None,
        library_config: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    };
    if let Some(transaction) = filesystem_transaction.as_mut() {
        transaction.commit();
    }
    Ok(BuildCompileOutput {
        compile,
        build: build_run,
    })
}

fn load_planned_runtime_bundle(
    build_file: &str,
    plan: &crate::Comptime::Build::BuildPlan,
    generated: &[GeneratedSourceProvenance],
    project_root: &std::path::Path,
) -> Result<crate::AST::ProgramBundle, Vec<Diagnostic>> {
    let sources = plan
        .selected_sources()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    let Some(entry_source) = sources.first() else {
        return Err(vec![Diagnostic::error(
            "E3502",
            "the selected build target has no Jet sources".to_string(),
            "BuildPlan selects the exact program passed back through lexer, parser, sema, and codegen".to_string(),
            "add the runtime entry file to the selected target's sources".to_string(),
            None,
        )]);
    };
    let resolve = |path: &str| {
        let path = std::path::Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        }
    };
    let entry_path = resolve(entry_source.as_str());
    let mut bundle = crate::Loader::load_entry_with_overlay(
        entry_path.to_str().unwrap_or(build_file),
        None,
        false,
    )?;

    // Additional selected roots and generated modules merge before the one
    // complete sema pass, so runtime code can call generated declarations.
    let mut additions = sources
        .iter()
        .skip(1)
        .map(|path| ("selected source", resolve(path.as_str())))
        .chain(
            generated
                .iter()
                .map(|item| (item.name.as_str(), item.path.clone())),
        )
        .collect::<Vec<_>>();
    additions.sort_by(|a, b| a.1.cmp(&b.1));
    additions.dedup_by(|a, b| a.1 == b.1);
    let selected_paths = additions
        .iter()
        .map(|(_, path)| normalize_project_path(project_root, path))
        .collect::<std::collections::BTreeSet<_>>();
    let already_loaded = bundle
        .modules
        .iter()
        .map(|module| normalize_project_path(project_root, &module.path))
        .collect::<std::collections::BTreeSet<_>>();
    let mut loaded_by_path = std::collections::BTreeMap::new();
    for (generator, path) in additions {
        let loaded = crate::Loader::load_entry_with_overlay(
            path.to_str().unwrap_or(build_file),
            None,
            false,
        )
        .map_err(|mut diagnostics| {
            for diagnostic in &mut diagnostics {
                diagnostic.what = format!("generated by `{generator}`: {}", diagnostic.what);
            }
            diagnostics
        })?;
        for module in loaded.modules {
            let key = normalize_project_path(project_root, &module.path);
            loaded_by_path.entry(key).or_insert(module);
        }
    }

    // Selected roots are promoted into the runtime entry below. Their file
    // imports cannot be copied verbatim: those paths are relative to the
    // generated file, while the promoted functions now live in the entry
    // module. Keep each loaded import target as an inline code module so the
    // promoted functions retain their namespace without a second loader pass.
    let mut inline_imports = std::collections::BTreeMap::new();
    for module in loaded_by_path.values() {
        for import in &module.imports {
            let crate::AST::ImportKind::File(import_path, _) = &import.kind else {
                continue;
            };
            let mut target_path = module
                .path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf();
            for part in import_path.split('/') {
                if !part.is_empty() && part != "." {
                    target_path.push(part);
                }
            }
            target_path.set_extension(crate::Syntax::FILE_EXT);
            let target_path = normalize_project_path(project_root, &target_path);
            let Some(target) = loaded_by_path.get(&target_path) else {
                continue;
            };
            let alias = import.import_alias();
            inline_imports.entry(alias).or_insert_with(|| {
                (
                    target.items.clone(),
                    target.web_target_ceiling,
                )
            });
        }
    }

    // Promote every selected root exactly once. A selected module can also be
    // discovered transitively while loading another selected root; collecting
    // first prevents that module from being both an inline import and a second
    // top-level declaration set.
    for path in &selected_paths {
        if already_loaded.contains(path) {
            continue;
        }
        if let Some(module) = loaded_by_path.remove(path) {
            bundle.modules[bundle.entry].items.extend(module.items);
        }
    }

    let inline_aliases = inline_imports.keys().cloned().collect::<std::collections::BTreeSet<_>>();
    for (alias, (items, web_target)) in inline_imports {
        bundle.modules[bundle.entry]
            .items
            .push(crate::AST::Item::CodeModule(crate::AST::CodeModule {
                name: alias,
                name_span: crate::Diagnostics::Span::new(0, 0),
                is_pub: false,
                is_package_pub: false,
                body: Some(items),
                imports: Vec::new(),
                web_target,
                instance_identity: None,
                span: crate::Diagnostics::Span::new(0, 0),
            }));
    }

    // Keep quoted imports from selected/generated roots available after their
    // top-level declarations are merged into the runtime entry. The ordinary
    // loader already checked these modules; wrapping their items as inline
    // CodeModules preserves the imported alias without teaching the runtime
    // bundle a second file-resolution mechanism.
    for (path, module) in loaded_by_path {
        if already_loaded.contains(&path)
            || selected_paths.contains(&path)
            || inline_aliases.contains(&module.alias)
        {
            continue;
        }
        let alias = module.alias.clone();
        bundle.modules[bundle.entry]
            .items
            .push(crate::AST::Item::CodeModule(crate::AST::CodeModule {
                name: alias,
                name_span: crate::Diagnostics::Span::new(0, 0),
                is_pub: false,
                is_package_pub: false,
                body: Some(module.items),
                imports: module.imports,
                web_target: module.web_target_ceiling,
                instance_identity: None,
                span: crate::Diagnostics::Span::new(0, 0),
            }));
    }
    Ok(bundle)
}

fn check_action_generated_sources(
    plan: &crate::Comptime::Build::BuildPlan,
    root: &std::path::Path,
    existing_source_paths: &[std::path::PathBuf],
    span: Option<crate::Diagnostics::Span>,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
) -> Result<Vec<GeneratedSourceProvenance>, Vec<Diagnostic>> {
    let registered = plan
        .generated_modules()
        .iter()
        .map(|module| module.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut out = Vec::new();
    let selected = plan
        .selected_action_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    for action in plan
        .actions()
        .iter()
        .filter(|action| selected.contains(&action.id))
    {
        for output in &action.outputs {
            if !output.as_str().ends_with(".jet") || registered.contains(output.as_str()) {
                continue;
            }
            let path = normalize_project_path(root, std::path::Path::new(output.as_str()));
            if has_symlinked_component(root, &path)
                || std::fs::symlink_metadata(&path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(vec![generated_collision_diag(
                    &action.name,
                    output.as_str(),
                    span,
                )]);
            }
            let source_path = existing_source_paths
                .iter()
                .map(|source| normalize_project_path(root, source))
                .any(|source| source == path);
            if source_path {
                return Err(vec![generated_collision_diag(
                    &action.name,
                    output.as_str(),
                    span,
                )]);
            }
            let source = String::from_utf8(
                read_build_file(root, &path)
                    .map_err(|error| vec![generated_io_diag(&action.name, &error)])?,
            )
            .map_err(|error| {
                vec![generated_io_diag(
                    &action.name,
                    &std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                )]
            })?;
            let mut generated_bundle = crate::Loader::load_entry_with_overlay(
                path.to_str().unwrap_or(output.as_str()),
                None,
                false,
            )
            .map_err(|mut diags| {
                for diag in &mut diags {
                    diag.what = format!("generated action `{}`: {}", action.name, diag.what);
                }
                diags
            })?;
            generated_bundle.build_facts = build_facts.clone();
            generated_bundle.active_os = build_facts.os;
            let generated_diags =
                crate::Sema::check_bundle(&mut generated_bundle, crate::Sema::CompileMode::Check);
            let mut diags = apply_package_lint_policy(&generated_bundle, generated_diags)?;
            diags.retain(|diag| diag.severity == Severity::Error);
            if !diags.is_empty() {
                for diag in &mut diags {
                    diag.what = format!("generated action `{}`: {}", action.name, diag.what);
                }
                return Err(diags);
            }
            out.push(GeneratedSourceProvenance {
                name: action.name.clone(),
                path,
                digest: crate::Comptime::Build::ContentDigest::from_bytes(source.as_bytes()),
            });
        }
    }
    Ok(out)
}

fn validate_selected_action_outputs(
    plan: &crate::Comptime::Build::BuildPlan,
    selected_actions: &std::collections::BTreeSet<crate::Comptime::Build::ActionId>,
    generated: &[&crate::Comptime::Build::BuildGeneratedModule],
    root: &std::path::Path,
    existing_source_paths: &[std::path::PathBuf],
    span: Option<crate::Diagnostics::Span>,
) -> Result<(), Vec<Diagnostic>> {
    let source_paths = existing_source_paths
        .iter()
        .map(|source| normalize_project_path(root, source))
        .collect::<std::collections::BTreeSet<_>>();
    for action in plan.actions().iter().filter(|action| selected_actions.contains(&action.id)) {
        for output in &action.outputs {
            let path = normalize_project_path(root, std::path::Path::new(output.as_str()));
            if has_symlinked_component(root, &path) {
                return Err(vec![Diagnostic::error(
                    "E3505",
                    format!(
                        "build action `{}` cannot write output `{}` through a symlink",
                        action.name,
                        output.as_str()
                    ),
                    "sandboxed build outputs must stay inside the project and must not follow symlinks".to_string(),
                    "remove the symlink or choose a real output directory inside the project".to_string(),
                    None,
                )]);
            }
            if source_paths.contains(&path) {
                return Err(vec![generated_collision_diag(&action.name, output.as_str(), span)]);
            }
            if generated.iter().any(|module| {
                normalize_project_path(root, std::path::Path::new(module.path.as_str())) == path
            }) {
                return Err(vec![generated_cycle_diag(
                    &action.name,
                    output.as_str(),
                    "a selected build action and a generated module both own this path",
                    span,
                )]);
            }
        }
    }
    Ok(())
}

fn valid_build_signature(func: &crate::AST::Func) -> bool {
    if func.params.len() != 1
        || func.params[0].ty
            != crate::AST::Type::Named(crate::Syntax::TYPE_BUILD_CONTEXT.to_string())
    {
        return false;
    }
    matches!(
        func.return_type.as_ref(),
        Some(crate::AST::Type::Result { ok, .. })
            if **ok == crate::AST::Type::Named(crate::Syntax::TYPE_BUILD_PLAN.to_string())
    )
}

fn contains_impure_gate(stmts: &[crate::AST::Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        crate::AST::Stmt::Impure { .. } => true,
        crate::AST::Stmt::While { body, .. }
        | crate::AST::Stmt::For { body, .. }
        | crate::AST::Stmt::Loop { body, .. }
        | crate::AST::Stmt::Reactive { body, .. }
        | crate::AST::Stmt::Shield { body, .. }
        | crate::AST::Stmt::Switched { body, .. }
        | crate::AST::Stmt::Region { body, .. }
        | crate::AST::Stmt::Policy { body, .. }
        | crate::AST::Stmt::TaskGroup { body, .. }
        | crate::AST::Stmt::Layout { body, .. }
        | crate::AST::Stmt::Caps { body, .. }
        | crate::AST::Stmt::Grant { body, .. }
        | crate::AST::Stmt::ComptimeBlock { body, .. }
        | crate::AST::Stmt::Live { body, .. }
        | crate::AST::Stmt::Transact { body, .. }
        | crate::AST::Stmt::Unsafe { body, .. }
        | crate::AST::Stmt::ScopeMember { body, .. } => contains_impure_gate(body),
        crate::AST::Stmt::Switch {
            arms, else_body, ..
        }
        | crate::AST::Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => arms.iter().any(|arm| contains_impure_gate(&arm.body))
            || else_body
                .as_deref()
                .is_some_and(contains_impure_gate),
        crate::AST::Stmt::CountedLoop { step, body, .. } => {
            step.as_deref().is_some_and(|step| contains_impure_gate(std::slice::from_ref(step)))
                || contains_impure_gate(body)
        }
        crate::AST::Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            contains_impure_gate(then_body)
                || else_body
                    .as_deref()
                    .is_some_and(contains_impure_gate)
        }
        crate::AST::Stmt::ContextBlock { body, .. }
        | crate::AST::Stmt::AssumeDet { body, .. } => contains_impure_gate(body),
        _ => false,
    })
}

/// Package source ownership follows the nearest declared dependency root
/// inside the active project boundary. The root package stays last so a
/// dependency directory cannot be swallowed by the project-root prefix.
fn compiler_package_specs(
    bundle: &crate::AST::ProgramBundle,
    root_name: &str,
) -> Vec<crate::Comptime::Build::CompilerPackageSpec> {
    let root = normalize_project_path(&bundle.project_root, &bundle.project_root);
    let mut roots = vec![(root_name.to_string(), root.clone())];
    let mut dependency_roots = bundle
        .dep_roots
        .iter()
        .map(|(name, path)| {
            (
                name.clone(),
                normalize_project_path(&bundle.project_root, path),
            )
        })
        .collect::<Vec<_>>();
    dependency_roots.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    for (name, path) in dependency_roots {
        if path != root && !roots.iter().any(|(_, known)| known == &path) {
            roots.push((name, path));
        }
    }
    roots.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    if let Some(root_index) = roots.iter().position(|(_, path)| path == &root) {
        let root_entry = roots.remove(root_index);
        roots.push(root_entry);
    }

    let module_packages = bundle
        .modules
        .iter()
        .map(|module| {
            let path = normalize_project_path(&bundle.project_root, &module.path);
            roots
                .iter()
                .filter(|(_, package_root)| path.strip_prefix(package_root).is_ok())
                .max_by_key(|(_, package_root)| package_root.components().count())
                .map(|(name, _)| name.clone())
        })
        .collect::<Vec<_>>();

    let mut source_parts = BTreeMap::<String, Vec<(String, String)>>::new();
    for (index, module) in bundle.modules.iter().enumerate() {
        let Some(package_name) = module_packages[index].as_ref() else {
            continue;
        };
        let normalized_path = normalize_project_path(&bundle.project_root, &module.path);
        let package_root = roots
            .iter()
            .find(|(name, _)| name == package_name)
            .map(|(_, path)| path);
        let relative = package_root
            .and_then(|path| normalized_path.strip_prefix(path).ok())
            .unwrap_or(normalized_path.as_path())
            .display()
            .to_string();
        source_parts
            .entry(package_name.clone())
            .or_default()
            .push((relative, module.source.clone()));
    }

    let mut dependencies = roots
        .iter()
        .map(|(name, _)| (name.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let Some(owner) = module_packages[module_index].as_ref() else {
            continue;
        };
        for import in &module.imports {
            let Some(target_index) = bundle.name_ledger.import_target(module_index, import.span)
            else {
                continue;
            };
            let Some(Some(target)) = module_packages.get(target_index) else {
                continue;
            };
            if target != owner {
                dependencies
                    .entry(owner.clone())
                    .or_default()
                    .insert(target.clone());
            }
        }
    }

    roots
        .into_iter()
        .map(|(name, _)| {
            let mut bytes = b"jet.package-source.v1\0".to_vec();
            let mut parts = source_parts.remove(&name).unwrap_or_default();
            parts.sort_by(|left, right| left.0.cmp(&right.0));
            if parts.is_empty() {
                bytes.extend_from_slice(name.as_bytes());
            } else {
                for (path, source) in parts {
                    bytes.extend_from_slice(&(path.len() as u64).to_be_bytes());
                    bytes.extend_from_slice(path.as_bytes());
                    bytes.extend_from_slice(&(source.len() as u64).to_be_bytes());
                    bytes.extend_from_slice(source.as_bytes());
                }
            }
            let package_dependencies = dependencies
                .remove(&name)
                .unwrap_or_default()
                .into_iter()
                .collect();
            crate::Comptime::Build::CompilerPackageSpec::new(
                name,
                crate::Comptime::Build::ContentDigest::from_bytes(&bytes),
                package_dependencies,
            )
        })
        .collect()
}

fn build_package_name(file: &str) -> Result<String, Vec<Diagnostic>> {
    let entry = std::path::Path::new(file);
    let absolute = if entry.is_absolute() {
        entry.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(entry))
            .unwrap_or_else(|_| entry.to_path_buf())
    };
    let parent = absolute.parent().unwrap_or(std::path::Path::new("."));
    let workspace_root = crate::Loader::find_workspace_root_checked(parent)
        .map_err(|diagnostic| vec![diagnostic])?;
    let package_root = crate::Loader::find_manifest_root_checked(parent)
        .map_err(|diagnostic| vec![diagnostic])?
        .filter(|root| {
            workspace_root
                .as_ref()
                .is_none_or(|workspace| crate::Loader::is_physically_within(workspace, root))
        });
    if let Some(root) = package_root {
        let resolver = AuthorityResolver::open(&root)
            .map_err(|error| vec![error.diagnostic()])?;
        let checked = resolver
            .checked_manifest(std::path::Path::new("."))
            .map_err(|error| vec![error.diagnostic()])?;
        let source = checked
            .file
            .text()
            .map_err(|error| vec![error.diagnostic()])?;
        resolver
            .revalidate_file(&checked.file)
            .map_err(|error| vec![error.diagnostic()])?;
        let manifest = crate::Package::PackageFacts::parse(
            &source,
            checked.file.path.display().to_string(),
        )
        .map_err(|error| {
            vec![Diagnostic::error(
                "E1206",
                "invalid package manifest".to_string(),
                error.to_string(),
                "fix the fields in package.jet before loading the project".to_string(),
                None,
            )]
        })?;
        resolver
            .revalidate_file(&checked.file)
            .map_err(|error| vec![error.diagnostic()])?;
        if !manifest.name.is_empty() {
            return Ok(manifest.name);
        }
    }
    if workspace_root.is_some() {
        return crate::Loader::authority_name_for_entry(&absolute)
            .map_err(|diagnostic| vec![diagnostic]);
    }
    Ok(absolute
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("app")
        .to_string())
}

fn package_build_entry_source(
    project_root: &std::path::Path,
) -> Result<Option<(std::path::PathBuf, String)>, Vec<Diagnostic>> {
    let resolver = AuthorityResolver::open(project_root)
        .map_err(|error| vec![error.diagnostic()])?;
    let checked = match resolver.checked_manifest(std::path::Path::new(".")) {
        Ok(checked) => checked,
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(vec![error.diagnostic()]),
    };
    let entry = checked
        .facts
        .resolve_build_entry_checked(&resolver)
        .map_err(|error| vec![build_entry_resolution_diagnostic(&error)])?;
    let Some(entry) = entry else {
        return Ok(None);
    };
    let raw_source = entry
        .text()
        .map_err(|error| vec![error.diagnostic()])?;
    let source = if entry.path.file_name().and_then(|name| name.to_str())
        == Some(crate::Syntax::PACKAGE_FILE)
    {
        crate::Package::build_entry_source(&raw_source).unwrap_or(raw_source)
    } else {
        raw_source
    };
    resolver
        .revalidate_file(&entry)
        .map_err(|error| vec![error.diagnostic()])?;
    resolver
        .revalidate_root()
        .map_err(|error| vec![error.diagnostic()])?;
    Ok(Some((entry.path, source)))
}

fn build_entry_resolution_diagnostic(error: &str) -> Diagnostic {
    let code = if error.contains("two build entries for the package:") {
        "E3520"
    } else {
        "E1334"
    };
    let (why, fix) = if code == "E3520" {
        (
            "one package has exactly one build entry so policy and provenance have one auditable home",
            "keep one `fn build` and remove the other entry",
        )
    } else {
        (
            "package entry discovery uses the checked package authority",
            "repair the package source and try the build again",
        )
    };
    Diagnostic::error(code, error.to_string(), why.to_string(), fix.to_string(), None)
}

fn package_manifest_build_overlay(
    file: &str,
) -> Result<Option<(std::path::PathBuf, String)>, Vec<Diagnostic>> {
    let path = std::path::Path::new(file);
    if path.file_name().and_then(|name| name.to_str()) != Some(crate::Syntax::PACKAGE_FILE) {
        return Ok(None);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| vec![Diagnostic::error(
                "E1334",
                "package manifest path cannot be resolved".to_string(),
                error.to_string(),
                "use an existing package.jet path".to_string(),
                None,
            )])?
    };
    let root = absolute.parent().unwrap_or(std::path::Path::new("."));
    let resolver = AuthorityResolver::open(root)
        .map_err(|error| vec![error.diagnostic()])?;
    let checked = resolver
        .checked_manifest(std::path::Path::new("."))
        .map_err(|error| vec![error.diagnostic()])?;
    if normalize_project_path(root, &absolute)
        != normalize_project_path(root, &checked.file.path)
    {
        return Err(vec![Diagnostic::error(
            "E1334",
            "package manifest path changed during resolution".to_string(),
            "the requested manifest path is not the checked package.jet object".to_string(),
            "use the canonical package.jet path".to_string(),
            None,
        )]);
    }
    let source = checked
        .file
        .text()
        .map_err(|error| vec![error.diagnostic()])?;
    resolver
        .revalidate_file(&checked.file)
        .map_err(|error| vec![error.diagnostic()])?;
    let result = crate::Package::build_entry_source(&source)
        .map(|source| (checked.file.path.clone(), source));
    resolver
        .revalidate_file(&checked.file)
        .map_err(|error| vec![error.diagnostic()])?;
    Ok(result)
}

fn validate_build_authority(
    plan: &crate::Comptime::Build::BuildPlan,
    declared: &std::collections::BTreeSet<crate::Comptime::Build::BuildCapability>,
    has_impure_gate: bool,
    options: &BuildRunOptions,
    dependency_name: Option<String>,
    span: crate::Diagnostics::Span,
) -> Result<(), Vec<Diagnostic>> {
    let selected = plan
        .selected_action_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    let mut required = plan
        .actions()
        .iter()
        .filter(|action| selected.contains(&action.id))
        .flat_map(|action| action.caps.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    if !plan
        .selected_probe_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?
        .is_empty()
    {
        required.insert(crate::Comptime::Build::BuildCapability::Exec);
    }
    for effect in required {
        if !declared.contains(&effect) {
            return Err(vec![Diagnostic::error(
                "E3503",
                format!("this build uses `{}` without declaring it on `fn build`", effect.name()),
                "the build signature is the static authority manifest audited before build code executes".to_string(),
                format!("add `{}` to the build function's `=[...]=>` effect row", effect.name()),
                Some(span),
            )]);
        }
        if !has_impure_gate {
            return Err(vec![Diagnostic::error(
                "E3502",
                format!("build authority `{}` must be used inside `#Impure(\"reason\")`", effect.name()),
                "ambient build effects need an audited source gate as well as a policy grant".to_string(),
                "wrap the action or probe declarations in `#Impure(\"why this build needs ambient authority\")`".to_string(),
                Some(span),
            )]);
        }
        if !options.inspect_only && (!options.gates.allows(crate::Policy::PolicyKey::Impure) || !effective_grants(options, plan).contains(&effect)) {
            if let Some(dependency_name) = dependency_name.as_deref() {
                return Err(vec![Diagnostic::error(
                    "E3504",
                    format!(
                        "dependency `{dependency_name}` build asks for `{}` not granted by the root",
                        effect.name()
                    ),
                    "dependency build code never gets ambient authority unless the workspace names it".to_string(),
                    format!("grant `{}` explicitly in workspace policy, or remove the dependency action", effect.name()),
                    Some(span),
                )]);
            }
            return Err(vec![Diagnostic::error(
                "E3503",
                format!("this build asks for `{}`, which effective policy has not granted", effect.name()),
                "a source declaration and `#Impure` gate do not widen CLI, package, or workspace policy".to_string(),
                format!("pass `--allow-{}` or grant it in package/workspace build policy", effect.flag()),
                Some(span),
            )]);
        }
    }
    Ok(())
}

fn validate_legacy_project_imports(
    plan: &crate::Comptime::Build::BuildPlan,
    project_root: &std::path::Path,
    span: crate::Diagnostics::Span,
) -> Result<(), Vec<Diagnostic>> {
    let selected = plan
        .selected_action_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    for action in plan
        .actions()
        .iter()
        .filter(|action| selected.contains(&action.id))
    {
        let import_marker = action.labels.get("legacy.import");
        let Some(project_file) = action.labels.get("legacy.project-file") else {
            if import_marker.is_some() {
                return Err(vec![Diagnostic::error(
                    "E3502",
                    "legacy project-file import is missing its canonical path".to_string(),
                    "a project-file import has one reserved marker and one canonical path"
                        .to_string(),
                    "pass the wrapper's canonical project file as the final import argument"
                        .to_string(),
                    Some(span),
                )]);
            }
            continue;
        };
        if import_marker.map(String::as_str) != Some("project-file") {
            return Err(vec![Diagnostic::error(
                "E3502",
                format!("legacy project file `{project_file}` has no canonical import marker"),
                "only the typed legacy project-file importer may attach this path to an action"
                    .to_string(),
                "declare the project file through the legacy wrapper import".to_string(),
                Some(span),
            )]);
        }
        let Some(kind) = action.legacy_wrapper else {
            return Err(vec![Diagnostic::error(
                "E3502",
                format!("legacy import `{}` is not attached to a legacy wrapper", project_file),
                "a project-file import must be owned by the typed legacy wrapper that reads it".to_string(),
                "declare the project file through the legacy wrapper import".to_string(),
                Some(span),
            )]);
        };
        if project_file != kind.project_file()
            || crate::Comptime::Build::BuildPath::new(project_file.clone()).is_err()
            || !action.inputs.iter().any(|input| input.as_str() == project_file)
        {
            return Err(vec![Diagnostic::error(
                "E3502",
                format!("legacy import `{}` is not the canonical project file for {}", project_file, kind.as_str()),
                "an imported legacy project file must be canonical, relative, and part of the typed action identity".to_string(),
                format!("use `{}` through the legacy wrapper import", kind.project_file()),
                Some(span),
            )]);
        }
        if let Err(error) = crate::Comptime::Build::LegacyWrapperSpec::read_legacy_project_file(
            project_root,
            kind,
        ) {
            let (what, fix) = match error {
                crate::Comptime::Build::BuildError::LegacyProjectFileMissing(_) => (
                    format!("legacy project file `{project_file}` is missing"),
                    "restore the declared project file or remove the import".to_string(),
                ),
                crate::Comptime::Build::BuildError::LegacyProjectFileInvalid(path) => (
                    format!("legacy project file `{path}` is not a bounded UTF-8 regular file"),
                    "declare a regular UTF-8 project file no larger than 1 MiB inside the project root".to_string(),
                ),
                other => (
                    format!("legacy project file `{project_file}` could not be imported: {other:?}"),
                    "restore the declared project file or remove the import".to_string(),
                ),
            };
            return Err(vec![Diagnostic::error(
                "E3502",
                what,
                "legacy graph import rejects links and directories before a wrapper can run".to_string(),
                fix,
                Some(span),
            )]);
        }
    }
    Ok(())
}

/// D-BUILDCTX-FLAGS1=A: CLI grants ∪ `fn build` default_allow (CLI cannot remove defaults by omission).
fn effective_grants(
    options: &BuildRunOptions,
    plan: &crate::Comptime::Build::BuildPlan,
) -> std::collections::BTreeSet<crate::Comptime::Build::BuildCapability> {
    let mut grants = options.grants.clone();
    for name in &plan.default_allows {
        if let Some(cap) = crate::Comptime::Build::BuildCapability::parse(name) {
            grants.insert(cap);
        }
    }
    grants
}

pub fn program_semantic_facts(
    bundle: &crate::AST::ProgramBundle,
    checked: &crate::Sema::SemIndexEffectFacts,
) -> crate::Comptime::ProgramSemanticFacts {
    let mut effects = std::collections::HashMap::new();
    let reaches_panic = checked.reachability.nodes_with("panic", "panic");
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            let crate::AST::Item::Func(func) = item else {
                continue;
            };
            let qualified = checked
                .name_ledger
                .semantic_identity(module_idx, &func.name)
                .unwrap_or_else(|| format!("{}::{}", module.alias, func.name));
            let values = checked
                .solved
                .get(&qualified)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            effects.insert(qualified.clone(), values);
        }
    }
    crate::Comptime::ProgramSemanticFacts {
        effects,
        reaches_panic,
        fact_registry: checked.fact_registry.clone(),
        name_ledger: checked.name_ledger.clone(),
    }
}

fn bad_build_signature(span: crate::Diagnostics::Span) -> Diagnostic {
    Diagnostic::error(
        "E3501",
        "`fn build` must take one `BuildContext` and return `BuildPlan ?`".to_string(),
        "the build entry is a typed contract: its parameter is its authority and its result is the graph Jet executes".to_string(),
        "write `fn build(b: BuildContext) => BuildPlan ?`".to_string(),
        Some(span),
    )
}

fn build_function_location(module: &crate::AST::LoadedModule, index: usize) -> String {
    let line = match module.items.get(index) {
        Some(crate::AST::Item::Func(function)) => source_line_at(&module.source, function.name_span.start),
        _ => 1,
    };
    format!("{}:{line}", module.path.display())
}

fn duplicate_build_entries(
    bundle: &crate::AST::ProgramBundle,
    indices: &[usize],
) -> Diagnostic {
    let module = &bundle.modules[bundle.entry];
    let first = build_function_location(module, indices[0]);
    let second = build_function_location(module, indices[1]);
    let span = match module.items.get(indices[1]) {
        Some(crate::AST::Item::Func(function)) => Some(function.name_span),
        _ => None,
    };
    build_entry_conflict("the entry unit", &first, &second, span)
}

fn build_source_location(path: &std::path::Path, source: &str) -> String {
    let offset = source.find("fn build").unwrap_or(0);
    format!("{}:{}", path.display(), source_line_at(source, offset))
}

fn source_line_at(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset.min(source.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn build_entry_conflict(
    unit: &str,
    first_location: &str,
    second_location: &str,
    span: Option<crate::Diagnostics::Span>,
) -> Diagnostic {
    Diagnostic::error(
        "E3520",
        format!(
            "two build entries for {unit}: {first_location} and {second_location}"
        ),
        "one package has exactly one build entry so policy and provenance have one auditable home"
            .to_string(),
        format!("keep the `fn build` in {first_location} and remove the entry in {second_location}"),
        span,
    )
}

fn materialize_and_check_generated(
    modules: &[&crate::Comptime::Build::BuildGeneratedModule],
    root: &std::path::Path,
    existing_source_paths: &[std::path::PathBuf],
    action_outputs: &[String],
    span: Option<crate::Diagnostics::Span>,
    build_facts: &jet_foundation::Facts::BuildFactSnapshot,
) -> Result<Vec<GeneratedSourceProvenance>, Vec<Diagnostic>> {
    let mut modules = modules.to_vec();
    modules.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()).then_with(|| left.name.cmp(&right.name)));
    let lock = crate::Lock::load(root);
    let existing_source_paths = existing_source_paths
        .iter()
        .map(|path| normalize_project_path(root, path))
        .collect::<Vec<_>>();
    let action_outputs = action_outputs
        .iter()
        .map(|path| normalize_project_path(root, std::path::Path::new(path)))
        .collect::<Vec<_>>();

    // Preflight every ownership decision before touching the tree. A failed
    // later module must not leave an earlier generated file visible to the
    // loader or to a concurrent observer.
    let mut planned_paths = std::collections::BTreeSet::new();
    for module in &modules {
        let path = normalize_project_path(root, std::path::Path::new(module.path.as_str()));
        if !planned_paths.insert(path.clone()) {
            return Err(vec![generated_cycle_diag(
                &module.name,
                module.path.as_str(),
                "two generated modules claim the same managed path",
                span,
            )]);
        }
        if action_outputs.iter().any(|output| output == &path) {
            return Err(vec![generated_cycle_diag(
                &module.name,
                module.path.as_str(),
                "a selected build action and a generated module both own this path",
                span,
            )]);
        }
        if has_symlinked_component(root, &path) {
            return Err(vec![generated_collision_diag(&module.name, module.path.as_str(), span)]);
        }
        let managed = managed_generated_file(lock.as_ref(), root, &path, &module.source_digest);
        if existing_source_paths.iter().any(|source| source == &path) && !managed {
            return Err(vec![generated_collision_diag(&module.name, module.path.as_str(), span)]);
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || (!metadata.is_file() && !managed) {
                return Err(vec![generated_collision_diag(&module.name, module.path.as_str(), span)]);
            }
            if metadata.is_file() && !managed {
                return Err(vec![generated_collision_diag(&module.name, module.path.as_str(), span)]);
            }
        }
    }

    let path_indices = modules
        .iter()
        .enumerate()
        .map(|(index, module)| {
            (
                normalize_project_path(root, std::path::Path::new(module.path.as_str())),
                index,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let dependencies = modules
        .iter()
        .map(|module| {
            generated_dependencies(
                module,
                &normalize_project_path(root, std::path::Path::new(module.path.as_str())),
                root,
                &path_indices,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rounds = generated_rounds(&dependencies, &modules, span)?;

    let mut provenance = Vec::new();
    for round in rounds {
        for index in round {
            let module = modules[index];
            let path = normalize_project_path(root, std::path::Path::new(module.path.as_str()));
            safe_atomic_write(&path, module.source.as_bytes())
                .map_err(|error| vec![generated_io_diag(&module.name, &error)])?;
            let mut generated_bundle = crate::Loader::load_entry_with_overlay(
                path.to_str().unwrap_or(module.path.as_str()),
                None,
                false,
            )
            .map_err(|mut diags| {
                for diag in &mut diags {
                    diag.what = format!("generated module `{}`: {}", module.name, diag.what);
                }
                diags
            })?;
            generated_bundle.build_facts = build_facts.clone();
            generated_bundle.active_os = build_facts.os;
            let generated_diags =
                crate::Sema::check_bundle(&mut generated_bundle, crate::Sema::CompileMode::Check);
            let mut diags = apply_package_lint_policy(&generated_bundle, generated_diags)?;
            diags.retain(|diag| diag.severity == Severity::Error);
            if !diags.is_empty() {
                for diag in &mut diags {
                    diag.what = format!("generated module `{}`: {}", module.name, diag.what);
                }
                return Err(diags);
            }
            provenance.push(GeneratedSourceProvenance {
                name: module.name.clone(),
                path,
                digest: module.source_digest.clone(),
            });
        }
    }
    Ok(provenance)
}

/// Build the dependency edges used by bounded generated-source staging. A
/// generated file can observe an earlier file through the ordinary quoted-file
/// import; no generation-only syntax or second semantic mechanism is needed.
fn generated_dependencies(
    module: &crate::Comptime::Build::BuildGeneratedModule,
    path: &std::path::Path,
    root: &std::path::Path,
    paths: &std::collections::BTreeMap<std::path::PathBuf, usize>,
) -> Result<Vec<usize>, Vec<Diagnostic>> {
    let (tokens, lex_diags) = crate::Lexer::lex(&module.source);
    if !lex_diags.is_empty() {
        return Err(annotate_generated_frontend_diags(&module.name, lex_diags));
    }
    let program = crate::Parser::parse(&tokens)
        .map_err(|diags| annotate_generated_frontend_diags(&module.name, diags))?;
    let mut dependencies = std::collections::BTreeSet::new();
    for import in program.imports {
        let crate::AST::ImportKind::File(import_path, _) = import.kind else {
            continue;
        };
        let Some(candidate) = generated_import_path(path, root, &import_path) else {
            continue;
        };
        if let Some(index) = paths.get(&candidate) {
            dependencies.insert(*index);
        }
    }
    Ok(dependencies.into_iter().collect())
}

fn generated_import_path(
    path: &std::path::Path,
    root: &std::path::Path,
    import_path: &str,
) -> Option<std::path::PathBuf> {
    if import_path.trim().is_empty() || import_path.starts_with('/') {
        return None;
    }
    let mut candidate = path.parent()?.to_path_buf();
    for part in import_path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        candidate.push(part);
    }
    candidate.set_extension(crate::Syntax::FILE_EXT);
    Some(normalize_project_path(root, &candidate))
}

fn annotate_generated_frontend_diags(
    name: &str,
    mut diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    for diagnostic in &mut diagnostics {
        diagnostic.what = format!("generated module `{name}`: {}", diagnostic.what);
    }
    diagnostics
}

/// Return dependency layers in stable path/name order. A layer is materialized
/// only after every module it imports has passed the front end, so a later
/// layer can observe an earlier layer while the total number of rounds remains
/// bounded by the number of generated modules.
fn generated_rounds(
    dependencies: &[Vec<usize>],
    modules: &[&crate::Comptime::Build::BuildGeneratedModule],
    span: Option<crate::Diagnostics::Span>,
) -> Result<Vec<Vec<usize>>, Vec<Diagnostic>> {
    let mut dependents = vec![Vec::<usize>::new(); modules.len()];
    let mut remaining = dependencies.iter().map(Vec::len).collect::<Vec<_>>();
    for (module, deps) in dependencies.iter().enumerate() {
        for dependency in deps {
            dependents[*dependency].push(module);
        }
    }
    for dependents in &mut dependents {
        dependents.sort_unstable();
    }

    let mut ready = std::collections::BTreeSet::new();
    for (index, count) in remaining.iter().enumerate() {
        if *count == 0 {
            ready.insert(index);
        }
    }
    let mut rounds = Vec::new();
    let mut emitted = 0;
    while !ready.is_empty() {
        let current = ready.iter().copied().collect::<Vec<_>>();
        ready.clear();
        for module in &current {
            emitted += 1;
            for dependent in &dependents[*module] {
                remaining[*dependent] -= 1;
                if remaining[*dependent] == 0 {
                    ready.insert(*dependent);
                }
            }
        }
        rounds.push(current);
    }
    if emitted == modules.len() {
        return Ok(rounds);
    }

    let mut state = vec![0u8; modules.len()];
    let mut stack = Vec::new();
    let cycle = (0..modules.len())
        .find_map(|index| generated_cycle_chain(index, dependencies, &mut state, &mut stack))
        .unwrap_or_else(|| vec![0]);
    let first = cycle[0];
    let chain = cycle
        .iter()
        .map(|index| modules[*index].name.as_str())
        .collect::<Vec<_>>()
        .join(" -> ");
    Err(vec![generated_cycle_diag(
        &modules[first].name,
        modules[first].path.as_str(),
        &format!("dependency chain `{chain}` is cyclic"),
        span,
    )])
}

fn generated_cycle_chain(
    index: usize,
    dependencies: &[Vec<usize>],
    state: &mut [u8],
    stack: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    if state[index] == 1 {
        let start = stack.iter().position(|item| *item == index).unwrap_or(0);
        let mut cycle = stack[start..].to_vec();
        cycle.push(index);
        return Some(cycle);
    }
    if state[index] == 2 {
        return None;
    }
    state[index] = 1;
    stack.push(index);
    for dependency in &dependencies[index] {
        if let Some(cycle) = generated_cycle_chain(*dependency, dependencies, state, stack) {
            return Some(cycle);
        }
    }
    stack.pop();
    state[index] = 2;
    None
}

fn normalize_project_path(root: &std::path::Path, path: &std::path::Path) -> std::path::PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn has_symlinked_component(root: &std::path::Path, path: &std::path::Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return true;
        };
        current.push(part);
        if let Ok(metadata) = std::fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return true;
            }
        }
    }
    false
}

fn managed_generated_file(
    lock: Option<&crate::Lock::LockFile>,
    root: &std::path::Path,
    path: &std::path::Path,
    digest: &crate::Comptime::Build::ContentDigest,
) -> bool {
    let Some(lock) = lock else {
        return false;
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    lock.comptime_inputs
        .iter()
        .any(|input| input.path == relative && input.hash == digest.as_str())
}

fn generated_collision_diag(
    name: &str,
    path: &str,
    span: Option<crate::Diagnostics::Span>,
) -> Diagnostic {
    Diagnostic::error(
        "E3510",
        format!("`b.generate(\"{name}\")` would shadow the module at `{path}`"),
        "generation is additive: what you wrote is always what compiles".to_string(),
        "rename the generated module, or delete the hand-written one".to_string(),
        span,
    )
}

fn generated_cycle_diag(
    name: &str,
    path: &str,
    reason: &str,
    span: Option<crate::Diagnostics::Span>,
) -> Diagnostic {
    Diagnostic::error(
        "E3511",
        format!("generation rounds form a cycle: `{name}` at `{path}`"),
        "generated source must reach a bounded deterministic order, not loop until quiescent".to_string(),
        format!("{reason}; break the dependency between these generators"),
        span,
    )
}

fn generated_io_diag(name: &str, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E3502",
        format!("generated module `{name}` could not be materialized"),
        format!("generated source must be a real file before it re-enters the front end: {error}"),
        "make sure `.jet/generated` is writable and the generated module name is unique"
            .to_string(),
        None,
    )
}

fn build_plan_diagnostic(error: &crate::Comptime::Build::BuildError) -> Diagnostic {
    Diagnostic::error(
        "E3502",
        format!("build plan is invalid: {error:?}"),
        "all graph handles must belong to one selected root build and every action output must have one owner".to_string(),
        "fix the named graph node and use `jet inspect explain-build` to inspect its inputs".to_string(),
        None,
    )
}

fn build_execution_diagnostic(error: crate::Comptime::Build::BuildExecutionError) -> Diagnostic {
    use crate::Comptime::Build::BuildExecutionError;
    match error {
        BuildExecutionError::MissingGrant { action, capability } => Diagnostic::error(
            "E3504",
            format!("build action `{action}` asks for ungranted `{capability:?}` authority"),
            "declaring a capability in `fn build` does not grant it; root policy must approve each ambient effect".to_string(),
            format!("pass `--allow-{}` for this run, or grant it in package/workspace policy", capability.flag()),
            None,
        ),
        BuildExecutionError::ActionFailed { action, exit_code, stderr } => Diagnostic::error(
            "E3505",
            format!("build action `{action}` exited with status {exit_code}"),
            if stderr.is_empty() {
                "the declared command failed inside the build sandbox without writing stderr".to_string()
            } else {
                format!("the sandboxed command reported: {stderr}")
            },
            "fix the action command, declared inputs/outputs, toolchain, or probe, then rerun `jet build`".to_string(),
            None,
        ),
        BuildExecutionError::ProbeFailed { probe, detail } => Diagnostic::error(
            "E3505",
            format!("build probe `{probe}` failed"),
            detail,
            "fix the typed probe or select a toolchain that provides it".to_string(),
            None,
        ),
        BuildExecutionError::SandboxUnavailable => Diagnostic::error(
            "E3505",
            "build sandbox is unavailable".to_string(),
            "Jet refuses to run typed build actions without the required bubblewrap isolation".to_string(),
            "install bubblewrap or run on a supported build worker; there is no ambient fallback".to_string(),
            None,
        ),
        BuildExecutionError::IO { action, detail } => Diagnostic::error(
            "E3505",
            format!("build action `{action}` could not access a declared build path"),
            detail,
            "fix the declared input/output path or make the project build directory writable".to_string(),
            None,
        ),
        BuildExecutionError::InvalidGraph(error) => build_plan_diagnostic(&error),
    }
}

/// The real implementation behind every `compile_bundle_path_opts*` facade —
/// see `compile_bundle_path_opts` (native) / `compile_bundle_path_opts_dbg`
/// (native debug) / `compile_bundle_path_opts_plugin` (c81 plugin guest) for
/// the public entry points.
fn compile_bundle_path_opts_full(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    gates: crate::Policy::GateSet,
    web_target: bool,
    plugin_target: bool,
    library_target: bool,
    debug_linemap: bool,
    cross_target: Option<&str>,
    explicit_output: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    // D-OSTARGET1=A: resolve the active native OS bucket once, from the same
    // `--target=<triple>` flag E2-M15 already threads through (host OS when
    // absent or unrecognized, e.g. a wasm/web pseudo-target).
    let active_os = crate::Syntax::OSTarget::active(cross_target);
    let timing = crate::PhaseTiming::enabled();
    let mut timer = crate::PhaseTiming::PhaseTimer::new();
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    // D-OSTARGET2=B: the `@if @build.os == { … }` desugar (run in sema)
    // must fold to the same OS bucket codegen filters `impl`s by, so seed the
    // bundle from the same resolved `active_os` as `emit_bundle`.
    bundle.active_os = active_os;
    seed_build_facts(&mut bundle, profile, false, setting_overrides)?;
    if web_target {
        bundle.web_partition_enforced = true;
    }
    if timing {
        timer.lap("load"); // lex + parse + module resolution
    }
    let diags = if let Some(output) = explicit_output {
        crate::Sema::check_bundle_for_output_opts(
            &mut bundle,
            mode,
            output,
            freestanding,
            gates,
        )
    } else if freestanding {
        crate::Sema::check_bundle_freestanding(&mut bundle, mode)
    } else if !gates.is_empty() {
        crate::Sema::check_bundle_gates(&mut bundle, mode, gates)
    } else {
        crate::Sema::check_bundle(&mut bundle, mode)
    };
    if timing {
        timer.lap("sema");
    }
    let extension_diags =
        crate::CompilerExtensionHook::post_sema_diagnostics(&bundle, None, &diags);
    // Freestanding / impure / output / default compile variants here do not
    // surface `SemIndexEffectFacts`. Pass `None` so the hook omits
    // `ReadEffects` honestly — never invent placeholders (D-DX5-HOOK1).
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let lints = gate_diagnostics(
        &bundle,
        parse_teaching,
        diags,
        extension_diags,
    )?;
    let ffi_result = match cross_target {
        Some(target) => crate::FFI::prepare_for_target(&bundle, target),
        None => crate::FFI::prepare(&bundle),
    };
    let ffi = match ffi_result {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    if timing {
        timer.lap("ffi");
    }
    if web_target {
        let web_tir_errors: Vec<_> =
            crate::Codegen::validate_web_tir_support(&bundle, ffi.as_ref())
                .into_iter()
                .map(|miss| {
                    Diagnostic::error(
                        "E-WEB-TIR-UNSUPPORTED",
                        format!("web output cannot compile `{}` yet", miss.func_name),
                        "web builds use the same checked executable body path as native builds; this function uses a construct the web output cannot lower today".to_string(),
                        "move the unsupported work behind a Wasm export that uses covered Jet constructs, or simplify this function for the web target".to_string(),
                        Some(miss.span),
                    )
                })
                .collect();
        if !web_tir_errors.is_empty() {
            return Err(web_tir_errors);
        }
    }
    let rust = crate::Codegen::emit_bundle_dbg(&bundle, ffi.as_ref(), debug_linemap, active_os);
    let web = if web_target {
        Some(
            crate::Codegen::emit_web(&bundle, mode, ffi.as_ref()).map_err(|miss| {
                vec![Diagnostic::error(
                    "E-WEB-TIR-UNSUPPORTED",
                    format!("web output cannot compile `{}` yet", miss.func_name),
                    "web emitter capability facts drifted after validation".to_string(),
                    "report this compiler bug with the named function".to_string(),
                    Some(miss.span),
                )]
            })?,
        )
    } else {
        None
    };
    // D-PLUGIN1=B / D-DEP-WASM1=A / D-PLUGIN-EXPORT1=A (c81): the guest side of
    // a `target: sandbox` build — a `.wit` world + wasm32 guest Rust, generated
    // from the entry module's exportable (`Int`/`Float`-only) `pub fn`s.
    let plugin = if plugin_target {
        // E1260: every `pub fn` in the entry module must be exportable —
        // never a silent skip (I3/I4).
        let surface_errors = crate::PluginExport::validate_export_surface(&bundle);
        if !surface_errors.is_empty() {
            return Err(surface_errors);
        }
        let export_name = crate::PluginExport::resolve_export_name(&bundle);
        // D-PLUGIN-VERSION1=A: freeze/diff the exported interface (E1257 on an
        // incompatible change) before handing artifacts to the wasm build step.
        crate::PluginExport::check_and_freeze_version(&bundle, &export_name)?;
        Some(crate::Codegen::emit_plugin(&bundle, &rust, &export_name))
    } else {
        None
    };
    let library_config = if library_target {
        if web_target || plugin_target {
            return Err(vec![Diagnostic::error(
                "E1341",
                "a Library output cannot also select a backend target".to_string(),
                "Library artifacts are native static/shared projections (D-LIB-EXPORT1=C), not web or sandbox guest outputs".to_string(),
                "remove `--target=web`/`--target=sandbox` and select the Library output directly".to_string(),
                None,
            )]);
        }
        let config = crate::LibraryExport::resolve_config(&bundle, explicit_output)?;
        let surface_errors = crate::LibraryExport::validate_export_surface(&bundle);
        if !surface_errors.is_empty() {
            return Err(surface_errors);
        }
        crate::LibraryExport::check_and_freeze_version(&bundle, &config.name)?;
        Some(config)
    } else {
        None
    };
    let library = library_config.as_ref().map(|config| {
        crate::Codegen::emit_library(&bundle, &rust, &config.name, &config.bindings)
    });
    if timing {
        timer.lap("codegen");
        timer.metric("rust_bytes", rust.len() as u128);
        timer.write_to(&bundle.project_root);
    }
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `#Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    let resolver = match AuthorityResolver::open(&bundle.project_root) {
        Ok(resolver) => Some(resolver),
        Err(error) if error.is_missing() => None,
        Err(error) => return Err(vec![error.diagnostic()]),
    };
    if let Some(resolver) = resolver {
        let checked = match resolver.checked_manifest(std::path::Path::new(".")) {
            Ok(checked) => Some(checked),
            Err(error) if error.is_missing() => None,
            Err(error) => return Err(vec![error.diagnostic()]),
        };
        if let Some(checked) = checked {
            let source = checked
                .file
                .text()
                .map_err(|error| vec![error.diagnostic()])?;
            let manifest = crate::Manifest::parse(&checked.file.path, &source)
                .map_err(|diagnostic| vec![diagnostic])?;
            resolver
                .revalidate_file(&checked.file)
                .map_err(|error| vec![error.diagnostic()])?;
            crate::Lock::record_inferred_layer(
                &bundle.project_root,
                &manifest.package.name,
                bundle.inferred_layer,
            );
        }
    }
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        // Native C link flags are resolved separately at build time (so that
        // codegen / front-end checks never depend on system link discovery);
        // see `resolve_c_links`.
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin,
        library,
        library_config,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileSrcOptions {
    pub web_target: bool,
}

/// In-memory pipeline: lex → parse → bundle → sema → ffi → codegen.
pub fn compile_src(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_src_with_options(src, file, mode, CompileSrcOptions::default())
}

/// Compile source wholly synthesized by the compiler or one of its tools.
/// Callers embedding user text must validate that text with `Lexer::lex` first.
pub fn compile_generated_src(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_src_with_options_and_policy(src, file, mode, CompileSrcOptions::default(), true)
}

pub fn compile_src_with_options(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
    options: CompileSrcOptions,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_src_with_options_and_policy(src, file, mode, options, false)
}

fn compile_src_with_options_and_policy(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
    options: CompileSrcOptions,
    generated: bool,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    crate::boot_tir_eval();
    let (toks, lex_diags) = if generated {
        crate::Lexer::lex_generated(src)
    } else {
        crate::Lexer::lex(src)
    };
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = crate::Parser::parse(&toks)?;
    let mut bundle = crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        modules: vec![crate::AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            script_body: std::mem::take(&mut prog.script_body),
            block_spans: std::mem::take(&mut prog.block_spans),
            source: src.to_string(),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            default_target: prog.default_target,
            html_path: prog.html_path.clone(),
            no_alloc_policy: prog.no_alloc_policy,
            policy_declarations: prog.policy_declarations.clone(),
            rule_facts: std::mem::take(&mut prog.rule_facts),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        ffi_callback_fns: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger: crate::AST::NameLedger::default(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: options.web_target,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        active_os: crate::Syntax::OSTarget::host(),
        build_facts: jet_foundation::Facts::BuildFactSnapshot::script(
            std::path::Path::new(file),
            crate::Syntax::OSTarget::host(),
            "dev",
        ),
        edition: crate::Manifest::latest_edition().to_string(),
    };
    // Active foreign caches may contribute generated C-ABI bridge modules.
    if let Err(diags) = crate::Foreign::assemble_active_namespaces(&mut bundle) {
        return Err(diags);
    }
    bundle.cffi = match crate::CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return Err(diags),
    };
    bundle.materialize_script_entries();
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in diags {
        match d.severity {
            Severity::Error => errors.push(d),
            Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    if options.web_target {
        let web_tir_errors: Vec<_> =
            crate::Codegen::validate_web_tir_support(&bundle, ffi.as_ref())
                .into_iter()
                .map(|miss| {
                    Diagnostic::error(
                        "E-WEB-TIR-UNSUPPORTED",
                        format!("web output cannot compile `{}` yet", miss.func_name),
                        "web builds use the same checked executable body path as native builds; this function uses a construct the web output cannot lower today".to_string(),
                        "move the unsupported work behind a Wasm export that uses covered Jet constructs, or simplify this function for the web target".to_string(),
                        Some(miss.span),
                    )
                })
                .collect();
        if !web_tir_errors.is_empty() {
            return Err(web_tir_errors);
        }
    }
    let rust = crate::Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    let web = if options.web_target {
        Some(crate::Codegen::emit_web(&bundle, mode, ffi.as_ref()).map_err(|miss| {
            vec![Diagnostic::error(
                "E-WEB-TIR-UNSUPPORTED",
                format!("web output cannot compile `{}` yet", miss.func_name),
                "web emitter capability facts drifted after validation".to_string(),
                "report this compiler bug with the named function".to_string(),
                Some(miss.span),
            )]
        })?)
    } else {
        None
    };
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `#Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin: None,
        library: None,
        library_config: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

/// Check-only from file (+ optional in-memory overlay).
///
/// The `overlay` pair is `(canonical_path, text)` — the same shape
/// `Loader::load_entry_with_overlay` expects. Pass `None` for a plain
/// on-disk check; pass `Some((&abs, text))` for an LSP unsaved-buffer check.
/// `is_lsp` is forwarded as the `for_check` flag to the loader.
pub fn check_file(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
) -> (Vec<Diagnostic>, Option<crate::AST::ProgramBundle>) {
    let (diags, bundle, _facts) = check_file_with_effect_facts(file, overlay, is_lsp);
    (diags, bundle)
}

/// Like `check_file` but also returns effect facts for D-SEMINDEX1.
pub fn check_file_with_effect_facts(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    check_file_with_effect_facts_and_settings(file, overlay, is_lsp, &BTreeMap::new())
}

pub fn check_file_with_effect_facts_and_settings(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
    setting_overrides: &BTreeMap<String, String>,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    let overlays = overlay.into_iter().collect::<Vec<_>>();
    let (diagnostics, bundle, facts, _) =
        check_file_with_effect_facts_impl(file, &overlays, is_lsp, None, "dev", setting_overrides);
    (diagnostics, bundle, facts)
}

/// Like `check_file_with_effect_facts`, with an explicitly selected build
/// profile for tooling that must explain profile-dependent specialization.
pub fn check_file_with_effect_facts_profile(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
    profile: &str,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    let overlays = overlay.into_iter().collect::<Vec<_>>();
    let (diagnostics, bundle, facts, _) =
        check_file_with_effect_facts_impl(file, &overlays, is_lsp, None, profile, &BTreeMap::new());
    (diagnostics, bundle, facts)
}

pub fn check_file_with_effect_facts_incremental(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
    cache: &mut crate::Sema::IncrementalSemaCache,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    let overlays = overlay.into_iter().collect::<Vec<_>>();
    let (diagnostics, bundle, facts, _) =
        check_file_with_effect_facts_impl(file, &overlays, is_lsp, Some(cache), "dev", &BTreeMap::new());
    (diagnostics, bundle, facts)
}

pub fn check_file_with_effect_facts_incremental_overlays(
    file: &str,
    overlays: &[(&Path, &str)],
    is_lsp: bool,
    cache: &mut crate::Sema::IncrementalSemaCache,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
    Vec<std::path::PathBuf>,
) {
    check_file_with_effect_facts_impl(file, overlays, is_lsp, Some(cache), "dev", &BTreeMap::new())
}

fn check_file_with_effect_facts_impl(
    file: &str,
    overlays: &[(&Path, &str)],
    is_lsp: bool,
    incremental: Option<&mut crate::Sema::IncrementalSemaCache>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
    Vec<std::path::PathBuf>,
) {
    let (loaded, dependencies) =
        crate::Loader::load_entry_with_overlays_and_dependencies(file, overlays, is_lsp);
    match loaded {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            if let Err(fact_diags) = seed_build_facts(
                &mut bundle,
                profile,
                false,
                setting_overrides,
            ) {
                diags.extend(fact_diags);
                return (
                    diags,
                    None,
                    crate::Sema::SemIndexEffectFacts::default(),
                    dependencies,
                );
            }
            let (check_diags, facts) = match incremental {
                Some(cache) => crate::Sema::check_bundle_with_effect_facts_incremental(
                    &mut bundle,
                    crate::Sema::CompileMode::Check,
                    cache,
                ),
                None => crate::Sema::check_bundle_with_effect_facts(
                    &mut bundle,
                    crate::Sema::CompileMode::Check,
                ),
            };
            diags.extend(check_diags);
            if let Some(crate::AST::Item::Func(build)) = bundle.modules[bundle.entry]
                .items
                .iter()
                .find(|item| matches!(item, crate::AST::Item::Func(func) if func.name == "build"))
            {
                if !valid_build_signature(build) {
                    diags.push(bad_build_signature(build.name_span));
                }
            }
            let extension_diags = crate::CompilerExtensionHook::post_sema_diagnostics(
                &bundle,
                Some(&facts),
                &diags,
            );
            diags.extend(extension_diags);
            match apply_package_lint_policy(&bundle, diags) {
                Ok(diags) => (diags, Some(bundle), facts, dependencies),
                Err(diags) => (diags, None, facts, dependencies),
            }
        }
        Err(diags) => (
            diags,
            None,
            crate::Sema::SemIndexEffectFacts::default(),
            dependencies,
        ),
    }
}

/// Check a staged multi-file tree. This is the authoritative compiler path;
/// callers do not need to mirror parser, loader, or sema behavior.
pub fn check_file_with_overlays(
    file: &str,
    overlays: &[(&Path, &str)],
    is_lsp: bool,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    let (diagnostics, bundle, facts, _) =
        check_file_with_effect_facts_impl(file, overlays, is_lsp, None, "dev", &BTreeMap::new());
    (diagnostics, bundle, facts)
}

/// Structural tools check a staged file in its actual output directory and
/// load adjacent modules referenced by unqualified imports. This retains the
/// same parser/sema authority as ordinary checking.
pub fn check_file_with_overlays_and_import_root(
    file: &str,
    overlays: &[(&Path, &str)],
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    match crate::Loader::load_entry_with_overlays_and_import_root(file, overlays, false) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            if let Err(fact_diags) = seed_build_facts(
                &mut bundle,
                "dev",
                false,
                &BTreeMap::new(),
            ) {
                diags.extend(fact_diags);
                return (
                    diags,
                    None,
                    crate::Sema::SemIndexEffectFacts::default(),
                );
            }
            let (check_diags, facts) = crate::Sema::check_bundle_with_effect_facts(
                &mut bundle,
                crate::Sema::CompileMode::Check,
            );
            diags.extend(check_diags);
            let extension_diags = crate::CompilerExtensionHook::post_sema_diagnostics(
                &bundle,
                Some(&facts),
                &diags,
            );
            diags.extend(extension_diags);
            match apply_package_lint_policy(&bundle, diags) {
                Ok(diags) => (diags, Some(bundle), facts),
                Err(diags) => (diags, None, facts),
            }
        }
        Err(diags) => (diags, None, crate::Sema::SemIndexEffectFacts::default()),
    }
}

/// Check-only from source text (eval mode), retaining the checked bundle and
/// the same effect facts used by the semantic-index and build reflection
/// consumers. This is the in-memory counterpart of
/// `check_file_with_effect_facts`; it performs no filesystem I/O.
pub fn check_eval_with_effect_facts(
    src: &str,
    file: &str,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return (
            lex_diags,
            None,
            crate::Sema::SemIndexEffectFacts::default(),
        );
    }
    let mut prog = match crate::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => {
            return (
                ds,
                None,
                crate::Sema::SemIndexEffectFacts::default(),
            )
        }
    };
    let mut bundle = crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from(
            std::path::Path::new(file)
                .parent()
                .unwrap_or(std::path::Path::new(".")),
        ),
        modules: vec![crate::AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            script_body: std::mem::take(&mut prog.script_body),
            block_spans: std::mem::take(&mut prog.block_spans),
            source: src.to_string(),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            default_target: prog.default_target,
            html_path: prog.html_path.clone(),
            no_alloc_policy: prog.no_alloc_policy,
            policy_declarations: prog.policy_declarations.clone(),
            rule_facts: std::mem::take(&mut prog.rule_facts),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        ffi_callback_fns: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger: crate::AST::NameLedger::default(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        active_os: crate::Syntax::OSTarget::host(),
        build_facts: Default::default(),
        edition: crate::Manifest::latest_edition().to_string(),
    };
    if let Err(diags) = crate::Foreign::assemble_active_namespaces(&mut bundle) {
        return (
            diags,
            None,
            crate::Sema::SemIndexEffectFacts::default(),
        );
    }
    bundle.cffi = match crate::CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => {
            return (
                diags,
                None,
                crate::Sema::SemIndexEffectFacts::default(),
            )
        }
    };
    bundle.materialize_script_entries();
    let (diags, facts) = crate::Sema::check_bundle_with_effect_facts(
        &mut bundle,
        crate::Sema::CompileMode::Eval,
    );
    (diags, Some(bundle), facts)
}

/// Check-only from source text (eval mode). Returns only error-severity diagnostics.
pub fn check_eval(src: &str, file: &str) -> Vec<Diagnostic> {
    let (diags, _, _) = check_eval_with_effect_facts(src, file);
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Test harness pipeline.
pub fn compile_tests(
    file: &str,
    coverage: bool,
) -> Result<(String, Option<crate::FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Test);
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let _lints = classify_diagnostics(
        &bundle,
        parse_teaching
            .into_iter()
            .chain(diags)
            .collect(),
        false,
    )?;
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((
        crate::Codegen::emit_bundle_tests_cov(&bundle, ffi.as_ref(), coverage),
        ffi,
    ))
}

/// D-TESTKIT1=A (c308 pass 2, gap #1): a CLI-level error selecting the `jet
/// fuzz` target — no property test, an ambiguous set, or a named test that
/// doesn't exist / isn't a property test. Same tier as `run_bench`'s "can't
/// find the file" message: argument validation, not a compiler diagnostic.
pub enum FuzzCompileError {
    Diagnostics(Vec<Diagnostic>),
    Target(String),
}

/// `jet fuzz <file> [<name>]` pipeline: same front end as `compile_tests`
/// (sema runs in `Test` mode — a property test's body is checked exactly as
/// `jet test` checks it), but codegen emits the fuzz driver harness instead.
pub fn compile_fuzz(
    file: &str,
    test_name: Option<&str>,
) -> Result<(String, Option<crate::FFI::FfiLink>), FuzzCompileError> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)
        .map_err(FuzzCompileError::Diagnostics)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Test);
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let _lints = classify_diagnostics(
        &bundle,
        parse_teaching
            .into_iter()
            .chain(diags)
            .collect(),
        false,
    )
    .map_err(FuzzCompileError::Diagnostics)?;
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(FuzzCompileError::Diagnostics(ffi_diags)),
    };
    match crate::Codegen::emit_bundle_fuzz(&bundle, ffi.as_ref(), file, test_name) {
        Ok(code) => Ok((code, ffi)),
        Err(msg) => Err(FuzzCompileError::Target(msg)),
    }
}

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when the file
/// defines a top-level `fn dev()` — compiles NATIVELY with `dev()` as the
/// program's real entry instead of `run()`. Mechanically: before sema runs,
/// park any existing `fn run` and inject a synthetic `fn run() { entry_fn() }`
/// (I3: codegen stays dumb; sema never special-cases any entry name other
/// than `"run"` — see `Registration.rs`/`Bundle.rs`'s `funcs.get("run")`).
/// The selected function keeps its source name so callers (D-JPK-TASKRUN1
/// plain-call job deps) still resolve. The same entry-swap seam serves
/// `jet dev` and dev-tier job argv.
/// Native only — never freestanding/impure/web (those toggles don't apply to
/// the `fn dev()` entry path; a `dev()` function's job is to configure and run
/// an ordinary value like `core.web.devserver`, nothing more).
pub fn compile_bundle_path_with_entry(
    file: &str,
    entry_fn: &str,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_with_entry_and_settings(file, entry_fn, &BTreeMap::new())
}

pub fn compile_bundle_path_with_entry_and_settings(
    file: &str,
    entry_fn: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    seed_build_facts(&mut bundle, "dev", false, setting_overrides)?;
    swap_entry_point(&mut bundle, entry_fn);
    let mode = crate::Sema::CompileMode::Run;
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    let extension_diags =
        crate::CompilerExtensionHook::post_sema_diagnostics(&bundle, None, &diags);
    // Entry-swap uses plain `check_bundle` (no effect-facts return). Pass
    // `None` → omit `ReadEffects`; do not invent effect rows (D-DX5-HOOK1).
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let lints = gate_diagnostics(
        &bundle,
        parse_teaching,
        diags,
        extension_diags,
    )?;
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    // D-OSTARGET1=A: `jet dev`'s entry-swap path never cross-compiles — host OS.
    let rust = crate::Codegen::emit_bundle_dbg(
        &bundle,
        ffi.as_ref(),
        false,
        crate::Syntax::OSTarget::host(),
    );
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web: None,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin: None,
        library: None,
        library_config: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

/// Make `entry_fn` the program entry without renaming it for name resolution.
///
/// Sema/codegen still require a literal `fn run` (Registration/Bundle
/// `funcs.get("run")`). D-JPK-TASKRUN1 also says a cross-task dependency is a
/// plain call — so renaming `#Job fn greet` → `run` would break
/// `seed()`'s `greet()` with E0102. Fix: park any existing `fn run` as
/// `__jet___unused_run`, then inject a synthetic `fn run(…) { entry_fn(…) }`
/// that forwards params (and return) while leaving `entry_fn` callable.
///
/// The wrapper is never `#Job` (avoids E0928 on reserved lifecycle name
/// `run`). A no-op when `entry_fn` is already `"run"`, or when no function
/// named `entry_fn` exists (caller surfaces E0101 / E1294 separately).
pub fn swap_entry_point(bundle: &mut crate::AST::ProgramBundle, entry_fn: &str) {
    use crate::Diagnostics::Span;
    use crate::AST::{Call, CallArg, CallArgFlags, Expr, Func, Item, Stmt};

    if entry_fn == "run" {
        return;
    }
    // Keep an invalid explicit-run script unchanged so sema can report the conflict and retain
    // its whole-file auto-wrap edit. Valid scripts have already been materialized by the loader.
    let entry_module = &bundle.modules[bundle.entry];
    if !entry_module.script_body.is_empty()
        && entry_module
            .items
            .iter()
            .any(|item| matches!(item, Item::Func(func) if func.name == "run"))
    {
        return;
    }

    let items = &mut bundle.modules[bundle.entry].items;
    let Some(target) = items.iter().find_map(|item| match item {
        Item::Func(f) if f.name == entry_fn => Some(f.clone()),
        _ => None,
    }) else {
        return;
    };

    for item in items.iter_mut() {
        if let Item::Func(f) = item {
            if f.name == "run" {
                f.name = jet_foundation::Names::mangle_generated("unused_run");
                // The shared script seam gives the synthetic entry a fallible
                // unit return so ordinary `jet run` can report a
                // default error. When `jet dev` selects another function,
                // that parked function is not an entry and must not retain a
                // fallthrough obligation (E0114).
                if f.span == f.name_span {
                    f.return_type = None;
                    f.return_type_span = None;
                }
            }
        }
    }

    let zero = Span::new(0, 0);
    let args: Vec<CallArg> = target
        .params
        .iter()
        .map(|p| CallArg {
            convention: p.convention,
            expr: Expr::Ident(p.name.clone(), p.name_span),
            span: p.name_span,
            flags: CallArgFlags::default(),
            label: None,
            spread: p.variadic,
        })
        .collect();
    let call = Expr::Call(Call {
        name: entry_fn.to_string(),
        name_span: target.name_span,
        type_args: Vec::new(),
        args,
        resolved_ret: None,
        range_checked: false,
        widen_approx: false,
    });
    let body = if target.return_type.is_some() {
        vec![Stmt::Return(Some(call), zero)]
    } else {
        vec![Stmt::Expr(call)]
    };

    items.push(Item::Func(Func {
        span: target.span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: "run".to_string(),
        name_span: target.name_span,
        meta: None,
        type_params: target.type_params.clone(),
        head_pattern: None,
        params: target.params.clone(),
        return_type: target.return_type.clone(),
        return_type_span: target.return_type_span,
        return_view_provenance: None,
        declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_sanitizer: false,
        scrub_tag: None,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        is_reactive: false,
                reactive_upgrades: Vec::new(),
        is_replayable: false,
        replayable_span: None,
        is_task: false,
        task_span: None,
        every: None,
        task_metadata: None,
        is_must_use: false,
        must_use_span: None,
        maturity: None,
        maturity_span: None,
        kernel: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        undo: None,
        markers: Vec::new(),
        body,
    }));
}

/// Bench pipeline.
pub fn compile_benches(
    file: &str,
) -> Result<(String, Option<crate::FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Bench);
    let parse_teaching = std::mem::take(&mut bundle.parse_teaching);
    let _lints = classify_diagnostics(
        &bundle,
        parse_teaching
            .into_iter()
            .chain(diags)
            .collect(),
        false,
    )?;
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((
        crate::Codegen::emit_bundle_benches(&bundle, ffi.as_ref()),
        ffi,
    ))
}

#[cfg(test)]
mod tests {
    use super::build_execution_diagnostic;
    use crate::Comptime::Build::BuildExecutionError;

    #[test]
    fn e3504_fix_text_snapshot_uses_canonical_effect_flag() {
        let diagnostic = build_execution_diagnostic(BuildExecutionError::MissingGrant {
            action: "compile".to_string(),
            capability: jet_foundation::BuildEffect::GPU,
        });
        assert_eq!(
            diagnostic.fix,
            "pass `--allow-gpu` for this run, or grant it in package/workspace policy"
        );
    }
}
