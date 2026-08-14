//! Multi-file program loading (M6 phase 3, S16; M12 package support).
//!
//! Resolves the import graph from an entry `.jet` file, detects cycles and
//! ambiguous module names, and returns a `ProgramBundle` for sema/codegen.
//! When a `package.jet` is found in the project root (walked upward from entry),
//! validates the manifest and wires package dep paths into module search (M12.1).

use crate::Diagnostics::{Diagnostic, Span};
use jet_pkg_model::Authority::{AuthorityResolver, CheckedFile};
use crate::Lexer;
use crate::Manifest;
use crate::Parser;
use crate::Syntax;
use crate::AST::{ImportDecl, ImportKind, Item, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A loader diagnostic with the source file and source text that produced it.
/// The ordinary loader API still returns bare diagnostics for existing callers;
/// inspection tools use this form to preserve imported-module provenance.
#[derive(Debug, Clone)]
pub struct LoaderDiagnostic {
    pub file: String,
    pub source: String,
    pub diagnostic: Diagnostic,
}

#[derive(Debug)]
struct LoaderError {
    diagnostics: Vec<LoaderDiagnostic>,
}

impl LoaderError {
    fn at(file: &str, source: &str, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics: diagnostics
                .into_iter()
                .map(|diagnostic| LoaderDiagnostic {
                    file: file.to_string(),
                    source: source.to_string(),
                    diagnostic,
                })
                .collect(),
        }
    }

    fn into_plain(self) -> Vec<Diagnostic> {
        self.diagnostics
            .into_iter()
            .map(|entry| entry.diagnostic)
            .collect()
    }
}

fn is_foreign_namespace_import(imp: &ImportDecl) -> Result<bool, Diagnostic> {
    let active = crate::Foreign::is_active_namespace_import(imp)
        .map_err(|error| error.diagnostic())?;
    let c = crate::CFFI::is_c_import(imp).map_err(|error| error.diagnostic())?;
    Ok(active || c)
}

fn record_loader_error(
    sink: &mut Option<&mut Vec<LoaderDiagnostic>>,
    error: LoaderError,
) -> Vec<Diagnostic> {
    if let Some(sink) = sink.as_deref_mut() {
        sink.extend(error.diagnostics.iter().cloned());
    }
    error.into_plain()
}

fn checked_source_file(
    path: &Path,
    display: &str,
) -> Result<(AuthorityResolver, CheckedFile, String), LoaderError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = AuthorityResolver::open(parent)
        .map_err(|error| LoaderError::at(display, "", vec![error.diagnostic()]))?;
    let name = path
        .file_name()
        .ok_or_else(|| {
            LoaderError::at(
                display,
                "",
                vec![Diagnostic::error(
                    "E0603",
                    format!("can't find the file `{display}`"),
                    "an import path must point at an existing `.jet` file".to_string(),
                    "check the spelling, or create the missing file".to_string(),
                    None,
                )],
            )
        })?;
    let checked = resolver
        .checked_file(Path::new(name))
        .map_err(|error| LoaderError::at(display, "", vec![error.diagnostic()]))?;
    let source = checked
        .text()
        .map_err(|error| LoaderError::at(display, "", vec![error.diagnostic()]))?;
    resolver
        .revalidate_file(&checked)
        .map_err(|error| LoaderError::at(display, &source, vec![error.diagnostic()]))?;
    Ok((resolver, checked, source))
}

fn project_parts_loader_error(
    project_root: &Path,
    conflicts: &[crate::ProjectParts::ProjectPartConflict],
) -> LoaderError {
    let diagnostics = conflicts
        .iter()
        .map(|conflict| {
            let path = conflict.paths.first().cloned();
            let (file, source) = path
                .as_ref()
                .and_then(|path| {
                    checked_source_file(path, &relative_display(project_root, path))
                        .ok()
                        .map(|(_, _, source)| (path, source))
                })
                .map(|(path, source)| (relative_display(project_root, path), source))
                .unwrap_or_else(|| (project_root.display().to_string(), String::new()));
            LoaderDiagnostic {
                file,
                source,
                diagnostic: conflict.diagnostic(project_root, None),
            }
        })
        .collect();
    LoaderError { diagnostics }
}

/// What the project's manifest + the shared hangar tell the module resolver
/// about consuming packages with `use <pkg>` (U17).
///
/// One import concept (S16) covers files, modules, and `library` packages: once
/// a `library` is realized (its source staged in the hangar), `use <pkg>`
/// resolves to that staged tree — it is just an extra module search root. An
/// `executable` named in `use` is a teaching error (executables go on PATH,
/// E0982); a declared-but-unrealized `library` dependency points the user at
/// `jetpack build` (E0983).
#[derive(Default)]
pub struct PkgResolution {
    /// Realized `library` packages → their staged source dir in the hangar
    /// (an extra module search root). The hangar is authoritative for the
    /// realized kind: an empty-`bin` entry is a library (U10).
    realized_libs: HashMap<String, PathBuf>,
    /// Realized `executable` packages (non-empty `bin`) by name — naming one in
    /// `use` is E0982.
    realized_exes: HashSet<String>,
    /// Names this project declares as dependencies (`deps:`). A declared dep
    /// that isn't realized yet is E0983 rather than an unknown-module error.
    declared_deps: HashSet<String>,
}

impl PkgResolution {
    fn is_empty(&self) -> bool {
        self.realized_libs.is_empty()
            && self.realized_exes.is_empty()
            && self.declared_deps.is_empty()
    }
}

/// Build the U17 package resolution from a project's `package.jet` text and the
/// shared hangar store.
///
/// Reading the hangar is a *pure lookup*: the compiler never realizes on demand
/// (that is `jetpack build`'s job). This keeps `jet build`/`run` offline and
/// deterministic, exactly like the existing pre-fetched-dependency flow
/// (`collect_dep_dirs` only links deps already present on disk; `jet fetch` is
/// the separate realize step). So a declared-but-unbuilt library is a friendly
/// "run `jetpack build`" (E0983), never a silent network fetch.
fn collect_pkg_resolution(raw: &str) -> Result<PkgResolution, Diagnostic> {
    let mut declared_deps = HashSet::new();
    let facts = crate::Package::PackageFacts::parse(raw, "package.jet").map_err(|error| {
        match &error {
            crate::Package::PackageParseError::Composition(detail)
                if detail.contains("is a diagnostic code") => {
                    crate::Manifest::manifest_parse_diagnostic(Path::new("package.jet"), &error)
                }
            _ => Diagnostic::error(
                "E1206",
                "invalid package manifest".to_string(),
                error.to_string(),
                "fix the fields in package.jet before loading the project".to_string(),
                None,
            ),
        }
    })?;
    for (name, source) in &facts.deps {
        // S59/D-CFFI2: a `c@…` native-library dep is a link dep, not a Jet
        // package — it must not shadow `use <pkg>` resolution (e.g. a dep
        // named `c`). Skip it here; CFFI.rs reads it for link flags.
        if matches!(source, crate::Package::DepSource::CLib { .. }) {
            continue;
        }
        declared_deps.insert(name.clone());
    }

    let mut realized_libs = HashMap::new();
    let mut realized_exes = HashSet::new();
    let roots = crate::Store::resolve();
    for entry in crate::Store::list(&roots) {
        if entry.bin.is_empty() {
            // A realized `library` stages source with an empty `bin` (U10).
            let out = PathBuf::from(&entry.out);
            if out.is_dir() {
                realized_libs.entry(entry.name.clone()).or_insert(out);
            }
        } else {
            realized_exes.insert(entry.name.clone());
        }
    }

    Ok(PkgResolution {
        realized_libs,
        realized_exes,
        declared_deps,
    })
}

pub fn load_entry(entry_path: &str) -> Result<ProgramBundle, Vec<Diagnostic>> {
    crate::boot_tir_eval();
    load_entry_with_overlay(entry_path, None, false)
}

/// Load an entry file and retain source provenance for every loader failure.
pub fn load_entry_with_diagnostics(entry_path: &str) -> Result<ProgramBundle, Vec<LoaderDiagnostic>> {
    crate::boot_tir_eval();
    let mut dependencies = Vec::new();
    let mut diagnostics = Vec::new();
    let result = load_entry_with_overlays_mode_with_sink(
        entry_path,
        &[],
        false,
        false,
        &mut dependencies,
        Some(&mut diagnostics),
    );
    match result {
        Ok(bundle) => Ok(bundle),
        Err(fallback) => {
            if diagnostics.is_empty() {
                let source = checked_source_file(Path::new(entry_path), entry_path)
                    .ok()
                    .map(|(_, _, source)| source)
                    .unwrap_or_default();
                diagnostics.extend(fallback.into_iter().map(|diagnostic| LoaderDiagnostic {
                    file: entry_path.to_string(),
                    source: source.clone(),
                    diagnostic,
                }));
            }
            Err(diagnostics)
        }
    }
}

/// Load a program, optionally substituting in-memory source for one file
/// (LSP unsaved buffer for the document being edited).
pub fn load_entry_with_overlay(
    entry_path: &str,
    overlay: Option<(&Path, &str)>,
    for_check: bool,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
    crate::boot_tir_eval();
    let overlays: Vec<(&Path, &str)> = overlay.into_iter().collect();
    load_entry_with_overlays(entry_path, &overlays, for_check)
}

/// Load a program while substituting an ordered set of in-memory sources.
///
/// Codemods use this to re-check a staged multi-file tree without exposing a
/// partially rewritten tree on disk. Duplicate canonical paths are rejected by
/// the caller; the loader deliberately uses the last matching overlay so a
/// staged transaction can replace an earlier snapshot deterministically.
pub fn load_entry_with_overlays(
    entry_path: &str,
    overlays: &[(&Path, &str)],
    for_check: bool,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
    load_entry_with_overlays_and_dependencies(entry_path, overlays, for_check).0
}

pub fn load_entry_with_overlays_and_dependencies(
    entry_path: &str,
    overlays: &[(&Path, &str)],
    for_check: bool,
) -> (Result<ProgramBundle, Vec<Diagnostic>>, Vec<PathBuf>) {
    let mut dependencies = Vec::new();
    let result = load_entry_with_overlays_mode(
        entry_path,
        overlays,
        for_check,
        false,
        &mut dependencies,
    );
    (result, dependencies)
}

/// Structural tooling loads adjacent modules named by `use alias.Item` from
/// the candidate file's real directory. Normal compilation keeps D-MOD3's
/// explicit already-loaded-alias rule; this mode supplies the project context
/// an editor/merge operation has without rewriting the candidate source.
pub fn load_entry_with_overlays_and_import_root(
    entry_path: &str,
    overlays: &[(&Path, &str)],
    for_check: bool,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
    let mut dependencies = Vec::new();
    load_entry_with_overlays_mode(
        entry_path,
        overlays,
        for_check,
        true,
        &mut dependencies,
    )
}

fn load_entry_with_overlays_mode(
    entry_path: &str,
    overlays: &[(&Path, &str)],
    for_check: bool,
    load_adjacent_unqualified: bool,
    dependencies: &mut Vec<PathBuf>,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
    load_entry_with_overlays_mode_with_sink(
        entry_path,
        overlays,
        for_check,
        load_adjacent_unqualified,
        dependencies,
        None,
    )
}

