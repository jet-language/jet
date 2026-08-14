//! D-PERFBUDGET-PROVIDER1: compiler-owned measurement-provider registry.
//!
//! Registry keys, executable paths, and response files are resolved by the
//! compiler. No provider lookup consults `PATH`. Every transport feeds the
//! same binary decoder and limit checker before evidence reaches evaluation.

use jet_foundation::PerformanceBudget::{CanonicalJson, Rational};
use jet_foundation::PerformanceBudget::{Comparison, Direction, Enforcement, Evaluation, MeasurementPolicy, Percentile};
use jet_foundation::SHA256::sha256_hex;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering}};
use std::time::{Duration, Instant};

pub const MAX_SAMPLES: usize = 1_000_000;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SPECS: usize = 4_096;
pub const MAX_DETAIL_SCALARS: usize = 512;
pub const MAX_METADATA_SCALARS: usize = 262_144;
const MAGIC: &[u8] = b"JETBUDGET1\n";
#[cfg(test)]static FILE_READER_DELAY_MS:AtomicU64=AtomicU64::new(0);
#[cfg(test)]static ACTIVE_FILE_READERS:AtomicU64=AtomicU64::new(0);
#[cfg(test)]static ACTIVE_ISOLATED_WORKERS:AtomicU64=AtomicU64::new(0);
#[cfg(test)]static LAST_ISOLATED_GROUP:AtomicU64=AtomicU64::new(0);
static COMPILE_WORKLOAD_SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]struct ActiveFileReader;
#[cfg(test)]impl ActiveFileReader{fn new()->Self{ACTIVE_FILE_READERS.fetch_add(1,AtomicOrdering::SeqCst);Self}}
#[cfg(test)]impl Drop for ActiveFileReader{fn drop(&mut self){ACTIVE_FILE_READERS.fetch_sub(1,AtomicOrdering::SeqCst);}}
#[cfg(test)]struct ActiveIsolatedWorker;
#[cfg(test)]impl ActiveIsolatedWorker{fn new()->Self{ACTIVE_ISOLATED_WORKERS.fetch_add(1,AtomicOrdering::SeqCst);Self}}
#[cfg(test)]impl Drop for ActiveIsolatedWorker{fn drop(&mut self){ACTIVE_ISOLATED_WORKERS.fetch_sub(1,AtomicOrdering::SeqCst);}}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSpec {
    pub budget_hash: String,
    pub metric: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRequest {
    pub schema: String,
    pub version: u32,
    pub request_id: String,
    pub provider_hash: String,
    pub context_hash: String,
    pub specs: Vec<ProviderSpec>,
    pub workload: CanonicalJson,
    pub policy: CanonicalJson,
}

impl ProviderRequest {
    pub fn validate(&self) -> Result<(), ProviderFailure> {
        if self.schema != "jet.provider-request" || self.version != 1 {
            return Err(ProviderFailure::malformed("unsupported provider request schema/version"));
        }
        for (name, value) in [("request_id", &self.request_id), ("provider_hash", &self.provider_hash), ("context_hash", &self.context_hash)] {
            if !is_hex64(value) { return Err(ProviderFailure::malformed(format!("{name} is not lowercase Hex64"))); }
        }
        if self.specs.is_empty() || self.specs.len() > MAX_SPECS { return Err(ProviderFailure::malformed("provider request spec count is outside 1..=4096")); }
        let mut previous: Option<(&str, &str)> = None;
        for spec in &self.specs {
            if !is_hex64(&spec.budget_hash) || spec.metric.is_empty() { return Err(ProviderFailure::malformed("provider request has an invalid budget hash or empty metric")); }
            let key = (spec.metric.as_str(), spec.budget_hash.as_str());
            if previous.is_some_and(|prior| prior >= key) { return Err(ProviderFailure::malformed("provider request specs are not strictly ordered by metric then budget hash")); }
            previous = Some(key);
        }
        Ok(())
    }

