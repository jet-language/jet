//! D-IMPACT1: `jet inspect impact` — blast-radius queries over the semantic index.

use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
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
        let impact = impact_value(&report, &checked.check);
        println!(
            "{}",
            StatusEnvelope::new("inspect.impact", report.found)
                .with_field("impact", impact)
                .json()
        );
    } else {
        print!("{}", crate::CmdInspect::check_result_text(&checked.check));
        print!("{}", report.render_text());
    }
    if !report.found {
        exit(ExitCodes::USER_ERROR);
    }
}

fn impact_value(report: &ImpactReport, check: &crate::CmdInspect::CheckResult) -> StatusValue {
    let span = |start: usize, end: usize| {
        StatusValue::object(
            StatusFields::new()
                .with("start", start)
                .with("end", end),
        )
    };
    let reference = |value: &jet_impact::ImpactRef| {
        StatusValue::object(
            StatusFields::new()
                .with("depth", value.depth)
                .with("name", value.name.as_str())
                .with("module_path", value.module_path.as_str())
                .with("span", span(value.span_start, value.span_end)),
        )
    };
    let edge = |value: &jet_impact::ImpactEdge| {
        StatusValue::object(
            StatusFields::new()
                .with("depth", value.depth)
                .with("caller", value.caller.as_str())
                .with("callee", value.callee.as_str())
                .with("module_path", value.module_path.as_str())
                .with("span", span(value.span_start, value.span_end)),
        )
    };
    StatusValue::object(
        StatusFields::new()
            .with("symbol", report.symbol.as_str())
            .with("found", report.found)
            .with("depth_limit", report.depth_limit)
            .with(
                "definition_module",
                report
                    .definition_module
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            )
            .with(
                "references",
                StatusValue::array(report.references.iter().map(reference)),
            )
            .with(
                "call_sites",
                StatusValue::array(report.call_sites.iter().map(edge)),
            )
            .with(
                "upstream_callers",
                StatusValue::array(report.upstream_callers.iter().map(edge)),
            )
            .with(
                "downstream_callees",
                StatusValue::array(report.downstream_callees.iter().map(edge)),
            )
            .with("check", crate::CmdInspect::check_result_value(check)),
    )
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
