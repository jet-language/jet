//! TIR control and data integration tests.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, build_and_run_full, compile, have_rustc, jit_run};
use jet::Interpreter::{dev_iteration, RunOutcome};

/// D-FAIL-ERROR1=A: the labelled/string shape builds a default `Err` value;
/// one positional typed value in a result arm still wraps that value.
#[test]
fn default_err_value_and_typed_err_arm_have_distinct_shapes() {
    let src = r#"
fn make() => Err {
    return Err("bad input", code: "E_BAD", cause: Err("root cause"))
}

fn typed(value: Err) ? Err {
    return Err(value)
}

fn run() ? Err {
    return typed(make())
}
"#;
    let rust = compile("tir_default_err_value", src);
    assert!(rust.contains("jet_err("), "default Err must call the Prelude constructor");
    assert!(rust.contains("return Err("), "typed Err arm must remain a Result wrapper");
}

/// D-FAIL-ERROR1=A / I9: the default `jet run` edge renders the same Prelude
/// value and cause chain as the AOT and standalone interpreter edges.
#[test]
fn default_err_value_runs_on_the_default_jit_edge() {
    let src = r#"
fn run() ? Err {
    return Err("unhandled", code: "E_RUN", cause: Err("root"))
}
"#;
    let (code, stdout, stderr) = jit_run("tir_default_err_run", src);
    assert_eq!(stdout, "");
    assert_eq!(code, 1, "default `jet run` should return an unhandled Err");
    assert_eq!(stderr, "Error [E_RUN]: unhandled\n  cause: root\n");
}

/// A program exit cannot forge Jet's branded ICE status. Every native and
/// interpreter adapter calls the same Prelude projection.
#[test]
fn explicit_exit_101_maps_to_user_error_on_every_tier() {
    let src = r#"
use core.process as process
fn run() {
    process.exit(101)
}
"#;
    let (jit_code, jit_stdout, jit_stderr) = jit_run("tir_exit_101_jit", src);
    assert_eq!((jit_code, jit_stdout.as_str(), jit_stderr.as_str()), (1, "", ""));

    let (interp_code, interp_stdout, interp_stderr) =
        tir_support::interpreter_run("tir_exit_101_interp", src);
    assert_eq!(
        (interp_code, interp_stdout.as_str(), interp_stderr.as_str()),
        (1, "", "")
    );

    if have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) =
            build_and_run_full("jet_tir_exit", "reserved_101", src);
        assert_eq!(
            (aot_code, aot_stdout.as_str(), aot_stderr.as_str()),
            (1, "", "")
        );
    }
}

/// A `?` from one typed error into a wider union must stay native on the
/// resident JIT and produce the same value on the explicit interpreter.
#[test]
fn typed_error_union_widening_runs_on_jit_and_interpreter() {
    let src = r#"
fn narrow() => Int ? String {
    return Err("narrow")
}

fn widen() => Int ? String | Bool {
    return narrow()?
}

fn run() {
    print(widen() ?? 7)
}
"#;
    let (jit_code, jit_stdout, jit_stderr) =
        tir_support::jit_run_traced("tir_error_union_widen_jit", src);
    assert_eq!(jit_code, 0, "{jit_stderr}");
    assert_eq!(jit_stdout, "7\n");
    assert!(
        jit_stderr
            .lines()
            .any(|line| line.starts_with("widen") && line.contains("tier1 native")),
        "typed error widening deoptimized instead of running on the JIT: {jit_stderr}"
    );
    assert!(!jit_stderr.contains("widen: tier0 interp"), "{jit_stderr}");

    let (interp_code, interp_stdout, interp_stderr) =
        tir_support::interpreter_run("tir_error_union_widen_interp", src);
    assert_eq!(interp_code, 0, "{interp_stderr}");
    assert_eq!(interp_stdout, jit_stdout);
}

/// `?? panic(...)` must stay on the native tier and use the full shared rich
/// stop renderer, including source and scalar-local context.
#[test]
fn jit_or_fallback_panic_keeps_rich_context() {
    let src = r#"use core.process as process
fn run() {
    count :: process.argv().len()
    missing :: process.argv().get(count + 1)
    print(missing ?? panic("missing argument"))
}
"#;
    let (code, stdout, stderr) = tir_support::jit_run_traced("jit_fallback_panic", src);
    assert_eq!(code, 70, "rich panic exit: out={stdout} err={stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(
        stderr
            .lines()
            .any(|line| line.starts_with("run") && line.contains("tier1 native")),
        "panic fallback deoptimized: {stderr}"
    );
    assert!(
        stderr.contains("Stop [E3001]: `panic: missing argument`")
            && stderr.contains("jit_fallback_panic.jet:5 in run")
            && stderr.contains("print(missing ?? panic(\"missing argument\"))")
            && stderr.contains("count = "),
        "panic fallback lost rich context: {stderr}"
    );
}

