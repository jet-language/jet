//! D-BPE-* Canvas: source-backed graph projection and v1 edit transactions.
//!
//! Canvas is a client of the checked front end. It does not parse or type-check
//! by a second path: projection comes from `ProgramBundle` + semindex facts, and
//! writes go back through `jet fmt` before the file is replaced.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Diagnostics::{Diagnostic, Severity, Span, TextEdit};
use crate::AST::{self, Expr, Item, Stmt};
use crate::{FixEngine, SHA256};
use jet_semindex::{SemIndex, SemIndexEffectFacts, SourceSpan, SymbolKind};

include!("Canvas/schema_api.rs");
include!("Canvas/html.rs");
include!("Canvas/js.rs");
include!("Canvas/project_scan.rs");
include!("Canvas/project_transactions.rs");
include!("Canvas/graph_projection.rs");
include!("Canvas/graph_json.rs");
include!("Canvas/query_actions.rs");
include!("Canvas/edit_actions.rs");
include!("Canvas/graph_helpers.rs");
include!("Canvas/debug_source_git.rs");
include!("Canvas/validation_json.rs");