fn load_entry_with_overlays_mode_with_sink(
    entry_path: &str,
    overlays: &[(&Path, &str)],
    for_check: bool,
    load_adjacent_unqualified: bool,
    dependencies: &mut Vec<PathBuf>,
    mut sink: Option<&mut Vec<LoaderDiagnostic>>,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
    // Check/LSP overlays skip `load_entry`; still need TirBridge before derive/comptime.
    crate::boot_tir_eval();
    let entry = PathBuf::from(entry_path);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let entry_abs = if entry.is_absolute() {
        entry
    } else {
        cwd.join(&entry)
    };
    let entry_abs = normalize_path(&entry_abs);

    // Walk upward from the entry file's directory to find the nearest package
    // and workspace roots. A nested workspace is a project boundary: an outer
    // package must not widen its module search root or make files outside the
    // nested workspace look importable.
    let entry_dir = entry_abs
        .parent()
        .map(normalize_path)
        .unwrap_or_else(|| cwd.clone());
    let workspace_root = match find_workspace_root_checked(&entry_dir) {
        Ok(root) => root,
        Err(diagnostic) => {
            return Err(record_loader_error(
                &mut sink,
                LoaderError::at(&entry_abs.display().to_string(), "", vec![diagnostic]),
            ));
        }
    };
    let manifest_root = match find_manifest_root_checked(&entry_dir) {
        Ok(root) => root.filter(|manifest| {
            workspace_root
                .as_ref()
                .is_none_or(|workspace| is_physically_within(workspace, manifest))
        }),
        Err(diagnostic) => {
            return Err(record_loader_error(
                &mut sink,
                LoaderError::at(&entry_abs.display().to_string(), "", vec![diagnostic]),
            ));
        }
    };
    if manifest_root.is_none() {
        if let Some((path, diagnostic)) = stale_manifest_name_diagnostic(&entry_dir) {
            return Err(record_loader_error(
                &mut sink,
                LoaderError::at(&path.display().to_string(), "", vec![diagnostic]),
            ));
        }
    }
    let validates_project_parts = manifest_root.is_some()
        || workspace_root.is_some()
        || entry_abs.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
            name == Syntax::DEFAULT_ENTRY_FILE
        });
    let mut layer_ceiling = None;
    let mut package_edition = Manifest::latest_edition().to_string();
    // U11 (D-JPK-SCRIPTDEP1=A): L0203 lints for inline `use pkg#version;`
    // deps found in the manifest-less branch below, merged into
    // `parse_teaching` once that's declared.
    let mut inline_dep_lints: Vec<Diagnostic> = Vec::new();
    let organization_policy = match load_organization_policy() {
        Ok(policy) => policy,
        Err(error) => return Err(record_loader_error(&mut sink, error)),
    };
    let (
        project_root,
        pkg_dep_dirs,
        pkg_resolution,
        package_policy,
        package_lints_deny,
    ) = if let Some(manifest_dir) = manifest_root
    {
        // Found a Package root — validate it and collect dep source paths.
        let resolver = match AuthorityResolver::open(&manifest_dir) {
            Ok(resolver) => resolver,
            Err(error) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &manifest_dir.display().to_string(),
                        "",
                        vec![error.diagnostic()],
                    ),
                ));
            }
        };
        let checked = match resolver.checked_manifest(std::path::Path::new(".")) {
            Ok(checked) => checked,
            Err(error) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &manifest_dir.display().to_string(),
                        "",
                        vec![error.diagnostic()],
                    ),
                ));
            }
        };
        let pack_path = checked.file.path.clone();
        let raw = match checked.file.text() {
            Ok(raw) => raw,
            Err(error) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &pack_path.display().to_string(),
                        "",
                        vec![error.diagnostic()],
                    ),
                ));
            }
        };
        let package_manifest = match crate::Package::PackageFacts::parse(&raw, pack_path.display().to_string()) {
            Ok(facts) => facts,
            Err(crate::Package::PackageParseError::BadMemoryPolicy { detail }) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &pack_path.display().to_string(),
                        &raw,
                        vec![Diagnostic::error(
                            "E0355",
                            "invalid package memory policy".to_string(),
                            detail,
                            "use `policy: .{ no_alloc: true, zero_rc: true, arena_bounded: 65536, gc: true, unsafe: .Forbid, sentries: .Off }` in `package.jet`".to_string(),
                            None,
                        )],
                    ),
                ));
            }
            Err(error @ crate::Package::PackageParseError::RetiredPolicyField { .. }) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &pack_path.display().to_string(),
                        &raw,
                        vec![crate::Manifest::manifest_parse_diagnostic(
                            &pack_path,
                            &error,
                        )],
                    ),
                ));
            }
            Err(crate::Package::PackageParseError::BadGuaranteePolicy { detail }) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &pack_path.display().to_string(),
                        &raw,
                        vec![Diagnostic::error(
                            "E1206",
                            "invalid package guarantee policy".to_string(),
                            detail,
                            "use `policy: .{ contain: [\"dependency\"], harden: true }` in `package.jet`; these keys are package-only and tighten-only".to_string(),
                            None,
                        )],
                    ),
                ));
            }
            Err(error) => {
                let diagnostic = match &error {
                    crate::Package::PackageParseError::Composition(detail)
                        if detail.contains("is a diagnostic code") => {
                            crate::Manifest::manifest_parse_diagnostic(&pack_path, &error)
                        }
                    _ => Diagnostic::error(
                        "E1206",
                        "invalid package manifest".to_string(),
                        error.to_string(),
                        "fix the fields in package.jet before loading the project".to_string(),
                        None,
                    ),
                };
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(
                        &pack_path.display().to_string(),
                        &raw,
                        vec![diagnostic],
                    ),
                ));
            }
        };
        match Manifest::parse(&pack_path, &raw) {
            Err(d) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(&pack_path.display().to_string(), &raw, vec![d]),
                ));
            }
            Ok(mf) => {
                layer_ceiling = mf.package.layer;
                package_edition = Manifest::effective_edition(&mf);
                // Check toolchain constraint (E1208).
                if let Err(d) = Manifest::check_toolchain(&mf, &pack_path.display().to_string()) {
                    return Err(record_loader_error(
                        &mut sink,
                        LoaderError::at(&pack_path.display().to_string(), &raw, vec![d]),
                    ));
                }

                // Check the `edition:` field (E2001, D-REL3): a manifest may not
                // ask for an edition this toolchain doesn't ship.
                if let Err(d) =
                    Manifest::check_edition_support(&mf, &pack_path.display().to_string())
                {
                    return Err(record_loader_error(
                        &mut sink,
                        LoaderError::at(&pack_path.display().to_string(), &raw, vec![d]),
                    ));
                }

                // If there are deps, check lock staleness (E1202) and
                // dry-resolve path dep graph to catch version conflicts (E1201).
                if !mf.dependencies.is_empty() {
                    // E1202: lock must exist and include all manifest deps.
                    let lock_file = match resolver.checked_file(Path::new(Syntax::UNIFIED_LOCK_FILE)) {
                        Ok(file) => Some(file),
                        Err(error) if error.is_missing() => None,
                        Err(error) => {
                            return Err(record_loader_error(
                                &mut sink,
                                LoaderError::at(
                                    &manifest_dir.display().to_string(),
                                    &raw,
                                    vec![error.diagnostic()],
                                ),
                            ));
                        }
                    };
                    if let Some(lock_file) = lock_file {
                        let lock_path = lock_file.path.clone();
                        let lock_raw = match lock_file.text() {
                            Ok(raw) => raw,
                            Err(error) => {
                                return Err(record_loader_error(
                                    &mut sink,
                                    LoaderError::at(
                                        &lock_path.display().to_string(),
                                        "",
                                        vec![error.diagnostic()],
                                    ),
                                ));
                            }
                        };
                        resolver.revalidate_file(&lock_file).map_err(|error| {
                            record_loader_error(
                                &mut sink,
                                LoaderError::at(
                                    &lock_path.display().to_string(),
                                    &lock_raw,
                                    vec![error.diagnostic()],
                                ),
                            )
                        })?;
                        if let Ok(lock) = crate::Lock::parse(&lock_raw) {
                            if let Err(d) = crate::Lock::verify_lock_matches_manifest(
                                &lock,
                                &mf,
                                &lock_path.display().to_string(),
                            ) {
                                return Err(record_loader_error(
                                    &mut sink,
                                    LoaderError::at(
                                        &lock_path.display().to_string(),
                                        &lock_raw,
                                        vec![d],
                                    ),
                                ));
                            }
                        } else {
                            return Err(record_loader_error(
                                &mut sink,
                                LoaderError::at(
                                    &lock_path.display().to_string(),
                                    &lock_raw,
                                    vec![crate::Lock::e1202(&lock_path.display().to_string())],
                                ),
                            ));
                        }
                        resolver.revalidate_file(&lock_file).map_err(|error| {
                            record_loader_error(
                                &mut sink,
                                LoaderError::at(
                                    &lock_path.display().to_string(),
                                    &lock_raw,
                                    vec![error.diagnostic()],
                                ),
                            )
                        })?;
                    }
                    // E1201: dry-resolve path deps for package name conflicts.
                    if let Err(d) = dry_resolve_path_deps(&mf, &manifest_dir) {
                        return Err(record_loader_error(
                            &mut sink,
                            LoaderError::at(&pack_path.display().to_string(), &raw, vec![d]),
                        ));
                    }
                }

                // E1212/E1213: each packages: entry must have exactly one
                // module declaration in the source tree (U10 Chunk 3).
                {
                    for pkg in &package_manifest.packages {
                        match crate::Package::discover_module_in(
                            &manifest_dir,
                            &pkg.name,
                        ) {
                            Ok(_) => {}
                            Err(crate::Package::DiscoveryError::NotFound {
                                name,
                            }) => {
                                return Err(record_loader_error(
                                    &mut sink,
                                    LoaderError::at(
                                        &pack_path.display().to_string(),
                                        &raw,
                                        vec![Manifest::e1212(
                                            &pack_path.display().to_string(),
                                            &name,
                                        )],
                                    ),
                                ));
                            }
                            Err(crate::Package::DiscoveryError::Ambiguous {
                                name,
                                paths,
                            }) => {
                                return Err(record_loader_error(
                                    &mut sink,
                                    LoaderError::at(
                                        &pack_path.display().to_string(),
                                        &raw,
                                        vec![Manifest::e1213(
                                            &pack_path.display().to_string(),
                                            &name,
                                            &paths,
                                        )],
                                    ),
                                ));
                            }
                        }
                    }
                }

                // Collect package dep source directories for module search.
                let dep_dirs = match collect_dep_dirs(&mf, &manifest_dir) {
                    Ok(dep_dirs) => dep_dirs,
                    Err(diagnostic) => {
                        return Err(record_loader_error(
                            &mut sink,
                            LoaderError::at(
                                &pack_path.display().to_string(),
                                &raw,
                                vec![diagnostic],
                            ),
                        ));
                    }
                };
                // U17: declared package kinds + realized library staging dirs.
                let resolution = collect_pkg_resolution(&raw).map_err(|diagnostic| {
                    record_loader_error(
                        &mut sink,
                        LoaderError::at(
                            &pack_path.display().to_string(),
                            &raw,
                            vec![diagnostic],
                        ),
                    )
                })?;
                let mut policy = organization_policy.clone();
                let package_lints_deny = package_manifest.policy.lints_deny.unwrap_or_default();
                policy.extend(package_manifest.policy.memory);
                let source = pack_path.display().to_string();
                for declaration in policy.iter_mut().filter(|declaration| declaration.scope == crate::Policy::PolicyScope::Package) { declaration.source = source.clone(); }
                resolver.revalidate_file(&checked.file).map_err(|error| {
                    record_loader_error(
                        &mut sink,
                        LoaderError::at(
                            &pack_path.display().to_string(),
                            &raw,
                            vec![error.diagnostic()],
                        ),
                    )
                })?;
                (manifest_dir, dep_dirs, resolution, policy, package_lints_deny)
            }
        }
    } else {
        // R9: no package manifest — single-file mode uses the nearest
        // workspace root when one exists, otherwise the entry directory.
        //
        // U11 (D-JPK-SCRIPTDEP1=A): a manifest-less entry may open with
        // inline `use pkg#version;` deps. Parse just the entry file to
        // collect them (load_file below reparses it as part of the normal
        // module-graph walk — a small duplicate parse keeps this pass
        // self-contained) and resolve each one up front, so the ordinary
        // `Module` import resolution (`resolve_module_import`) finds them in
        // `realized_libs` exactly like a hangar-realized `library` (U17).
        let project_root = workspace_root.clone().unwrap_or_else(|| entry_dir.clone());
        let mut resolution = PkgResolution::default();
        let (entry_resolver, entry_checked, raw) = match checked_source_file(
            &entry_abs,
            &entry_abs.display().to_string(),
        ) {
            Ok(checked) => checked,
            Err(error) => return Err(record_loader_error(&mut sink, error)),
        };
        let (toks, lex_diags) = crate::Lexer::lex(&raw);
        if lex_diags.is_empty() {
            if let Ok(prog) = crate::Parser::parse(&toks) {
                for dep in crate::ScriptDeps::collect(&prog) {
                    if !crate::ScriptDeps::is_pinned(&dep.selector) {
                        inline_dep_lints.push(crate::ScriptDeps::l0203_unpinned(&dep));
                    }
                    match crate::ScriptDeps::resolve(&dep, &entry_dir) {
                        Ok(resolved) => {
                            resolution
                                .realized_libs
                                .entry(resolved.name.clone())
                                .or_insert(resolved.dir);
                        }
                        Err(reason) => {
                            return Err(vec![crate::ScriptDeps::e1253(&dep, &reason)]);
                        }
                    }
                }
            }
        }
        if let Err(error) = entry_resolver.revalidate_file(&entry_checked) {
            return Err(record_loader_error(
                &mut sink,
                LoaderError::at(
                    &entry_abs.display().to_string(),
                    &raw,
                    vec![error.diagnostic()],
                ),
            ));
        }
        (
            project_root,
            HashMap::new(),
            resolution,
            organization_policy,
            Vec::new(),
        )
    };

    let mut modules = Vec::new();
    let mut path_to_idx: HashMap<PathBuf, usize> = HashMap::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    let mut parse_teaching = inline_dep_lints;
    let project_part_overlays = overlays
        .iter()
        .map(|(path, source)| (normalize_path(path), (*source).to_string()))
        .collect::<Vec<_>>();
    let (project_parts, project_part_failures) =
        crate::ProjectParts::scan_with_diagnostics(&project_root, &project_part_overlays);
    if let Some(failure) = project_part_failures.iter().find(|failure| failure.authority) {
        let file = relative_display(&project_root, &failure.path);
        return Err(record_loader_error(
            &mut sink,
            LoaderError::at(&file, "", vec![failure.problem.clone()]),
        ));
    }

    if let Err(error) = load_file(
        &entry_abs,
        entry_path,
        &project_root,
        &pkg_dep_dirs,
        &pkg_resolution,
        &package_policy,
        &package_lints_deny,
        &mut modules,
        &mut path_to_idx,
        &mut stack,
        overlays,
        for_check,
        &mut parse_teaching,
        &project_parts,
        &project_part_failures,
        dependencies,
    ) {
        return Err(record_loader_error(&mut sink, error));
    }

    // Explicit project imports report the same conflict at their source span
    // while loading. Any conflict left here still invalidates discovery,
    // including duplicate declarations whose internal names stay skipped.
    if validates_project_parts && !project_parts.conflicts.is_empty() {
        return Err(record_loader_error(
            &mut sink,
            project_parts_loader_error(&project_root, &project_parts.conflicts),
        ));
    }

    if load_adjacent_unqualified {
        let mut aliases = Vec::new();
        for import in &modules[0].imports {
            if !matches!(import.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            let is_foreign = import
                .foreign_imports()
                .map_err(|error| vec![error.diagnostic()])?;
            if !is_foreign.is_empty() {
                continue;
            }
            if let Some(binding) = import.walk_bindings().into_iter().next() {
                if crate::AST::core_list_prefix(binding.module_alias).is_some() {
                    continue;
                }
                aliases.push((
                    binding.module_alias.to_string(),
                    binding.module_alias_span,
                ));
            }
        }
        for (alias, span) in aliases {
            if modules.iter().any(|module| module.alias == alias) {
                continue;
            }
            let target = entry_abs
                .parent()
                .unwrap_or(Path::new("."))
                .join(&alias)
                .with_extension(Syntax::FILE_EXT);
            let norm = normalize_path(&target);
            let staged = overlays.iter().any(|(path, _)| normalize_path(path) == norm);
            let authority_exists = if staged {
                false
            } else {
                let parent = target.parent().unwrap_or_else(|| Path::new("."));
                let resolver = AuthorityResolver::open(parent)
                    .map_err(|error| vec![error.diagnostic()])?;
                let name = target.file_name().ok_or_else(|| {
                    vec![Diagnostic::error(
                        "E0603",
                        format!("can't find the file `{}`", target.display()),
                        "an adjacent module must name one regular `.jet` file".to_string(),
                        "use a valid module name or add the adjacent source file".to_string(),
                        None,
                    )]
                })?;
                match resolver.checked_file(Path::new(name)) {
                    Ok(file) => {
                        resolver
                            .revalidate_file(&file)
                            .map_err(|error| vec![error.diagnostic()])?;
                        true
                    }
                    Err(error) if error.is_missing() => false,
                    Err(error) => return Err(vec![error.diagnostic()]),
                }
            };
            if authority_exists || staged {
                let display = relative_display(&project_root, &target);
                if let Err(error) = load_file(
                    &target,
                    &display,
                    &project_root,
                    &pkg_dep_dirs,
                    &pkg_resolution,
                    &package_policy,
                    &package_lints_deny,
                    &mut modules,
                    &mut path_to_idx,
                    &mut stack,
                    overlays,
                    for_check,
                    &mut parse_teaching,
                    &project_parts,
                    &project_part_failures,
                    dependencies,
                ) {
                    return Err(record_loader_error(&mut sink, error));
                }
                // Supply the real adjacent module edge to sema. The user's
                // source remains untouched; this is the structural workspace
                // context corresponding to `use alias.Item`.
                modules[0].imports.push(ImportDecl {
                    kind: ImportKind::File(alias.clone(), span),
                    alias: alias.clone(),
                    alias_span: span,
                    span,
                    is_pub: false,
                    is_package_pub: false,
                    inline_version: None,
                });
            }
        }
    }

    let entry_idx = *path_to_idx.get(&entry_abs).ok_or_else(|| {
        vec![Diagnostic::error(
            "E0603",
            format!("can't find the file `{}`", entry_path),
            "the entry file must exist on disk".to_string(),
            "check the spelling and path".to_string(),
            None,
        )]
    })?;

    // Two files with the same stem (a/util.jet, b/util.jet) would emit two
    // `mod user_util` blocks; make every module alias unique.
    let mut seen_aliases: HashSet<String> = HashSet::new();
    for m in modules.iter_mut() {
        if !seen_aliases.insert(m.alias.clone()) {
            let mut n = 2usize;
            while !seen_aliases.insert(format!("{}_{}", m.alias, n)) {
                n += 1;
            }
            m.alias = format!("{}_{}", m.alias, n);
        }
    }

    // Pre-resolve every file import to its loaded module index so Codegen doesn't
    // need to call back into Loader (breaks the Codegen→Loader dep cycle).
    let mut name_ledger = crate::AST::NameLedger::default();
    for module_idx in 0..modules.len() {
        let (module_path, imports) = {
            let m = &modules[module_idx];
            (m.path.clone(), m.imports.clone())
        };
        for imp in &imports {
            if matches!(imp.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            if is_foreign_namespace_import(imp).map_err(|diagnostic| vec![diagnostic])? {
                continue;
            }
            if core_module_path(imp).is_some() {
                continue;
            }
            if let Ok(target_path) = resolve_import(
                imp,
                &module_path,
                &project_root,
                &pkg_dep_dirs,
                &pkg_resolution,
                &project_parts,
                &project_part_failures,
            ) {
                let norm = normalize_path(&target_path);
                if let Some(&target_idx) = path_to_idx.get(&norm) {
                    name_ledger.record_import_target(module_idx, imp.span, target_idx);
                }
            }
        }
    }

    // D-EFFBUDGET1: dependency name → resolved source root, for both `deps:`
    // entries and hangar-realized `use <pkg>` libraries (U17).
    let mut dep_roots: HashMap<String, PathBuf> = pkg_dep_dirs
        .iter()
        .map(|(name, dependency)| (name.clone(), dependency.source_root.clone()))
        .collect();
    for (name, dir) in &pkg_resolution.realized_libs {
        dep_roots.entry(name.clone()).or_insert_with(|| dir.clone());
    }

    let build_facts = jet_foundation::Facts::BuildFactSnapshot::script(
        &modules[entry_idx].path,
        Syntax::OSTarget::host(),
        "dev",
    );
    let mut bundle = ProgramBundle {
        entry: entry_idx,
        project_root,
        modules,
        parse_teaching,
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger,
        layer_ceiling,
        inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots,
        // D-OSTARGET2=B: default to the host OS; the driver overrides this from
        // `--target=<triple>` before sema runs (LSP/tests keep the host bucket).
        active_os: Syntax::OSTarget::host(),
        build_facts,
        edition: package_edition,
    };
    if bundle.modules.iter().any(|module| {
        module
            .imports
            .iter()
            .any(|import| core_module_path(import).as_deref() == Some("core.archive"))
    }) {
        let source = include_str!("../../../corelib/core.archive/pkgs/archive/archive.jet").to_string();
        let display = "corelib/core.archive/pkgs/archive/archive.jet".to_string();
        let (tokens, lex_diags) = Lexer::lex_generated(&source);
        if !lex_diags.is_empty() {
            return Err(record_loader_error(
                &mut sink,
                LoaderError::at(&display, &source, lex_diags),
            ));
        }
        let (mut program, teaching) = match Parser::parse_for_check(&tokens) {
            Ok(parsed) => parsed,
            Err(diags) => {
                return Err(record_loader_error(
                    &mut sink,
                    LoaderError::at(&display, &source, diags),
                ));
            }
        };
        bundle.parse_teaching.extend(teaching);
        let alias = "core_archive".to_string();
        if bundle.modules.iter().any(|module| module.alias == alias) {
            return Err(record_loader_error(
                &mut sink,
                LoaderError::at(
                    &display,
                    &source,
                    vec![Diagnostic::error(
                        "E0608",
                        "the reserved Core source module alias is already in use".to_string(),
                        "Core source packages use a private module namespace during emission".to_string(),
                        "rename the imported file module that uses `core_archive`".to_string(),
                        None,
                    )],
                ),
            ));
        }
        bundle.modules.push(LoadedModule {
            path: PathBuf::from("<corelib>/core.archive/pkgs/archive/archive.jet"),
            display,
            source,
            alias,
            imports: std::mem::take(&mut program.imports),
            items: program.items,
            script_body: program.script_body,
            block_spans: program.block_spans,
            web_target_ceiling: program.web_target_ceiling,
            pub_file: program.pub_file,
            no_prelude: program.no_prelude,
            default_target: program.default_target,
            html_path: program.html_path,
            no_alloc_policy: program.no_alloc_policy,
            policy_declarations: program.policy_declarations,
            rule_facts: program.rule_facts,
        });
    }
    // S59 (E2-M14): fold every `#Extern`/`#Bindgen module c.<lib>` into merged
    // synthetic modules and resolve C `use` forms before sema sees the tree.
    if let Err(diagnostics) = crate::Foreign::assemble_active_namespaces_with_provenance(&mut bundle) {
        return Err(record_loader_error(
            &mut sink,
            LoaderError {
                diagnostics: diagnostics
                    .into_iter()
                    .map(|entry| LoaderDiagnostic {
                        file: entry.file,
                        source: entry.source,
                        diagnostic: entry.diagnostic,
                    })
                    .collect(),
            },
        ));
    }
    match crate::CFFI::assemble_with_provenance(&mut bundle) {
        Ok(cffi) => bundle.cffi = cffi,
        Err(diagnostics) => {
            return Err(record_loader_error(
                &mut sink,
                LoaderError {
                    diagnostics: diagnostics
                        .into_iter()
                        .map(|entry| LoaderDiagnostic {
                            file: entry.file,
                            source: entry.source,
                            diagnostic: entry.diagnostic,
                        })
                        .collect(),
                },
            ));
        }
    }
    bundle.materialize_script_entries();
    Ok(bundle)
}

/// D-UNSAFE-OBLIG1=A: optional admin/CI organization floor. The configured
/// path is an explicit build input; unreadable or malformed input fails closed.
fn load_organization_policy() -> Result<Vec<crate::Policy::PolicyDeclaration>, LoaderError> {
    let Ok(configured) = std::env::var(Syntax::ENV_ORG_UNSAFE_POLICY) else { return Ok(Vec::new()) };
    let path = PathBuf::from(&configured);
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            return Err(LoaderError::at(
                &path.display().to_string(),
                "",
                vec![Diagnostic::error(
                    "E3109",
                    "cannot read the configured organization gate policy".to_string(),
                    format!("{} names `{}`, but it could not be read: {error}", Syntax::ENV_ORG_UNSAFE_POLICY, path.display()),
                    "fix the policy path or remove the environment variable".to_string(),
                    None,
                )],
            ));
        }
    };
    let mut declarations = match crate::Package::parse_policy_document(&source) {
        Ok(declarations) => declarations,
        Err(error) => {
            return Err(LoaderError::at(
                &path.display().to_string(),
                &source,
                vec![Diagnostic::error(
                    "E3109",
                    "the configured organization gate policy is malformed".to_string(),
                    format!("`{}` must contain a manifest-shaped `policy: .{{ unsafe: .Obligations, impure: .GateOnly, nondeterministic: .GateOnly }}` block: {error:?}", path.display()),
                    "fix the organization policy; configured policy never fails open".to_string(),
                    None,
                )],
            ));
        }
    };
    if declarations.is_empty() || declarations.iter().any(|declaration| !declaration.key.is_audited_gate()) {
        return Err(LoaderError::at(
            &path.display().to_string(),
            &source,
            vec![Diagnostic::error(
                "E3109",
                "the organization gate policy has the wrong shape".to_string(),
                "this admin input contains only the shared audited-gate fields".to_string(),
                "use `policy: .{ unsafe: .Obligations, impure: .GateOnly, nondeterministic: .GateOnly }` (with any subset of those fields)".to_string(),
                None,
            )],
        ));
    }
    for declaration in &mut declarations {
        declaration.scope = crate::Policy::PolicyScope::Organization;
        declaration.source = path.display().to_string();
    }
    Ok(declarations)
}