/// An expert Index hook miss is one ordinary program-side E3001 stop on both
/// the default run path and the explicit evaluator.
#[test]
fn index_hook_miss_uses_the_structured_runtime_stop() {
    let src = r#"struct Tile {
    value: Int
}
struct Grid {
    cells: [Tile]
}
impl Grid.Index {
    type Key = Int
    type Value = Int
    fn get(self, key: Int) => Int? {
        if key < 0 || key >= self.cells.len() -> return None
        return Val(self.cells[key].value)
    }
}
fn run() {
    grid :: Grid.{cells: [Tile.{value: 1}]}
    print(grid[9])
}
"#;
    for (tier, (code, stdout, stderr)) in [
        ("default", tir_support::jit_run("index_hook_miss_jit", src)),
        (
            "interpreter",
            tir_support::interpreter_run("index_hook_miss_interp", src),
        ),
    ] {
        assert_eq!(code, 70, "{tier}: out={stdout} err={stderr}");
        assert!(stdout.is_empty(), "{tier}: {stdout}");
        assert!(
            stderr.contains("Stop [E3001]: `panic: index miss`")
                && stderr.contains("print(grid[9])")
                && stderr.contains("in run"),
            "{tier}: {stderr}"
        );
        assert!(!stderr.contains("unsupported"), "{tier}: {stderr}");
    }
}

/// D-CALLVALUE1=B / I9: the named returned-function example exercises the
/// canonical `.call(...)` projection through AOT, default `jet run`, and the
/// forced interpreter against one checked-in byte oracle.
#[test]
fn returned_function_call_example_matches_all_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "functions/returned_function_call",
        include_str!("../examples/features/expected/functions/returned_function_call.out"),
    );
}
/// D-BODY-LAST1=B / D-SIG-SHAPE1=B / D-LOOP-STMT-ARROW1=C / I9: the body-rule example
/// produces byte-identical output through AOT, default `jet run`, and the
/// forced interpreter.
#[test]
fn body_rules_example_matches_all_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "basics/body_rules",
        include_str!("../examples/features/expected/basics/body_rules.out"),
    );
}

/// D-LOOP-SUBJECT1=A / I9: bindingless collection loops use their item as the
/// implicit subject, while the scalar example keeps its explicit binding.
#[test]
fn bindingless_loop_example_matches_all_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "basics/loop_bindingless",
        include_str!("../examples/features/expected/basics/loop_bindingless.out"),
    );

    let path = format!(
        "{}/examples/features/basics/loop_bindingless.jet",
        env!("CARGO_MANIFEST_DIR")
    );
    match dev_iteration(&path, false, true) {
        RunOutcome::Ran {
            exit_code,
            stdout,
            stderr,
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(
                stdout,
                include_str!("../examples/features/expected/basics/loop_bindingless.out")
            );
            assert_eq!(stderr, "");
        }
        RunOutcome::Problems(diagnostics) => {
            panic!("forced interpreter rejected bindingless loop example: {diagnostics:?}")
        }
    }
}

/// D-CONC-CHAN1 / D-CONC-CHAN2 / I9: plain-endpoint readiness tables, task
/// waits, and the one Duration time rail agree on AOT, default `jet run`, and
/// the forced interpreter against the executable example oracle.
#[test]
fn channel_select_examples_match_all_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "concurrency/select_channel",
        include_str!("../examples/features/expected/concurrency/select_channel.out"),
    );
    tir_support::assert_example_cli_tiers_agree(
        "concurrency/select_generic",
        include_str!("../examples/features/expected/concurrency/select_generic.out"),
    );
}

/// D-FAIL-EXIT1=A / I9: the default-fallible entry keeps its journey and exit
/// code byte-identical on debug AOT, default jet run, and the interpreter.
#[test]
fn default_entry_error_golden_matches_all_execution_tiers() {
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/default_error_conversion",
        1,
        include_str!("../examples/features/expected/errors/default_error_conversion.err.out"),
    );
}

/// D-FAIL-CTX1=A / I9: the `?`-propagation trail is one byte oracle across
/// debug AOT, `--release`, default `jet run`, and the forced interpreter. The
/// interpreter printed `Error: file not found` with NO trail while the other
/// three printed three hops, because the evaluator records each hop on its own
/// worker thread and the report edge drained the caller's empty journey. No
/// per-stem battery named this example, so the divergence sat in the shipped
/// binary: the trail's own example now names every lens.
#[test]
fn propagation_trail_golden_matches_all_execution_tiers() {
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/error_context",
        1,
        include_str!("../examples/features/expected/errors/error_context.err.out"),
    );
}

