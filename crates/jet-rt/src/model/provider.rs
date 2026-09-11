//! ONNX Runtime 1.29.0 provider for the runtime-owned model package.
//!
//! The provider is deliberately an adapter, not a second model registry or
//! loader. Package selection, artifact identity, tensor contracts, limits,
//! cancellation, and diagnostics remain owned by the runtime model package;
//! this module only marshals an already checked package into one of the two
//! supported runtime paths.
//!
//! Native execution uses the ONNX Runtime C API through a small, pinned,
//! dynamically loaded bridge. There is no ONNX Rust wrapper dependency. Web
//! execution receives the same checked bytes and policy through an explicit
//! host callback. A host that cannot provide a web runtime returns the ordinary
//! model-load error; it never gets a fabricated tensor result.
use super::{
    BertWordPieceTokenizer, EncodedBatch, EmbeddingBatch, ModelArtifact, ModelError, ModelPackage,
    ModelProvider, ModelSource, VerifiedModel, TensorDType, TensorDimension, TensorSpec,
};

use crate::SHA256;
use std::collections::BTreeSet;
use std::fmt::{self, Write as _};
use std::future::Future;
use std::pin::Pin;
use std::os::raw::c_int;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake};
use std::thread;
use std::time::{Duration, Instant};

/// The owner-approved ONNX Runtime release selected by D-MODEL-BACKEND1=A.
pub const ONNX_RUNTIME_VERSION: &str = "1.29.0";
/// Release date reported by the official release API.
pub const ONNX_RUNTIME_RELEASE_DATE: &str = "2026-08-12";
/// Official release page for the pinned runtime.
pub const ONNX_RUNTIME_RELEASE_URL: &str =
    "https://github.com/microsoft/onnxruntime/releases/tag/v1.29.0";
/// Official C API reference used by this adapter.
pub const ONNX_RUNTIME_C_API_URL: &str = "https://onnxruntime.ai/docs/api/c/";
/// Official download URL for the admitted Linux x64 archive.
pub const ONNX_RUNTIME_LINUX_X64_ARTIFACT_URL: &str =
    "https://github.com/microsoft/onnxruntime/releases/download/v1.29.0/onnxruntime-linux-x64-1.29.0.tgz";
/// Linux x64 release archive recorded in provider provenance.
pub const ONNX_RUNTIME_LINUX_X64_ARTIFACT: &str = "onnxruntime-linux-x64-1.29.0.tgz";
/// SHA-256 reported by the official release API for the Linux x64 archive.
pub const ONNX_RUNTIME_LINUX_X64_ARTIFACT_SHA256: &str =
    "c3fddc4f139a045b0c4902c57410f0694f1c2fdf9b6939fbe38b1aeae7cd14ba";
/// Official npm download URL for the admitted web package.
pub const ONNX_RUNTIME_WEB_ARTIFACT_URL: &str =
    "https://registry.npmjs.org/onnxruntime-web/-/onnxruntime-web-1.29.0.tgz";
/// Web package artifact name and pinned SHA-256.
pub const ONNX_RUNTIME_WEB_ARTIFACT: &str = "onnxruntime-web-1.29.0.tgz";
pub const ONNX_RUNTIME_WEB_ARTIFACT_SHA256: &str =
    "7a934b7811c3b050ecfb7619722e2b4de771ce6da20520e17a2018a440316ef3";
/// Package identity sent to a browser host for the web runtime.
pub const ONNX_RUNTIME_WEB_PACKAGE: &str = "onnxruntime-web@1.29.0";
/// Shared-library name inside the admitted Linux x64 archive.
pub const ONNX_RUNTIME_LINUX_X64_LIBRARY: &str = "libonnxruntime.so.1.29.0";
/// SHA-256 of the shared library inside the admitted Linux x64 archive.
pub const ONNX_RUNTIME_LINUX_X64_LIBRARY_SHA256: &str =
    "5715f06d8992ca8eeeddcce43df3a7d38f97d537052126f558e912cb312460ca";
/// Shared provider library shipped beside the admitted Linux x64 runtime.
pub const ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY: &str = "libonnxruntime_providers_shared.so";
/// SHA-256 of the shared provider library shipped in the archive.
pub const ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY_SHA256: &str =
    "086ec1d5388f64153d9c63470d126693db9a182c8ce236d3a1119068471b8a0d";
/// Source commit recorded by the official v1.29.0 release.
pub const ONNX_RUNTIME_RELEASE_COMMIT: &str = "2e2543fbe9fae542f921d47a72d21d5a4ef0b710";
/// ONNX Runtime's provider identity in a `.Model` package contract.
pub const ONNX_PACKAGE_PROVIDER: &str = "onnxruntime@1.29.0";
/// The license pinned for the ONNX Runtime distribution used by this adapter.
pub const ONNX_RUNTIME_LICENSE: &str = "MIT";
/// The provenance record schema emitted by this provider.
pub const ONNX_PROVENANCE_SCHEMA: &str = "jet-model-onnxruntime-provenance-v1";
/// The only execution provider in the initial, qualified support matrix.
pub const CPU_EXECUTION_PROVIDER: &str = "CPUExecutionProvider";

const RUNTIME_ARTIFACT_PATH: &str = "<onnxruntime>";
const INFERENCE_ARTIFACT_PATH: &str = "<onnxruntime-inference>";
const WEB_RUNTIME_ARTIFACT_PATH: &str = "<onnxruntime-web>";
const MAX_WARM_RUNS: usize = 10_000;
#[path = "graph.rs"]
mod graph;
#[path = "web.rs"]
pub mod browser;

/// Execute a provider future on a synchronous generated host boundary.
///
/// Native generated applications have no async executor in the emitted
/// adapter.  Provider operations are required to complete in one poll; a
/// future that would suspend is reported as a typed resource error rather
/// than being silently fabricated or blocked forever.
pub fn run_ready<T>(
    future: impl Future<Output = Result<T, ModelError>>,
) -> Result<T, ModelError> {
    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }
    let waker = std::task::Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match Future::poll(future.as_mut(), &mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => Err(ModelError::ResourceLimit {
            package: "<model-runtime>".to_string(),
            reason: "model operation suspended on synchronous host".to_string(),
        }),
    }
}
    struct RuntimeSignature {
        dtype: TensorDType,
        dimensions: Vec<i64>,
    }

    fn validate_declared_signature(
        package: &ModelPackage,
        expected: &TensorSpec,
        actual_name: &str,
        actual: RuntimeSignature,
    ) -> Result<(), ModelError> {
        if expected.name != actual_name || expected.dtype != actual.dtype || expected.shape.dimensions.len() != actual.dimensions.len() {
            return Err(ModelError::SignatureMismatch {
                package: package.package.clone(),
                expected: expected.to_string(),
                actual: format!("{}:{}[{}]", actual_name, actual.dtype, actual.dimensions.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")),
            });
        }
        for (expected_dimension, actual_dimension) in expected.shape.dimensions.iter().zip(actual.dimensions) {
            match (expected_dimension, actual_dimension) {
                (TensorDimension::Static(expected), actual) if actual >= 0 && *expected == actual as u64 => {}
                (TensorDimension::Dynamic { min, max, .. }, actual) if actual < 0 || (*min..=*max).contains(&(actual as u64)) => {}
                _ => {
                    return Err(ModelError::SignatureMismatch {
                        package: package.package.clone(),
                        expected: expected.to_string(),
                        actual: format!("runtime signature for `{}` has incompatible dimensions", expected.name),
                    });
                }
            }
        }
        Ok(())
    }
    fn map_tensor_dtype(dtype: TensorDType) -> c_int {
        match dtype {
            TensorDType::F32 => 1,
            TensorDType::U8 => 2,
            TensorDType::I8 => 3,
            TensorDType::U16 => 4,
            TensorDType::I16 => 5,
            TensorDType::I32 => 6,
            TensorDType::I64 => 7,
            TensorDType::Bool => 9,
            TensorDType::F16 => 10,
            TensorDType::F64 => 11,
            TensorDType::U32 => 12,
            TensorDType::U64 => 13,
            TensorDType::BF16 => 16,
        }
    }

    fn map_onnx_dtype(dtype: c_int) -> Option<TensorDType> {
        Some(match dtype {
            1 => TensorDType::F32,
            2 => TensorDType::U8,
            3 => TensorDType::I8,
            4 => TensorDType::U16,
            5 => TensorDType::I16,
            6 => TensorDType::I32,
            7 => TensorDType::I64,
            9 => TensorDType::Bool,
            10 => TensorDType::F16,
            11 => TensorDType::F64,
            12 => TensorDType::U32,
            13 => TensorDType::U64,
            16 => TensorDType::BF16,
            _ => return None,
        })
    }
// Values are stable in the pinned ONNX Runtime C API's `OrtErrorCode`.
const ORT_CODE_NOT_IMPLEMENTED: i32 = 9;
const ORT_CODE_EP_FAIL: i32 = 11;
const ORT_CODE_MODEL_LOAD_CANCELED: i32 = 12;

/// Native or web execution.  The package-facing contract is identical for
/// both modes; the mode is retained only in provenance and measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OnnxRuntimeMode {
    Native,
    Web,
}

impl OnnxRuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Web => "web",
        }
    }
}

/// Numerical-output promise made by a provider and recorded in provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputPolicy {
    /// Outputs are compared byte-for-byte after the declared dtype/shape check.
    Exact,
    /// The provider may use a declared approximate numerical mode; callers
    /// must compare with a domain tolerance rather than byte equality.
    Approximate,
}

impl OutputPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Approximate => "approximate",
        }
    }
}
/// Policy for model-declared executable extensions.  The initial provider
/// never loads custom operators or other model-controlled code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CustomCodePolicy {
    Denied,
}

impl CustomCodePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
        }
    }
}


/// A pinned ONNX Runtime distribution.  Native providers verify the exact
/// file bytes before calling `dlopen`; web providers retain the same identity
/// in their provenance and let the browser host perform its own byte check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePin {
    pub version: String,
    pub path: PathBuf,
    pub sha256: String,
    pub license: String,
}

impl RuntimePin {
    /// Construct the native runtime pin with the ratified version and license.
    pub fn new(path: impl Into<PathBuf>, sha256: impl Into<String>) -> Result<Self, ModelError> {
        Self::from_parts(ONNX_RUNTIME_VERSION, path, sha256, ONNX_RUNTIME_LICENSE)
    }

    /// Construct the admitted Linux x64 pin without accepting a caller-chosen
    /// version or digest. The file is still hash-checked immediately before
    /// dynamic loading.
    pub fn official_linux_x64(path: impl Into<PathBuf>) -> Result<Self, ModelError> {
        Self::from_parts(
            ONNX_RUNTIME_VERSION,
            path,
            ONNX_RUNTIME_LINUX_X64_LIBRARY_SHA256,
            ONNX_RUNTIME_LICENSE,
        )
    }

    /// Construct a web-runtime identity.  The web host owns the actual
    /// loading mechanism; `path` is retained only as an explicit identity
    /// label and must not be interpreted as a network URL.
    pub fn web(sha256: impl Into<String>) -> Result<Self, ModelError> {
        Self::from_parts(ONNX_RUNTIME_VERSION, WEB_RUNTIME_ARTIFACT_PATH, sha256, ONNX_RUNTIME_LICENSE)
    }

    /// Construct the admitted ONNX Runtime Web package pin.  A browser host
    /// must bind this identity to its locally supplied package bytes; this
    /// constructor never downloads or discovers a package.
    pub fn official_web() -> Result<Self, ModelError> {
        Self::from_parts(
            ONNX_RUNTIME_VERSION,
            WEB_RUNTIME_ARTIFACT_PATH,
            ONNX_RUNTIME_WEB_ARTIFACT_SHA256,
            ONNX_RUNTIME_LICENSE,
        )
    }

    /// Build a pin while requiring the owner-approved release and license.
    pub fn from_parts(
        version: impl Into<String>,
        path: impl Into<PathBuf>,
        sha256: impl Into<String>,
        license: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let pin = Self {
            version: version.into(),
            path: path.into(),
            sha256: sha256.into(),
            license: license.into(),
        };
        pin.validate_metadata()?;
        Ok(pin)
    }

    fn validate_metadata(&self) -> Result<(), ModelError> {
        if self.version != ONNX_RUNTIME_VERSION {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!(
                    "runtime version `{}` is not the pinned ONNX Runtime {ONNX_RUNTIME_VERSION}",
                    self.version
                ),
            ));
        }
        if self.license != ONNX_RUNTIME_LICENSE {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!(
                    "runtime license `{}` is not the pinned {ONNX_RUNTIME_LICENSE} license",
                    self.license
                ),
            ));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                "runtime identity is not a lowercase SHA-256",
            ));
        }
        if self.path.as_os_str().is_empty()
            || self
                .path
                .as_os_str()
                .to_string_lossy()
                .bytes()
                .any(|byte| byte.is_ascii_control())
            || self.path.to_string_lossy().contains("://")
        {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                "runtime path is empty, a URL, or contains a control character",
            ));
        }
        Ok(())
    }

    #[cfg(any(unix, windows))]
    fn verify_native_file(&self) -> Result<(), ModelError> {
        self.validate_metadata()?;
        let metadata = std::fs::symlink_metadata(&self.path).map_err(|error| {
            provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!("cannot inspect pinned runtime `{}`: {error}", self.path.display()),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!("pinned runtime `{}` is not a regular file", self.path.display()),
            ));
        }
        let bytes = std::fs::read(&self.path).map_err(|error| {
            provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!("cannot read pinned runtime `{}`: {error}", self.path.display()),
            )
        })?;
        let actual = SHA256::sha256_hex(&bytes);
        if actual != self.sha256 {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!(
                    "pinned runtime hash is `{actual}`, expected `{}`",
                    self.sha256
                ),
            ));
        }
        Ok(())
    }
    #[cfg(unix)]
    fn verify_native_provider_file(&self) -> Result<(), ModelError> {
        // The admitted Linux release loads this shared provider beside the
        // main library.  Verify the exact companion bytes before ORT can
        // resolve them through the dynamic loader.
        if self.sha256 != ONNX_RUNTIME_LINUX_X64_LIBRARY_SHA256 {
            return Ok(());
        }
        let directory = self.path.parent().unwrap_or_else(|| Path::new("."));
        let path = directory.join(ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!("cannot inspect pinned provider `{}`: {error}", path.display()),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!("pinned provider `{}` is not a regular file", path.display()),
            ));
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!("cannot read pinned provider `{}`: {error}", path.display()),
            )
        })?;
        let actual = SHA256::sha256_hex(&bytes);
        if actual != ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY_SHA256 {
            return Err(provenance_error(
                RUNTIME_ARTIFACT_PATH,
                format!(
                    "pinned provider hash is `{actual}`, expected `{ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY_SHA256}`"
                ),
            ));
        }
        Ok(())
    }
}

