//! D-FAILURE-FOUNDATION1=A / I9: a declared error conversion keeps one carrier
//! and one report meaning across the resident, interpreter, and AOT edges.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const CONVERSION: &str = r#"
#Error
enum StoreFailure {
    Missing
}

impl StoreFailure -> Err {
    return Err("converted")
}

fn read() Int !StoreFailure -> Err(StoreFailure.Missing)

fn run() ! {
    read()
}
"#;

fn normalize_journey_paths(stderr: &str) -> String {
    let mut normalized = stderr
        .lines()
        .map(|line| {
            let Some(open) = line.find(" (") else {
                return line.to_string();
            };
            let Some(colon) = line.rfind(':') else {
                return line.to_string();
            };
            if !line[open + 2..colon].contains(".jet") {
                return line.to_string();
            }
            format!(
                "{}<source>:{}",
                &line[..open + 2],
                &line[colon + 1..]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if stderr.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

#[test]
fn declared_conversion_keeps_one_report_across_runtime_tiers() {
    let (jit_code, jit_out, jit_err) = tir_support::jit_run("failure_conversion_tiers", CONVERSION);
    assert_eq!(jit_code, 1, "default JIT must report the converted failure");
    assert!(jit_out.is_empty(), "converted failure must not print stdout");
    assert!(jit_err.contains("converted"), "JIT report: {jit_err}");

    let (interpreter_code, interpreter_out, interpreter_err) =
        tir_support::interpreter_run("failure_conversion_tiers", CONVERSION);
    assert_eq!(interpreter_code, jit_code);
    assert_eq!(interpreter_out, jit_out);
    assert_eq!(
        normalize_journey_paths(&interpreter_err),
        normalize_journey_paths(&jit_err)
    );

    if tir_support::have_rustc() {
        let (aot_code, aot_out, aot_err) =
            tir_support::build_and_run_full("failure_conversion_tiers", "main", CONVERSION);
        assert_eq!(aot_code, jit_code);
        assert_eq!(aot_out, jit_out);
        assert_eq!(
            normalize_journey_paths(&aot_err),
            normalize_journey_paths(&jit_err)
        );
    }
}
