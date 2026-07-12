//! D-BPE-* Canvas: source-backed graph projection and v1 edit transactions.
//!
//! Canvas is a client of the checked front end. It does not parse or type-check
//! by a second path: projection comes from `ProgramBundle` + semindex facts, and
//! writes go back through `jet fmt` before the file is replaced.

#[path = "Canvas/schema_api.rs"]
mod schema_api;
pub use schema_api::*;
#[path = "Canvas/html.rs"]
mod html;
pub use html::*;
#[path = "Canvas/js.rs"]
mod js;
pub use js::*;
#[path = "Canvas/project_scan.rs"]
mod project_scan;
#[path = "Canvas/project_transactions.rs"]
mod project_transactions;
#[path = "Canvas/graph_projection.rs"]
mod graph_projection;
#[path = "Canvas/graph_json.rs"]
mod graph_json;
#[path = "Canvas/query_actions.rs"]
mod query_actions;
#[path = "Canvas/edit_actions.rs"]
mod edit_actions;
#[path = "Canvas/graph_helpers.rs"]
mod graph_helpers;
#[path = "Canvas/debug_source_git.rs"]
mod debug_source_git;
#[path = "Canvas/validation_json.rs"]
mod validation_json;
