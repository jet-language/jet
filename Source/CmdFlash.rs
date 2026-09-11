//! Source-side device flash command seam.
//!
//! A flash plan is derived from the canonical [`TargetMachine`] and its
//! hardware and explicit programmer facts. The plan selects only a tool declared
//! by that profile, verifies the image's target audit before launch, and hands the
//! native process authority.  CLI dispatch and command registration remain in
//! `main.rs`.

#![allow(dead_code)]

use jet::Comptime::Build::{self, NativeSandboxError};
use jet::ReceiptStore::{Receipt, ReceiptSection};
use jet::TargetMachine::{
    MemoryKind, TargetMachine, TargetMachineUse, TargetProgrammerAdapter, TargetProgrammerFacts,
};
use jet::ExitCodes;
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const FLASH_RECEIPT_SCHEMA: &str = "jet.flash.receipt";
const FLASH_PLAN_SCHEMA: &str = "jet.flash.plan";
const FLASH_AUTHORITY: &str = "Build.native_sandboxed";
const MAX_AUDIT_BYTES: u64 = 1024 * 1024;
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const CAPTURE_TRUNCATION_MARKER: &[u8] = b"\n<output truncated>\n";

/// Tools that a target profile may declare for programming or emulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlashAdapter {
    Emulator,
    ProbeRs,
    OpenOcd,
}

impl FlashAdapter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Emulator => "emulator",
            Self::ProbeRs => "probe-rs",
            Self::OpenOcd => "openocd",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "emulator" => Some(Self::Emulator),
            "probe-rs" => Some(Self::ProbeRs),
            "openocd" => Some(Self::OpenOcd),
            _ => None,
        }
    }
}

/// One argument in a declared adapter invocation.  The image is always passed
/// as a single argument; no shell or string command line is ever constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlashArgument {
    Literal(&'static str),
    ImageName,
    Dynamic(String),
    OpenOcdProgram { reset: bool },
}

/// A profile-owned executable declaration.  A bare executable is resolved
/// against the captured PATH, then its canonical path and digest are bound
/// into the plan before invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashToolDeclaration {
    pub adapter: FlashAdapter,
    pub executable: String,
    pub arguments: Vec<FlashArgument>,
    pub programmer: TargetProgrammerFacts,
}

impl FlashToolDeclaration {
    fn from_facts(facts: &TargetProgrammerFacts) -> Self {
        let adapter = match facts.adapter {
            TargetProgrammerAdapter::Emulator => FlashAdapter::Emulator,
            TargetProgrammerAdapter::ProbeRs => FlashAdapter::ProbeRs,
            TargetProgrammerAdapter::OpenOcd => FlashAdapter::OpenOcd,
        };
        let mut arguments = Vec::new();
        match facts.adapter {
            TargetProgrammerAdapter::Emulator => {
                arguments.push(FlashArgument::Literal("-machine"));
                if let Some(machine) = facts.machine.as_deref() {
                    arguments.push(FlashArgument::Dynamic(machine.to_string()));
                }
                arguments.push(FlashArgument::Literal("-cpu"));
                if let Some(cpu) = facts.cpu.as_deref() {
                    arguments.push(FlashArgument::Dynamic(cpu.to_string()));
                }
                arguments.extend([
                    FlashArgument::Literal("-nographic"),
                    FlashArgument::Literal("-kernel"),
                    FlashArgument::ImageName,
                ]);
            }
            TargetProgrammerAdapter::ProbeRs => {
                arguments.push(FlashArgument::Literal("download"));
                if let Some(chip) = facts.chip.as_deref() {
                    arguments.push(FlashArgument::Literal("--chip"));
                    arguments.push(FlashArgument::Dynamic(chip.to_string()));
                }
                if let Some(interface) = facts.interface.as_deref() {
                    arguments.push(FlashArgument::Literal("--probe"));
                    arguments.push(FlashArgument::Dynamic(interface.to_string()));
                }
                if let Some(speed) = facts.speed_khz {
                    arguments.push(FlashArgument::Literal("--speed"));
                    arguments.push(FlashArgument::Dynamic(speed.to_string()));
                }
                if facts.reset {
                    arguments.push(FlashArgument::Literal("--connect-under-reset"));
                }
                arguments.push(FlashArgument::ImageName);
            }
            TargetProgrammerAdapter::OpenOcd => {
                if let Some(interface) = facts.interface.as_deref() {
                    arguments.push(FlashArgument::Literal("-f"));
                    arguments.push(FlashArgument::Dynamic(interface.to_string()));
                }
                for config in &facts.config {
                    arguments.push(FlashArgument::Literal("-f"));
                    arguments.push(FlashArgument::Dynamic(config.clone()));
                }
                if let Some(speed) = facts.speed_khz {
                    arguments.push(FlashArgument::Literal("-c"));
                    arguments.push(FlashArgument::Dynamic(format!("adapter speed {speed}")));
                }
                arguments.push(FlashArgument::Literal("-c"));
                arguments.push(FlashArgument::OpenOcdProgram { reset: facts.reset });
            }
        }
        Self {
            adapter,
            executable: facts.executable.clone(),
            arguments,
            programmer: facts.clone(),
        }
    }

