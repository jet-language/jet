use super::actions_policy::BuildAction;
#[cfg(not(unix))]
use super::execution_runtime::prepare_output_destination;
use super::targets::BuildPath;
use super::validation::resolve_under;
use crate::SHA256;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionKey(pub(super) String);

impl ActionKey {
    pub fn new(value: impl Into<String>) -> Self {
        ActionKey(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        ContentDigest(format!("sha256:{}", SHA256::sha256_hex(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "digest must start with `sha256:`"));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "sha256 digest must contain exactly 64 hexadecimal digits"));
        }
        Ok(ContentDigest(format!("sha256:{}", hex.to_ascii_lowercase())))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Succeeded { exit_code: i32 },
    Failed { exit_code: i32 },
    RestoredFromCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHitReason {
    LocalActionRecordMatched,
    DeclaredOutputsRestored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMissReason {
    NoLocalActionRecord,
    ActionKeyChanged,
    DeclaredOutputMissing,
    CacheRecordInvalid,
    CacheRestoreFailed,
    RemoteDenied,
    UncachedAction,
    FrontEndIncomplete,
}

/// Stages that must finish before any action-cache lookup (E4-JP2 / #419).
/// Cache hits never skip parser, sema, policy, or diagnostics emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrontEndCompletion {
    pub parsed: bool,
    pub sema_checked: bool,
    pub policy_checked: bool,
    pub diagnostics_complete: bool,
}

impl FrontEndCompletion {
    pub fn all_complete() -> Self {
        FrontEndCompletion {
            parsed: true,
            sema_checked: true,
            policy_checked: true,
            diagnostics_complete: true,
        }
    }