/// D-FAIL-BREACH1 / I9: the checked-in runtime-stop reports are one byte oracle
/// across AOT, default `jet run`, and the forced interpreter. Keep one example
/// for each shipped stop kind so exit 101 or a second renderer cannot hide in a
/// single tier.
#[test]
fn runtime_stop_goldens_match_all_execution_tiers() {
    tir_support::assert_example_cli_error_tiers_agree(
        "collections/list_bounds",
        70,
        include_str!("../examples/features/expected/collections/list_bounds.err.out"),
    );
    tir_support::assert_example_cli_error_tiers_agree(
        "collections/map_key",
        70,
        include_str!("../examples/features/expected/collections/map_key.err.out"),
    );
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/panic",
        70,
        include_str!("../examples/features/expected/errors/panic.err.out"),
    );
    // #1967: the `??` right side is a stop too. `assert_example_cli_error_tiers_agree`
    // requires EMPTY stdout, so a tier that reports the panic without ending the
    // program fails here on the unreachable `print` — that is the whole point of
    // this stem, and why `errors/panic` alone did not catch it.
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/qq_panic",
        70,
        include_str!("../examples/features/expected/errors/qq_panic.err.out"),
    );
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/stack_overflow",
        70,
        include_str!("../examples/features/expected/errors/stack_overflow.err.out"),
    );
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/todo_stop",
        70,
        include_str!("../examples/features/expected/errors/todo_stop.err.out"),
    );
    tir_support::assert_example_cli_error_tiers_agree(
        "errors/u8_divide_zero",
        70,
        include_str!("../examples/features/expected/errors/u8_divide_zero.err.out"),
    );
}

/// D-FAIL-EXIT1=A: explicit process termination unwinds Jet cleanup in the
/// documented order before returning its requested code.
#[test]
fn explicit_process_exit_cleanup_golden_matches_all_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "io/process_exit_cleanup",
        include_str!("../examples/features/expected/io/process_exit_cleanup.out"),
    );
}

/// Arithmetic + a helper call + interpolation. The helper `double` and `main`
/// are both fully covered, so both route through the TIR.
#[test]
fn arithmetic_and_helper_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) => Int {
    return (n * 2)
}
fn run() {
    sum :: (7 + (3 * 4))
    print(\"sum {sum}\")
    print(double(sum))
}
";
    let (code, stdout) = build_and_run("tir_arith", src);
    assert_eq!(code, 0, "should exit cleanly");
    assert_eq!(stdout, "sum 19\n38\n");
}

#[test]
fn range_value_windows_are_no_copy_and_write_through() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    values := [10, 20, 30, 40, 50]
    band :: 1..<4
    window :: values[band]
    print(~window)
    edit :: &values[band]
    edit[1] = 99
    print(values)
}
"#;
    let rust = compile("tir_range_value_windows", src);
    assert!(
        rust.contains("let __jet_window = jet_view_range_new"),
        "bare Range projection must borrow without copying: {rust}"
    );
    assert!(
        rust.contains("let __jet_edit = jet_view_mut_range_new"),
        "write Range projection must borrow the owner: {rust}"
    );
    let (code, stdout) = build_and_run("tir_range_value_windows", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[20, 30, 40]\n[10, 20, 99, 40, 50]\n");
}

/// An if-expression (S68) bound to a local, plus a String param helper.
#[test]
fn if_expression_and_string_param() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn shout(s: String) => String {
    return \"{s}!\"
}
fn run() {
    n :: 7
    parity :: if ((n % 2) == 0) -> \"even\" else -> \"odd\"
    print(shout(parity))
}
";
    let (code, stdout) = build_and_run("tir_ifexpr", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "odd!\n");
}

/// Statement-form if / else-if / else with a returning helper — mirrors the
/// shape of examples/features/basics/fizzbuzz.jet's `label`.
#[test]
fn if_else_chain_and_return() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn label(n: Int) => String {
    if ((n % 15) == 0) {
        return \"FizzBuzz\"
    } else if ((n % 3) == 0) {
        return \"Fizz\"
    } else if ((n % 5) == 0) {
        return \"Buzz\"
    }
    return \"{n}\"
}
fn run() {
    print(label(3))
    print(label(5))
    print(label(15))
    print(label(7))
}
";
    let (code, stdout) = build_and_run("tir_ifchain", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Fizz\nBuzz\nFizzBuzz\n7\n");
}

/// D-IFGUARD1=A: statement/value guards share ordered `if` lowering. Heads run
/// once, stop at the first match, preserve Boolean short-circuiting, and an
/// unmatched statement table performs no action.
#[test]
fn subjectless_guards_order_totality_and_nested_forms() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn check(note: String, answer: Bool) => Bool {
    print(note)
    return answer
}

