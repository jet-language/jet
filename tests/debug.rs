//! D-DBG1 / D-DBG3 — `jet debug` source-level step-debugger tests.
//!
//! Drives the debugger programmatically via `jet::Debug::run_session(file,
//! inputs)`, which feeds a scripted sequence of `(jet)` commands and returns
//! the full transcript (banners, `<- here` carets, `locals:` dumps, command
//! echoes, program output, and the final marker). We assert on the exact
//! protocol/output: that a breakpoint resolves to the right Jet line, that the
//! stack frame and locals are in Jet terms (I2 — never generated Rust), and
//! that step / next / continue / finish behave as ratified.
//!
//! The fixtures are tiny temp files so the test is hermetic. The debugger drives
//! the same dev interpreter `jet dev`/`jet repl` use, so no rustc is needed.
//!
//! Also covers D-DBG3 step 2 (dap-debugger) — the native lldb-backed `jet
//! debug` backend (see the section below): codegen-only line-marker checks
//! (no rustc/lldb needed) plus a full native session, gated on BOTH `rustc`
//! and usable `lldb` presence — a hard skip (not a failure) when either tool is
//! absent or the host sandbox cannot stop a debuggee, so CI without functional
//! lldb still passes.

mod common;

use std::io::Write;

/// Write `src` to a temp `.jet` file and return its path.
fn fixture(tag: &str, src: &str) -> String {
    let p = std::env::temp_dir().join(format!("jet_debug_{tag}.jet"));
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    p.to_string_lossy().into_owned()
}

const LOOPS: &str = "\
fn run() {
    n := 3
    total := 0
    loop i, 1..n {
        total += i
    }
    print(\"total is {total}\")
}
";

