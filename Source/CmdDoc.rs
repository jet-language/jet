//! `jet doc` — generate deterministic reference documentation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::Diagnostics::{Diagnostic, ReportPath, Span};
use jet::ExitCodes;
use jet_semindex::build_doc_graph;

use crate::OutputMode;

pub(crate) fn run_doc(target: &str, mode: OutputMode, check: bool) {
    let entry = PathBuf::from(crate::resolve_source_path(target));
    let projection =
        match crate::CmdInspect::check_projection_for_effects(&entry, "dev", &BTreeMap::new()) {
            Ok(projection) => projection,
            Err(diagnostics) => {
                let source = fs::read_to_string(&entry).unwrap_or_default();
                if mode.json {
                    print!(
                        "{}",
                        jet::render_all_json(&ReportPath::from_path(&entry), &source, &diagnostics)
                    );
                } else {
                    eprint!(
                        "{}",
                        jet::render_all_colored(
                            &entry.display().to_string(),
                            &source,
                            &diagnostics,
                            mode.color_stderr(),
                        )
                    );
                }
                exit(ExitCodes::USER_ERROR);
            }
        };
    let graph = build_doc_graph(&projection.bundle, &projection.index);

    if check {
        let diagnostics = undocumented_diagnostics(&graph);
        if !diagnostics.is_empty() {
            render_doc_diagnostics(&projection.bundle, &diagnostics, mode);
            exit(ExitCodes::USER_ERROR);
        }
        if mode.json {
            println!("{}", graph.to_json());
        } else {
            println!(
                "doc check: passed ({} doctest block(s))",
                graph.doctests.len()
            );
        }
        return;
    }

    if mode.json {
        println!("{}", graph.to_json());
        return;
    }

    let output_dir = entry
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("docs");
    if let Err(error) = fs::create_dir_all(&output_dir)
        .and_then(|_| fs::write(output_dir.join("index.html"), graph.to_html()))
        .and_then(|_| fs::write(output_dir.join("index.md"), graph.to_markdown()))
    {
        crate::cli_error!(
            "E2105",
            "couldn't write generated documentation in `{}`: {}",
            output_dir.display(),
            error
        );
        exit(ExitCodes::USER_ERROR);
    }
    println!("doc: generated {}", output_dir.display());
}

fn undocumented_diagnostics(graph: &jet_semindex::DocGraph) -> Vec<(String, Diagnostic)> {
    graph
        .undocumented_public()
        .into_iter()
        .map(|item| {
            let name = item.qualified_name.clone();
            (
                item.source.path.clone(),
                Diagnostic::from_row(
                    "L2201",
                    &[("name", name.as_str())],
                    Some(Span::new(item.source.start, item.source.end)),
                ),
            )
        })
        .collect()
}

fn render_doc_diagnostics(
    bundle: &jet::AST::ProgramBundle,
    diagnostics: &[(String, Diagnostic)],
    mode: OutputMode,
) {
    let mut by_path: BTreeMap<&str, Vec<Diagnostic>> = BTreeMap::new();
    for (path, diagnostic) in diagnostics {
        by_path
            .entry(path.as_str())
            .or_default()
            .push(diagnostic.clone());
    }
    for (path, diagnostics) in by_path {
        let Some(module) = bundle.modules.iter().find(|module| {
            let stable = module
                .path
                .strip_prefix(&bundle.project_root)
                .ok()
                .map_or_else(
                    || module.display.replace('\\', "/"),
                    |relative| relative.to_string_lossy().replace('\\', "/"),
                );
            stable == path
        }) else {
            continue;
        };
        if mode.json {
            print!(
                "{}",
                jet::render_all_json(
                    &ReportPath::from_path(&module.path),
                    &module.source,
                    &diagnostics,
                )
            );
        } else {
            eprint!(
                "{}",
                jet::render_all_colored(
                    &module.display,
                    &module.source,
                    &diagnostics,
                    mode.color_stderr(),
                )
            );
        }
    }
}