    pub fn authorize_cache_lookup(self) -> Result<(), CacheBypassDenied> {
        if !self.parsed {
            return Err(CacheBypassDenied::Parser);
        }
        if !self.sema_checked {
            return Err(CacheBypassDenied::Sema);
        }
        if !self.policy_checked {
            return Err(CacheBypassDenied::Policy);
        }
        if !self.diagnostics_complete {
            return Err(CacheBypassDenied::Diagnostics);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheBypassDenied {
    Parser,
    Sema,
    Policy,
    Diagnostics,
}

impl CacheBypassDenied {
    pub fn as_str(self) -> &'static str {
        match self {
            CacheBypassDenied::Parser => "parser",
            CacheBypassDenied::Sema => "sema",
            CacheBypassDenied::Policy => "policy",
            CacheBypassDenied::Diagnostics => "diagnostics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCacheStatus {
    Hit(CacheHitReason),
    Miss(CacheMissReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCacheProvenance {
    pub status: ActionCacheStatus,
    pub remote_policy: RemoteCachePolicy,
}

impl ActionCacheProvenance {
    pub fn hit(reason: CacheHitReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Hit(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }

    pub fn miss(reason: CacheMissReason) -> Self {
        ActionCacheProvenance {
            status: ActionCacheStatus::Miss(reason),
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteActionRequest {
    CacheRead,
    CacheWrite,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDeniedReason {
    MissingGrantAndSandboxProof,
    GrantNotAllowed,
    ProofDoesNotMatchAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCacheDenied {
    pub request: RemoteActionRequest,
    pub reason: RemoteDeniedReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSandboxProof {
    pub sandbox_id: String,
    pub action_key: String,
    pub provenance_digest: ContentDigest,
}

impl RemoteSandboxProof {
    pub fn new(
        sandbox_id: impl Into<String>,
        action_key: impl Into<String>,
        provenance_digest: ContentDigest,
    ) -> Self {
        RemoteSandboxProof {
            sandbox_id: sandbox_id.into(),
            action_key: action_key.into(),
            provenance_digest,
        }
    }

    fn is_complete(&self) -> bool {
        !self.sandbox_id.is_empty()
            && !self.action_key.is_empty()
            && ContentDigest::parse(self.provenance_digest.as_str()).is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCachePolicy {
    DisabledUntilGrantAndSandboxProof,
    Granted {
        cache_read: bool,
        cache_write: bool,
        execute: bool,
        proof: RemoteSandboxProof,
    },
}

impl RemoteCachePolicy {
    pub fn disabled_until_grant_and_sandbox_proof() -> Self {
        RemoteCachePolicy::DisabledUntilGrantAndSandboxProof
    }

    pub fn granted(proof: RemoteSandboxProof) -> Self {
        RemoteCachePolicy::Granted {
            cache_read: true,
            cache_write: true,
            execute: true,
            proof,
        }
    }

    pub fn with_grants(
        cache_read: bool,
        cache_write: bool,
        execute: bool,
        proof: RemoteSandboxProof,
    ) -> Self {
        RemoteCachePolicy::Granted {
            cache_read,
            cache_write,
            execute,
            proof,
        }
    }

    pub fn proof(&self) -> Option<&RemoteSandboxProof> {
        match self {
            RemoteCachePolicy::DisabledUntilGrantAndSandboxProof => None,
            RemoteCachePolicy::Granted { proof, .. } => Some(proof),
        }
    }

    pub fn check(&self, request: RemoteActionRequest) -> Result<(), RemoteCacheDenied> {
        match self {
            RemoteCachePolicy::DisabledUntilGrantAndSandboxProof => {
                Err(RemoteCacheDenied {
                    request,
                    reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
                })
            }
            RemoteCachePolicy::Granted {
                cache_read,
                cache_write,
                execute,
                proof,
            } => {
                let allowed = match request {
                    RemoteActionRequest::CacheRead => *cache_read,
                    RemoteActionRequest::CacheWrite => *cache_write,
                    RemoteActionRequest::Execute => *execute,
                };
                if !proof.is_complete() {
                    return Err(RemoteCacheDenied {
                        request,
                        reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
                    });
                }
                if !allowed {
                    return Err(RemoteCacheDenied {
                        request,
                        reason: RemoteDeniedReason::GrantNotAllowed,
                    });
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub enum RemoteCacheError {
    Denied(RemoteCacheDenied),
    Io(io::Error),
    InvalidRecord(String),
}

impl std::fmt::Display for RemoteCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteCacheError::Denied(denied) => write!(
                f,
                "remote {:?} denied: {:?}",
                denied.request, denied.reason
            ),
            RemoteCacheError::Io(error) => write!(f, "remote cache I/O failed: {error}"),
            RemoteCacheError::InvalidRecord(message) => {
                write!(f, "remote cache record is invalid: {message}")
            }
        }
    }
}

impl std::error::Error for RemoteCacheError {}

impl From<io::Error> for RemoteCacheError {
    fn from(error: io::Error) -> Self {
        RemoteCacheError::Io(error)
    }
}

/// Filesystem-backed remote transport used by the remote-cache and remote-
/// execution seams. It is a transport, not a local fallback: every operation
/// requires a grant plus a complete sandbox proof, and records are keyed by
/// the action key rather than by a caller-controlled filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCacheTransport {
    root: PathBuf,
    cas: LocalCas,
}

impl RemoteCacheTransport {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        RemoteCacheTransport {
            cas: LocalCas::new(root.join("cas")),
            root,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn upload_blob(
        &self,
        bytes: &[u8],
        policy: &RemoteCachePolicy,
    ) -> Result<ContentDigest, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheWrite)
            .map_err(RemoteCacheError::Denied)?;
        Ok(self.cas.put_blob(bytes)?)
    }

    pub fn download_blob(
        &self,
        digest: &ContentDigest,
        policy: &RemoteCachePolicy,
    ) -> Result<Vec<u8>, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheRead)
            .map_err(RemoteCacheError::Denied)?;
        Ok(self.cas.read_blob(digest)?)
    }

    pub fn upload_action_record(
        &self,
        record: &ActionResultRecord,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheWrite)
            .map_err(RemoteCacheError::Denied)?;
        self.validate_proof_for_key(policy, &record.key, RemoteActionRequest::CacheWrite)?;
        validate_action_key(&record.key)?;
        for output in &record.outputs {
            ContentDigest::parse(output.digest.as_str())?;
            validate_remote_path(output.path.as_str())?;
        }
        validate_unique_remote_paths(record.outputs.iter().map(|output| output.path.as_str()))?;
        let path = self.record_path(&record.key)?;
        let bytes = encode_remote_record(record);
        ensure_remote_root(&self.root)?;
        atomic_restore_file(&self.root, &path, bytes.as_bytes())?;
        Ok(())
    }

    pub fn download_action_record(
        &self,
        key: &ActionKey,
        policy: &RemoteCachePolicy,
    ) -> Result<ActionResultRecord, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::CacheRead)
            .map_err(RemoteCacheError::Denied)?;
        self.validate_proof_for_key(policy, key, RemoteActionRequest::CacheRead)?;
        let path = self.record_path(key)?;
        let bytes = secure_read_file(&self.root, &path)?;
        let record = decode_remote_record(&bytes)?;
        if &record.key != key {
            return Err(RemoteCacheError::InvalidRecord(
                "record key does not match lookup key".to_string(),
            ));
        }
        validate_action_key(&record.key)?;
        for output in &record.outputs {
            validate_remote_path(output.path.as_str())?;
            ContentDigest::parse(output.digest.as_str())?;
        }
        validate_unique_remote_paths(record.outputs.iter().map(|output| output.path.as_str()))?;
        Ok(record)
    }

    fn validate_proof_for_key(
        &self,
        policy: &RemoteCachePolicy,
        key: &ActionKey,
        request: RemoteActionRequest,
    ) -> Result<(), RemoteCacheError> {
        if let Some(proof) = policy.proof() {
            if proof.action_key != key.as_str() {
                return Err(RemoteCacheError::Denied(RemoteCacheDenied {
                    request,
                    reason: RemoteDeniedReason::ProofDoesNotMatchAction,
                }));
            }
        }
        Ok(())
    }

    fn record_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        let digest = ContentDigest::from_bytes(key.as_str().as_bytes());
        let digest = ContentDigest::parse(digest.as_str())?;
        let hex = digest.as_str().strip_prefix("sha256:").ok_or_else(|| {
            RemoteCacheError::InvalidRecord("action key digest has no sha256 prefix".to_string())
        })?;
        Ok(self
            .root
            .join("records")
            .join(&hex[..2])
            .join(&hex[2..]))
    }

    /// Queue one remote execution request. Submission only writes a request;
    /// it never executes locally and never treats a missing remote worker as
    /// success.
    pub fn submit_execution(
        &self,
        request: &RemoteExecutionRequest,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.validate_proof_for_key(
            policy,
            &request.key,
            RemoteActionRequest::Execute,
        )?;
        validate_remote_execution_request(request)?;
        ensure_remote_root(&self.root)?;
        let path = self.execution_request_path(&request.key)?;
        let bytes = encode_remote_execution_request(request);
        atomic_restore_file(&self.root, &path, bytes.as_bytes())?;
        Ok(())
    }

    /// Publish a result produced by a remote worker. The worker must return
    /// the exact action, toolchain, sandbox, and provenance identities that
    /// were submitted; a mismatched result is rejected before it is visible.
    pub fn publish_execution_result(
        &self,
        result: &RemoteExecutionResult,
        policy: &RemoteCachePolicy,
    ) -> Result<(), RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.validate_proof_for_key(
            policy,
            &result.key,
            RemoteActionRequest::Execute,
        )?;
        validate_remote_execution_result(result)?;
        let request = self.read_execution_request(&result.key)?;
        validate_execution_parity(&request, result)?;
        ensure_remote_root(&self.root)?;
        let path = self.execution_result_path(&result.key)?;
        let bytes = encode_remote_execution_result(result);
        atomic_restore_file(&self.root, &path, bytes.as_bytes())?;
        Ok(())
    }

    /// Read a worker result for a submitted action. This is the explicit
    /// remote-execution read path; it does not run a local action on a miss.
    pub fn download_execution_result(
        &self,
        key: &ActionKey,
        policy: &RemoteCachePolicy,
    ) -> Result<RemoteExecutionResult, RemoteCacheError> {
        policy
            .check(RemoteActionRequest::Execute)
            .map_err(RemoteCacheError::Denied)?;
        self.validate_proof_for_key(policy, key, RemoteActionRequest::Execute)?;
        let request = self.read_execution_request(key)?;
        let path = self.execution_result_path(key)?;
        let bytes = secure_read_file(&self.root, &path)?;
        let result = decode_remote_execution_result(&bytes)?;
        if &result.key != key {
            return Err(RemoteCacheError::InvalidRecord(
                "execution result key does not match lookup key".to_string(),
            ));
        }
        validate_execution_parity(&request, &result)?;
        Ok(result)
    }

    fn read_execution_request(
        &self,
        key: &ActionKey,
    ) -> Result<RemoteExecutionRequest, RemoteCacheError> {
        let path = self.execution_request_path(key)?;
        let bytes = secure_read_file(&self.root, &path)?;
        let request = decode_remote_execution_request(&bytes)?;
        if &request.key != key {
            return Err(RemoteCacheError::InvalidRecord(
                "execution request key does not match lookup key".to_string(),
            ));
        }
        Ok(request)
    }

    fn execution_request_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        self.execution_path("requests", key)
    }

    fn execution_result_path(&self, key: &ActionKey) -> Result<PathBuf, RemoteCacheError> {
        self.execution_path("results", key)
    }

    fn execution_path(
        &self,
        kind: &str,
        key: &ActionKey,
    ) -> Result<PathBuf, RemoteCacheError> {
        let digest = ContentDigest::from_bytes(key.as_str().as_bytes());
        let hex = digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| {
                RemoteCacheError::InvalidRecord(
                    "execution key digest has no sha256 prefix".to_string(),
                )
            })?;
        Ok(self
            .root
            .join("execution")
            .join(kind)
            .join(&hex[..2])
            .join(&hex[2..]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecutionRequest {
    pub key: ActionKey,
    pub argv: Vec<String>,
    pub inputs: Vec<ActionInputSnapshot>,
    pub outputs: Vec<BuildPath>,
    pub toolchain_digest: ContentDigest,
    pub sandbox: RemoteSandboxProof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecutionResult {
    pub key: ActionKey,
    pub outcome: ActionOutcome,
    pub outputs: Vec<ActionOutputRecord>,
    pub toolchain_digest: ContentDigest,
    pub sandbox: RemoteSandboxProof,
}

fn validate_remote_execution_request(
    request: &RemoteExecutionRequest,
) -> Result<(), RemoteCacheError> {
    validate_action_key(&request.key)?;
    validate_remote_argv(&request.argv)?;
    ContentDigest::parse(request.toolchain_digest.as_str())?;
    if request.sandbox.action_key != request.key.as_str() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::ProofDoesNotMatchAction,
        }));
    }
    if !request.sandbox.is_complete() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
        }));
    }
    for input in &request.inputs {
        validate_remote_path(input.path.as_str())?;
        ContentDigest::parse(input.digest.as_str())?;
    }
    validate_unique_remote_paths(request.inputs.iter().map(|input| input.path.as_str()))?;
    for output in &request.outputs {
        validate_remote_path(output.as_str())?;
    }
    validate_unique_remote_paths(request.outputs.iter().map(|output| output.as_str()))?;
    Ok(())
}

fn validate_remote_execution_result(
    result: &RemoteExecutionResult,
) -> Result<(), RemoteCacheError> {
    validate_action_key(&result.key)?;
    ContentDigest::parse(result.toolchain_digest.as_str())?;
    if !result.sandbox.is_complete() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::MissingGrantAndSandboxProof,
        }));
    }
    if result.sandbox.action_key != result.key.as_str() {
        return Err(RemoteCacheError::Denied(RemoteCacheDenied {
            request: RemoteActionRequest::Execute,
            reason: RemoteDeniedReason::ProofDoesNotMatchAction,
        }));
    }
    for output in &result.outputs {
        validate_remote_path(output.path.as_str())?;
        ContentDigest::parse(output.digest.as_str())?;
    }
    validate_unique_remote_paths(result.outputs.iter().map(|output| output.path.as_str()))?;
    Ok(())
}

