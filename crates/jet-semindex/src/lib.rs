//! D-SEMINDEX1: stable semantic-index query API over compiler facts.

#![allow(non_snake_case)]
#![deny(warnings)]

mod Build;
mod Json;
mod Types;

pub use jet_sema::SemIndexEffectFacts;
pub use Build::{
    build_index, build_symbol_db, HoverEntry, InlayHint, SymDef, SymKind, SymRef, SymbolDB,
};
pub use Types::{
    CallEdge, EffectFact, MemberFact, MemberKind, MemberOrigin, SemIndex, SourceSpan,
    StructuralAudit, SymbolDef, SymbolKind, SymbolRef, TypeDossier, SCHEMA_VERSION,
};

use jet_foundation::Diagnostics::Diagnostic;
use jet_foundation::AST::ProgramBundle;
use std::path::Path;

/// Structured errors for project loading / I/O only (compiler diagnostics stay
/// on the front-end check path).
#[derive(Debug)]
pub enum SemIndexError {
    Load(Vec<Diagnostic>),
}

impl std::fmt::Display for SemIndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SemIndexError::Load(diags) => write!(f, "{} load/check diagnostic(s)", diags.len()),
        }
    }
}

impl std::error::Error for SemIndexError {}

/// Build a semantic index from an already-checked bundle and captured effect facts.
pub fn from_checked(bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> SemIndex {
    build_index(bundle, facts)
}

/// Load, check, and build the semantic index for an entry file (loader → parser → sema).
pub fn open(entry: &Path) -> Result<SemIndex, SemIndexError> {
    let entry_str = entry.to_string_lossy();
    let (diags, bundle, facts) =
        jet_driver::Driver::check_file_with_effect_facts(&entry_str, None, false);
    if !diags
        .iter()
        .any(|d| d.severity == jet_foundation::Diagnostics::Severity::Error)
    {
        if let Some(bundle) = bundle {
            return Ok(build_index(&bundle, &facts));
        }
    }
    Err(SemIndexError::Load(diags))
}

/// Build the index from the compiler's staged multi-file overlay path.
pub fn open_with_overlays(
    entry: &Path,
    overlays: &[(&Path, &str)],
) -> Result<SemIndex, SemIndexError> {
    let entry_str = entry.to_string_lossy();
    let (diags, bundle, facts) =
        jet_driver::Driver::check_file_with_overlays(&entry_str, overlays, false);
    if !diags
        .iter()
        .any(|d| d.severity == jet_foundation::Diagnostics::Severity::Error)
    {
        if let Some(bundle) = bundle {
            return Ok(build_index(&bundle, &facts));
        }
    }
    Err(SemIndexError::Load(diags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/features")
            .join(name)
    }

    #[test]
    fn schema_version_constant() {
        assert_eq!(SCHEMA_VERSION, 3);
    }

    #[test]
    fn open_finds_definitions_and_calls() {
        let path = fixture("effects/effects.jet");
        let idx = open(&path).expect("effects example should index");
        assert_eq!(idx.schema_version(), SCHEMA_VERSION);
        assert!(idx.lookup("run").is_some());
        assert!(idx.lookup("report").is_some());
        assert!(
            !idx.call_edges().is_empty(),
            "expected call graph edges in effects example"
        );
        assert!(
            idx.effect_of("report").is_some(),
            "expected effect facts for report()"
        );
    }

    #[test]
    fn references_filter_by_name() {
        let path = fixture("basics/hello.jet");
        let idx = open(&path).expect("hello example should index");
        let refs = idx.references_to("print");
        assert!(!refs.is_empty());
    }

    #[test]
    fn json_snapshot_shape() {
        let path = fixture("basics/hello.jet");
        let idx = open(&path).expect("hello example should index");
        let json = idx.to_json();
        assert!(json.contains("\"schema_version\":3"));
        assert!(json.contains("\"definitions\""));
        assert!(json.contains("\"identity\""));
        assert!(json.contains("\"references\""));
        assert!(json.contains("\"calls\""));
        assert!(json.contains("\"effects\""));
        assert!(json.contains("\"members\""));
        assert!(json.contains("\"run\""));
    }
}
