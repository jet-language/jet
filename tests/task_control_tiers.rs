//! D-CONC-SPAWN1=D + D-COROUTINE1=A (card #1685): the canonical task surface
//! means the same thing on every execution tier.

#[path = "common/mod.rs"]
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc, run_default_multi};
use std::{fs, thread};

const SOURCE: &str = r#"
use core.time as time

fn slow_three() => Int {
    time.sleep(25)
    return 3
}

fn slow_six() => Int {
    time.sleep(25)
    return 6
}

fn run() {
    handle :: task 7
    handle.pause()
    handle.resume()
    print(handle.join() ?? 0)
    task.group workers(limit: 2) {
        child :: task 9
        print(child.join() ?? 0)
    }
    print((task.all { 1, 2 }) ?? [])
    print((task.race { slow_three(), 4 }) ?? 0)
    print((task.any { slow_six(), 5 }) ?? 0)
}
"#;

const EXPECTED: &str = "\
7
9
[1, 2]
4
5
";

const NESTED_SOURCE: &str = r#"
use core.time as time

fn slow_one() => Int {
    time.sleep(100)
    return 1
}

fn slow_three() => Int {
    time.sleep(100)
    return 3
}

fn run() {
    task.group workers {
        nested :: task.all {
            task.race { slow_one(), 2 } ?? 0,
            task.any { slow_three(), 4 } ?? 0
        } ?? []
        print(nested)
    }
}
"#;

const LOSER_CLEANUP_SOURCE: &str = r#"
use core.time as time

fn win(state: Shared<[Int]>) => Int {
    state.edit((value: [Int]) => value[0] = 1)
    return 1
}

fn lose(state: Shared<[Int]>) => Int {
    time.sleep(100)
    state.edit((value: [Int]) => value[0] = 2)
    return 2
}

fn run() {
    race_state :: Shared.new([0])
    any_state :: Shared.new([0])
    task.group workers {
        race_result :: (task.race { win(race_state), lose(race_state) }) ?? 0
        any_result :: (task.any { win(any_state), lose(any_state) }) ?? 0
        time.sleep(200)
        print(race_result)
        print(race_state.read((value: [Int]) => value[0]))
        print(any_result)
        print(any_state.read((value: [Int]) => value[0]))
    }
}
"#;

fn limit_source(limit: i64) -> String {
    format!(
        r#"
use core.tasks as tasks
use core.time as time

fn run() {{
    (sender, receiver) :: tasks.channel<Int>()
    second_sender :: ~sender
    task.group limited(limit: {limit}) {{
        first :: task {{
            sender.send(1)
            time.sleep(25)
            sender.send(2)
        }}
        receiver.receive() ?? 0
        second :: task {{
            second_sender.send(3)
        }}
        print(receiver.receive() ?? 0)
        print(receiver.receive() ?? 0)
        first.join() ?? panic("first task failed")
        second.join() ?? panic("second task failed")
    }}
}}
"#
    )
}

#[test]
fn parser_accepts_one_keyword_task_family() {
    let lexed = jet::Compiler::lex_source(SOURCE);
    assert!(lexed.diagnostics.is_empty(), "lexer diagnostics: {:?}", lexed.diagnostics);
    for spelling in ["task", "all", "race", "any", "group"] {
        assert!(
            lexed.tokens.iter().any(|token| token.text == spelling),
            "canonical task surface must lex `{spelling}`"
        );
    }
    let parsed = jet::Compiler::parse_source(SOURCE);
    assert!(parsed.diagnostics.is_empty(), "parser diagnostics: {:?}", parsed.diagnostics);
}

fn checked_task_bundle_for(
    name: &str,
    source: &str,
) -> (jet::AST::ProgramBundle, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("jet_task_control_frontend_{name}_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, source).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("task source must load");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "sema diagnostics: {errors:?}");
    (bundle, dir)
}

fn checked_task_bundle(name: &str) -> (jet::AST::ProgramBundle, std::path::PathBuf) {
    checked_task_bundle_for(name, SOURCE)
}

fn run_forced_interpreter(name: &str, source: &str) -> String {
    let dir = std::env::temp_dir().join(format!("jet_task_control_interp_{name}_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, source).unwrap();
    let result = match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(exit_code, 0, "forced interpreter: {stderr}");
            assert_eq!(stderr, "", "forced interpreter diagnostics: {stderr}");
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected {name}: {diagnostics:?}");
        }
    };
    let _ = fs::remove_dir_all(dir);
    result
}

#[test]
fn sema_resolves_task_failure_rail_and_core_reachability() {
    let (bundle, dir) = checked_task_bundle("sema");
    assert!(
        bundle.used_core.contains("core.concurrency::task"),
        "canonical task syntax must reach the shared Prelude: {:?}",
        bundle.used_core
    );
    let run = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == "run" => Some(function),
            _ => None,
        })
        .expect("task fixture run function");
    let debug = format!("{run:?}");
    assert!(
        debug.contains("TaskFailure"),
        "sema must publish TaskFailure on the task result rail: {debug}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn sema_resolves_direct_nested_task_control_bindings() {
    // Direct nested task controls bind as branch expressions without wrapper
    // parentheses, across sema, the interpreter, resident JIT, and AOT.
    // Nested combinator inference walks the same branch/type tables as the
    // compiler's command thread. Give this focused frontend proof that normal
    // command-sized stack so the test harness's small worker stack does not
    // turn a valid program into a harness abort.
    thread::Builder::new()
        .name("task-nested-sema".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let (bundle, dir) = checked_task_bundle_for("nested", NESTED_SOURCE);
            assert!(bundle.used_core.contains("core.concurrency::task"));
            assert!(
                jet_jit::resident_jit_safe_bundle(&bundle),
                "nested task source must stay resident-JIT safe: {}",
                jet_jit::resident_jit_safe_bundle_detail(&bundle)
            );
            let _ = fs::remove_dir_all(dir);

            let interpreted = run_forced_interpreter("nested", NESTED_SOURCE);
            assert_eq!(interpreted, "[2, 4]\n");

            let (code, stdout, stderr) =
                run_default_multi("task_nested", "main.jet", &[("main.jet", NESTED_SOURCE)]);
            assert_eq!(code, 0, "resident JIT: {stderr}");
            assert_eq!(stdout, interpreted, "resident JIT output drifted: {stderr}");
            assert!(
                stderr.lines().any(|line| line.contains("tier1 native")),
                "nested task source did not execute in resident JIT: {stderr}"
            );

            if have_rustc() {
                let (code, stdout) = build_and_run("task_nested", NESTED_SOURCE);
                assert_eq!(code, 0);
                assert_eq!(stdout, interpreted, "AOT output drifted");
            }
        })
        .expect("nested sema proof thread")
        .join()
        .expect("nested sema proof must not abort");
}