/// The provider closure pinned alongside a package.  The first supported
/// release deliberately qualifies CPU only; accepting a provider name and
/// silently letting ONNX Runtime fall back to CPU would violate I9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnnxRuntimePolicy {
    pub execution_providers: Vec<String>,
    pub enabled_operators: BTreeSet<String>,
    pub output_policy: OutputPolicy,
    pub custom_code: CustomCodePolicy,
    /// Always true for this provider.  There is no download fallback.
    pub offline: bool,
}
impl OnnxRuntimePolicy {
    /// Create the initial CPU support matrix with an explicit operator closure.
    pub fn cpu<I, S>(enabled_operators: I) -> Result<Self, ModelError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let policy = Self {
            execution_providers: vec![CPU_EXECUTION_PROVIDER.to_string()],
            enabled_operators: enabled_operators.into_iter().map(Into::into).collect(),
            output_policy: OutputPolicy::Exact,
            custom_code: CustomCodePolicy::Denied,
            offline: true,
        };
        policy.validate("<provider>")?;
        Ok(policy)
    }

    /// Build the explicit CPU closure from an authenticated ONNX graph.
    ///
    /// The package descriptor still controls artifact identity and custom-code
    /// policy; this only avoids duplicating a graph's standard operator list in
    /// generated adapters.
    pub fn cpu_for_graph(graph_bytes: &[u8]) -> Result<Self, ModelError> {
        let operators = graph::operator_names(graph_bytes)
            .map_err(|reason| provenance_error("<model-graph>", reason))?;
        Self::cpu(operators)
    }

    /// Select the declared numerical policy without changing provider or
    /// operator closure.
    pub fn with_output_policy(mut self, output_policy: OutputPolicy) -> Self {
        self.output_policy = output_policy;
        self
    }

    fn validate(&self, package: &str) -> Result<(), ModelError> {
        if !self.offline {
            return Err(provenance_error(
                package,
                "ONNX Runtime provider must remain offline; model downloads are undeclared",
            ));
        }
        if self.execution_providers != [CPU_EXECUTION_PROVIDER.to_string()] {
            let actual = self.execution_providers.join(", ");
            return Err(ModelError::UnsupportedProvider {
                package: package.to_string(),
                expected: CPU_EXECUTION_PROVIDER.to_string(),
                actual,
            });
        }
        if self.enabled_operators.is_empty() {
            return Err(provenance_error(
                package,
                "enabled ONNX operator closure must be pinned and non-empty",
            ));
        }
        if self
            .enabled_operators
            .iter()
            .any(|operator| operator.trim().is_empty() || operator.bytes().any(|byte| byte.is_ascii_control()))
        {
            return Err(provenance_error(
                package,
                "enabled ONNX operator closure contains an empty or control-bearing name",
            ));
        }
        Ok(())
    }
}


/// A concrete tensor buffer crossing the provider boundary.  The bytes are
/// caller-owned for the duration of `run`; a native session copies outputs
/// before releasing the corresponding ORT values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorData {
    pub spec: TensorSpec,
    pub bytes: Vec<u8>,
}

impl TensorData {
    pub fn new(spec: TensorSpec, bytes: Vec<u8>) -> Result<Self, ModelError> {
        let value = Self { spec, bytes };
        value.validate("<tensor>")?;
        Ok(value)
    }

    fn validate(&self, package: &str) -> Result<(), ModelError> {
        let mut elements = 1usize;
        for dimension in &self.spec.shape.dimensions {
            let TensorDimension::Static(value) = dimension else {
                return Err(declaration_error(
                    package,
                    format!("tensor `{}` needs a concrete runtime shape", self.spec.name),
                ));
            };
            let value = usize::try_from(*value).map_err(|_| {
                declaration_error(package, format!("tensor `{}` shape exceeds host size", self.spec.name))
            })?;
            if value == 0 {
                return Err(declaration_error(
                    package,
                    format!("tensor `{}` has a zero dimension", self.spec.name),
                ));
            }
            elements = elements.checked_mul(value).ok_or_else(|| {
                declaration_error(package, format!("tensor `{}` element count overflows", self.spec.name))
            })?;
        }
        let element_size = dtype_size(self.spec.dtype);
        let expected = elements.checked_mul(element_size).ok_or_else(|| {
            declaration_error(package, format!("tensor `{}` byte count overflows", self.spec.name))
        })?;
        if self.bytes.len() != expected {
            return Err(ModelError::SignatureMismatch {
                package: package.to_string(),
                expected: format!("{} bytes", expected),
                actual: format!("{} bytes", self.bytes.len()),
            });
        }
        Ok(())
    }
}

/// A cancellation token shared by the caller and a provider session.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<CancellationState>);

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: Arc<AtomicBool>,
    // Only suspended browser operations register a wakeup.
    waiters: std::sync::Mutex<std::collections::BTreeMap<u32, std::task::Waker>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Share the scheduler-owned cancellation flag without inventing a second
    /// scope.  Callers register a scheduler callback that invokes `cancel` so
    /// suspended provider futures also receive their wakeup.
    pub fn from_cancel_flag(cancelled: Arc<AtomicBool>) -> Self {
        Self(Arc::new(CancellationState {
            cancelled,
            waiters: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }))
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        let waiters = std::mem::take(&mut *self.0.waiters.lock().unwrap_or_else(|e| e.into_inner()));
        for waker in waiters.into_values() {
            waker.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
}

/// The complete reproducibility record retained by both provider modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnnxRuntimeProvenance {
    pub mode: OnnxRuntimeMode,
    pub package: String,
    pub package_version: String,
    pub package_license: String,
    pub model_identity_digest: String,
    pub artifacts: Vec<ModelArtifact>,
    pub runtime_version: String,
    pub runtime_path: String,
    pub runtime_sha256: String,
    pub runtime_license: String,
    pub enabled_operators: Vec<String>,
    pub execution_providers: Vec<String>,
    pub custom_code_policy: String,
    pub download_policy: String,
    pub output_policy: OutputPolicy,
}

impl OnnxRuntimeProvenance {
    fn for_package(
        package: &ModelPackage,
        pin: &RuntimePin,
        policy: &OnnxRuntimePolicy,
        mode: OnnxRuntimeMode,
    ) -> Self {
        let mut enabled_operators: Vec<_> = policy.enabled_operators.iter().cloned().collect();
        enabled_operators.sort();
        Self {
            mode,
            package: package.package.clone(),
            package_version: package.identity.package_version.clone(),
            package_license: package.identity.license.clone(),
            model_identity_digest: package.identity.digest(),
            artifacts: package.artifacts.clone(),
            runtime_version: pin.version.clone(),
            runtime_path: pin.path.display().to_string(),
            runtime_sha256: pin.sha256.clone(),
            runtime_license: pin.license.clone(),
            enabled_operators,
            execution_providers: policy.execution_providers.clone(),
            custom_code_policy: policy.custom_code.as_str().to_string(),
            download_policy: "offline;undeclared-downloads-denied".to_string(),
            output_policy: policy.output_policy,
        }
    }

    /// Render one deterministic, line-oriented record suitable for a lock or
    /// measurement artifact.  Values are escaped so package-controlled text
    /// cannot inject an extra provenance row.
    pub fn render(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "provenance-schema={ONNX_PROVENANCE_SCHEMA}");
        let _ = writeln!(output, "mode={}", self.mode.as_str());
        let _ = writeln!(output, "package={}", provenance_escape(&self.package));
        let _ = writeln!(output, "package-version={}", provenance_escape(&self.package_version));
        let _ = writeln!(output, "package-license={}", provenance_escape(&self.package_license));
        let _ = writeln!(
            output,
            "model-identity-digest={}",
            self.model_identity_digest
        );
        let _ = writeln!(output, "runtime-release-date={ONNX_RUNTIME_RELEASE_DATE}");
        let _ = writeln!(output, "runtime-release-commit={ONNX_RUNTIME_RELEASE_COMMIT}");
        let _ = writeln!(output, "runtime-release-url={ONNX_RUNTIME_RELEASE_URL}");
        let _ = writeln!(output, "runtime-c-api-url={ONNX_RUNTIME_C_API_URL}");
        let (artifact, artifact_url, artifact_sha256) = match self.mode {
            OnnxRuntimeMode::Native => (
                ONNX_RUNTIME_LINUX_X64_ARTIFACT,
                ONNX_RUNTIME_LINUX_X64_ARTIFACT_URL,
                ONNX_RUNTIME_LINUX_X64_ARTIFACT_SHA256,
            ),
            OnnxRuntimeMode::Web => (
                ONNX_RUNTIME_WEB_ARTIFACT,
                ONNX_RUNTIME_WEB_ARTIFACT_URL,
                ONNX_RUNTIME_WEB_ARTIFACT_SHA256,
            ),
        };
        let _ = writeln!(output, "runtime-artifact={artifact}");
        let _ = writeln!(output, "runtime-artifact-url={artifact_url}");
        let _ = writeln!(output, "runtime-artifact-sha256={artifact_sha256}");
        if self.mode == OnnxRuntimeMode::Native {
            let _ = writeln!(output, "runtime-library={ONNX_RUNTIME_LINUX_X64_LIBRARY}");
            let _ = writeln!(output, "runtime-library-sha256={ONNX_RUNTIME_LINUX_X64_LIBRARY_SHA256}");
            let _ = writeln!(
                output,
                "runtime-provider-library={ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY}"
            );
            let _ = writeln!(
                output,
                "runtime-provider-library-sha256={ONNX_RUNTIME_LINUX_X64_PROVIDER_LIBRARY_SHA256}"
            );
        } else {
            let _ = writeln!(output, "runtime-web-package={ONNX_RUNTIME_WEB_PACKAGE}");
        }
        let _ = writeln!(output, "runtime-version={}", self.runtime_version);
        let _ = writeln!(output, "runtime-path={}", provenance_escape(&self.runtime_path));
        let _ = writeln!(output, "runtime-sha256={}", self.runtime_sha256);
        let _ = writeln!(output, "runtime-license={}", self.runtime_license);
        let _ = writeln!(output, "custom-code-policy={}", self.custom_code_policy);
        let _ = writeln!(output, "download-policy={}", self.download_policy);
        let _ = writeln!(output, "output-policy={}", self.output_policy.as_str());
        for (index, artifact) in self.artifacts.iter().enumerate() {
            let role = match index {
                0 => "graph",
                1 => "weights",
                2 => "tokenizer",
                _ => "adapter",
            };
            let _ = writeln!(
                output,
                "artifact={}|{}|{}",
                provenance_escape(&artifact.path),
                artifact.sha256,
                role,
            );
        }
        for provider in &self.execution_providers {
            let _ = writeln!(output, "execution-provider={}", provenance_escape(provider));
        }
        for operator in &self.enabled_operators {
            let _ = writeln!(output, "enabled-operator={}", provenance_escape(operator));
        }
        output
    }
}

/// The browser receives already verified package bytes. It owns runtime
/// handles, not package validation, provider selection, or numerical policy.
pub struct WebOpenRequest<'a> {
    pub package: &'a ModelPackage,
    pub policy: &'a OnnxRuntimePolicy,
    pub provenance: &'a OnnxRuntimeProvenance,
    pub graph: &'a [u8],
    pub cancellation: &'a CancellationToken,
}

pub type WebFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ModelError>> + 'a>>;

/// Browser creation and execution must yield, including CPU WASM execution.
/// Dropping an unfinished operation cancels its foreign work.
pub trait WebSessionHandle {
    fn run<'a>(
        &'a mut self,
        inputs: &'a [TensorData],
        cancellation: &'a CancellationToken,
    ) -> WebFuture<'a, Vec<TensorData>>;
}

pub trait WebRuntime: Send + Sync {
    fn open<'a>(&'a self, request: WebOpenRequest<'a>)
        -> WebFuture<'a, Box<dyn WebSessionHandle>>;
}

/// Native provider configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOnnxProvider {
    pub runtime: RuntimePin,
    pub policy: OnnxRuntimePolicy,
}

impl NativeOnnxProvider {
    pub fn new(runtime: RuntimePin, policy: OnnxRuntimePolicy) -> Result<Self, ModelError> {
        runtime.validate_metadata()?;
        policy.validate("<provider>")?;
        Ok(Self { runtime, policy })
    }

    fn open(
        &self,
        model: VerifiedModel<'_>,
        _cancellation: &CancellationToken,
    ) -> Result<OnnxRuntimeSession, ModelError> {
        let tokenizer = model.artifacts.get(2).cloned().ok_or_else(|| {
            provenance_error(
                &model.package.package,
                "ONNX model session is missing the checked tokenizer artifact",
            )
        })?;
        let package = model.package;
        let prepared = prepare_package(model, &self.policy)?;
        let provenance = OnnxRuntimeProvenance::for_package(
            package,
            &self.runtime,
            &self.policy,
            OnnxRuntimeMode::Native,
        );
        let session = native::Session::open(package, prepared, &self.runtime, &self.policy)?;
        Ok(OnnxRuntimeSession::Native(NativeSession {
            session,
            package: package.clone(),
            provenance,
            tokenizer,
        }))
    }
}

/// Web provider configuration.  It has the same package policy and error
/// meaning as [`NativeOnnxProvider`], but delegates actual execution to a
/// browser-supported host implementation.
#[derive(Clone)]
pub struct WebOnnxProvider {
    pub runtime: RuntimePin,
    pub policy: OnnxRuntimePolicy,
    runtime_host: Arc<dyn WebRuntime>,
}

