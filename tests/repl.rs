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
    let inputs = &["x :: 10", "x * 2"];
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
    let out = run_transcript(&["_ignored :: 42"], None);
    assert!(out.trim().is_empty(), "expected no output, got: {:?}", out);
}

#[test]
fn repl_reset_clears_bindings() {
    let inputs = &["x :: 10", ":reset", ":type x"];
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
    let inputs = &["y :: 42", ":type y"];
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
fn repl_notebook_turn_controls() {
    let out = run_transcript(
        &[
            "1 + 2",
            ":turns",
            ":pin 1",
            ":fold 1",
            ":turns",
            ":unfold 1",
            ":unpin 1",
            ":rerun 1",
        ],
        None,
    );
    assert!(out.contains("#1 ok"), "got: {out:?}");
    assert!(out.contains("turn pinned"), "got: {out:?}");
    assert!(out.contains("turn folded"), "got: {out:?}");
    assert!(out.contains("#1 ok pinned folded"), "got: {out:?}");
    assert!(out.contains("rerun #1: 1 + 2"), "got: {out:?}");
    assert!(out.matches("3 : Int").count() >= 2, "got: {out:?}");
}

#[test]
fn repl_question_name_shows_type() {
    let out = run_transcript(&["answer :: 42", ":? answer"], None);
    assert!(out.contains("answer: Int :: 42"), "got: {out:?}");
}

#[test]
fn repl_strips_terminal_control_bytes() {
    let out = run_transcript(&["\x0e1 + 2"], None);
    assert_eq!(out.trim(), "3 : Int");
    assert!(
        !out.contains('\x0e'),
        "control byte leaked into transcript: {:?}",
        out
    );
}

#[test]
fn repl_load_hello() {
    // Load examples/features/basics/hello.jet and check it runs.
    let out = run_transcript(&[":load examples/features/basics/hello.jet"], None);
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
    // `t :: s` moves s (String is non-scalar). A later reference to s must error.
    let inputs = &["s :: \"hello\"", "t :: s", "s"];
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
    let inputs = &["x :: \"hi\"", "y :: x; x"];
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
    // Int is a scalar — `t :: s` where s is an Int does NOT move s.
    let inputs = &["s :: 42", "t :: s", "s"];
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
    let out = run_transcript(&["s :: \"hello\"", "t :: s", "s"], None);
    eprintln!("MOVE_STRING: {:?}", out);

    // Move: Int binding s copied into t, then use of s
    let out = run_transcript(&["s :: 42", "t :: s", "s"], None);
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
    let out = run_transcript(&["use core.math as math", "r :: math.sqrt(16.0)"], None);
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
        &["use core.math as math", "x :: 1", "r :: math.sqrt(9.0)"],
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
    // Native-only modules (HTTP, DB, …) hard-reject with E1802 before import.
    let out = run_transcript(&["use core.http.client as http"], None);
    assert!(
        out.contains("E1802"),
        "core.http.client should hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_infinite_loop_hits_e1801_fuel_cap() {
    let out = run_transcript(&["loop { }"], None);
    assert!(out.contains("Error [E1801]:"), "missing REPL fuel diagnostic:\n{out}");
    assert!(out.contains("Why:"), "missing E1801 reason:\n{out}");
    assert!(out.contains("Fix:"), "missing E1801 fix:\n{out}");
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cli/repl_e1801.txt");
    if std::env::var_os("UPDATE_EXPECT").is_some() {
        std::fs::write(&path, &out).unwrap();
    }
    assert_eq!(out, std::fs::read_to_string(path).unwrap());
}

#[test]
fn repl_use_core_fs_import_accepted() {
    let out = run_transcript(&["use core.files as fs"], None);
    assert!(
        out.contains("ok"),
        "core.files import should work, got: {:?}",
        out
    );
    assert!(!out.contains("E1802"), "got: {:?}", out);
}

#[test]
fn repl_reset_clears_imports() {
    // After :reset, a carried import is gone — the alias no longer resolves.
    let out = run_transcript(
        &["use core.math as math", ":reset", "r :: math.sqrt(4.0)"],
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
    let inputs = &["use core.math as math", "r :: math.sqrt(9.0)", "r"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("3") && !out.contains("E0956"),
        "binding from math.sqrt should work, got: {:?}",
        out
    );
}

#[test]
fn repl_core_io_eprint_inline() {
    // c133: Tier-2 core.io calls run in the REPL sandbox (no #Impure gate).
    let inputs = &["use core.io as io", "io.eprint(\"repl-err\")"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("ok") && out.contains("repl-err"),
        "io.eprint should run inline, got: {:?}",
        out
    );
    assert!(
        !out.contains("E3410") && !out.contains("E0956"),
        "got: {:?}",
        out
    );
}

#[test]
fn repl_core_json_parse_inline() {
    let inputs = &[
        "use core.encoding.json as json",
        "json.to_string(json.parse(\"[42]\") ?? panic(\"bad\"))",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("42") && !out.contains("E0956"),
        "json.parse/to_string should work inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_path_join_inline() {
    let inputs = &["use core.path as path", "path.join(\"a\", \"b\")"];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("a/b") || out.contains("a\\b"),
        "path.join should work, got: {:?}",
        out
    );
}

#[test]
fn repl_core_regex_is_match_inline() {
    let inputs = &[
        "use core.regex as re",
        "re.is_match(\"\\\\d+\", \"order 42\") ?? panic(\"bad\")",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("true") && !out.contains("E0956"),
        "regex.is_match should work inline, got: {:?}",
        out
    );
}

#[test]
fn repl_core_fs_read_inline() {
    let fixture = std::env::temp_dir().join(format!(
        "jet_repl_fs_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&fixture, "repl-fs-payload").expect("write fixture");
    let path = fixture.to_string_lossy().to_string();
    let read_expr = format!(
        "fs.read(\"{}\") ?? panic(\"read failed\")",
        path.replace('\\', "\\\\")
    );
    let inputs = &["use core.files as fs", &read_expr];
    let out = run_transcript(inputs, None);
    std::fs::remove_file(&fixture).ok();
    assert!(
        out.contains("repl-fs-payload") && !out.contains("E1802"),
        "fs.read should work in REPL, got: {:?}",
        out
    );
}

#[test]
fn repl_http_client_import_hard_rejected() {
    let out = run_transcript(&["use core.http.client as Client"], None);
    assert!(
        out.contains("E1802"),
        "HTTP client import should hard-reject, got: {:?}",
        out
    );
}

#[test]
fn repl_mut_binding_allows_reassign() {
    // D-BIND4: `:=` bindings must stay mutable across REPL inputs for sema.
    let out = run_transcript(&["hi := 2", "hi = hi * 2", "hi"], None);
    assert!(
        !out.contains("E0111") && !out.contains("made with"),
        "mutable REPL binding should allow `=`, got: {:?}",
        out
    );
    assert!(out.contains("4"), "hi * 2 should be 4, got: {:?}", out);
}

#[test]
fn repl_immut_binding_rejects_reassign() {
    let out = run_transcript(&["hi :: 2", "hi = hi * 2"], None);
    assert!(
        out.contains("E0111"),
        "immutable `::` binding should reject `=`, got: {:?}",
        out
    );
}

// ── D-FE-REPL1=D: hybrid REPL — turn gutter, fold, pin rail, docs, rerun ───
//
// The raw-mode TTY event loop (`Source/REPL/Interactive.rs`) isn't
// exercised here (no real pty in the test harness — see
// `Source/REPL/Terminal.rs` for the raw-mode guard/key-decoder unit tests
// instead). What IS tested here, against the same `pub` API the interactive
// loop calls, is every decision that shapes the interactive UX: the turn
// gutter, the auto-fold threshold, the pin rail, `?name` docs, and the
// rerun replay plan (D-FE-REPL-RERUN1=A) — plus the bare `?name` transcript
// path (D-FE-REPL-DOCS1=B), which runs in every mode including this one.

use jet::REPL::{Docs, Render, RerunPlan, ReplTurn, ReplTurnStatus, Session};

#[test]
fn banner_wording_matches_hybrid_ratification() {
    // D-FE-REPL1=D ratified banner text (tests/cli/no_args_repl_banner.txt
    // pins the same wording end-to-end through the real `jet` binary).
    let banner = Render::render_banner("1.0.0", false);
    assert_eq!(banner, "Jet 1.0.0 — interactive REPL  (:quit, :help, ^B bindings)");
}

#[test]
fn interactive_prompt_carries_a_turn_gutter() {
    // `1 user> `, `2 user> `, … — the dim one-character-cost gutter that
    // distinguishes the interactive TTY prompt from the plain `user> ` the
    // non-TTY floor keeps (see `no_args_repl_banner_golden` in tests/cli.rs).
    assert_eq!(Render::render_prompt(1, false), "1 user> ");
    assert_eq!(Render::render_prompt(2, false), "2 user> ");
    assert_ne!(Render::render_prompt(1, false), "user> ");
}

#[test]
fn long_list_folds_past_threshold_short_list_does_not() {
    let short = jet::AST::CtValue::List((0..5).map(jet::AST::CtValue::Int).collect());
    assert!(
        Render::fold_decision_for_value(&short).is_none(),
        "a short list must print plainly, not fold"
    );

    let long = jet::AST::CtValue::List((0..42).map(jet::AST::CtValue::Int).collect());
    let (count, elem_ty) = Render::fold_decision_for_value(&long).expect("long list should fold");
    assert_eq!(count, 42);
    let marker = Render::render_fold_marker(count, &elem_ty, false);
    assert_eq!(marker, "⋯ 42 rows folded · [Int] · unfold ⏎");
}

#[test]
fn pin_rail_renders_binding_name_type_value_and_unpin_hint() {
    let rail = Render::render_pin_rail("total: Int :: 15", 3, 62, false);
    let head = rail.lines().next().unwrap();
    assert!(head.contains("total: Int :: 15"), "got: {head:?}");
    assert!(head.contains("turn 3"), "got: {head:?}");
    assert!(head.contains("unpin"), "got: {head:?}");
    assert!(head.contains("^P"), "got: {head:?}");
}

#[test]
fn docs_lookup_builtin_list_filter_matches_ratified_mock() {
    // D-FE-REPL-DOCS1=B ratified example: `?List.filter`.
    let session = Session::new();
    let doc = Docs::lookup(&session, "List.filter").expect("List.filter has builtin docs");
    assert!(
        doc.starts_with("List.filter(f: fn(T) -> Bool) -> List<T>"),
        "got: {doc:?}"
    );
    assert!(doc.contains("Keeps items where f(item) is true."), "got: {doc:?}");
    assert!(doc.contains("Source:"), "got: {doc:?}");
}

#[test]
fn docs_lookup_local_binding_shows_live_value() {
    let mut session = Session::new();
    session.scope.insert("answer".to_string(), jet::AST::CtValue::Int(42));
    let doc = Docs::lookup(&session, "answer").expect("bound name should resolve");
    assert_eq!(doc, "answer: Int :: 42\n");
    session.mutable_names.insert("answer".to_string());
    let doc = Docs::lookup(&session, "answer").expect("bound name should resolve");
    assert_eq!(doc, "answer: Int := 42\n");
}

#[test]
fn bare_question_name_is_the_primary_docs_spelling() {
    // D-FE-REPL-DOCS1=B: bare `?name` (no colon) is the primary spelling;
    // `:? name` stays an accepted alias to the same lookup.
    let bare = run_transcript(&["answer :: 42", "?answer"], None);
    let colon = run_transcript(&["answer :: 42", ":? answer"], None);
    assert!(bare.contains("answer: Int :: 42"), "got: {bare:?}");
    assert!(colon.contains("answer: Int :: 42"), "got: {colon:?}");
}

#[test]
fn bare_question_mark_alone_shows_help_like_colon_form() {
    let out = run_transcript(&["?"], None);
    assert!(out.contains("REPL meta-commands"), "got: {out:?}");
}

fn fixture_turn(id: usize, input: &str, had_effect: bool, bound_name: Option<&str>) -> ReplTurn {
    ReplTurn {
        id,
        input: input.to_string(),
        summary: String::new(),
        status: ReplTurnStatus::Ok,
        folded: false,
        pinned: false,
        had_effect,
        bound_name: bound_name.map(str::to_string),
        pending_unfold: None,
    }
}

#[test]
fn rerun_plan_gates_effectful_turns_and_lets_pure_turns_auto_replay() {
    // D-FE-REPL-RERUN1=A ratified shape: editing turn 1 replays turn 1 and
    // every turn after it; pure/binding turns replay automatically, a turn
    // that had an effect the first time needs a `y`/`N` confirmation.
    let turns = vec![
        fixture_turn(1, "rate :: 0.07", false, Some("rate")),
        fixture_turn(
            2,
            "invoice_total :: subtotal * (1.0 + rate)",
            false,
            Some("invoice_total"),
        ),
        fixture_turn(3, "write_file(\"out.txt\", invoice_total)", true, None),
    ];

    let plan = RerunPlan::build_replay_plan(&turns, 1, Some("rate :: 0.08")).expect("plan");
    assert_eq!(plan.steps.len(), 3);
    assert_eq!(plan.steps[0].input, "rate :: 0.08", "edited turn's text is substituted");
    assert!(RerunPlan::plan_needs_confirmation(&plan), "the write_file step needs confirmation");

    let rendered = RerunPlan::render_replay_plan(&plan, false);
    assert!(rendered.starts_with("Replay plan:\n"), "got: {rendered:?}");
    assert!(rendered.contains("auto"), "got: {rendered:?}");
    assert!(rendered.contains("confirm effect"), "got: {rendered:?}");
    assert!(rendered.trim_end().ends_with("Apply? [y/N]"), "got: {rendered:?}");

    // A plan with no effectful steps needs no confirmation at all.
    let pure_only = RerunPlan::build_replay_plan(&turns, 1, None).map(|mut p| {
        p.steps.truncate(2);
        p
    });
    assert!(!RerunPlan::plan_needs_confirmation(&pure_only.unwrap()));
}

#[test]
fn rerun_plan_rejects_unknown_turn_id() {
    let turns = vec![fixture_turn(1, "x :: 1", false, Some("x"))];
    assert!(RerunPlan::build_replay_plan(&turns, 99, None).is_err());
}

#[test]
fn textual_rerun_fallback_still_previews_a_turn() {
    // `run_transcript` is the non-TTY floor — it keeps the pre-redesign
    // `:rerun <id>` preview shape byte-for-byte (same assertion
    // `repl_notebook_turn_controls` already pins for `:turns`/`:pin`/`:fold`).
    // The plan-based replay (`Replay plan: … Apply? [y/N]`) is the cooked/
    // interactive-loop behavior (`Source/REPL/mod.rs::handle_meta`, exercised
    // by `RerunPlan`'s own unit/integration tests above) — `run_transcript`
    // doesn't route through it, by design (I6/floor: no pty in this harness).
    let out = run_transcript(&["1 + 2", ":rerun 1"], None);
    assert!(out.contains("rerun #1: 1 + 2"), "got: {out:?}");
    assert!(out.matches("3 : Int").count() >= 2, "got: {out:?}");
}

// ── D-BIGINT1: comptime/REPL tier-0 BigInt (card #392) ─────────────────────
// Same expression shapes and expected decimal strings as the AOT golden
// example `examples/features/text/bigint.jet` /
// `examples/features/expected/text/bigint.out` — proves R12 parity, not just
// "doesn't crash".

#[test]
fn repl_bigint_construct_and_show() {
    let out = run_transcript(&["BigInt(100)"], None);
    assert!(out.trim().ends_with("100 : BigInt"), "got: {out:?}");
    assert!(!out.contains("E0956"), "got: {out:?}");
}

#[test]
fn repl_bigint_add_beyond_i64_max() {
    // max_i64 + 1 overflows a fixed Int; BigInt must not (that's the whole
    // point of D-BIGINT1 — no silent promotion, no overflow trap either).
    let inputs = &[
        "max_i64 :: BigInt(9223372036854775807)",
        "one :: BigInt(1)",
        "sum :: max_i64 + one",
        "sum.to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("\"9223372036854775808\" : String"),
        "got: {out:?}"
    );
}

#[test]
fn repl_bigint_from_string_add() {
    let inputs = &[
        "huge :: BigInt(\"999999999999999999999999999999\")",
        "doubled :: huge + huge",
        "doubled.to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains("\"1999999999999999999999999999998\" : String"),
        "got: {out:?}"
    );
}

#[test]
fn repl_bigint_mul_massive() {
    let inputs = &[
        "massive :: BigInt(\"239238749287396287369263958629386592683596293856293659263596235962935869236592863592635962395862935629368592635926359623956293659236592635986239562936592635962395629365926359863892659826398568639269856\")",
        "massdoub :: massive * massive",
        "massdoub.to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        out.contains(
            "\"57235179160597657597531047020200956544362763118771376009314169310643907927975523987347975666104048258728304782749129351109033383492052353630602575371754435856735360169485983219218233794801194422942999175714144363822988057872537938863053251946320370964445676871073768260912400343608038289822197795736070764739253602318976703277723078277315814109217255310422643607649043148897685808219536598364790260736\" : String"
        ),
        "got: {out:?}"
    );
}

#[test]
fn repl_bigint_sub_and_neg() {
    let inputs = &[
        "a :: BigInt(5)",
        "b :: BigInt(3)",
        "a.sub(b).to_string()",
        "b.neg().to_string()",
    ];
    let out = run_transcript(inputs, None);
    assert!(out.contains("\"2\" : String"), "got: {out:?}");
    assert!(out.contains("\"-3\" : String"), "got: {out:?}");
}

// ── card #392: `core.random` widened ambient draws now dispatch at comptime ──
// (`bool`/`float_range`/`normal`/`exponential`/`bytes`/`pick`/`weighted_pick`/
// `sample`/`shuffle`/`split` — same SplitMix64 stream as AOT's `jet_std_random_*`
// (Process.rs), so a seeded transcript reproduces AOT's numbers exactly, R12).

#[test]
fn repl_core_random_widened_draws_dispatch() {
    let inputs = &[
        "use core.random as random",
        "random.seed(1)",
        "random.bool(0.5)",
        "random.float_range(1.0, 2.0)",
        "random.normal(0.0, 1.0)",
        "random.exponential(1.0)",
        "random.bytes(3)",
        "xs :: [1, 2, 3, 4, 5]",
        "random.pick(xs)",
        "random.sample(xs, 2)",
        "random.weighted_pick(xs, [1.0, 1.0, 1.0, 1.0, 1.0])",
        "ys := [1, 2, 3, 4, 5]",
        "random.shuffle(&ys)",
        "ys",
        "random.split(7)",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.random widened draws should dispatch at comptime, got: {out}"
    );
    // Deterministic from seed 1 — same SplitMix64 stream as AOT's
    // `jet_std_random_*` (Process.rs), so these numbers pin both the dispatch
    // AND the algorithm (a wrong constant would silently diverge from AOT).
    assert!(out.contains("false : Bool"), "got: {out}");
    assert!(out.contains("1.5516928307103677 : Float"), "got: {out}");
    assert!(out.contains("-0.04268100696719249 : Float"), "got: {out}");
    assert!(out.contains("0.6199045686318072 : Float"), "got: {out}");
    assert!(out.contains("[116, 44, 76] : [U8]"), "got: {out}");
    assert!(out.contains("5 : Option"), "got: {out}");
    assert!(out.contains("[4, 5] : List"), "got: {out}");
    assert!(out.contains("4 : Option"), "got: {out}");
    assert!(out.contains("[3, 2, 1, 5, 4] : List"), "got: {out}");
    assert!(out.contains("Rng(state: -6449093003948547574) : Struct"), "got: {out}");
}

// ── card #392: `core.fmt` (pure text formatting) now dispatches at comptime,
// byte-identical to AOT's `jet_fmt_*` (DataFmt.rs).

#[test]
fn repl_core_fmt_dispatch() {
    let inputs = &[
        "use core.fmt as fmt",
        "fmt.number(1234567)",
        "fmt.decimal(3.14159, 2)",
        "fmt.percent(0.4567, 1)",
        "fmt.bytes(1500000)",
        "fmt.duration(93784000)",
        "fmt.ordinal(23)",
        "fmt.plural(1, \"item\", \"items\")",
        "fmt.plural(3, \"item\", \"items\")",
        "fmt.pad_left(\"7\", 3, \"0\")",
        "fmt.pad_right(\"ab\", 5, \".\")",
        "fmt.pad_center(\"hi\", 6, \"*\")",
    ];
    let out = run_transcript(inputs, None);
    assert!(!out.contains("E0956"), "core.fmt should dispatch at comptime, got: {out}");
    assert!(out.contains("\"1,234,567\" : String"), "got: {out}");
    assert!(out.contains("\"3.14\" : String"), "got: {out}");
    assert!(out.contains("\"45.7%\" : String"), "got: {out}");
    assert!(out.contains("\"1.5 MB\" : String"), "got: {out}");
    assert!(out.contains("\"1d 2h 3m\" : String"), "got: {out}");
    assert!(out.contains("\"23rd\" : String"), "got: {out}");
    assert!(out.contains("\"1 item\" : String"), "got: {out}");
    assert!(out.contains("\"3 items\" : String"), "got: {out}");
    assert!(out.contains("\"007\" : String"), "got: {out}");
    assert!(out.contains("\"ab...\" : String"), "got: {out}");
    assert!(out.contains("\"**hi**\" : String"), "got: {out}");
}

// ── card #392: `core.encoding.base32` and base64's URL-safe variant now
// dispatch at comptime, byte-identical to AOT's `jet_std_base32_*` /
// `jet_std_b64url_*` (EncodingCodecs.rs).

#[test]
fn repl_core_encoding_base32_and_base64url_dispatch() {
    let inputs = &[
        "use core.encoding.base32 as base32",
        "use core.encoding.base64 as base64",
        "base32.encode([104, 101, 108, 108, 111])",
        "base32.decode(\"NBSWY3DP\")",
        "base64.encode_url([251, 239, 190])",
    ];
    let out = run_transcript(inputs, None);
    assert!(
        !out.contains("E0956"),
        "core.encoding.base32/base64 url variant should dispatch at comptime, got: {out}"
    );
    assert!(out.contains("\"NBSWY3DP\" : String"), "got: {out}");
    assert!(
        out.contains("[104, 101, 108, 108, 111] : Result"),
        "got: {out}"
    );
    assert!(out.contains("\"----\" : String"), "got: {out}");
}