fn run() {
    if {
        check("first", false) -> print("wrong")
        check("second", true) -> {
            print("chosen")
            if true -> print("nested")
        }
        check("never", true) -> print("wrong")
    }
    if {
        false && check("short-circuited", true) -> print("wrong")
    }
    label :: if {
        false -> "wrong"
        true -> "value"
        else -> "fallback"
    }
    print(label)
}
"#;
    let (code, stdout) = build_and_run("tir_subjectless_guards", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "first\nsecond\nchosen\nnested\nvalue\n");
}

#[test]
fn subjectless_guard_pattern_bindings_dominate_selected_body() {
    if !have_rustc() {
        return;
    }
    let src = r#"
enum Choice {
    A(Int)
    B(Int)
}

fn choose(note: String, value: Int) => Choice {
    print(note)
    return Choice.A(value)
}

fn run() {
    left :: Choice.A(4)
    right :: Choice.A(6)
    if choose("first only", 4) == .B(x) && choose("never", 6) == .A(y) && x < y -> print("wrong")
    if {
        left == .A(x) && right == .A(y) && x < y -> print("table {x} {y}")
        left == .B(_) -> print(0)
    }
    if choose("left", 4) == .A(x) && choose("right", 6) == .A(y) && x < y -> print("inline {x} {y}")
    if true && left == .A(n) -> print("pre {n}")
    label :: if {
        left == .A(x) && right == .A(y) && x < y -> "value {x} {y}"
        else -> "other"
    }
    print(label)
}
"#;
    let (code, stdout) = build_and_run("tir_subjectless_guard_patterns", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "first only\ntable 4 6\nleft\nright\ninline 4 6\npre 4\nvalue 4 6\n");
}

/// Coexistence: a free function and a method in the same program both route
/// through the executable TIR. A construct outside TIR coverage is an ICE gate,
/// not a legacy AST fallback.
#[test]
fn tir_function_and_method_coexist() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int
}
impl Counter {
    fn bumped(self) => Int {
        return (self.n + 1)
    }
}
fn add(a: Int, b: Int) => Int {
    return (a + b)
}
fn run() {
    c :: Counter.{ n: 41 }
    print(add(c.bumped(), 0))
}
";
    let (code, stdout) = build_and_run("tir_coexist", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// Fixed-width integer overflow must still trap when the function is on the
/// TIR path (the `overflow` flag is computed at lowering, not in codegen).
#[test]
fn overflow_still_traps_on_tir_path() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
a :: U8.{ 200 }
b :: U8.{ 100 }
    print(a + b)
}
";
    let (code, _stdout) = build_and_run("tir_overflow", src);
    assert_eq!(code, 70, "U8 overflow should trap (exit 70)");
}

#[test]
fn unrelated_codable_does_not_require_union_codecs() {
    if !have_rustc() {
        return;
    }
    let src = r#"
struct Raw {
    value: Int
}

fn hold(value: Int | Raw) => Int | Raw {
    return ~value
}

#Codable
struct Encoded {
    value: Int
}

fn run() {
    raw :: Raw.{ value: 7 }
    _ :: hold(raw)
    print("ok")
}
"#;
    let (code, stdout) = build_and_run("union_unrelated_codable", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn union_codec_emission_respects_derive_direction() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.encoding.json as json

struct WriteOnly {
    marker: Int
}

impl WriteOnly.Encode {
    fn encode(self) => DataTree {
        return DataTree.Text("write")
    }
}

#Encode
struct Output {
    value: Int | WriteOnly
}

#Decode
struct ReadOnly {
    marker: Int
}

#Decode
struct Input {
    value: Int | ReadOnly
}

fn run() {
    output :: Output.{ value: WriteOnly.{ marker: 1 } }
    print(json.to_string(output))
    _ :: json.decode<Input>("{{\"value\":7}}") ?? panic("input")
    print("ok")
}
"#;
    let (code, stdout) = build_and_run("union_codec_direction", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "{\"value\":\"write\"}\nok\n");
}

#[test]
fn optional_union_null_and_enum_payload_round_trip() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.encoding.json as json

#Codable
struct MaybeRow {
    value: Int? | String
}

#Codable
enum Envelope {
    Value(Int | String)
}

fn run() {
    decoded :: json.decode<MaybeRow>("{{\"value\":null}}") ?? panic("row")
    print(json.to_string(decoded))

    envelope :: Envelope.Value(7)
    envelope_wire :: json.to_string(envelope)
    print(envelope_wire)
    back :: json.decode<Envelope>(envelope_wire) ?? panic("envelope")
    print(json.to_string(back))
}
"#;
    let (code, stdout) = build_and_run("union_codable_roundtrip", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "{\"value\":null}\n{\"Value\":7}\n{\"Value\":7}\n"
    );
}