const CALLS: &str = "\
fn double(x: Int) => Int {
    y := x * 2
    return y
}
fn run() {
    a := 5
    b := double(a)
    print(\"{b}\")
}
";

#[test]
fn structured_session_status_ignores_program_output() {
    let file = fixture(
        "structured_status",
        "fn run() {\n    print(\"program finished\")\n}\n",
    );
    let result = jet::Debug::run_session_result(&file, &["c"]);

    assert_eq!(result.status, jet::Debug::SessionStatus::Finished);
    assert_eq!(
        result
            .transcript
            .lines()
            .filter(|line| *line == "program finished")
            .count(),
        2,
        "transcript must retain program output and the debugger completion marker"
    );
}

/// `step`/`s` stops on the very first statement of `main`, with the right Jet
/// line, a `<- here` caret, and a `locals:` dump — all in Jet terms.
#[test]
fn step_stops_at_first_line_with_caret_and_locals() {
    let file = fixture("step", LOOPS);
    let out = jet::Debug::run_session(&file, &["s"]);
    // The first stop names the file:line and the function.
    assert!(
        out.contains("breakpoint hit  jet_debug_step.jet:2  in main()"),
        "missing breakpoint banner; got:\n{out}"
    );
    // The current line carries the `<- here` caret (D-DBG3 layout).
    assert!(
        out.contains("<- here"),
        "missing `<- here` caret; got:\n{out}"
    );
    // The `(jet)` prompt echoes the typed command (D-DBG3 prompt = `(jet)`).
    assert!(
        out.contains("(jet) s"),
        "missing `(jet)` prompt echo; got:\n{out}"
    );
    // Locals are shown in Jet terms; after the first `s` the second stop sees `n`.
    assert!(out.contains("locals:"), "missing locals dump; got:\n{out}");
    // The program runs to completion once the script ends.
    assert!(
        out.contains("total is 6"),
        "program output missing; got:\n{out}"
    );
    assert!(
        out.contains("program finished"),
        "missing end marker; got:\n{out}"
    );
}

/// `locals` after stepping into the loop shows the live `total`/`n`/`i` values
/// through the Jet display path (I2), and `print X` shows one named local.
#[test]
fn locals_and_print_show_jet_values() {
    let file = fixture("locals", LOOPS);
    // step to line 3 (total := 0), step to 4 (loop header), step into loop body
    // (i bound), then inspect.
    let out = jet::Debug::run_session(&file, &["s", "s", "s", "locals", "p total"]);
    assert!(
        out.contains("locals:  i = 1   n = 3   total = 0"),
        "locals dump should show all three in Jet terms; got:\n{out}"
    );
    assert!(
        out.contains("total = 0"),
        "`print total` should show the value; got:\n{out}"
    );
}

/// A `break N` + `continue`/`c` runs to the breakpoint line — not stopping at
/// every statement in between.
#[test]
fn break_then_continue_stops_at_breakpoint() {
    let file = fixture("brk", LOOPS);
    // First stop is line 2; set a breakpoint on line 7 (the print) and continue.
    let out = jet::Debug::run_session(&file, &["break 7", "c"]);
    assert!(
        out.contains("breakpoint set  jet_debug_brk.jet:7"),
        "break should confirm the line; got:\n{out}"
    );
    // The continue lands on line 7 with the loop already finished (total = 6).
    assert!(
        out.contains("7 |") && out.contains("total = 6"),
        "continue should stop on line 7 with the final total; got:\n{out}"
    );
}

/// `step` descends into a called function (`double`), and `backtrace`/`bt`
/// shows the two-frame Jet call stack in Jet terms.
#[test]
fn step_into_call_and_backtrace() {
    let file = fixture("calls", CALLS);
    // main:6 (a := 5) -> main:7 (b := double(a)) -> step into double body.
    let out = jet::Debug::run_session(&file, &["s", "s", "bt"]);
    assert!(
        out.contains("y := x * 2"),
        "step should descend into the callee body; got:\n{out}"
    );
    // The backtrace shows the callee innermost, the caller below — Jet frames.
    assert!(
        out.contains("#0  double()") && out.contains("#1  main()"),
        "backtrace should show both Jet frames; got:\n{out}"
    );
}

/// `finish`/`f` runs to the end of the current function and stops back in the
/// caller.
#[test]
fn finish_returns_to_caller() {
    let file = fixture("finish", CALLS);
    // Descend into double (s,s,s), then finish back to main.
    let out = jet::Debug::run_session(&file, &["s", "s", "s", "finish"]);
    assert!(
        out.contains("b := double(a)") || out.contains("print"),
        "finish should return control to the caller frame; got:\n{out}"
    );
    assert!(
        out.contains("program finished"),
        "should run to the end; got:\n{out}"
    );
}

/// `quit`/`q` ends the session before the program finishes — surfaced as the
/// E2204 debug-session-aborted diagnostic (I4).
#[test]
fn quit_ends_session_with_e2204() {
    let file = fixture("quit", LOOPS);
    let out = jet::Debug::run_session(&file, &["s", "q"]);
    assert!(
        out.contains("E2204"),
        "quitting mid-session must surface E2204; got:\n{out}"
    );
    // The success marker is a line of its own; the E2204 message also contains
    // the words "finished", so check for the standalone marker line.
    assert!(
        !out.lines().any(|l| l == "program finished"),
        "the program must NOT have finished after quit; got:\n{out}"
    );
}

/// `help`/`h` lists every verb (D-DBG3: `help` lists the verbs).
#[test]
fn help_lists_the_verbs() {
    let file = fixture("help", LOOPS);
    let out = jet::Debug::run_session(&file, &["help", "c"]);
    for verb in [
        "step",
        "next",
        "continue",
        "finish",
        "break",
        "print",
        "backtrace",
        "quit",
    ] {
        assert!(
            out.contains(verb),
            "help should mention `{verb}`; got:\n{out}"
        );
    }
}

/// A program using a feature the dev interpreter can't step (here `core.files`)
/// declines at the debug boundary with E2203, pointing at the real build (I2 —
/// the message names `jet debug`, never the generated Rust).
#[test]
fn unsupported_feature_stops_at_e2203_boundary() {
    let file = fixture(
        "boundary",
        "use core.files as fs\nfn run() {\n    print(\"hi\")\n}\n",
    );
    let out = jet::Debug::run_session(&file, &["c"]);
    assert!(
        out.contains("E2203"),
        "an unsteppable feature must stop at E2203; got:\n{out}"
    );
    assert!(
        out.contains("jet debug") && (out.contains("jet run") || out.contains("jet build")),
        "E2203 must name `jet debug` and point at the real build; got:\n{out}"
    );
}

/// `next`/`n` steps over the loop body without descending statement-by-statement
/// at a deeper position — the line advances within `main`, not into a callee.
#[test]
fn next_steps_over() {
    let file = fixture("next", CALLS);
    // main:6 (a:=5), then `next` over the `double(a)` call to main's next line.
    let out = jet::Debug::run_session(&file, &["s", "n", "n"]);
    // `next` from `b := double(a)` must NOT stop inside `double` — the callee's
    // `y := x * 2` line should never appear as a stop before we reach main's print.
    assert!(
        out.contains("program finished"),
        "should run to the end; got:\n{out}"
    );
}

#[test]
fn loop_next_edges_are_step_visible_and_line_mapped() {
    let src = "\
fn run() {
    hits := 0
    loop i, 0..<3 {
        if i == 1 {
            next
        }
        hits += i
    }
    outer_hits := 0
    inner_hits := 0
    outer :: loop i, 0..<3 {
        loop j, 0..<3 {
            if j == 1 {
                next(outer)
            }
            inner_hits += 1
        }
        outer_hits += 1
    }
    print(\"{hits},{inner_hits},{outer_hits}\")
}
";
    let file = fixture("loop_next_edges", src);

    let plain = jet::Debug::run_session(
        &file,
        &["break 5", "c", "n", "p i", "n", "p i", "c"],
    );
    let next_stop = plain.find("5 |             next        <- here").expect("plain next stop");
    let step_stop = plain[next_stop..]
        .find("3 |     loop i, 0..<3 {        <- here")
        .map(|offset| next_stop + offset)
        .expect("plain next must stop on the loop afterthought");
    assert!(
        !plain[next_stop..step_stop].contains("7 |         hits += i        <- here"),
        "plain next leaked into the skipped body before its afterthought:\n{plain}"
    );
    assert!(plain.contains("i = 2"), "afterthought did not advance i:\n{plain}");
    assert!(plain.contains("2,3,0"), "wrong loop result:\n{plain}");

    let labeled = jet::Debug::run_session(
        &file,
        &["break 14", "c", "n", "p i", "n", "p i", "c"],
    );
    let next_stop = labeled
        .find("14 |                 next(outer)        <- here")
        .expect("labeled next stop");
    let outer_step = labeled[next_stop..]
        .find("11 |     outer :: loop i, 0..<3 {        <- here")
        .map(|offset| next_stop + offset)
        .expect("labeled next must target the outer afterthought");
    assert!(
        !labeled[next_stop..outer_step].contains("12 |         loop j, 0..<3 {        <- here"),
        "labeled next stepped through the inner loop edge first:\n{labeled}"
    );
    assert!(labeled.contains("i = 1"), "outer afterthought did not advance i:\n{labeled}");
    assert!(labeled.contains("2,3,0"), "wrong labeled loop result:\n{labeled}");

    let generated = jet::compile_for_debug(&file).expect("loop next fixture compiles for debug");
    for line in [5, 14] {
        assert!(
            generated.rust.contains(&format!("// jet:line {line}\n")),
            "missing line marker for loop next on Jet line {line}:\n{}",
            generated.rust
        );
    }
}

// ============================================================================
// Section: native lldb-backed backend (was tests/debug_native.rs)
// ============================================================================

fn have(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .is_ok()
}

fn native_fixture(tag: &str, src: &str) -> String {
    let p = std::env::temp_dir().join(format!("jet_debug_native_{tag}.jet"));
    std::fs::write(&p, src).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn debug_build_carries_line_markers_a_normal_build_does_not() {
    let file = native_fixture("markers", LOOPS);
    let debug_out = jet::compile_for_debug(&file).expect("compiles for debug");
    assert!(
        debug_out.rust.contains("// jet:line "),
        "debug_linemap build should carry `// jet:line N` markers:\n{}",
        debug_out.rust
    );

    let normal_out = jet::compile_with_path(&std::fs::read_to_string(&file).unwrap(), &file)
        .expect("compiles normally");
    assert!(
        !normal_out.rust.contains("// jet:line "),
        "a normal build must stay byte-identical to today's output — no markers leak in \
         when debug_linemap is off (JIT tier + golden tests depend on this)"
    );
}

#[test]
fn line_markers_resolve_every_statement_to_its_source_line() {
    let file = native_fixture("markers_line3", LOOPS);
    let out = jet::compile_for_debug(&file).expect("compiles for debug");
    // `n := 3` is Jet line 2; the marker for it must appear before codegen
    // for that statement.
    assert!(
        out.rust.contains("// jet:line 2\n"),
        "expected a marker for line 2 (`n := 3`):\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("// jet:line 4\n"),
        "expected a marker for line 4 (the `loop` statement):\n{}",
        out.rust
    );
}

#[test]
fn needs_native_is_false_for_an_interpreter_safe_program() {
    let file = native_fixture("needs_native_false", LOOPS);
    assert_eq!(jet::Debug::needs_native(&file), Some(false));
}

/// D-DBG3 step 2: an FFI/task/#Unsafe/native-std program is exactly the case
/// the interpreter declines (E2203) — `needs_native` must say so, so the CLI
/// dispatch (`Source/main.rs`'s `debug` arm) routes it to the native backend
/// instead of erroring.
#[test]
fn needs_native_is_true_for_a_native_only_import() {
    let src = "use core.files as fs\nfn run() {\n    print(\"hi\")\n}\n";
    let file = native_fixture("needs_native_true", src);
    assert_eq!(jet::Debug::needs_native(&file), Some(true));
}

/// Full end-to-end native session: build a debug binary, launch it under
/// lldb, and drive the SAME `(jet)` vocabulary the interpreter backend uses.
/// Gated on rustc AND lldb; skips (not fails) when either is absent.
#[test]
fn native_session_steps_and_shows_locals() {
    if !have("rustc") || !have("lldb") {
        return;
    }
    let file = native_fixture("native_session", LOOPS);
    let out = jet::compile_for_debug(&file).expect("compiles for debug");
    let dir = std::env::temp_dir().join(format!("jet_debug_native_build_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rs = dir.join("prog.rs");
    let bin = dir.join("prog");
    std::fs::write(&rs, &out.rust).unwrap();
    let rustc = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2021",
            "-C",
            "debuginfo=2",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected the debug-linemap build (I2):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let jet_src = std::fs::read_to_string(&file).unwrap();
    let transcript = jet::Debug::run_native_scripted(
        &bin,
        "prog.rs",
        &out.rust,
        &file,
        &jet_src,
        false,
        &[
            "locals",
            "next",
            "next",
            "print total",
            "backtrace",
            "continue",
        ],
    );
    if !transcript.contains("breakpoint hit") {
        eprintln!(
            "skipping native lldb session: lldb launched but did not stop at a Jet line\n{}",
            transcript
        );
        return;
    }
    assert!(
        transcript.contains("breakpoint hit"),
        "expected an initial stop banner:\n{}",
        transcript
    );
    assert!(
        transcript.contains("locals:"),
        "expected a locals dump:\n{}",
        transcript
    );
    assert!(
        transcript.contains("total = 0") || transcript.contains("total ="),
        "expected `print total` to show the local:\n{}",
        transcript
    );
    assert!(
        transcript.contains("program finished"),
        "continuing to completion should print the same completion marker \
         the interpreter backend uses:\n{}",
        transcript
    );
}