fn validate_execution_parity(
    request: &RemoteExecutionRequest,
    result: &RemoteExecutionResult,
) -> Result<(), RemoteCacheError> {
    if request.key != result.key
        || request.toolchain_digest != result.toolchain_digest
        || request.sandbox != result.sandbox
    {
        return Err(RemoteCacheError::InvalidRecord(
            "remote execution result changed action, toolchain, or sandbox provenance"
                .to_string(),
        ));
    }
    if request.outputs.len() != result.outputs.len()
        || request
            .outputs
            .iter()
            .zip(&result.outputs)
            .any(|(declared, actual)| declared != &actual.path)
    {
        return Err(RemoteCacheError::InvalidRecord(
            "remote execution result outputs do not match declarations".to_string(),
        ));
    }
    Ok(())
}

fn encode_remote_execution_request(request: &RemoteExecutionRequest) -> String {
    let mut encoded = format!(
        "version=1\nkey={}\ntoolchain={}\nsandbox={}\nproof_action={}\nproof_provenance={}\nargv={}\n",
        hex_encode(request.key.as_str().as_bytes()),
        request.toolchain_digest.as_str(),
        hex_encode(request.sandbox.sandbox_id.as_bytes()),
        hex_encode(request.sandbox.action_key.as_bytes()),
        request.sandbox.provenance_digest.as_str(),
        request.argv.len(),
    );
    for arg in &request.argv {
        encoded.push_str("arg=");
        encoded.push_str(&hex_encode(arg.as_bytes()));
        encoded.push('\n');
    }
    encoded.push_str(&format!("inputs={}\n", request.inputs.len()));
    for input in &request.inputs {
        encoded.push_str(&format!(
            "input\t{}\t{}\t{}\n",
            hex_encode(input.path.as_str().as_bytes()),
            input.digest.as_str(),
            input.byte_len
        ));
    }
    encoded.push_str(&format!("outputs={}\n", request.outputs.len()));
    for output in &request.outputs {
        encoded.push_str("output=");
        encoded.push_str(&hex_encode(output.as_str().as_bytes()));
        encoded.push('\n');
    }
    encoded
}