/// Return the canonical manifest path for legacy read-only callers.
/// Authority-sensitive callers must use `manifest_path_checked` so metadata
/// failures cannot become an absent Package.
pub fn manifest_path(root: &Path) -> Option<PathBuf> {
    manifest_path_checked(root).ok().flatten()
}

/// Return the checked canonical manifest path without following symlinks or
/// hiding authority errors. `pkg.jet` is never a fallback.
pub fn manifest_path_checked(root: &Path) -> Result<Option<PathBuf>, Diagnostic> {
    let resolver = match AuthorityResolver::open(root) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(error.diagnostic()),
    };
    match resolver.checked_manifest(Path::new(".")) {
        Ok(manifest) => {
            resolver
                .revalidate_file(&manifest.file)
                .map_err(|error| error.diagnostic())?;
            Ok(Some(manifest.file.path))
        }
        Err(error) if error.is_missing() => Ok(None),
        Err(error) => Err(error.diagnostic()),
    }
}

/// Walk upward from `start` to find the nearest Package root, stopping at the
/// nearest declaration-resolved workspace boundary. Discovery errors are
/// returned so an inner malformed or ambiguous workspace cannot expose an
/// outer package authority.
pub fn find_manifest_root_checked(start: &Path) -> Result<Option<PathBuf>, Diagnostic> {
    let mut dir = AuthorityResolver::authority_walk_root(start)
        .map_err(|error| error.diagnostic())?;
    loop {
        if let Some(resolver) = AuthorityResolver::open_for_authority_walk(&dir)
            .map_err(|error| error.diagnostic())?
        {
            match resolver.resolve_workspace_source() {
                Ok(Some(_)) => {
                    return resolver
                        .checked_manifest(Path::new("."))
                        .map(|_| Some(dir.clone()))
                        .or_else(|error| {
                            if error.is_missing() {
                                Ok(None)
                            } else {
                                Err(error.diagnostic())
                            }
                        });
                }
                Ok(None) => {}
                Err(error) => return Err(error.workspace_diagnostic()),
            }
            match resolver.checked_manifest(Path::new(".")) {
                Ok(_) => return Ok(Some(dir)),
                Err(error) if error.is_missing() => {}
                Err(error) => return Err(error.diagnostic()),
            }
        }
        let Some(parent) = AuthorityResolver::authority_walk_parent(&dir) else {
            return Ok(None);
        };
        dir = parent;
    }
}