    fn argv(&self, image_name: &str) -> Result<Vec<String>, String> {
        if let Some(missing) = self.programmer.missing_fact() {
            return Err(format!(
                "target programmer `{}` is missing explicit `{missing}` fact",
                self.adapter.as_str()
            ));
        }
        if self.adapter == FlashAdapter::OpenOcd && !safe_openocd_image_name(image_name) {
            return Err(format!(
                "image filename `{image_name}` cannot be passed to OpenOCD safely"
            ));
        }
        self.arguments
            .iter()
            .map(|argument| match argument {
                FlashArgument::Literal(value) => Ok((*value).to_string()),
                FlashArgument::ImageName => Ok(image_name.to_string()),
                FlashArgument::Dynamic(value) => Ok(value.clone()),
                FlashArgument::OpenOcdProgram { reset } => Ok(if *reset {
                    format!("program {image_name} verify reset exit")
                } else {
                    format!("program {image_name} verify exit")
                }),
            })
            .collect()
    }
}

/// The adapter declarations projected from one canonical target profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashProfile {
    pub target: String,
    pub target_triple: String,
    pub target_identity: String,
    pub hardware_identity: String,
    pub tools: Vec<FlashToolDeclaration>,
}

impl FlashProfile {
    /// Project only explicit programmer facts from the canonical target
    /// machine. There is no adapter, chip, config, or interface inference.
    pub fn from_machine(machine: &TargetMachine) -> Result<Self, String> {
        let tools = machine
            .programmers
            .iter()
            .map(FlashToolDeclaration::from_facts)
            .collect::<Vec<_>>();
        if tools.is_empty() {
            return Err(format!(
                "target `{}` has no owner-approved programmer facts; declare an adapter plus chip for probe-rs, interface/config for OpenOCD, or machine/cpu for an emulator",
                machine.name
            ));
        }
        Ok(Self {
            target: machine.name.clone(),
            target_triple: machine.triple.clone(),
            target_identity: machine.provider_identity(),
            hardware_identity: jet::SHA256::sha256_hex(machine.hardware.audit_json().as_bytes()),
            tools,
        })
    }

    fn select(&self, requested: Option<FlashAdapter>) -> Result<&FlashToolDeclaration, String> {
        if let Some(tool) = self.tools.iter().find(|tool| {
            requested.is_none_or(|adapter| tool.adapter == adapter)
                && tool.programmer.missing_fact().is_none()
        }) {
            return Ok(tool);
        }
        if let Some(tool) = self
            .tools
            .iter()
            .find(|tool| requested.is_none_or(|adapter| tool.adapter == adapter))
        {
            if let Some(missing) = tool.programmer.missing_fact() {
                return Err(format!(
                    "target `{}` `{}` programmer is missing explicit `{missing}` fact",
                    self.target,
                    tool.adapter.as_str()
                ));
            }
        }
        let available = self
            .tools
            .iter()
            .map(|tool| tool.adapter.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        match requested {
            Some(adapter) => Err(format!(
                "target `{}` does not declare the `{}` adapter (declared: {available})",
                self.target,
                adapter.as_str()
            )),
            None => Err(format!(
                "target `{}` declares no usable programmer ({available})",
                self.target
            )),
        }
    }
}

/// An invocation whose image, target, adapter, and exact executable identity
/// have all been materialised before a child is allowed to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashPlan {
    pub schema: String,
    pub target: String,
    pub target_triple: String,
    pub target_identity: String,
    pub hardware_identity: String,
    pub adapter: FlashAdapter,
    pub programmer: TargetProgrammerFacts,
    pub tool: String,
    pub tool_identity: String,
    pub executable: PathBuf,
    pub image: PathBuf,
    pub audit: PathBuf,
    pub audit_sha256: String,
    pub image_bytes: u64,
    pub image_sha256: String,
    /// Full argv with the declared tool as argv[0].  The authority receives
    /// only `argv[1..]` because it receives the executable separately.
    pub argv: Vec<String>,
    pub authority: String,
}

impl FlashPlan {
    /// Build a plan using the named canonical target and the image's sibling
    /// target audit sidecar.  Supplying `audit` is useful for generated output
    /// directories; omission still resolves only deterministic sidecar names.
    pub fn from_target_and_image(
        target: &str,
        image: &Path,
        audit: Option<&Path>,
        adapter: Option<FlashAdapter>,
    ) -> Result<Self, String> {
        let machine = jet::Driver::target_machine_by_name(target)
            .ok_or_else(|| format!("unknown target machine `{target}`"))?;
        Self::from_machine(&machine, image, audit, adapter)
    }

    /// Build a plan from an already selected canonical target machine.
    pub fn from_machine(
        machine: &TargetMachine,
        image: &Path,
        audit: Option<&Path>,
        adapter: Option<FlashAdapter>,
    ) -> Result<Self, String> {
        if !machine.no_os {
            return Err(format!(
                "target `{}` is hosted; flashing requires a no-OS board or emulator profile",
                machine.name
            ));
        }
        let usage = TargetMachineUse::default();
        let validation = machine.validate(&usage);
        if !validation.is_empty() {
            return Err(format!(
                "target `{}` failed canonical profile validation: {validation:?}",
                machine.name
            ));
        }
        let flash_capacity = machine
            .memory
            .iter()
            .filter(|region| region.kind == MemoryKind::Flash)
            .map(|region| region.size.bytes)
            .max()
            .ok_or_else(|| format!("target `{}` declares no flash memory region", machine.name))?;

        let image = regular_file(image, "flash image")?;
        let image_metadata = fs::metadata(&image)
            .map_err(|error| format!("cannot inspect flash image `{}`: {error}", image.display()))?;
        if image_metadata.len() > flash_capacity {
            return Err(format!(
                "flash image `{}` is {} bytes but target `{}` declares only {} flash bytes",
                image.display(),
                image_metadata.len(),
                machine.name,
                flash_capacity
            ));
        }
        let image_sha256 = jet::SHA256::sha256_file_hex(&image)
            .map_err(|error| format!("cannot hash flash image `{}`: {error}", image.display()))?;
        let image_name = image
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("flash image `{}` has no UTF-8 filename", image.display()))?;