fn decode_remote_execution_request(
    bytes: &[u8],
) -> Result<RemoteExecutionRequest, RemoteCacheError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteCacheError::InvalidRecord("execution request is not UTF-8".to_string()))?;
    let mut version = None;
    let mut key = None;
    let mut toolchain_digest = None;
    let mut sandbox_id = None;
    let mut proof_action = None;
    let mut proof_provenance = None;
    let mut argv_count = None;
    let mut argv = Vec::new();
    let mut input_count = None;
    let mut inputs = Vec::new();
    let mut output_count = None;
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            version = Some(parse_remote_version(value, "execution request")?);
        } else if let Some(value) = line.strip_prefix("key=") {
            key = Some(ActionKey::new(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution key is not UTF-8".to_string())
            })?));
        } else if let Some(value) = line.strip_prefix("toolchain=") {
            toolchain_digest = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("sandbox=") {
            sandbox_id = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("sandbox id is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_action=") {
            proof_action = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("proof action key is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_provenance=") {
            proof_provenance = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("argv=") {
            argv_count = Some(parse_remote_count(value, "argv")?);
        } else if let Some(value) = line.strip_prefix("arg=") {
            argv.push(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution argument is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("inputs=") {
            input_count = Some(parse_remote_count(value, "inputs")?);
        } else if let Some(value) = line.strip_prefix("input\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(RemoteCacheError::InvalidRecord(
                    "execution input needs path, digest, and byte length".to_string(),
                ));
            }
            inputs.push(ActionInputSnapshot {
                path: BuildPath(String::from_utf8(hex_decode(fields[0])?).map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution input path is not UTF-8".to_string())
                })?),
                digest: ContentDigest::parse(fields[1])?,
                byte_len: fields[2].parse::<u64>().map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution input byte length is not a number".to_string())
                })?,
            });
        } else if let Some(value) = line.strip_prefix("outputs=") {
            output_count = Some(parse_remote_count(value, "outputs")?);
        } else if let Some(value) = line.strip_prefix("output=") {
            outputs.push(BuildPath(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution output path is not UTF-8".to_string())
            })?));
        }
    }
    if version != Some(1)
        || argv_count != Some(argv.len())
        || input_count != Some(inputs.len())
        || output_count != Some(outputs.len())
    {
        return Err(RemoteCacheError::InvalidRecord(
            "execution request count does not match records".to_string(),
        ));
    }
    let key = key.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution key".to_string()))?;
    let sandbox = RemoteSandboxProof::new(
        sandbox_id.ok_or_else(|| RemoteCacheError::InvalidRecord("missing sandbox id".to_string()))?,
        proof_action.ok_or_else(|| RemoteCacheError::InvalidRecord("missing proof action key".to_string()))?,
        proof_provenance.ok_or_else(|| RemoteCacheError::InvalidRecord("missing proof provenance".to_string()))?,
    );
    let request = RemoteExecutionRequest {
        key,
        argv,
        inputs,
        outputs,
        toolchain_digest: toolchain_digest
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing toolchain digest".to_string()))?,
        sandbox,
    };
    validate_remote_execution_request(&request)?;
    Ok(request)
}