#[test]
fn codable_union_distinct_member_uses_base_wire_shape() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.encoding.json as json

#CodableAsBase
Usd :: distinct Int

#Codable
struct Row {
    value: Usd | String
}

fn run() {
    row :: Row.{ value: Usd.from_int(7) }
    wire :: json.to_string(row)
    print(wire)
    decoded :: json.decode<Row>(wire) ?? panic("row")
    decoded_value :: decoded.value
    if decoded_value == {
        .Usd(value) -> print(value.raw())
        .String(value) -> print(value)
    }
}
"#;
    let (code, stdout) = build_and_run("union_codable_distinct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "{\"value\":7}\n7\n");
}

// --- c109 Phase 2: control-flow loops ---------------------------------------

/// Infinite `loop { … }` with a `break`, plus the `loop cond` while form. Both
/// loop kinds, plus a compound assign and an if inside, route through the TIR.
#[test]
fn infinite_and_while_loops() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    x := 0
    loop {
        x = (x + 1)
        if (x == 3) {
            break
        }
    }
    print(x)
    fuel := 3
    loop fuel > 0 {
        print(\"t-minus {fuel}\")
        fuel-= 1
    }
    print(\"liftoff\")
}
";
    let (code, stdout) = build_and_run("tir_loops", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\nt-minus 3\nt-minus 2\nt-minus 1\nliftoff\n");
}

/// Numeric range loops: inclusive `1..5` and a strided `0..10 step 2`. The
/// inclusive semantics (`..=`) and the `.step_by` lowering are read off the TIR.
#[test]
fn range_loops_inclusive_and_step() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    total := 0
    loop n, 1..5 {
        total = (total + n)
    }
    print(total)
    loop k, 0..10, 2 {
        print(k)
    }
}
";
    let generated = compile("tir_ranges_codegen", src);
    assert!(
        generated.contains("for __jet_n in (1i64)..=(5i64) {"),
        "literal range loop must remain a direct Rust range jump:\n{generated}"
    );
    assert!(
        !generated.contains("let __jet_range = JetRange"),
        "literal range loop must not allocate or construct a Range value:\n{generated}"
    );
    let (code, stdout) = build_and_run("tir_ranges", src);
    assert_eq!(code, 0);
    // 1+2+3+4+5 = 15, then 0,2,4,6,8,10 (inclusive end).
    assert_eq!(stdout, "15\n0\n2\n4\n6\n8\n10\n");
}

/// D-RANGE-EXCL1=C: half-open `..<` excludes the end and is empty when start >= end.
#[test]
fn range_loops_exclusive() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    total := 0
    loop n, 0..<5 {
        total = (total + n)
    }
    print(total)
    empty := 0
    loop n, 3..<3 {
        empty = (empty + 1)
    }
    print(empty)
}
";
    let (code, stdout) = build_and_run("tir_ranges_excl", src);
    assert_eq!(code, 0);
    // 0+1+2+3+4 = 10; empty exclusive range runs 0 times.
    assert_eq!(stdout, "10\n0\n");
}

/// D-RANGE-VALUE1=A: both range spellings construct one storable `Range`.
/// A Range keeps its bounds and inclusivity when passed, returned, looped,
/// queried, and used as a slice bound.
#[test]
fn range_values_store_pass_return_loop_and_slice() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn identity(band: ^Range) => Range {
    return band
}
fn run() {
    bands :: [1..3, 8..<10]
    print(bands[0])
    print(\"{bands[1]:Debug}\")
    print(bands[0] == (1..3))
    print(bands[0] == (1..<3))
    print(bands[0].contains(3))
    band :: identity(4..<7)
    print(band.start)
    print(band.end)
    print(band.contains(6))
    print(band.contains(7))
    print((7..4).contains(5))
    total := 0
    loop n, band {
        total = (total + n)
    }
    print(total)
    xs :: [10, 20, 30, 40, 50, 60, 70, 80]
    print(~xs[band])
}
";
    let (code, stdout) = build_and_run("tir_range_values", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "\
Range { start: 1, end: 3, exclusive: false }
Range { start: 8, end: 10, exclusive: true }
true
false
true
4
7
true
false
false
15
[50, 60, 70]
"
    );
}

/// Named loops: `next(outer)` and `break(outer)` driving a nested
/// range loop. The `'__jet_<name>:` labels are resolved at lowering.
#[test]
fn labeled_break_and_continue() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    outer :: loop i, 1..3 {
        loop j, 1..3 {
            if (j == 2) {
                next(outer)
            }
            print(\"{i}-{j}\")
            if (i == 2) {
                break(outer)
            }
        }
    }
    print(\"done\")
}
";
    let rust = compile("tir_labeled", src);
    assert!(rust.contains("continue '__jet_outer;"), "{rust}");
    assert!(rust.contains("break '__jet_outer;"), "{rust}");
    let (code, stdout) = build_and_run("tir_labeled", src);
    assert_eq!(code, 0);
    // i=1: j=1 prints 1-1, i!=2 so j=2 -> next(outer).
    // i=2: j=1 prints 2-1, i==2 -> break(outer).
    assert_eq!(stdout, "1-1\n2-1\ndone\n");
}