        let audit = resolve_audit_path(&image, &machine.name, audit)?;
        let audit_text = read_audit(&audit)?;
        let audit_sha256 = jet::SHA256::sha256_hex(audit_text.as_bytes());
        let audit_fields = parse_top_level_string_fields(&audit_text)
            .map_err(|error| format!("target audit `{}` is malformed: {error}", audit.display()))?;
        verify_audit_identity(&audit_fields, machine, &image_sha256)?;
        let profile = FlashProfile::from_machine(machine)?;
        let declaration = profile.select(adapter)?;
        let declared_argv = declaration.argv(image_name)?;
        let executable = resolve_declared_tool(&declaration.executable)?;
        let tool_identity = jet::SHA256::sha256_file_hex(&executable).map_err(|error| {
            format!(
                "cannot hash declared flash tool `{}`: {error}",
                executable.display()
            )
        })?;
        let mut argv = Vec::with_capacity(declaration.arguments.len() + 1);
        argv.push(declaration.executable.clone());
        argv.extend(declared_argv);

        Ok(Self {
            schema: FLASH_PLAN_SCHEMA.to_string(),
            target: machine.name.clone(),
            target_triple: machine.triple.clone(),
            target_identity: profile.target_identity.clone(),
            hardware_identity: profile.hardware_identity.clone(),
            adapter: declaration.adapter,
            programmer: declaration.programmer.clone(),
            tool: declaration.executable.clone(),
            tool_identity,
            executable,
            image,
            audit,
            audit_sha256,
            image_bytes: image_metadata.len(),
            image_sha256,
            argv,
            authority: FLASH_AUTHORITY.to_string(),
        })
    }

    pub fn identity(&self) -> String {
        String::from_utf8(
            CanonicalJson::object([
                ("adapter".into(), CanonicalJson::String(self.adapter.as_str().into())),
                ("authority".into(), CanonicalJson::String(self.authority.clone())),
                (
                    "audit".into(),
                    CanonicalJson::String(self.audit.to_string_lossy().into_owned()),
                ),
                (
                    "audit_sha256".into(),
                    CanonicalJson::String(self.audit_sha256.clone()),
                ),
                (
                    "hardware_identity".into(),
                    CanonicalJson::String(self.hardware_identity.clone()),
                ),
                (
                    "image".into(),
                    CanonicalJson::String(self.image.to_string_lossy().into_owned()),
                ),
                (
                    "image_bytes".into(),
                    CanonicalJson::Integer(self.image_bytes.to_string()),
                ),
                (
                    "image_sha256".into(),
                    CanonicalJson::String(self.image_sha256.clone()),
                ),
                (
                    "argv".into(),
                    CanonicalJson::Array(
                        self.argv.iter().cloned().map(CanonicalJson::String).collect(),
                    ),
                ),
                ("plan_schema".into(), CanonicalJson::String(self.schema.clone())),
                ("programmer".into(), programmer_json(&self.programmer)),
                ("target".into(), CanonicalJson::String(self.target.clone())),
                (
                    "target_identity".into(),
                    CanonicalJson::String(self.target_identity.clone()),
                ),
                (
                    "target_triple".into(),
                    CanonicalJson::String(self.target_triple.clone()),
                ),
                ("tool".into(), CanonicalJson::String(self.tool.clone())),
                (
                    "tool_identity".into(),
                    CanonicalJson::String(self.tool_identity.clone()),
                ),
            ])
            .expect("flash plan JSON keys are unique")
            .bytes(),
        )
        .expect("flash plan identity JSON is UTF-8")
        .trim_end_matches('\n')
        .to_string()
    }
}

/// A deterministic typed result section carried by the shared receipt store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashReceipt {
    pub schema: String,
    pub plan_identity: String,
    pub target: String,
    pub target_triple: String,
    pub target_identity: String,
    pub hardware_identity: String,
    pub adapter: FlashAdapter,
    pub programmer: TargetProgrammerFacts,
    pub tool: String,
    pub tool_identity: String,
    pub image: String,
    pub audit: String,
    pub audit_sha256: String,
    pub image_bytes: u64,
    pub image_sha256: String,
    pub argv: Vec<String>,
    pub authority: String,
    pub mechanism: String,
    pub policy: String,
    pub status: Option<i32>,
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub failure: Option<FlashFailure>,
    pub result_identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashFailure {
    pub kind: String,
    pub detail: String,
}

