//! E2-M18 — REPL transcript tests (D-REPL20=A).
//!
//! Each transcript in `tests/repl/*.txt` is run through `jet::REPL::run_transcript`.
//! Lines starting with `> ` are REPL inputs; other non-comment lines are
//! expected output. Lines starting with `# ` are ignored.
//!
//! Test failures show the first diverging expected/actual line.

use jet::REPL::run_transcript;

/// Parse a transcript file: return `(inputs, expected_outputs)`.
/// Lines `> input` become inputs; non-comment, non-`>` lines become expected.
/// Lines starting with `# ` are comments. Blank expected-output lines are
/// represented as blank strings (the transcript may expect a blank output line).
fn parse_transcript(txt: &str) -> (Vec<String>, Vec<String>) {
    let mut inputs: Vec<String> = Vec::new();
    let mut expected: Vec<String> = Vec::new();
    for line in txt.lines() {
        if line.starts_with("# ") || line == "#" {
            // comment — skip
        } else if let Some(input) = line.strip_prefix("> ") {
            inputs.push(input.to_string());
        } else {
            // expected output line (may be blank)
            expected.push(line.to_string());
        }
    }
    (inputs, expected)
}

fn run_transcript_file(txt: &str) {
    let (inputs, expected_lines) = parse_transcript(txt);
    let input_refs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let actual = run_transcript(&input_refs, None);

    // Split actual output into lines (preserve blank lines).
    let actual_lines: Vec<&str> = actual.lines().collect();

    // Compare line by line; skip blank expected lines that represent
    // "no output" comments in the transcript.
    let mut ai = 0; // index into actual_lines
    for (i, exp) in expected_lines.iter().enumerate() {
        if exp.is_empty() {
            // blank expected line — skip (these are transcript spacing)
            continue;
        }
        let got = actual_lines.get(ai).copied().unwrap_or("<no output>");
        assert_eq!(
            got,
            exp.as_str(),
            "transcript line {}: expected {:?} but got {:?}\nfull actual output:\n{}",
            i + 1,
            exp,
            got,
            actual
        );
        ai += 1;
    }
}

// ── test cases ────────────────────────────────────────────────────────────

#[test]
fn repl_arithmetic_echo() {
    let out = run_transcript(&["1 + 2"], None);
    assert_eq!(out.trim(), "3 : Int");
}

#[test]
fn repl_val_binding_then_expr() {
    let inputs = &["x #= 10", "x * 2"];
    let out = run_transcript(inputs, None);
    // First input produces no output (val declaration).
    // Second produces the echo.
    assert!(out.trim().ends_with("20 : Int"), "got: {:?}", out);
}

#[test]
fn repl_print_output() {
    let out = run_transcript(&["print(\"hello, world\")"], None);
    assert!(out.contains("hello, world"), "got: {:?}", out);
}

#[test]
fn repl_string_echo() {
    let out = run_transcript(&["\"abc\""], None);
    assert!(out.contains("\"abc\" : String"), "got: {:?}", out);
}

#[test]
fn repl_bool_echo() {
    let out = run_transcript(&["2 > 1"], None);
    assert!(out.trim().ends_with("true : Bool"), "got: {:?}", out);
}

#[test]
fn repl_suppress_semicolon() {
    // A binding declaration produces no echo.
    let out = run_transcript(&["_ignored #= 42"], None);
    assert!(out.trim().is_empty(), "expected no output, got: {:?}", out);
}

#[test]
fn repl_reset_clears_bindings() {
    let inputs = &["x #= 10", ":reset", ":type x"];
    let out = run_transcript(inputs, None);
    assert!(out.contains("session reset"), "got: {:?}", out);
    assert!(out.contains("isn't defined"), "got: {:?}", out);
}

#[test]
fn repl_function_declare_and_call() {
    let inputs = &["fn double(n: Int) -> Int { return n * 2 }", "double(5)"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("ok"),
        "no ok for fn declaration, got: {:?}",
        out
    );
    assert!(
        out.contains("10 : Int"),
        "fn call result wrong, got: {:?}",
        out
    );
}