/// D-LOOPLABEL3 / D-ORRETURN-CANON1: named exits are also valid `??`
/// fallbacks and target the named loop's normal continuation/break edges.
#[test]
fn labeled_break_and_continue_fallbacks() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn run() {
    outer :: loop text, ["skip", "7"] {
        loop {
            value :: Int.parse(text) ?? next(outer)
            print(value)
            Int.parse("stop") ?? break(outer)
        }
    }
    print("done")
}
"#;
    let (code, stdout) = build_and_run("tir_labeled_fallbacks", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\ndone\n");
}

#[test]
fn yielding_and_result_loops_compile_and_run() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn find(xs: [Int]) => Int {
    found :: loop {
        loop x, xs {
            if x > 2 -> break(found, x)
        }
        break -1
    }
    found
}

fn outer_result() => Int {
    result :: loop {
        ignored :: loop {
            break(result, 9)
        }
        break 0
    }
    result
}

fn identity(value: Int) => Int :: value

fn nested_binary_exit() => Int {
    result :: loop {
        ignored :: (loop {
            break(result, 11)
            break 1
        }) + 2
        break 0
    }
    result
}

fn nested_call_exit() => Int {
    result :: loop {
        ignored :: identity(loop {
            break(result, 12)
            break 1
        })
        break 0
    }
    result
}

fn nested_condition_exit() => Int {
    result :: loop {
        if (loop {
            break(result, 13)
            break 1
        }) > 0 {
            break 0
        }
        break -1
    }
    result
}

fn counted_init_exit() => Int {
    result :: loop {
        loop i := (loop {
            break(result, 14)
            break 0
        }), i < 1 {
            i++
        }
        break 0
    }
    result
}

fn counted_step_exit() => Int {
    result :: loop {
        loop i := 0, i < 2 {
            i = (loop {
                break(result, 15)
                break 0
            })
        }
        break 0
    }
    result
}

fn value_if_exit() => Int {
    result :: loop {
        ignored :: if true -> {
            break(result, 16)
            0
        } else -> 0
        break 0
    }
    result
}

fn run() {
    xs :: [Int].{ 1, 2, 3, 4 }
    prefix :: loop x, xs -> {
        if x > 3 -> break
        x * 2
    }
    rows :: loop x, xs, y, [10, 20] -> {
        if x == 2 && y == 20 -> break
        x + y
    }
    outer :: loop x, xs {
        ignored :: loop {
            if x == 1 -> next(outer)
            if x == 2 -> break(outer)
            break 0
        }
        print(ignored)
    }
    print(prefix)
    print(rows)
    print(find(xs))
    print(outer_result())
    print(nested_binary_exit())
    print(nested_call_exit())
    print(nested_condition_exit())
    print(counted_init_exit())
    print(counted_step_exit())
    print(value_if_exit())
}
"#;
    let (code, stdout) = build_and_run("tir_loop_values", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "[2, 4, 6]\n[11, 21, 12]\n3\n9\n11\n12\n13\n14\n15\n16\n"
    );
}

/// D-LOOP-COMMA1=A/D-LOOP-ADVANCE2/D-LOOP-CONTROLWORD1: every multi-clause loop
/// header separates its clauses with commas, source/stride expressions run once,
/// and `next` enters the target loop's advancement edge. D-LOOP-HEADER3=D retired
/// the C-style counter header, so mutable state advances in the body and a
/// counted walk uses a source range.
#[test]
fn unified_loop_headers_stride_and_next_edges() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn source() => [Int] {
    print("source")
    return [0, 1, 2, 3, 4, 5, 6]
}

fn stride() => Int {
    print("stride")
    return 3
}

fn run() {
    loop item, source(), stride() {
        print(item)
        if item == 0 { next }
    }

    outer :: loop i, 0..<3 {
        loop {
            if i < 2 { next(outer) }
            break
        }
        print("state {i}")
    }
}
"#;
    let (code, stdout) = build_and_run("tir_unified_loops", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "source\nstride\n0\n3\n6\nstate 2\n");

    let invalid = r#"
fn source() => [Int] {
    print("source")
    return [1]
}
fn stride() => Int {
    print("stride")
    return 0
}
fn run() {
    loop item, source(), stride() {
        print(item)
    }
}
"#;
    let (code, stdout, stderr) = build_and_run_full(
        "jet_tir_test",
        "tir_unified_loop_invalid_stride",
        invalid,
    );
    assert_eq!(code, 70);
    assert_eq!(stdout, "source\nstride\n", "no source item may be pulled");
    assert!(stderr.contains("E0123: loop stride must be positive"));
}