impl FlashReceipt {
    fn from_execution(plan: &FlashPlan, execution: FlashExecution) -> Self {
        let (stdout, stdout_truncated) = bound_capture(&execution.stdout);
        let (stderr, stderr_truncated) = bound_capture(&execution.stderr);
        let mut receipt = Self {
            schema: FLASH_RECEIPT_SCHEMA.to_string(),
            plan_identity: plan.identity(),
            target: plan.target.clone(),
            target_triple: plan.target_triple.clone(),
            target_identity: plan.target_identity.clone(),
            hardware_identity: plan.hardware_identity.clone(),
            adapter: plan.adapter,
            programmer: plan.programmer.clone(),
            tool: plan.tool.clone(),
            tool_identity: plan.tool_identity.clone(),
            image: plan.image.to_string_lossy().into_owned(),
            audit: plan.audit.to_string_lossy().into_owned(),
            audit_sha256: plan.audit_sha256.clone(),
            image_bytes: plan.image_bytes,
            image_sha256: plan.image_sha256.clone(),
            argv: plan.argv.clone(),
            authority: plan.authority.clone(),
            mechanism: execution.mechanism,
            policy: execution.policy,
            status: execution.status,
            success: execution.failure.is_none(),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            failure: execution.failure,
            result_identity: String::new(),
        };
        receipt.result_identity = format!("flash-result-v1:{}", receipt.result_json().sha256());
        receipt
    }

    fn result_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("adapter".into(), CanonicalJson::String(self.adapter.as_str().into())),
            (
                "argv".into(),
                CanonicalJson::Array(self.argv.iter().cloned().map(CanonicalJson::String).collect()),
            ),
            ("audit".into(), CanonicalJson::String(self.audit.clone())),
            ("audit_sha256".into(), CanonicalJson::String(self.audit_sha256.clone())),
            ("failure".into(), self.failure_json()),
            (
                "hardware_identity".into(),
                CanonicalJson::String(self.hardware_identity.clone()),
            ),
            ("image_sha256".into(), CanonicalJson::String(self.image_sha256.clone())),
            ("mechanism".into(), CanonicalJson::String(self.mechanism.clone())),
            ("plan_identity".into(), CanonicalJson::String(self.plan_identity.clone())),
            ("policy".into(), CanonicalJson::String(self.policy.clone())),
            ("programmer".into(), programmer_json(&self.programmer)),
            (
                "status".into(),
                self.status
                    .map_or(CanonicalJson::Null, |value| CanonicalJson::Integer(value.to_string())),
            ),
            (
                "stderr".into(),
                CanonicalJson::String(String::from_utf8_lossy(&self.stderr).into_owned()),
            ),
            (
                "stdout".into(),
                CanonicalJson::String(String::from_utf8_lossy(&self.stdout).into_owned()),
            ),
            ("success".into(), CanonicalJson::Bool(self.success)),
            (
                "target_identity".into(),
                CanonicalJson::String(self.target_identity.clone()),
            ),
            ("tool_identity".into(), CanonicalJson::String(self.tool_identity.clone())),
        ])
        .expect("flash result JSON keys are unique")
    }

    fn failure_json(&self) -> CanonicalJson {
        self.failure.as_ref().map_or(CanonicalJson::Null, |failure| {
            CanonicalJson::object([
                ("detail".into(), CanonicalJson::String(failure.detail.clone())),
                ("kind".into(), CanonicalJson::String(failure.kind.clone())),
            ])
            .expect("flash failure JSON keys are unique")
        })
    }

    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            ("adapter".into(), CanonicalJson::String(self.adapter.as_str().into())),
            (
                "argv".into(),
                CanonicalJson::Array(self.argv.iter().cloned().map(CanonicalJson::String).collect()),
            ),
            ("audit".into(), CanonicalJson::String(self.audit.clone())),
            ("audit_sha256".into(), CanonicalJson::String(self.audit_sha256.clone())),
            ("authority".into(), CanonicalJson::String(self.authority.clone())),
            ("failure".into(), self.failure_json()),
            (
                "hardware_identity".into(),
                CanonicalJson::String(self.hardware_identity.clone()),
            ),
            ("image".into(), CanonicalJson::String(self.image.clone())),
            ("image_bytes".into(), CanonicalJson::Integer(self.image_bytes.to_string())),
            ("image_sha256".into(), CanonicalJson::String(self.image_sha256.clone())),
            ("mechanism".into(), CanonicalJson::String(self.mechanism.clone())),
            ("plan_identity".into(), CanonicalJson::String(self.plan_identity.clone())),
            ("policy".into(), CanonicalJson::String(self.policy.clone())),
            ("programmer".into(), programmer_json(&self.programmer)),
            ("receipt_schema".into(), CanonicalJson::String(self.schema.clone())),
            (
                "result".into(),
                CanonicalJson::String(if self.success { "ok" } else { "failed" }.into()),
            ),
            ("result_identity".into(), CanonicalJson::String(self.result_identity.clone())),
            (
                "status".into(),
                self.status
                    .map_or(CanonicalJson::Null, |value| CanonicalJson::Integer(value.to_string())),
            ),
            (
                "stderr".into(),
                CanonicalJson::String(String::from_utf8_lossy(&self.stderr).into_owned()),
            ),
            ("stderr_truncated".into(), CanonicalJson::Bool(self.stderr_truncated)),
            (
                "stdout".into(),
                CanonicalJson::String(String::from_utf8_lossy(&self.stdout).into_owned()),
            ),
            ("stdout_truncated".into(), CanonicalJson::Bool(self.stdout_truncated)),
            ("target".into(), CanonicalJson::String(self.target.clone())),
            (
                "target_identity".into(),
                CanonicalJson::String(self.target_identity.clone()),
            ),
            ("target_triple".into(), CanonicalJson::String(self.target_triple.clone())),
            ("tool".into(), CanonicalJson::String(self.tool.clone())),
            ("tool_identity".into(), CanonicalJson::String(self.tool_identity.clone())),
        ])
        .expect("flash receipt JSON keys are unique")
    }

    pub fn section(&self) -> Result<ReceiptSection, String> {
        ReceiptSection::from_json("flash", "FlashReceipt", self.to_json())
    }
}