/// Walk upward from `start` to find the nearest directory containing a Package
/// root, stopping at the active workspace boundary.
pub fn find_manifest_root(start: &Path) -> Option<PathBuf> {
    find_manifest_root_checked(start).ok().flatten()
}

/// Name the package/module authority from the canonical `run.jet` entry when
/// one exists. A retired `main.jet` never becomes the authority fallback.
pub fn authority_name_for_entry(entry: &Path) -> Result<String, Diagnostic> {
    let parent = entry.parent().unwrap_or_else(|| Path::new("."));
    let resolver = AuthorityResolver::open(parent).map_err(|error| error.diagnostic())?;
    let authority = match resolver.checked_file(Path::new(Syntax::DEFAULT_ENTRY_FILE)) {
        Ok(run) => {
            resolver
                .revalidate_file(&run)
                .map_err(|error| error.diagnostic())?;
            run.path
        }
        Err(error) if error.is_missing() => entry.to_path_buf(),
        Err(error) => return Err(error.diagnostic()),
    };
    let legacy_stem = Syntax::LEGACY_ENTRY_FILE
        .strip_suffix(".jet")
        .unwrap_or(Syntax::LEGACY_ENTRY_FILE);
    Ok(authority
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != legacy_stem)
        .unwrap_or("app")
        .to_string())
}

/// Walk upward from `start` to find the nearest workspace declaration and
/// preserve malformed or ambiguous declaration diagnostics.
///
/// Workspace roots are independent project boundaries. In particular, a
/// workspace nested below a package root must own file and module imports
/// inside that workspace instead of inheriting the package's wider tree.
pub fn find_workspace_root_checked(start: &Path) -> Result<Option<PathBuf>, Diagnostic> {
    let mut dir = AuthorityResolver::authority_walk_root(start)
        .map_err(|error| error.diagnostic())?;
    loop {
        if let Some(resolver) = AuthorityResolver::open_for_authority_walk(&dir)
            .map_err(|error| error.diagnostic())?
        {
            match resolver.resolve_workspace_source() {
                Ok(Some(_)) => return Ok(Some(dir)),
                Ok(None) => {}
                Err(error) => return Err(error.workspace_diagnostic()),
            }
        }
        let Some(parent) = AuthorityResolver::authority_walk_parent(&dir) else {
            return Ok(None);
        };
        dir = parent;
    }
}

/// D-JPK-FILENAME2=B (A2): walk upward the same way [`find_manifest_root`]
/// does, but look for a *retired* manifest filename instead of a Package root.
/// Stops (returns `None`) the moment a directory has `pkg.jet` — nothing
/// stale to report once the real manifest is found. Used to upgrade a plain
/// "no pkg.jet found" message into the E1226 teaching diagnostic when the
/// user's project still carries an old filename.
pub fn find_stale_manifest_name(start: &Path) -> Option<(PathBuf, &'static str)> {
    let mut dir = AuthorityResolver::authority_walk_root(start).ok()?;
    loop {
        let resolver = AuthorityResolver::open_for_authority_walk(&dir).ok()?;
        let Some(resolver) = resolver else {
            let parent = AuthorityResolver::authority_walk_parent(&dir)?;
            dir = parent;
            continue;
        };
        match resolver.checked_manifest(Path::new(".")) {
            Ok(_) => return None,
            Err(error) if error.is_missing() => {}
            Err(jet_pkg_model::Authority::AuthorityError::RetiredManifest(_)) => {
                return Some((dir, Syntax::PAYLOAD_FILE));
            }
            Err(_) => return None,
        }
        for name in Syntax::STALE_MANIFEST_NAMES {
            match resolver.checked_file(Path::new(name)) {
                Ok(file) => {
                    resolver.revalidate_file(&file).ok()?;
                    return Some((dir, name));
                }
                Err(error) if error.is_missing() => {}
                Err(_) => return None,
            }
        }
        match resolver.resolve_workspace_source() {
            Ok(Some(_)) => return None,
            Ok(None) => {}
            Err(_) => return None,
        }
        let Some(parent) = AuthorityResolver::authority_walk_parent(&dir) else {
            return None;
        };
        dir = parent;
    }
}

/// Build the shared E1226 diagnostic for a retired manifest filename.
pub fn stale_manifest_name_diagnostic(start: &Path) -> Option<(PathBuf, Diagnostic)> {
    let (dir, stale) = find_stale_manifest_name(start)?;
    let path = dir.join(stale);
    Some((
        path.clone(),
        Diagnostic::error(
            "E1226",
            format!(
                "`{stale}` is not the package manifest name — Jet reads `{}`",
                Syntax::PACKAGE_FILE
            ),
            "the Package root filename is frozen to one spelling (D-ECO-FILEROOT1) so tooling, docs, and every worked example never have to guess which file to read".to_string(),
            format!(
                "rename `{}` to `{}`",
                path.display(),
                dir.join(Syntax::PACKAGE_FILE).display()
            ),
            None,
        ),
    ))
}

/// Render the E1226 `old-manifest-filename` teaching diagnostic for `dir`
/// carrying the retired manifest name `stale` — or `None` when `start`
/// carries no retired name (the caller keeps its own "no pkg.jet found"
/// message in that case).
pub fn stale_manifest_name_message(start: &Path) -> Option<String> {
    let (_, diagnostic) = stale_manifest_name_diagnostic(start)?;
    Some(format!(
        "Error [{}]: {}\n Why: {}\n Fix: {}\n",
        diagnostic.code, diagnostic.what, diagnostic.why, diagnostic.fix,
    ))
}