fn encode_remote_execution_result(result: &RemoteExecutionResult) -> String {
    let mut encoded = format!(
        "version=1\nkey={}\noutcome={}\ntoolchain={}\nsandbox={}\nproof_action={}\nproof_provenance={}\noutputs={}\n",
        hex_encode(result.key.as_str().as_bytes()),
        encode_remote_outcome(result.outcome),
        result.toolchain_digest.as_str(),
        hex_encode(result.sandbox.sandbox_id.as_bytes()),
        hex_encode(result.sandbox.action_key.as_bytes()),
        result.sandbox.provenance_digest.as_str(),
        result.outputs.len(),
    );
    for output in &result.outputs {
        encoded.push_str(&format!(
            "output\t{}\t{}\t{}\n",
            hex_encode(output.path.as_str().as_bytes()),
            output.digest.as_str(),
            output.byte_len
        ));
    }
    encoded
}

fn decode_remote_execution_result(
    bytes: &[u8],
) -> Result<RemoteExecutionResult, RemoteCacheError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteCacheError::InvalidRecord("execution result is not UTF-8".to_string()))?;
    let mut version = None;
    let mut key = None;
    let mut outcome = None;
    let mut toolchain_digest = None;
    let mut sandbox_id = None;
    let mut proof_action = None;
    let mut proof_provenance = None;
    let mut output_count = None;
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            version = Some(parse_remote_version(value, "execution result")?);
        } else if let Some(value) = line.strip_prefix("key=") {
            key = Some(ActionKey::new(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result key is not UTF-8".to_string())
            })?));
        } else if let Some(value) = line.strip_prefix("outcome=") {
            outcome = Some(parse_remote_outcome(value)?);
        } else if let Some(value) = line.strip_prefix("toolchain=") {
            toolchain_digest = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("sandbox=") {
            sandbox_id = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result sandbox is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_action=") {
            proof_action = Some(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("execution result action key is not UTF-8".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("proof_provenance=") {
            proof_provenance = Some(ContentDigest::parse(value)?);
        } else if let Some(value) = line.strip_prefix("outputs=") {
            output_count = Some(parse_remote_count(value, "outputs")?);
        } else if let Some(value) = line.strip_prefix("output\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(RemoteCacheError::InvalidRecord(
                    "execution output needs path, digest, and byte length".to_string(),
                ));
            }
            outputs.push(ActionOutputRecord {
                path: BuildPath(String::from_utf8(hex_decode(fields[0])?).map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution output path is not UTF-8".to_string())
                })?),
                digest: ContentDigest::parse(fields[1])?,
                byte_len: fields[2].parse::<u64>().map_err(|_| {
                    RemoteCacheError::InvalidRecord("execution output byte length is not a number".to_string())
                })?,
            });
        }
    }
    if version != Some(1) || output_count != Some(outputs.len()) {
        return Err(RemoteCacheError::InvalidRecord(
            "execution result version or output count is invalid".to_string(),
        ));
    }
    let key = key.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution result key".to_string()))?;
    let result = RemoteExecutionResult {
        key,
        outcome: outcome
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution outcome".to_string()))?,
        outputs,
        toolchain_digest: toolchain_digest
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution toolchain digest".to_string()))?,
        sandbox: RemoteSandboxProof::new(
            sandbox_id.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution sandbox".to_string()))?,
            proof_action.ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution proof action".to_string()))?,
            proof_provenance
                .ok_or_else(|| RemoteCacheError::InvalidRecord("missing execution proof provenance".to_string()))?,
        ),
    };
    validate_remote_execution_result(&result)?;
    Ok(result)
}

fn encode_remote_outcome(outcome: ActionOutcome) -> String {
    match outcome {
        ActionOutcome::Succeeded { exit_code } => format!("succeeded:{exit_code}"),
        ActionOutcome::Failed { exit_code } => format!("failed:{exit_code}"),
        ActionOutcome::RestoredFromCache => "restored".to_string(),
    }
}

fn parse_remote_count(value: &str, field: &str) -> Result<usize, RemoteCacheError> {
    value.parse::<usize>().map_err(|_| {
        RemoteCacheError::InvalidRecord(format!("execution {field} count is not a number"))
    })
}

fn parse_remote_version(value: &str, record: &str) -> Result<u32, RemoteCacheError> {
    value.parse::<u32>().map_err(|_| {
        RemoteCacheError::InvalidRecord(format!("{record} version is not a number"))
    })
}

fn validate_action_key(key: &ActionKey) -> Result<(), RemoteCacheError> {
    if key.as_str().is_empty()
        || key
            .as_str()
            .chars()
            .any(|character| character == '\n' || character == '\r' || character == '\0')
    {
        return Err(RemoteCacheError::InvalidRecord(
            "action key is empty or contains a record delimiter".to_string(),
        ));
    }
    Ok(())
}

fn validate_remote_argv(argv: &[String]) -> Result<(), RemoteCacheError> {
    if argv.is_empty() || argv.iter().any(|arg| arg.trim().is_empty()) {
        return Err(RemoteCacheError::InvalidRecord(
            "remote execution argv must contain a non-empty command and arguments".to_string(),
        ));
    }
    Ok(())
}

