//! D-SHAPE-MODULEINTERNAL1=A: one parsed project-part index shared by
//! explicit import resolution and user-facing tooling.

use crate::AST::{ImportKind, Item};
use crate::{Lexer, Parser, Syntax};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectPartConflict {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectPartsReport {
    pub parts: Vec<ProjectPart>,
    pub conflicts: Vec<ProjectPartConflict>,
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
    let mut files = Vec::new();
    collect_jet_files(root, &mut files);
    files.sort();

    let mut explicit = BTreeSet::new();
    let mut declarations: BTreeMap<String, Vec<(PathBuf, bool)>> = BTreeMap::new();
    for path in files {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (tokens, lex_diags) = Lexer::lex(&source);
        if !lex_diags.is_empty() {
            continue;
        }
        let Ok(program) = Parser::parse(&tokens) else {
            continue;
        };
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

    let mut report = ProjectPartsReport::default();
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
    report
}

fn collect_jet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "target" | "build" | "node_modules") {
            continue;
        }
        if path.is_dir() {
            collect_jet_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(Syntax::FILE_EXT)
            && path.file_name().and_then(|name| name.to_str()) != Some(Syntax::PAYLOAD_FILE)
        {
            out.push(path);
        }
    }
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
}