/// Dry-resolve path dependencies to catch version conflicts (E1201).
/// Does not fetch, store, or link anything — only loads manifests.
fn dry_resolve_path_deps(mf: &Manifest::Manifest, project_root: &Path) -> Result<(), Diagnostic> {
    // pkg_name → (version, blame_chain)
    let mut seen: std::collections::HashMap<String, (String, Vec<String>)> =
        std::collections::HashMap::new();
    let root_name = mf.package.name.clone();
    dry_resolve_recursive(mf, project_root, &[root_name], &mut seen)
}

fn dry_resolve_recursive(
    mf: &Manifest::Manifest,
    pkg_dir: &Path,
    chain: &[String],
    seen: &mut std::collections::HashMap<String, (String, Vec<String>)>,
) -> Result<(), Diagnostic> {
    for (dep_alias, spec) in &mf.dependencies {
        let Manifest::DepSpec::Path { path } = spec else {
            continue; // only path deps are resolved dry; git deps need fetch
        };
        let dep_path = normalize_path(&pkg_dir.join(path));
        // Load the dep's manifest to get its package name + version.
        let dep_mf = match Manifest::load(&dep_path) {
            None => continue, // missing manifest is caught later; not an E1201
            Some(Ok(manifest)) => manifest,
            Some(Err(diagnostic)) => return Err(diagnostic),
        };
        let dep_pkg_name = dep_mf.package.name.clone();
        let dep_version = dep_mf.package.version.clone();

        let mut child_chain: Vec<String> = chain.to_vec();
        child_chain.push(dep_alias.clone());

        if let Some((prev_ver, prev_chain)) = seen.get(&dep_pkg_name).cloned() {
            if prev_ver != dep_version {
                return Err(crate::Lock::e1201(
                    &dep_pkg_name,
                    &prev_ver,
                    &prev_chain,
                    &dep_version,
                    &child_chain,
                ));
            }
        } else {
            seen.insert(dep_pkg_name.clone(), (dep_version, child_chain.clone()));
            // Recurse into transitive deps.
            dry_resolve_recursive(&dep_mf, &dep_path, &child_chain, seen)?;
        }
    }
    Ok(())
}

#[derive(Clone)]
struct DependencyDir {
    manifest_root: PathBuf,
    source_root: PathBuf,
}

/// Collect each dependency's owning manifest root and source root.
fn collect_dep_dirs(
    mf: &Manifest::Manifest,
    project_root: &Path,
) -> Result<HashMap<String, DependencyDir>, Diagnostic> {
    let mut dirs = HashMap::new();
    for (dep_name, spec) in &mf.dependencies {
        match spec {
            Manifest::DepSpec::Path { path } => {
                let abs = normalize_path(&project_root.join(path));
                let resolver = match AuthorityResolver::open(&abs) {
                    Ok(resolver) => resolver,
                    Err(error) if error.is_missing() => continue,
                    Err(error) => return Err(error.diagnostic()),
                };
                // Source root for the dep: if .jet/ subdir exists use it, else the dep root.
                let src_root = match resolver.checked_directory(Path::new(".jet")) {
                    Ok(directory) => directory.path,
                    Err(error) if error.is_missing() => resolver.root().to_path_buf(),
                    Err(error) => return Err(error.diagnostic()),
                };
                dirs.insert(
                    dep_name.clone(),
                    DependencyDir {
                        manifest_root: resolver.root().to_path_buf(),
                        source_root: src_root,
                    },
                );
            }
            Manifest::DepSpec::Git { .. } => {
                // Git deps are in .jet-build/deps/<name>/ after `jet fetch`.
                let linked = project_root.join(".jet-build").join("deps").join(dep_name);
                match AuthorityResolver::open(&linked) {
                    Err(error) if error.is_missing() => continue,
                    Err(error) => return Err(error.diagnostic()),
                    Ok(resolver) => {
                    let src_root = match resolver.checked_directory(Path::new(".jet")) {
                        Ok(directory) => directory.path,
                        Err(error) if error.is_missing() => resolver.root().to_path_buf(),
                        Err(error) => return Err(error.diagnostic()),
                    };
                    dirs.insert(
                        dep_name.clone(),
                        DependencyDir {
                            manifest_root: resolver.root().to_path_buf(),
                            source_root: src_root,
                        },
                    );
                    }
                }
            }
            Manifest::DepSpec::Registry(_) => {
                // Registry source trees are materialized by `jet fetch` before
                // loading. Keep unresolved manifests out of module search
                // instead of inventing a path or silently using a stale one.
            }
        }
    }
    Ok(dirs)
}

/// Rebuild a project's dependency source dirs (M12.1) and U17 package
/// resolution from its `package.jet`, for callers that only have the project
/// root (e.g. `resolve_import_target`, run after the bundle is loaded). Returns
/// empty maps when there is no manifest (R9 single-file mode); malformed
/// authority is returned to the import caller.
fn project_resolution(
    project_root: &Path,
) -> Result<(HashMap<String, DependencyDir>, PkgResolution), Diagnostic> {
    let resolver = AuthorityResolver::open(project_root).map_err(|error| error.diagnostic())?;
    let checked = match resolver.checked_manifest(Path::new(".")) {
        Ok(checked) => checked,
        Err(error) if error.is_missing() => {
            return Ok((HashMap::new(), PkgResolution::default()));
        }
        Err(error) => return Err(error.diagnostic()),
    };
    let pack_path = checked.file.path.clone();
    let raw = checked.file.text().map_err(|error| error.diagnostic())?;
    resolver
        .revalidate_file(&checked.file)
        .map_err(|error| error.diagnostic())?;
    let mf = Manifest::parse(&pack_path, &raw)?;
    resolver
        .revalidate_file(&checked.file)
        .map_err(|error| error.diagnostic())?;
    let dep_dirs = collect_dep_dirs(&mf, project_root)?;
    Ok((dep_dirs, collect_pkg_resolution(&raw)?))
}

fn auto_derive_default_for_file(
    path: &Path,
    project_root: &Path,
    project_lints_deny: &[String],
    dependency_roots: &HashMap<String, DependencyDir>,
) -> Result<bool, Diagnostic> {
    let dependency = dependency_roots
        .values()
        .filter(|dependency| path.starts_with(&dependency.source_root))
        .max_by_key(|dependency| dependency.source_root.components().count());
    if let Some(dependency) = dependency {
        let package = crate::Package::PackageFacts::load_checked(&dependency.manifest_root)
            .map_err(|error| error.diagnostic())?
            .ok_or_else(|| {
                Diagnostic::error(
                    "E1334",
                    format!(
                        "package manifest `{}` is missing",
                        dependency.manifest_root.display()
                    ),
                    "a dependency source root must have one checked package manifest".to_string(),
                    "restore `package.jet` in the dependency root".to_string(),
                    None,
                )
            })?;
        let deny = package.policy.lints_deny.as_deref().unwrap_or_default();
        return Ok(!jet_foundation::LintPolicy::is_denied(
            deny,
            jet_foundation::LintPolicy::AUTO_DERIVE_LINT.code,
        ));
    }
    if path.starts_with(project_root) {
        Ok(!jet_foundation::LintPolicy::is_denied(
            project_lints_deny,
            jet_foundation::LintPolicy::AUTO_DERIVE_LINT.code,
        ))
    } else {
        Ok(true)
    }
}

fn apply_auto_derive_default(items: &mut [Item], default: bool) {
    for item in items {
        match item {
            Item::Struct(def) => def.auto_derive_default = default,
            Item::Enum(def) => def.auto_derive_default = default,
            Item::CodeModule(module) => {
                if let Some(body) = &mut module.body {
                    apply_auto_derive_default(body, default);
                }
            }
            Item::GenericModule(module) => apply_auto_derive_default(&mut module.body, default),
            _ => {}
        }
    }
}