impl fmt::Debug for WebOnnxProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebOnnxProvider")
            .field("runtime", &self.runtime)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl WebOnnxProvider {
    pub fn new(
        runtime: RuntimePin,
        policy: OnnxRuntimePolicy,
        runtime_host: Arc<dyn WebRuntime>,
    ) -> Result<Self, ModelError> {
        runtime.validate_metadata()?;
        policy.validate("<provider>")?;
        Ok(Self {
            runtime,
            policy,
            runtime_host,
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub fn browser(policy: OnnxRuntimePolicy) -> Result<Self, ModelError> {
        Self::new(RuntimePin::official_web()?, policy, Arc::new(browser::BrowserWebRuntime))
    }

    async fn open(
        &self,
        model: VerifiedModel<'_>,
        cancellation: &CancellationToken,
    ) -> Result<OnnxRuntimeSession, ModelError> {
        let tokenizer = model.artifacts.get(2).cloned().ok_or_else(|| {
            provenance_error(
                &model.package.package,
                "ONNX model session is missing the checked tokenizer artifact",
            )
        })?;
        let package = model.package;
        let prepared = prepare_package(model, &self.policy)?;
        let provenance = OnnxRuntimeProvenance::for_package(
            package,
            &self.runtime,
            &self.policy,
            OnnxRuntimeMode::Web,
        );
        let handle = self
            .runtime_host
            .open(WebOpenRequest {
                package,
                policy: &self.policy,
                provenance: &provenance,
                graph: prepared.graph()?.bytes.as_slice(),
                cancellation,
            })
            .await?;
        Ok(OnnxRuntimeSession::Web(WebSession {
            package: package.clone(),
            provenance,
            handle,
            tokenizer,
        }))
    }
}

/// One provider choice, with one `ModelProvider` implementation and one
/// session result type for both target modes.
#[derive(Debug, Clone)]
pub enum OnnxRuntimeProvider {
    Native(NativeOnnxProvider),
    Web(WebOnnxProvider),
}

impl OnnxRuntimeProvider {
    pub fn native(runtime: RuntimePin, policy: OnnxRuntimePolicy) -> Result<Self, ModelError> {
        Ok(Self::Native(NativeOnnxProvider::new(runtime, policy)?))
    }

    pub fn web(
        runtime: RuntimePin,
        policy: OnnxRuntimePolicy,
        runtime_host: Arc<dyn WebRuntime>,
    ) -> Result<Self, ModelError> {
        Ok(Self::Web(WebOnnxProvider::new(runtime, policy, runtime_host)?))
    }

    pub fn mode(&self) -> OnnxRuntimeMode {
        match self {
            Self::Native(_) => OnnxRuntimeMode::Native,
            Self::Web(_) => OnnxRuntimeMode::Web,
        }
    }

    fn policy(&self) -> &OnnxRuntimePolicy {
        match self {
            Self::Native(provider) => &provider.policy,
            Self::Web(provider) => &provider.policy,
        }
    }

    /// Run one matched workload and retain the exact observations needed by a
    /// same-run comparison.  Peak memory is `None` unless the caller supplies
    /// a real sampler; no number is invented for an unavailable sampler.
    pub async fn measure<'a>(
        &self,
        package: &ModelPackage,
        source: impl Into<ModelSource<'a>>,
        inputs: &[TensorData],
        warm_runs: usize,
        sampler: &mut dyn PeakMemorySampler,
    ) -> Result<ProviderMeasurement, ModelError> {
        if warm_runs == 0 || warm_runs > MAX_WARM_RUNS {
            return Err(declaration_error(
                &package.package,
                format!("warm run count must be between 1 and {MAX_WARM_RUNS}"),
            ));
        }
        let cancellation = CancellationToken::new();
        let input_bytes_per_run = validate_inputs(package, inputs, &cancellation)?;
        let input_transfer_bytes = input_bytes_per_run
            .checked_mul(warm_runs as u64)
            .ok_or_else(|| ModelError::BufferLimit {
                package: package.package.clone(),
                bytes: u64::MAX,
            })?;
        let workload_digest = workload_digest(inputs);
        let load_start = clock_start();
        let mut session = package.open_with(source, self, &cancellation).await?;
        let load_time_ns = elapsed_ns(load_start);
        let mut peak_memory_bytes = sampler.sample_bytes();
        let mut warm_inference_ns = Vec::with_capacity(warm_runs);
        let mut output_transfer_bytes = 0u64;
        let mut output_digest = String::new();
        for _ in 0..warm_runs {
            let started = clock_start();
            let outputs = session.run(inputs, &cancellation).await?;
            warm_inference_ns.push(elapsed_ns(started));
            let output_bytes = outputs.iter().try_fold(0u64, |total, output| {
                total.checked_add(output.bytes.len() as u64).ok_or_else(|| {
                    ModelError::BufferLimit {
                        package: package.package.clone(),
                        bytes: u64::MAX,
                    }
                })
            })?;
            output_transfer_bytes = output_transfer_bytes
                .checked_add(output_bytes)
                .ok_or_else(|| ModelError::BufferLimit {
                    package: package.package.clone(),
                    bytes: u64::MAX,
                })?;
            output_digest = output_digest_for(&outputs);
            if let Some(observed) = sampler.sample_bytes() {
                peak_memory_bytes = Some(peak_memory_bytes.map_or(observed, |old| old.max(observed)));
            }
        }
        Ok(ProviderMeasurement {
            mode: self.mode(),
            provider: ONNX_PACKAGE_PROVIDER.to_string(),
            model_identity_digest: package.identity.digest(),
            workload_digest,
            load_time_ns,
            warm_inference_ns,
            input_transfer_bytes,
            output_transfer_bytes,
            peak_memory_bytes,
            output_policy: self.policy().output_policy,
            output_digest,
            provenance: self.provenance_for(package),
        })
    }

    fn provenance_for(&self, package: &ModelPackage) -> String {
        match self {
            Self::Native(provider) => OnnxRuntimeProvenance::for_package(
                package,
                &provider.runtime,
                &provider.policy,
                OnnxRuntimeMode::Native,
            )
            .render(),
            Self::Web(provider) => OnnxRuntimeProvenance::for_package(
                package,
                &provider.runtime,
                &provider.policy,
                OnnxRuntimeMode::Web,
            )
            .render(),
        }
    }

}

impl ModelProvider for OnnxRuntimeProvider {
    type Session = OnnxRuntimeSession;

    async fn open(
        &self,
        model: VerifiedModel<'_>,
        cancellation: &CancellationToken,
    ) -> Result<Self::Session, ModelError> {
        match self {
            Self::Native(provider) => provider.open(model, cancellation),
            Self::Web(provider) => provider.open(model, cancellation).await,
        }
    }
}

/// A provider-owned session with identical typed operations in native and web
/// modes.
pub enum OnnxRuntimeSession {
    Native(NativeSession),
    Web(WebSession),
}

impl OnnxRuntimeSession {
    pub fn mode(&self) -> OnnxRuntimeMode {
        match self {
            Self::Native(_) => OnnxRuntimeMode::Native,
            Self::Web(_) => OnnxRuntimeMode::Web,
        }
    }

    pub fn provenance(&self) -> &OnnxRuntimeProvenance {
        match self {
            Self::Native(session) => &session.provenance,
            Self::Web(session) => &session.provenance,
        }
    }

    pub async fn run(
        &mut self,
        inputs: &[TensorData],
        cancellation: &CancellationToken,
    ) -> Result<Vec<TensorData>, ModelError> {
        match self {
            Self::Native(session) => session.run(inputs, cancellation),
            Self::Web(session) => session.run(inputs, cancellation).await,
        }
    }
    /// Tokenize and execute one document batch through the checked package
    /// boundary, then return the private space-bound embedding carrier.
    pub async fn embed_documents(
        &mut self,
        documents: &[String],
        cancellation: &CancellationToken,
    ) -> Result<EmbeddingBatch, ModelError> {
        let (package, tokenizer_bytes) = match self {
            Self::Native(session) => (session.package.clone(), session.tokenizer.clone()),
            Self::Web(session) => (session.package.clone(), session.tokenizer.clone()),
        };
        let tokenizer = BertWordPieceTokenizer::from_json(&package, &tokenizer_bytes)?;
        let encoded = tokenizer.encode_documents(&package, documents)?;
        let inputs = tensors_from_encoded(&package, &encoded)?;
        let outputs = self.run(&inputs, cancellation).await?;
        let [output] = outputs.as_slice() else {
            return Err(ModelError::signature(
                &package.package,
                "one hidden-state output tensor",
                format!("{} output tensors", outputs.len()),
            ));
        };
        package.embedding_batch_from_output(&encoded, &output.spec, &output.bytes)
    }
}

fn tensors_from_encoded(
    package: &ModelPackage,
    encoded: &EncodedBatch,
) -> Result<Vec<TensorData>, ModelError> {
    package.check_batch(encoded.documents.len() as u64)?;
    if encoded.documents.is_empty() || encoded.sequence_length == 0 {
        return Err(ModelError::declaration(
            &package.package,
            "encoded document batch must not be empty",
        ));
    }
    let batch = encoded.documents.len();
    let sequence_length = encoded.sequence_length;
    if encoded.documents.iter().any(|document| {
        document.input_ids.len() != sequence_length
            || document.attention_mask.len() != sequence_length
            || document.token_type_ids.len() != sequence_length
    }) {
        return Err(ModelError::signature(
            &package.package,
            format!("all encoded tensors have sequence length {sequence_length}"),
            "encoded tensors have inconsistent sequence lengths",
        ));
    }
    let mut input_ids = Vec::with_capacity(batch * sequence_length * std::mem::size_of::<i64>());
    let mut attention_mask =
        Vec::with_capacity(batch * sequence_length * std::mem::size_of::<i64>());
    let mut token_type_ids =
        Vec::with_capacity(batch * sequence_length * std::mem::size_of::<i64>());
    for document in &encoded.documents {
        for value in &document.input_ids {
            input_ids.extend_from_slice(&value.to_le_bytes());
        }
        for value in &document.attention_mask {
            attention_mask.extend_from_slice(&value.to_le_bytes());
        }
        for value in &document.token_type_ids {
            token_type_ids.extend_from_slice(&value.to_le_bytes());
        }
    }
    let inputs = vec![
        TensorData::new(
            TensorSpec::runtime("input_ids", TensorDType::I64, [batch as u64, sequence_length as u64]),
            input_ids,
        )?,
        TensorData::new(
            TensorSpec::runtime("attention_mask", TensorDType::I64, [batch as u64, sequence_length as u64]),
            attention_mask,
        )?,
        TensorData::new(
            TensorSpec::runtime("token_type_ids", TensorDType::I64, [batch as u64, sequence_length as u64]),
            token_type_ids,
        )?,
    ];
    validate_inputs(package, &inputs, &CancellationToken::new())?;
    Ok(inputs)
}

/// Native session wrapper.  The ORT session owns all runtime state; the
/// package and provenance copies keep the checked contract available for each
/// invocation and measurement.
pub struct NativeSession {
    session: native::Session,
    package: ModelPackage,
    provenance: OnnxRuntimeProvenance,
    tokenizer: Arc<Vec<u8>>,
}

impl NativeSession {
    fn run(
        &mut self,
        inputs: &[TensorData],
        cancellation: &CancellationToken,
    ) -> Result<Vec<TensorData>, ModelError> {
        self.session.run(&self.package, inputs, cancellation)
    }
}

/// Web session wrapper with the same post-host validation as native execution.
pub struct WebSession {
    package: ModelPackage,
    provenance: OnnxRuntimeProvenance,
    handle: Box<dyn WebSessionHandle>,
    tokenizer: Arc<Vec<u8>>,
}

impl WebSession {
    async fn run(
        &mut self,
        inputs: &[TensorData],
        cancellation: &CancellationToken,
    ) -> Result<Vec<TensorData>, ModelError> {
        let input_bytes = validate_inputs(&self.package, inputs, cancellation)?;
        let outputs = self.handle.run(inputs, cancellation).await?;
        if cancellation.is_cancelled() {
            return Err(self.package.cancelled());
        }
        let output_bytes = validate_outputs(&self.package, &outputs)?;
        let total_bytes = input_bytes.checked_add(output_bytes).ok_or_else(|| ModelError::BufferLimit {
            package: self.package.package.clone(),
            bytes: u64::MAX,
        })?;
        self.package.check_buffer(total_bytes)?;
        Ok(outputs)
    }
}

/// A real measurement source for peak memory.  Platform adapters may sample
/// RSS or a provider-specific allocator; an unavailable sampler must return
/// `None` and is retained as unavailable in the report.
pub trait PeakMemorySampler {
    fn sample_bytes(&mut self) -> Option<u64>;
}

/// Explicit no-observation sampler used by callers that cannot measure peak
/// memory on their target.
#[derive(Debug, Default)]
pub struct UnavailablePeakMemory;

impl PeakMemorySampler for UnavailablePeakMemory {
    fn sample_bytes(&mut self) -> Option<u64> {
        None
    }
}

/// One matched provider observation.  All values are measured from the same
/// package and input workload; the mode and full provenance prevent hidden
/// trust or tier differences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMeasurement {
    pub mode: OnnxRuntimeMode,
    pub provider: String,
    pub model_identity_digest: String,
    pub workload_digest: String,
    pub load_time_ns: u64,
    pub warm_inference_ns: Vec<u64>,
    pub input_transfer_bytes: u64,
    pub output_transfer_bytes: u64,
    pub peak_memory_bytes: Option<u64>,
    pub output_policy: OutputPolicy,
    pub output_digest: String,
    pub provenance: String,
}

/// Same-run comparison report.  `output_match` is `None` for approximate
/// output policy because this package contract carries no ratified domain
/// tolerance; the report is measurement evidence, not numeric-match proof.
pub struct SameRunComparison {
    pub workload_digest: String,
    pub output_policy: OutputPolicy,
    pub output_match: Option<bool>,
    pub observations: Vec<ProviderMeasurement>,
}

/// Run native/web providers (or multiple provider configurations) on exactly
/// the same model and input workload.  This is the measurement path for card
/// criterion 4; it performs no synthetic timing or memory estimation.
pub async fn compare_same_run<'a>(
    providers: &[&OnnxRuntimeProvider],
    package: &ModelPackage,
    source: impl Into<ModelSource<'a>>,
    inputs: &[TensorData],
    warm_runs: usize,
    sampler: &mut dyn PeakMemorySampler,
) -> Result<SameRunComparison, ModelError> {
    let source = source.into();
    if providers.is_empty() {
        return Err(declaration_error(&package.package, "same-run comparison needs a provider"));
    }
    let workload_digest = workload_digest(inputs);
    let mut observations = Vec::with_capacity(providers.len());
    for provider in providers {
        let observation = provider.measure(package, source, inputs, warm_runs, sampler).await?;
        if observation.workload_digest != workload_digest {
            return Err(provenance_error(
                &package.package,
                "provider measurement changed the matched workload identity",
            ));
        }
        observations.push(observation);
    }
    let output_policy = observations[0].output_policy;
    if observations.iter().any(|observation| observation.output_policy != output_policy) {
        return Err(provenance_error(
            &package.package,
            "native and web providers declared different output policies",
        ));
    }
    let output_match = if output_policy == OutputPolicy::Exact {
        let first = &observations[0].output_digest;
        Some(observations.iter().all(|observation| &observation.output_digest == first))
    } else {
        // No domain tolerance is ratified for approximate output, so this
        // comparison remains intentionally non-proof.
        None
    };
    Ok(SameRunComparison {
        workload_digest,
        output_policy,
        output_match,
        observations,
    })
}