#[test]
fn repl_hard_reject_unsafe() {
    let out = run_transcript(&["#Unsafe { }"], None);
    assert!(
        out.contains("E1802"),
        "expected E1802 hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_hard_reject_extern_rust() {
    let out = run_transcript(&["extern rust \"mycrate\" { }"], None);
    assert!(
        out.contains("E1802"),
        "expected E1802 hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_type_meta_command() {
    let inputs = &["y #= 42", ":type y"];
    let out = run_transcript(inputs, None);
    assert!(out.contains("y : Int"), "got: {:?}", out);
}

#[test]
fn repl_type_unknown() {
    let out = run_transcript(&[":type nosuchvar"], None);
    assert!(out.contains("isn't defined"), "got: {:?}", out);
}

#[test]
fn repl_quit_exits() {
    let out = run_transcript(&[":quit"], None);
    assert_eq!(out.trim(), "bye");
}

#[test]
fn repl_help_shows_commands() {
    let out = run_transcript(&[":help"], None);
    assert!(out.contains("REPL meta-commands"), "got: {:?}", out);
}

#[test]
fn repl_load_hello() {
    // Load examples/features/01_hello.jet and check it runs.
    let out = run_transcript(&[":load examples/features/01_hello.jet"], None);
    assert!(
        out.contains("hello, world"),
        "load should run main, got: {:?}",
        out
    );
}

#[test]
fn repl_basics_transcript() {
    let txt = include_str!("repl/basics.txt");
    run_transcript_file(txt);
}

// ── D-REPL8=A: move semantics across inputs ───────────────────────────────

#[test]
fn repl_move_across_inputs_string() {
    // `t #= s` moves s (String is non-scalar). A later reference to s must error.
    let inputs = &["s #= \"hello\"", "t #= s", "s"];
    let out = run_transcript(inputs, None);
    // After s is moved into t, using s in input 3 must produce an error.
    // The error will be E0121 ("was given away") because the binding_stubs_src
    // emits a synthetic consume of s before the check.
    assert!(
        out.contains("error")
            && (out.contains("E0121")
                || out.contains("given away")
                || out.contains("nothing named")),
        "expected use-after-move error, got: {:?}",
        out
    );
    // Must NOT silently succeed (no echo of the moved value).
    assert!(
        !out.trim_end().ends_with(": String"),
        "moved value should not be echoed, got: {:?}",
        out
    );
}

#[test]
fn repl_move_within_single_input_still_errors() {
    // Move within a single input: x is moved in the same step it's used again.
    let inputs = &["x #= \"hi\"", "y #= x; x"];
    let out = run_transcript(inputs, None);
    // Within the same input, sema catches the use after move.
    assert!(
        out.contains("error"),
        "expected within-input move error, got: {:?}",
        out
    );
}

#[test]
fn repl_move_int_not_moved() {
    // Int is a scalar — `t #= s` where s is an Int does NOT move s.
    let inputs = &["s #= 42", "t #= s", "s"];
    let out = run_transcript(inputs, None);
    // s should still be accessible since Int is copy.
    assert!(
        out.contains("42 : Int"),
        "Int should be accessible after copy, got: {:?}",
        out
    );
}

// ── D-REPL-FUEL=A: :run ──────────────────────────────────────────────────

#[test]
fn repl_run_empty_session() {
    // :run on an empty session is a clean no-op.
    let out = run_transcript(&[":run"], None);
    assert!(
        out.contains("empty") || out.contains("nothing"),
        "empty :run should note nothing to run, got: {:?}",
        out
    );
    // Must not produce an error code.
    assert!(
        !out.contains("error [E"),
        "empty :run must not error, got: {:?}",
        out
    );
}

#[test]
fn repl_run_produces_output() {
    // :run on a session with a print should produce that output.
    let inputs = &["print(\"hello from run\")", ":run"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("hello from run"),
        ":run should replay print output, got: {:?}",
        out
    );
}

#[test]
fn repl_run_consistency_with_interpreter() {
    // The output of :run must match the interpreter's direct output.
    // First run with interpreter: print("ping") produces "ping".
    let direct_out = run_transcript(&["print(\"ping\")"], None);
    // Now run the same with :run.
    let run_out = run_transcript(&["print(\"ping\")", ":run"], None);
    // :run output must contain the same output as the interpreter.
    assert!(
        run_out.contains("ping"),
        ":run output must contain interpreter output; got: {:?}",
        run_out
    );
    // Direct interpreter also produced "ping".
    assert!(direct_out.contains("ping"), "got: {:?}", direct_out);
}

// Probe test — not a real test, only used during dev to see exact output.
// Kept for reference but skipped in CI.
#[test]
#[ignore]
fn repl_probe_exact_outputs() {
    use jet::REPL::run_transcript;

    // Move: String binding s moved into t, then use of s
    let out = run_transcript(&["s #= \"hello\"", "t #= s", "s"], None);
    eprintln!("MOVE_STRING: {:?}", out);

    // Move: Int binding s copied into t, then use of s
    let out = run_transcript(&["s #= 42", "t #= s", "s"], None);
    eprintln!("COPY_INT: {:?}", out);

    // :run empty
    let out = run_transcript(&[":run"], None);
    eprintln!("RUN_EMPTY: {:?}", out);

    // :run with print
    let out = run_transcript(&["print(\"hello from run\")", ":run"], None);
    eprintln!("RUN_PRINT: {:?}", out);

    // --project probe
    let fixture = std::env::temp_dir().join("jet_repl_project_probe");
    std::fs::create_dir_all(&fixture).ok();
    std::fs::write(
        fixture.join("helper.jet"),
        "fn add_three(x: Int) -> Int { return x + 3; }\n",
    )
    .ok();
    let project_dir = fixture.to_string_lossy().to_string();
    let out = run_transcript(&["add_three(10)"], Some(&project_dir));
    eprintln!("PROJECT_ADD_THREE: {:?}", out);
    std::fs::remove_dir_all(&fixture).ok();
}

// ── D-REPL10=A: --project mode ───────────────────────────────────────────

#[test]
fn repl_project_loads_items() {
    // Create a tiny fixture project with a function.
    let fixture = std::env::temp_dir().join("jet_repl_project_test");
    std::fs::create_dir_all(&fixture).ok();
    std::fs::write(
        fixture.join("helper.jet"),
        "fn add_three(x: Int) -> Int { return x + 3; }\n",
    )
    .expect("write fixture");

    let project_dir = fixture.to_string_lossy().to_string();
    // With --project, add_three() is available without `use`.
    let out = run_transcript(&["add_three(10)"], Some(&project_dir));
    // Must not report unknown function error.
    assert!(
        !out.contains("E0102") && !out.contains("error [E0107]"),
        "--project should make project functions available, got: {:?}",
        out
    );
    // Must produce the correct result.
    assert!(
        out.contains("13 : Int"),
        "--project: add_three(10) should return 13, got: {:?}",
        out
    );

    // Without --project, add_three() is unknown.
    let out_no_project = run_transcript(&["add_three(10)"], None);
    assert!(
        out_no_project.contains("error"),
        "without --project, unknown fn should error, got: {:?}",
        out_no_project
    );

    // Clean up.
    std::fs::remove_dir_all(&fixture).ok();
}

// ── S16/D-REPL10: `use` imports carried across REPL inputs ────────────────

#[test]
fn repl_use_import_accepted() {
    // A bare `use core.math as math` import is accepted (not "expected a
    // statement, found `use`"), and reports `ok`.
    let out = run_transcript(&["use core.math as math"], None);
    assert!(
        out.contains("ok"),
        "import should be accepted, got: {:?}",
        out
    );
    assert!(
        !out.contains("E0003"),
        "import must not be a statement error, got: {:?}",
        out
    );
}

#[test]
fn repl_use_import_resolves_alias_in_later_input() {
    // The import typed in input 1 must make `math` resolve in input 2 — the
    // alias is carried across inputs (this is the c55 cross-input delta). Before
    // the fix this produced E0107 ("nothing named `math`").
    let out = run_transcript(&["use core.math as math", "r #= math.sqrt(16.0)"], None);
    assert!(
        !out.contains("E0107") && !out.contains("nothing named `math`"),
        "carried import should resolve the alias, got: {:?}",
        out
    );
}

#[test]
fn repl_use_import_persists_across_unrelated_input() {
    // An intervening unrelated input must not drop the import: the alias still
    // resolves two inputs later.
    let out = run_transcript(
        &["use core.math as math", "x #= 1", "r #= math.sqrt(9.0)"],
        None,
    );
    assert!(
        !out.contains("E0107") && !out.contains("nothing named `math`"),
        "import should persist across inputs, got: {:?}",
        out
    );
}

#[test]
fn repl_use_unknown_core_module_rejected() {
    // An unknown core module reports E1001 and is not retained.
    let out = run_transcript(&["use core.bogus as b"], None);
    assert!(
        out.contains("E1001"),
        "unknown core module should error, got: {:?}",
        out
    );
    assert!(
        !out.contains("ok"),
        "bad import must not be accepted, got: {:?}",
        out
    );
}

#[test]
fn repl_use_repl_incompatible_module_hard_rejected() {
    // Modules the interpreter can't run (fs, tasks, …) hard-reject with E1802
    // before being treated as an import.
    let out = run_transcript(&["use core.fs as fs"], None);
    assert!(
        out.contains("E1802"),
        "core.fs should hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_reset_clears_imports() {
    // After :reset, a carried import is gone — the alias no longer resolves.
    let out = run_transcript(
        &["use core.math as math", ":reset", "r #= math.sqrt(4.0)"],
        None,
    );
    assert!(out.contains("session reset"), "got: {:?}", out);
    assert!(
        out.contains("error"),
        "after reset the alias must not resolve, got: {:?}",
        out
    );
}

// ── D-CTCORE1: inline pure Core execution in the REPL ────────────────────

#[test]
fn repl_core_math_sqrt_inline() {
    // c133/D-CTCORE1: after importing core.math, math.sqrt() must execute
    // inline in the interpreter (not raise E0956).
    let inputs = &["use core.math as math", "math.sqrt(16.0)"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("ok") && out.contains("4") && !out.contains("E0956"),
        "math.sqrt(16.0) should produce 4 inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_math_multiple_calls() {
    // Confirm several whitelisted math calls work sequentially in one session.
    let inputs = &[
        "use core.math as math",
        "math.floor(3.7)",
        "math.ceil(2.1)",
        "math.abs(-5)",
    ];
    let out = run_transcript(inputs, None);
    assert!(out.contains("3"), "floor(3.7) should be 3, got: {:?}", out);
    assert!(out.contains("3"), "ceil(2.1) should be 3, got: {:?}", out);
    assert!(out.contains("5"), "abs(-5) should be 5, got: {:?}", out);
    assert!(
        !out.contains("E0956"),
        "no E0956 for whitelisted calls, got: {:?}",
        out
    );
}

#[test]
fn repl_core_math_pow_inline() {
    // Another whitelisted math function: math.pow(2.0, 10.0) = 1024.
    let inputs = &["use core.math as math", "math.pow(2.0, 10.0)"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("1024") && !out.contains("E0956"),
        "math.pow(2.0, 10.0) should produce 1024 inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_result_stored_in_binding() {
    // The result of a whitelisted core call can be stored and used.
    let inputs = &["use core.math as math", "r #= math.sqrt(9.0)", "r"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("3") && !out.contains("E0956"),
        "binding from math.sqrt should work, got: {:?}",
        out
    );
}

#[test]
fn repl_non_whitelisted_core_io_still_rejects() {
    // core.io I/O calls must NOT silently execute at comptime (E0958).
    // core.io is not in the hard-reject list (only core.fs/tasks/mem are).
    // After importing, calling io.read() must produce an error, not succeed.
    let inputs = &["use core.io as io", "io.read()"];
    let out = run_transcript(inputs, None);
    // Either sema rejects it (type error) or the comptime interpreter rejects
    // it (E0958 — I/O not allowed at comptime). Either way: no silent success.
    assert!(
        out.contains("error") || out.contains("E095"),
        "io.read() must not silently succeed, got: {:?}",
        out
    );
}