#[test]
fn tir_lowers_each_canonical_task_combinator_and_group() {
    let (bundle, dir) = checked_task_bundle("tir");
    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("canonical task fixture must lower to TIR");
    let run = program
        .funcs
        .iter()
        .find(|function| function.name == "run")
        .expect("lowered run function");
    let mut groups = 0;
    let mut all = 0;
    let mut race = 0;
    let mut any = 0;
    fn count_expr(expr: &jet::Codegen::TIR::TExpr, all: &mut usize, race: &mut usize, any: &mut usize) {
        match &expr.kind {
            jet::Codegen::TIR::TExprKind::TaskGroupAll { .. } => *all += 1,
            jet::Codegen::TIR::TExprKind::TaskGroupRace { .. } => *race += 1,
            jet::Codegen::TIR::TExprKind::TaskGroupAny { .. } => *any += 1,
            jet::Codegen::TIR::TExprKind::Print(inner)
            | jet::Codegen::TIR::TExprKind::Try { inner, .. }
            | jet::Codegen::TIR::TExprKind::OrFallback { value: inner, .. } => {
                count_expr(inner, all, race, any)
            }
            _ => {}
        }
    }
    for statement in &run.body {
        match statement {
            jet::Codegen::TIR::TStmt::TaskGroup { .. } => groups += 1,
            jet::Codegen::TIR::TStmt::Let { init, .. }
            | jet::Codegen::TIR::TStmt::ExprStmt(init) => count_expr(init, &mut all, &mut race, &mut any),
            _ => {}
        }
    }
    assert_eq!((groups, all, race, any), (1, 1, 1, 1));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn task_control_plane_matches_on_default_run() {
    let (code, stdout, stderr) = run_default_multi("task_control", "main.jet", &[("main.jet", SOURCE)]);
    assert_eq!(code, 0, "default `jet run` must succeed\n{stdout}\n{stderr}");
    // The canonical keyword and nested combinators must stay on one path.
    assert!(
        !stderr.contains("E0956"),
        "no tier gap may reach the surface for the ratified set\n{stderr}"
    );
    assert_eq!(strip_tier_trace(&stdout), EXPECTED, "stderr:\n{stderr}");
}

#[test]
fn task_control_plane_matches_under_aot() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("task_control", SOURCE);
    assert_eq!(code, 0, "AOT build must run\n{stdout}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn explicit_group_limit_matches_aot_jit_and_interpreter() {
    for limit in [1, 0, -3] {
        let source = limit_source(limit);
        let expected = "2\n3\n";

        if have_rustc() {
            let (aot_code, aot_stdout) =
                build_and_run(&format!("task_group_limit_{limit}"), &source);
            assert_eq!(aot_code, 0);
            assert_eq!(aot_stdout, expected);
        }

        let (jit_code, jit_stdout, jit_stderr) = run_default_multi(
            &format!("task_group_limit_{limit}"),
            "main.jet",
            &[("main.jet", source.as_str())],
        );
        assert_eq!(jit_code, 0, "{jit_stderr}");
        assert_eq!(jit_stdout, expected, "resident JIT drifted for limit {limit}: {jit_stderr}");

        let interpreted = run_forced_interpreter(&format!("limit_{limit}"), &source);
        assert_eq!(interpreted, expected, "forced interpreter drifted for limit {limit}");
    }
}

#[test]
fn task_combinators_cancel_losers_on_every_tier() {
    let expected = "1\n1\n1\n1\n";
    let interpreted = run_forced_interpreter("loser_cleanup", LOSER_CLEANUP_SOURCE);
    assert_eq!(interpreted, expected);

    let (code, stdout, stderr) =
        run_default_multi("task_loser_cleanup", "main.jet", &[("main.jet", LOSER_CLEANUP_SOURCE)]);
    assert_eq!(code, 0, "resident JIT: {stderr}");
    assert_eq!(stdout, expected, "resident JIT output drifted: {stderr}");
    assert!(
        stderr.lines().any(|line| line.contains("tier1 native")),
        "loser-cleanup source did not execute in resident JIT: {stderr}"
    );

    if have_rustc() {
        let (code, stdout) = build_and_run("task_loser_cleanup", LOSER_CLEANUP_SOURCE);
        assert_eq!(code, 0);
        assert_eq!(stdout, expected);
    }
}

/// `run_default_multi` passes `--trace-tiers`, which may prefix diagnostics of
/// its own on stdout. Keep only the program's transcript.
fn strip_tier_trace(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !line.starts_with("[tier"))
        .map(|line| format!("{line}\n"))
        .collect()
}
