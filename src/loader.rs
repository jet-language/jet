//! Multi-file program loading (M6 phase 3, S16; M12 package support).
//!
//! Resolves the import graph from an entry `.jet` file, detects cycles and
//! ambiguous module names, and returns a `ProgramBundle` for sema/codegen.
//! When a `payload.jet` is found in the project root (walked upward from entry),
//! validates the manifest and wires package dep paths into module search (M12.1).

use crate::ast::{ImportDecl, ImportKind, LoadedModule, ProgramBundle};
use crate::diag::{Diagnostic, Span};
use crate::lexer;
use crate::manifest;
use crate::parser;
use crate::syntax;
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

/// Build the U17 package resolution from a project's `payload.jet` text and the
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
    if let Ok(pm) = crate::jetpack::packmanifest::parse(raw) {
        for dep in &pm.deps {
            declared_deps.insert(dep.name.clone());
        }
    }

    let mut realized_libs = HashMap::new();
    let mut realized_exes = HashSet::new();
    let roots = crate::jetpack::store::resolve();
    for entry in crate::jetpack::store::list(&roots) {
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
    let entry = PathBuf::from(entry_path);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let entry_abs = if entry.is_absolute() {
        entry
    } else {
        cwd.join(&entry)
    };
    let entry_abs = normalize_path(&entry_abs);

    // M12.1: walk upward from the entry file's directory to find payload.jet.
    // If found, use that directory as project_root and validate the manifest.
    // If none found, fall back to the entry file's directory (R9 — single-file mode).
    let entry_dir = entry_abs
        .parent()
        .map(normalize_path)
        .unwrap_or_else(|| cwd.clone());
    let (project_root, pkg_dep_dirs, pkg_resolution) = if let Some(manifest_dir) =
        find_manifest_root(&entry_dir)
    {
        // Found a payload.jet — validate it and collect dep source paths.
        let pack_path = manifest_dir.join(syntax::PAYLOAD_FILE);
        let raw = fs::read_to_string(&pack_path).unwrap_or_default();
        match manifest::parse(&pack_path, &raw) {
            Err(d) => return Err(vec![d]),
            Ok(mf) => {
                // Check toolchain constraint (E1208).
                if let Err(d) = manifest::check_toolchain(&mf, &pack_path.display().to_string()) {
                    return Err(vec![d]);
                }

                // Check the `edition:` field (E2001, D-REL3): a manifest may not
                // ask for an edition this toolchain doesn't ship.
                if let Err(d) =
                    manifest::check_edition_support(&mf, &pack_path.display().to_string())
                {
                    return Err(vec![d]);
                }

                // If there are deps, check lock staleness (E1202) and
                // dry-resolve path dep graph to catch version conflicts (E1201).
                if !mf.dependencies.is_empty() {
                    // E1202: lock must exist and include all manifest deps.
                    let lock_path = manifest_dir.join(syntax::UNIFIED_LOCK_FILE);
                    if lock_path.is_file() {
                        let lock_raw = fs::read_to_string(&lock_path).unwrap_or_default();
                        if let Ok(lock) = crate::lock::parse(&lock_raw) {
                            if let Err(d) = crate::lock::verify_lock_matches_manifest(
                                &lock,
                                &mf,
                                &lock_path.display().to_string(),
                            ) {
                                return Err(vec![d]);
                            }
                        } else {
                            return Err(vec![crate::lock::e1202(&lock_path.display().to_string())]);
                        }
                    }
                    // E1201: dry-resolve path deps for package name conflicts.
                    if let Err(d) = dry_resolve_path_deps(&mf, &manifest_dir) {
                        return Err(vec![d]);
                    }
                }

                // E1212/E1213: each packages: entry must have exactly one
                // module declaration in the source tree (U10 Chunk 3).
                if let Ok(pm) = crate::jetpack::packmanifest::parse(&raw) {
                    for pkg in &pm.packages {
                        match crate::jetpack::packmanifest::discover_module_in(
                            &manifest_dir,
                            &pkg.name,
                        ) {
                            Ok(_) => {}
                            Err(crate::jetpack::packmanifest::DiscoveryError::NotFound {
                                name,
                            }) => {
                                return Err(vec![manifest::e1212(
                                    &pack_path.display().to_string(),
                                    &name,
                                )]);
                            }
                            Err(crate::jetpack::packmanifest::DiscoveryError::Ambiguous {
                                name,
                                paths,
                            }) => {
                                return Err(vec![manifest::e1213(
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
        (entry_dir, HashMap::new(), PkgResolution::default())
    };

    let mut modules = Vec::new();
    let mut path_to_idx: HashMap<PathBuf, usize> = HashMap::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    let mut parse_teaching = Vec::new();

    load_file(
        &entry_abs,
        entry_path,
        &project_root,
        &pkg_dep_dirs,
        &pkg_resolution,
        &mut modules,
        &mut path_to_idx,
        &mut stack,
        overlay,
        for_check,
        &mut parse_teaching,
    )?;

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

    let mut bundle = ProgramBundle {
        entry: entry_idx,
        project_root,
        modules,
        parse_teaching,
        used_std: HashSet::new(),
        cffi: crate::cffi::CFfi::default(),
    };
    // S59 (E2-M14): fold every `@extern`/`@bindgen module c.<lib>` into merged
    // synthetic modules and resolve C `use` forms before sema sees the tree.
    bundle.cffi = crate::cffi::assemble(&mut bundle)?;
    Ok(bundle)
}

/// Walk upward from `start` to find the nearest directory containing `payload.jet`.
pub fn find_manifest_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(syntax::PAYLOAD_FILE).is_file() {
            return Some(dir);
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => return None,
        }
    }
}

/// Dry-resolve path dependencies to catch version conflicts (E1201).
/// Does not fetch, store, or link anything — only loads manifests.
fn dry_resolve_path_deps(mf: &manifest::Manifest, project_root: &Path) -> Result<(), Diagnostic> {
    // pkg_name → (version, blame_chain)
    let mut seen: std::collections::HashMap<String, (String, Vec<String>)> =
        std::collections::HashMap::new();
    let root_name = mf.package.name.clone();
    dry_resolve_recursive(mf, project_root, &[root_name], &mut seen)
}

fn dry_resolve_recursive(
    mf: &manifest::Manifest,
    pkg_dir: &Path,
    chain: &[String],
    seen: &mut std::collections::HashMap<String, (String, Vec<String>)>,
) -> Result<(), Diagnostic> {
    for (dep_alias, spec) in &mf.dependencies {
        let manifest::DepSpec::Path { path } = spec else {
            continue; // only path deps are resolved dry; git deps need fetch
        };
        let dep_path = normalize_path(&pkg_dir.join(path));
        // Load the dep's manifest to get its package name + version.
        let Some(Ok(dep_mf)) = manifest::load(&dep_path) else {
            continue; // missing manifest is caught later; not an E1201
        };
        let dep_pkg_name = dep_mf.package.name.clone();
        let dep_version = dep_mf.package.version.clone();

        let mut child_chain: Vec<String> = chain.to_vec();
        child_chain.push(dep_alias.clone());

        if let Some((prev_ver, prev_chain)) = seen.get(&dep_pkg_name).cloned() {
            if prev_ver != dep_version {
                return Err(crate::lock::e1201(
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
fn collect_dep_dirs(mf: &manifest::Manifest, project_root: &Path) -> HashMap<String, PathBuf> {
    let mut dirs = HashMap::new();
    for (dep_name, spec) in &mf.dependencies {
        match spec {
            manifest::DepSpec::Path { path } => {
                let abs = normalize_path(&project_root.join(path));
                // Source root for the dep: if .jet/ subdir exists use it, else the dep root.
                let src_root = if abs.join(".jet").is_dir() {
                    abs.join(".jet")
                } else {
                    abs
                };
                dirs.insert(dep_name.clone(), src_root);
            }
            manifest::DepSpec::Git { .. } => {
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
            manifest::DepSpec::Registry(_) => {
                // Registry deps not available in M12.1.
            }
        }
    }
    dirs
}

/// Rebuild a project's dependency source dirs (M12.1) and U17 package
/// resolution from its `payload.jet`, for callers that only have the project
/// root (e.g. `resolve_import_target`, run after the bundle is loaded). Returns
/// empty maps when there is no manifest (R9 single-file mode).
fn project_resolution(project_root: &Path) -> (HashMap<String, PathBuf>, PkgResolution) {
    let pack_path = project_root.join(syntax::PAYLOAD_FILE);
    let Some(Ok(mf)) = manifest::load(project_root) else {
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
    overlay: Option<(&Path, &str)>,
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

    let norm_overlay = overlay.map(|(p, _)| normalize_path(p));
    let source = if norm_overlay.as_ref() == Some(&norm) {
        overlay.unwrap().1.to_string()
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

    let (toks, lex_diags) = lexer::lex(&source);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = if for_check {
        match parser::parse_for_check(&toks) {
            Ok((p, teaching)) => {
                parse_teaching.extend(teaching);
                p
            }
            Err(diags) => return Err(diags),
        }
    } else {
        match parser::parse(&toks) {
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
    });

    for imp in &imports {
        // S59: C `use` forms use the reserved `c.` root legitimately.
        if crate::cffi::is_c_import(imp) {
            continue;
        }
        if let Err(d) = check_reserved_import(imp) {
            stack.pop();
            return Err(vec![d]);
        }
        if std_module_path(imp).is_some() {
            continue;
        }
        // S59 (E2-M14): C `use` forms (`use c.<lib>` / `use "<header>.h"`) do
        // not load `.jet` files — `cffi::assemble` materializes their modules.
        if crate::cffi::is_c_import(imp) {
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
            overlay,
            for_check,
            parse_teaching,
        ) {
            stack.pop();
            return Err(diags);
        }
    }

    stack.pop();
    Ok(())
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
    }
}

pub fn std_module_path(imp: &ImportDecl) -> Option<String> {
    let ImportKind::Module(name, _) = &imp.kind else {
        return None;
    };
    normalize_std_module(name)
}

pub fn normalize_std_module(name: &str) -> Option<String> {
    if name == syntax::STD_SHORT {
        return Some(syntax::STD_SHORT.to_string());
    }
    if let Some(rest) = name.strip_prefix("core.") {
        return Some(format!("core.{rest}"));
    }
    if name == syntax::STD_CANONICAL {
        return Some(syntax::STD_SHORT.to_string());
    }
    if let Some(rest) = name.strip_prefix("jet.core.") {
        return Some(format!("core.{rest}"));
    }
    None
}

pub fn is_legacy_std_import(name: &str) -> bool {
    name == syntax::LEGACY_STD_SHORT
        || name.starts_with("std.")
        || name == syntax::LEGACY_STD_CANONICAL
        || name.starts_with("jet.std.")
}

pub fn is_known_std_module(name: &str) -> bool {
    matches!(
        name,
        "core"
            | "core.fs"
            | "core.io"
            | "core.env"
            | "core.process"
            | "core.math"
            | "core.random"
            | "core.time"
            | "core.json"
            | "core.tasks"
    )
}

pub fn std_modules_list() -> &'static str {
    "core, core.fs, core.io, core.env, core.process, core.math, core.random, core.time, core.json, core.tasks"
}

fn check_reserved_import(imp: &ImportDecl) -> Result<(), Diagnostic> {
    if let Some(module) = std_module_path(imp) {
        if !is_known_std_module(&module) {
            let span = match &imp.kind {
                ImportKind::Module(_, span) => *span,
                ImportKind::File(_, _) => imp.span,
            };
            return Err(Diagnostic::error(
                "E1001",
                format!("there is no core module `{}`", module),
                "`core` is compiler-known in M10, and only the frozen core modules exist"
                    .to_string(),
                format!("use one of: {}", std_modules_list()),
                Some(span),
            ));
        }
        return Ok(());
    }

    let alias = import_alias(imp);
    if syntax::FIRST_PARTY_RESERVED.contains(&alias.as_str()) {
        return Err(Diagnostic::error(
            "E1002",
            format!("`{}` is reserved for first-party packages", alias),
            "`core`, `jet`, and the first-party ring names can't be used for local modules"
                .to_string(),
            format!(
                "rename the module or use it with `{} other_name`",
                syntax::KW_AS
            ),
            Some(imp.alias_span),
        ));
    }
    if let ImportKind::Module(name, span) = &imp.kind {
        let root = name.split('.').next().unwrap_or(name);
        if syntax::FIRST_PARTY_RESERVED.contains(&root) {
            return Err(Diagnostic::error(
                "E1002",
                format!("`{}` is reserved for first-party packages", root),
                "`core`, `jet`, and the first-party ring names can't be used for local modules"
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
    resolved.set_extension(syntax::FILE_EXT);
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
                syntax::FILE_EXT,
                syntax::KW_USE,
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
        let main_jet = dep_root.join(format!("main.{}", syntax::FILE_EXT));
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
                syntax::FILE_EXT,
                name,
                name,
                name,
                syntax::FILE_EXT,
                syntax::FILE_EXT
            ),
            format!(
                "add `{}.{}` under this project, or fix the `{}` name",
                name,
                syntax::FILE_EXT,
                syntax::KW_USE
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

    let direct = normalize_path(&dir.join(format!("{}.{}", name, syntax::FILE_EXT)));
    if direct.is_file() {
        insert_unique(found, seen, direct);
    }

    let sub = dir.join(name);
    if sub.is_dir() {
        for leaf in [name, "main"] {
            let p = normalize_path(&sub.join(format!("{}.{}", leaf, syntax::FILE_EXT)));
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

fn default_import_alias(kind: &ImportKind) -> String {
    match kind {
        ImportKind::File(path, _) => path.rsplit('/').next().unwrap_or("module").to_string(),
        ImportKind::Module(name, _) => name.clone(),
    }
}

pub fn resolve_import_target(
    bundle: &ProgramBundle,
    importing_idx: usize,
    imp: &ImportDecl,
) -> Result<usize, Diagnostic> {
    if std_module_path(imp).is_some() {
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
    if imp.alias.is_empty() {
        default_import_alias(&imp.kind)
    } else {
        imp.alias.clone()
    }
}

/// E0982 (U17): `use <pkg>` named a package declared `executable` in the
/// manifest. Executables go on PATH (you run them), not into code.
fn e0982_executable_use(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0982",
        format!("`{}` is an executable package, so it can't be used with `{}`", name, syntax::KW_USE),
        "an `executable` package installs a binary on your PATH — you run it, you don't import its code; only a `library` package is brought into code with `use` (U17)"
            .to_string(),
        format!(
            "remove `{} {};` and run the `{}` binary instead, or change `{}` to `library` in `{}` if you meant to import its code",
            syntax::KW_USE, name, name, name, syntax::PAYLOAD_FILE
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
            name, syntax::KW_USE, name
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
