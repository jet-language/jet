//! Multi-file program loading (M6 phase 3, S16).
//!
//! Resolves the import graph from an entry `.jet` file, detects cycles and
//! ambiguous module names, and returns a `ProgramBundle` for sema/codegen.

use crate::ast::{ImportDecl, ImportKind, LoadedModule, ProgramBundle};
use crate::diag::{Diagnostic, Span};
use crate::lexer;
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
    let project_root = entry_abs
        .parent()
        .map(normalize_path)
        .unwrap_or_else(|| cwd.clone());

    let mut modules = Vec::new();
    let mut path_to_idx: HashMap<PathBuf, usize> = HashMap::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    let mut parse_teaching = Vec::new();

    load_file(
        &entry_abs,
        entry_path,
        &project_root,
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
    })
}

fn load_file(
    path: &Path,
    display: &str,
    project_root: &Path,
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
        let target = match resolve_import(imp, path, project_root) {
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
) -> Result<PathBuf, Diagnostic> {
    match &imp.kind {
        ImportKind::File(path_str, span) => resolve_file_import(importing, path_str, project_root, *span),
        ImportKind::Module(name, span) => resolve_module_import(name, project_root, *span),
    }
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
    span: Span,
) -> Result<PathBuf, Diagnostic> {
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
        ImportKind::File(path, _) => path
            .rsplit('/')
            .next()
            .unwrap_or("module")
            .to_string(),
        ImportKind::Module(name, _) => name.clone(),
    }
}

pub fn resolve_import_target(
    bundle: &ProgramBundle,
    importing_idx: usize,
    imp: &ImportDecl,
) -> Result<usize, Diagnostic> {
    let importing = &bundle.modules[importing_idx];
    let target_path = match resolve_import(imp, &importing.path, &bundle.project_root) {
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
