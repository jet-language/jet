//! Native execution of the bounded Nix derivation boundary.
//!
//! The evaluator owns derivation identity. This module only runs the already
//! evaluated builder, validates its declared outputs, and hands the bytes to
//! the existing Hangar Nix projection. It never invokes Nix or a network tool.

#![allow(dead_code)] // #2158 wires this seam into the provider dispatch.

use crate::NixEval::NativeDerivationEvaluation;
use crate::Provider::{self, ProviderError, Realized, SourceState};
use crate::Store::{self, CacheIdentity, Roots, StoreEntry};
use crate::{Envelope, SHA256};
use jet_foundation::Diagnostics::Diagnostic;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);
const OUTPUT_ROOT: &str = "/work/output";

/// A failure at the native Nix build boundary. The native route projects each
/// failure through the same provider diagnostic contract as substitution; the
/// route itself remains visible only in the producer provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeNixBuildError {
    SandboxUnavailable(String),
    Invalid(String),
    Failed(String),
    Store(String),
}

impl NativeNixBuildError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::SandboxUnavailable(_) => "E1275",
            Self::Invalid(_) => "E1340",
            Self::Failed(_) => "E1273",
            Self::Store(_) => "E1315",
        }
    }

    pub(crate) fn what(&self) -> String {
        match self {
            Self::SandboxUnavailable(_) => "build sandboxing is required but unavailable".into(),
            Self::Invalid(_) => "couldn't understand the provider's output".into(),
            Self::Failed(_) => "package build failed at a logged step".into(),
            Self::Store(_) => "hangar ingest aborted".into(),
        }
    }

    pub(crate) fn why(&self) -> &str {
        match self {
            Self::SandboxUnavailable(reason)
            | Self::Invalid(reason)
            | Self::Failed(reason)
            | Self::Store(reason) => reason,
        }
    }

    pub(crate) fn fix(&self) -> &'static str {
        match self {
            Self::SandboxUnavailable(_) => {
                "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry."
            }
            Self::Invalid(_) => "this is likely a Jetpack bug — please report it.",
            Self::Failed(_) => {
                "run `jet logs <pkg>` for full output, or rerun with `--shell-on-fail`."
            }
            Self::Store(_) => {
                "re-run ingest against a stable output, or quarantine and rebuild it from a trusted source."
            }
        }
    }

    /// Build the same registered diagnostic that the substituted provider
    /// route exposes to the terminal and machine-facing diagnostic consumers.
    pub(crate) fn diagnostic(&self) -> Diagnostic {
        Diagnostic::error(
            self.code(),
            self.what(),
            self.why().to_string(),
            self.fix().to_string(),
            None,
        )
    }

    /// Render through the shared Jetpack terminal surface. Callers do not
    /// need a native-only error renderer or route-specific wording.
    pub(crate) fn report(&self, theme: &crate::Output::Theme) {
        let diagnostic = self.diagnostic();
        theme.error_coded(
            &diagnostic.code,
            &diagnostic.what,
            &diagnostic.why,
            &diagnostic.fix,
        );
    }
}

impl From<NativeNixBuildError> for ProviderError {
    fn from(error: NativeNixBuildError) -> Self {
        match error {
            NativeNixBuildError::SandboxUnavailable(reason) => {
                ProviderError::SandboxUnavailable(reason)
            }
            NativeNixBuildError::Invalid(reason) => ProviderError::BadOutput(reason),
            NativeNixBuildError::Failed(reason) => ProviderError::BuildDebug(reason),
            NativeNixBuildError::Store(reason) => ProviderError::Ingest(reason),
        }
    }
}

/// One output after the native builder has run but before Hangar promotion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeNixOutput {
    pub(crate) name: String,
    pub(crate) store_path: String,
    pub(crate) path: PathBuf,
    pub(crate) digest: String,
}

/// Native builder result. The scratch guard keeps the output paths alive until
/// the caller promotes this result into Hangar.
#[derive(Debug)]
pub(crate) struct NativeNixBuild {
    pub(crate) drv_path: String,
    pub(crate) outputs: BTreeMap<String, NativeNixOutput>,
    pub(crate) sandbox_mechanism: String,
    pub(crate) sandbox_policy: String,
    scratch: BuildScratch,
}

