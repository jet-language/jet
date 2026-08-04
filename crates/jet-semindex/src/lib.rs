//! D-SEMINDEX1: stable semantic-index query API over compiler facts.

#![allow(non_snake_case)]
#![deny(warnings)]

mod Build;
mod JSON;
mod Symbols;
mod Types;

pub use jet_sema::SemIndexEffectFacts;
pub use Build::{
    build_index, build_symbol_db, structural_nodes_from_parsed, HoverEntry, InlayHint, SymDef,
    SymKind, SymRef, SymbolDB,
};
pub use Types::{
    CallEdge, DefinitionAnchor, DefinitionFact, EffectFact, EffectProvenance,
    ExpandLens, ExpandProjection, ExpandValue, InstanceApplicationFact, InstanceFact, MemberFact,
    MemberKind, MemberOrigin, OutputEntryFact, OutputFact, SemIndex, SourceSpan, StructuralAudit, StructuralNode,
    StructuralSlotBoundary, StructuralSlotKind, SymbolDef, SymbolKind, SymbolRef, TypeDossier,
    ViewProjectionFact, ViewProvenanceFact, ViewSourceFact, ViewSourcePathFact,
    SCHEMA_VERSION,
};
pub use Symbols::{
    build_semantic_symbol_index, SemanticProvenance, SemanticSymbol, SemanticSymbolIndex,
    SemanticSymbolKind, SemanticVisibilityAnchor,
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

/// Load, check, and build consumer-neutral symbol facts for docs/completion/help.
pub fn open_symbols(entry: &Path) -> Result<SemanticSymbolIndex, SemIndexError> {
    let entry_str = entry.to_string_lossy();
    let (diags, bundle, facts) =
        jet_driver::Driver::check_file_with_effect_facts(&entry_str, None, false);
    if !diags
        .iter()
        .any(|d| d.severity == jet_foundation::Diagnostics::Severity::Error)
    {
        if let Some(bundle) = bundle {
            return Ok(build_symbol_db(&bundle, &facts).symbols);
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

/// Structural diff/merge index: check against the candidate path's real
/// directory so adjacent semantic module imports resolve before output exists.
pub fn open_structural_with_overlays(
    entry: &Path,
    overlays: &[(&Path, &str)],
) -> Result<SemIndex, SemIndexError> {
    let entry_str = entry.to_string_lossy();
    let (diags, bundle, facts) =
        jet_driver::Driver::check_file_with_overlays_and_import_root(&entry_str, overlays);
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

/// Index a staged fixture even when its deliberately expected diagnostics are
/// present. The returned facts still come from the real loader/sema pass; this
/// is not a syntax-only fallback.
pub fn open_with_overlays_and_diagnostics(
    entry: &Path,
    overlays: &[(&Path, &str)],
) -> Result<(SemIndex, Vec<Diagnostic>), SemIndexError> {
    open_with_overlays_diagnostics_and_inputs(entry, overlays)
        .map(|(index, diagnostics, _)| (index, diagnostics))
}

/// The same staged compiler pass plus every physical module read by the
/// loader. Transactional tools fingerprint this complete input set, including
/// read-only imports that happen to define no symbols.
pub fn open_with_overlays_diagnostics_and_inputs(
    entry: &Path,
    overlays: &[(&Path, &str)],
) -> Result<(SemIndex, Vec<Diagnostic>, Vec<std::path::PathBuf>), SemIndexError> {
    let entry_str = entry.to_string_lossy();
    let (diags, bundle, facts) =
        jet_driver::Driver::check_file_with_overlays(&entry_str, overlays, false);
    match bundle {
        Some(bundle) => {
            let inputs = bundle
                .modules
                .iter()
                .map(|module| module.path.clone())
                .collect();
            Ok((build_index(&bundle, &facts), diags, inputs))
        }
        None => Err(SemIndexError::Load(diags)),
    }
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
        assert_eq!(SCHEMA_VERSION, 12);
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
        assert!(json.contains("\"schema_version\":11"));
        assert!(json.contains("\"outputs\""));
        assert!(json.contains("\"definitions\""));
        assert!(json.contains("\"identity\""));
        assert!(json.contains("\"references\""));
        assert!(json.contains("\"calls\""));
        assert!(json.contains("\"effects\""));
        assert!(json.contains("\"members\""));
        assert!(json.contains("\"instances\""));
        assert!(json.contains("\"run\""));
    }

    #[test]
    fn generic_instance_identity_reaches_index_and_symbol_database() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_generic_instance_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        std::fs::write(
            &main,
            "module boxed<T, n: Int> { pub fn value() => Int { return n } }\nmodule a = boxed<Int, 3>\nmodule b = boxed<Int, 3>\nfn run() {}\n",
        ).unwrap();

        let index = open(&main).expect("generic module should index");
        assert_eq!(index.instances().len(), 1, "equivalent aliases share one instance");
        let instance = &index.instances()[0];
        assert_eq!(instance.fingerprint.len(), 64);
        assert!(!instance.full_key_hex.is_empty());
        assert_eq!(instance.applications.iter().map(|application| application.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(instance.applications.iter().all(|application| {
            application.module_path == main.to_string_lossy()
                && application.semantic_identity == format!("instance:{}", instance.fingerprint)
        }));
        assert_eq!(instance.arguments.len(), 2);
        assert_eq!(instance.exported_members, vec!["value"]);
        assert_eq!(instance.template_definition_id.len(), 64);
        let alias_defs: Vec<_> = index.definitions().iter()
            .filter(|definition| definition.identity == format!("instance:{}", instance.fingerprint))
            .map(|definition| definition.name.as_str())
            .collect();
        assert_eq!(alias_defs, vec!["a", "b"]);
        assert!(index.definitions().iter().any(|definition| {
            definition.identity == format!("instance:{}", instance.fingerprint)
        }));
        let json = index.to_json();
        assert!(json.contains(&instance.fingerprint));
        assert!(json.contains(&instance.full_key_hex));
        assert!(json.contains("\"applications\":[{\"name\":\"a\""));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_structural_child_has_explicit_slot() {
        let path = fixture("basics/default_refs.jet");
        let index = open(&path).expect("default-ref example should index");
        assert!(index.structural_nodes().iter().all(|node| {
            node.parent.is_none() || node.slot != "root"
        }));
    }

    #[test]
    fn checked_reference_facts_cover_import_members_aliases_and_receivers() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_reference_facts_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let library = root.join("library.jet");
        let main = root.join("main.jet");
        std::fs::write(&library, "pub fn report() { print(\"library\") }\n").unwrap();
        std::fs::write(
            &main,
            "use \"./library\" as api\nuse api.{report as alias_report}\n\nstruct Worker { value: Int }\nimpl Worker {\n    fn step(self) { print(\"worker\") }\n}\nstruct Other { value: Int }\nimpl Other {\n    fn step(self) { print(\"other\") }\n}\n\nfn run() {\n    api.report()\n    alias_report()\n    worker :: Worker.{value: 1}\n    worker.step()\n    other :: Other.{value: 2}\n    other.step()\n}\n",
        )
        .unwrap();

        let index = open(&main).expect("multi-file reference fixture should index");
        let target_kind = |name: &str| {
            index
                .references()
                .iter()
                .filter(|reference| reference.name == name)
                .filter_map(|reference| reference.target.as_ref().map(|target| target.kind.as_str()))
                .collect::<Vec<_>>()
        };
        assert!(target_kind("api").contains(&"import_alias"));
        assert!(target_kind("report").contains(&"function"));
        assert!(target_kind("alias_report").contains(&"function"));
        assert!(target_kind("step").contains(&"function"));
        let step_targets = index.references().iter()
            .filter(|reference| reference.name == "step")
            .filter_map(|reference| reference.target.as_ref())
            .map(|target| (target.module_path.clone(), target.def_span.start, target.def_span.end))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(step_targets.len(), 2, "receiver type must select exact method definition");
        let _ = std::fs::remove_dir_all(root);
    }
}