fn load_file(
    path: &Path,
    display: &str,
    project_root: &Path,
    pkg_dep_dirs: &HashMap<String, DependencyDir>,
    pkg_resolution: &PkgResolution,
    package_policy: &[crate::Policy::PolicyDeclaration],
    package_lints_deny: &[String],
    modules: &mut Vec<LoadedModule>,
    path_to_idx: &mut HashMap<PathBuf, usize>,
    stack: &mut Vec<PathBuf>,
    overlays: &[(&Path, &str)],
    for_check: bool,
    parse_teaching: &mut Vec<Diagnostic>,
    project_parts: &crate::ProjectParts::ProjectPartsReport,
    project_part_failures: &[crate::ProjectParts::ProjectPartScanFailure],
    dependencies: &mut Vec<PathBuf>,
) -> Result<(), LoaderError> {
    let norm = normalize_path(path);
    if !dependencies.contains(&norm) {
        dependencies.push(norm.clone());
    }
    if stack.contains(&norm) {
        let cycle: Vec<String> = stack
            .iter()
            .chain(std::iter::once(&norm))
            .map(|p| relative_display(project_root, p))
            .collect();
        return Err(LoaderError::at(display, "", vec![Diagnostic::error(
            "E0604",
            "these files import each other in a circle".to_string(),
            "Jet loads every imported file before compiling, so imports can't loop".to_string(),
            format!("break the cycle: {}", cycle.join(" → ")),
            None,
        )]));
    }
    if path_to_idx.contains_key(&norm) {
        return Ok(());
    }

    let overlay_source = overlays
        .iter()
        .rev()
        .find(|(candidate, _)| normalize_path(candidate) == norm)
        .map(|(_, text)| (*text).to_string());
    let checked_authority = if overlay_source.is_none() {
        Some(checked_source_file(path, display)?)
    } else {
        None
    };
    let source = overlay_source
        .or_else(|| checked_authority.as_ref().map(|(_, _, source)| source.clone()))
        .expect("one source reader must provide the module text");

    let (toks, lex_diags) = Lexer::lex(&source);
    if !lex_diags.is_empty() {
        return Err(LoaderError::at(display, &source, lex_diags));
    }
    let mut prog = match Parser::parse_for_check(&toks) {
        Ok((p, teaching)) => {
            parse_teaching.extend(teaching);
            p
        }
        Err(diags) => return Err(LoaderError::at(display, &source, diags)),
    };
    let auto_derive_default = auto_derive_default_for_file(
        &norm,
        project_root,
        package_lints_deny,
        pkg_dep_dirs,
    )
    .map_err(|diagnostic| LoaderError::at(display, &source, vec![diagnostic]))?;
    apply_auto_derive_default(&mut prog.items, auto_derive_default);

    let alias = default_module_alias(path);
    stack.push(norm.clone());
    let module_idx = modules.len();
    path_to_idx.insert(norm.clone(), module_idx);

    let imports = std::mem::take(&mut prog.imports);
    for declaration in &mut prog.policy_declarations { declaration.source = display.to_string(); }
    let mut effective_declarations = package_policy.to_vec();
    effective_declarations.extend(prog.policy_declarations.clone());
    for key in crate::Policy::POLICY_RULES.iter().map(|rule| rule.key) {
        let module_chain = effective_declarations.iter().filter(|d| matches!(d.scope, crate::Policy::PolicyScope::Organization | crate::Policy::PolicyScope::Package | crate::Policy::PolicyScope::Module)).cloned().collect::<Vec<_>>();
        if let Err(error) = crate::Policy::resolve(key, module_chain) { return Err(LoaderError::at(display, &source, vec![policy_ladder_diagnostic(key, error)])); }
        let targets = effective_declarations.iter().filter(|d| matches!(d.scope, crate::Policy::PolicyScope::Function | crate::Policy::PolicyScope::Block) && d.key == key).filter_map(|d| d.target).collect::<Vec<_>>();
        for target in targets {
            let chain = effective_declarations.iter().filter(|d| matches!(d.scope, crate::Policy::PolicyScope::Organization | crate::Policy::PolicyScope::Package | crate::Policy::PolicyScope::Module) || (matches!(d.scope, crate::Policy::PolicyScope::Function | crate::Policy::PolicyScope::Block) && d.target == Some(target))).cloned().collect::<Vec<_>>();
            if let Err(error) = crate::Policy::resolve(key, chain) { return Err(LoaderError::at(display, &source, vec![policy_ladder_diagnostic(key, error)])); }
        }
    }
    modules.push(LoadedModule {
        path: path.to_path_buf(),
        display: display.to_string(),
        source: source.clone(),
        alias,
        imports: imports.clone(),
        items: prog.items,
        script_body: prog.script_body,
        block_spans: prog.block_spans,
        web_target_ceiling: prog.web_target_ceiling,
        pub_file: prog.pub_file,
        no_prelude: prog.no_prelude,
        default_target: prog.default_target,
        html_path: prog.html_path.clone(),
        no_alloc_policy: prog.no_alloc_policy,
        policy_declarations: effective_declarations,
        rule_facts: std::mem::take(&mut prog.rule_facts),
    });

    for imp in &imports {
        let is_foreign = is_foreign_namespace_import(imp)
            .map_err(|diagnostic| LoaderError::at(display, &source, vec![diagnostic]))?;
        // S59: C `use` forms use the reserved `c.` root legitimately.
        if is_foreign {
            continue;
        }
        // D-MOD3: `use alias.Item` / `use alias.[A,B]` forms don't load new files;
        // sema resolves them against already-loaded modules (E0609–E0611).
        if matches!(imp.kind, ImportKind::Unqualified { .. }) {
            continue;
        }
        if let Err(d) = check_reserved_import(imp) {
            stack.pop();
            return Err(LoaderError::at(display, &source, vec![d]));
        }
        if core_module_path(imp).is_some() {
            continue;
        }
        let target = match resolve_import(
            imp,
            path,
            project_root,
            pkg_dep_dirs,
            pkg_resolution,
            project_parts,
            project_part_failures,
        ) {
            Ok(p) => p,
            Err(d) => {
                stack.pop();
                return Err(LoaderError::at(display, &source, vec![d]));
            }
        };
        let child_display = relative_display(project_root, &target);
        if let Err(diags) = load_file(
            &target,
            &child_display,
            project_root,
            pkg_dep_dirs,
            pkg_resolution,
            package_policy,
            package_lints_deny,
            modules,
            path_to_idx,
            stack,
            overlays,
            for_check,
            parse_teaching,
            project_parts,
            project_part_failures,
            dependencies,
        ) {
            stack.pop();
            return Err(diags);
        }
    }

    // D-MOD1: resolve `module name;` file declarations — search adjacent to the
    // current file, then load and register as synthetic imports.
    // Collect the metadata we need before borrowing `modules` mutably below.
    #[derive(Clone)]
    struct CmMeta {
        name: String,
        name_span: crate::Diagnostics::Span,
        span: crate::Diagnostics::Span,
        is_pub: bool,
        is_package_pub: bool,
    }
    let code_module_decls: Vec<CmMeta> = modules[module_idx]
        .items
        .iter()
        .filter_map(|item| {
            if let Item::CodeModule(cm) = item {
                if cm.body.is_none() {
                    return Some(CmMeta {
                        name: cm.name.clone(),
                        name_span: cm.name_span,
                        span: cm.span,
                        is_pub: cm.is_pub,
                        is_package_pub: cm.is_package_pub,
                    });
                }
            }
            None
        })
        .collect();

    for cm in code_module_decls {
        let target = match resolve_code_module_file(&cm.name, cm.name_span, path, dependencies) {
            Ok(p) => p,
            Err(d) => {
                stack.pop();
                return Err(LoaderError::at(display, &source, vec![d]));
            }
        };
        let child_display = relative_display(project_root, &target);
        if let Err(diags) = load_file(
            &target,
            &child_display,
            project_root,
            pkg_dep_dirs,
            pkg_resolution,
            package_policy,
            package_lints_deny,
            modules,
            path_to_idx,
            stack,
            overlays,
            for_check,
            parse_teaching,
            project_parts,
            project_part_failures,
            dependencies,
        ) {
            stack.pop();
            return Err(diags);
        }
        // Add a synthetic import so sema can resolve `module_name.func()`.
        // Use the path relative to the importing file (without extension) so
        // that resolve_file_import can find it — for a directory module
        // "text", target is ".../text/module.jet" and we need "text/module".
        let importing_dir = path.parent().unwrap_or(Path::new("."));
        let rel_path = target
            .strip_prefix(importing_dir)
            .map(|p| p.with_extension(""))
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| cm.name.clone());
        let synthetic = ImportDecl {
            kind: ImportKind::File(rel_path, cm.name_span),
            alias: cm.name.clone(),
            alias_span: cm.name_span,
            span: cm.span,
            is_pub: cm.is_pub,
            is_package_pub: cm.is_package_pub,
            inline_version: None,
        };
        modules[module_idx].imports.push(synthetic);
    }

    if let Some((resolver, checked, _)) = checked_authority.as_ref() {
        resolver
            .revalidate_file(checked)
            .map_err(|error| LoaderError::at(display, &source, vec![error.diagnostic()]))?;
    }
    stack.pop();
    Ok(())
}

fn policy_ladder_diagnostic(key: crate::Policy::PolicyKey, error: crate::Policy::PolicyError) -> Diagnostic {
    let span = match error { crate::Policy::PolicyError::ProhibitedScope { span, .. } | crate::Policy::PolicyError::Widening { span, .. } => span, crate::Policy::PolicyError::Conflict { second, .. } => second };
    Diagnostic::error(
        "E0355",
        format!("invalid effective `{}` policy", key.name()),
        "package, module, function, and block declarations share one ladder; inner declarations may not conflict with or widen an outer safety constraint".to_string(),
        "remove the conflict or tighten the inner declaration".to_string(),
        Some(span),
    )
}

/// D-MOD1: find the file for `module name;` — look in the same directory as
/// `importing` for `{name}.jet` then `{name}/module.jet`.
fn resolve_code_module_file(
    name: &str,
    name_span: Span,
    importing: &Path,
    dependencies: &mut Vec<PathBuf>,
) -> Result<PathBuf, Diagnostic> {
    let dir = importing.parent().unwrap_or(Path::new("."));
    let direct = normalize_path(&dir.join(format!("{}.{}", name, Syntax::FILE_EXT)));
    let module_jet = normalize_path(&dir.join(name).join(format!("module.{}", Syntax::FILE_EXT)));
    for candidate in [&direct, &module_jet] {
        if !dependencies.contains(candidate) {
            dependencies.push(candidate.clone());
        }
    }
    let resolver = AuthorityResolver::open(dir).map_err(|error| error.diagnostic())?;
    for relative in [
        PathBuf::from(format!("{name}.{}", Syntax::FILE_EXT)),
        PathBuf::from(name).join(format!("module.{}", Syntax::FILE_EXT)),
    ] {
        match resolver.checked_file(&relative) {
            Ok(file) => {
                resolver.revalidate_file(&file).map_err(|error| error.diagnostic())?;
                return Ok(file.path);
            }
            Err(error) if error.is_missing() => {}
            Err(error) => return Err(error.diagnostic()),
        }
    }
    // E0607: neither search path found
    Err(Diagnostic::error(
        "E0607",
        format!("module `{}` not found", name),
        format!("looked for `{name}.jet` and `{name}/module.jet` next to this file"),
        format!("create `{}.{}` next to this file", name, Syntax::FILE_EXT),
        Some(name_span),
    ))
}

fn resolve_import(
    imp: &ImportDecl,
    importing: &Path,
    project_root: &Path,
    pkg_dep_dirs: &HashMap<String, DependencyDir>,
    pkg_resolution: &PkgResolution,
    project_parts: &crate::ProjectParts::ProjectPartsReport,
    project_part_failures: &[crate::ProjectParts::ProjectPartScanFailure],
) -> Result<PathBuf, Diagnostic> {
    match &imp.kind {
        ImportKind::File(path_str, span) => {
            resolve_file_import(importing, path_str, project_root, *span)
        }
        ImportKind::Module(name, span) => {
            resolve_module_import(
                name,
                project_root,
                pkg_dep_dirs,
                pkg_resolution,
                *span,
                project_parts,
                project_part_failures,
            )
        }
        ImportKind::Unqualified { .. } => {
            // Unqualified imports don't map to a new file — the module is
            // already imported. Resolution is handled by sema (E0609–E0611).
            Err(Diagnostic::error(
                "E0003",
                "unqualified import cannot be resolved as a file".to_string(),
                "use alias.Item imports are resolved by sema against an already-loaded module"
                    .to_string(),
                "ensure the module is imported first: `use module_name as alias;`".to_string(),
                Some(imp.span),
            ))
        }
    }
}

// ── Pure module-name helpers ──────────────────────────────────────────────────
// Canonical implementations live in Syntax (strings-only) and AST (uses
// ImportDecl). These forwarding fns keep existing call-sites working while
// the workspace split proceeds.

pub fn core_module_path(imp: &ImportDecl) -> Option<String> {
    imp.core_module_path()
}

pub use crate::Syntax::{
    core_modules_list, is_known_core_module, is_legacy_std_import, is_ring_module,
    is_ring_module_staged, KNOWN_CORE_MODULES,
};

/// D-JPK-RINGSHIP1=C: where a ring module's implementation comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RingResolution {
    /// A prebuilt artifact staged from the active toolchain object (the hangar).
    Staged(PathBuf),
    /// The compiler-embedded bridge template (`FFI.rs` / `CoreLib.rs`) — the
    /// zero-config fallback that preserves rung-0 magic.
    Embedded,
}

/// Resolve where a ring module (`http`, `regex`, …) is realized from. Prefers the
/// staged hangar artifact when the active toolchain object carries one for this
/// platform; otherwise the compiler-embedded template. One resolution path, two
/// sources, no user-visible difference (D-JPK-RINGSHIP1=C). Ring version =
/// toolchain version by construction: the artifact only exists in a toolchain
/// object, so a staged ring can never skew from the compiler that ships it.
pub fn resolve_ring_module(name: &str) -> RingResolution {
    match crate::Syntax::staged_ring_artifact(name) {
        Some(artifact) => RingResolution::Staged(artifact),
        None => RingResolution::Embedded,
    }
}

/// E1241 — a ring library is listed in `deps: { core.<ring> }` expecting a
/// staged hangar object, but the active toolchain object carries no prebuilt
/// artifact for this platform. Under D-JPK-RINGSHIP1=C the loader falls back to
/// the embedded template (this diagnostic is the informational form); under
/// RINGSHIP1=B, where ring libs are independently versioned source packages, it
/// is a hard error.
pub fn e1241_ring_platform_miss(ring: &str) -> Diagnostic {
    Diagnostic::error(
        "E1241",
        format!("the staged `core.{ring}` artifact is missing for this platform"),
        "the active toolchain object carries prebuilt ring artifacts, but none for \
         `core.{ring}` on this platform."
            .replace("{ring}", ring),
        "the build falls back to the compiler-embedded `core.{ring}`; to ship the staged \
         artifact, realize a toolchain object built for this platform (`jet update jet`)."
            .replace("{ring}", ring),
        None,
    )
}

