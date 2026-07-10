//! Build-plan graph foundation for D-BUILDTARGET1 and D-BUILDACTION1.
//!
//! This is the typed Rust substrate the future `BuildContext` comptime method
//! router will call. It intentionally contains no user-facing syntax and no
//! scheduling/cache execution policy.

use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

include!("Build/handles.rs");
include!("Build/targets.rs");
include!("Build/actions_policy.rs");
include!("Build/cache_cas.rs");
include!("Build/provenance_toolchains.rs");
include!("Build/plugins_modules.rs");
include!("Build/plan_graph.rs");
include!("Build/plan_impl.rs");
include!("Build/execution_helpers.rs");
include!("Build/context.rs");
include!("Build/errors_keys.rs");
include!("Build/validation.rs");
include!("Build/runtime_bridge.rs");
include!("Build/execution_runtime.rs");
