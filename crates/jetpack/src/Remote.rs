//! Package-facing view of the canonical remote execution transport.
//!
//! Jetpack does not define a second wire protocol, CAS, or scheduler. These
//! types are the host-owned binding, capability model, deterministic builder
//! selection, grant policy, and authenticated result records used by the
//! canonical build engine.

pub use jet_comptime::Comptime::Build::{
    remote_execution_identity, ActionKey, BuildCapability, BuildResourcePool, ContentDigest,
    RemoteActionRequest, RemoteAttemptError, RemoteBuildBinding, RemoteBuildRequest, RemoteBuilder,
    RemoteBuilderCapabilities, RemoteCacheDenied, RemoteCacheError, RemoteCachePolicy,
    RemoteCacheTransport, RemoteDeniedReason, RemoteDispatch, RemoteExecutionRequest,
    RemoteExecutionResult, RemoteSandboxProof, RemoteScheduleError, RemoteScheduler,
};