struct FlashExecution {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    mechanism: String,
    policy: String,
    failure: Option<FlashFailure>,
}

impl FlashExecution {
    fn failed(kind: impl Into<String>, detail: impl Into<String>, mechanism: impl Into<String>, policy: impl Into<String>) -> Self {
        Self {
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            mechanism: mechanism.into(),
            policy: policy.into(),
            failure: Some(FlashFailure { kind: kind.into(), detail: detail.into() }),
        }
    }
}

fn execute_flash(plan: &FlashPlan) -> FlashExecution {
    let status = Build::native_sandbox_status();
    if !status.available {
        return FlashExecution::failed(
            "authority_unavailable",
            format!("native process authority unavailable: {}", status.reason),
            status.mechanism.clone(),
            status.policy.clone(),
        );
    }

    let output_dir = match temporary_output_dir(plan) {
        Ok(path) => path,
        Err(error) => {
            return FlashExecution::failed(
                "authority_setup",
                error,
                status.mechanism.clone(),
                status.policy.clone(),
            )
        }
    };
    let source_dir = plan.image.parent().unwrap_or_else(|| Path::new("."));
    let args = &plan.argv[1..];
    let result = Build::run_native_sandboxed(
        &plan.executable,
        args,
        source_dir,
        Some(&output_dir),
        &BTreeMap::new(),
        false,
    );
    let _ = fs::remove_dir_all(&output_dir);

    match result {
        Ok(result) => {
            let status_code = result.output.status.code();
            let failure = if result.output.status.success() {
                None
            } else {
                Some(FlashFailure {
                    kind: if status_code.is_some() {
                        "tool_exit".to_string()
                    } else {
                        "tool_signal".to_string()
                    },
                    detail: format!("declared flash tool exited with status {status_code:?}"),
                })
            };
            FlashExecution {
                status: status_code,
                stdout: result.output.stdout,
                stderr: result.output.stderr,
                mechanism: result.mechanism,
                policy: result.policy,
                failure,
            }
        }
        Err(error) => {
            let detail = match error {
                NativeSandboxError::Unsupported(detail) => detail,
                NativeSandboxError::Io(detail) => detail,
            };
            FlashExecution::failed(
                "authority_spawn",
                detail,
                status.mechanism.clone(),
                status.policy.clone(),
            )
        }
    }
}

fn temporary_output_dir(plan: &FlashPlan) -> Result<PathBuf, String> {
    let suffix = &plan.image_sha256[..16.min(plan.image_sha256.len())];
    let path = std::env::temp_dir().join(format!("jet-flash-{}-{suffix}", std::process::id()));
    fs::create_dir(&path).map_err(|error| format!("cannot create private flash output: {error}"))?;
    Ok(path)
}

/// Run the source-only flash seam.  The dispatcher can pass either the full
/// argv (`flash --target …`) or the command's argument tail.
pub(crate) fn run_flash(args: &[String], mode: crate::OutputMode) -> i32 {
    let parsed = match parse_flash_args(args) {
        Ok(FlashParse::Help) => {
            println!("{}", flash_help());
            return ExitCodes::OK;
        }
        Ok(FlashParse::Options(options)) => options,
        Err(error) => {
            crate::emit_cli_report(
                "E2104",
                error,
                "flash needs a declared target; the image defaults to the selected target artifact".to_string(),
                flash_help(),
                mode.json,
            );
            return ExitCodes::USAGE;
        }
    };
    let target = match parsed.target.as_deref() {
        Some(target) => target,
        None => {
            crate::emit_cli_report(
                "E2104",
                "`jet flash` needs `--target <board>`".to_string(),
                "flash selects a canonical no-OS target profile before invocation".to_string(),
                flash_help(),
                mode.json,
            );
            return ExitCodes::USAGE;
        }
    };
    let image = parsed
        .image
        .unwrap_or_else(|| default_flash_image(target));
    let plan = match FlashPlan::from_target_and_image(
        target,
        &image,
        parsed.audit.as_deref(),
        parsed.adapter,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            crate::emit_cli_report(
                "E2104",
                error,
                "flash refuses an unverified image, target, adapter, or tool".to_string(),
                "build the selected target or pass its firmware image and target audit".to_string(),
                mode.json,
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let execution = execute_flash(&plan);
    let success = execution.failure.is_none();
    let receipt = FlashReceipt::from_execution(&plan, execution);
    let receipt_args = canonical_flash_args(args);
    let stored = match record_flash_receipt(&receipt_args, &plan, &receipt) {
        Ok(receipt) => receipt,
        Err(error) => {
            crate::emit_cli_report(
                "E2105",
                format!("flash result could not be recorded: {error}"),
                "a device operation must leave an authenticated typed receipt".to_string(),
                "repair the receipt store and retry the flash".to_string(),
                mode.json,
            );
            return ExitCodes::USER_ERROR;
        }
    };

    if !success {
        let detail = receipt
            .failure
            .as_ref()
            .map(|failure| format!("{}: {}", failure.kind, failure.detail))
            .unwrap_or_else(|| "unknown flash failure".to_string());
        crate::emit_cli_report(
            "E2105",
            format!("flash failed for `{}` ({detail}); receipt {}", plan.target, stored.digest),
            "the declared flash adapter did not produce a successful result".to_string(),
            "inspect the typed flash receipt, repair the board or tool, and retry".to_string(),
            mode.json,
        );
        return ExitCodes::USER_ERROR;
    }
    render_flash_report(&plan, &receipt, &stored, mode);
    ExitCodes::OK
}

fn record_flash_receipt(
    args: &[String],
    plan: &FlashPlan,
    receipt: &FlashReceipt,
) -> Result<Receipt, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("cannot read current directory: {error}"))?;
    let root = jet::ReceiptStore::receipt_root_for("flash", args, &cwd);
    let store = jet::ReceiptStore::ReceiptStore::new(root);
    let section = receipt.section()?;
    store.record_with_sections(
        "flash",
        args,
        &[plan.image.clone(), plan.audit.clone()],
        receipt.status.unwrap_or(ExitCodes::USER_ERROR),
        &receipt.stdout,
        &receipt.stderr,
        &[section],
    )
}