    pub fn bytes(&self) -> Result<Vec<u8>, ProviderFailure> {
        self.validate()?;
        let specs = self.specs.iter().map(|spec| CanonicalJson::object([
            ("budget_hash".into(), CanonicalJson::String(spec.budget_hash.clone())),
            ("metric".into(), CanonicalJson::String(spec.metric.clone())),
        ]).expect("fixed keys")).collect();
        Ok(CanonicalJson::object([
            ("context_hash".into(), CanonicalJson::String(self.context_hash.clone())),
            ("policy".into(), self.policy.clone()),
            ("provider_hash".into(), CanonicalJson::String(self.provider_hash.clone())),
            ("request_id".into(), CanonicalJson::String(self.request_id.clone())),
            ("schema".into(), CanonicalJson::String(self.schema.clone())),
            ("specs".into(), CanonicalJson::Array(specs)),
            ("version".into(), CanonicalJson::Integer(self.version.to_string())),
            ("workload".into(), self.workload.clone()),
        ]).expect("fixed keys").bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderEvent {
    Sample { spec: u32, metric: String, value: Rational },
    /// Bounded canonical provenance attached to one sample family. Compile
    /// probes use this extension so the shared provider protocol carries the
    /// exact workload and phase evidence into the shared report path.
    Metadata { spec: u32, details: Vec<(String, String)> },
    Unavailable { spec: u32, reason: String, details: Vec<(String, String)> },
    Complete { request_id: String, samples: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderEvidence { pub events: Vec<ProviderEvent> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass { Unavailable, Malformed, Panic, Timeout, Execution, Incompatible, Unsupported, Unresolved }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderFailure { pub class: FailureClass, pub reason: String }
impl ProviderFailure {
    pub fn malformed(reason: impl Into<String>) -> Self { Self { class: FailureClass::Malformed, reason: reason.into() } }
    fn operation(class: FailureClass, reason: impl Into<String>) -> Self { Self { class, reason: reason.into() } }
    pub fn diagnostic(&self, budget: &str) -> ProviderDiagnostic {
        match self.class {
            FailureClass::Unavailable | FailureClass::Incompatible => ProviderDiagnostic { code: "E2906", what: format!("performance budget {budget} has no usable evidence"), why: self.reason.clone(), fix: "correct the provider evidence or bootstrap only when absent or stale evidence is eligible".into() },
            FailureClass::Unsupported => ProviderDiagnostic { code: "E2903", what: format!("performance budget {budget} is not valid"), why: self.reason.clone(), fix: "use one supported metric and provider pair".into() },
            FailureClass::Unresolved => ProviderDiagnostic { code: "E2905", what: format!("performance budget {budget} cannot resolve provider"), why: self.reason.clone(), fix: "name one registered provider identity".into() },
            _ => ProviderDiagnostic { code: "E2908", what: "performance budget operation failed".into(), why: format!("measurement provider refused the operation: {}", self.reason), fix: "correct the named provider failure and retry the operation".into() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDiagnostic { pub code: &'static str, pub what: String, pub why: String, pub fix: String }
impl ProviderDiagnostic { pub fn render(&self)->String{format!("# Error [{}]: {}\n\nWhat: {}\nWhy: {}\nFix: {}\n",self.code,self.what,self.what,self.why,self.fix)} }

#[derive(Clone)]
pub struct ProviderCancellation { cancelled: Arc<AtomicBool> }
impl ProviderCancellation { pub fn cancelled(&self)->bool{self.cancelled.load(AtomicOrdering::Acquire)} }
type InProcessProvider = fn(&ProviderRequest,&ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure>;
#[derive(Clone)]
enum Provider { InProcess(InProcessProvider), Subprocess(PathBuf), File(PathBuf) }

#[derive(Default)]
pub struct ProviderRegistry { providers: BTreeMap<String, Provider> }
impl ProviderRegistry {
    /// Registry used by `jet budget` for compiler-owned facts and compile probes.
    /// Values travel through the same typed request/stream validation as every
    /// other provider; the registry never consults PATH.
    pub fn with_compiler_facts() -> Self {
        let mut registry = Self::default();
        registry.register_in_process("CompilerFacts", compiler_facts_provider)
            .expect("fixed compiler provider identity");
        registry.register_in_process("CompilerProbe", compiler_latency_provider)
            .expect("fixed compiler probe provider identity");
        registry
    }
    pub fn with_builtins() -> Self {
        let mut registry = Self::with_compiler_facts();
        registry.register_in_process("BuildArtifact", build_artifact_provider)
            .expect("fixed build-artifact provider identity");
        registry
    }
    /// Provider runs in an isolated process group; collection terminates the
    /// entire group at the deadline without waiting on a blocked worker.
    pub fn register_in_process(&mut self, identity: impl Into<String>, provider: InProcessProvider) -> Result<(), String> { self.insert(identity.into(), Provider::InProcess(provider)) }
    pub fn register_subprocess(&mut self, identity: impl Into<String>, executable: PathBuf) -> Result<(), String> {
        if !executable.is_absolute() { return Err("provider executable must be an absolute compiler-resolved path".into()); }
        self.insert(identity.into(), Provider::Subprocess(executable))
    }
    pub fn register_file(&mut self, identity: impl Into<String>, response: PathBuf) -> Result<(), String> {
        if !response.is_absolute() { return Err("provider response must be an absolute compiler-resolved path".into()); }
        self.insert(identity.into(), Provider::File(response))
    }
    fn insert(&mut self, identity: String, provider: Provider) -> Result<(), String> {
        if identity.is_empty() { return Err("provider identity is empty".into()); }
        if self.providers.insert(identity.clone(), provider).is_some() { return Err(format!("duplicate provider identity `{identity}`")); }
        Ok(())
    }
    pub fn collect(&self, identity: &str, request: &ProviderRequest, timeout: Duration) -> Result<ProviderEvidence, ProviderFailure> {
        request.validate()?;
        let provider = self.providers.get(identity).ok_or_else(|| ProviderFailure::operation(FailureClass::Unresolved, format!("provider `{identity}` is unresolved")))?;
        let events = match provider {
            Provider::InProcess(function) => run_in_process(*function, request, timeout, identity)?,
            Provider::Subprocess(path) => decode_stream(&run_subprocess(path, &request.bytes()?, timeout)?, request)?,
            Provider::File(path) => decode_stream(&read_bounded(path, timeout)?, request)?,
        };
        validate_events(events, request)
    }
}

fn compiler_facts_provider(request: &ProviderRequest, _: &ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let CanonicalJson::Array(values) = &request.workload else {
        return Err(ProviderFailure::malformed("CompilerFacts workload is not an ordered sample array"));
    };
    if values.len() != request.specs.len() {
        return Err(ProviderFailure::malformed("CompilerFacts workload/spec count differs"));
    }
    let mut events = Vec::with_capacity(values.len() + 1);
    for (index, value) in values.iter().enumerate() {
        let CanonicalJson::Integer(value) = value else {
            return Err(ProviderFailure::malformed("CompilerFacts sample is not an integer"));
        };
        let value = Rational::parse(value, "1").map_err(ProviderFailure::malformed)?;
        events.push(ProviderEvent::Sample { spec: index as u32, metric: request.specs[index].metric.clone(), value });
    }
    events.push(ProviderEvent::Complete { request_id: request.request_id.clone(), samples: values.len() as u64 });
    Ok(events)
}

fn build_artifact_provider(request: &ProviderRequest, _: &ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let CanonicalJson::Object(workload) = &request.workload else {
        return Err(ProviderFailure::malformed("BuildArtifact workload is not an object"));
    };
    let Some(CanonicalJson::String(path)) = workload.get("path") else {
        return Err(ProviderFailure::malformed("BuildArtifact workload has no artifact path"));
    };
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(ProviderFailure::malformed("BuildArtifact path is not compiler-resolved absolute text"));
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| ProviderFailure::operation(FailureClass::Unavailable, format!("built artifact is unavailable: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProviderFailure::operation(FailureClass::Incompatible, "built artifact is not a regular file"));
    }
    let value = Rational::parse(&metadata.len().to_string(), "1").map_err(ProviderFailure::malformed)?;
    let mut events = Vec::with_capacity(request.specs.len() + 1);
    for (index, spec) in request.specs.iter().enumerate() {
        if !matches!(spec.metric.as_str(), "BinarySize" | "ArtifactSize") {
            return Err(ProviderFailure::operation(FailureClass::Unsupported, format!("BuildArtifact does not support metric `{}`", spec.metric)));
        }
        events.push(ProviderEvent::Sample { spec: index as u32, metric: spec.metric.clone(), value: value.clone() });
    }
    events.push(ProviderEvent::Complete { request_id: request.request_id.clone(), samples: request.specs.len() as u64 });
    Ok(events)
}

const COMPILE_WARMUPS: u128 = 1;
const COMPILE_SAMPLES: usize = 20;
const COMPILE_MAX_PROJECT_BYTES: u64 = 64 * 1024 * 1024;

/// Build the command-owned workload identity. The path is carried only in the
/// request; report identity uses the source and patch digests below.
pub fn compiler_probe_workload(
    project_root: &Path,
    entry: &Path,
    mode: &str,
    target: &str,
    profile: &str,
    patch: Option<&str>,
) -> Result<CanonicalJson, String> {
    let root = project_root.canonicalize().map_err(|error| format!("cannot resolve compile workload root: {error}"))?;
    let entry = entry.canonicalize().map_err(|error| format!("cannot resolve compile workload entry: {error}"))?;
    if !entry.starts_with(&root) { return Err("compile workload entry escapes its project root".into()); }
    let descriptor = compile_descriptor(&root, mode, target, profile, patch)?;
    CanonicalJson::object([
        ("entry".into(), CanonicalJson::String(entry.to_string_lossy().into_owned())),
        ("mode".into(), CanonicalJson::String(mode.into())),
        ("patch".into(), patch.map(|value| CanonicalJson::String(value.into())).unwrap_or(CanonicalJson::Null)),
        ("patch_sha256".into(), CanonicalJson::String(descriptor.patch_sha256)),
        ("profile".into(), CanonicalJson::String(profile.into())),
        ("project_root".into(), CanonicalJson::String(root.to_string_lossy().into_owned())),
        ("samples".into(), CanonicalJson::Integer(COMPILE_SAMPLES.to_string())),
        ("source_tree_sha256".into(), CanonicalJson::String(descriptor.source_tree_sha256)),
        ("target".into(), CanonicalJson::String(target.into())),
        ("warmups".into(), CanonicalJson::Integer(COMPILE_WARMUPS.to_string())),
    ])
}

/// Stable provider version used by the shared context key. It changes when
/// the measured source tree or named patch changes, but never contains time.
pub fn compiler_probe_version(
    project_root: &Path,
    mode: &str,
    target: &str,
    profile: &str,
    patch: Option<&str>,
) -> Result<String, String> {
    let root = project_root.canonicalize().map_err(|error| format!("cannot resolve compile workload root: {error}"))?;
    let descriptor = compile_descriptor(&root, mode, target, profile, patch)?;
    Ok(format!(
        "jet-compile-latency-v1;mode={mode};target={target};profile={profile};backend=rustc;linker={};warmups=1;samples=20;source={};patch={}",
        std::env::var("RUSTC_LINKER").or_else(|_| std::env::var("CC")).unwrap_or_else(|_| "native".into()),
        descriptor.source_tree_sha256, descriptor.patch_sha256
    ))
}

#[derive(Clone)]
struct CompileDescriptor {
    source_tree_sha256: String,
    patch_sha256: String,
}

fn compile_descriptor(root: &Path, mode: &str, target: &str, profile: &str, patch: Option<&str>) -> Result<CompileDescriptor, String> {
    if !matches!(mode, "Clean" | "NoChange" | "Edit") || target.is_empty() || profile.is_empty() {
        return Err("compile workload has an unsupported mode, empty target, or empty profile".into());
    }
    if target != "cli" { return Err(format!("compile workload target `{target}` is unsupported by the resident fixture compiler")); }
    validate_compile_profile(root, profile)?;
    if mode == "Edit" && patch.is_none() { return Err("CompileProbe Edit workload has no patch path".into()); }
    if mode != "Edit" && patch.is_some() { return Err("only an Edit workload may name a patch path".into()); }
    let patch_sha256 = match patch {
        Some(path) if safe_relative_path(path) => {
            let path = root.join(path);
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| format!("compile workload patch is unavailable: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() { return Err("compile workload patch is not a regular file".into()); }
            let bytes = std::fs::read(&path).map_err(|error| format!("cannot read compile workload patch: {error}"))?;
            sha256_hex(&bytes)
        }
        Some(_) => return Err("compile workload patch path is not project-relative".into()),
        None => sha256_hex(&[]),
    };
    Ok(CompileDescriptor { source_tree_sha256: source_tree_digest(root)?, patch_sha256 })
}

fn validate_compile_profile(root: &Path, profile: &str) -> Result<(), String> {
    if matches!(profile, "dev" | "release" | "debug" | "ci" | "small") {
        return Ok(());
    }
    let manifest = root.join("package.jet");
    let raw = std::fs::read_to_string(&manifest).map_err(|_| {
        format!("compile workload profile `{profile}` is unsupported by the resident fixture compiler")
    })?;
    let facts = crate::Package::PackageFacts::parse(&raw, manifest.display().to_string()).map_err(|_| {
        format!("compile workload profile `{profile}` is unsupported by the resident fixture compiler")
    })?;
    if facts.build_profiles.iter().any(|candidate| candidate.name == profile) {
        Ok(())
    } else {
        Err(format!("compile workload profile `{profile}` is unsupported by the resident fixture compiler"))
    }
}

fn compiler_latency_provider(request: &ProviderRequest, _: &ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let CanonicalJson::Object(workload) = &request.workload else {
        return Err(ProviderFailure::malformed("CompilerProbe workload is not an object"));
    };
    let mode = workload_text(workload, "mode")?;
    let target = workload_text(workload, "target")?;
    let profile = workload_text(workload, "profile")?;
    let root = PathBuf::from(workload_text(workload, "project_root")?);
    let entry = PathBuf::from(workload_text(workload, "entry")?);
    let patch = match workload.get("patch") {
        Some(CanonicalJson::Null) => None,
        Some(CanonicalJson::String(value)) => Some(value.as_str()),
        _ => return Err(ProviderFailure::malformed("CompilerProbe patch is not text or null")),
    };
    let warmups = workload_unsigned(workload, "warmups")?;
    let sample_count = workload_unsigned(workload, "samples")?;
    if warmups != COMPILE_WARMUPS || sample_count != COMPILE_SAMPLES as u128 {
        return Err(ProviderFailure::operation(FailureClass::Incompatible, "CompilerProbe workload does not use the pinned warmup/sample policy"));
    }
    if target != "cli" {
        return Err(ProviderFailure::operation(FailureClass::Unsupported, format!("compile workload target `{target}` is unsupported by the resident fixture compiler")));
    }
    for spec in &request.specs {
        if !matches!(spec.metric.as_str(), "CompileTime(P50)" | "CompileTime(P90)" | "CompileTime(P95)" | "CompileTime(P99)" | "CompileTime(P999)") {
            return Err(ProviderFailure::operation(FailureClass::Unsupported, format!("CompilerProbe does not support metric `{}`", spec.metric)));
        }
    }
    let root = root.canonicalize().map_err(|error| ProviderFailure::operation(FailureClass::Unavailable, format!("compile workload root is unavailable: {error}")))?;
    let entry = entry.canonicalize().map_err(|error| ProviderFailure::operation(FailureClass::Unavailable, format!("compile workload entry is unavailable: {error}")))?;
    if !entry.starts_with(&root) { return Err(ProviderFailure::operation(FailureClass::Incompatible, "compile workload entry escapes its project root")); }
    let descriptor = compile_descriptor(&root, mode.as_str(), target.as_str(), profile.as_str(), patch).map_err(ProviderFailure::malformed)?;
    let source_tree_sha256 = workload_text(workload, "source_tree_sha256")?;
    let patch_sha256 = workload_text(workload, "patch_sha256")?;
    if source_tree_sha256.as_str() != descriptor.source_tree_sha256.as_str() || patch_sha256.as_str() != descriptor.patch_sha256.as_str() {
        return Err(ProviderFailure::operation(FailureClass::Incompatible, "compile workload source or patch identity is stale or forged"));
    }
    let scratch = std::env::temp_dir().join(format!(
        "jet-compile-latency-{}-{}-{}",
        std::process::id(),
        COMPILE_WORKLOAD_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed),
        &request.request_id[..12]
    ));
    let result = compile_latency_samples(&root, &entry, &scratch, mode.as_str(), patch, &descriptor.source_tree_sha256, &descriptor.patch_sha256, target.as_str(), profile.as_str(), warmups as usize, sample_count as usize, &request.specs, &request.request_id);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn compile_latency_samples(
    root: &Path,
    entry: &Path,
    scratch: &Path,
    mode: &str,
    patch: Option<&str>,
    expected_source_tree_sha256: &str,
    expected_patch_sha256: &str,
    target: &str,
    profile: &str,
    warmups: usize,
    sample_count: usize,
    specs: &[ProviderSpec],
    request_id: &str,
) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let relative_entry = entry.strip_prefix(root).map_err(|_| ProviderFailure::malformed("compile workload entry is outside its project root"))?;
    let workload_bytes = source_tree_bytes(root).map_err(ProviderFailure::malformed)?;
    let edit_bytes_data = patch.map(|path| std::fs::read(root.join(path))).transpose().map_err(|error| ProviderFailure::malformed(format!("cannot read edit bytes: {error}")))?;
    let edit_bytes = edit_bytes_data.as_ref().map(|bytes| bytes.len() as u128).unwrap_or(0);
    let edit_sha256 = edit_bytes_data.as_deref().map(sha256_hex).unwrap_or_else(|| sha256_hex(&[]));
    if edit_sha256.as_str() != expected_patch_sha256 {
        return Err(ProviderFailure::operation(FailureClass::Incompatible, "compile workload patch changed while it was copied"));
    }
    let compiler_digest = env!("JET_COMPILER_BUILD_ID").to_string();
    let core_digest = env!("JET_STDLIB_BUILD_ID").to_string();
    let host = format!("{}:{}", std::env::consts::OS, env!("JET_BUILD_TARGET"));
    let backend = "rustc";
    let linker = std::env::var("RUSTC_LINKER").or_else(|_| std::env::var("CC")).unwrap_or_else(|_| "native".into());
    let mut sample_values = Vec::with_capacity(sample_count);
    let mut sample_records = Vec::with_capacity(sample_count);
    let edit_patch = if mode == "Edit" {
        Some(patch.ok_or_else(|| ProviderFailure::malformed("Edit workload has no patch path"))?)
    } else {
        None
    };
    if mode == "Edit" {
        std::fs::create_dir_all(scratch).map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot create edit workload scratch directory: {error}")))?;
    } else {
        copy_compile_project(root, scratch).map_err(ProviderFailure::malformed)?;
        let copied_source_tree_sha256 = source_tree_digest(scratch).map_err(ProviderFailure::malformed)?;
        if copied_source_tree_sha256.as_str() != expected_source_tree_sha256 {
            return Err(ProviderFailure::operation(FailureClass::Incompatible, "compile workload source changed while it was copied"));
        }
    }
    for _ in 0..warmups {
        if mode == "Edit" {
            let trial = scratch.join("warmup");
            let _ = compile_edit_trial(root, relative_entry, &trial, edit_patch.ok_or_else(|| ProviderFailure::malformed("Edit workload has no patch path"))?, expected_source_tree_sha256, expected_patch_sha256, target, profile)?;
        } else {
            let scratch_entry = scratch.join(relative_entry);
            if mode == "Clean" { reset_compile_cache(scratch)?; }
            clear_compile_timing(scratch)?;
            run_compile_child(&scratch_entry, scratch, target, profile)?;
        }
    }
    for sample_index in 0..sample_count {
        let (elapsed_ns, phase_totals) = if mode == "Edit" {
            let trial = scratch.join(format!("sample-{sample_index}"));
            compile_edit_trial(root, relative_entry, &trial, edit_patch.ok_or_else(|| ProviderFailure::malformed("Edit workload has no patch path"))?, expected_source_tree_sha256, expected_patch_sha256, target, profile)?
        } else {
            let scratch_entry = scratch.join(relative_entry);
            if mode == "Clean" { reset_compile_cache(scratch)?; }
            clear_compile_timing(scratch)?;
            let started = Instant::now();
            run_compile_child(&scratch_entry, scratch, target, profile)?;
            (started.elapsed().as_nanos(), read_compile_phases(scratch)?)
        };
        sample_values.push(Rational::parse(&elapsed_ns.to_string(), "1").map_err(ProviderFailure::malformed)?);
        sample_records.push(CanonicalJson::object([
            ("backend".into(), CanonicalJson::String(backend.into())),
            ("cache_state".into(), CanonicalJson::String(mode.into())),
            ("compiler_digest".into(), CanonicalJson::String(compiler_digest.clone())),
            ("core_digest".into(), CanonicalJson::String(core_digest.clone())),
            ("edit_bytes".into(), CanonicalJson::Integer(edit_bytes.to_string())),
            ("elapsed_ns".into(), CanonicalJson::Integer(elapsed_ns.to_string())),
            ("host".into(), CanonicalJson::String(host.clone())),
            ("linker".into(), CanonicalJson::String(linker.clone())),
            ("phase_totals".into(), phase_totals_json(&phase_totals)?),
            ("profile".into(), CanonicalJson::String(profile.into())),
            ("source_tree_sha256".into(), CanonicalJson::String(expected_source_tree_sha256.into())),
            ("target".into(), CanonicalJson::String(target.into())),
            ("workload_bytes".into(), CanonicalJson::Integer(workload_bytes.to_string())),
        ]).map_err(ProviderFailure::malformed)?);
    }
    let mean = sample_values.iter().try_fold(Rational::zero(), |sum, value| sum.add(value)).map_err(ProviderFailure::malformed)?.div(&Rational::integer(sample_values.len() as i128)).map_err(ProviderFailure::malformed)?;
    let variance = sample_values.iter().try_fold(Rational::zero(), |sum, value| {
        let delta = value.sub(&mean)?;
        sum.add(&delta.mul(&delta)?)
    }).map_err(ProviderFailure::malformed)?.div(&Rational::integer(sample_values.len() as i128)).map_err(ProviderFailure::malformed)?;
    let aggregate_phases = aggregate_compile_phases(&sample_records)?;
    let metadata = CanonicalJson::object([
        ("backend".into(), CanonicalJson::String(backend.into())),
        ("cache_state".into(), CanonicalJson::String(mode.into())),
        ("compiler_digest".into(), CanonicalJson::String(compiler_digest)),
        ("core_digest".into(), CanonicalJson::String(core_digest)),
        ("edit_bytes".into(), CanonicalJson::Integer(edit_bytes.to_string())),
        ("edit_sha256".into(), CanonicalJson::String(edit_sha256)),
        ("host".into(), CanonicalJson::String(host)),
        ("linker".into(), CanonicalJson::String(linker)),
        ("phase_totals".into(), aggregate_phases),
        ("profile".into(), CanonicalJson::String(profile.into())),
        ("sample_records".into(), CanonicalJson::Array(sample_records)),
        ("samples".into(), CanonicalJson::Integer(sample_count.to_string())),
        ("source_tree_sha256".into(), CanonicalJson::String(expected_source_tree_sha256.into())),
        ("target".into(), CanonicalJson::String(target.into())),
        ("variance".into(), variance.to_json()),
        ("warmups".into(), CanonicalJson::Integer(warmups.to_string())),
        ("workload_bytes".into(), CanonicalJson::Integer(workload_bytes.to_string())),
    ]).map_err(ProviderFailure::malformed)?;
    let metadata = String::from_utf8(metadata.bytes()).map_err(|_| ProviderFailure::malformed("compile metadata is not UTF-8"))?;
    let mut events = Vec::with_capacity(sample_count.saturating_mul(specs.len()).saturating_add(specs.len() + 1));
    for (spec, request_spec) in specs.iter().enumerate() {
        for value in &sample_values {
            events.push(ProviderEvent::Sample { spec: spec as u32, metric: request_spec.metric.clone(), value: value.clone() });
        }
        events.push(ProviderEvent::Metadata { spec: spec as u32, details: vec![("compile".into(), metadata.clone())] });
    }
    events.push(ProviderEvent::Complete { request_id: request_id.into(), samples: (sample_count * specs.len()) as u64 });
    Ok(events)
}

fn compile_edit_trial(
    root: &Path,
    relative_entry: &Path,
    trial: &Path,
    patch: &str,
    expected_source_tree_sha256: &str,
    expected_patch_sha256: &str,
    target: &str,
    profile: &str,
) -> Result<(u128, Vec<(String, u128)>), ProviderFailure> {
    let result = (|| {
        copy_compile_project(root, trial).map_err(ProviderFailure::malformed)?;
        let copied_source_tree_sha256 = source_tree_digest(trial).map_err(ProviderFailure::malformed)?;
        if copied_source_tree_sha256.as_str() != expected_source_tree_sha256 {
            return Err(ProviderFailure::operation(FailureClass::Incompatible, "compile workload source changed while it was copied"));
        }
        let patch_bytes = std::fs::read(trial.join(patch)).map_err(|error| ProviderFailure::operation(FailureClass::Incompatible, format!("compile workload patch changed while it was copied: {error}")))?;
        if sha256_hex(&patch_bytes).as_str() != expected_patch_sha256 {
            return Err(ProviderFailure::operation(FailureClass::Incompatible, "compile workload patch changed while it was copied"));
        }
        reset_compile_cache(trial)?;
        let entry = trial.join(relative_entry);
        run_compile_child(&entry, trial, target, profile)?;
        apply_unified_patch(trial, patch).map_err(ProviderFailure::malformed)?;
        clear_compile_timing(trial)?;
        let started = Instant::now();
        run_compile_child(&entry, trial, target, profile)?;
        Ok((started.elapsed().as_nanos(), read_compile_phases(trial)?))
    })();
    let _ = std::fs::remove_dir_all(trial);
    result
}

fn workload_text(fields: &BTreeMap<String, CanonicalJson>, key: &str) -> Result<String, ProviderFailure> {
    match fields.get(key) {
        Some(CanonicalJson::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(ProviderFailure::malformed(format!("CompilerProbe workload has no nonempty `{key}` text"))),
    }
}

fn workload_unsigned(fields: &BTreeMap<String, CanonicalJson>, key: &str) -> Result<u128, ProviderFailure> {
    let value = match fields.get(key) {
        Some(CanonicalJson::Integer(value)) => value,
        _ => return Err(ProviderFailure::malformed(format!("CompilerProbe workload `{key}` is not a canonical unsigned integer"))),
    };
    let parsed = value.parse::<u128>().map_err(|_| ProviderFailure::malformed(format!("CompilerProbe workload `{key}` is not a canonical unsigned integer")))?;
    if parsed.to_string() != value.as_str() { return Err(ProviderFailure::malformed(format!("CompilerProbe workload `{key}` is not a canonical unsigned integer"))); }
    Ok(parsed)
}

fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn source_files(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    fn visit(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
        let mut entries = std::fs::read_dir(dir).map_err(|error| format!("cannot read compile workload directory: {error}"))?.collect::<Result<Vec<_>, _>>().map_err(|error| format!("cannot enumerate compile workload directory: {error}"))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".jet" | ".jet-compile-cache" | "build" | "target" | ".git") { continue; }
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| format!("cannot inspect compile workload path: {error}"))?;
            if metadata.file_type().is_symlink() { return Err(format!("compile workload contains a symlink: {}", path.display())); }
            if metadata.is_dir() {
                visit(root, &path, files)?;
            } else if metadata.is_file() {
                let relative = path.strip_prefix(root).map_err(|_| "compile workload path escaped its root".to_string())?.to_string_lossy().replace('\\', "/");
                if relative == "package.jet" || relative.ends_with(".jet") {
                    files.push((relative, path));
                }
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    if files.is_empty() { return Err("compile workload has no package or Jet source files".into()); }
    Ok(files)
}

fn source_tree_digest(root: &Path) -> Result<String, String> {
    let files = source_files(root)?;
    let mut frame = Vec::new();
    for (relative, path) in files {
        let bytes = std::fs::read(path).map_err(|error| format!("cannot read compile workload source: {error}"))?;
        frame.extend_from_slice(&(relative.len() as u64).to_be_bytes());
        frame.extend_from_slice(relative.as_bytes());
        frame.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        frame.extend_from_slice(&bytes);
    }
    Ok(sha256_hex(&frame))
}

fn source_tree_bytes(root: &Path) -> Result<u128, String> {
    source_files(root)?.into_iter().try_fold(0u128, |total, (_, path)| {
        let bytes = std::fs::metadata(path).map_err(|error| format!("cannot inspect compile workload source: {error}"))?.len() as u128;
        total.checked_add(bytes).ok_or_else(|| "compile workload source byte count overflowed".to_string())
    })
}

fn copy_compile_project(source: &Path, destination: &Path) -> Result<(), String> {
    fn copy_dir(source: &Path, destination: &Path, total: &mut u64) -> Result<(), String> {
        std::fs::create_dir_all(destination).map_err(|error| format!("cannot create compile workload scratch directory: {error}"))?;
        let mut entries = std::fs::read_dir(source).map_err(|error| format!("cannot read compile workload project: {error}"))?.collect::<Result<Vec<_>, _>>().map_err(|error| format!("cannot enumerate compile workload project: {error}"))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".jet" | ".jet-compile-cache" | "build" | "target" | ".git") { continue; }
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| format!("cannot inspect compile workload project path: {error}"))?;
            if metadata.file_type().is_symlink() { return Err(format!("compile workload contains a symlink: {}", path.display())); }
            let target = destination.join(entry.file_name());
            if metadata.is_dir() {
                copy_dir(&path, &target, total)?;
            } else if metadata.is_file() {
                *total = total.checked_add(metadata.len()).ok_or_else(|| "compile workload exceeds its byte limit".to_string())?;
                if *total > COMPILE_MAX_PROJECT_BYTES { return Err("compile workload exceeds the 64 MiB project limit".into()); }
                std::fs::copy(&path, &target).map_err(|error| format!("cannot copy compile workload input: {error}"))?;
            }
        }
        Ok(())
    }
    let mut total = 0;
    copy_dir(source, destination, &mut total)
}

fn reset_compile_cache(root: &Path) -> Result<(), ProviderFailure> {
    for cache in [root.join("build"), root.join(".jet-compile-cache")] {
        match std::fs::remove_dir_all(cache) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(ProviderFailure::operation(FailureClass::Execution, format!("cannot reset compile workload cache: {error}"))),
        }
    }
    Ok(())
}

fn clear_compile_timing(root: &Path) -> Result<(), ProviderFailure> {
    for path in [root.join("jet-timing.json"), root.join("build/jet-timing-backend.json")] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(ProviderFailure::operation(FailureClass::Execution, format!("cannot clear compile timing artifact: {error}"))),
        }
    }
    Ok(())
}

fn apply_unified_patch(root: &Path, patch: &str) -> Result<(), String> {
    if !safe_relative_path(patch) { return Err("compile workload patch path is not project-relative".into()); }
    let patch_bytes = std::fs::read(root.join(patch)).map_err(|error| format!("cannot read compile workload patch: {error}"))?;
    let patch_text = std::str::from_utf8(&patch_bytes).map_err(|_| "compile workload patch is not UTF-8".to_string())?;
    let lines = patch_text.lines().collect::<Vec<_>>();
    let old_header = lines.iter().find(|line| line.starts_with("--- ")).ok_or("compile workload patch has no old-file header")?;
    let new_header = lines.iter().find(|line| line.starts_with("+++ ")).ok_or("compile workload patch has no new-file header")?;
    let old_path = patch_header_path(old_header)?;
    let new_path = patch_header_path(new_header)?;
    if old_path != new_path { return Err("compile workload patch changes more than one file".into()); }
    let hunk_index = lines.iter().position(|line| line.starts_with("@@")).ok_or("compile workload patch has no hunk")?;
    let mut header = lines[hunk_index].split_whitespace();
    if header.next() != Some("@@") { return Err("compile workload patch has an invalid hunk header".into()); }
    let parse_range = |value: &str| -> Result<(usize, usize), String> {
        let value = value.get(1..).ok_or("compile workload patch has an invalid hunk range")?;
        let (start, count) = value.split_once(',').map_or((value, "1"), |(start, count)| (start, count));
        let start = start.parse::<usize>().map_err(|_| "compile workload patch has an invalid hunk start".to_string())?;
        let count = count.parse::<usize>().map_err(|_| "compile workload patch has an invalid hunk count".to_string())?;
        if start == 0 { return Err("compile workload patch has a zero hunk start".into()); }
        Ok((start, count))
    };
    let (old_start, old_count) = parse_range(header.next().ok_or("compile workload patch has no old hunk range")?)?;
    let (_, new_count) = parse_range(header.next().ok_or("compile workload patch has no new hunk range")?)?;
    let target = root.join(&new_path);
    let metadata = std::fs::symlink_metadata(&target).map_err(|error| format!("compile workload patch target is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() { return Err("compile workload patch target is not a regular file".into()); }
    let original = std::fs::read_to_string(&target).map_err(|error| format!("cannot read compile workload patch target: {error}"))?;
    let trailing_newline = original.ends_with('\n');
    let current = original.lines().map(str::to_owned).collect::<Vec<_>>();
    let old_start = old_start - 1;
    if old_start > current.len() { return Err("compile workload patch hunk starts outside the source tree".into()); }
    let mut output = current[..old_start].to_vec();
    let mut cursor = old_start;
    let mut seen_old = 0usize;
    let mut seen_new = 0usize;
    for line in &lines[hunk_index + 1..] {
        if line.starts_with("@@") { return Err("compile workload patch must contain exactly one hunk".into()); }
        if line.starts_with("\\ No newline") { continue; }
        let (kind, body) = line.split_at(1);
        match kind {
            " " => {
                if current.get(cursor).map(String::as_str) != Some(body) { return Err("compile workload patch context does not match the source tree".into()); }
                output.push(body.to_string());
                cursor += 1;
                seen_old += 1;
                seen_new += 1;
            }
            "-" => {
                if current.get(cursor).map(String::as_str) != Some(body) { return Err("compile workload patch removal does not match the source tree".into()); }
                cursor += 1;
                seen_old += 1;
            }
            "+" => {
                output.push(body.to_string());
                seen_new += 1;
            }
            _ => return Err("compile workload patch contains an invalid hunk line".into()),
        }
    }
    if seen_old != old_count || seen_new != new_count { return Err("compile workload patch hunk counts do not match its body".into()); }
    output.extend_from_slice(&current[cursor..]);
    let mut rewritten = output.join("\n");
    if trailing_newline { rewritten.push('\n'); }
    std::fs::write(target, rewritten).map_err(|error| format!("cannot apply compile workload patch: {error}"))
}

fn patch_header_path(header: &str) -> Result<String, String> {
    let value = header[4..].split('\t').next().unwrap_or_default();
    let value = value.strip_prefix("a/").or_else(|| value.strip_prefix("b/")).unwrap_or(value);
    if !safe_relative_path(value) { return Err("compile workload patch header has an unsafe path".into()); }
    Ok(value.into())
}

fn run_compile_child(entry: &Path, root: &Path, target: &str, profile: &str) -> Result<(), ProviderFailure> {
    if target != "cli" {
        return Err(ProviderFailure::operation(FailureClass::Unsupported, format!("compile workload target `{target}` is unsupported by the resident fixture compiler")));
    }
    let executable = std::env::current_exe().map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot identify resident Jet compiler: {error}")))?;
    let mut command = Command::new(executable);
    command.arg("build").arg(entry).current_dir(root).env("JET_TIMING", "1").env("JET_CACHE_DIR", root.join(".jet-compile-cache")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match profile {
        "dev" => {}
        "release" => { command.arg("--release"); }
        "small" => { command.arg("--small"); }
        name => { command.arg(format!("--profile={name}")); }
    }
    #[cfg(unix)] { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn().map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot start compile workload build: {error}")))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(ProviderFailure::operation(FailureClass::Execution, format!("compile workload build exited with {status}"))),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            Ok(None) => { terminate_group(&mut child); return Err(ProviderFailure::operation(FailureClass::Timeout, "compile workload build exceeded its deadline")); }
            Err(error) => { terminate_group(&mut child); return Err(ProviderFailure::operation(FailureClass::Execution, format!("cannot supervise compile workload build: {error}"))); }
        }
    }
}

fn read_compile_phases(root: &Path) -> Result<Vec<(String, u128)>, ProviderFailure> {
    let mut phases = BTreeMap::<String, u128>::new();
    for path in [root.join("jet-timing.json"), root.join("build/jet-timing-backend.json")] {
        let bytes = std::fs::read(&path).map_err(|error| ProviderFailure::operation(FailureClass::Unavailable, format!("compile timing artifact `{}` is unavailable: {error}", path.display())))?;
        let value = CanonicalJson::parse_canonical(&bytes).map_err(ProviderFailure::malformed)?;
        let CanonicalJson::Object(fields) = value else { return Err(ProviderFailure::malformed("compile timing artifact is not an object")); };
        let Some(CanonicalJson::Array(entries)) = fields.get("phases") else { return Err(ProviderFailure::malformed("compile timing artifact has no phases")); };
        for entry in entries {
            let CanonicalJson::Object(entry) = entry else { return Err(ProviderFailure::malformed("compile timing phase is not an object")); };
            let Some(CanonicalJson::String(name)) = entry.get("name") else { return Err(ProviderFailure::malformed("compile timing phase has no name")); };
            if name == "rust_bytes" { continue; }
            let Some(CanonicalJson::Integer(us)) = entry.get("us") else { return Err(ProviderFailure::malformed("compile timing phase has no duration")); };
            let ns = us.parse::<u128>().map_err(|_| ProviderFailure::malformed("compile timing duration is not an unsigned integer"))?.checked_mul(1_000).ok_or_else(|| ProviderFailure::malformed("compile timing duration overflowed"))?;
            let slot = phases.entry(name.clone()).or_default();
            *slot = slot.checked_add(ns).ok_or_else(|| ProviderFailure::malformed("compile phase total overflowed"))?;
        }
    }
    if phases.is_empty() { return Err(ProviderFailure::operation(FailureClass::Unavailable, "compile timing artifacts contained no phase totals")); }
    Ok(phases.into_iter().collect())
}

fn phase_totals_json(phases: &[(String, u128)]) -> Result<CanonicalJson, ProviderFailure> {
    Ok(CanonicalJson::Array(phases.iter().map(|(name, ns)| CanonicalJson::object([
        ("name".into(), CanonicalJson::String(name.clone())),
        ("ns".into(), CanonicalJson::Integer(ns.to_string())),
    ]).map_err(ProviderFailure::malformed)).collect::<Result<Vec<_>, _>>()?))
}

fn aggregate_compile_phases(records: &[CanonicalJson]) -> Result<CanonicalJson, ProviderFailure> {
    let mut totals = BTreeMap::<String, u128>::new();
    for record in records {
        let CanonicalJson::Object(record) = record else { return Err(ProviderFailure::malformed("compile sample record is not an object")); };
        let Some(CanonicalJson::Array(phases)) = record.get("phase_totals") else { return Err(ProviderFailure::malformed("compile sample record has no phase totals")); };
        for phase in phases {
            let CanonicalJson::Object(phase) = phase else { return Err(ProviderFailure::malformed("compile phase total is not an object")); };
            let Some(CanonicalJson::String(name)) = phase.get("name") else { return Err(ProviderFailure::malformed("compile phase total has no name")); };
            let Some(CanonicalJson::Integer(ns)) = phase.get("ns") else { return Err(ProviderFailure::malformed("compile phase total has no duration")); };
            let ns = ns.parse::<u128>().map_err(|_| ProviderFailure::malformed("compile phase total is not an unsigned integer"))?;
            let slot = totals.entry(name.clone()).or_default();
            *slot = slot.checked_add(ns).ok_or_else(|| ProviderFailure::malformed("compile phase total overflowed"))?;
        }
    }
    phase_totals_json(&totals.into_iter().collect::<Vec<_>>())
}

fn run_in_process(function: InProcessProvider, request: &ProviderRequest, timeout: Duration, identity: &str) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    #[cfg(target_os="linux")]{let cancellation=ProviderCancellation{cancelled:Arc::new(AtomicBool::new(false))};let bytes=run_isolated_bytes(timeout,&format!("provider `{identity}`"),move||std::panic::catch_unwind(std::panic::AssertUnwindSafe(||function(request,&cancellation))).map_err(|_|ProviderFailure::operation(FailureClass::Panic,"in-process provider failed unexpectedly"))?.map(|events|encode_stream(&events)))?;decode_stream(&bytes,request)}
    #[cfg(not(target_os="linux"))]{let _=(function,request,timeout,identity);Err(ProviderFailure::operation(FailureClass::Execution,"bounded in-process providers are enabled only on Linux"))}
}

fn read_bounded(path: &Path, timeout: Duration) -> Result<Vec<u8>, ProviderFailure> {
    #[cfg(target_os="linux")]{read_bounded_isolated(path,timeout)}
    #[cfg(not(target_os="linux"))]{let _=(path,timeout);Err(ProviderFailure::operation(FailureClass::Execution,"isolated provider file reads are enabled only on Linux"))}
}

#[cfg(target_os="linux")]
fn read_bounded_isolated(path:&Path,timeout:Duration)->Result<Vec<u8>,ProviderFailure>{
    use std::ffi::CString;use std::os::unix::ffi::OsStrExt;
    let name=CString::new(path.as_os_str().as_bytes()).map_err(|_|ProviderFailure::operation(FailureClass::Execution,"provider response path contains NUL"))?;
    #[repr(C)]struct StatxTimestamp{sec:i64,nsec:u32,reserved:i32}#[repr(C)]struct Statx{mask:u32,blksize:u32,attributes:u64,nlink:u32,uid:u32,gid:u32,mode:u16,spare0:u16,ino:u64,size:u64,blocks:u64,attributes_mask:u64,atime:StatxTimestamp,btime:StatxTimestamp,ctime:StatxTimestamp,mtime:StatxTimestamp,rdev_major:u32,rdev_minor:u32,dev_major:u32,dev_minor:u32,mnt_id:u64,dio_mem_align:u32,dio_offset_align:u32,spare3:[u64;12]}
    const O_RDONLY:i32=0;const O_NONBLOCK:i32=0o4000;const O_CLOEXEC:i32=0o2000000;const O_NOFOLLOW:i32=0o400000;const AT_EMPTY_PATH:i32=0x1000;const STATX_TYPE:u32=1;const S_IFMT:u16=0o170000;const S_IFREG:u16=0o100000;
    extern "C"{fn open(path:*const i8,flags:i32,...)->i32;fn read(fd:i32,buffer:*mut u8,count:usize)->isize;fn close(fd:i32)->i32;fn statx(fd:i32,path:*const i8,flags:i32,mask:u32,stat:*mut Statx)->i32;#[cfg(test)]fn usleep(micros:u32)->i32;}
    #[cfg(test)]let _active=ActiveFileReader::new();
    run_isolated_bytes(timeout,&format!("provider response {}",path.display()),move||unsafe{#[cfg(test)]{let delay=FILE_READER_DELAY_MS.load(AtomicOrdering::Relaxed);if delay>0{usleep((delay.min(u32::MAX as u64)*1000)as u32);}}let fd=open(name.as_ptr(),O_RDONLY|O_NONBLOCK|O_CLOEXEC|O_NOFOLLOW);if fd<0{return Err(ProviderFailure::operation(FailureClass::Execution,"cannot open provider response without following links"))}let empty=b"\0";let mut info:Statx=std::mem::zeroed();if statx(fd,empty.as_ptr()as*const i8,AT_EMPTY_PATH,STATX_TYPE,&mut info)!=0||info.mode&S_IFMT!=S_IFREG{close(fd);return Err(ProviderFailure::operation(FailureClass::Execution,"provider response is not a regular file"))}let mut bytes=Vec::new();let mut buffer=[0u8;8192];loop{let count=read(fd,buffer.as_mut_ptr(),buffer.len());if count<0{close(fd);return Err(ProviderFailure::operation(FailureClass::Execution,"cannot read provider response"))}if count==0{break}bytes.extend_from_slice(&buffer[..count as usize]);if bytes.len()>MAX_BYTES{close(fd);return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"))}}close(fd);Ok(bytes)})
}

#[cfg(target_os="linux")]
fn run_isolated_bytes<F>(timeout:Duration,label:&str,work:F)->Result<Vec<u8>,ProviderFailure>where F:FnOnce()->Result<Vec<u8>,ProviderFailure>{
    const O_NONBLOCK:i32=0o4000;const F_SETFL:i32=4;const WNOHANG:i32=1;const SIGKILL:i32=9;const PR_SET_PDEATHSIG:i32=1;
    extern "C"{fn pipe(fds:*mut i32)->i32;fn fork()->i32;fn close(fd:i32)->i32;fn read(fd:i32,buffer:*mut u8,count:usize)->isize;fn write(fd:i32,buffer:*const u8,count:usize)->isize;fn fcntl(fd:i32,command:i32,...)->i32;fn waitpid(pid:i32,status:*mut i32,options:i32)->i32;fn kill(pid:i32,signal:i32)->i32;fn setpgid(pid:i32,pgid:i32)->i32;fn getpid()->i32;fn getppid()->i32;fn prctl(option:i32,arg2:usize,arg3:usize,arg4:usize,arg5:usize)->i32;fn _exit(status:i32)->!;}
    fn class_byte(class:FailureClass)->u8{match class{FailureClass::Unavailable=>0,FailureClass::Malformed=>1,FailureClass::Panic=>2,FailureClass::Timeout=>3,FailureClass::Execution=>4,FailureClass::Incompatible=>5,FailureClass::Unsupported=>6,FailureClass::Unresolved=>7}}
    fn byte_class(value:u8)->Option<FailureClass>{Some(match value{0=>FailureClass::Unavailable,1=>FailureClass::Malformed,2=>FailureClass::Panic,3=>FailureClass::Timeout,4=>FailureClass::Execution,5=>FailureClass::Incompatible,6=>FailureClass::Unsupported,7=>FailureClass::Unresolved,_=>return None})}
    let supervisor=unsafe{getpid()};let mut pipes=[-1,-1];if unsafe{pipe(pipes.as_mut_ptr())}!=0{return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot create isolated worker pipe: {}",std::io::Error::last_os_error())))}let pid=unsafe{fork()};if pid<0{unsafe{close(pipes[0]);close(pipes[1]);}return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot isolate {label}: {}",std::io::Error::last_os_error())))}
    if pid==0{unsafe{if prctl(PR_SET_PDEATHSIG,SIGKILL as usize,0,0,0)!=0||getppid()!=supervisor{_exit(1)}close(pipes[0]);setpgid(0,0);close(2);let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|_|ProviderFailure::operation(FailureClass::Panic,"isolated worker panicked")).and_then(|value|value);let frame=match result{Ok(bytes)=>{let mut frame=Vec::with_capacity(bytes.len()+1);frame.push(0);frame.extend_from_slice(&bytes);frame},Err(failure)=>{let mut frame=Vec::with_capacity(failure.reason.len()+2);frame.push(1);frame.push(class_byte(failure.class));frame.extend_from_slice(failure.reason.as_bytes());frame}};let mut at=0;while at<frame.len(){let sent=write(pipes[1],frame[at..].as_ptr(),frame.len()-at);if sent<=0{break}at+=sent as usize}close(pipes[1]);_exit(0)}}
    #[cfg(test)]let _active=ActiveIsolatedWorker::new();unsafe{close(pipes[1]);setpgid(pid,pid);fcntl(pipes[0],F_SETFL,O_NONBLOCK);}#[cfg(test)]LAST_ISOLATED_GROUP.store(pid as u64,AtomicOrdering::SeqCst);let deadline=Instant::now()+timeout;let mut frame=Vec::new();let mut worker_reaped=false;loop{let mut buffer=[0u8;8192];let count=unsafe{read(pipes[0],buffer.as_mut_ptr(),buffer.len())};if count>0{frame.extend_from_slice(&buffer[..count as usize]);continue}if count==0{unsafe{close(pipes[0]);}break}if !worker_reaped{let mut status=0;let waited=unsafe{waitpid(pid,&mut status,WNOHANG)};if waited==pid{worker_reaped=true}}if Instant::now()>=deadline{unsafe{kill(-pid,SIGKILL);if !worker_reaped{let mut status=0;waitpid(pid,&mut status,0);}close(pipes[0]);}return Err(ProviderFailure::operation(FailureClass::Timeout,format!("{label} timed out and its isolated process group was terminated")))}std::thread::sleep(Duration::from_millis(1));}if !worker_reaped{let mut status=0;unsafe{waitpid(pid,&mut status,0);}}
    match frame.first().copied(){Some(0)=>Ok(frame[1..].to_vec()),Some(1)=>{let class=frame.get(1).copied().and_then(byte_class).ok_or_else(||ProviderFailure::operation(FailureClass::Execution,"isolated worker returned an invalid failure class"))?;let reason=String::from_utf8(frame[2..].to_vec()).map_err(|_|ProviderFailure::operation(FailureClass::Execution,"isolated worker returned non-UTF-8 failure text"))?;Err(ProviderFailure::operation(class,reason))},_=>Err(ProviderFailure::operation(FailureClass::Execution,format!("{label} exited without a result")))}
}

fn run_subprocess(path: &Path, request: &[u8], timeout: Duration) -> Result<Vec<u8>, ProviderFailure> {
    let mut command = Command::new(path);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)] { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn().map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot launch provider {}: {error}", path.display())))?;
    let mut stdin = child.stdin.take().ok_or_else(|| ProviderFailure::operation(FailureClass::Execution, "provider stdin was unavailable"))?; let request=request.to_vec();
    let (writer_tx,writer_rx)=mpsc::sync_channel(1);std::thread::spawn(move || {let _=writer_tx.send(stdin.write_all(&request));});let stdout=child.stdout.take().ok_or_else(|| ProviderFailure::operation(FailureClass::Execution, "provider stdout was unavailable"))?;
    let (tx,rx)=mpsc::channel();std::thread::spawn(move || { let mut bytes=Vec::new();let result=stdout.take((MAX_BYTES+1) as u64).read_to_end(&mut bytes).map(|_|bytes);let _=tx.send(result); });
    let deadline=Instant::now()+timeout;loop { match child.try_wait() { Ok(Some(status)) => { if !status.success(){terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Execution,format!("provider exited with {status}")));}let remaining=deadline.saturating_duration_since(Instant::now());let bytes=rx.recv_timeout(remaining).map_err(|_|{terminate_group(&mut child);ProviderFailure::operation(FailureClass::Timeout,"provider stdout did not close before deadline")})?.map_err(|e|ProviderFailure::operation(FailureClass::Execution,format!("cannot read provider stdout: {e}")))?;writer_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|{terminate_group(&mut child);ProviderFailure::operation(FailureClass::Timeout,"provider stdin did not close before deadline")})?.map_err(|e|ProviderFailure::operation(FailureClass::Execution,format!("cannot write provider stdin: {e}")))?;if bytes.len()>MAX_BYTES{return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"));}return Ok(bytes); },Ok(None) if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(2)),Ok(None)=>{terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Timeout,"provider timed out and was terminated"));},Err(error)=>{terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot supervise provider: {error}")))} } }
}

#[doc(hidden)]
pub fn terminate_group(child: &mut std::process::Child) {
    #[cfg(unix)] { unsafe { extern "C" { fn kill(pid: i32, signal: i32) -> i32; } let _ = kill(-(child.id() as i32), 9); } }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn encode_stream(events: &[ProviderEvent]) -> Vec<u8> { let mut out=MAGIC.to_vec();for event in events{match event{ProviderEvent::Sample{spec,metric,value}=>{out.push(1);put_u32(&mut out,*spec);put_text(&mut out,metric);put_text(&mut out,&value.num.to_string());put_text(&mut out,&value.den.to_string());},ProviderEvent::Unavailable{spec,reason,details}=>{out.push(2);put_u32(&mut out,*spec);put_text(&mut out,reason);put_u32(&mut out,details.len() as u32);for(k,v)in details{put_text(&mut out,k);put_text(&mut out,v);}},ProviderEvent::Metadata{spec,details}=>{out.push(4);put_u32(&mut out,*spec);put_u32(&mut out,details.len() as u32);for(k,v)in details{put_text(&mut out,k);put_text(&mut out,v);}},ProviderEvent::Complete{request_id,samples}=>{out.push(3);put_text(&mut out,request_id);out.extend_from_slice(&samples.to_be_bytes());}}}out }
fn put_u32(out:&mut Vec<u8>,value:u32){out.extend_from_slice(&value.to_be_bytes())}fn put_text(out:&mut Vec<u8>,value:&str){put_u32(out,value.len() as u32);out.extend_from_slice(value.as_bytes())}

fn decode_stream(bytes:&[u8],request:&ProviderRequest)->Result<Vec<ProviderEvent>,ProviderFailure>{if bytes.len()>MAX_BYTES{return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"));}let mut r=Reader{bytes,at:0};if r.take(MAGIC.len())?!=MAGIC{return Err(ProviderFailure::malformed("provider stream has bad magic"));}let mut events=Vec::new();while r.at<bytes.len(){let tag=r.byte()?;let event=match tag{1=>ProviderEvent::Sample{spec:r.u32()?,metric:r.text()?,value:Rational::parse(&r.text()?,&r.text()?).map_err(ProviderFailure::malformed)?},2=>{let spec=r.u32()?;let reason=r.text()?;let count=r.u32()? as usize;if count>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}let mut details=Vec::with_capacity(count);for _ in 0..count{details.push((r.text()?,r.text()?));}if detail_scalars(&reason,&details)>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}ProviderEvent::Unavailable{spec,reason,details}},4=>{let spec=r.u32()?;let count=r.u32()? as usize;if count>MAX_METADATA_SCALARS{return Err(ProviderFailure::malformed("provider metadata exceeds its scalar limit"));}let mut details=Vec::with_capacity(count);for _ in 0..count{details.push((r.text()?,r.text()?));}ProviderEvent::Metadata{spec,details}},3=>ProviderEvent::Complete{request_id:r.text()?,samples:r.u64()?},_=>return Err(ProviderFailure::malformed("provider stream has unknown event tag"))};events.push(event);}validate_events(events,request).map(|v|v.events)}
struct Reader<'a>{bytes:&'a[u8],at:usize}impl<'a>Reader<'a>{fn take(&mut self,n:usize)->Result<&'a[u8],ProviderFailure>{let end=self.at.checked_add(n).ok_or_else(||ProviderFailure::malformed("provider frame length overflow"))?;let value=self.bytes.get(self.at..end).ok_or_else(||ProviderFailure::malformed("provider stream is truncated"))?;self.at=end;Ok(value)}fn byte(&mut self)->Result<u8,ProviderFailure>{Ok(self.take(1)?[0])}fn u32(&mut self)->Result<u32,ProviderFailure>{Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))}fn u64(&mut self)->Result<u64,ProviderFailure>{Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))}fn text(&mut self)->Result<String,ProviderFailure>{let n=self.u32()? as usize;String::from_utf8(self.take(n)?.to_vec()).map_err(|_|ProviderFailure::malformed("provider text is not UTF-8"))}}

fn validate_events(events:Vec<ProviderEvent>,request:&ProviderRequest)->Result<ProviderEvidence,ProviderFailure>{if events.is_empty(){return Err(ProviderFailure::malformed("provider stream is empty"));}let mut sample_count=0usize;let mut complete=false;let mut last_spec=0u32;let mut seen=false;for(index,event)in events.iter().enumerate(){if complete{return Err(ProviderFailure::malformed("event follows final Complete"));}match event{ProviderEvent::Sample{spec,metric,..}=>{if *spec as usize>=request.specs.len()||request.specs[*spec as usize].metric!=*metric{return Err(ProviderFailure::operation(FailureClass::Incompatible,"provider sample does not match requested spec/metric"));}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;sample_count+=1;if sample_count>MAX_SAMPLES{return Err(ProviderFailure::malformed("provider emitted more than 1000000 samples"));}},ProviderEvent::Metadata{spec,details}=>{if *spec as usize>=request.specs.len()||details.is_empty(){return Err(ProviderFailure::malformed("provider metadata has an invalid spec or no fields"));}if detail_scalars("",details)>MAX_METADATA_SCALARS{return Err(ProviderFailure::malformed("provider metadata exceeds its scalar limit"));}let mut previous=None;for(key,value)in details{if key.is_empty()||previous.is_some_and(|prior|prior>=key.as_str()){return Err(ProviderFailure::malformed("provider metadata keys are not unique and sorted"));}if value.is_empty(){return Err(ProviderFailure::malformed("provider metadata contains an empty value"));}previous=Some(key.as_str());}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;},ProviderEvent::Unavailable{spec,reason,details}=>{if *spec as usize>=request.specs.len()||reason.is_empty(){return Err(ProviderFailure::malformed("provider Unavailable has invalid spec or empty reason"));}if detail_scalars(reason,details)>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;},ProviderEvent::Complete{request_id,samples}=>{if index+1!=events.len()||request_id!=&request.request_id||*samples!=sample_count as u64{return Err(ProviderFailure::malformed("provider Complete request id/count/finality mismatch"));}complete=true;}}}if !complete{return Err(ProviderFailure::malformed("provider stream has no final Complete"));}Ok(ProviderEvidence{events})}
fn detail_scalars(reason:&str,details:&[(String,String)])->usize{reason.chars().count().saturating_add(details.iter().map(|(key,value)|key.chars().count().saturating_add(value.chars().count())).sum::<usize>())}
fn is_hex64(value:&str)->bool{value.len()==64&&value.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}

pub fn unavailable_if_too_few(budget:&str, evidence:&ProviderEvidence, minimum:usize)->Result<(),ProviderDiagnostic>{let count=evidence.events.iter().filter(|e|matches!(e,ProviderEvent::Sample{..})).count();if let Some(ProviderEvent::Unavailable{reason,..})=evidence.events.iter().find(|e|matches!(e,ProviderEvent::Unavailable{..})){return Err(ProviderFailure::operation(FailureClass::Unavailable,reason.clone()).diagnostic(budget));}if count<minimum{return Err(ProviderFailure::operation(FailureClass::Unavailable,format!("provider returned {count} samples; policy requires {minimum}")).diagnostic(budget));}Ok(())}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_provider_evidence(budget:&str,evidence_id:&str,context_key:&str,baseline_report_ids:&[String],evidence:&ProviderEvidence,spec:u32,baseline:&[Rational],percentile:Option<Percentile>,comparison:&Comparison,direction:Direction,enforcement:Enforcement,policy:Option<&MeasurementPolicy>,minimum_samples:usize)->Result<Evaluation,ProviderDiagnostic>{
    let mut samples=Vec::new();for event in &evidence.events{match event{ProviderEvent::Sample{spec:event_spec,value,..}if *event_spec==spec=>samples.push(value.clone()),ProviderEvent::Unavailable{spec:event_spec,reason,..}if *event_spec==spec=>return Err(ProviderFailure::operation(FailureClass::Unavailable,reason.clone()).diagnostic(budget)),_=>{}}}
    if samples.len()<minimum_samples{return Err(ProviderFailure::operation(FailureClass::Unavailable,format!("provider returned {} samples; policy requires {minimum_samples}",samples.len())).diagnostic(budget));}
    jet_foundation::PerformanceBudget::evaluate(evidence_id,context_key,baseline_report_ids,&samples,baseline,percentile,comparison,direction,enforcement,policy).map_err(|reason|ProviderFailure::operation(FailureClass::Execution,format!("shared evaluator rejected provider evidence: {reason}")).diagnostic(budget))
}

pub fn evaluation_diagnostic(budget:&str,evaluation:&Evaluation,direction:Direction,baseline_report_ids:&[String])->Option<ProviderDiagnostic>{
    use jet_foundation::PerformanceBudget::Evidence;match evaluation.evidence{Evidence::Pass=>None,Evidence::Unavailable=>Some(ProviderFailure::operation(FailureClass::Unavailable,"the shared evaluator found no compatible nonzero baseline evidence").diagnostic(budget)),Evidence::Regression|Evidence::Inconclusive=>{let state=if evaluation.evidence==Evidence::Regression{"regressed"}else{"is inconclusive"};let rational=|v:&Rational|format!("{}/{}",v.num,v.den);let lower=evaluation.lower95.as_ref().map(&rational).unwrap_or_else(||"none".into());let upper=evaluation.upper95.as_ref().map(&rational).unwrap_or_else(||"none".into());Some(ProviderDiagnostic{code:"E2907",what:format!("performance budget {budget} {state}"),why:format!("estimator {} with confidence [{lower}, {upper}] in {} direction did not prove the limit; baseline reports [{}]",rational(&evaluation.point),match direction{Direction::LowerIsBetter=>"lower-is-better",Direction::HigherIsBetter=>"higher-is-better"},baseline_report_ids.join(",")),fix:"improve the measured behavior, inspect the named evidence, or record an explicit exception".into()})}}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};use std::sync::Mutex;
    static PROCESS_TEST_LOCK:Mutex<()>=Mutex::new(());

    fn request() -> ProviderRequest {
        ProviderRequest { schema:"jet.provider-request".into(), version:1, request_id:"1".repeat(64), provider_hash:"2".repeat(64), context_hash:"3".repeat(64), specs:vec![ProviderSpec{budget_hash:"4".repeat(64),metric:"BenchTime".into()}], workload:CanonicalJson::Null, policy:CanonicalJson::Null }
    }
    fn valid_events(request:&ProviderRequest)->Vec<ProviderEvent>{vec![ProviderEvent::Sample{spec:0,metric:"BenchTime".into(),value:Rational::integer(42)},ProviderEvent::Complete{request_id:request.request_id.clone(),samples:1}]}
    fn panic_provider(_: &ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{panic!("hostile provider panic")}
    fn unavailable_provider(request:&ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{Ok(vec![ProviderEvent::Unavailable{spec:0,reason:"probe could not observe ready event".into(),details:vec![]},ProviderEvent::Complete{request_id:request.request_id.clone(),samples:0}])}
    fn temporary(name:&str)->PathBuf{static NEXT:AtomicU64=AtomicU64::new(0);std::env::temp_dir().join(format!("jet-budget-provider-{}-{name}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)))}
    #[test]
    fn compiler_probe_rejects_forged_workload_identity_before_execution(){
        let root=temporary("compile-identity");std::fs::create_dir_all(root.join("src")).unwrap();std::fs::create_dir_all(root.join("edits")).unwrap();
        std::fs::write(root.join("package.jet"),"name: \"identity\"\nversion: \"0.1.0\"\n").unwrap();std::fs::write(root.join("src/run.jet"),"fn run() {}\n").unwrap();std::fs::write(root.join("edits/change.patch"),"patch\n").unwrap();
        let workload=compiler_probe_workload(&root,&root.join("src/run.jet"),"Edit","cli","dev",Some("edits/change.patch")).unwrap();
        let CanonicalJson::Object(mut fields)=workload else{panic!("workload object")};fields.insert("source_tree_sha256".into(),CanonicalJson::String("0".repeat(64)));
        let request=ProviderRequest{schema:"jet.provider-request".into(),version:1,request_id:"1".repeat(64),provider_hash:"2".repeat(64),context_hash:"3".repeat(64),specs:vec![ProviderSpec{budget_hash:"4".repeat(64),metric:"CompileTime(P95)".into()}],workload:CanonicalJson::Object(fields),policy:CanonicalJson::Null};
        let cancellation=ProviderCancellation{cancelled:Arc::new(AtomicBool::new(false))};let failure=compiler_latency_provider(&request,&cancellation).unwrap_err();assert_eq!(failure.class,FailureClass::Incompatible);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn compiler_probe_rejects_unsupported_profile_before_execution(){
        let root=temporary("compile-profile");std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.jet"),"name: \"profile\"\nversion: \"0.1.0\"\n").unwrap();std::fs::write(root.join("src/run.jet"),"fn run() {}\n").unwrap();
        let error=compiler_probe_workload(&root,&root.join("src/run.jet"),"Clean","cli","unsupported",None).unwrap_err();
        assert!(error.contains("profile `unsupported` is unsupported"),"{error}");
        let _=std::fs::remove_dir_all(root);
    }
    #[cfg(target_os="linux")]fn assert_last_group_gone(){extern "C"{fn kill(pid:i32,signal:i32)->i32;}let group=LAST_ISOLATED_GROUP.load(Ordering::SeqCst)as i32;let deadline=Instant::now()+Duration::from_millis(100);while unsafe{kill(-group,0)}==0&&Instant::now()<deadline{std::thread::yield_now()}assert_ne!(unsafe{kill(-group,0)},0,"isolated provider process group survived timeout");}
    #[cfg(target_os="linux")]const SYS_PIDFD_SEND_SIGNAL:isize=424;
    #[cfg(target_os="linux")]const SYS_PIDFD_OPEN:isize=434;
    #[cfg(target_os="linux")]struct PidFd(std::fs::File);
    #[cfg(target_os="linux")]impl PidFd{
        fn open(pid:i32)->std::io::Result<Self>{use std::os::fd::FromRawFd;extern "C"{fn syscall(number:isize,...)->isize;}let fd=unsafe{syscall(SYS_PIDFD_OPEN,pid,0u32)};if fd<0{Err(std::io::Error::last_os_error())}else{Ok(Self(unsafe{std::fs::File::from_raw_fd(fd as i32)}))}}
        fn signal(&self,signal:i32)->std::io::Result<()>{use std::os::fd::AsRawFd;extern "C"{fn syscall(number:isize,...)->isize;}if unsafe{syscall(SYS_PIDFD_SEND_SIGNAL,self.0.as_raw_fd(),signal,std::ptr::null::<u8>(),0u32)}==0{Ok(())}else{Err(std::io::Error::last_os_error())}}
        fn alive(&self)->std::io::Result<bool>{match self.signal(0){Ok(())=>Ok(true),Err(error)if error.raw_os_error()==Some(3)=>Ok(false),Err(error)=>Err(error)}}
    }

    #[cfg(target_os="linux")]
    #[test]
    fn isolated_worker_dies_with_supervisor(){
        const CHILD_ENV:&str="JET_TEST_ISOLATED_WORKER_SUPERVISOR";
        const PID_PATH_ENV:&str="JET_TEST_ISOLATED_WORKER_PID_PATH";
        if std::env::var_os(CHILD_ENV).is_some(){
            let path=PathBuf::from(std::env::var_os(PID_PATH_ENV).expect("worker PID path"));
            let _=run_isolated_bytes(Duration::from_secs(10),"parent-death regression",move||{
                std::fs::write(&path,std::process::id().to_string()).map_err(|error|ProviderFailure::operation(FailureClass::Execution,format!("cannot publish isolated worker PID: {error}")))?;
                loop{std::hint::spin_loop()}
            });
            return;
        }

        const SIGKILL:i32=9;
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        let path=temporary("isolated-worker-pid");
        let mut supervisor=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","BudgetProviders::tests::isolated_worker_dies_with_supervisor","--nocapture"])
            .env(CHILD_ENV,"1").env(PID_PATH_ENV,&path).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let publish_deadline=Instant::now()+Duration::from_secs(5);
        let worker=loop{
            if let Ok(text)=std::fs::read_to_string(&path){if let Ok(pid)=text.parse::<i32>(){break pid;}}
            if Instant::now()>=publish_deadline{
                let _=supervisor.wait();let _=std::fs::remove_file(&path);
                return assert!(false,"isolated worker did not publish its PID before the deadline");
            }
            assert!(supervisor.try_wait().unwrap().is_none(),"isolated worker supervisor exited before publishing its PID");
            std::thread::sleep(Duration::from_millis(2));
        };
        let worker=match PidFd::open(worker){Ok(worker)=>worker,Err(error)=>{let _=supervisor.wait();let _=std::fs::remove_file(&path);return assert!(false,"cannot open isolated worker pidfd: {error}")}};
        supervisor.kill().unwrap();supervisor.wait().unwrap();
        let exit_deadline=Instant::now()+Duration::from_secs(2);
        while worker.alive().unwrap()&&Instant::now()<exit_deadline{std::thread::sleep(Duration::from_millis(2));}
        let gone=!worker.alive().unwrap();
        if !gone{
            worker.signal(SIGKILL).unwrap();
            let cleanup_deadline=Instant::now()+Duration::from_secs(2);
            while worker.alive().unwrap()&&Instant::now()<cleanup_deadline{std::thread::sleep(Duration::from_millis(2));}
        }
        let _=std::fs::remove_file(&path);
        assert!(gone,"isolated worker survived its supervisor");
    }

    #[test]
    fn file_transport_round_trips_and_rejects_hostile_frames(){
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        let req=request();let path=temporary("response");std::fs::write(&path,encode_stream(&valid_events(&req))).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("fixture",path.clone()).unwrap();let evidence=registry.collect("fixture",&req,Duration::from_secs(1)).unwrap();assert_eq!(evidence.events,valid_events(&req));
        for (name,bytes) in [("bad-magic",b"NOTBUDGET\n".to_vec()),("truncated",{let mut b=MAGIC.to_vec();b.extend_from_slice(&[1,0,0]);b}),("trailing",{let mut b=encode_stream(&valid_events(&req));b.push(99);b})] { let hostile=temporary(name);std::fs::write(&hostile,bytes).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("hostile",hostile.clone()).unwrap();let error=registry.collect("hostile",&req,Duration::from_secs(1)).unwrap_err();assert_eq!(error.diagnostic("api").code,"E2908");let _=std::fs::remove_file(hostile); }
        let _=std::fs::remove_file(path);
    }

    #[test]
    fn in_process_panic_and_unavailable_are_separate_diagnostic_classes(){
        let req=request();let mut registry=ProviderRegistry::default();registry.register_in_process("panic",panic_provider).unwrap();let failure=registry.collect("panic",&req,Duration::from_secs(1)).unwrap_err();let diagnostic=failure.diagnostic("api-p99");assert_eq!((diagnostic.code,diagnostic.what.as_str()),("E2908","performance budget operation failed"));
        registry.register_in_process("unavailable",unavailable_provider).unwrap();let evidence=registry.collect("unavailable",&req,Duration::from_secs(1)).unwrap();let diagnostic=unavailable_if_too_few("api-p99",&evidence,20).unwrap_err();assert_eq!((diagnostic.code,diagnostic.what.as_str()),("E2906","performance budget api-p99 has no usable evidence"));assert!(diagnostic.why.contains("ready event"));
        fn hostile(_: &ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{loop{std::hint::spin_loop()}}
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());registry.register_in_process("hostile",hostile).unwrap();let started=Instant::now();for _ in 0..25{let failure=registry.collect("hostile",&req,Duration::from_millis(5)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert_last_group_gone();assert_eq!(ACTIVE_ISOLATED_WORKERS.load(Ordering::SeqCst),0);}assert!(started.elapsed()<Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_uses_exact_path_and_is_bounded_and_timed(){
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        use std::os::unix::fs::PermissionsExt;
        let req=request();let response=temporary("subprocess-response");std::fs::write(&response,encode_stream(&valid_events(&req))).unwrap();let script=temporary("provider.sh");std::fs::write(&script,format!("#!/bin/sh\ncat '{}'\n",response.display())).unwrap();let mut permissions=std::fs::metadata(&script).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&script,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("process",script.clone()).unwrap();assert_eq!(registry.collect("process",&req,Duration::from_secs(2)).unwrap().events,valid_events(&req));
        let sleeper=temporary("sleep.sh");std::fs::write(&sleeper,"#!/bin/sh\nsleep 5\n").unwrap();let mut permissions=std::fs::metadata(&sleeper).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&sleeper,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("slow",sleeper.clone()).unwrap();let started=Instant::now();let failure=registry.collect("slow",&req,Duration::from_millis(30)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(2));
        let descendant=temporary("descendant.sh");std::fs::write(&descendant,"#!/bin/sh\nsleep 5 &\nexit 0\n").unwrap();let mut permissions=std::fs::metadata(&descendant).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&descendant,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("descendant",descendant.clone()).unwrap();let started=Instant::now();let failure=registry.collect("descendant",&req,Duration::from_millis(30)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(1));
        for path in [response,script,sleeper,descendant]{let _=std::fs::remove_file(path);}
    }

    #[test]
    fn file_transport_rejects_symlinks_and_scalar_overflow(){
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        let req=request();let target=temporary("target");std::fs::write(&target,encode_stream(&valid_events(&req))).unwrap();
        #[cfg(unix)]{use std::os::unix::fs::symlink;let link=temporary("link");symlink(&target,&link).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("link",link.clone()).unwrap();assert!(registry.collect("link",&req,Duration::from_secs(1)).is_err());let _=std::fs::remove_file(link);}
        let events=vec![ProviderEvent::Unavailable{spec:0,reason:"x".repeat(513),details:Vec::new()},ProviderEvent::Complete{request_id:req.request_id.clone(),samples:0}];let path=temporary("detail");std::fs::write(&path,encode_stream(&events)).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("detail",path.clone()).unwrap();assert!(registry.collect("detail",&req,Duration::from_secs(1)).unwrap_err().reason.contains("512 scalars"));for path in [target,path]{let _=std::fs::remove_file(path);}
    }

    #[cfg(unix)]#[test]fn repeated_timed_file_readers_are_killed_and_reaped(){let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());let req=request();let path=temporary("delayed");std::fs::write(&path,encode_stream(&valid_events(&req))).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("delayed",path.clone()).unwrap();FILE_READER_DELAY_MS.store(100,Ordering::SeqCst);let started=Instant::now();for _ in 0..25{let failure=registry.collect("delayed",&req,Duration::from_millis(5)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert_last_group_gone();assert_eq!(ACTIVE_FILE_READERS.load(Ordering::SeqCst),0);}FILE_READER_DELAY_MS.store(0,Ordering::SeqCst);assert!(started.elapsed()<Duration::from_secs(1));let _=std::fs::remove_file(path);}

    #[cfg(target_os="linux")]
    #[test]
    fn concurrent_isolated_workers_do_not_mask_file_reader_reaping(){
        fn hostile(_: &ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{loop{std::hint::spin_loop()}}
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        let workers=(0..4).map(|_|std::thread::spawn(||{let req=request();let mut registry=ProviderRegistry::default();registry.register_in_process("hostile",hostile).unwrap();registry.collect("hostile",&req,Duration::from_millis(500)).unwrap_err()})).collect::<Vec<_>>();
        let deadline=Instant::now()+Duration::from_secs(1);while ACTIVE_ISOLATED_WORKERS.load(Ordering::SeqCst)<4&&Instant::now()<deadline{std::thread::yield_now()}assert!(ACTIVE_ISOLATED_WORKERS.load(Ordering::SeqCst)>=4,"hostile isolated workers did not overlap");
        let req=request();let path=temporary("parallel-delayed");std::fs::write(&path,encode_stream(&valid_events(&req))).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("delayed",path.clone()).unwrap();FILE_READER_DELAY_MS.store(100,Ordering::SeqCst);
        for _ in 0..10{let failure=registry.collect("delayed",&req,Duration::from_millis(5)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert_eq!(ACTIVE_FILE_READERS.load(Ordering::SeqCst),0);assert!(ACTIVE_ISOLATED_WORKERS.load(Ordering::SeqCst)>=4,"generic workers ended before file-reader lifecycle was checked");}
        FILE_READER_DELAY_MS.store(0,Ordering::SeqCst);for worker in workers{assert_eq!(worker.join().unwrap().class,FailureClass::Timeout);}assert_eq!(ACTIVE_FILE_READERS.load(Ordering::SeqCst),0);let _=std::fs::remove_file(path);
    }
}
