//! D-SEMINDEX1: `jet inspect semindex` — smoke CLI for the stable semantic-index JSON API.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_semindex::{open, SemIndexError, SCHEMA_VERSION};

pub(crate) fn run_semindex(args: &[String], json: bool) {
    let path = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    let Some(path) = path else {
        crate::cli_error!(@fix "E2104", "`jet inspect semindex` needs an entry file", "jet inspect semindex examples/features/basics/hello.jet");
        exit(ExitCodes::USER_ERROR);
    };

    let abs = absolutize(path);
    match open(&abs) {
        Ok(idx) => {
            if json {
                println!("{}", idx.to_json());
            } else {
                println!("semantic index (schema v{})", SCHEMA_VERSION);
                println!("  definitions: {}", idx.definitions().len());
                println!("  references:  {}", idx.references().len());
                println!("  call edges:  {}", idx.call_edges().len());
                println!("  effects:     {}", idx.effects().len());
                println!("note: pass --json for the full stable document");
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