fn prepare_package(
    model: VerifiedModel<'_>,
    policy: &OnnxRuntimePolicy,
) -> Result<PreparedPackage, ModelError> {
    let package = model.package;
    policy.validate(&package.package)?;
    package.require_provider(ONNX_PACKAGE_PROVIDER)?;
    package.check_custom_operators(false)?;
    validate_artifact_closure(package)?;
    let mut prepared = PreparedPackage {
        artifacts: model
            .artifacts
            .into_iter()
            .map(|bytes| PreparedArtifact { bytes })
            .collect(),
    };
    if prepared.graph()?.bytes.is_empty() {
        return Err(backend_error(
            &package.package,
            RUNTIME_ARTIFACT_PATH,
            "declared ONNX graph is empty",
        ));
    }
    // We require all three package-bound artifacts even if a particular graph
    // embeds its weights/tokenizer.  This keeps the package closure explicit
    // and prevents a runtime from reaching for undeclared side files.
    let _ = prepared.weights()?;
    let _ = prepared.tokenizer()?;
    // Inspect every nested operator and bind external tensors from the same
    // checked closure before either runtime can see the graph.
    let bound = graph::prepare(package, policy, &prepared.artifacts)?;
    if let Some(bytes) = bound {
        prepared.artifacts[0].bytes = Arc::new(bytes);
    }
    Ok(prepared)
}

fn validate_artifact_closure(package: &ModelPackage) -> Result<(), ModelError> {
    if package.artifacts.len() != 3 && package.artifacts.len() != 4 {
        return Err(provenance_error(
            &package.package,
            "ONNX package must declare graph, weights, tokenizer, and optional adapter artifacts",
        ));
    }
    let expected = [
        (&package.identity.graph_sha256, &package.artifacts[0].sha256),
        (&package.identity.weights_sha256, &package.artifacts[1].sha256),
        (&package.identity.tokenizer_sha256, &package.artifacts[2].sha256),
    ];
    if expected.iter().any(|(identity, artifact)| *identity != *artifact) {
        return Err(provenance_error(
            &package.package,
            "model identity hashes do not match the declared artifact closure",
        ));
    }
    if package.identity.adapter_sha256.as_deref()
        != package.artifacts.get(3).map(|artifact| artifact.sha256.as_str())
    {
        return Err(provenance_error(
            &package.package,
            "model identity adapter hash does not match the declared adapter closure",
        ));
    }
    let graph_and_weights_share_artifact =
        package.artifacts[0].path == package.artifacts[1].path
            && package.artifacts[0].sha256 == package.artifacts[1].sha256;
    if package.artifacts[0].path == package.artifacts[1].path && !graph_and_weights_share_artifact {
        return Err(provenance_error(
            &package.package,
            "graph and weights roles share a path with different hashes",
        ));
    }
    let mut paths = BTreeSet::new();
    for (index, artifact) in package.artifacts.iter().enumerate() {
        let path = Path::new(&artifact.path);
        if artifact.path.is_empty()
            || artifact.path.bytes().any(|byte| byte.is_ascii_control())
            || path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
        {
            return Err(provenance_error(
                &package.package,
                format!("invalid ONNX artifact path `{}`", artifact.path),
            ));
        }
        if index == 1 && graph_and_weights_share_artifact {
            continue;
        }
        if !paths.insert(artifact.path.as_str()) {
            return Err(provenance_error(
                &package.package,
                format!("ONNX artifact path `{}` is declared more than once", artifact.path),
            ));
        }
        if artifact.sha256.len() != 64
            || !artifact
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(provenance_error(
                &package.package,
                format!("model artifact digest `{}` is not lowercase SHA-256", artifact.sha256),
            ));
        }
    }
    Ok(())
}

struct PreparedArtifact {
    bytes: Arc<Vec<u8>>,
}

struct PreparedPackage {
    artifacts: Vec<PreparedArtifact>,
}

impl PreparedPackage {

    fn graph(&self) -> Result<&PreparedArtifact, ModelError> {
        // The graph hash is the first identity artifact by ModelPackage's
        // canonical construction; matching by position avoids a second name
        // registry while still checking the descriptor hash during reading.
        self.artifacts
            .first()
            .ok_or_else(|| provenance_error(RUNTIME_ARTIFACT_PATH, "model package has no graph artifact"))
    }

    fn weights(&self) -> Result<&PreparedArtifact, ModelError> {
        self.artifacts
            .get(1)
            .ok_or_else(|| provenance_error(RUNTIME_ARTIFACT_PATH, "model package has no weights artifact"))
    }

    fn tokenizer(&self) -> Result<&PreparedArtifact, ModelError> {
        self.artifacts
            .get(2)
            .ok_or_else(|| provenance_error(RUNTIME_ARTIFACT_PATH, "model package has no tokenizer artifact"))
    }

}

fn validate_inputs(
    package: &ModelPackage,
    inputs: &[TensorData],
    cancellation: &CancellationToken,
) -> Result<u64, ModelError> {
    if cancellation.is_cancelled() {
        return Err(package.cancelled());
    }
    let specs: Vec<_> = inputs.iter().map(|input| input.spec.clone()).collect();
    package.check_inputs(&specs)?;
    let mut bytes = 0u64;
    for input in inputs {
        input.validate(&package.package)?;
        bytes = bytes.checked_add(input.bytes.len() as u64).ok_or_else(|| ModelError::BufferLimit {
            package: package.package.clone(),
            bytes: u64::MAX,
        })?;
        if let Some(TensorDimension::Static(batch)) = input.spec.shape.dimensions.first() {
            package.check_batch(*batch)?;
        }
        if let Some(TensorDimension::Static(context)) = input.spec.shape.dimensions.get(1) {
            package.check_context(*context)?;
        }
    }
    package.check_buffer(bytes)?;
    Ok(bytes)
}

fn validate_outputs(package: &ModelPackage, outputs: &[TensorData]) -> Result<u64, ModelError> {
    let specs: Vec<_> = outputs.iter().map(|output| output.spec.clone()).collect();
    package.check_outputs(&specs)?;
    let mut bytes = 0u64;
    for output in outputs {
        output.validate(&package.package)?;
        bytes = bytes.checked_add(output.bytes.len() as u64).ok_or_else(|| ModelError::BufferLimit {
            package: package.package.clone(),
            bytes: u64::MAX,
        })?;
    }
    package.check_buffer(bytes)?;
    Ok(bytes)
}

fn workload_digest(inputs: &[TensorData]) -> String {
    let mut canonical = String::new();
    for input in inputs {
        let _ = write!(canonical, "{}:{}:{}:", input.spec.name, input.spec.dtype, input.spec.shape);
        canonical.push_str(&SHA256::sha256_hex(&input.bytes));
        canonical.push(';');
    }
    SHA256::sha256_hex(canonical.as_bytes())
}

fn output_digest_for(outputs: &[TensorData]) -> String {
    let mut canonical = String::new();
    for output in outputs {
        let _ = write!(canonical, "{}:{}:{}:", output.spec.name, output.spec.dtype, output.spec.shape);
        canonical.push_str(&SHA256::sha256_hex(&output.bytes));
        canonical.push(';');
    }
    SHA256::sha256_hex(canonical.as_bytes())
}

#[cfg(not(target_arch = "wasm32"))]
fn clock_start() -> Instant { Instant::now() }
#[cfg(target_arch = "wasm32")]
fn clock_start() -> u64 { browser::now_ns() }

#[cfg(not(target_arch = "wasm32"))]
fn elapsed_ns(started: Instant) -> u64 {
    started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}
#[cfg(target_arch = "wasm32")]
fn elapsed_ns(started: u64) -> u64 { browser::now_ns().saturating_sub(started) }

fn dtype_size(dtype: TensorDType) -> usize {
    match dtype {
        TensorDType::Bool | TensorDType::I8 | TensorDType::U8 => 1,
        TensorDType::I16 | TensorDType::U16 | TensorDType::F16 | TensorDType::BF16 => 2,
        TensorDType::I32 | TensorDType::U32 | TensorDType::F32 => 4,
        TensorDType::I64 | TensorDType::U64 | TensorDType::F64 => 8,
    }
}

fn provenance_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            '=' => "\\=".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

fn declaration_error(package: &str, reason: impl Into<String>) -> ModelError {
    ModelError::Declaration {
        package: package.to_string(),
        reason: reason.into(),
    }
}

fn provenance_error(package: &str, reason: impl Into<String>) -> ModelError {
    ModelError::Provenance {
        package: package.to_string(),
        reason: reason.into(),
    }
}

fn backend_error(package: &str, path: &str, reason: impl Into<String>) -> ModelError {
    ModelError::Artifact {
        package: package.to_string(),
        path: path.to_string(),
        reason: reason.into(),
    }
}

/// Classify an ONNX Runtime failure at the Jet model boundary.  Native C
/// statuses and web-host failures use this same mapping so operator/provider
/// absence, shape mismatch, cancellation, and ordinary runtime failures never
/// acquire a second backend-specific error type.
pub fn classify_backend_failure(package: &str, operation: &str, code: i32, message: &str) -> ModelError {
    let lower = message.to_ascii_lowercase();
    if code == ORT_CODE_MODEL_LOAD_CANCELED || lower.contains("cancel") || lower.contains("terminate") {
        return ModelError::Cancelled { package: package.to_string() };
    }
    if code == ORT_CODE_NOT_IMPLEMENTED
        || lower.contains("no op registered")
        || lower.contains("not implemented")
        || lower.contains("unsupported operator")
        || lower.contains("operator") && lower.contains("not found")
    {
        return backend_error(
            package,
            INFERENCE_ARTIFACT_PATH,
            format!("operator unavailable during {operation}: {message}"),
        );
    }
    if lower.contains("execution provider")
        || lower.contains("provider")
            && (lower.contains("unavailable")
                || lower.contains("not found")
                || lower.contains("missing")
                || lower.contains("absent"))
        || code == ORT_CODE_EP_FAIL && (lower.contains("provider") || lower.contains("execution"))
    {
        return ModelError::UnsupportedProvider {
            package: package.to_string(),
            expected: CPU_EXECUTION_PROVIDER.to_string(),
            actual: message.to_string(),
        };
    }
    if lower.contains("shape") || lower.contains("dimension") {
        return ModelError::SignatureMismatch {
            package: package.to_string(),
            expected: "declared ONNX tensor boundary".to_string(),
            actual: format!("runtime status {code} during {operation}: {message}"),
        };
    }
    backend_error(
        package,
        INFERENCE_ARTIFACT_PATH,
        format!("ONNX Runtime status {code} during {operation}: {message}"),
    )
}

#[cfg(any(unix, windows))]
mod native {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::mem::size_of;
    use std::os::raw::{c_char, c_int, c_void};
    use std::ptr;

    #[repr(C)]
    struct OrtEnv {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtStatus {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtSession {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtSessionOptions {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtValue {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtRunOptions {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtTypeInfo {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtTensorTypeAndShapeInfo {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtMemoryInfo {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct OrtAllocator {
        _private: [u8; 0],
    }

    type StatusPtr = *mut OrtStatus;
    type EnvPtr = *mut OrtEnv;
    type SessionPtr = *mut OrtSession;
    type SessionOptionsPtr = *mut OrtSessionOptions;
    type ValuePtr = *mut OrtValue;
    type RunOptionsPtr = *mut OrtRunOptions;
    type TypeInfoPtr = *mut OrtTypeInfo;
    type TensorInfoPtr = *mut OrtTensorTypeAndShapeInfo;
    type MemoryInfoPtr = *mut OrtMemoryInfo;
    type AllocatorPtr = *mut OrtAllocator;

    // ORT_API_CALL is C on Unix and __stdcall on Windows. `system` selects
    // exactly that platform calling convention in both Rust targets.
    type CreateEnv = unsafe extern "system" fn(c_int, *const c_char, *mut EnvPtr) -> StatusPtr;
    type CreateSessionFromArray = unsafe extern "system" fn(
        *const OrtEnv,
        *const c_void,
        usize,
        *const OrtSessionOptions,
        *mut SessionPtr,
    ) -> StatusPtr;
    type Run = unsafe extern "system" fn(
        *mut OrtSession,
        *const OrtRunOptions,
        *const *const c_char,
        *const *const OrtValue,
        usize,
        *const *const c_char,
        usize,
        *mut ValuePtr,
    ) -> StatusPtr;
    type CreateSessionOptions = unsafe extern "system" fn(*mut SessionOptionsPtr) -> StatusPtr;
    type AppendExecutionProviderCpu = unsafe extern "system" fn(*mut OrtSessionOptions, c_int) -> StatusPtr;

