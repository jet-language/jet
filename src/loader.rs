//! Multi-file program loading (M6 phase 3, S16; M12 package support).
//!
//! Resolves the import graph from an entry `.jet` file, detects cycles and
//! ambiguous module names, and returns a `ProgramBundle` for sema/codegen.
//! When a `jet.toml` is found in the project root (walked upward from entry),
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

    // M12.1: walk upward from the entry file's directory to find jet.toml.
    // If found, use that directory as project_root and validate the manifest.
    // If none found, fall back to the entry file's directory (R9 — single-file mode).
    let entry_dir = entry_abs
        .parent()
        .map(normalize_path)
        .unwrap_or_else(|| cwd.clone());
    let (project_root, pkg_dep_dirs) = if let Some(manifest_dir) = find_manifest_root(&entry_dir) {
        // Found a jet.toml — validate it and collect dep source paths.
        let toml_path = manifest_dir.join("jet.toml");
        let raw = fs::read_to_string(&toml_path).unwrap_or_default();
        match manifest::parse(&toml_path, &raw) {
            Err(d) => return Err(vec![d]),
            Ok(mf) => {
                // Check toolchain constraint (E1208).
                if let Err(d) = manifest::check_toolchain(&mf, &toml_path.display().to_string()) {
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

                // Collect package dep source directories for module search.
                let dep_dirs = collect_dep_dirs(&mf, &manifest_dir);
                (manifest_dir, dep_dirs)
            }
        }
    } else {
        // R9: no manifest — single-file mode, project root is the entry dir.
        (entry_dir, HashMap::new())
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

    Ok(ProgramBundle {
        entry: entry_idx,
        project_root,
        modules,
        parse_teaching,
        used_std: HashSet::new(),
    })
}

/// Walk upward from `start` to find the nearest directory containing `jet.toml`.
pub fn find_manifest_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("jet.toml").is_file() {
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

fn load_file(
    path: &Path,
    display: &str,
    project_root: &Path,
    pkg_dep_dirs: &HashMap<String, PathBuf>,
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
        if let Err(d) = check_reserved_import(imp) {
            stack.pop();
            return Err(vec![d]);
        }
        if std_module_path(imp).is_some() {
            continue;
        }
        let target = match resolve_import(imp, path, project_root, pkg_dep_dirs) {
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
) -> Result<PathBuf, Diagnostic> {
    match &imp.kind {
        ImportKind::File(path_str, span) => {
            resolve_file_import(importing, path_str, project_root, *span)
        }
        ImportKind::Module(name, span) => {
            resolve_module_import(name, project_root, pkg_dep_dirs, *span)
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
    if let Some(rest) = name.strip_prefix("std.") {
        return Some(format!("std.{rest}"));
    }
    if name == syntax::STD_CANONICAL {
        return Some(syntax::STD_SHORT.to_string());
    }
    if let Some(rest) = name.strip_prefix("jet.std.") {
        return Some(format!("std.{rest}"));
    }
    None
}

pub fn is_known_std_module(name: &str) -> bool {
    matches!(
        name,
        "std"
            | "std.fs"
            | "std.io"
            | "std.env"
            | "std.process"
            | "std.math"
            | "std.random"
            | "std.time"
            | "std.json"
            | "std.tasks"
    )
}

pub fn std_modules_list() -> &'static str {
    "std, std.fs, std.io, std.env, std.process, std.math, std.random, std.time, std.json, std.tasks"
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
                format!("there is no standard module `{}`", module),
                "`std` is compiler-known in M10, and only the frozen core modules exist"
                    .to_string(),
                format!("import one of: {}", std_modules_list()),
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
            "`std`, `jet`, and the first-party ring names can't be used for local modules"
                .to_string(),
            format!(
                "rename the module or import it with `{} other_name`",
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
                "`std`, `jet`, and the first-party ring names can't be used for local modules"
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
                syntax::KW_IMPORT,
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
                syntax::KW_IMPORT
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
            "standard modules do not resolve to files".to_string(),
            "`std` is provided by the compiler in M10".to_string(),
            "handle this import as a compiler-known module".to_string(),
            Some(imp.span),
        ));
    }
    let importing = &bundle.modules[importing_idx];
    let target_path =
        match resolve_import(imp, &importing.path, &bundle.project_root, &HashMap::new()) {
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
