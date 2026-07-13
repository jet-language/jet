//! Multi-file program loading (M6 phase 3, S16; M12 package support).
//!
//! Resolves the import graph from an entry `.jet` file, detects cycles and
//! ambiguous module names, and returns a `ProgramBundle` for sema/codegen.
//! When a `pkg.jet` is found in the project root (walked upward from entry),
//! validates the manifest and wires package dep paths into module search (M12.1).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Lexer;
use crate::Manifest;
use crate::Parser;
use crate::Syntax;
use crate::AST::{ImportDecl, ImportKind, Item, LoadedModule, ProgramBundle};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

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

/// Build the U17 package resolution from a project's `pkg.jet` text and the
/// shared hangar store.
///
/// Reading the hangar is a *pure lookup*: the compiler never realizes on demand
/// (that is `jetpack build`'s job). This keeps `jet build`/`run` offline and
/// deterministic, exactly like the existing pre-fetched-dependency flow
/// (`collect_dep_dirs` only links deps already present on disk; `jet fetch` is
/// the separate realize step). So a declared-but-unbuilt library is a friendly
/// "run `jetpack build`" (E0983), never a silent network fetch.
fn collect_pkg_resolution(raw: &str) -> PkgResolution {
    let mut declared_deps = HashSet::new();
    if let Ok(pm) = crate::PackageManifest::parse(raw) {
        for dep in &pm.deps {
            // S59/D-CFFI2: a `c@…` native-library dep is a link dep, not a Jet
            // package — it must not shadow `use <pkg>` resolution (e.g. a dep
            // named `c`). Skip it here; CFFI.rs reads it for link flags.
            if matches!(
                dep.source,
                crate::PackageManifest::DepSource::CLib { .. }
            ) {
                continue;
            }
            declared_deps.insert(dep.name.clone());
        }
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

    PkgResolution {
        realized_libs,
        realized_exes,
        declared_deps,
    }
}

pub fn load_entry(entry_path: &str) -> Result<ProgramBundle, Vec<Diagnostic>> {
    load_entry_with_overlay(entry_path, None, false)
}

/// Load a program, optionally substituting in-memory source for one file
/// (LSP unsaved buffer for the document being edited).
pub fn load_entry_with_overlay(
    entry_path: &str,
    overlay: Option<(&Path, &str)>,
    for_check: bool,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
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
    load_entry_with_overlays_mode(entry_path, overlays, for_check, false)
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
    load_entry_with_overlays_mode(entry_path, overlays, for_check, true)
}

fn load_entry_with_overlays_mode(
    entry_path: &str,
    overlays: &[(&Path, &str)],
    for_check: bool,
    load_adjacent_unqualified: bool,
) -> Result<ProgramBundle, Vec<Diagnostic>> {
    let entry = PathBuf::from(entry_path);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let entry_abs = if entry.is_absolute() {
        entry
    } else {
        cwd.join(&entry)
    };
    let entry_abs = normalize_path(&entry_abs);

    // M12.1: walk upward from the entry file's directory to find pkg.jet.
    // If found, use that directory as project_root and validate the manifest.
    // If none found, fall back to the entry file's directory (R9 — single-file mode).
    let entry_dir = entry_abs
        .parent()
        .map(normalize_path)
        .unwrap_or_else(|| cwd.clone());
    let mut layer_ceiling = None;
    // U11 (D-JPK-SCRIPTDEP1=A): L0203 lints for inline `use pkg#version;`
    // deps found in the manifest-less branch below, merged into
    // `parse_teaching` once that's declared.
    let mut inline_dep_lints: Vec<Diagnostic> = Vec::new();
    let (project_root, pkg_dep_dirs, pkg_resolution) = if let Some(manifest_dir) =
        find_manifest_root(&entry_dir)
    {
        // Found a pkg.jet — validate it and collect dep source paths.
        let pack_path = manifest_dir.join(Syntax::PAYLOAD_FILE);
        let raw = fs::read_to_string(&pack_path).unwrap_or_default();
        match Manifest::parse(&pack_path, &raw) {
            Err(d) => return Err(vec![d]),
            Ok(mf) => {
                layer_ceiling = mf.package.layer;
                // Check toolchain constraint (E1208).
                if let Err(d) = Manifest::check_toolchain(&mf, &pack_path.display().to_string()) {
                    return Err(vec![d]);
                }

                // Check the `edition:` field (E2001, D-REL3): a manifest may not
                // ask for an edition this toolchain doesn't ship.
                if let Err(d) =
                    Manifest::check_edition_support(&mf, &pack_path.display().to_string())
                {
                    return Err(vec![d]);
                }

                // If there are deps, check lock staleness (E1202) and
                // dry-resolve path dep graph to catch version conflicts (E1201).
                if !mf.dependencies.is_empty() {
                    // E1202: lock must exist and include all manifest deps.
                    let lock_path = manifest_dir.join(Syntax::UNIFIED_LOCK_FILE);
                    if lock_path.is_file() {
                        let lock_raw = fs::read_to_string(&lock_path).unwrap_or_default();
                        if let Ok(lock) = crate::Lock::parse(&lock_raw) {
                            if let Err(d) = crate::Lock::verify_lock_matches_manifest(
                                &lock,
                                &mf,
                                &lock_path.display().to_string(),
                            ) {
                                return Err(vec![d]);
                            }
                        } else {
                            return Err(vec![crate::Lock::e1202(&lock_path.display().to_string())]);
                        }
                    }
                    // E1201: dry-resolve path deps for package name conflicts.
                    if let Err(d) = dry_resolve_path_deps(&mf, &manifest_dir) {
                        return Err(vec![d]);
                    }
                }

                // E1212/E1213: each packages: entry must have exactly one
                // module declaration in the source tree (U10 Chunk 3).
                if let Ok(pm) = crate::PackageManifest::parse(&raw) {
                    for pkg in &pm.packages {
                        match crate::PackageManifest::discover_module_in(
                            &manifest_dir,
                            &pkg.name,
                        ) {
                            Ok(_) => {}
                            Err(crate::PackageManifest::DiscoveryError::NotFound {
                                name,
                            }) => {
                                return Err(vec![Manifest::e1212(
                                    &pack_path.display().to_string(),
                                    &name,
                                )]);
                            }
                            Err(crate::PackageManifest::DiscoveryError::Ambiguous {
                                name,
                                paths,
                            }) => {
                                return Err(vec![Manifest::e1213(
                                    &pack_path.display().to_string(),
                                    &name,
                                    &paths,
                                )]);
                            }
                        }
                    }
                }

                // Collect package dep source directories for module search.
                let dep_dirs = collect_dep_dirs(&mf, &manifest_dir);
                // U17: declared package kinds + realized library staging dirs.
                let resolution = collect_pkg_resolution(&raw);
                (manifest_dir, dep_dirs, resolution)
            }
        }
    } else {
        // R9: no manifest — single-file mode, project root is the entry dir.
        //
        // U11 (D-JPK-SCRIPTDEP1=A): a manifest-less entry may open with
        // inline `use pkg#version;` deps. Parse just the entry file to
        // collect them (load_file below reparses it as part of the normal
        // module-graph walk — a small duplicate parse keeps this pass
        // self-contained) and resolve each one up front, so the ordinary
        // `Module` import resolution (`resolve_module_import`) finds them in
        // `realized_libs` exactly like a hangar-realized `library` (U17).
        let mut resolution = PkgResolution::default();
        let raw = fs::read_to_string(&entry_abs).unwrap_or_default();
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
        (entry_dir, HashMap::new(), resolution)
    };

    let mut modules = Vec::new();
    let mut path_to_idx: HashMap<PathBuf, usize> = HashMap::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    let mut parse_teaching = inline_dep_lints;

    load_file(
        &entry_abs,
        entry_path,
        &project_root,
        &pkg_dep_dirs,
        &pkg_resolution,
        &mut modules,
        &mut path_to_idx,
        &mut stack,
        overlays,
        for_check,
        &mut parse_teaching,
    )?;

    if load_adjacent_unqualified {
        let aliases: Vec<(String, crate::Diagnostics::Span)> = modules[0]
            .imports
            .iter()
            .filter_map(|import| match &import.kind {
                ImportKind::Unqualified { module_alias, module_alias_span, .. } => {
                    Some((module_alias.clone(), *module_alias_span))
                }
                _ => None,
            })
            .collect();
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
            if target.is_file() || staged {
                let display = relative_display(&project_root, &target);
                load_file(
                    &target,
                    &display,
                    &project_root,
                    &pkg_dep_dirs,
                    &pkg_resolution,
                    &mut modules,
                    &mut path_to_idx,
                    &mut stack,
                    overlays,
                    for_check,
                    &mut parse_teaching,
                )?;
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
    let mut import_targets = HashMap::new();
    for module_idx in 0..modules.len() {
        let (module_path, imports) = {
            let m = &modules[module_idx];
            (m.path.clone(), m.imports.clone())
        };
        for imp in &imports {
            if matches!(imp.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            if crate::Foreign::is_active_namespace_import(imp) || crate::CFFI::is_c_import(imp) {
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
            ) {
                let norm = normalize_path(&target_path);
                if let Some(&target_idx) = path_to_idx.get(&norm) {
                    import_targets.insert((module_idx, imp.span), target_idx);
                }
            }
        }
    }

    // D-EFFBUDGET1: dependency name → resolved source root, for both `deps:`
    // entries and hangar-realized `use <pkg>` libraries (U17).
    let mut dep_roots = pkg_dep_dirs.clone();
    for (name, dir) in &pkg_resolution.realized_libs {
        dep_roots.entry(name.clone()).or_insert_with(|| dir.clone());
    }

    let mut bundle = ProgramBundle {
        entry: entry_idx,
        project_root,
        modules,
        parse_teaching,
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets,
        layer_ceiling,
        inferred_layer: Syntax::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots,
        // D-OSTARGET2=B: default to the host OS; the driver overrides this from
        // `--target=<triple>` before sema runs (LSP/tests keep the host bucket).
        active_os: Syntax::OsTarget::host(),
    };
    // S59 (E2-M14): fold every `#Extern`/`#Bindgen module c.<lib>` into merged
    // synthetic modules and resolve C `use` forms before sema sees the tree.
    crate::Foreign::assemble_active_namespaces(&mut bundle)?;
    bundle.cffi = crate::CFFI::assemble(&mut bundle)?;
    Ok(bundle)
}

/// Walk upward from `start` to find the nearest directory containing `pkg.jet`.
pub fn find_manifest_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(Syntax::PAYLOAD_FILE).is_file() {
            return Some(dir);
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => return None,
        }
    }
}

/// D-JPK-FILENAME2=B (A2): walk upward the same way [`find_manifest_root`]
/// does, but look for a *retired* manifest filename instead of `pkg.jet`.
/// Stops (returns `None`) the moment a directory has `pkg.jet` — nothing
/// stale to report once the real manifest is found. Used to upgrade a plain
/// "no pkg.jet found" message into the E1226 teaching diagnostic when the
/// user's project still carries an old filename.
pub fn find_stale_manifest_name(start: &Path) -> Option<(PathBuf, &'static str)> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(Syntax::PAYLOAD_FILE).is_file() {
            return None;
        }
        for name in Syntax::STALE_MANIFEST_NAMES {
            if dir.join(name).is_file() {
                return Some((dir, name));
            }
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => return None,
        }
    }
}

