//! Package-facing view of the canonical remote execution transport.
//!
//! Jetpack does not define a second scheduler or trust protocol. These types
//! are the same host-bound binding, grant policy, CAS transport, and signed
//! result records used by the build engine.

pub use jet_comptime::Comptime::Build::{
    BuildCapability, BuildResourcePool, ContentDigest, RemoteActionRequest, RemoteBuildBinding,
    RemoteCacheDenied, RemoteCacheError, RemoteCachePolicy, RemoteCacheTransport,
    RemoteDeniedReason, RemoteExecutionRequest, RemoteExecutionResult,
};