fn validate_unique_remote_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Result<(), RemoteCacheError> {
    let mut seen = BTreeSet::new();
    for path in paths {
        if !seen.insert(path) {
            return Err(RemoteCacheError::InvalidRecord(format!(
                "remote execution path `{path}` is declared more than once"
            )));
        }
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, RemoteCacheError> {
    if value.len() % 2 != 0 {
        return Err(RemoteCacheError::InvalidRecord(
            "hex field has odd length".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| {
            RemoteCacheError::InvalidRecord("hex field contains a non-hex digit".to_string())
        })?;
        let low = hex_digit(pair[1]).ok_or_else(|| {
            RemoteCacheError::InvalidRecord("hex field contains a non-hex digit".to_string())
        })?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn ensure_remote_root(root: &Path) -> io::Result<()> {
    ensure_real_directory(root)
}

/// Create a directory tree without ever following an existing symlink. The
/// remote transport, CAS, and action-record paths all share this rule so a
/// cache root cannot redirect writes into an unrelated host directory.
pub(super) fn ensure_real_directory(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains `..`",
                ));
            }
            Component::Normal(part) => current.push(part),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("directory path `{}` is a symlink", current.display()),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!("directory path `{}` is not a directory", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                format!("directory path `{}` is not a real directory", current.display()),
                            ));
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn validate_remote_path(path: &str) -> Result<(), RemoteCacheError> {
    if path.is_empty()
        || Path::new(path).is_absolute()
        || path
            .chars()
            .any(|character| matches!(character, '\t' | '\n' | '\r' | '\0'))
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RemoteCacheError::InvalidRecord(format!(
            "output path `{path}` is not relative and normal"
        )));
    }
    Ok(())
}

fn encode_remote_record(record: &ActionResultRecord) -> String {
    let outcome = match record.outcome {
        ActionOutcome::Succeeded { exit_code } => format!("succeeded:{exit_code}"),
        ActionOutcome::Failed { exit_code } => format!("failed:{exit_code}"),
        ActionOutcome::RestoredFromCache => "restored".to_string(),
    };
    let status = match record.provenance.status {
        ActionCacheStatus::Hit(reason) => format!("hit:{reason:?}"),
        ActionCacheStatus::Miss(reason) => format!("miss:{reason:?}"),
    };
    let mut encoded = format!(
        "version=1\nkey={}\noutcome={}\nstatus={}\noutputs={}\n",
        hex_encode(record.key.as_str().as_bytes()),
        outcome,
        status,
        record.outputs.len()
    );
    for output in &record.outputs {
        encoded.push_str(&format!(
            "output\t{}\t{}\t{}\n",
            output.path.as_str(),
            output.digest.as_str(),
            output.byte_len
        ));
    }
    encoded
}

fn decode_remote_record(bytes: &[u8]) -> Result<ActionResultRecord, RemoteCacheError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| RemoteCacheError::InvalidRecord("record is not UTF-8".to_string()))?;
    let mut version = None;
    let mut key = None;
    let mut outcome = None;
    let mut status = None;
    let mut output_count = None;
    let mut outputs = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            version = Some(parse_remote_version(value, "cache record")?);
        } else if let Some(value) = line.strip_prefix("key=") {
            key = Some(ActionKey::new(String::from_utf8(hex_decode(value)?).map_err(|_| {
                RemoteCacheError::InvalidRecord("cache record key is not UTF-8".to_string())
            })?));
        } else if let Some(value) = line.strip_prefix("outcome=") {
            outcome = Some(parse_remote_outcome(value)?);
        } else if let Some(value) = line.strip_prefix("status=") {
            status = Some(parse_remote_status(value)?);
        } else if let Some(value) = line.strip_prefix("outputs=") {
            output_count = Some(value.parse::<usize>().map_err(|_| {
                RemoteCacheError::InvalidRecord("output count is not a number".to_string())
            })?);
        } else if let Some(value) = line.strip_prefix("output\t") {
            let fields = value.split('\t').collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(RemoteCacheError::InvalidRecord(
                    "output record needs path, digest, and byte length".to_string(),
                ));
            }
            let digest = ContentDigest::parse(fields[1])?;
            let byte_len = fields[2].parse::<u64>().map_err(|_| {
                RemoteCacheError::InvalidRecord("output byte length is not a number".to_string())
            })?;
            outputs.push(ActionOutputRecord {
                path: BuildPath(fields[0].to_string()),
                digest,
                byte_len,
            });
        }
    }
    if version != Some(1) || output_count != Some(outputs.len()) {
        return Err(RemoteCacheError::InvalidRecord(
            "cache record version or output count is invalid".to_string(),
        ));
    }
    let key = key.ok_or_else(|| RemoteCacheError::InvalidRecord("missing action key".to_string()))?;
    validate_action_key(&key)?;
    Ok(ActionResultRecord {
        key,
        outcome: outcome
            .ok_or_else(|| RemoteCacheError::InvalidRecord("missing action outcome".to_string()))?,
        outputs,
        provenance: ActionCacheProvenance {
            status: status
                .ok_or_else(|| RemoteCacheError::InvalidRecord("missing cache status".to_string()))?,
            remote_policy: RemoteCachePolicy::disabled_until_grant_and_sandbox_proof(),
        },
    })
}

fn parse_remote_outcome(value: &str) -> Result<ActionOutcome, RemoteCacheError> {
    if value == "restored" {
        return Ok(ActionOutcome::RestoredFromCache);
    }
    let (kind, code) = value.split_once(':').ok_or_else(|| {
        RemoteCacheError::InvalidRecord("outcome has no exit code".to_string())
    })?;
    let exit_code = code.parse::<i32>().map_err(|_| {
        RemoteCacheError::InvalidRecord("outcome exit code is not a number".to_string())
    })?;
    match kind {
        "succeeded" => Ok(ActionOutcome::Succeeded { exit_code }),
        "failed" => Ok(ActionOutcome::Failed { exit_code }),
        _ => Err(RemoteCacheError::InvalidRecord(
            "unknown action outcome".to_string(),
        )),
    }
}

