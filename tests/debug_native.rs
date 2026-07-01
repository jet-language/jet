//! D-DBG3 step 2 (dap-debugger) — the native lldb-backed `jet debug` backend.
//!
//! Two tiers:
//! - Codegen-only checks (no rustc/lldb needed): `emit_bundle_dbg`'s line
//!   markers show up, `jet::compile_for_debug` produces them, and a normal
//!   build (`emit_bundle`) never does (byte-identical output — the JIT tier
//!   and golden tests must never see a marker).
//! - A full native session, gated on BOTH `rustc` and `lldb` presence (the
//!   same posture `tests/observe.rs` takes for `rustc` alone) — this is the
//!   one place the lldb-output parsing in `Source/Debug/Inferior.rs` gets
//!   exercised against a REAL lldb; it's a hard skip (not a failure) when
//!   either tool is absent, so CI without lldb still passes.

use std::fs;
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn fixture(tag: &str, src: &str) -> String {
    let p = std::env::temp_dir().join(format!("jet_debug_native_{tag}.jet"));
    fs::write(&p, src).unwrap();
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

#[test]
fn debug_build_carries_line_markers_a_normal_build_does_not() {
    let file = fixture("markers", LOOPS);
    let debug_out = jet::compile_for_debug(&file).expect("compiles for debug");
    assert!(
        debug_out.rust.contains("// jet:line "),
        "debug_linemap build should carry `// jet:line N` markers:\n{}",
        debug_out.rust
    );

    let normal_out = jet::compile_with_path(&fs::read_to_string(&file).unwrap(), &file)
        .expect("compiles normally");
    assert!(
        !normal_out.rust.contains("// jet:line "),
        "a normal build must stay byte-identical to today's output — no markers leak in \
         when debug_linemap is off (JIT tier + golden tests depend on this)"
    );
}

#[test]
fn line_markers_resolve_every_statement_to_its_source_line() {
    let file = fixture("markers_line3", LOOPS);
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
    let file = fixture("needs_native_false", LOOPS);
    assert_eq!(jet::Debug::needs_native(&file), Some(false));
}

/// D-DBG3 step 2: an FFI/task/#Unsafe/native-std program is exactly the case
/// the interpreter declines (E2203) — `needs_native` must say so, so the CLI
/// dispatch (`Source/main.rs`'s `debug` arm) routes it to the native backend
/// instead of erroring.
#[test]
fn needs_native_is_true_for_a_native_only_import() {
    let src = "use core.fs as fs\nfn main() {\n    print(\"hi\")\n}\n";
    let file = fixture("needs_native_true", src);
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
    let file = fixture("native_session", LOOPS);
    let out = jet::compile_for_debug(&file).expect("compiles for debug");
    let dir = std::env::temp_dir().join(format!("jet_debug_native_build_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let rs = dir.join("prog.rs");
    let bin = dir.join("prog");
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
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
    let jet_src = fs::read_to_string(&file).unwrap();
    let transcript = jet::Debug::run_native_scripted(
        &bin,
        "prog.rs",
        &out.rust,
        &file,
        &jet_src,
        false,
        &["locals", "next", "next", "print total", "backtrace", "continue"],
    );
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
