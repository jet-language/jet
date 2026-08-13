//! D-SEMINDEX1: stable semantic-index query API over compiler facts.

#![allow(non_snake_case)]
#![deny(warnings)]

mod Build;
mod JSON;
mod Symbols;
mod Types;

pub use JSON::{package_facts_json, workspace_overlay_policy_json};
pub use jet_sema::SemIndexEffectFacts;
pub use Build::{
    binder_active_parameter, build_index, build_symbol_db, function_parameter_parts,
    structural_nodes_from_parsed,
    HoverEntry, InlayHint, SymDef, SymKind, SymRef, SymbolDB,
};
pub use Types::{
    CallEdge, CallableParameterFact, CallableSignatureFact, DefinitionAnchor, DefinitionFact, EffectFact, EffectProvenance,
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
pub use jet_pkg_model::Package::PackageFacts;

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

/// Load the canonical Package facts that own an entry. Standalone source has
/// no package projection. A package root is one typed authority: malformed or
/// ambiguous package sources are returned as errors instead of becoming an
/// empty projection.
pub fn package_facts_for_entry(entry: &Path) -> Result<Option<PackageFacts>, String> {
    let dir = match jet_pkg_model::Authority::AuthorityResolver::open(entry) {
        Ok(resolver) => resolver.root().to_path_buf(),
        Err(jet_pkg_model::Authority::AuthorityError::WrongKind { .. }) => {
            let parent = entry.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
            jet_pkg_model::Authority::AuthorityResolver::open(parent)
                .map_err(|error| error.to_string())?
                .root()
                .to_path_buf()
        }
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let Some(root) = jet_driver::Loader::find_manifest_root_checked(&dir)
        .map_err(|diagnostic| {
            format!(
                "{}: {} — {}",
                diagnostic.code, diagnostic.what, diagnostic.why
            )
        })?
    else {
        return Ok(None);
    };
    PackageFacts::load_checked(&root)
        .map_err(|error| error.to_string())
}

/// Render the registered package-shape diagnostic shared by semantic-index
/// and Canvas when a package projection cannot be loaded.
pub fn package_facts_diagnostic(entry: &Path, error: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("package facts for `{}` have a shape error", entry.display()),
        format!(
            "one typed Package fact graph must own this projection; {error}"
        ),
        "fix package.jet or its declared Config files before tooling uses package facts".to_string(),
        None,
    )
}

/// Find the nearest validated workspace lock for a source entry. Overlay
/// policy is deliberately read from the persisted lock so Canvas, the
/// semantic index, and Jetpack do not each reparse workspace policy.
pub fn workspace_overlay_policy_for_entry(
    entry: &Path,
) -> Result<Option<jet_pkg_model::Overlay::OverlayPolicy>, Diagnostic> {
    let Some(parent) = entry.parent() else {
        return Ok(None);
    };
    let mut dir = jet_pkg_model::Authority::AuthorityResolver::open(parent)
        .map_err(|error| error.diagnostic())?
        .root()
        .to_path_buf();
    loop {
        let resolver = jet_pkg_model::Authority::AuthorityResolver::open(&dir)
            .map_err(|error| error.diagnostic())?;
        match resolver.resolve_workspace_source() {
            Err(error) => return Err(error.workspace_diagnostic()),
            Ok(Some(_)) => {
                return Ok(workspace_lock_at(&resolver)?
                    .and_then(|plan| (!plan.overlay_policy.is_empty()).then_some(plan.overlay_policy)))
            }
            Ok(None) => {
                if let Some(plan) = workspace_lock_at(&resolver)? {
                    return Ok((!plan.overlay_policy.is_empty()).then_some(plan.overlay_policy));
                }
            }
        }
        let Some(next) = dir.parent() else {
            return Ok(None);
        };
        if next == dir {
            return Ok(None);
        }
        dir = next.to_path_buf();
    }
}

/// Read one lock candidate and preserve the existing E1202 stale/invalid
/// workspace-lock diagnostic. `Some(plan)` is also a boundary marker when the
/// policy itself is empty, so callers do not continue into an outer authority.
fn workspace_lock_at(
    resolver: &jet_pkg_model::Authority::AuthorityResolver,
) -> Result<Option<jet_pkg_model::WorkspacePlan::WorkspacePlan>, Diagnostic> {
    let lock_file = match resolver
        .checked_file(Path::new(jet_pkg_model::Syntax::UNIFIED_LOCK_FILE))
    {
        Ok(file) => file,
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(error.diagnostic()),
    };
    let lock_path = lock_file.path.clone();
    let raw = lock_file.text().map_err(|error| error.diagnostic())?;
    if !jet_pkg_model::Lock::looks_like_workspace_lock(&raw) {
        return Ok(None);
    }
    match jet_pkg_model::WorkspaceLock::load_checked_file(resolver, lock_file) {
        Some(plan) => Ok(Some(plan)),
        None => Err(jet_pkg_model::Lock::e1202_workspace(
            &lock_path.display().to_string(),
        )),
    }
}

fn attach_package_facts(index: &mut SemIndex, entry: &Path) -> Result<(), SemIndexError> {
    if let Some(facts) = package_facts_for_entry(entry)
        .map_err(|error| SemIndexError::Load(vec![package_facts_diagnostic(entry, &error)]))?
    {
        index.attach_package_facts(facts);
    }
    if let Some(policy) = workspace_overlay_policy_for_entry(entry)
        .map_err(|diagnostic| SemIndexError::Load(vec![diagnostic]))?
    {
        index.attach_workspace_overlay_policy(policy);
    }
    Ok(())
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
            let mut index = build_index(&bundle, &facts);
            attach_package_facts(&mut index, entry)?;
            return Ok(index);
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
            let mut index = build_index(&bundle, &facts);
            attach_package_facts(&mut index, entry)?;
            return Ok(index);
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
            let mut index = build_index(&bundle, &facts);
            attach_package_facts(&mut index, entry)?;
            return Ok(index);
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
            let mut index = build_index(&bundle, &facts);
            attach_package_facts(&mut index, entry)?;
            Ok((index, diags, inputs))
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
        assert_eq!(SCHEMA_VERSION, 13);
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
        assert!(json.contains(&format!("\"schema_version\":{}", SCHEMA_VERSION)));
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
    fn package_config_facts_are_shared_with_the_index_json() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_package_facts_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.jet"),
            "name: \"demo\"\nversion: \"0.1.0\"\njet: \"0.4\"\nservices: .{ cache: .{ enable: true, ports: [6379], ready: \"ping\" } }\nenvironments: .{ dev: .Environment.{ tools: [\"git\"], services: .{ cache: .{ enable: true, ports: [6379] } }, secrets: .{ token: \"x\" } } }\nconfigs: [\"release.jet\"]\ndefaults: .{ run: app }\ndev :: Config.{ source: \"local\" }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("release.jet"),
            "Config.{ outputs: .{ app: .Executable.{ entry: run } } }\n",
        )
        .unwrap();
        let entry = root.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        let index = open(&entry).expect("package entry should index");
        let facts = index.package_facts().expect("package facts attached");
        assert_eq!(facts.name, "demo");
        assert!(facts.outputs.contains_key("app"));
        assert_eq!(facts.configs, vec!["release.jet"]);
        assert!(facts
            .field_provenance("outputs.app")
            .iter()
            .any(|origin| origin.ends_with("release.jet")));
        let json = index.to_json();
        assert!(json.contains("\"package_facts\""));
        assert!(json.contains("\"semantic_digest\":\""));
        for field in [
            "\"jet\":\"0.4\"",
            "\"services\":{\"cache\"",
            "\"environments\":{\"dev\"",
            "\"defaults\":{\"run\":\"app\"}",
            "\"resolved_config_paths\":[\"release.jet\"]",
            "\"inline_configs\":{\"dev\"",
            "\"kind\":\"executable\"",
        ] {
            assert!(json.contains(field), "package projection omitted {field}: {json}");
        }
        assert!(json.contains("release.jet"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_package_facts_are_not_dropped_from_projection() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_ambiguous_package_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.jet"),
            "name: \"canonical\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("pkg.jet"),
            "name: \"legacy\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        let entry = root.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        let error = package_facts_for_entry(&entry)
            .expect_err("ambiguous package roots must remain an error");
        assert!(error.contains("both `package.jet` and migration-era `pkg.jet`"), "{error}");
        let diagnostic = package_facts_diagnostic(&entry, &error);
        assert_eq!(diagnostic.code, "E1206");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn semantic_index_consumes_persisted_workspace_overlay_facts() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_workspace_overlay_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        std::fs::write(root.join("workspace.jet"), "module workspace { members: [] }\n").unwrap();
        let workspace_digest = jet_pkg_model::SHA256::sha256_hex(
            std::fs::read(root.join("workspace.jet")).unwrap().as_slice(),
        );
        std::fs::write(
            root.join(".jet/lock"),
            format!(
                "version = 1\nworkspace_source_digest = \"{workspace_digest}\"\nworkspace_policy_allow_unfree = [\"discord\"]\n\n[[workspace_overlay]]\nname = \"beta\"\nprovider = \"nixpkgs\"\nchannel = \"plasma-beta\"\n\n[[workspace_overlay_package]]\noverlay = \"beta\"\npackage = \"discord\"\nallow_unfree = true\nfield_priorities = [\"version=7\"]\n"
            ),
        )
        .unwrap();
        let entry = root.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        let index = open(&entry).expect("workspace source should index");
        let policy = index
            .workspace_overlay_policy()
            .expect("overlay facts must come from the lock");
        assert_eq!(policy.allow_unfree, vec!["discord"]);
        assert_eq!(policy.overlays[0].name, "beta");
        let json = index.to_json();
        assert!(json.contains("\"workspace_overlays\""));
        assert!(json.contains("plasma-beta"));
        assert!(json.contains("discord"));
        assert!(json.contains("\"field_priorities\":{\"version\":7}"), "{json}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_workspace_lock_returns_e1202_instead_of_empty_policy() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_stale_workspace_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        std::fs::write(root.join("workspace.jet"), "module workspace { members: [] }\n").unwrap();
        std::fs::write(
            root.join(".jet/lock"),
            "version = 1\nworkspace_source_digest = \"sha256-stale\"\n",
        )
        .unwrap();
        let entry = root.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        let diagnostic = workspace_overlay_policy_for_entry(&entry)
            .expect_err("stale workspace authority must remain visible");
        assert_eq!(diagnostic.code, "E1202");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn outer_stale_workspace_lock_does_not_cross_inner_workspace_boundary() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_nested_stale_workspace_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let child = root.join("child");
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(root.join("workspace.jet"), "module workspace { members: [] }\n").unwrap();
        std::fs::write(
            root.join(".jet/lock"),
            "version = 1\nworkspace_source_digest = \"sha256-stale\"\n",
        )
        .unwrap();
        std::fs::write(child.join("inner-authority.jet"), "module workspace { members: [] }\n").unwrap();
        let entry = child.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        assert!(workspace_overlay_policy_for_entry(&entry).unwrap().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ancestor_lock_scan_uses_an_arbitrary_workspace_source() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_ancestor_workspace_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let child = root.join("packages/app");
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        let source = "module workspace { members: [] }\n";
        std::fs::write(root.join("authority.jet"), source).unwrap();
        let digest = jet_pkg_model::SHA256::sha256_hex(source.as_bytes());
        std::fs::write(
            root.join(".jet/lock"),
            format!(
                "version = 1\nworkspace_source_digest = \"{digest}\"\nworkspace_policy_allow_unfree = [\"discord\"]\n"
            ),
        )
        .unwrap();
        let entry = child.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        let policy = workspace_overlay_policy_for_entry(&entry)
            .expect("ancestor workspace lock should load")
            .expect("persisted policy should be attached");
        assert_eq!(policy.allow_unfree, vec!["discord"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_workspace_boundary_is_not_hidden_by_an_outer_lock() {
        let root = std::env::temp_dir().join(format!(
            "jet_semindex_ambiguous_workspace_boundary_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let child = root.join("packages/app");
        std::fs::create_dir_all(root.join(".jet")).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        let source = "module workspace { members: [] }\n";
        std::fs::write(root.join("authority.jet"), source).unwrap();
        let digest = jet_pkg_model::SHA256::sha256_hex(source.as_bytes());
        std::fs::write(
            root.join(".jet/lock"),
            format!("version = 1\nworkspace_source_digest = \"{digest}\"\n"),
        )
        .unwrap();
        std::fs::write(child.join("a.jet"), source).unwrap();
        std::fs::write(child.join("b.jet"), source).unwrap();
        let entry = child.join("run.jet");
        std::fs::write(&entry, "fn run() {}\n").unwrap();

        let diagnostic = workspace_overlay_policy_for_entry(&entry)
            .expect_err("ambiguous boundary must remain visible");
        assert_eq!(diagnostic.code, "E1239");
        let _ = std::fs::remove_dir_all(root);
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
            "module boxed<T>(n: Int) { pub fn value() => Int { return n } }\nmodule a :: boxed<Int>(3)\nmodule b :: boxed<Int>(3)\nfn run() {}\n",
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
            "use \"./library\" as api\nuse api.[report as alias_report]\n\nstruct Worker { value: Int }\nimpl Worker {\n    fn step(self) { print(\"worker\") }\n}\nstruct Other { value: Int }\nimpl Other {\n    fn step(self) { print(\"other\") }\n}\n\nfn run() {\n    api.report()\n    alias_report()\n    worker :: Worker.{value: 1}\n    worker.step()\n    other :: Other.{value: 2}\n    other.step()\n}\n",
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