/// Execute one evaluated derivation in the shared native sandbox.
pub(crate) fn build_derivation(
    roots: &Roots,
    source_dir: &Path,
    evaluation: &NativeDerivationEvaluation,
) -> Result<NativeNixBuild, NativeNixBuildError> {
    let status = crate::Comptime::Build::native_sandbox_status();
    if !status.available {
        return Err(NativeNixBuildError::SandboxUnavailable(status.reason));
    }
    validate_source_dir(source_dir)?;
    validate_builder(evaluation)?;
    if !evaluation.input_sources().is_empty() {
        return Err(NativeNixBuildError::Invalid(format!(
            "derivation has {} input source(s) but no admitted Hangar input closure",
            evaluation.input_sources().len()
        )));
    }
    validate_literal_store_paths(evaluation)?;

    let scratch = BuildScratch::new(&roots.hangar_dir(), evaluation.drv_path())?;
    let output = scratch.path.join("output");
    create_real_directory(&output, "native Nix output sandbox")?;

    let mut local_outputs = BTreeMap::new();
    for (name, store_path) in evaluation.outputs() {
        validate_output_name(name)?;
        let path = output.join(name);
        let _ = local_outputs.insert(name.clone(), (store_path.clone(), path));
    }

    let mut env = BTreeMap::new();
    for (key, value) in evaluation.env() {
        let mut value = value.clone();
        for (name, (store_path, _)) in &local_outputs {
            if !store_path.is_empty() {
                value = value.replace(store_path, &format!("{OUTPUT_ROOT}/{name}"));
            }
        }
        let _ = env.insert(key.clone(), value);
    }
    for name in local_outputs.keys() {
        let _ = env.insert(name.clone(), format!("{OUTPUT_ROOT}/{name}"));
    }

    let mut args = evaluation.args().to_vec();
    for arg in &mut args {
        for (store_path, (name, _)) in local_outputs
            .iter()
            .map(|(name, (store_path, path))| (store_path, (name, path)))
        {
            if !store_path.is_empty() {
                *arg = arg.replace(store_path, &format!("{OUTPUT_ROOT}/{name}"));
            }
        }
    }

    let sandbox = crate::Comptime::Build::run_native_sandboxed(
        Path::new(evaluation.builder()),
        &args,
        source_dir,
        Some(&output),
        &env,
        false,
    )
    .map_err(|error| NativeNixBuildError::SandboxUnavailable(format_native_sandbox_error(error)))?;
    if !sandbox.output.status.success() {
        return Err(NativeNixBuildError::Failed(command_failure(
            &sandbox.output,
        )));
    }

    let mut outputs = BTreeMap::new();
    for (name, (store_path, path)) in local_outputs {
        validate_built_output(&name, &path, evaluation.output_specs().get(&name))?;
        Store::seal_local_output(&path).map_err(|error| {
            NativeNixBuildError::Store(format!("sealing output `{name}`: {error}"))
        })?;
        let digest = Envelope::try_output_hash_of(&path.to_string_lossy())
            .map_err(|reason| NativeNixBuildError::Store(format!("output `{name}`: {reason}")))?;
        let _ = outputs.insert(
            name.clone(),
            NativeNixOutput {
                name,
                store_path,
                path,
                digest,
            },
        );
    }

    Ok(NativeNixBuild {
        drv_path: evaluation.drv_path().to_string(),
        outputs,
        sandbox_mechanism: sandbox.mechanism,
        sandbox_policy: sandbox.policy,
        scratch,
    })
}