/// Render the E1226 `old-manifest-filename` teaching diagnostic for `dir`
/// carrying the retired manifest name `stale` — or `None` when `start`
/// carries no retired name (the caller keeps its own "no pkg.jet found"
/// message in that case).
pub fn stale_manifest_name_message(start: &Path) -> Option<String> {
    let (dir, stale) = find_stale_manifest_name(start)?;
    Some(format!(
        "Error [E1226]: `{stale}` is not the package manifest name — Jet reads `pkg.jet`\n \
         Why: the manifest filename is frozen to one spelling (D-JPK-FILES/D-JPK-FILENAME2) so \
         tooling, docs, and every worked example never have to guess which file to read\n \
         Fix: rename `{}` to `{}`\n",
        dir.join(stale).display(),
        dir.join(Syntax::PAYLOAD_FILE).display(),
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
        let Some(Ok(dep_mf)) = Manifest::load(&dep_path) else {
            continue; // missing manifest is caught later; not an E1201
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

/// Collect the source directories for each dependency from a manifest.
/// Returns a map of dep alias → source root (path dep directory or `.jet-build/deps/<name>`).
fn collect_dep_dirs(mf: &Manifest::Manifest, project_root: &Path) -> HashMap<String, PathBuf> {
    let mut dirs = HashMap::new();
    for (dep_name, spec) in &mf.dependencies {
        match spec {
            Manifest::DepSpec::Path { path } => {
                let abs = normalize_path(&project_root.join(path));
                // Source root for the dep: if .jet/ subdir exists use it, else the dep root.
                let src_root = if abs.join(".jet").is_dir() {
                    abs.join(".jet")
                } else {
                    abs
                };
                dirs.insert(dep_name.clone(), src_root);
            }
            Manifest::DepSpec::Git { .. } => {
                // Git deps are in .jet-build/deps/<name>/ after `jet fetch`.
                let linked = project_root.join(".jet-build").join("deps").join(dep_name);
                if linked.is_dir() {
                    let src_root = if linked.join(".jet").is_dir() {
                        linked.join(".jet")
                    } else {
                        linked
                    };
                    dirs.insert(dep_name.clone(), src_root);
                }
            }
            Manifest::DepSpec::Registry(_) => {
                // Registry deps not available in M12.1.
            }
        }
    }
    dirs
}

/// Rebuild a project's dependency source dirs (M12.1) and U17 package
/// resolution from its `pkg.jet`, for callers that only have the project
/// root (e.g. `resolve_import_target`, run after the bundle is loaded). Returns
/// empty maps when there is no manifest (R9 single-file mode).
fn project_resolution(project_root: &Path) -> (HashMap<String, PathBuf>, PkgResolution) {
    let pack_path = project_root.join(Syntax::PAYLOAD_FILE);
    let Some(Ok(mf)) = Manifest::load(project_root) else {
        return (HashMap::new(), PkgResolution::default());
    };
    let dep_dirs = collect_dep_dirs(&mf, project_root);
    let raw = fs::read_to_string(&pack_path).unwrap_or_default();
    (dep_dirs, collect_pkg_resolution(&raw))
}

fn load_file(
    path: &Path,
    display: &str,
    project_root: &Path,
    pkg_dep_dirs: &HashMap<String, PathBuf>,
    pkg_resolution: &PkgResolution,
    modules: &mut Vec<LoadedModule>,
    path_to_idx: &mut HashMap<PathBuf, usize>,
    stack: &mut Vec<PathBuf>,
    overlays: &[(&Path, &str)],
    for_check: bool,
    parse_teaching: &mut Vec<Diagnostic>,
) -> Result<(), Vec<Diagnostic>> {
    let norm = normalize_path(path);
    if stack.contains(&norm) {
        let cycle: Vec<String> = stack
            .iter()
            .chain(std::iter::once(&norm))
            .map(|p| relative_display(project_root, p))
            .collect();
        return Err(vec![Diagnostic::error(
            "E0604",
            "these files import each other in a circle".to_string(),
            "Jet loads every imported file before compiling, so imports can't loop".to_string(),
            format!("break the cycle: {}", cycle.join(" → ")),
            None,
        )]);
    }
    if path_to_idx.contains_key(&norm) {
        return Ok(());
    }

    let source = if let Some((_, text)) = overlays
        .iter()
        .rev()
        .find(|(candidate, _)| normalize_path(candidate) == norm)
    {
        (*text).to_string()
    } else {
        match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                return Err(vec![Diagnostic::error(
                    "E0603",
                    format!("can't find the file `{}`", display),
                    "an import path must point at an existing `.jet` file".to_string(),
                    "check the spelling, or create the missing file".to_string(),
                    None,
                )]);
            }
        }
    };

    let (toks, lex_diags) = Lexer::lex(&source);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = if for_check {
        match Parser::parse_for_check(&toks) {
            Ok((p, teaching)) => {
                parse_teaching.extend(teaching);
                p
            }
            Err(diags) => return Err(diags),
        }
    } else {
        match Parser::parse(&toks) {
            Ok(p) => p,
            Err(diags) => return Err(diags),
        }
    };

    let alias = default_module_alias(path);
    stack.push(norm.clone());
    let module_idx = modules.len();
    path_to_idx.insert(norm.clone(), module_idx);

    let imports = std::mem::take(&mut prog.imports);
    modules.push(LoadedModule {
        path: path.to_path_buf(),
        display: display.to_string(),
        source,
        alias,
        imports: imports.clone(),
        items: prog.items,
        web_target_ceiling: prog.web_target_ceiling,
        pub_file: prog.pub_file,
        no_prelude: prog.no_prelude,
        html_path: prog.html_path.clone(),
        no_alloc_policy: prog.no_alloc_policy,
    });

    for imp in &imports {
        // S59: C `use` forms use the reserved `c.` root legitimately.
        if crate::Foreign::is_active_namespace_import(imp) || crate::CFFI::is_c_import(imp) {
            continue;
        }
        // D-MOD3: `use alias.Item` / `use alias.{A,B}` forms don't load new files;
        // sema resolves them against already-loaded modules (E0609–E0611).
        if matches!(imp.kind, ImportKind::Unqualified { .. }) {
            continue;
        }
        if let Err(d) = check_reserved_import(imp) {
            stack.pop();
            return Err(vec![d]);
        }
        if core_module_path(imp).is_some() {
            continue;
        }
        // Active foreign namespace imports (`use c.<lib>`, `use js.<lib>`) do
        // not load local `.jet` modules. Each binder owns its cache/materializer.
        if crate::Foreign::is_active_namespace_import(imp) || crate::CFFI::is_c_import(imp) {
            continue;
        }
        let target = match resolve_import(imp, path, project_root, pkg_dep_dirs, pkg_resolution) {
            Ok(p) => p,
            Err(d) => {
                stack.pop();
                return Err(vec![d]);
            }
        };
        let child_display = relative_display(project_root, &target);
        if let Err(diags) = load_file(
            &target,
            &child_display,
            project_root,
            pkg_dep_dirs,
            pkg_resolution,
            modules,
            path_to_idx,
            stack,
            overlays,
            for_check,
            parse_teaching,
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
                    });
                }
            }
            None
        })
        .collect();

    for cm in code_module_decls {
        let target = match resolve_code_module_file(&cm.name, cm.name_span, path) {
            Ok(p) => p,
            Err(d) => {
                stack.pop();
                return Err(vec![d]);
            }
        };
        let child_display = relative_display(project_root, &target);
        if let Err(diags) = load_file(
            &target,
            &child_display,
            project_root,
            pkg_dep_dirs,
            pkg_resolution,
            modules,
            path_to_idx,
            stack,
            overlays,
            for_check,
            parse_teaching,
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
            is_pub: false,
            is_package_pub: false,
            inline_version: None,
        };
        modules[module_idx].imports.push(synthetic);
    }

    stack.pop();
    Ok(())
}

