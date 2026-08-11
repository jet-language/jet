//! Re-export shim — the declaration-resolved workspace evaluator now lives in
//! `jet-env-model`
//! (card #367 D-PRODUCT-SPLIT1=C slice 5).
//!
//! All types and functions are re-exported here so existing call sites
//! that use `jetpack::WorkspaceFile::…` continue to compile unchanged.

pub use jet_env_model::WorkspaceFile::{
    evaluate, evaluate_source, has_build_entry, load, resolve_workspace_source, WorkspaceMember,
    WorkspacePlan, WorkspaceSource, WorkspaceSourceRole,
};
