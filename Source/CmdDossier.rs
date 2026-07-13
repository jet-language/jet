//! D-WD2/D-DOSSIER1: `jet inspect dossier` — one explainable view over semantic facts.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_semindex::{open, SemIndexError};

pub(crate) fn run_dossier(args: &[String], json: bool) {
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    let (path, target) = match positional.as_slice() {
        [path] => (*path, None),
        [path, target] => (*path, Some(*target)),
        _ => {
            eprintln!("error: `jet inspect dossier` needs an entry file and optional symbol");
            eprintln!(" Fix: jet inspect dossier examples/features/basics/hello.jet run");
            exit(ExitCodes::USER_ERROR);
        }
    };

    let abs = absolutize(path);
    match open(&abs) {
        Ok(idx) => {
            let target = target.unwrap_or_else(|| {
                idx.definitions()
                    .iter()
                    .find(|d| matches!(d.kind, jet_semindex::SymbolKind::Struct { .. }))
                    .or_else(|| idx.definitions().iter().find(|d| d.name == "run"))
                    .map(|d| d.name.as_str())
                    .unwrap_or("run")
            });
            let dossier = idx.dossier(target);
            let budgets = budget_projection(&abs);
            if json {
                let mut value = dossier.to_json();
                if value.ends_with('}') {
                    value.pop();
                    value.push_str(",\"performance_budgets\":");
                    value.push_str(&budgets.to_json());
                    value.push('}');
                }
                println!("{value}");
            } else {
                print!("{}", dossier.render_text());
                print!("{}", budgets.render_text());
            }
            if dossier.definition.is_none() {
                exit(ExitCodes::USER_ERROR);
            }
        }
        Err(SemIndexError::Load(diags)) => {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(
                        &abs.display().to_string(),
                        "",
                        std::slice::from_ref(d)
                    )
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn budget_projection(entry: &Path) -> jet::BudgetView::BudgetProjection {
    let entry_text = entry.to_string_lossy();
    let (diagnostics, bundle, _) = jet::Driver::check_file_with_effect_facts(&entry_text, None, false);
    if diagnostics.iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error) {
        return jet::BudgetView::BudgetProjection::default();
    }
    let Some(bundle) = bundle else { return jet::BudgetView::BudgetProjection::default() };
    let root = jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|| entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let sources = bundle.modules.iter().map(|module| {
        let path = module.path.strip_prefix(&root).unwrap_or(&module.path).to_string_lossy().replace('\\', "/");
        (path, jet::SHA256::sha256_hex(module.source.as_bytes()))
    }).collect::<Vec<_>>();
    jet::BudgetView::read_compatible(&root, &sources)
}

fn absolutize(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}