fn render_flash_report(
    plan: &FlashPlan,
    receipt: &FlashReceipt,
    stored: &Receipt,
    mode: crate::OutputMode,
) {
    if mode.json {
        let fields = StatusFields::new()
            .with("adapter", plan.adapter.as_str())
            .with("plan", plan_value(plan))
            .with("receipt", status_value(&receipt.to_json()))
            .with("receipt_digest", stored.digest.as_str());
        println!(
            "{}",
            StatusEnvelope::new("flash", true)
                .with_fields(fields)
                .json()
        );
    } else {
        println!("flash: ok");
        println!("target: {} ({})", plan.target, plan.target_triple);
        println!("adapter: {}", plan.adapter.as_str());
        println!("image: {}", plan.image.display());
        println!("image sha256: {}", plan.image_sha256);
        println!("target audit sha256: {}", plan.audit_sha256);
        println!("receipt: {}", stored.digest);
    }
}

#[derive(Default)]
struct FlashOptions {
    target: Option<String>,
    image: Option<PathBuf>,
    audit: Option<PathBuf>,
    adapter: Option<FlashAdapter>,
}

enum FlashParse {
    Help,
    Options(FlashOptions),
}

fn parse_flash_args(args: &[String]) -> Result<FlashParse, String> {
    let mut options = FlashOptions::default();
    let mut index = usize::from(args.first().is_some_and(|arg| arg == "flash"));
    while index < args.len() {
        let argument = args[index].as_str();
        if matches!(argument, "--json" | "--quiet") {
            index += 1;
            continue;
        }
        if matches!(argument, "--help" | "help") {
            return Ok(FlashParse::Help);
        }
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(flag, value)| (flag, Some(value)));
        let value = |name: &str, inline: Option<&str>, index: &mut usize| {
            if let Some(value) = inline {
                if value.is_empty() {
                    return Err(format!("`{name}` needs a value"));
                }
                return Ok(value.to_string());
            }
            *index += 1;
            args.get(*index)
                .filter(|value| !value.starts_with('-'))
                .cloned()
                .ok_or_else(|| format!("`{name}` needs a value"))
        };
        match flag {
            "--target" => options.target = Some(value("--target", inline, &mut index)?),
            "--image" => options.image = Some(PathBuf::from(value("--image", inline, &mut index)?)),
            "--audit" => options.audit = Some(PathBuf::from(value(flag, inline, &mut index)?)),
            "--adapter" => {
                let value = value(flag, inline, &mut index)?;
                options.adapter = FlashAdapter::parse(&value)
                    .ok_or_else(|| format!("unknown flash adapter `{value}` (use probe-rs, openocd, or emulator)"))
                    .map(Some)?;
            }
            _ => return Err(format!("unexpected flash argument `{argument}`")),
        }
        index += 1;
    }
    Ok(FlashParse::Options(options))
}

fn canonical_flash_args(args: &[String]) -> Vec<String> {
    if args.first().map(String::as_str) == Some("flash") {
        return args.to_vec();
    }
    let mut result = Vec::with_capacity(args.len() + 1);
    result.push("flash".to_string());
    result.extend(args.iter().cloned());
    result
}
fn default_flash_image(target: &str) -> PathBuf {
    let canonical_target = jet::Driver::target_machine_by_name(target)
        .map(|machine| machine.name)
        .unwrap_or_else(|| target.to_string());
    PathBuf::from(".jet")
        .join("target")
        .join(canonical_target)
        .join("firmware.elf")
}

fn resolve_declared_tool(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(format!("declared flash tool `{name}` is not a bare executable"));
    }
    let path = std::env::var_os("PATH")
        .ok_or_else(|| "cannot resolve declared flash tool without a PATH snapshot".to_string())?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        let metadata = match fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        return candidate
            .canonicalize()
            .map_err(|error| format!("cannot canonicalize declared flash tool `{name}`: {error}"));
    }
    Err(format!(
        "declared flash tool `{name}` was not found in the captured PATH"
    ))
}

fn regular_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} `{}` is not a regular file", path.display()));
    }
    path.canonicalize()
        .map_err(|error| format!("cannot canonicalize {label} `{}`: {error}", path.display()))
}

fn resolve_audit_path(image: &Path, target: &str, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return regular_file(path, "target audit");
    }
    let parent = image.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = vec![image.with_extension("target.json")];
    let target_name = target
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
        .collect::<String>();
    let named = parent.join(format!("{target_name}.target.json"));
    if named != candidates[0] {
        candidates.push(named);
    }
    let mut existing = Vec::new();
    for candidate in candidates {
        if fs::symlink_metadata(&candidate).is_ok() {
            existing.push(regular_file(&candidate, "target audit")?);
        }
    }
    match existing.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!(
            "no target audit sidecar found beside `{}`; pass `--audit <target.json>`",
            image.display()
        )),
        _ => Err(format!(
            "multiple target audit sidecars found beside `{}`; pass one with `--audit`",
            image.display()
        )),
    }
}

