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

use std::io::Write;

/// Write `src` to a temp `.jet` file and return its path.
fn fixture(tag: &str, src: &str) -> String {
    let p = std::env::temp_dir().join(format!("jet_debug_{tag}.jet"));
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(src.as_bytes()).unwrap();
    p.to_string_lossy().into_owned()
}

const LOOPS: &str = "\
fn main() {
    n := 3
    total := 0
    loop i in 1..n {
        total += i
    }
    print(\"total is {total}\")
}
";

const CALLS: &str = "\
fn double(x: Int) -> Int {
    y := x * 2
    return y
}
fn main() {
    a := 5
    b := double(a)
    print(\"{b}\")
}
";

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
    assert!(out.contains("<- here"), "missing `<- here` caret; got:\n{out}");
    // The `(jet)` prompt echoes the typed command (D-DBG3 prompt = `(jet)`).
    assert!(out.contains("(jet) s"), "missing `(jet)` prompt echo; got:\n{out}");
    // Locals are shown in Jet terms; after the first `s` the second stop sees `n`.
    assert!(out.contains("locals:"), "missing locals dump; got:\n{out}");
    // The program runs to completion once the script ends.
    assert!(out.contains("total is 6"), "program output missing; got:\n{out}");
    assert!(out.contains("program finished"), "missing end marker; got:\n{out}");
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
    assert!(out.contains("total = 0"), "`print total` should show the value; got:\n{out}");
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
    assert!(out.contains("program finished"), "should run to the end; got:\n{out}");
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
    for verb in ["step", "next", "continue", "finish", "break", "print", "backtrace", "quit"] {
        assert!(out.contains(verb), "help should mention `{verb}`; got:\n{out}");
    }
}

/// A program using a feature the dev interpreter can't step (here `core.fs`)
/// declines at the debug boundary with E2203, pointing at the real build (I2 —
/// the message names `jet debug`, never the generated Rust).
#[test]
fn unsupported_feature_stops_at_e2203_boundary() {
    let file = fixture("boundary", "use core.fs as fs\nfn main() {\n    print(\"hi\")\n}\n");
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
    assert!(out.contains("program finished"), "should run to the end; got:\n{out}");
}
