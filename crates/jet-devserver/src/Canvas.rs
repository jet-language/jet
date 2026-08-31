//! D-BPE-* Canvas: source-backed graph projection and v1 edit transactions.
//!
//! Canvas is a client of the checked front end. It does not parse or type-check
//! by a second path: projection comes from `ProgramBundle` + semindex facts, and
//! writes go back through `jet fmt` before the file is replaced.

#[path = "Canvas/edit_actions.rs"]
mod edit_transaction;
mod schema_api;
pub use schema_api::*;
// D-ARCH-SOURCE1=A: browser projection assets stay dependency-free in
// jet-canvas; semantic/edit APIs live with their dev-server host.
pub use jet_canvas::{canvas_html, canvas_html_for, canvas_html_query, canvas_js};
mod debug_source_git;
mod graph_helpers;
mod graph_json;
mod graph_projection;
mod project_scan;
mod project_transactions;
mod query_actions;
mod source_model;
pub(crate) use source_model::read_source_without_symlinks;
pub use source_model::source_revision;
pub use debug_source_git::DebugSessions;
mod validation_json;