// --- c109 Phase 3: structs --------------------------------------------------

/// Struct literal, a struct-typed param with scalar field reads (borrow
/// position — no clone), a struct return value, and a struct-typed local. All
/// of `sum_pt`, `origin`, and `main` are inside the subset, so all route
/// through the TIR. The scalar field-read arithmetic (`p.x + p.y`) must NOT
/// overflow-trap: the old emitter baseline left this field operand unresolved,
/// so the plain `+` was used — the TIR reproduces that parity exactly.
#[test]
fn struct_literal_field_read_and_return() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point {
    x: Int
    y: Int
}
fn sum_pt(p: Point) => Int {
    return (p.x + p.y)
}
fn origin() => Point {
    return Point.{ x: 0, y: 0 }
}
fn run() {
    p :: Point.{ x: 3, y: 4 }
    print(sum_pt(p))
    print(p.x)
    o :: origin()
    print(sum_pt(o))
}
";
    let (code, stdout) = build_and_run("tir_struct_pt", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n3\n0\n");
}

/// A String struct field read in interpolation (a borrow-position read, so no
/// clone is inserted) plus a struct literal whose String field is moved from an
/// owned local. `describe` and `main` both route through the TIR.
#[test]
fn struct_string_field_in_interpolation() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Person {
    name: String
    age: Int
}
fn describe(p: Person) {
    print(\"{p.name} is {p.age}\")
}
fn run() {
    label :: \"Ada\"
    p :: Person.{ name: label, age: 36 }
    describe(p)
    print(p.age)
}
";
    let (code, stdout) = build_and_run("tir_struct_person", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Ada is 36\n36\n");
}

/// Nested struct: a struct field whose type is itself a covered struct. Both the
/// nested literal (`Outer.{ inner: Inner { … }, … }`) and the chained field read
/// (`o.inner.v`) are covered, so `deep` and `main` route through the TIR.
#[test]
fn nested_struct_literal_and_chained_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Inner {
    v: Int
}
struct Outer {
    inner: Inner
    label: Int
}
fn deep(o: Outer) => Int {
    return (o.inner.v + o.label)
}
fn run() {
    o :: Outer.{ inner: Inner.{ v: 10 }, label: 5 }
    print(deep(o))
    print(o.inner.v)
}
";
    let (code, stdout) = build_and_run("tir_struct_nested", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "15\n10\n");
}

// --- c109 Phase 4: enums + when/match + patterns ----------------------------