/// D-MOD1: find the file for `module name;` — look in the same directory as
/// `importing` for `{name}.jet` then `{name}/module.jet`.
fn resolve_code_module_file(
    name: &str,
    name_span: Span,
    importing: &Path,
) -> Result<PathBuf, Diagnostic> {
    let dir = importing.parent().unwrap_or(Path::new("."));
    let direct = normalize_path(&dir.join(format!("{}.{}", name, Syntax::FILE_EXT)));
    if direct.is_file() {
        return Ok(direct);
    }
    let module_jet = normalize_path(&dir.join(name).join(format!("module.{}", Syntax::FILE_EXT)));
    if module_jet.is_file() {
        return Ok(module_jet);
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
    pkg_dep_dirs: &HashMap<String, PathBuf>,
    pkg_resolution: &PkgResolution,
) -> Result<PathBuf, Diagnostic> {
    match &imp.kind {
        ImportKind::File(path_str, span) => {
            resolve_file_import(importing, path_str, project_root, *span)
        }
        ImportKind::Module(name, span) => {
            resolve_module_import(name, project_root, pkg_dep_dirs, pkg_resolution, *span)
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
    is_ring_module_staged, normalize_core_module, KNOWN_CORE_MODULES,
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
    if !resolved.starts_with(normalize_path(project_root)) {
        return Err(e0602(span));
    }
    if !resolved.is_file() {
        return Err(Diagnostic::error(
            "E0603",
            format!("can't find the file `{}`", path_str),
            "a file import path must point at an existing `.jet` file next to this file's tree"
                .to_string(),
            format!(
                "create `{}.{}`, or fix the path in `{} \"{}\"`",
                path_str,
                Syntax::FILE_EXT,
                Syntax::KW_USE,
                path_str
            ),
            Some(span),
        ));
    }
    Ok(resolved)
}

fn resolve_module_import(
    name: &str,
    project_root: &Path,
    pkg_dep_dirs: &HashMap<String, PathBuf>,
    pkg_resolution: &PkgResolution,
    span: Span,
) -> Result<PathBuf, Diagnostic> {
    // M12.1: check package dep dirs first.
    // `import words;` where "words" is a dep name → look in the dep's source root.
    let first_segment = name.split('.').next().unwrap_or(name);
    if let Some(dep_root) = pkg_dep_dirs.get(first_segment) {
        // Search within the dep's source tree for the module.
        let dep_matches = find_module_files(name, dep_root);
        if !dep_matches.is_empty() {
            return Ok(dep_matches[0].clone());
        }
        // If the dep root itself has the dep name as the top-level module,
        // look for main.jet in the dep root.
        let main_jet = dep_root.join(format!("main.{}", Syntax::FILE_EXT));
        if main_jet.is_file() && name == first_segment {
            return Ok(normalize_path(&main_jet));
        }
    }

    // U17: a `library` package is brought into code with the ordinary
    // `use <pkg>` form — once realized, its staged source in the shared hangar
    // is just an extra module search root. The hangar is authoritative for the
    // realized kind (empty `bin` = library, U10).
    if !pkg_resolution.is_empty() {
        // Realized library → resolve through its staged source tree.
        if let Some(staged) = pkg_resolution.realized_libs.get(first_segment) {
            let lib_matches = find_module_files(name, staged);
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
            && find_module_files(name, project_root).is_empty()
            && !pkg_dep_dirs.contains_key(first_segment)
        {
            return Err(e0983_unrealized_library(first_segment, span));
        }
    }

    let matches = find_module_files(name, project_root);
    match matches.len() {
        0 => Err(Diagnostic::error(
            "E0603",
            format!("can't find a module named `{}`", name),
            format!(
                "search from the project root for `{}.{}`, or `{}/{}/{}.{}` / `main.{}`",
                name,
                Syntax::FILE_EXT,
                name,
                name,
                name,
                Syntax::FILE_EXT,
                Syntax::FILE_EXT
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

fn find_module_files(name: &str, project_root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    collect_module_files(project_root, name, project_root, &mut found, &mut seen);
    found.sort();
    found
}

fn collect_module_files(
    dir: &Path,
    name: &str,
    project_root: &Path,
    found: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    if skip_search_dir(dir) {
        return;
    }

    let direct = normalize_path(&dir.join(format!("{}.{}", name, Syntax::FILE_EXT)));
    if direct.is_file() {
        insert_unique(found, seen, direct);
    }

    let sub = dir.join(name);
    if sub.is_dir() {
        for leaf in [name, "main"] {
            let p = normalize_path(&sub.join(format!("{}.{}", leaf, Syntax::FILE_EXT)));
            if p.is_file() {
                insert_unique(found, seen, p);
            }
        }
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_module_files(&p, name, project_root, found, seen);
        }
    }
}

fn insert_unique(found: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, p: PathBuf) {
    if seen.insert(p.clone()) {
        found.push(p);
    }
}

fn skip_search_dir(dir: &Path) -> bool {
    let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name == "build" || name == "target" || name.starts_with('.')
}

/// File stems become Rust `mod user_<alias>` names, so the alias must be a
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
    let (pkg_dep_dirs, pkg_resolution) = project_resolution(&bundle.project_root);
    let target_path = match resolve_import(
        imp,
        &importing.path,
        &bundle.project_root,
        &pkg_dep_dirs,
        &pkg_resolution,
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
            Syntax::KW_USE, name, name, name, Syntax::PAYLOAD_FILE
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
    fn message_names_e1226_and_both_filenames() {
        let dir = tempdir("message");
        fs::write(dir.join("pack.jet"), "").unwrap();
        let msg = stale_manifest_name_message(&dir).expect("message");
        assert!(msg.contains("E1226"));
        assert!(msg.contains("pack.jet"));
        assert!(msg.contains("pkg.jet"));
    }
}