fn check_reserved_import(imp: &ImportDecl) -> Result<(), Diagnostic> {
    if let Some(module) = core_module_path(imp) {
        if !is_known_core_module(&module) {
            let span = match &imp.kind {
                ImportKind::Module(_, span) => *span,
                ImportKind::File(_, _) | ImportKind::Unqualified { .. } => imp.span,
            };
            return Err(Diagnostic::error(
                "E1001",
                format!("there is no core module `{}`", module),
                "`core` is compiler-known in M10, and only the frozen core modules exist"
                    .to_string(),
                format!("use one of: {}", core_modules_list()),
                Some(span),
            ));
        }
        return Ok(());
    }

    let alias = import_alias(imp);
    if Syntax::FIRST_PARTY_RESERVED.contains(&alias.as_str()) {
        return Err(Diagnostic::error(
            "E1002",
            format!("`{}` is reserved for first-party or foreign packages", alias),
            "`core`, `jet`, first-party ring names, and foreign-language roots can't be used for local modules"
                .to_string(),
            format!(
                "rename the module or use it with `{} other_name`",
                Syntax::KW_AS
            ),
            Some(imp.alias_span),
        ));
    }
    if let ImportKind::Module(name, span) = &imp.kind {
        if let Some(ring) = name.strip_prefix("jet.") {
            if is_ring_module(ring) {
                return Err(Diagnostic::error(
                    "E0341",
                    format!("`use jet.{ring}` is the old first-party library spelling"),
                    "first-party libraries moved to the `core.*` namespace (D-CORENS1)".to_string(),
                    format!("write `use core.{ring}` instead"),
                    Some(*span),
                ));
            }
        }
        let root = name.split('.').next().unwrap_or(name);
        if Syntax::FIRST_PARTY_RESERVED.contains(&root) {
            return Err(Diagnostic::error(
                "E1002",
                format!("`{}` is reserved for first-party or foreign packages", root),
                "`core`, `jet`, first-party ring names, and foreign-language roots can't be used for local modules"
                    .to_string(),
                "choose a different module name".to_string(),
                Some(*span),
            ));
        }
    }
    Ok(())
}

fn resolve_file_import(
    importing: &Path,
    path_str: &str,
    project_root: &Path,
    span: Span,
) -> Result<PathBuf, Diagnostic> {
    if path_str.contains("..") {
        return Err(e0602(span));
    }
    let base = importing.parent().unwrap_or(Path::new("."));
    let mut resolved = base.to_path_buf();
    for part in path_str.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        resolved.push(part);
    }
    resolved.set_extension(Syntax::FILE_EXT);
    let resolved = normalize_path(&resolved);
    let resolver = AuthorityResolver::open(project_root).map_err(|error| error.diagnostic())?;
    let checked = resolver.checked_file(&resolved).map_err(|error| {
        Diagnostic::error(
            "E0603",
            format!("can't find the file `{}`", path_str),
            "a file import path must point at an existing `.jet` file next to this file's tree"
                .to_string(),
            format!(
                "create `{}.{}`, or fix the path in `{} \"{}\"` ({error})",
                path_str,
                Syntax::FILE_EXT,
                Syntax::KW_USE,
                path_str
            ),
            Some(span),
        )
    })?;
    resolver.revalidate_file(&checked).map_err(|error| {
        Diagnostic::error(
            "E0603",
            format!("can't find the file `{}`", path_str),
            "a file import path must point at an existing `.jet` file next to this file's tree"
                .to_string(),
            format!(
                "restore the checked file and fix the path in `{} \"{}\"` ({error})",
                Syntax::KW_USE,
                path_str
            ),
            Some(span),
        )
    })?;
    Ok(checked.path)
}

fn resolve_module_import(
    name: &str,
    project_root: &Path,
    pkg_dep_dirs: &HashMap<String, DependencyDir>,
    pkg_resolution: &PkgResolution,
    span: Span,
    project_parts: &crate::ProjectParts::ProjectPartsReport,
    project_part_failures: &[crate::ProjectParts::ProjectPartScanFailure],
) -> Result<PathBuf, Diagnostic> {
    if let Some(local_name) = name.strip_prefix(Syntax::PROJECT_IMPORT_PREFIX) {
        let matches = project_parts.named(local_name);
        let scan_failure = project_part_failures
            .iter()
            .find(|failure| failure.module_names.iter().any(|name| name == local_name));
        return match matches.as_slice() {
            [] => match scan_failure {
                Some(failure) => Err(failure.diagnostic(local_name, project_root, span)),
                None => Err(Diagnostic::error(
                    "E0603",
                    format!(
                        "can't find a project module named `{}{}`",
                        Syntax::PROJECT_IMPORT_PREFIX,
                        local_name,
                    ),
                    "project-local imports resolve declared module names, not filenames"
                        .to_string(),
                    format!("declare `module {local_name} {{ ... }}` under this project"),
                    Some(span),
                )),
            },
            [part] => Ok(normalize_path(&part.path)),
            _ => Err(project_parts
                .conflicts
                .iter()
                .find(|conflict| conflict.name == local_name)
                .expect("duplicate project parts have a conflict record")
                .diagnostic(project_root, Some(span))),
        };
    }
    // M12.1: check package dep dirs first.
    // `import words;` where "words" is a dep name → look in the dep's source root.
    let first_segment = name.split('.').next().unwrap_or(name);
    if let Some(dependency) = pkg_dep_dirs.get(first_segment) {
        let dep_root = &dependency.source_root;
        // Search within the dep's source tree for the module.
        let dep_matches = find_module_files(name, dep_root)?;
        if !dep_matches.is_empty() {
            return Ok(dep_matches[0].clone());
        }
        // If the dep root itself has the dep name as the top-level module,
        // anchor the package authority on the canonical run entry.
        if name == first_segment {
            let resolver = AuthorityResolver::open(dep_root)
                .map_err(|error| error.diagnostic())?;
            match resolver.checked_file(Path::new(Syntax::DEFAULT_ENTRY_FILE)) {
                Ok(run_jet) => {
                    resolver
                        .revalidate_file(&run_jet)
                        .map_err(|error| error.diagnostic())?;
                    return Ok(run_jet.path);
                }
                Err(error) if error.is_missing() => {}
                Err(error) => return Err(error.diagnostic()),
            }
        }
    }

    // U17: a `library` package is brought into code with the ordinary
    // `use <pkg>` form — once realized, its staged source in the shared hangar
    // is just an extra module search root. The hangar is authoritative for the
    // realized kind (empty `bin` = library, U10).
    if !pkg_resolution.is_empty() {
        // Realized library → resolve through its staged source tree.
        if let Some(staged) = pkg_resolution.realized_libs.get(first_segment) {
            let lib_matches = find_module_files(name, staged)?;
            if !lib_matches.is_empty() {
                return Ok(lib_matches[0].clone());
            }
            // Realized, but the named submodule isn't in the staged tree — fall
            // through to the normal not-found report below.
        }
        // Realized executable named in `use` → executables go on PATH (E0982).
        if pkg_resolution.realized_exes.contains(first_segment) {
            return Err(e0982_executable_use(first_segment, span));
        }
        // Declared as a dependency but not realized yet → run `jetpack build`
        // (E0983), rather than a generic unknown-module error.
        if pkg_resolution.declared_deps.contains(first_segment)
            && find_module_files(name, project_root)?.is_empty()
            && !pkg_dep_dirs.contains_key(first_segment)
        {
            return Err(e0983_unrealized_library(first_segment, span));
        }
    }

    let matches = find_module_files(name, project_root)?;
    match matches.len() {
        0 => Err(Diagnostic::error(
            "E0603",
            format!("can't find a module named `{}`", name),
            format!(
                "search from the project root for `{}.{}`, or `{}/{}/{}.{}` / `{}`",
                name,
                Syntax::FILE_EXT,
                name,
                name,
                name,
                Syntax::FILE_EXT,
                Syntax::DEFAULT_ENTRY_FILE
            ),
            format!(
                "add `{}.{}` under this project, or fix the `{}` name",
                name,
                Syntax::FILE_EXT,
                Syntax::KW_USE
            ),
            Some(span),
        )),
        1 => Ok(matches[0].clone()),
        _ => {
            let list = matches
                .iter()
                .map(|p| relative_display(project_root, p))
                .collect::<Vec<_>>()
                .join(", ");
            Err(Diagnostic::error(
                "E0606",
                format!("the module name `{}` matches more than one file", name),
                "module imports must name exactly one file under the project root".to_string(),
                format!("pick one file and use a file import instead: {}", list),
                Some(span),
            ))
        }
    }
}

fn find_module_files(name: &str, project_root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let resolver = AuthorityResolver::open(project_root).map_err(|error| error.diagnostic())?;
    let files = resolver
        .discover_source_files()
        .map_err(|error| error.diagnostic())?;
    let direct_name = format!("{name}.{}", Syntax::FILE_EXT);
    let matches = files
        .into_iter()
        .filter(|file| {
            let Some(file_name) = file.relative.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            if file_name == direct_name {
                return true;
            }
            file_name == Syntax::DEFAULT_ENTRY_FILE
                && file
                    .relative
                    .parent()
                    .and_then(|parent| parent.file_name())
                    .and_then(|name| name.to_str())
                    == Some(name)
        })
        .map(|file| file.path)
        .collect::<Vec<_>>();
    let mut found = matches;
    found.sort();
    found.dedup();
    Ok(found)
}

/// File stems become generated Rust `mod __jet_<alias>` names, so the alias must be a
/// valid identifier: non-alphanumeric characters map to `_`, and a leading
/// digit gets a `_` prefix.
fn default_module_alias(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    sanitize_alias(stem)
}

fn sanitize_alias(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("module");
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub fn resolve_import_target(
    bundle: &ProgramBundle,
    importing_idx: usize,
    imp: &ImportDecl,
) -> Result<usize, Diagnostic> {
    if core_module_path(imp).is_some() {
        return Err(Diagnostic::error(
            "E1001",
            "core modules do not resolve to files".to_string(),
            "`core` is provided by the compiler in M10".to_string(),
            "handle this import as a compiler-known module".to_string(),
            Some(imp.span),
        ));
    }
    let importing = &bundle.modules[importing_idx];
    // Rebuild the same dep-source dirs (M12.1) and U17 package resolution
    // (realized library staging dirs) the loader used, so a `use <pkg>`
    // re-resolves to the exact file already pulled into the bundle.
    let (pkg_dep_dirs, pkg_resolution) = project_resolution(&bundle.project_root)?;
    let (project_parts, project_part_failures) =
        crate::ProjectParts::scan_with_diagnostics(&bundle.project_root, &[]);
    if let Some(failure) = project_part_failures.iter().find(|failure| failure.authority) {
        return Err(failure.problem.clone());
    }
    let target_path = match resolve_import(
        imp,
        &importing.path,
        &bundle.project_root,
        &pkg_dep_dirs,
        &pkg_resolution,
        &project_parts,
        &project_part_failures,
    ) {
        Ok(p) => normalize_path(&p),
        Err(d) => return Err(d),
    };
    for (i, m) in bundle.modules.iter().enumerate() {
        if normalize_path(&m.path) == target_path {
            return Ok(i);
        }
    }
    Err(Diagnostic::error(
        "E0603",
        "imported file isn't part of this program".to_string(),
        "the loader should have pulled in every imported file already".to_string(),
        "report this as a compiler bug".to_string(),
        Some(imp.span),
    ))
}

pub fn import_alias(imp: &ImportDecl) -> String {
    imp.import_alias()
}

/// E0982 (U17): `use <pkg>` named a package declared `executable` in the
/// manifest. Executables go on PATH (you run them), not into code.
fn e0982_executable_use(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0982",
        format!("`{}` is an executable package, so it can't be used with `{}`", name, Syntax::KW_USE),
        "an `executable` package installs a binary on your PATH — you run it, you don't import its code; only a `library` package is brought into code with `use` (U17)"
            .to_string(),
        format!(
            "remove `{} {};` and run the `{}` binary instead, or change `{}` to `library` in `{}` if you meant to import its code",
            Syntax::KW_USE, name, name, name, Syntax::PACKAGE_FILE
        ),
        Some(span),
    )
}