/// Read one repository flake, evaluate its bounded package output, and admit
/// the result through the native builder. This is the production seam for a
/// local flake/package route; the file is source input, never a pinned output
/// fixture.
pub(crate) fn build_repository_output(
    roots: &Roots,
    project_dir: &Path,
    flake_path: &Path,
    system: &str,
    attribute: &str,
    reference: &str,
    version: &str,
) -> Result<StoreEntry, NativeNixBuildError> {
    validate_source_dir(project_dir)?;
    let project_dir = project_dir.canonicalize().map_err(|error| {
        NativeNixBuildError::Invalid(format!("canonicalizing project source: {error}"))
    })?;
    let metadata = fs::symlink_metadata(flake_path).map_err(|error| {
        NativeNixBuildError::Invalid(format!("flake source `{}`: {error}", flake_path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(NativeNixBuildError::Invalid(format!(
            "flake source `{}` is not a real file",
            flake_path.display()
        )));
    }
    let flake_path = flake_path.canonicalize().map_err(|error| {
        NativeNixBuildError::Invalid(format!("canonicalizing flake source: {error}"))
    })?;
    if !flake_path.starts_with(&project_dir) {
        return Err(NativeNixBuildError::Invalid(format!(
            "flake source `{}` escapes project root `{}`",
            flake_path.display(),
            project_dir.display()
        )));
    }
    let source = fs::read_to_string(&flake_path).map_err(|error| {
        NativeNixBuildError::Invalid(format!(
            "reading flake source `{}`: {error}",
            flake_path.display()
        ))
    })?;
    let evaluation = crate::NixEval::evaluate_derivation_output(&source, system, attribute)
        .map_err(|error| NativeNixBuildError::Invalid(error.to_string()))?;
    let build = build_derivation(roots, &project_dir, &evaluation)?;
    admit_built_derivation(roots, build, reference, version)
}

/// Promote a native build through the same Nix Store registration used by
/// substituted outputs. The build's scratch guard stays alive until the
/// projection has atomically moved every output into Hangar.
pub(crate) fn admit_built_derivation(
    roots: &Roots,
    build: NativeNixBuild,
    reference: &str,
    version: &str,
) -> Result<StoreEntry, NativeNixBuildError> {
    let primary = build
        .outputs
        .get("out")
        .or_else(|| build.outputs.get("bin"))
        .ok_or_else(|| {
            NativeNixBuildError::Invalid("derivation has no `out` or `bin` output".into())
        })?;
    let mut named_outputs = BTreeMap::new();
    let mut facts = Provider::nix_build_facts_record();
    let _ = facts.insert("nix.drv_path".into(), build.drv_path.clone());
    let _ = facts.insert("nix.reference".into(), reference.to_string());
    let _ = facts.insert("build.sandbox".into(), "native".into());
    let _ = facts.insert("build.sandbox_policy".into(), build.sandbox_policy.clone());
    let _ = facts.insert(
        "build.sandbox_mechanism".into(),
        build.sandbox_mechanism.clone(),
    );
    for (name, output) in &build.outputs {
        let _ = named_outputs.insert(name.clone(), output.path.to_string_lossy().into_owned());
        let _ = facts.insert(format!("nix.output.{name}"), output.store_path.clone());
        let _ = facts.insert(format!("nix.output.{name}.digest"), output.digest.clone());
    }
    let identity = CacheIdentity {
        source_fingerprint: primary.digest.clone(),
        recipe_fingerprint: SHA256::sha256_hex(Provider::NIX_RECIPE_ID.as_bytes()),
        policy_fingerprint: crate::RuntimePolicy::cache_policy_fingerprint(false),
        platform: Envelope::host_platform(),
    };
    let derivation_digest = SHA256::sha256_hex(build.drv_path.as_bytes());
    let producer = Provider::producer_record(
        "nix",
        &build.drv_path,
        &derivation_digest,
        facts.clone(),
        &format!("nix-derivation:{}", build.drv_path),
        &identity,
        facts,
    )
    .map_err(|error| NativeNixBuildError::Store(format!("producer record: {error:?}")))?;
    let primary_path = primary.path.to_string_lossy().into_owned();
    let bin = build
        .outputs
        .get("bin")
        .or_else(|| build.outputs.get("out"))
        .map(|output| output.path.join("bin").to_string_lossy().into_owned())
        .unwrap_or_default();
    let realized = Realized {
        name: nix_name_from_store_path(&primary.store_path),
        version: version.to_string(),
        reference: reference.to_string(),
        out: primary_path,
        bin,
        rlib: String::new(),
        envelope: Envelope::Envelope {
            output_hash: primary.digest.clone(),
            platform: identity.platform.clone(),
            signature: String::new(),
            provenance: format!("{reference} via nix-native-build"),
        },
        cache_identity: identity,
        source_state: SourceState::Built,
        named_outputs,
        references: Vec::new(),
        producer,
    };
    let entry = Store::record_realized_mode(roots, &realized)
        .map_err(|error| NativeNixBuildError::Store(error.to_string()))?;
    drop(build);
    Ok(entry)
}

fn validate_source_dir(path: &Path) -> Result<(), NativeNixBuildError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        NativeNixBuildError::Invalid(format!("source directory `{}`: {error}", path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(NativeNixBuildError::Invalid(format!(
            "source directory `{}` is not a real directory",
            path.display()
        )));
    }
    Ok(())
}

fn validate_builder(evaluation: &NativeDerivationEvaluation) -> Result<(), NativeNixBuildError> {
    let builder = Path::new(evaluation.builder());
    if !builder.is_absolute() {
        return Err(NativeNixBuildError::Invalid(format!(
            "builder `{}` is not an absolute path",
            evaluation.builder()
        )));
    }
    if builder.starts_with(crate::NixDrv::DEFAULT_STORE_DIR) {
        return Err(NativeNixBuildError::Invalid(
            "builders from `/nix/store` require an admitted native tool closure".into(),
        ));
    }
    Ok(())
}

fn validate_literal_store_paths(
    evaluation: &NativeDerivationEvaluation,
) -> Result<(), NativeNixBuildError> {
    let output_paths = evaluation.outputs().values().collect::<Vec<_>>();
    let check = |value: &str| {
        if value.contains("/nix/store/")
            && value != "/nix/store"
            && !output_paths
                .iter()
                .any(|path| value.contains(path.as_str()))
        {
            return Err(NativeNixBuildError::Invalid(
                "derivation refers to an unadmitted `/nix/store` input".into(),
            ));
        }
        Ok(())
    };
    for value in evaluation.env().values() {
        check(value)?;
    }
    for value in evaluation.args() {
        check(value)?;
    }
    Ok(())
}

fn validate_output_name(name: &str) -> Result<(), NativeNixBuildError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(NativeNixBuildError::Invalid(format!(
            "output name `{name}` is not one path component"
        )));
    }
    Ok(())
}

fn validate_built_output(
    name: &str,
    path: &Path,
    spec: Option<&(String, String)>,
) -> Result<(), NativeNixBuildError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        NativeNixBuildError::Failed(format!("declared output `{name}` is missing: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(NativeNixBuildError::Store(format!(
            "declared output `{name}` is a symlink"
        )));
    }
    if let Some((method_algo, expected)) = spec {
        if !expected.is_empty() {
            if method_algo != "sha256" || !metadata.is_file() {
                return Err(NativeNixBuildError::Invalid(format!(
                    "fixed output `{name}` uses unsupported native verification method `{method_algo}`"
                )));
            }
            let actual = SHA256::sha256_file_hex(path).map_err(|error| {
                NativeNixBuildError::Store(format!("fixed output `{name}`: {error}"))
            })?;
            if actual != *expected {
                return Err(NativeNixBuildError::Store(format!(
                    "fixed output `{name}` hash mismatch: expected `{expected}`, got `{actual}`"
                )));
            }
        }
    }
    Ok(())
}