    type SetSessionExecutionMode = unsafe extern "system" fn(*mut OrtSessionOptions, c_int) -> StatusPtr;
    type SetSessionGraphOptimizationLevel = unsafe extern "system" fn(*mut OrtSessionOptions, c_int) -> StatusPtr;
    type SessionGetInputCount = unsafe extern "system" fn(*const OrtSession, *mut usize) -> StatusPtr;
    type SessionGetOutputCount = unsafe extern "system" fn(*const OrtSession, *mut usize) -> StatusPtr;
    type SessionGetInputTypeInfo = unsafe extern "system" fn(*const OrtSession, usize, *mut TypeInfoPtr) -> StatusPtr;
    type SessionGetOutputTypeInfo = unsafe extern "system" fn(*const OrtSession, usize, *mut TypeInfoPtr) -> StatusPtr;
    type SessionGetInputName = unsafe extern "system" fn(*const OrtSession, usize, AllocatorPtr, *mut *mut c_char) -> StatusPtr;
    type SessionGetOutputName = unsafe extern "system" fn(*const OrtSession, usize, AllocatorPtr, *mut *mut c_char) -> StatusPtr;
    type CreateRunOptions = unsafe extern "system" fn(*mut RunOptionsPtr) -> StatusPtr;
    type RunOptionsSetTerminate = unsafe extern "system" fn(*mut OrtRunOptions) -> StatusPtr;
    type CreateTensorWithData = unsafe extern "system" fn(
        *const OrtMemoryInfo,
        *mut c_void,
        usize,
        *const i64,
        usize,
        c_int,
        *mut ValuePtr,
    ) -> StatusPtr;
    type IsTensor = unsafe extern "system" fn(*const OrtValue, *mut c_int) -> StatusPtr;
    type GetTensorMutableData = unsafe extern "system" fn(ValuePtr, *mut *mut c_void) -> StatusPtr;
    type CastTypeInfoToTensorInfo = unsafe extern "system" fn(*const OrtTypeInfo, *mut *const OrtTensorTypeAndShapeInfo) -> StatusPtr;
    type GetOnnxTypeFromTypeInfo = unsafe extern "system" fn(*const OrtTypeInfo, *mut c_int) -> StatusPtr;
    type GetTensorElementType = unsafe extern "system" fn(*const OrtTensorTypeAndShapeInfo, *mut c_int) -> StatusPtr;
    type GetDimensionsCount = unsafe extern "system" fn(*const OrtTensorTypeAndShapeInfo, *mut usize) -> StatusPtr;
    type GetDimensions = unsafe extern "system" fn(*const OrtTensorTypeAndShapeInfo, *mut i64, usize) -> StatusPtr;
    type GetTensorTypeAndShape = unsafe extern "system" fn(*const OrtValue, *mut TensorInfoPtr) -> StatusPtr;
    type CreateCpuMemoryInfo = unsafe extern "system" fn(c_int, c_int, *mut MemoryInfoPtr) -> StatusPtr;
    type GetAllocatorWithDefaultOptions = unsafe extern "system" fn(*mut AllocatorPtr) -> StatusPtr;
    type AllocatorFree = unsafe extern "system" fn(AllocatorPtr, *mut c_void) -> StatusPtr;
    type GetErrorCode = unsafe extern "system" fn(*const OrtStatus) -> c_int;
    type GetErrorMessage = unsafe extern "system" fn(*const OrtStatus) -> *const c_char;
    type ReleaseEnv = unsafe extern "system" fn(EnvPtr);
    type ReleaseStatus = unsafe extern "system" fn(StatusPtr);
    type ReleaseMemoryInfo = unsafe extern "system" fn(MemoryInfoPtr);
    type ReleaseSession = unsafe extern "system" fn(SessionPtr);
    type ReleaseValue = unsafe extern "system" fn(ValuePtr);
    type ReleaseRunOptions = unsafe extern "system" fn(RunOptionsPtr);
    type ReleaseTypeInfo = unsafe extern "system" fn(TypeInfoPtr);
    type ReleaseTensorTypeAndShapeInfo = unsafe extern "system" fn(TensorInfoPtr);
    type ReleaseSessionOptions = unsafe extern "system" fn(SessionOptionsPtr);

    // These are the first 104 entries of OrtApi in v1.29.0.  Every entry is a
    // function pointer, so using pointer-sized slots preserves the official
    // C layout while avoiding a copied or stale wrapper crate.  Required
    // slots are resolved once against ORT_API_VERSION=29 below.
    #[repr(C)]
    struct OrtApiTable {
        slots: [*const c_void; 104],
    }

    #[repr(C)]
    struct OrtApiBase {
        get_api: unsafe extern "system" fn(u32) -> *const OrtApiTable,
        get_version_string: unsafe extern "system" fn() -> *const c_char,
    }

    const ORT_API_VERSION: u32 = 29;
    const ORT_LOGGING_LEVEL_WARNING: c_int = 2;
    const ORT_SEQUENTIAL: c_int = 0;
    const ORT_ENABLE_BASIC: c_int = 1;
    const ORT_ARENA_ALLOCATOR: c_int = 1;
    const ORT_MEM_TYPE_DEFAULT: c_int = 0;
    const ONNX_TYPE_TENSOR: c_int = 1;

    // Direct fields precede ORT_API2_STATUS entries in OrtApi.  The indices
    // below are pinned to the v1.29.0 header and are checked by the API table
    // length.  The C API itself remains the sole ABI authority.
    const CREATE_ENV: usize = 3;
    const CREATE_SESSION_FROM_ARRAY: usize = 8;
    const RUN: usize = 9;
    const CREATE_SESSION_OPTIONS: usize = 10;
    const SET_SESSION_EXECUTION_MODE: usize = 13;
    const SET_SESSION_GRAPH_OPTIMIZATION_LEVEL: usize = 23;
    const SESSION_GET_INPUT_COUNT: usize = 30;
    const SESSION_GET_OUTPUT_COUNT: usize = 31;
    const SESSION_GET_INPUT_TYPE_INFO: usize = 33;
    const SESSION_GET_OUTPUT_TYPE_INFO: usize = 34;
    const SESSION_GET_INPUT_NAME: usize = 36;
    const SESSION_GET_OUTPUT_NAME: usize = 37;
    const CREATE_RUN_OPTIONS: usize = 39;
    const RUN_OPTIONS_SET_TERMINATE: usize = 43;
    const CREATE_TENSOR_WITH_DATA: usize = 49;
    const IS_TENSOR: usize = 50;
    const GET_TENSOR_MUTABLE_DATA: usize = 51;
    const CAST_TYPE_INFO_TO_TENSOR_INFO: usize = 55;
    const GET_ONNX_TYPE_FROM_TYPE_INFO: usize = 56;
    const GET_TENSOR_ELEMENT_TYPE: usize = 60;
    const GET_DIMENSIONS_COUNT: usize = 61;
    const GET_DIMENSIONS: usize = 62;
    const GET_TENSOR_TYPE_AND_SHAPE: usize = 65;
    const CREATE_CPU_MEMORY_INFO: usize = 69;
    const ALLOCATOR_FREE: usize = 76;
    const GET_ALLOCATOR_WITH_DEFAULT_OPTIONS: usize = 78;
    const RELEASE_ENV: usize = 92;
    const RELEASE_STATUS: usize = 93;
    const RELEASE_MEMORY_INFO: usize = 94;
    const RELEASE_SESSION: usize = 95;
    const RELEASE_VALUE: usize = 96;
    const RELEASE_RUN_OPTIONS: usize = 97;
    const RELEASE_TYPE_INFO: usize = 98;
    const RELEASE_TENSOR_INFO: usize = 99;
    const RELEASE_SESSION_OPTIONS: usize = 100;

    struct NativeLibrary {
        handle: *mut c_void,
    }

    impl NativeLibrary {
        fn open(path: &Path) -> Result<Self, ModelError> {
            let path_text = path.to_string_lossy();
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
                    backend_error("<runtime>", RUNTIME_ARTIFACT_PATH, "runtime path contains NUL")
                })?;
                // SAFETY: `c_path` is a stable NUL-terminated path and the
                // runtime bytes were hash-checked before this call.  RTLD_NOW
                // resolves all transitive symbols before any session exists.
                let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
                if handle.is_null() {
                    return Err(backend_error(
                        "<runtime>",
                        RUNTIME_ARTIFACT_PATH,
                        format!("cannot load pinned runtime `{path_text}`"),
                    ));
                }
                Ok(Self { handle })
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
                // SAFETY: `wide` is a stable NUL-terminated UTF-16 path and
                // the runtime bytes were hash-checked before this call.
                let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
                if handle.is_null() {
                    return Err(backend_error(
                        "<runtime>",
                        RUNTIME_ARTIFACT_PATH,
                        format!("cannot load pinned runtime `{path_text}`"),
                    ));
                }
                Ok(Self { handle })
            }
        }

        fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, ModelError> {
            if size_of::<T>() != size_of::<*mut c_void>() {
                return Err(backend_error(
                    "<runtime>",
                    RUNTIME_ARTIFACT_PATH,
                    "runtime function pointer ABI width is unsupported",
                ));
            }
            #[cfg(unix)]
            {
                // SAFETY: `self.handle` came from `dlopen`; `name` is a
                // compile-time NUL-terminated symbol.  The caller selects the
                // exact C function type from the v1.29.0 header.
                let pointer = unsafe { dlsym(self.handle, name.as_ptr().cast()) };
                if pointer.is_null() {
                    return Err(backend_error(
                        "<runtime>",
                        RUNTIME_ARTIFACT_PATH,
                        format!("pinned runtime is missing symbol `{}`", String::from_utf8_lossy(name)),
                    ));
                }
                // SAFETY: the requested type is the ABI declaration for this
                // symbol; function/data pointers have the platform ABI size.
                Ok(unsafe { std::mem::transmute_copy(&pointer) })
            }
            #[cfg(windows)]
            {
                // SAFETY: `self.handle` came from `LoadLibraryW`; `name` is a
                // compile-time NUL-terminated symbol and the requested type
                // is the v1.29.0 C API declaration.
                let pointer = unsafe { GetProcAddress(self.handle, name.as_ptr().cast()) };
                if pointer.is_null() {
                    return Err(backend_error(
                        "<runtime>",
                        RUNTIME_ARTIFACT_PATH,
                        format!("pinned runtime is missing symbol `{}`", String::from_utf8_lossy(name)),
                    ));
                }
                // SAFETY: same ABI/layout argument as the Unix branch.
                Ok(unsafe { std::mem::transmute_copy(&pointer) })
            }
        }
    }

    impl Drop for NativeLibrary {
        fn drop(&mut self) {
            #[cfg(unix)]
            if !self.handle.is_null() {
                // SAFETY: this handle was returned by `dlopen` and all ORT
                // objects are released by Session before the library drops.
                unsafe {
                    let _ = dlclose(self.handle);
                }
            }
            #[cfg(windows)]
            if !self.handle.is_null() {
                // SAFETY: this handle was returned by `LoadLibraryW` and all
                // ORT objects are released by Session before the library drops.
                unsafe {
                    let _ = FreeLibrary(self.handle);
                }
            }
        }
    }

    #[cfg(unix)]
    const RTLD_NOW: c_int = 2;
    #[cfg(unix)]
    const RTLD_LOCAL: c_int = 0;