/// A unit-variant enum, enum literals (`Light.Red` etc.), and two exhaustive
/// variant matches (the `_ => unreachable!` fallthrough is dead but mandatory).
/// `next`, `label`, and `main` (an enum-typed local + covered helper calls) all
/// route through the TIR. Mirrors examples/features/types/enums.jet.
#[test]
fn enum_unit_variants_and_exhaustive_match() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Light {
    Red
    Yellow
    Green
}
fn next(light: Light) => Light {
    if light == {
        .Red -> { return Light.Yellow }
        .Yellow -> { return Light.Green }
        .Green -> { return Light.Red }
    }
}
fn label(light: Light) => String {
    if light == {
        .Red -> { return \"stop\" }
        .Yellow -> { return \"caution\" }
        .Green -> { return \"go\" }
    }
}
fn run() {
    start :: Light.Red
    print(label(start))
    print(label(next(start)))
}
";
    let (code, stdout) = build_and_run("tir_enum_unit", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "stop\ncaution\n");
}

/// Scalar-payload variants, an enum literal with a payload (`Conn.Active(42)`), a
/// payload binding read in the arm body, an or-pattern sharing a binding
/// (`Active(id) | Reconnecting(id)`), and a wildcard slot (`Idle(_)`).
#[test]
fn enum_payload_or_pattern_and_binding() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Conn {
    Active(Int)
    Reconnecting(Int)
    Idle(Int)
    Closed
}
fn describe(c: Conn) => String {
    if c == {
        .Active(id) | .Reconnecting(id) -> { return \"live:{id}\" }
        .Idle(_) -> { return \"idle\" }
        .Closed -> { return \"closed\" }
    }
    return \"unknown\"
}
fn run() {
    print(describe(Conn.Active(42)))
    print(describe(Conn.Reconnecting(7)))
    print(describe(Conn.Idle(99)))
    print(describe(Conn.Closed))
}
";
    let (code, stdout) = build_and_run("tir_enum_payload", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "live:42\nlive:7\nidle\nclosed\n");
}

/// S83 / D-CHOOSE-HEADS1=A: multi-head declarations lower to the same TIR
/// enum table used by ordinary `if subject == { … }` dispatch.
#[test]
fn multi_head_functions_use_table_dispatch() {
    assert!(
        have_rustc(),
        "multi-head AOT parity requires rustc; project harness must provision it"
    );
    let src = "\
enum Shape {
    Circle(Float)
    Rect(w: Float, h: Float)
}
fn area(Circle(r: Float)) => Float {
    return r * r
}
fn area(Rect(w: Float, h: Float)) => Float {
    return w * h
}
fn run() {
    print(area(Shape.Circle(3.0)))
    print(area(.Rect.{ w: 2.0, h: 4.0 }))
}
";
    let (code, stdout) = build_and_run("tir_multi_head_functions", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "9.0\n8.0\n");
}

/// A range pattern in a *payload* slot (`Good(200..299)`, lowered to a match-arm
/// guard) alongside wildcard slots, all over an exhaustive enum match.
#[test]
fn enum_payload_range_pattern_guard() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum HTTP {
    Good(Int)
    Fail(Int)
}
fn classify(r: HTTP) => String {
    if r == {
        .Good(200..299) -> { return \"success\" }
        .Good(400..499) -> { return \"client error\" }
        .Good(_) -> { return \"other\" }
        .Fail(_) -> { return \"network error\" }
    }
    return \"unknown\"
}
fn run() {
    print(classify(HTTP.Good(201)))
    print(classify(HTTP.Good(404)))
    print(classify(HTTP.Good(302)))
    print(classify(HTTP.Fail(0)))
}
";
    let (code, stdout) = build_and_run("tir_enum_range", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "success\nclient error\nother\nnetwork error\n");
}

/// An arm-head range dispatch over a scalar subject with an `else` (the mixed-dispatch
/// `if/else if … else` lowering, with the parity `__jet_switch_subject` binding).
/// Mirrors examples/features/basics/pattern_matching.jet's `score_grade`.
#[test]
fn arm_head_range_dispatch() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn grade(score: Int) => String {
    if score == {
        0..59 -> { return \"F\" }
        60..69 -> { return \"D\" }
        70..89 -> { return \"C\" }
        90..100 -> { return \"A\" }
        else -> { return \"?\" }
    }
}
fn run() {
    print(grade(95))
    print(grade(72))
    print(grade(45))
    print(grade(120))
}
";
    let (code, stdout) = build_and_run("tir_range_dispatch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "A\nC\nF\n?\n");
}

#[test]
fn branch_classifier_emits_table_and_ordered_shapes_with_one_subject_evaluation() {
    let table = compile(
        "tir_branch_table",
        r#"
fn dense(n: Int) => String {
    if n == {
        1 -> { return "one" }
        2 -> { return "two" }
        3 -> { return "three" }
        else -> { return "other" }
    }
}
fn sparse(n: Int) => String {
    if n == {
        1 -> { return "one" }
        100 -> { return "hundred" }
        else -> { return "other" }
    }
}
fn truth(flag: Bool) => String {
    if flag == {
        true -> { return "yes" }
        false -> { return "no" }
        else -> { return "no" }
    }
}
fn run() {
    print(dense(1))
    print(sparse(100))
    print(truth(true))
}
"#,
    );
    assert!(table.contains("// jet:branch dense-table"), "{table}");
    assert!(table.contains("// jet:branch sparse-search"), "{table}");
    assert!(table.contains("// jet:branch bool-two-way"), "{table}");
    assert!(
        table.contains("else if *__jet___switch_subject < 100"),
        "sparse integers should emit a balanced search tree: {table}"
    );
    assert!(
        table.contains("if *__jet___switch_subject {"),
        "two-way Bool dispatch should branch on the subject directly: {table}"
    );
    assert_eq!(
        table.matches("match *__jet___switch_subject").count(),
        1,
        "only dense integer arms should use table lowering: {table}"
    );

    let ordered = compile(
        "tir_branch_ordered",
        r#"
fn subject() => Int { return 7 }
fn run() {
    if subject() == {
        0..3 -> { print("low") }
        4..9 -> { print("mid") }
        42 -> { print("answer") }
        else -> { print("other") }
    }
}
"#,
    );
    assert!(
        ordered.contains("let __jet___switch_subject = &(__jet_subject())"),
        "{ordered}"
    );
    assert!(
        ordered.contains("(*__jet___switch_subject)"),
        "conditions must reuse the evaluated subject: {ordered}"
    );
    assert_eq!(
        ordered.matches("&(__jet_subject())").count(),
        1,
        "branch subject was evaluated more than once: {ordered}"
    );
    assert!(
        !ordered.contains("__jet___switch_subject.clone()"),
        "branch dispatch must not clone its subject: {ordered}"
    );
}
