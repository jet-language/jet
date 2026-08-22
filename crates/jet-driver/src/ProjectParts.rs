//! D-SHAPE-MODULEINTERNAL1=A: one parsed project-part index shared by
//! explicit import resolution and user-facing tooling.

use crate::AST::{ImportKind, Item};
use crate::Diagnostics::{Diagnostic, Span};
use crate::{Lexer, Parser, Syntax};
use jet_pkg_model::Authority::{AuthorityError, AuthorityResolver};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectPartState {
    Automatic,
    Explicit,
    Skipped,
}

impl ProjectPartState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Explicit => "explicit",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectPart {
    pub name: String,
    pub path: PathBuf,
    pub state: ProjectPartState,
}

impl ProjectPart {
    /// The source spelling accepted by `use` and shown by user-facing
    /// projections. Keep the scanner's leaf `name` for lookup and storage.
    pub fn canonical_name(&self) -> String {
        format!("{}{}", Syntax::PROJECT_IMPORT_PREFIX, self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectPartConflict {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

impl ProjectPartConflict {
    pub fn canonical_name(&self) -> String {
        format!("{}{}", Syntax::PROJECT_IMPORT_PREFIX, self.name)
    }
}

#[derive(Clone, Debug)]
pub struct ProjectPartScanFailure {
    pub path: PathBuf,
    pub module_names: Vec<String>,
    pub problem: Diagnostic,
    pub authority: bool,
}

impl ProjectPartScanFailure {
    pub fn diagnostic(&self, name: &str, root: &Path, span: Span) -> Diagnostic {
        let path = self
            .path
            .strip_prefix(root)
            .unwrap_or(&self.path)
            .to_string_lossy()
            .replace('\\', "/");
        Diagnostic::error(
            "E0603",
            format!(
                "can't load project module `{}{}` from `{path}`",
                Syntax::PROJECT_IMPORT_PREFIX,
                name
            ),
            format!("`{path}` has a source error: {}", self.problem.what),
            format!(
                "fix `{path}` first, then import `{}{}` again",
                Syntax::PROJECT_IMPORT_PREFIX,
                name
            ),
            Some(span),
        )
    }
}

impl ProjectPartConflict {
    pub fn diagnostic(&self, root: &Path, span: Option<Span>) -> Diagnostic {
        let paths = self
            .paths
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect::<Vec<_>>()
            .join(", ");
        Diagnostic::error(
            "E0606",
            format!(
                "project module `{}` is declared more than once",
                self.canonical_name()
            ),
            "a project module name must resolve to one declaration".to_string(),
            format!("keep one `module {}` declaration; found {paths}", self.name),
            span,
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectPartsReport {
    pub parts: Vec<ProjectPart>,
    pub conflicts: Vec<ProjectPartConflict>,
    /// Successfully parsed source files without a declared module part.
    /// These remain loaded project modules for tooling, but are not names
    /// accepted by `use project.…`.
    pub source_files: Vec<PathBuf>,
}

impl ProjectPartsReport {
    pub fn named(&self, name: &str) -> Vec<&ProjectPart> {
        self.parts.iter().filter(|part| part.name == name).collect()
    }

    /// Open files and files without module declarations remain visible.
    /// A file containing only unrequested internal parts stays out of automatic
    /// semantic indexes, while mixed and explicitly imported files stay in.
    pub fn should_index(&self, path: &Path) -> bool {
        let matching: Vec<_> = self.parts.iter().filter(|part| part.path == path).collect();
        matching.is_empty()
            || matching
                .iter()
                .any(|part| part.state != ProjectPartState::Skipped)
    }
}

pub fn scan(root: &Path) -> ProjectPartsReport {
    scan_with_diagnostics(root, &[]).0
}

pub fn scan_with_overlays(root: &Path, overlays: &[(PathBuf, String)]) -> ProjectPartsReport {
    scan_with_diagnostics(root, overlays).0
}

/// Every source file this package compiles, sorted, without `scan`'s parse
/// pass: the same authority-checked enumeration the part index is built from,
/// minus package manifests and nested packages. Tooling that must act on every
/// file in the package — bare `jet test` collecting `#Test` blocks package-wide
/// (spec S43) — enumerates here instead of walking the filesystem again, so the
/// file set stays the project scan's, not a second opinion about it. An
/// authority failure yields an empty set: the caller's ordinary compile path
/// reports the real error rather than a second, worse copy of it.
pub fn source_files(root: &Path) -> Vec<PathBuf> {
    let Ok(resolver) = AuthorityResolver::open(root) else {
        return Vec::new();
    };
    let Ok(files) = resolver.discover_source_files() else {
        return Vec::new();
    };
    // A nested package is its own package: its files are that package's
    // modules, not this one's. Stop at every nested manifest — the boundary
    // package build-entry discovery already draws.
    let nested = files
        .iter()
        .filter(|file| {
            file.relative.file_name().and_then(|name| name.to_str()) == Some(Syntax::PACKAGE_FILE)
        })
        .filter_map(|file| file.relative.parent())
        .filter(|parent| !parent.as_os_str().is_empty() && *parent != Path::new("."))
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    files
        .iter()
        .filter(|file| {
            !is_manifest_file(&file.relative)
                && !nested.iter().any(|package| file.relative.starts_with(package))
        })
        .map(|file| file.path.clone())
        .collect()
}

/// The package manifest (and its migration-era spelling) carries package
/// facts, not project code: never a part, never a source file.
fn is_manifest_file(relative: &Path) -> bool {
    relative
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == Syntax::PACKAGE_FILE || name == Syntax::PAYLOAD_FILE)
}

pub fn scan_with_diagnostics(
    root: &Path,
    overlays: &[(PathBuf, String)],
) -> (ProjectPartsReport, Vec<ProjectPartScanFailure>) {
    let (resolver, checked_files, authority_failure) = match AuthorityResolver::open(root) {
        Ok(resolver) => match resolver.discover_source_files() {
            Ok(files) => (Some(resolver), files, None),
            Err(error) => (Some(resolver), Vec::new(), Some((root.to_path_buf(), error))),
        },
        Err(error) if error.is_missing() => (None, Vec::new(), None),
        Err(error) => (None, Vec::new(), Some((root.to_path_buf(), error))),
    };
    let mut files = checked_files
        .iter()
        .filter(|file| !is_manifest_file(&file.relative))
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    files.extend(
        overlays
            .iter()
            .map(|(path, _)| path)
            .filter(|path| {
                path.starts_with(root)
                    && path.extension().and_then(|ext| ext.to_str()) == Some(Syntax::FILE_EXT)
            })
            .cloned(),
    );
    files.sort();
    files.dedup();

    let mut explicit = BTreeSet::new();
    let mut declarations: BTreeMap<String, Vec<(PathBuf, bool)>> = BTreeMap::new();
    let mut source_files = Vec::new();
    let mut failures = Vec::new();
    if let Some((path, error)) = authority_failure {
        failures.push(authority_failure_for(path, error));
        return (ProjectPartsReport::default(), failures);
    }
    for path in files {
        let source = if let Some((_, source)) = overlays.iter().rev().find(|(p, _)| p == &path) {
            source.clone()
        } else {
            let Some(file) = checked_files.iter().find(|file| file.path == path) else {
                continue;
            };
            if let Some(resolver) = resolver.as_ref() {
                if let Err(error) = resolver.revalidate_file(file) {
                    failures.push(authority_failure_for(file.path.clone(), error));
                    return (ProjectPartsReport::default(), failures);
                }
            }
            match file.text() {
                Ok(source) => source,
                Err(error) => {
                    failures.push(ProjectPartScanFailure {
                        module_names: path
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .map(|stem| vec![stem.to_string()])
                            .unwrap_or_default(),
                        problem: error.diagnostic(),
                        path,
                        authority: true,
                    });
                    return (ProjectPartsReport::default(), failures);
                }
            }
        };
        let (tokens, lex_diags) = Lexer::lex(&source);
        if !lex_diags.is_empty() {
            failures.push(ProjectPartScanFailure {
                path,
                module_names: declared_module_names(&tokens),
                problem: lex_diags[0].clone(),
                authority: false,
            });
            continue;
        }
        let program = match Parser::parse(&tokens) {
            Ok(program) => program,
            Err(parse_diags) => {
                failures.push(ProjectPartScanFailure {
                    path,
                    module_names: declared_module_names(&tokens),
                    problem: parse_diags[0].clone(),
                    authority: false,
                });
                continue;
            }
        };
        source_files.push(path.clone());
        if let Some(file) = checked_files.iter().find(|file| file.path == path) {
            if let Some(resolver) = resolver.as_ref() {
                if let Err(error) = resolver.revalidate_file(file) {
                    failures.push(authority_failure_for(file.path.clone(), error));
                    return (ProjectPartsReport::default(), failures);
                }
            }
        }
        for import in &program.imports {
            let ImportKind::Module(name, _) = &import.kind else {
                continue;
            };
            if let Some(name) = name.strip_prefix(Syntax::PROJECT_IMPORT_PREFIX) {
                explicit.insert(name.to_string());
            }
        }
        for item in &program.items {
            let (name, automatic) = match item {
                Item::Module(module) => (module.name.clone(), module.is_auto_discovered()),
                Item::CodeModule(module) => (
                    module.name.clone(),
                    !module.name.starts_with(Syntax::MODULE_INTERNAL_PREFIX),
                ),
                _ => continue,
            };
            declarations
                .entry(name)
                .or_default()
                .push((path.clone(), automatic));
        }
    }

    let declared_paths = declarations
        .values()
        .flat_map(|parts| parts.iter().map(|(path, _)| path.clone()))
        .collect::<BTreeSet<_>>();
    let mut report = ProjectPartsReport {
        source_files: source_files
            .into_iter()
            .filter(|path| !declared_paths.contains(path))
            .collect(),
        ..ProjectPartsReport::default()
    };
    for (name, declarations) in declarations {
        if declarations.len() > 1 {
            report.conflicts.push(ProjectPartConflict {
                name: name.clone(),
                paths: declarations.iter().map(|(path, _)| path.clone()).collect(),
            });
        }
        for (path, automatic) in declarations {
            let state = if automatic {
                ProjectPartState::Automatic
            } else if explicit.contains(&name) {
                ProjectPartState::Explicit
            } else {
                ProjectPartState::Skipped
            };
            report.parts.push(ProjectPart {
                name: name.clone(),
                path,
                state,
            });
        }
    }
    if let Some(resolver) = resolver {
        if let Err(error) = resolver.revalidate_root() {
            failures.push(authority_failure_for(root.to_path_buf(), error));
            return (ProjectPartsReport::default(), failures);
        }
    }
    (report, failures)
}

fn authority_failure_for(path: PathBuf, error: AuthorityError) -> ProjectPartScanFailure {
    ProjectPartScanFailure {
        path,
        module_names: Vec::new(),
        problem: error.diagnostic(),
        authority: true,
    }
}

fn declared_module_names(tokens: &[Lexer::Token]) -> Vec<String> {
    tokens
        .windows(2)
        .filter_map(|pair| match (&pair[0].kind, &pair[1].kind) {
            (Lexer::TokKind::KwModule, Lexer::TokKind::Ident(name)) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jet-project-parts-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn internal_part_is_skipped_until_explicitly_imported() {
        let root = tempdir("explicit");
        let internal = root.join("bench-config.jet");
        let source = "module _bench { }\n";
        std::fs::write(&internal, source).unwrap();
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty());
        let program = Parser::parse(&tokens).expect("parse exact internal module spelling");
        assert!(matches!(
            program.items.as_slice(),
            [Item::CodeModule(module)] if module.name == "_bench"
        ));
        let report = scan(&root);
        assert_eq!(report.parts[0].name, "_bench");
        assert_eq!(report.parts[0].state, ProjectPartState::Skipped);
        assert!(!report.should_index(&internal));

        std::fs::write(root.join("main.jet"), "use project._bench;\nfn run() {}\n").unwrap();
        let report = scan(&root);
        assert_eq!(report.parts[0].state, ProjectPartState::Explicit);
        assert!(report.should_index(&internal));
    }

    #[test]
    fn duplicate_declarations_are_conflicts_even_when_skipped() {
        let root = tempdir("conflict");
        std::fs::write(root.join("a.jet"), "module _bench { }\n").unwrap();
        std::fs::write(root.join("b.jet"), "module _bench { }\n").unwrap();
        let report = scan(&root);
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(report.conflicts[0].name, "_bench");
        assert_eq!(report.conflicts[0].paths.len(), 2);
    }

    #[test]
    fn source_files_are_the_package_own_modules() {
        // Bare `jet test` enumerates here (spec S43): every module the package
        // compiles, no manifests, and nothing belonging to a nested package.
        let root = tempdir("source-files");
        std::fs::write(root.join(Syntax::PACKAGE_FILE), "name: \"outer\"\n").unwrap();
        std::fs::write(root.join("run.jet"), "fn run() {}\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/math.jet"), "fn double() Int -> 2\n").unwrap();
        std::fs::create_dir_all(root.join("vendor/inner")).unwrap();
        std::fs::write(
            root.join("vendor/inner").join(Syntax::PACKAGE_FILE),
            "name: \"inner\"\n",
        )
        .unwrap();
        std::fs::write(root.join("vendor/inner/lib.jet"), "fn theirs() {}\n").unwrap();

        let base = std::fs::canonicalize(&root).unwrap();
        let files = source_files(&root);
        let names = files
            .iter()
            .map(|path| {
                path.strip_prefix(&base)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["run.jet".to_string(), "src/math.jet".to_string()]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn scan_ignores_non_regular_jet_entries() {
        let root = tempdir("non-regular");
        std::fs::write(root.join("main.jet"), "fn run() {}\n").unwrap();
        let fifo = root.join("blocked.jet");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());

        let (opened, observed) = std::sync::mpsc::channel();
        let writer_fifo = fifo.clone();
        let writer = std::thread::spawn(move || {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(writer_fifo)
                .unwrap();
            opened.send(()).unwrap();
            drop(file);
        });

        let report = scan(&root);
        let scanned_fifo = observed.try_recv().is_ok();
        if !scanned_fifo {
            drop(
                std::fs::OpenOptions::new()
                    .read(true)
                    .open(&fifo)
                    .unwrap(),
            );
        }
        writer.join().unwrap();
        assert!(!scanned_fifo, "project scans must not open a FIFO as source");
        assert!(report.parts.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
