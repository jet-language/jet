//! D-IMPACT1: `jet inspect impact` — blast-radius queries over the semantic index.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_impact::ImpactReport;
use jet_semindex::{open, SemIndexError};

pub(crate) fn run_impact(args: &[String], json: bool) {
    let mut depth = 3usize;
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if let Some(n) = a.strip_prefix("--depth=") {
            depth = n.parse().unwrap_or_else(|_| {
                crate::cli_error!("E2104", "`--depth` must be a positive integer");
                exit(ExitCodes::USER_ERROR);
            });
        } else if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    let (path, symbol) = match positional.as_slice() {
        [path, symbol] => (*path, *symbol),
        _ => {
            crate::cli_error!(@fix "E2104", "`jet inspect impact` needs an entry file and a symbol name", "jet inspect impact examples/features/effects/effects.jet report");
            exit(ExitCodes::USER_ERROR);
        }
    };

    if depth == 0 {
        crate::cli_error!("E2104", "`--depth` must be at least 1");
        exit(ExitCodes::USER_ERROR);
    }

    let abs = absolutize(path);
    match open(&abs) {
        Ok(idx) => {
            let report = ImpactReport::analyze(&idx, symbol, depth);
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.render_text());
            }
            if !report.found {
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