fn parse_remote_status(value: &str) -> Result<ActionCacheStatus, RemoteCacheError> {
    let (kind, name) = value.split_once(':').ok_or_else(|| {
        RemoteCacheError::InvalidRecord("cache status has no reason".to_string())
    })?;
    match (kind, name) {
        ("hit", "LocalActionRecordMatched") => {
            Ok(ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched))
        }
        ("hit", "DeclaredOutputsRestored") => {
            Ok(ActionCacheStatus::Hit(CacheHitReason::DeclaredOutputsRestored))
        }
        ("miss", "NoLocalActionRecord") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::NoLocalActionRecord))
        }
        ("miss", "ActionKeyChanged") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged))
        }
        ("miss", "DeclaredOutputMissing") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::DeclaredOutputMissing))
        }
        ("miss", "CacheRecordInvalid") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::CacheRecordInvalid))
        }
        ("miss", "CacheRestoreFailed") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::CacheRestoreFailed))
        }
        ("miss", "RemoteDenied") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::RemoteDenied))
        }
        ("miss", "UncachedAction") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::UncachedAction))
        }
        ("miss", "FrontEndIncomplete") => {
            Ok(ActionCacheStatus::Miss(CacheMissReason::FrontEndIncomplete))
        }
        _ => Err(RemoteCacheError::InvalidRecord(
            "cache status kind does not match reason".to_string(),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutputRecord {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInputSnapshot {
    pub path: BuildPath,
    pub digest: ContentDigest,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResultRecord {
    pub key: ActionKey,
    pub outcome: ActionOutcome,
    pub outputs: Vec<ActionOutputRecord>,
    pub provenance: ActionCacheProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCas {
    root: PathBuf,
}

impl LocalCas {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalCas { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_blob(&self, bytes: &[u8]) -> io::Result<ContentDigest> {
        let digest = ContentDigest::from_bytes(bytes);
        let path = self.blob_path(&digest)?;
        ensure_real_directory(&self.root)?;
        match secure_read_file(&self.root, &path) {
            Ok(existing) => if ContentDigest::from_bytes(&existing) != digest {
                atomic_restore_file(&self.root, &path, bytes)?;
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                atomic_restore_file(&self.root, &path, bytes)?;
            }
            Err(error) => return Err(error),
        }
        Ok(digest)
    }

    pub fn read_blob(&self, digest: &ContentDigest) -> io::Result<Vec<u8>> {
        let bytes = secure_read_file(&self.root, &self.blob_path(digest)?)?;
        let actual = ContentDigest::from_bytes(&bytes);
        if &actual != digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("CAS blob digest mismatch: expected {}", digest.as_str()),
            ));
        }
        Ok(bytes)
    }

    pub fn snapshot_declared_inputs(
        &self,
        base: &Path,
        action: &BuildAction,
    ) -> io::Result<Vec<ActionInputSnapshot>> {
        let mut inputs = Vec::new();
        for input in &action.inputs {
            let path = resolve_under(base, input.as_str())?;
            let bytes = secure_read_file(base, &path)?;
            let digest = self.put_blob(&bytes)?;
            inputs.push(ActionInputSnapshot {
                path: input.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(inputs)
    }

    pub fn capture_declared_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        key: ActionKey,
        outcome: ActionOutcome,
        provenance: ActionCacheProvenance,
    ) -> io::Result<ActionResultRecord> {
        let mut outputs = Vec::new();
        for output in &action.outputs {
            let path = resolve_under(base, output.as_str())?;
            let bytes = secure_read_file(base, &path)?;
            let digest = self.put_blob(&bytes)?;
            outputs.push(ActionOutputRecord {
                path: output.clone(),
                digest,
                byte_len: bytes.len() as u64,
            });
        }
        Ok(ActionResultRecord {
            key,
            outcome,
            outputs,
            provenance,
        })
    }

    pub fn restore_declared_outputs(
        &self,
        base: &Path,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        self.restore_outputs(base, record)
    }

    pub fn restore_action_outputs(
        &self,
        base: &Path,
        action: &BuildAction,
        record: &ActionResultRecord,
    ) -> io::Result<()> {
        if record.outputs.len() != action.outputs.len()
            || record.outputs.iter().zip(&action.outputs).any(|(recorded, declared)| recorded.path != *declared)
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "cached output record does not exactly match action declarations"));
        }
        self.restore_outputs(base, record)
    }

    fn restore_outputs(&self, base: &Path, record: &ActionResultRecord) -> io::Result<()> {
        for output in &record.outputs {
            let path = resolve_under(base, output.path.as_str())?;
            let bytes = self.read_blob(&output.digest)?;
            atomic_restore_file(base, &path, &bytes)?;
        }
        Ok(())
    }

    fn blob_path(&self, digest: &ContentDigest) -> io::Result<PathBuf> {
        let digest = ContentDigest::parse(digest.as_str())?;
        let hex = digest.0.strip_prefix("sha256:").expect("validated prefix");
        let (prefix, rest) = hex.split_at(2);
        Ok(self.root
            .join("blobs")
            .join("sha256")
            .join(prefix)
            .join(rest))
    }
}

#[cfg(unix)]
pub(super) fn secure_read_file(base: &Path, path: &Path) -> io::Result<Vec<u8>> {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    let name = |value: &std::ffi::OsStr| CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in cache path"));
    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root"))?;
    let file_name = relative.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cache path has no file name"))?;
    let root = fs::OpenOptions::new().read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC).open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let file_name = name(file_name)?;
    let fd = unsafe { openat(dirfd, file_name.as_ptr(), O_RDONLY | O_NOFOLLOW | O_CLOEXEC, 0) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cache entry is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(unix))]
pub(super) fn secure_read_file(base: &Path, path: &Path) -> io::Result<Vec<u8>> {
    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "cache path escapes root"))?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else { continue };
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cache path contains a symlink"));
        }
    }
    fs::read(current)
}