    #[cfg(unix)]
    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    #[cfg(windows)]
    unsafe extern "system" {
        fn LoadLibraryW(path: *const u16) -> *mut c_void;
        fn GetProcAddress(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(handle: *mut c_void) -> i32;
    }

    struct NativeApi {
        append_execution_provider_cpu: AppendExecutionProviderCpu,

        _library: NativeLibrary,
        get_error_code: GetErrorCode,
        get_error_message: GetErrorMessage,
        release_status: ReleaseStatus,
        create_env: CreateEnv,
        create_session_from_array: CreateSessionFromArray,
        run: Run,
        create_session_options: CreateSessionOptions,
        set_session_execution_mode: SetSessionExecutionMode,
        set_session_graph_optimization_level: SetSessionGraphOptimizationLevel,
        session_get_input_count: SessionGetInputCount,
        session_get_output_count: SessionGetOutputCount,
        session_get_input_type_info: SessionGetInputTypeInfo,
        session_get_output_type_info: SessionGetOutputTypeInfo,
        session_get_input_name: SessionGetInputName,
        session_get_output_name: SessionGetOutputName,
        create_run_options: CreateRunOptions,
        run_options_set_terminate: RunOptionsSetTerminate,
        create_tensor_with_data: CreateTensorWithData,
        is_tensor: IsTensor,
        get_tensor_mutable_data: GetTensorMutableData,
        cast_type_info_to_tensor_info: CastTypeInfoToTensorInfo,
        get_onnx_type_from_type_info: GetOnnxTypeFromTypeInfo,
        get_tensor_element_type: GetTensorElementType,
        get_dimensions_count: GetDimensionsCount,
        get_dimensions: GetDimensions,
        get_tensor_type_and_shape: GetTensorTypeAndShape,
        create_cpu_memory_info: CreateCpuMemoryInfo,
        allocator_free: AllocatorFree,
        get_allocator_with_default_options: GetAllocatorWithDefaultOptions,
        release_env: ReleaseEnv,
        release_memory_info: ReleaseMemoryInfo,
        release_session: ReleaseSession,
        release_value: ReleaseValue,
        release_run_options: ReleaseRunOptions,
        release_type_info: ReleaseTypeInfo,
        release_tensor_info: ReleaseTensorTypeAndShapeInfo,
        release_session_options: ReleaseSessionOptions,
    }

    impl NativeApi {
        fn load(pin: &RuntimePin) -> Result<Self, ModelError> {
            pin.verify_native_file()?;
            #[cfg(unix)]
            pin.verify_native_provider_file()?;
            let library = NativeLibrary::open(&pin.path)?;
            let append_execution_provider_cpu: AppendExecutionProviderCpu =
                library.symbol(b"OrtSessionOptionsAppendExecutionProvider_CPU\0")?;

            let get_api_base: unsafe extern "system" fn() -> *const OrtApiBase =
                library.symbol(b"OrtGetApiBase\0")?;
            // SAFETY: the symbol is the official ONNX Runtime entry point and
            // returns a process-live API base for the loaded library.
            let base = unsafe { get_api_base() };
            if base.is_null() {
                return Err(backend_error("<runtime>", RUNTIME_ARTIFACT_PATH, "OrtGetApiBase returned null"));
            }
            // SAFETY: `base` is owned by the loaded runtime and its first two
            // fields are the v1.29.0 `OrtApiBase` function pointers.
            let version_pointer = unsafe { ((*base).get_version_string)() };
            if version_pointer.is_null() {
                return Err(backend_error("<runtime>", RUNTIME_ARTIFACT_PATH, "runtime returned no version string"));
            }
            // SAFETY: ONNX Runtime documents this pointer as a NUL-terminated
            // UTF-8 version string valid until the library unloads.
            let version = unsafe { CStr::from_ptr(version_pointer) }.to_string_lossy();
            if version != pin.version {
                return Err(provenance_error(
                    RUNTIME_ARTIFACT_PATH,
                    format!("runtime reports version `{version}`, expected `{}`", pin.version),
                ));
            }
            // SAFETY: `get_api` is the first field of the loaded API base and
            // the requested version is the exact header version used below.
            let table = unsafe { ((*base).get_api)(ORT_API_VERSION) };
            if table.is_null() {
                return Err(backend_error(
                    "<runtime>",
                    RUNTIME_ARTIFACT_PATH,
                    format!("runtime does not expose API version {ORT_API_VERSION}"),
                ));
            }
            // SAFETY: all slots are official function pointers in OrtApi.  A
            // missing slot is reported before it can be called.
            let slot = |index: usize, name: &'static str| -> Result<*const c_void, ModelError> {
                if index >= 104 {
                    return Err(backend_error("<runtime>", RUNTIME_ARTIFACT_PATH, format!("invalid API slot {index} for {name}")));
                }
                // SAFETY: `table` points to the loaded OrtApi prefix; each
                // slot is pointer-aligned and within the pinned table prefix.
                let pointer = unsafe { (*table).slots[index] };
                if pointer.is_null() {
                    Err(backend_error("<runtime>", RUNTIME_ARTIFACT_PATH, format!("API slot `{name}` is unavailable")))
                } else {
                    Ok(pointer)
                }
            };
            // SAFETY: `pointer` is the matching C function pointer slot and T
            // is the corresponding v1.29.0 declaration.
            let cast = |index: usize, name: &'static str| -> Result<*const c_void, ModelError> { slot(index, name) };
            macro_rules! fn_slot {
                ($index:expr, $name:literal, $ty:ty) => {{
                    let pointer = cast($index, $name)?;
                    if size_of::<$ty>() != size_of::<*const c_void>() {
                        return Err(backend_error(
                            "<runtime>",
                            RUNTIME_ARTIFACT_PATH,
                            format!("runtime API function `{}` has an unsupported pointer width", $name),
                        ));
                    }
                    // SAFETY: `pointer` is the non-null slot for the exact C
                    // function declaration named by this macro invocation.
                    unsafe { std::mem::transmute_copy::<*const c_void, $ty>(&pointer) }
                }};
            }
            let get_error_code = fn_slot!(1, "GetErrorCode", GetErrorCode);
            let get_error_message = fn_slot!(2, "GetErrorMessage", GetErrorMessage);
            let release_status = fn_slot!(RELEASE_STATUS, "ReleaseStatus", ReleaseStatus);
            Ok(Self {
                _library: library,
                get_error_code,
                get_error_message,
                release_status,
                append_execution_provider_cpu,
                create_env: fn_slot!(CREATE_ENV, "CreateEnv", CreateEnv),
                create_session_from_array: fn_slot!(CREATE_SESSION_FROM_ARRAY, "CreateSessionFromArray", CreateSessionFromArray),
                run: fn_slot!(RUN, "Run", Run),
                create_session_options: fn_slot!(CREATE_SESSION_OPTIONS, "CreateSessionOptions", CreateSessionOptions),
                set_session_execution_mode: fn_slot!(SET_SESSION_EXECUTION_MODE, "SetSessionExecutionMode", SetSessionExecutionMode),
                set_session_graph_optimization_level: fn_slot!(SET_SESSION_GRAPH_OPTIMIZATION_LEVEL, "SetSessionGraphOptimizationLevel", SetSessionGraphOptimizationLevel),
                session_get_input_count: fn_slot!(SESSION_GET_INPUT_COUNT, "SessionGetInputCount", SessionGetInputCount),
                session_get_output_count: fn_slot!(SESSION_GET_OUTPUT_COUNT, "SessionGetOutputCount", SessionGetOutputCount),
                session_get_input_type_info: fn_slot!(SESSION_GET_INPUT_TYPE_INFO, "SessionGetInputTypeInfo", SessionGetInputTypeInfo),
                session_get_output_type_info: fn_slot!(SESSION_GET_OUTPUT_TYPE_INFO, "SessionGetOutputTypeInfo", SessionGetOutputTypeInfo),
                session_get_input_name: fn_slot!(SESSION_GET_INPUT_NAME, "SessionGetInputName", SessionGetInputName),
                session_get_output_name: fn_slot!(SESSION_GET_OUTPUT_NAME, "SessionGetOutputName", SessionGetOutputName),
                create_run_options: fn_slot!(CREATE_RUN_OPTIONS, "CreateRunOptions", CreateRunOptions),
                run_options_set_terminate: fn_slot!(RUN_OPTIONS_SET_TERMINATE, "RunOptionsSetTerminate", RunOptionsSetTerminate),
                create_tensor_with_data: fn_slot!(CREATE_TENSOR_WITH_DATA, "CreateTensorWithDataAsOrtValue", CreateTensorWithData),
                is_tensor: fn_slot!(IS_TENSOR, "IsTensor", IsTensor),
                get_tensor_mutable_data: fn_slot!(GET_TENSOR_MUTABLE_DATA, "GetTensorMutableData", GetTensorMutableData),
                cast_type_info_to_tensor_info: fn_slot!(CAST_TYPE_INFO_TO_TENSOR_INFO, "CastTypeInfoToTensorInfo", CastTypeInfoToTensorInfo),
                get_onnx_type_from_type_info: fn_slot!(GET_ONNX_TYPE_FROM_TYPE_INFO, "GetOnnxTypeFromTypeInfo", GetOnnxTypeFromTypeInfo),
                get_tensor_element_type: fn_slot!(GET_TENSOR_ELEMENT_TYPE, "GetTensorElementType", GetTensorElementType),
                get_dimensions_count: fn_slot!(GET_DIMENSIONS_COUNT, "GetDimensionsCount", GetDimensionsCount),
                get_dimensions: fn_slot!(GET_DIMENSIONS, "GetDimensions", GetDimensions),
                get_tensor_type_and_shape: fn_slot!(GET_TENSOR_TYPE_AND_SHAPE, "GetTensorTypeAndShape", GetTensorTypeAndShape),
                create_cpu_memory_info: fn_slot!(CREATE_CPU_MEMORY_INFO, "CreateCpuMemoryInfo", CreateCpuMemoryInfo),
                allocator_free: fn_slot!(ALLOCATOR_FREE, "AllocatorFree", AllocatorFree),
                get_allocator_with_default_options: fn_slot!(GET_ALLOCATOR_WITH_DEFAULT_OPTIONS, "GetAllocatorWithDefaultOptions", GetAllocatorWithDefaultOptions),
                release_env: fn_slot!(RELEASE_ENV, "ReleaseEnv", ReleaseEnv),
                release_memory_info: fn_slot!(RELEASE_MEMORY_INFO, "ReleaseMemoryInfo", ReleaseMemoryInfo),
                release_session: fn_slot!(RELEASE_SESSION, "ReleaseSession", ReleaseSession),
                release_value: fn_slot!(RELEASE_VALUE, "ReleaseValue", ReleaseValue),
                release_run_options: fn_slot!(RELEASE_RUN_OPTIONS, "ReleaseRunOptions", ReleaseRunOptions),
                release_type_info: fn_slot!(RELEASE_TYPE_INFO, "ReleaseTypeInfo", ReleaseTypeInfo),
                release_tensor_info: fn_slot!(RELEASE_TENSOR_INFO, "ReleaseTensorTypeAndShapeInfo", ReleaseTensorTypeAndShapeInfo),
                release_session_options: fn_slot!(RELEASE_SESSION_OPTIONS, "ReleaseSessionOptions", ReleaseSessionOptions),
            })
        }

        fn status(&self, status: StatusPtr, package: &str, operation: &str) -> Result<(), ModelError> {
            if status.is_null() {
                return Ok(());
            }
            // SAFETY: `status` is a live status returned by the same loaded
            // API table.  GetErrorMessage is valid until ReleaseStatus below.
            let code = unsafe { (self.get_error_code)(status) };
            let message = unsafe {
                let pointer = (self.get_error_message)(status);
                if pointer.is_null() {
                    format!("ONNX Runtime status {code}")
                } else {
                    CStr::from_ptr(pointer).to_string_lossy().into_owned()
                }
            };
            // SAFETY: every status returned by this API must be released once,
            // including a failed load, operator absence, or cancelled run.
            unsafe { (self.release_status)(status) };
            Err(classify_backend_failure(package, operation, code, &message))
        }
    }

    struct ValueGuard<'a> {
        api: &'a NativeApi,
        pointer: ValuePtr,
    }

    impl<'a> ValueGuard<'a> {
        fn new(api: &'a NativeApi, pointer: ValuePtr) -> Self {
            Self { api, pointer }
        }
    }

    impl Drop for ValueGuard<'_> {
        fn drop(&mut self) {
            if !self.pointer.is_null() {
                // SAFETY: pointer came from CreateTensor/Run of this API and
                // is released exactly once after all borrowed buffers finish.
                unsafe { (self.api.release_value)(self.pointer) };
            }
        }
    }

    struct RunOptionsGuard<'a> {
        api: &'a NativeApi,
        pointer: RunOptionsPtr,
    }

    /// Watches the scheduler-backed token while ORT is inside a foreign call.
    /// ORT's terminate bit is the only safe way to interrupt that call; the
    /// guard joins before either the run options or output values are touched.
    struct RunCancellation {
        done: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl RunCancellation {
        fn new(api: &NativeApi, options: RunOptionsPtr, cancellation: &CancellationToken) -> Self {
            let done = Arc::new(AtomicBool::new(false));
            let finished = done.clone();
            let token = cancellation.clone();
            let set_terminate = api.run_options_set_terminate;
            let release_status = api.release_status;
            let options = options as usize;
            let join = thread::spawn(move || {
                while !finished.load(Ordering::Acquire) && !token.is_cancelled() {
                    thread::park_timeout(Duration::from_millis(1));
                }
                if token.is_cancelled() && !finished.load(Ordering::Acquire) {
                    // SAFETY: the run owns this live options object until the
                    // guard joins; the function pointer came from its pinned
                    // API table.
                    let status = unsafe { set_terminate(options as RunOptionsPtr) };
                    if !status.is_null() {
                        // SAFETY: every status returned by the pinned API must
                        // be released, including a termination request.
                        unsafe { release_status(status) };
                    }
                }
            });
            Self {
                done,
                join: Some(join),
            }
        }
    }

    impl Drop for RunCancellation {
        fn drop(&mut self) {
            self.done.store(true, Ordering::Release);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    impl Drop for RunOptionsGuard<'_> {
        fn drop(&mut self) {
            if !self.pointer.is_null() {
                // SAFETY: pointer came from CreateRunOptions and is released
                // exactly once after the run returns.
                unsafe { (self.api.release_run_options)(self.pointer) };
            }
        }
    }

    struct TypeInfoGuard<'a> {
        api: &'a NativeApi,
        pointer: TypeInfoPtr,
    }

    impl Drop for TypeInfoGuard<'_> {
        fn drop(&mut self) {
            if !self.pointer.is_null() {
                // SAFETY: pointer came from SessionGet*TypeInfo and remains
                // live until this guard drops.
                unsafe { (self.api.release_type_info)(self.pointer) };
            }
        }
    }

    struct TensorInfoGuard<'a> {
        api: &'a NativeApi,
        pointer: TensorInfoPtr,
    }

    impl Drop for TensorInfoGuard<'_> {
        fn drop(&mut self) {
            if !self.pointer.is_null() {
                // SAFETY: pointer came from GetTensorTypeAndShape and is
                // released exactly once after dimensions/data are copied.
                unsafe { (self.api.release_tensor_info)(self.pointer) };
            }
        }
    }


    struct SessionOptionsGuard<'a> {
        api: &'a NativeApi,
        pointer: SessionOptionsPtr,
    }

