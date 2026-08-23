//! Build-plan graph foundation for D-BUILDTARGET1 and D-BUILDACTION1.
//!
//! This is the typed Rust substrate the future `BuildContext` comptime method
//! router will call. It intentionally contains no user-facing syntax and no
//! scheduling/cache execution policy.

use std::sync::atomic::AtomicU64;

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

mod handles;
pub use handles::*;
mod targets;
pub use targets::*;
mod actions_policy;
pub use actions_policy::*;
mod cache_cas;
pub use cache_cas::*;
mod remote_scheduler;
pub use remote_scheduler::*;
mod provenance_toolchains;
pub use provenance_toolchains::*;
mod plugins_modules;
pub use plugins_modules::*;
mod plan_graph;
pub use plan_graph::*;
mod plan_impl;
mod replay;
pub use replay::*;
mod context;
mod execution_helpers;
pub use context::*;
mod errors_keys;
pub use errors_keys::*;
mod runtime_bridge;
mod validation;
pub use runtime_bridge::*;
mod execution_runtime;
pub use execution_runtime::*;
#[cfg(target_os = "windows")]
mod windows_sandbox;