fn read_audit(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect target audit `{}`: {error}", path.display()))?;
    if metadata.len() > MAX_AUDIT_BYTES {
        return Err(format!(
            "target audit `{}` exceeds the {MAX_AUDIT_BYTES}-byte limit",
            path.display()
        ));
    }
    fs::read_to_string(path)
        .map_err(|error| format!("cannot read target audit `{}`: {error}", path.display()))
}

fn verify_audit_identity(
    fields: &BTreeMap<String, String>,
    machine: &TargetMachine,
    image_sha256: &str,
) -> Result<(), String> {
    let provider_identity = machine.provider_identity();
    for (key, expected) in [
        ("name", machine.name.as_str()),
        ("triple", machine.triple.as_str()),
        ("environment", machine.environment_identity()),
        ("provider_identity", provider_identity.as_str()),
    ] {
        let expected_token = json_string_token(expected);
        let actual = fields
            .get(key)
            .ok_or_else(|| format!("target audit has no `{key}` identity field"))?;
        if actual != &expected_token {
            return Err(format!(
                "target audit `{key}` does not match selected target `{}`",
                machine.name
            ));
        }
    }
    for key in ["image_sha256", "artifact_sha256", "image_digest"] {
        if let Some(actual) = fields.get(key) {
            let expected = json_string_token(image_sha256);
            let prefixed = json_string_token(&format!("sha256:{image_sha256}"));
            if actual != &expected && actual != &prefixed {
                return Err(format!("target audit `{key}` does not match the image digest"));
            }
            break;
        }
    }
    Ok(())
}

/// Parse just enough JSON to identify unique top-level string facts.  Target
/// audits are compiler-produced JSON, but this parser keeps verification
/// independent of a permissive JSON implementation and rejects duplicates.
fn parse_top_level_string_fields(text: &str) -> Result<BTreeMap<String, String>, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    skip_json_space(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return Err("top-level value is not an object".into());
    }
    index += 1;
    let mut fields = BTreeMap::new();
    loop {
        skip_json_space(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            index += 1;
            break;
        }
        let key = parse_json_string_token(bytes, &mut index)?;
        if key.contains('\\') {
            return Err("escaped object keys are not supported".into());
        }
        skip_json_space(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return Err("object key is not followed by `:`".into());
        }
        index += 1;
        skip_json_space(bytes, &mut index);
        let value = if bytes.get(index) == Some(&b'"') {
            parse_json_string_token(bytes, &mut index)?
        } else {
            skip_json_value(bytes, &mut index)?;
            String::new()
        };
        if fields.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate top-level field `{key}`"));
        }
        skip_json_space(bytes, &mut index);
        match bytes.get(index) {
            Some(b',') => {
                index += 1;
                let mut next = index;
                skip_json_space(bytes, &mut next);
                if bytes.get(next) == Some(&b'}') {
                    return Err("object has a trailing comma".into());
                }
            }
            Some(b'}') => {
                index += 1;
                break;
            }
            _ => return Err("object field is not followed by `,` or `}`".into()),
        }
    }
    skip_json_space(bytes, &mut index);
    if index != bytes.len() {
        return Err("trailing bytes after target audit object".into());
    }
    Ok(fields)
}

fn parse_json_string_token(bytes: &[u8], index: &mut usize) -> Result<String, String> {
    if bytes.get(*index) != Some(&b'"') {
        return Err("expected a JSON string".into());
    }
    *index += 1;
    let start = *index;
    let mut escaped = false;
    while let Some(byte) = bytes.get(*index).copied() {
        *index += 1;
        if escaped {
            escaped = false;
            if byte == b'u' {
                for _ in 0..4 {
                    if !bytes.get(*index).is_some_and(|digit| digit.is_ascii_hexdigit()) {
                        return Err("malformed JSON unicode escape".into());
                    }
                    *index += 1;
                }
            }
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'"' => {
                return String::from_utf8(bytes[start..*index - 1].to_vec())
                    .map_err(|_| "JSON string is not UTF-8".into())
            }
            0..=0x1f => return Err("JSON string contains a control character".into()),
            _ => {}
        }
    }
    Err("unterminated JSON string".into())
}

fn skip_json_value(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    let Some(first) = bytes.get(*index).copied() else {
        return Err("missing JSON value".into());
    };
    if first == b'"' {
        parse_json_string_token(bytes, index)?;
        return Ok(());
    }
    if matches!(first, b'{' | b'[') {
        let mut stack = vec![first];
        *index += 1;
        let mut escaped = false;
        while let Some(byte) = bytes.get(*index).copied() {
            *index += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                while let Some(inner) = bytes.get(*index).copied() {
                    *index += 1;
                    if inner == b'\\' {
                        *index += 1;
                    } else if inner == b'"' {
                        break;
                    }
                }
                continue;
            }
            if byte == b'{' || byte == b'[' {
                stack.push(byte);
            } else if byte == b'}' || byte == b']' {
                let Some(open) = stack.pop() else {
                    return Err("unbalanced JSON value".into());
                };
                if (open == b'{' && byte != b'}') || (open == b'[' && byte != b']') {
                    return Err("mismatched JSON delimiters".into());
                }
                if stack.is_empty() {
                    return Ok(());
                }
            }
        }
        return Err("unterminated JSON value".into());
    }
    let start = *index;
    while let Some(byte) = bytes.get(*index).copied() {
        if matches!(byte, b',' | b'}' | b']') || byte.is_ascii_whitespace() {
            break;
        }
        *index += 1;
    }
    if *index == start {
        Err("empty JSON value".into())
    } else {
        Ok(())
    }
}

