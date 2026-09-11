//! D-CONC-GROUP1=A / card #1564: a `Group` parameter keeps one meaning across
//! every compiler and execution surface that can see the concurrency feature.

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;


const SOURCE: &str = include_str!("../examples/features/concurrency/task_group_parameter.jet");
const EXPECTED: &str =
    include_str!("../examples/features/expected/concurrency/task_group_parameter.out");


fn assert_runtime_output(name: &str, result: (i32, String, String)) {
    let (code, stdout, stderr) = result;
    assert_eq!(code, 0, "{name} failed: {stderr}");
    assert_eq!(stdout, EXPECTED, "{name} output drifted: {stderr}");
}

#[test]
fn parser_accepts_group_parameters_in_the_canonical_example() {
    let (tokens, diagnostics) = jet::Lexer::lex(SOURCE);
    assert!(diagnostics.is_empty(), "lexer diagnostics: {diagnostics:?}");
    assert!(
        jet::Parser::parse(&tokens).is_ok(),
        "group parameter source must parse"
    );
}

#[test]
fn sema_accepts_group_parameters_in_the_canonical_example() {
    jet::compile(SOURCE).expect("group parameter source must pass sema");
}


#[test]
fn aot_runs_group_parameter_example() {
    if tir_support::have_rustc() {
        assert_runtime_output(
            "AOT",
            tir_support::build_and_run_full("jet_taskgroup_parameter", "aot", SOURCE),
        );
    }
}

#[test]
fn jit_runs_group_parameter_example() {
    assert_runtime_output("default jet run", tir_support::jit_run("jit", SOURCE));
}

#[test]
fn interpreter_runs_group_parameter_example() {
    assert_runtime_output(
        "interpreter",
        tir_support::interpreter_run("interpreter", SOURCE),
    );
}

#[test]
fn comptime_accepts_group_parameter_example() {
    let source = format!("{SOURCE}\n@folded :: 40 + 2\n\nfn show() {{ print(@folded) }}\n");
    jet::compile(&source).expect("comptime and Group-parameter code must share the front end");
}

#[test]
fn repl_runs_group_parameter_example() {
    // The canonical TIR task-group evaluator needs more than libtest's small
    // worker stack. Keep the REPL assertion on the same stack budget as the
    // evaluator's own task workers.
    let transcript = std::thread::Builder::new()
        .name("taskgroup-parameter-repl".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| jet::REPL::run_transcript(&[SOURCE, "run()"], None))
        .expect("spawn Group-parameter REPL test")
        .join()
        .expect("Group-parameter REPL test panicked");
    assert_eq!(transcript, format!("ok\n{EXPECTED}"));
}

#[test]
fn web_keeps_group_parameter_at_the_existing_checked_boundary() {
    let diagnostics = jet::compile_web_with_path(SOURCE, "task_group_parameter_web.jet")
        .expect_err("web must reject the structured task.group boundary honestly");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E-WEB-TIR-UNSUPPORTED"),
        "web Group behavior must use the registered TIR boundary diagnostic: {diagnostics:?}"
    );
}