/// E0983 (U17): `use <pkg>` named a `library` package that the manifest
/// declares but that hasn't been realized (no staged source in the hangar).
fn e0983_unrealized_library(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0983",
        format!("the library package `{}` hasn't been built yet", name),
        "a `library` package must be realized — its source staged in the shared store (hangar) — before `use` can find it (U17)"
            .to_string(),
        format!(
            "run `jetpack build` to realize `{}`, then `{} {};` will resolve it",
            name, Syntax::KW_USE, name
        ),
        Some(span),
    )
}

fn e0602(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0602",
        "this import path escapes the project".to_string(),
        "file imports stay inside the folder that contains the entry `.jet` file — `..` isn't allowed"
            .to_string(),
        format!(
            "use a path without `..`, or move the file inside the project tree"
        ),
        Some(span),
    )
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Compare existing paths by physical identity so a symlink cannot widen a
/// workspace or project boundary.
pub fn is_physically_within(root: &Path, path: &Path) -> bool {
    let Ok(root) = AuthorityResolver::open(root) else {
        return false;
    };
    let Ok(path) = AuthorityResolver::open(path) else {
        return false;
    };
    path.root().starts_with(root.root())
}

#[cfg(test)]
mod stale_manifest_name_tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "loader-stale-manifest-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn finds_pack_jet() {
        let dir = tempdir("pack");
        fs::write(dir.join("pack.jet"), "").unwrap();
        let (found_dir, name) = find_stale_manifest_name(&dir).expect("should find pack.jet");
        assert_eq!(found_dir, dir);
        assert_eq!(name, "pack.jet");
    }

    #[test]
    fn finds_payload_jet() {
        let dir = tempdir("payload");
        fs::write(dir.join("payload.jet"), "").unwrap();
        let (_, name) = find_stale_manifest_name(&dir).expect("should find payload.jet");
        assert_eq!(name, "payload.jet");
    }

    #[test]
    fn finds_jet_toml() {
        let dir = tempdir("jettoml");
        fs::write(dir.join("jet.toml"), "").unwrap();
        let (_, name) = find_stale_manifest_name(&dir).expect("should find jet.toml");
        assert_eq!(name, "jet.toml");
    }

    #[test]
    fn jetpack_toml_is_not_stale() {
        // jetpack.toml is a different, still-live file (repo metadata) —
        // must never be mistaken for a retired manifest name.
        let dir = tempdir("jetpacktoml");
        fs::write(dir.join("jetpack.toml"), "").unwrap();
        assert_eq!(find_stale_manifest_name(&dir), None);
    }

    #[test]
    fn pkg_jet_present_means_nothing_stale() {
        let dir = tempdir("both");
        fs::write(dir.join("pkg.jet"), "").unwrap();
        fs::write(dir.join("pack.jet"), "").unwrap();
        // pkg.jet exists right here — the walk stops with nothing to report,
        // even though a stale name also happens to sit alongside it.
        assert_eq!(find_stale_manifest_name(&dir), None);
    }

    #[test]
    fn no_manifest_at_all_is_none() {
        let dir = tempdir("empty");
        assert_eq!(find_stale_manifest_name(&dir), None);
    }

    #[test]
    fn workspace_boundary_blocks_outer_stale_manifest_lookup() {
        let outer = tempdir("workspace-boundary");
        let inner = outer.join("child");
        let source = inner.join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(outer.join("pack.jet"), "").unwrap();
        fs::write(
            inner.join("authority.jet"),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();

        assert_eq!(find_stale_manifest_name(&source), None);
        assert_eq!(find_manifest_root(&source), None);
    }

    #[test]
    fn ambiguous_workspace_boundary_blocks_outer_package_authority() {
        let outer = tempdir("ambiguous-workspace-authority");
        let inner = outer.join("child");
        let source = inner.join("src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            outer.join(Syntax::PACKAGE_FILE),
            "name: \"outer\"\nversion: \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            inner.join("a.jet"),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();
        fs::write(
            inner.join("b.jet"),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();

        let diagnostic = find_manifest_root_checked(&source)
            .expect_err("ambiguous inner workspace must not expose an outer Package");
        assert_eq!(diagnostic.code, "E1239");
        assert_eq!(find_manifest_root(&source), None);
    }

    #[cfg(unix)]
    #[test]
    fn file_import_rejects_physical_symlink_escape_as_e0603() {
        use std::os::unix::fs::symlink;

        let root = tempdir("file-import-symlink");
        let outside = tempdir("file-import-symlink-outside");
        let entry = root.join("run.jet");
        fs::write(&entry, "use \"./escape\" as escape\nfn run() {}\n").unwrap();
        fs::write(outside.join("secret.jet"), "pub fn fixture() {}\n").unwrap();
        symlink(outside.join("secret.jet"), root.join("escape.jet")).unwrap();

        let diagnostics = load_entry(entry.to_str().unwrap()).unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0603" && diagnostic.what.contains("./escape")
        }));
    }

    #[test]
    fn explicit_project_import_keeps_internal_module_available() {
        let dir = tempdir("internal-module");
        let entry = dir.join("main.jet");
        fs::write(&entry, "use project._bench as bench\nfn run() { bench.fixture() }\n").unwrap();
        fs::write(
            dir.join("arbitrary.jet"),
            "module _bench { }\npub fn fixture() {}\n",
        )
        .unwrap();

        let (diagnostics, bundle) = crate::Driver::check_file(entry.to_str().unwrap(), None, false);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let bundle = bundle.unwrap();
        assert!(bundle.modules.iter().any(|module| module.path == dir.join("arbitrary.jet")));
    }

    #[test]
    fn explicit_project_import_propagates_invalid_target_diagnostics() {
        for (tag, broken) in [
            ("lex", "module _bench { }\n§\n"),
            ("parse", "module _bench { }\nfn broken(\n"),
        ] {
            let dir = tempdir(&format!("internal-module-{tag}"));
            let entry = dir.join("main.jet");
            fs::write(&entry, "use project._bench\nfn run() {}\n").unwrap();
            fs::write(dir.join("broken.jet"), broken).unwrap();

            let diagnostics = load_entry(entry.to_str().unwrap()).unwrap_err();
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "E0603"
                        && diagnostic.what.contains("project module `project._bench`")
                        && diagnostic.what.contains("broken.jet")
                        && diagnostic.span.is_some()
                }),
                "{diagnostics:#?}"
            );
        }
    }

    #[test]
    fn unrelated_invalid_file_does_not_mask_missing_project_module() {
        let dir = tempdir("internal-module-unrelated-invalid");
        let entry = dir.join("main.jet");
        fs::write(&entry, "use project._bench\nfn run() {}\n").unwrap();
        fs::write(dir.join("broken.jet"), "module _other { }\n§\n").unwrap();

        let diagnostics = load_entry(entry.to_str().unwrap()).unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0603"
                && diagnostic.what == "can't find a project module named `project._bench`"
        }));
    }

    #[test]
    fn explicit_project_import_rejects_duplicate_internal_declarations() {
        let dir = tempdir("internal-module-conflict");
        let entry = dir.join("main.jet");
        fs::write(&entry, "use project._bench as bench\nfn run() {}\n").unwrap();
        fs::write(dir.join("a.jet"), "module _bench { }\n").unwrap();
        fs::write(dir.join("b.jet"), "module _bench { }\n").unwrap();

        let diagnostics = load_entry(entry.to_str().unwrap()).unwrap_err();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0606" && diagnostic.span.is_some()));
    }

    #[test]
    fn duplicate_internal_declarations_fail_without_an_explicit_import() {
        let dir = tempdir("internal-module-unimported-conflict");
        let entry = dir.join("main.jet");
        fs::write(&entry, "fn run() {}\n").unwrap();
        fs::write(dir.join("a.jet"), "module _bench { }\n").unwrap();
        fs::write(dir.join("b.jet"), "module _bench { }\n").unwrap();

        let diagnostics = load_entry(entry.to_str().unwrap()).unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0606"
                && diagnostic.span.is_none()
                && diagnostic.fix.contains("a.jet, b.jet")
        }));
    }

    #[test]
    fn single_file_mode_ignores_unrelated_sibling_module_conflicts() {
        let dir = tempdir("single-file-unrelated-conflict");
        let entry = dir.join("fixture.jet");
        fs::write(&entry, "fn run() {}\n").unwrap();
        fs::write(dir.join("a.jet"), "module _bench { }\n").unwrap();
        fs::write(dir.join("b.jet"), "module _bench { }\n").unwrap();

        load_entry(entry.to_str().unwrap()).expect("single-file mode ignores siblings");
    }

    #[test]
    fn explicit_project_import_uses_overlay_declarations() {
        let dir = tempdir("internal-module-overlay");
        let entry = dir.join("main.jet");
        let part = dir.join("part.jet");
        fs::write(&entry, "use project._bench\nfn run() {}\n").unwrap();
        fs::write(&part, "module _other { }\n").unwrap();

        let bundle = load_entry_with_overlay(
            entry.to_str().unwrap(),
            Some((&part, "module _bench { }\n")),
            false,
        )
        .unwrap();
        assert!(bundle.modules.iter().any(|module| module.path == part));
    }

    #[test]
    fn module_directory_uses_canonical_run_entry_not_main() {
        let dir = tempdir("module-directory-entry");
        let module_dir = dir.join("tool");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(module_dir.join("main.jet"), "pub fn run() {}").unwrap();
        assert!(find_module_files("tool", &dir).unwrap().is_empty());

        let run = module_dir.join(Syntax::DEFAULT_ENTRY_FILE);
        fs::write(&run, "pub fn run() {}\n").unwrap();
        assert_eq!(
            find_module_files("tool", &dir).unwrap(),
            vec![normalize_path(&run)]
        );
    }

    #[test]
    fn authority_name_uses_run_and_rejects_main_fallback() {
        let dir = tempdir("authority-entry-name");
        let main = dir.join(Syntax::LEGACY_ENTRY_FILE);
        fs::write(&main, "fn run() {}\n").unwrap();
        assert_eq!(authority_name_for_entry(&main).unwrap(), "app");

        let run = dir.join(Syntax::DEFAULT_ENTRY_FILE);
        fs::write(&run, "fn run() {}\n").unwrap();
        assert_eq!(authority_name_for_entry(&main).unwrap(), "run");
    }

    #[test]
    fn message_names_e1226_and_both_filenames() {
        let dir = tempdir("message");
        fs::write(dir.join("pack.jet"), "").unwrap();
        let msg = stale_manifest_name_message(&dir).expect("message");
        assert!(msg.contains("E1226"));
        assert!(msg.contains("pack.jet"));
        assert!(msg.contains("pkg.jet"));
    }
}