    impl Drop for SessionOptionsGuard<'_> {
        fn drop(&mut self) {
            if !self.pointer.is_null() {
                // SAFETY: pointer came from CreateSessionOptions and is
                // released once after CreateSessionFromArray returns.
                unsafe { (self.api.release_session_options)(self.pointer) };
            }
        }
    }

    pub struct Session {
        api: NativeApi,
        env: EnvPtr,
        session: SessionPtr,
        memory_info: MemoryInfoPtr,
        input_names: Vec<CString>,
        output_names: Vec<CString>,
        _model_bytes: Vec<Arc<Vec<u8>>>,
    }

    impl Session {
        pub fn open(
            package: &ModelPackage,
            prepared: PreparedPackage,
            pin: &RuntimePin,
            policy: &OnnxRuntimePolicy,
        ) -> Result<Self, ModelError> {
            let api = NativeApi::load(pin)?;
            let log_id = CString::new(package.package.as_str()).map_err(|_| {
                declaration_error(&package.package, "package name contains NUL and cannot identify an ORT session")
            })?;
            let mut env = ptr::null_mut();
            // SAFETY: output pointer is valid, log_id is NUL-terminated, and
            // the call uses the v1.29.0 API function resolved above.
            let status = unsafe { (api.create_env)(ORT_LOGGING_LEVEL_WARNING, log_id.as_ptr(), &mut env) };
            if let Err(error) = api.status(status, &package.package, "CreateEnv") {
                return Err(error);
            }
            if env.is_null() {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "CreateEnv returned null"));
            }
            let mut options = ptr::null_mut();
            // SAFETY: output pointer is valid and function comes from the
            // same API table as the environment.
            let status = unsafe { (api.create_session_options)(&mut options) };
            if let Err(error) = api.status(status, &package.package, "CreateSessionOptions") {
                // SAFETY: env is live and owned by this path after CreateEnv.
                unsafe { (api.release_env)(env) };
                return Err(error);
            }
            if options.is_null() {
                unsafe { (api.release_env)(env) };
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "CreateSessionOptions returned null"));
            }
            // The pinned policy accepts only CPUExecutionProvider.  Append
            // that provider explicitly instead of relying on ORT's implicit
            // default, which could change with a future runtime build.
            let options_guard = SessionOptionsGuard { api: &api, pointer: options };
            // SAFETY: options is live and the symbol is the v1.29.0 CPU
            // provider entry point from the same loaded runtime.
            let status = unsafe { (api.append_execution_provider_cpu)(options, 1) };
            if let Err(error) = api.status(status, &package.package, "OrtSessionOptionsAppendExecutionProvider_CPU") {
                drop(options_guard);
                unsafe { (api.release_env)(env) };
                return Err(error);
            }
            // SAFETY: options is live and enum values match the pinned C API.
            let status = unsafe { (api.set_session_execution_mode)(options, ORT_SEQUENTIAL) };
            if let Err(error) = api.status(status, &package.package, "SetSessionExecutionMode") {
                drop(options_guard);
                unsafe { (api.release_env)(env) };
                return Err(error);
            }
            let status = unsafe { (api.set_session_graph_optimization_level)(options, ORT_ENABLE_BASIC) };
            if let Err(error) = api.status(status, &package.package, "SetSessionGraphOptimizationLevel") {
                drop(options_guard);
                unsafe { (api.release_env)(env) };
                return Err(error);
            }
            let graph = prepared.graph()?.bytes.as_slice();
            let mut session = ptr::null_mut();
            // SAFETY: graph remains owned by `model_bytes` below for the whole
            // session lifetime; ORT copies/validates the model during creation.
            let status = unsafe {
                (api.create_session_from_array)(
                    env,
                    graph.as_ptr().cast(),
                    graph.len(),
                    options,
                    &mut session,
                )
            };
            if let Err(error) = api.status(status, &package.package, "CreateSessionFromArray") {
                drop(options_guard);
                unsafe { (api.release_env)(env) };
                return Err(error);
            }
            drop(options_guard);
            if session.is_null() {
                unsafe { (api.release_env)(env) };
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "CreateSessionFromArray returned null"));
            }
            let mut memory_info = ptr::null_mut();
            // SAFETY: output pointer is valid; allocator/memory enum values are
            // the documented CPU input memory configuration.
            let status = unsafe {
                (api.create_cpu_memory_info)(ORT_ARENA_ALLOCATOR, ORT_MEM_TYPE_DEFAULT, &mut memory_info)
            };
            if let Err(error) = api.status(status, &package.package, "CreateCpuMemoryInfo") {
                unsafe {
                    (api.release_session)(session);
                    (api.release_env)(env);
                }
                return Err(error);
            }
            if memory_info.is_null() {
                unsafe {
                    (api.release_session)(session);
                    (api.release_env)(env);
                }
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "CreateCpuMemoryInfo returned null"));
            }
            let mut result = Self {
                api,
                env,
                session,
                memory_info,
                input_names: Vec::new(),
                output_names: Vec::new(),
                _model_bytes: prepared.artifacts.into_iter().map(|artifact| artifact.bytes).collect(),
            };
            if let Err(error) = result.validate_session(package, policy) {
                return Err(error);
            }
            Ok(result)
        }

        fn validate_session(&mut self, package: &ModelPackage, _policy: &OnnxRuntimePolicy) -> Result<(), ModelError> {
            let mut input_count = 0usize;
            // SAFETY: count output pointer and session pointer are valid for a
            // live ORT session.
            let status = unsafe { (self.api.session_get_input_count)(self.session, &mut input_count) };
            self.api.status(status, &package.package, "SessionGetInputCount")?;
            if input_count != package.contract.inputs.len() {
                return Err(ModelError::SignatureMismatch {
                    package: package.package.clone(),
                    expected: format!("{} input(s)", package.contract.inputs.len()),
                    actual: format!("{input_count} input(s)"),
                });
            }
            let mut output_count = 0usize;
            let status = unsafe { (self.api.session_get_output_count)(self.session, &mut output_count) };
            self.api.status(status, &package.package, "SessionGetOutputCount")?;
            if output_count != package.contract.outputs.len() {
                return Err(ModelError::SignatureMismatch {
                    package: package.package.clone(),
                    expected: format!("{} output(s)", package.contract.outputs.len()),
                    actual: format!("{output_count} output(s)"),
                });
            }
            let mut allocator = ptr::null_mut();
            let status = unsafe { (self.api.get_allocator_with_default_options)(&mut allocator) };
            self.api.status(status, &package.package, "GetAllocatorWithDefaultOptions")?;
            if allocator.is_null() {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "GetAllocatorWithDefaultOptions returned null"));
            }
            for (index, expected) in package.contract.inputs.iter().enumerate() {
                let name = self.query_name(package, allocator, index, true)?;
                let actual = self.query_signature(package, index, true)?;
                validate_declared_signature(package, expected, &name, actual)?;
                self.input_names.push(CString::new(name).map_err(|_| declaration_error(&package.package, "runtime input name contains NUL"))?);
            }
            for (index, expected) in package.contract.outputs.iter().enumerate() {
                let name = self.query_name(package, allocator, index, false)?;
                let actual = self.query_signature(package, index, false)?;
                validate_declared_signature(package, expected, &name, actual)?;
                self.output_names.push(CString::new(name).map_err(|_| declaration_error(&package.package, "runtime output name contains NUL"))?);
            }
            Ok(())
        }

        fn query_name(&self, package: &ModelPackage, allocator: AllocatorPtr, index: usize, input: bool) -> Result<String, ModelError> {
            let mut pointer = ptr::null_mut();
            // SAFETY: allocator/session are live; ORT writes one allocated,
            // NUL-terminated UTF-8 name to the output pointer.
            let status = if input {
                unsafe { (self.api.session_get_input_name)(self.session, index, allocator, &mut pointer) }
            } else {
                unsafe { (self.api.session_get_output_name)(self.session, index, allocator, &mut pointer) }
            };
            self.api.status(status, &package.package, if input { "SessionGetInputName" } else { "SessionGetOutputName" })?;
            if pointer.is_null() {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "runtime returned a null tensor name"));
            }
            // SAFETY: pointer is the NUL-terminated allocation returned by
            // SessionGet*Name.  We copy before returning it to ORT's allocator.
            let name = unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned();
            // SAFETY: pointer belongs to the default allocator returned above.
            let status = unsafe { (self.api.allocator_free)(allocator, pointer.cast()) };
            self.api.status(status, &package.package, "AllocatorFree")?;
            Ok(name)
        }

        fn query_signature(&self, package: &ModelPackage, index: usize, input: bool) -> Result<RuntimeSignature, ModelError> {
            let mut type_info = ptr::null_mut();
            let status = if input {
                unsafe { (self.api.session_get_input_type_info)(self.session, index, &mut type_info) }
            } else {
                unsafe { (self.api.session_get_output_type_info)(self.session, index, &mut type_info) }
            };
            self.api.status(status, &package.package, if input { "SessionGetInputTypeInfo" } else { "SessionGetOutputTypeInfo" })?;
            if type_info.is_null() {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "runtime returned null tensor type information"));
            }
            let type_guard = TypeInfoGuard { api: &self.api, pointer: type_info };
            let mut onnx_type = 0;
            let status = unsafe { (self.api.get_onnx_type_from_type_info)(type_info, &mut onnx_type) };
            self.api.status(status, &package.package, "GetOnnxTypeFromTypeInfo")?;
            if onnx_type != ONNX_TYPE_TENSOR {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "model endpoint is not a tensor"));
            }
            let mut tensor_info = ptr::null();
            let status = unsafe { (self.api.cast_type_info_to_tensor_info)(type_info, &mut tensor_info) };
            self.api.status(status, &package.package, "CastTypeInfoToTensorInfo")?;
            if tensor_info.is_null() {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "runtime returned null tensor shape information"));
            }
            let mut dtype = 0;
            let status = unsafe { (self.api.get_tensor_element_type)(tensor_info, &mut dtype) };
            self.api.status(status, &package.package, "GetTensorElementType")?;
            let dtype = map_onnx_dtype(dtype).ok_or_else(|| backend_error(&package.package, RUNTIME_ARTIFACT_PATH, format!("unsupported ONNX tensor dtype {dtype}")))?;
            let mut rank = 0usize;
            let status = unsafe { (self.api.get_dimensions_count)(tensor_info, &mut rank) };
            self.api.status(status, &package.package, "GetDimensionsCount")?;
            if rank > 64 {
                return Err(backend_error(&package.package, RUNTIME_ARTIFACT_PATH, "runtime tensor rank exceeds the checked boundary"));
            }
            let mut dimensions = vec![0i64; rank];
            let status = unsafe { (self.api.get_dimensions)(tensor_info, dimensions.as_mut_ptr(), rank) };
            self.api.status(status, &package.package, "GetDimensions")?;
            drop(type_guard);
            Ok(RuntimeSignature { dtype, dimensions })
        }

        pub fn run(
            &mut self,
            package: &ModelPackage,
            inputs: &[TensorData],
            cancellation: &CancellationToken,
        ) -> Result<Vec<TensorData>, ModelError> {
            let input_bytes = validate_inputs(package, inputs, cancellation)?;
            let mut shapes = Vec::with_capacity(inputs.len());
            let mut names = Vec::with_capacity(inputs.len());
            let mut values = Vec::with_capacity(inputs.len());
            for input in inputs {
                shapes.push(concrete_shape(package, &input.spec)?);
                names.push(CString::new(input.spec.name.as_str()).map_err(|_| declaration_error(&package.package, "input tensor name contains NUL"))?);
                let mut value = ptr::null_mut();
                // SAFETY: memory_info is live, input bytes/shapes remain
                let shape = shapes.last().ok_or_else(|| {
                    declaration_error(&package.package, "internal shape marshalling lost its input")
                })?;
                let shape_len = shape.len();
                let status = unsafe {
                    (self.api.create_tensor_with_data)(
                        self.memory_info,
                        input.bytes.as_ptr().cast_mut().cast(),
                        input.bytes.len(),
                        shape.as_ptr(),
                        shape_len,
                        map_tensor_dtype(input.spec.dtype),
                        &mut value,
                    )
                };
                self.api.status(status, &package.package, "CreateTensorWithDataAsOrtValue")?;
                if value.is_null() {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "runtime returned null input tensor"));
                }
                values.push(ValueGuard::new(&self.api, value));
            }
            let input_name_pointers: Vec<_> = names.iter().map(|name| name.as_ptr()).collect();
            let output_name_pointers: Vec<_> = self.output_names.iter().map(|name| name.as_ptr()).collect();
            let mut run_options = ptr::null_mut();
            let status = unsafe { (self.api.create_run_options)(&mut run_options) };
            self.api.status(status, &package.package, "CreateRunOptions")?;
            if run_options.is_null() {
                return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "CreateRunOptions returned null"));
            }
            let _run_options_guard = RunOptionsGuard { api: &self.api, pointer: run_options };
            if cancellation.is_cancelled() {
                return Err(package.cancelled());
            }
            let _run_cancellation = RunCancellation::new(&self.api, run_options, cancellation);
            let mut outputs = vec![ptr::null_mut(); self.output_names.len()];
            let input_values: Vec<_> = values.iter().map(|value| value.pointer as *const OrtValue).collect();
            // SAFETY: all names/values/output slots are valid for this call;
            // the run option is owned by the guard.  ONNX Runtime may run
            // worker threads internally but does not retain these pointers.
            let status = unsafe {
                (self.api.run)(
                    self.session,
                    run_options,
                    input_name_pointers.as_ptr(),
                    input_values.as_ptr(),
                    input_values.len(),
                    output_name_pointers.as_ptr(),
                    output_name_pointers.len(),
                    outputs.as_mut_ptr(),
                )
            };
            if !status.is_null() {
                if cancellation.is_cancelled() {
                    // The runtime may have completed foreign work before the
                    // caller cancelled.  Discard every returned value and
                    // expose cancellation rather than partial/fabricated data.
                    let _ = self.api.status(status, &package.package, "Run");
                    return Err(package.cancelled());
                }
                return match self.api.status(status, &package.package, "Run") {
                    Ok(()) => Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "Run returned an unclassified failure")),
                    Err(error) => Err(error),
                };
            }
            if cancellation.is_cancelled() {
                return Err(package.cancelled());
            }
            drop(_run_cancellation);
            let mut output_guards: Vec<_> = outputs
                .into_iter()
                .map(|pointer| ValueGuard::new(&self.api, pointer))
                .collect();
            let mut decoded = Vec::with_capacity(output_guards.len());
            for (index, guard) in output_guards.iter_mut().enumerate() {
                if guard.pointer.is_null() {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, format!("runtime returned null output tensor {index}")));
                }
                let mut is_tensor = 0;
                let status = unsafe { (self.api.is_tensor)(guard.pointer, &mut is_tensor) };
                self.api.status(status, &package.package, "IsTensor")?;
                if is_tensor == 0 {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, format!("output {index} is not a tensor")));
                }
                let mut tensor_info = ptr::null_mut();
                let status = unsafe { (self.api.get_tensor_type_and_shape)(guard.pointer, &mut tensor_info) };
                self.api.status(status, &package.package, "GetTensorTypeAndShape")?;
                if tensor_info.is_null() {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "runtime returned null output shape"));
                }
                let tensor_info_guard = TensorInfoGuard { api: &self.api, pointer: tensor_info };
                let mut dtype_code = 0;
                let status = unsafe { (self.api.get_tensor_element_type)(tensor_info, &mut dtype_code) };
                self.api.status(status, &package.package, "GetTensorElementType")?;
                let dtype = map_onnx_dtype(dtype_code).ok_or_else(|| backend_error(&package.package, INFERENCE_ARTIFACT_PATH, format!("unsupported output dtype {dtype_code}")))?;
                let mut rank = 0usize;
                let status = unsafe { (self.api.get_dimensions_count)(tensor_info, &mut rank) };
                self.api.status(status, &package.package, "GetDimensionsCount")?;
                if rank == 0 || rank > 64 {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "runtime output rank is outside the checked boundary"));
                }
                let mut dimensions = vec![0i64; rank];
                let status = unsafe { (self.api.get_dimensions)(tensor_info, dimensions.as_mut_ptr(), rank) };
                self.api.status(status, &package.package, "GetDimensions")?;
                if dimensions.iter().any(|dimension| *dimension < 0) {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "runtime returned a symbolic output shape after inference"));
                }
                let dimensions_u64: Vec<u64> = dimensions
                    .iter()
                    .map(|dimension| u64::try_from(*dimension))
                    .collect::<Result<_, _>>()
                    .map_err(|_| declaration_error(&package.package, "output dimension exceeds u64 shape range"))?;
                let dimensions_usize: Vec<usize> = dimensions
                    .iter()
                    .map(|dimension| usize::try_from(*dimension))
                    .collect::<Result<_, _>>()
                    .map_err(|_| declaration_error(&package.package, "output dimension exceeds host size"))?;
                let spec = TensorSpec::runtime(self.output_names[index].to_string_lossy().into_owned(), dtype, dimensions_u64);
                let mut data = ptr::null_mut();
                let status = unsafe { (self.api.get_tensor_mutable_data)(guard.pointer, &mut data) };
                self.api.status(status, &package.package, "GetTensorMutableData")?;
                let element_count = dimensions_usize.iter().try_fold(1usize, |total, dimension| {
                    total.checked_mul(*dimension).ok_or_else(|| declaration_error(&package.package, "output element count overflows host size"))
                })?;
                let byte_count = element_count.checked_mul(dtype_size(dtype)).ok_or_else(|| declaration_error(&package.package, "output byte count overflows host size"))?;
                if byte_count > 0 && data.is_null() {
                    return Err(backend_error(&package.package, INFERENCE_ARTIFACT_PATH, "runtime returned null output data"));
                }
                // SAFETY: ORT guarantees tensor data remains valid until the
                // guarded OrtValue is released; byte_count comes from the
                // checked runtime shape and dtype.
                let bytes = if byte_count == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(data.cast::<u8>(), byte_count).to_vec() }
                };
                decoded.push(TensorData { spec, bytes });
                drop(tensor_info_guard);
            }
            let output_bytes = validate_outputs(package, &decoded)?;
            let total_bytes = input_bytes.checked_add(output_bytes).ok_or_else(|| ModelError::BufferLimit {
                package: package.package.clone(),
                bytes: u64::MAX,
            })?;
            package.check_buffer(total_bytes)?;
            Ok(decoded)
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // SAFETY: release order is session → memory info → environment,
            // matching ONNX Runtime ownership.  No status-returning function
            // is called from Drop, so a runtime teardown error cannot panic.
            unsafe {
                if !self.session.is_null() {
                    (self.api.release_session)(self.session);
                }
                if !self.memory_info.is_null() {
                    (self.api.release_memory_info)(self.memory_info);
                }
                if !self.env.is_null() {
                    (self.api.release_env)(self.env);
                }
            }
        }
    }


    fn concrete_shape(package: &ModelPackage, spec: &TensorSpec) -> Result<Vec<i64>, ModelError> {
        spec.shape
            .dimensions
            .iter()
            .map(|dimension| {
                let TensorDimension::Static(value) = dimension else {
                    return Err(ModelError::SignatureMismatch {
                        package: package.package.clone(),
                        expected: spec.to_string(),
                        actual: "dynamic runtime input shape was not concretized".to_string(),
                    });
                };
                i64::try_from(*value).map_err(|_| declaration_error(&package.package, "tensor dimension exceeds ONNX i64 shape range"))
            })
            .collect()
    }


}

