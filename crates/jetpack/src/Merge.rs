//! Historical path re-export (card #367 slice 4): the §6 merge engine now
//! lives in `jet-pkg-model` (pure/std-only, shared by both realizers via
//! `jet-env-model`). Kept as `crate::Merge` so every internal call site in
//! this crate is unchanged.

pub use jet_pkg_model::Merge::*;
