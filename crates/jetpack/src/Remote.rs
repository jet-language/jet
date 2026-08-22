//! Package-facing view of the canonical remote execution transport.
//!
//! Jetpack does not define a second wire protocol, CAS, or scheduler. These
//! types are the host-owned binding, capability model, deterministic builder
//! selection, grant policy, and signed result statements used by the canonical
//! build engine. Result statements use the ratified HMAC-SHA256 transport
//! envelope; the statement covers the action, outputs, logs, worker proof, and
//! execution identity before any result can become local truth.

pub use jet_comptime::Comptime::Build::{
    remote_execution_identity, ActionInputSnapshot, ActionKey, BuildCapability, BuildPath,
    BuildResourcePool, ContentDigest, RemoteActionRequest, RemoteAttemptError, RemoteBuildBinding,
    RemoteBuildRequest, RemoteBuilder, RemoteBuilderCapabilities, RemoteCacheDenied,
    RemoteCacheError, RemoteCachePolicy, RemoteCacheTransport, RemoteDeniedReason, RemoteDispatch,
    RemoteExecutionRequest, RemoteExecutionResult, RemoteSandboxProof, RemoteScheduleError,
    RemoteScheduler,
};
