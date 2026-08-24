//! D-IMPACT1: `jet inspect impact` — blast-radius queries over the semantic index.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_foundation::Report::render_status_json;
use jet_impact::ImpactReport;
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
    let checked = crate::CmdInspect::check_projection(&abs).unwrap_or_else(|diagnostics| {
        crate::CmdInspect::render_check_failure(&abs, &diagnostics, json, false);
    });
    let report = ImpactReport::analyze(&checked.index, symbol, depth);
    if json {
        let document = crate::CmdInspect::with_check_json(report.to_json(), &checked.check);
        println!(
            "{}",
            render_status_json(
                "ok",
                true,
                "inspect.impact",
                &format!(",\"impact\":{document}"),
            )
        );
    } else {
        print!("{}", crate::CmdInspect::check_result_text(&checked.check));
        print!("{}", report.render_text());
    }
    if !report.found {
        exit(ExitCodes::USER_ERROR);
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