#[cfg(all(test, unix))]
mod hostile_tests {
    use super::*;
    use super::super::execution_runtime::read_action_record;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("jet-cas-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn put_blob_rejects_cache_root_and_blob_tree_symlinks() {
        let outside = temp("outside");
        let root_parent = temp("root-link");
        symlink(&outside, root_parent.join("cache")).unwrap();
        assert!(LocalCas::new(root_parent.join("cache")).put_blob(b"secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

        let root = temp("blob-link");
        let cas = root.join("cache");
        fs::create_dir_all(&cas).unwrap();
        symlink(&outside, cas.join("blobs")).unwrap();
        assert!(LocalCas::new(&cas).put_blob(b"secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn known_digest_blob_and_action_record_symlinks_are_never_read() {
        let root = temp("read-link");
        let cas = LocalCas::new(root.join("cas"));
        let digest = cas.put_blob(b"host-bytes").unwrap();
        let blob = cas.blob_path(&digest).unwrap();
        fs::remove_file(&blob).unwrap();
        let host = root.join("host");
        fs::write(&host, b"host-bytes").unwrap();
        symlink(&host, &blob).unwrap();
        assert!(cas.read_blob(&digest).is_err());

        let records = root.join("records");
        fs::create_dir_all(&records).unwrap();
        let key = ActionKey("act-sha256:known".to_string());
        let host_record = root.join("host-record");
        fs::write(&host_record, format!("{}\n", key.as_str())).unwrap();
        let record = records.join("known");
        symlink(host_record, &record).unwrap();
        assert!(read_action_record(&records, &record, key).is_err());
    }

    #[test]
    fn concurrent_same_size_restores_use_unique_create_new_temps() {
        let root = temp("concurrent-restore");
        let output = root.join("out");
        let payloads = (0..16).map(|index| format!("payload-{index:08}").into_bytes()).collect::<Vec<_>>();
        std::thread::scope(|scope| {
            let jobs = payloads.iter().map(|payload| {
                let root = &root;
                let output = &output;
                scope.spawn(move || atomic_restore_file(root, output, payload))
            }).collect::<Vec<_>>();
            for job in jobs { job.join().unwrap().unwrap(); }
        });
        let final_bytes = fs::read(&output).unwrap();
        assert!(payloads.contains(&final_bytes));
        assert!(!fs::read_dir(&root).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".jet-restore-")));
    }
}

#[cfg(unix)]
pub(super) fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::ffi::CString;
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    extern "C" {
        fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32;
        fn mkdirat(dirfd: i32, pathname: *const i8, mode: u32) -> i32;
        fn unlinkat(dirfd: i32, pathname: *const i8, flags: i32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8) -> i32;
    }
    fn name(value: &std::ffi::OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in build output path"))
    }

    let relative = path.strip_prefix(base).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "build output escapes root"))?;
    let file_name = relative.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "build output has no file name"))?;
    let root = fs::OpenOptions::new().read(true).custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC).open(base)?;
    let mut held = Vec::<OwnedFd>::new();
    let mut dirfd = root.as_raw_fd();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else { continue };
            let part = name(part)?;
            let mut fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            if fd < 0 && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
                if unsafe { mkdirat(dirfd, part.as_ptr(), 0o755) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists { return Err(error); }
                }
                fd = unsafe { openat(dirfd, part.as_ptr(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) };
            }
            if fd < 0 { return Err(io::Error::last_os_error()); }
            held.push(unsafe { OwnedFd::from_raw_fd(fd) });
            dirfd = held.last().unwrap().as_raw_fd();
        }
    }
    let final_name = name(file_name)?;
    let (temp_name, fd) = loop {
        let mut random = [0u8; 16];
        std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut random)?;
        let nonce = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let temp_name = CString::new(format!(".jet-restore-{nonce}")).unwrap();
        let fd = unsafe { openat(dirfd, temp_name.as_ptr(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) };
        if fd >= 0 { break (temp_name, fd); }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists { return Err(error); }
    };
    let mut temp = unsafe { fs::File::from_raw_fd(fd) };
    temp.write_all(bytes)?;
    temp.sync_all()?;
    drop(temp);
    if unsafe { renameat(dirfd, temp_name.as_ptr(), dirfd, final_name.as_ptr()) } != 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { unlinkat(dirfd, temp_name.as_ptr(), 0) };
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn atomic_restore_file(base: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    prepare_output_destination(base, path)?;
    let mut random = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut random)?;
    let nonce = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let temp = path.with_extension(format!("jet-cache-restore-{nonce}.tmp"));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    std::io::Write::write_all(&mut file, bytes)?;
    if fs::symlink_metadata(path).is_ok() { fs::remove_file(path)?; }
    fs::rename(temp, path)
}
