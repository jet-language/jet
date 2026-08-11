mod common;

// Card #1658: DomRuntime.js is claimed to be a faithful line-by-line port of
// the Rust `JetBackend` pipeline (Ui.rs), but nothing checked that. This
// structural-diff guard extracts the measure/layout/paint/on_event stage
// names, in order, from both sides and asserts they match. It fails if a
// stage is renamed or added on only one side.

use std::fs;
use std::path::PathBuf;

/// The canonical pipeline stage order, as declared by the Rust
/// `JetBackend` trait (the single source of truth both hosts must port).
fn rust_backend_stage_order(ui_rs: &str) -> Vec<String> {
    let start = ui_rs
        .find("pub trait JetBackend")
        .expect("Ui.rs must declare `pub trait JetBackend`");
    let body_start = ui_rs[start..].find('{').expect("trait body") + start;
    let body_end = ui_rs[body_start..].find('}').expect("trait body end") + body_start;
    let body = &ui_rs[body_start..body_end];

    let mut stages = Vec::new();
    let mut rest = body;
    while let Some(fn_at) = rest.find("fn ") {
        rest = &rest[fn_at + 3..];
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        stages.push(name);
    }
    stages
}

/// Rust `snake_case` stage name -> the DomRuntime.js `camelCase` export name
/// that ports it. `paint`/`measure`/`layout` are already identical.
fn js_export_name(rust_stage: &str) -> String {
    match rust_stage {
        "on_event" => "onEvent".to_string(),
        other => other.to_string(),
    }
}

/// Every top-level `export function NAME(` in DomRuntime.js, in file order.
fn js_export_order(js: &str) -> Vec<String> {
    js.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line.strip_prefix("export function ")?;
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            Some(name)
        })
        .collect()
}

#[test]
fn dom_runtime_ports_the_same_pipeline_stages_in_the_same_order() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui_rs = fs::read_to_string(repo.join("crates/jet-codegen/src/Prelude/Ui.rs"))
        .expect("read Ui.rs");
    let dom_js = fs::read_to_string(repo.join("crates/jet-codegen/src/Prelude/DomRuntime.js"))
        .expect("read DomRuntime.js");

    let rust_stages = rust_backend_stage_order(&ui_rs);
    assert!(
        !rust_stages.is_empty(),
        "expected `JetBackend` to declare at least one stage"
    );

    let js_exports = js_export_order(&dom_js);

    // The JS pipeline stages, filtered down to only the names that port a
    // Rust `JetBackend` stage, must appear in the same relative order as
    // the Rust trait declares them — with none missing and none renamed.
    let expected: Vec<String> = rust_stages.iter().map(|s| js_export_name(s)).collect();
    let js_stage_names: Vec<&String> = js_exports
        .iter()
        .filter(|name| expected.contains(name))
        .collect();
    let actual: Vec<String> = js_stage_names.into_iter().cloned().collect();

    assert_eq!(
        actual, expected,
        "DomRuntime.js must export exactly the JetBackend pipeline stages \
         {expected:?}, in that order (Ui.rs is the source of truth) — found \
         {actual:?} instead. Rename or reorder drift means DomRuntime.js is \
         no longer a faithful port of Ui.rs."
    );
}

/// Card #1658: DEFAULT_MOUNT_COLS/ROWS is declared once in Ui.rs and once
/// (necessarily, since JS can't `include!` Rust) in DomRuntime.js. Nothing
/// stops the two literals from drifting apart, so read both and compare
/// the numbers directly.
#[test]
fn dom_runtime_default_mount_matches_ui_rs() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui_rs = fs::read_to_string(repo.join("crates/jet-codegen/src/Prelude/Ui.rs")).expect("read Ui.rs");
    let dom_js =
        fs::read_to_string(repo.join("crates/jet-codegen/src/Prelude/DomRuntime.js")).expect("read DomRuntime.js");

    fn number_after(source: &str, needle: &str) -> f64 {
        let at = source.find(needle).unwrap_or_else(|| panic!("expected `{needle}` in source"));
        let rest = &source[at + needle.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        digits.parse().unwrap_or_else(|e| panic!("parse number after `{needle}`: {e} ({digits:?})"))
    }

    let rust_cols = number_after(&ui_rs, "pub const DEFAULT_MOUNT_COLS: f64 = ");
    let rust_rows = number_after(&ui_rs, "pub const DEFAULT_MOUNT_ROWS: f64 = ");
    let js_cols = number_after(&dom_js, "export const DEFAULT_MOUNT_COLS = ");
    let js_rows = number_after(&dom_js, "export const DEFAULT_MOUNT_ROWS = ");

    assert_eq!(rust_cols, js_cols, "Ui.rs DEFAULT_MOUNT_COLS ({rust_cols}) drifted from DomRuntime.js ({js_cols})");
    assert_eq!(rust_rows, js_rows, "Ui.rs DEFAULT_MOUNT_ROWS ({rust_rows}) drifted from DomRuntime.js ({js_rows})");
}