fn create_real_directory(path: &Path, label: &str) -> Result<(), NativeNixBuildError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            NativeNixBuildError::Store(format!("{label} is not a real directory")),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)
            .map_err(|error| NativeNixBuildError::Store(format!("{label}: {error}"))),
        Err(error) => Err(NativeNixBuildError::Store(format!("{label}: {error}"))),
    }
}

fn format_native_sandbox_error(error: crate::Comptime::Build::NativeSandboxError) -> String {
    match error {
        crate::Comptime::Build::NativeSandboxError::Unsupported(detail) => detail,
        crate::Comptime::Build::NativeSandboxError::Io(detail) => detail,
    }
}

fn command_failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    if detail.is_empty() {
        format!("exit status {}", output.status)
    } else {
        detail.chars().take(4096).collect()
    }
}

fn nix_name_from_store_path(path: &str) -> String {
    let basename = path.rsplit('/').next().unwrap_or(path);
    basename
        .split_once('-')
        .map(|(_, name)| name.to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "nix-output".into())
}

#[derive(Debug)]
struct BuildScratch {
    path: PathBuf,
}

impl BuildScratch {
    fn new(hangar: &Path, drv_path: &str) -> Result<Self, NativeNixBuildError> {
        create_real_directory(hangar, "Hangar root")?;
        let root = hangar.join(Provider::BUILD_SCRATCH_DIR);
        create_real_directory(&root, "Hangar build scratch root")?;
        let digest = SHA256::sha256_hex(drv_path.as_bytes());
        let path = root.join(format!(
            "native-{digest}-{}",
            NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).map_err(|error| {
            NativeNixBuildError::Store(format!("creating native build scratch: {error}"))
        })?;
        fs::write(
            path.join(Provider::ACTIVE_TMP_MARKER),
            format!("pid={}\n", std::process::id()),
        )
        .map_err(|error| {
            NativeNixBuildError::Store(format!("marking native build scratch: {error}"))
        })?;
        Ok(Self { path })
    }
}

impl Drop for BuildScratch {
    fn drop(&mut self) {
        make_tree_writable(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn make_tree_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_tree_writable(&entry.path());
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        let _ = fs::set_permissions(path, permissions);
    }
    #[cfg(not(unix))]
    {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

#[cfg(test)]
mod Tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> (Roots, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "jet-native-nix-builder-{label}-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        (Roots::at(path.clone()), path)
    }

    fn test_source(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jet-native-nix-builder-source-{label}-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixed_evaluation() -> NativeDerivationEvaluation {
        let hash = SHA256::sha256_hex(b"hi\n");
        crate::NixEval::evaluate_derivation(
            &format!(
                r#"builtins.derivationStrict {{ name = "native-hello"; system = "x86_64-linux"; builder = "/bin/sh"; args = [ "-c" "echo hi > $out" ]; outputHashAlgo = "sha256"; outputHashMode = "flat"; outputHash = "{hash}"; }}"#
            ),
            "x86_64-linux",
        )
        .unwrap()
    }

    #[test]
    fn native_derivation_build_enters_hangar_with_receipt() {
        let evaluation = fixed_evaluation();
        let (roots, root) = test_root("admit");
        let source = test_source("admit");
        let build = build_derivation(&roots, &source, &evaluation).unwrap();
        assert_eq!(
            build.outputs.get("out").unwrap().store_path,
            evaluation.outputs().get("out").unwrap().as_str()
        );
        let entry = admit_built_derivation(&roots, build, "./flake.nix#native-hello", "").unwrap();
        assert!(Path::new(&entry.out).starts_with(roots.hangar_dir().join("objects")));
        assert_eq!(
            entry.envelope.output_hash,
            Envelope::try_output_hash_of_in_hangar(&entry.out, &roots.hangar_dir(), false).unwrap()
        );
        assert!(!entry.receipt.is_empty());
        assert!(roots
            .hangar_dir()
            .join("receipts")
            .join(&entry.receipt)
            .is_file());
        let receipt =
            fs::read_to_string(roots.hangar_dir().join("receipts").join(&entry.receipt)).unwrap();
        assert!(receipt.starts_with("jet-development-receipt-v1\n"));
        assert!(receipt.contains("act\t\t7061636b6167652d7265616c697a6174696f6e\n"));
        assert!(receipt.contains("input\t70726f64756365722d7265636f7264\t"));
        assert!(receipt.contains("outcome\t\t706173736564\n"));
        assert_eq!(
            entry.receipt,
            format!("sha256-{}", SHA256::sha256_hex(receipt.as_bytes()))
        );
        let producer = Store::ProducerRecord::decode(&entry.producer_record).unwrap();
        assert_eq!(producer.provider, "nix");
        assert_eq!(
            producer.facts.get("build.sandbox").map(String::as_str),
            Some("native")
        );
        assert_eq!(
            producer.facts.get("nix.drv_path").map(String::as_str),
            Some(evaluation.drv_path())
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn native_receipt_contract_is_independent_of_source_route() {
        let evaluation = fixed_evaluation();
        let (roots, root) = test_root("receipt-route");
        let source = test_source("receipt-route");
        let native = admit_built_derivation(
            &roots,
            build_derivation(&roots, &source, &evaluation).unwrap(),
            "./flake.nix#native-hello",
            "",
        )
        .unwrap();
        let mut substituted_envelope = native.envelope.clone();
        substituted_envelope.provenance = format!("{} via nix", native.reference);
        let substituted = Realized {
            name: native.name.clone(),
            version: native.version.clone(),
            reference: native.reference.clone(),
            out: native.out.clone(),
            bin: native.bin.clone(),
            rlib: native.rlib.clone(),
            envelope: substituted_envelope,
            cache_identity: native.cache_identity.clone(),
            source_state: SourceState::Substituted,
            named_outputs: BTreeMap::from([("out".into(), native.out.clone())]),
            references: native.references.clone(),
            producer: Store::ProducerRecord::decode(&native.producer_record).unwrap(),
        };
        let substituted_entry = Store::record_realized_mode(&roots, &substituted).unwrap();

        assert_eq!(native.receipt, substituted_entry.receipt);
        assert_eq!(
            native.envelope.output_hash,
            substituted_entry.envelope.output_hash
        );
        assert_ne!(
            native.envelope.provenance,
            substituted_entry.envelope.provenance
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn native_derivation_rebuild_reuses_the_same_content_identity() {
        let evaluation = fixed_evaluation();
        let source = test_source("repro");
        let (left_roots, left_root) = test_root("repro-left");
        let (right_roots, right_root) = test_root("repro-right");
        let left = admit_built_derivation(
            &left_roots,
            build_derivation(&left_roots, &source, &evaluation).unwrap(),
            "./flake.nix#native-hello",
            "",
        )
        .unwrap();
        let right = admit_built_derivation(
            &right_roots,
            build_derivation(&right_roots, &source, &evaluation).unwrap(),
            "./flake.nix#native-hello",
            "",
        )
        .unwrap();
        assert_eq!(left.envelope.output_hash, right.envelope.output_hash);
        assert_eq!(left.receipt, right.receipt);
        let left_producer = Store::ProducerRecord::decode(&left.producer_record).unwrap();
        let right_producer = Store::ProducerRecord::decode(&right.producer_record).unwrap();
        assert_eq!(
            left_producer.facts.get("nix.drv_path"),
            right_producer.facts.get("nix.drv_path")
        );
        let _ = fs::remove_dir_all(left_root);
        let _ = fs::remove_dir_all(right_root);
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn native_repository_derivation_builds_without_nix_realization() {
        let flake = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/nix-compat/native-builder-flake.nix");
        let project = flake.parent().unwrap();
        let (roots, root) = test_root("repository");
        let entry = build_repository_output(
            &roots,
            project,
            &flake,
            "x86_64-linux",
            "native",
            "./tests/fixtures/nix-compat/native-builder-flake.nix#native",
            "",
        )
        .unwrap();
        assert_eq!(entry.name, "native-repository");
        assert!(Path::new(&entry.out).starts_with(roots.hangar_dir().join("objects")));
        let producer = Store::ProducerRecord::decode(&entry.producer_record).unwrap();
        assert_eq!(
            producer.facts.get("build.sandbox").map(String::as_str),
            Some("native")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_native_derivation_uses_registered_build_diagnostic() {
        let evaluation = crate::NixEval::evaluate_derivation(
            r#"builtins.derivationStrict { name = "native-fail"; system = "x86_64-linux"; builder = "/bin/sh"; args = [ "-c" "exit 7" ]; }"#,
            "x86_64-linux",
        )
        .unwrap();
        let (roots, root) = test_root("failure");
        let source = test_source("failure");
        let error = build_derivation(&roots, &source, &evaluation).unwrap_err();
        assert_eq!(error.code(), "E1273");
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn native_build_diagnostics_match_substituted_provider_contract() {
        let cases = [
            (
                NativeNixBuildError::SandboxUnavailable("sandbox detail".into()),
                "E1275",
                "build sandboxing is required but unavailable",
                "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry.",
            ),
            (
                NativeNixBuildError::Invalid("invalid derivation".into()),
                "E1340",
                "couldn't understand the provider's output",
                "this is likely a Jetpack bug — please report it.",
            ),
            (
                NativeNixBuildError::Failed("builder detail".into()),
                "E1273",
                "package build failed at a logged step",
                "run `jet logs <pkg>` for full output, or rerun with `--shell-on-fail`.",
            ),
            (
                NativeNixBuildError::Store("store detail".into()),
                "E1315",
                "hangar ingest aborted",
                "re-run ingest against a stable output, or quarantine and rebuild it from a trusted source.",
            ),
        ];

        for (error, code, what, fix) in cases {
            let diagnostic = error.diagnostic();
            assert_eq!(diagnostic.code, code);
            assert_eq!(diagnostic.what, what);
            assert_eq!(diagnostic.fix, fix);
            assert!(diagnostic.why.ends_with("detail") || diagnostic.why == "invalid derivation");

            let provider_error: ProviderError = error.into();
            assert_eq!(provider_error.code().unwrap_or("E1340"), code);
        }
    }
}