#[cfg(not(any(unix, windows)))]
mod native {
    use super::*;

    /// Native execution is unavailable on targets without a supported dynamic
    /// loader.  The web provider remains available through WebRuntime.
    pub struct Session;

    impl Session {
        pub fn open(
            package: &ModelPackage,
            _prepared: PreparedPackage,
            _pin: &RuntimePin,
            _policy: &OnnxRuntimePolicy,
        ) -> Result<Self, ModelError> {
            Err(backend_error(
                &package.package,
                RUNTIME_ARTIFACT_PATH,
                "native ONNX Runtime C bridge is unavailable on this target",
            ))
        }

        pub fn run(
            &mut self,
            package: &ModelPackage,
            _inputs: &[TensorData],
            _cancellation: &CancellationToken,
        ) -> Result<Vec<TensorData>, ModelError> {
            Err(backend_error(
                &package.package,
                INFERENCE_ARTIFACT_PATH,
                "native ONNX Runtime C bridge is unavailable on this target",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Native provider operations complete on the first poll. This helper must
    // never be used for browser futures or as an event-loop blocking executor.
    fn ready<T>(future: impl Future<Output = T>) -> T {
        struct Wake;
        impl std::task::Wake for Wake {
            fn wake(self: Arc<Self>) {}
        }
        let waker = std::task::Waker::from(Arc::new(Wake));
        let mut context = std::task::Context::from_waker(&waker);
        match std::pin::pin!(future).as_mut().poll(&mut context) {
            std::task::Poll::Ready(result) => result,
            std::task::Poll::Pending => panic!("native operation unexpectedly suspended"),
        }
    }


    #[test]
    fn policy_rejects_unqualified_provider_and_online_fallback() {
        let mut policy = OnnxRuntimePolicy::cpu(["Add"]).unwrap();
        policy.execution_providers = vec!["CUDAExecutionProvider".into()];
        assert!(matches!(
            policy.validate("demo"),
            Err(ModelError::UnsupportedProvider { .. })
        ));
        let mut policy = OnnxRuntimePolicy::cpu(["Add"]).unwrap();
        policy.offline = false;
        assert!(matches!(policy.validate("demo"), Err(ModelError::Provenance { .. })));
    }

    #[test]
    fn cancellation_is_shared_and_backend_failures_keep_typed_meaning() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        let clone = token.clone();
        clone.cancel();
        assert!(token.is_cancelled());
        assert!(matches!(
            classify_backend_failure("demo", "Run", 1, "No op registered for MatMul"),
            ModelError::Artifact { .. }
        ));
        assert!(matches!(
            classify_backend_failure("demo", "Run", 2, "execution provider unavailable"),
            ModelError::UnsupportedProvider { .. }
        ));
        assert!(matches!(
            classify_backend_failure("demo", "Run", 3, "invalid shape"),
            ModelError::SignatureMismatch { .. }
        ));
    }
    #[test]
    fn backend_failures_are_declared_results_with_registered_diagnostics() {
        let require_load = |error: ModelError| {
            assert_eq!(error.code(), "E-MODEL-LOAD");
            assert_eq!(error.diagnostic().code, error.code());
        };
        require_load(classify_backend_failure(
            "demo",
            "CreateSession",
            3,
            "failed to load graph",
        ));
        require_load(classify_backend_failure(
            "demo",
            "CreateSession",
            ORT_CODE_NOT_IMPLEMENTED,
            "No op registered for CustomOp",
        ));
        require_load(classify_backend_failure(
            "demo",
            "CreateSession",
            ORT_CODE_EP_FAIL,
            "provider not found",
        ));
        assert_eq!(
            classify_backend_failure("demo", "Run", 3, "invalid shape").code(),
            "E-MODEL-SIGNATURE"
        );
        require_load(classify_backend_failure(
            "demo",
            "Run",
            6,
            "kernel execution failed",
        ));
    }

    #[test]
    fn runtime_pin_rejects_wrong_release_or_license() {
        let digest = "a".repeat(64);
        let version = RuntimePin::from_parts("1.28.0", "libonnxruntime.so", digest.clone(), ONNX_RUNTIME_LICENSE);
        assert!(matches!(version, Err(ModelError::Provenance { .. })));
        let license = RuntimePin::from_parts(ONNX_RUNTIME_VERSION, "libonnxruntime.so", digest, "Apache-2.0");
        assert!(matches!(license, Err(ModelError::Provenance { .. })));
    }

    #[test]
    fn tensor_marshalling_rejects_wrong_byte_count_without_fallback() {
        let spec = TensorSpec::runtime("input", TensorDType::F32, [1, 2]);
        let error = TensorData::new(spec, vec![0; 4]).unwrap_err();
        assert!(matches!(error, ModelError::SignatureMismatch { .. }));
    }

    #[test]
    fn provenance_records_closure_license_hashes_and_offline_policy() {
        let package = ModelPackage {
            package: "demo".into(),
            output: "encoder".into(),
            signature_name: None,
            identity: crate::model::ModelIdentity {
                package: "demo".into(),
                package_version: "1.0.0".into(),
                license: "Apache-2.0".into(),
                graph_sha256: "a".repeat(64),
                weights_sha256: "b".repeat(64),
                tokenizer_sha256: "c".repeat(64),
                adapter_sha256: None,
                preprocessing: "none".into(),
                pooling: "mean".into(),
                normalization: "l2".into(),
                output_meaning: "embedding".into(),
                metric: "cosine".into(),
            },
            contract: crate::model::ModelContract {
                inputs: vec![TensorSpec::runtime("input", TensorDType::F32, [1, 2])],
                outputs: vec![TensorSpec::runtime("output", TensorDType::F32, [1, 2])],
                provider: ONNX_PACKAGE_PROVIDER.into(),
                custom_operators: false,
                max_context: None,
                max_batch: None,
                max_buffer_bytes: None,
            },
            artifacts: vec![
                ModelArtifact { path: "graph.onnx".into(), sha256: "a".repeat(64) },
                ModelArtifact { path: "weights.bin".into(), sha256: "b".repeat(64) },
                ModelArtifact { path: "tokenizer.json".into(), sha256: "c".repeat(64) },
            ],
        };
        let policy = OnnxRuntimePolicy::cpu(["Add"]).unwrap();
        let pin = RuntimePin::official_web().unwrap();
        let record =
            OnnxRuntimeProvenance::for_package(&package, &pin, &policy, OnnxRuntimeMode::Web).render();
        assert!(record.contains("runtime-version=1.29.0"));
        assert!(record.contains("provenance-schema=jet-model-onnxruntime-provenance-v1"));
        assert!(record.contains("mode=web"));
        assert!(record.contains("package=demo"));
        assert!(record.contains("package-version=1.0.0"));
        assert!(record.contains("package-license=Apache-2.0"));
        assert!(record.contains("model-identity-digest="));
        assert!(record.contains("runtime-release-commit=2e2543fbe9fae542f921d47a72d21d5a4ef0b710"));
        assert!(record.contains("runtime-release-url=https://github.com/microsoft/onnxruntime/releases/tag/v1.29.0"));
        assert!(record.contains("runtime-c-api-url=https://onnxruntime.ai/docs/api/c/"));
        assert!(record.contains("runtime-artifact=onnxruntime-web-1.29.0.tgz"));
        assert!(record.contains("runtime-artifact-url=https://registry.npmjs.org/onnxruntime-web/-/onnxruntime-web-1.29.0.tgz"));
        assert!(record.contains("runtime-artifact-sha256=7a934b7811c3b050ecfb7619722e2b4de771ce6da20520e17a2018a440316ef3"));
        assert!(record.contains("runtime-web-package=onnxruntime-web@1.29.0"));
        assert!(record.contains("runtime-license=MIT"));
        assert!(record.contains("custom-code-policy=denied"));
    }
    #[cfg(unix)]
    #[test]
    fn native_runtime_executes_real_onnx_fixture_through_typed_package() {
        use std::path::PathBuf;

        let runtime = std::env::var_os("JET_ONNX_RUNTIME_LIBRARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").expect("HOME"))
                    .join(".cache/jet-test-scratch/onnxruntime-1.29.0/onnxruntime-linux-x64-1.29.0")
                    .join("lib/libonnxruntime.so.1.29.0")
            });
        if !runtime.is_file() {
            eprintln!(
                "SKIP native ONNX Runtime fixture: missing {}. Fetch the pinned \
                 v1.29.0 archive to ~/.cache/jet-test-scratch/onnxruntime-1.29.0 \
                 or set JET_ONNX_RUNTIME_LIBRARY.",
                runtime.display()
            );
            return;
        }
        let root = PathBuf::from(std::env::var_os("HOME").expect("HOME"))
            .join(".cache/jet-test-scratch/onnxruntime-provider-native");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // ModelProto: x + bias -> y, with x/y shaped [1, 2] and float32 data.
        let field = |number: u8, value: &[u8]| {
            assert!(number < 16);
            let mut encoded = vec![number << 3 | 2];
            let mut length = value.len();
            while length >= 128 {
                encoded.push((length as u8 & 127) | 128);
                length >>= 7;
            }
            encoded.push(length as u8);
            encoded.extend_from_slice(value);
            encoded
        };
        let varint = |number: u8, mut value: u64| {
            let mut encoded = vec![number << 3];
            while value >= 128 {
                encoded.push((value as u8 & 127) | 128);
                value >>= 7;
            }
            encoded.push(value as u8);
            encoded
        };
        let join = |parts: &[Vec<u8>]| parts.iter().flat_map(|part| part.iter().copied()).collect::<Vec<_>>();
        let dimension = |value: u8| field(1, &[8, value]);
        let shape = join(&[dimension(1), dimension(2)]);
        let tensor_type = join(&[varint(1, 1), field(2, &shape)]);
        let type_info = field(1, &tensor_type);
        let value_info = |name: &[u8]| join(&[field(1, name), field(2, &type_info)]);
        let node = join(&[field(1, b"x"), field(1, b"bias"), field(2, b"y"), field(4, b"Add")]);
        let initializer = join(&[
            varint(1, 1), varint(1, 2), varint(2, 1),
            field(8, b"bias"), field(9, &[0, 0, 128, 63, 0, 0, 0, 64]),
        ]);
        let input = value_info(b"x");
        let output = value_info(b"y");
        let graph_body = join(&[
            field(1, &node), field(2, b"jet-add"), field(5, &initializer),
            field(11, &input), field(12, &output),
        ]);
        let graph = join(&[varint(1, 8), field(8, &[16, 13]), field(7, &graph_body)]);
        let weights = b"weights are embedded in the checked graph\n";
        let tokenizer = b"{\"tokenizer\":\"none\"}\n";
        std::fs::write(root.join("graph.onnx"), &graph).unwrap();
        std::fs::write(root.join("weights.bin"), weights).unwrap();
        std::fs::write(root.join("tokenizer.json"), tokenizer).unwrap();
        let hash = |bytes: &[u8]| SHA256::sha256_hex(bytes);
        let package = ModelPackage {
            package: "onnx-fixture".into(),
            output: "encoder".into(),
            signature_name: None,
            identity: crate::model::ModelIdentity {
                package: "onnx-fixture".into(),
                package_version: "1.0.0".into(),
                license: "MIT".into(),
                graph_sha256: hash(&graph),
                weights_sha256: hash(weights),
                tokenizer_sha256: hash(tokenizer),
                adapter_sha256: None,
                preprocessing: "none".into(),
                pooling: "none".into(),
                normalization: "none".into(),
                output_meaning: "embedding".into(),
                metric: "cosine".into(),
            },
            contract: crate::model::ModelContract {
                inputs: vec![TensorSpec::runtime("x", TensorDType::F32, [1, 2])],
                outputs: vec![TensorSpec::runtime("y", TensorDType::F32, [1, 2])],
                provider: ONNX_PACKAGE_PROVIDER.into(),
                custom_operators: false,
                max_context: None,
                max_batch: Some(1),
                max_buffer_bytes: Some(1024),
            },
            artifacts: vec![
                ModelArtifact { path: "graph.onnx".into(), sha256: hash(&graph) },
                ModelArtifact { path: "weights.bin".into(), sha256: hash(weights) },
                ModelArtifact { path: "tokenizer.json".into(), sha256: hash(tokenizer) },
            ],
        };
        let policy = OnnxRuntimePolicy::cpu(["Add"]).unwrap();
        let provider = OnnxRuntimeProvider::native(RuntimePin::official_linux_x64(runtime).unwrap(), policy).unwrap();
        let cancellation = CancellationToken::new();
        let mut session = ready(package.open_with(&root, &provider, &cancellation)).unwrap();
        let input = TensorData::new(
            TensorSpec::runtime("x", TensorDType::F32, [1, 2]),
            vec![0, 0, 64, 64, 0, 0, 128, 64],
        )
        .unwrap();
        let outputs = ready(session.run(&[input], &CancellationToken::new())).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].spec, TensorSpec::runtime("y", TensorDType::F32, [1, 2]));
        assert_eq!(outputs[0].bytes, vec![0, 0, 128, 64, 0, 0, 192, 64]);
        let _ = std::fs::remove_dir_all(root);
    }
}