fn skip_json_space(bytes: &[u8], index: &mut usize) {
    while bytes
        .get(*index)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *index += 1;
    }
}

fn json_string_token(value: &str) -> String {
    let bytes = CanonicalJson::String(value.to_string()).bytes();
    String::from_utf8(bytes[1..bytes.len() - 2].to_vec()).expect("canonical JSON string is UTF-8")
}

fn bound_capture(bytes: &[u8]) -> (Vec<u8>, bool) {
    if bytes.len() <= MAX_CAPTURE_BYTES {
        return (bytes.to_vec(), false);
    }
    let keep = MAX_CAPTURE_BYTES.saturating_sub(CAPTURE_TRUNCATION_MARKER.len());
    let mut bounded = Vec::with_capacity(MAX_CAPTURE_BYTES);
    bounded.extend_from_slice(&bytes[..keep]);
    bounded.extend_from_slice(CAPTURE_TRUNCATION_MARKER);
    (bounded, true)
}

fn safe_openocd_image_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

fn status_value(value: &CanonicalJson) -> StatusValue {
    match value {
        CanonicalJson::Null => StatusValue::Null,
        CanonicalJson::Bool(value) => StatusValue::Bool(*value),
        CanonicalJson::Integer(value) => StatusValue::Integer(
            value
                .parse()
                .expect("canonical flash integer must fit status integer"),
        ),
        CanonicalJson::String(value) => StatusValue::String(value.clone()),
        CanonicalJson::Array(values) => StatusValue::array(values.iter().map(status_value)),
        CanonicalJson::Object(values) => {
            let mut fields = StatusFields::new();
            for (name, value) in values {
                fields = fields.with(name.as_str(), status_value(value));
            }
            StatusValue::object(fields)
        }
    }
}

fn plan_value(plan: &FlashPlan) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("adapter", plan.adapter.as_str())
            .with("authority", plan.authority.as_str())
            .with("audit", plan.audit.to_string_lossy().into_owned())
            .with("audit_sha256", plan.audit_sha256.as_str())
            .with("hardware_identity", plan.hardware_identity.as_str())
            .with("image", plan.image.to_string_lossy().into_owned())
            .with("image_bytes", plan.image_bytes)
            .with("image_sha256", plan.image_sha256.as_str())
            .with(
                "argv",
                StatusValue::array(
                    plan.argv
                        .iter()
                        .map(|value| StatusValue::from(value.as_str())),
                ),
            )
            .with("plan_schema", plan.schema.as_str())
            .with("programmer", programmer_value(&plan.programmer))
            .with("target", plan.target.as_str())
            .with("target_identity", plan.target_identity.as_str())
            .with("target_triple", plan.target_triple.as_str())
            .with("tool", plan.tool.as_str())
            .with("tool_identity", plan.tool_identity.as_str()),
    )
}

fn programmer_value(facts: &TargetProgrammerFacts) -> StatusValue {
    let optional = |value: &Option<String>| {
        value
            .as_deref()
            .map(StatusValue::from)
            .unwrap_or(StatusValue::Null)
    };
    StatusValue::object(
        StatusFields::new()
            .with("adapter", facts.adapter.as_str())
            .with("chip", optional(&facts.chip))
            .with(
                "config",
                StatusValue::array(
                    facts
                        .config
                        .iter()
                        .map(|value| StatusValue::from(value.as_str())),
                ),
            )
            .with("cpu", optional(&facts.cpu))
            .with("executable", facts.executable.as_str())
            .with("interface", optional(&facts.interface))
            .with("machine", optional(&facts.machine))
            .with("reset", facts.reset)
            .with(
                "speed_khz",
                facts
                    .speed_khz
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn programmer_json(facts: &TargetProgrammerFacts) -> CanonicalJson {
    let optional_string = |value: &Option<String>| {
        value
            .as_ref()
            .map_or(CanonicalJson::Null, |value| CanonicalJson::String(value.clone()))
    };
    CanonicalJson::object([
        (
            "adapter".into(),
            CanonicalJson::String(facts.adapter.as_str().into()),
        ),
        ("chip".into(), optional_string(&facts.chip)),
        (
            "config".into(),
            CanonicalJson::Array(
                facts
                    .config
                    .iter()
                    .cloned()
                    .map(CanonicalJson::String)
                    .collect(),
            ),
        ),
        ("cpu".into(), optional_string(&facts.cpu)),
        (
            "executable".into(),
            CanonicalJson::String(facts.executable.clone()),
        ),
        ("interface".into(), optional_string(&facts.interface)),
        ("machine".into(), optional_string(&facts.machine)),
        ("reset".into(), CanonicalJson::Bool(facts.reset)),
        (
            "speed_khz".into(),
            facts
                .speed_khz
                .map_or(CanonicalJson::Null, |speed| CanonicalJson::Integer(speed.to_string())),
        ),
    ])
    .expect("flash programmer JSON keys are unique")
}

fn flash_help() -> String {
    "jet flash --target <board.name> [--image <firmware.elf>] [--audit <target.json>] [--adapter <probe-rs|openocd|emulator>]".to_string()
}
