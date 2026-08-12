#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_tmp(prefix: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), n))
}

pub fn have_rustc() -> bool {
    let present = Command::new("rustc").arg("--version").output().is_ok();
    if !present && std::env::var("JET_REQUIRE_RUSTC").as_deref() == Ok("1") {
        panic!(
            "JET_REQUIRE_RUSTC=1 but rustc not found on PATH — refusing to \
             silently skip I2 (rustc-must-accept) coverage. Fix the CI \
             environment; do not unset JET_REQUIRE_RUSTC to paper over this."
        );
    }
    present
}

pub fn build_and_run(name: &str, src: &str) -> (i32, String) {
    let (code, stdout, _stderr) = build_and_run_full("jet_tir_test", name, src);
    (code, stdout)
}

pub fn compile(name: &str, src: &str) -> String {
    let dir = unique_tmp("jet_tir_compile");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected:\n{}",
                jet::render_diagnostics(&shown, src, &diags)
            )
        })
        .rust
}

pub fn build_and_run_full(prefix: &str, name: &str, src: &str) -> (i32, String, String) {
    build_and_run_full_inner(prefix, name, src, None)
}

/// Run a snippet the way `jet run` does — through the Cranelift host, with the
/// interpreter picking up whatever the host deopts on. `build_and_run*` above
/// only ever proves AOT, so a rule re-encoded in an engine would pass every one
/// of those and still be wrong (I9). Needs no rustc.
///
/// Returns `(exit code, stdout, stderr)`.
pub fn jit_run(name: &str, src: &str) -> (i32, String, String) {
    jit_run_with_env(name, src, &[])
}

/// `jit_run` with environment variables the program can read back.
///
/// A trap test needs an operand the comptime evaluator cannot see. It folds
/// literals, calls, and loops over literal lists, so a value that only exists
/// in the process environment is the smallest thing that reaches the Cranelift
/// host instead of stopping the build with a comptime diagnostic.
pub fn jit_run_with_env(
    name: &str,
    src: &str,
    vars: &[(&str, &str)],
) -> (i32, String, String) {
    jit_run_with_env_args(name, src, vars, &[])
}

/// `jit_run` with process env and program argv after `--`.
pub fn jit_run_with_env_args(
    name: &str,
    src: &str,
    vars: &[(&str, &str)],
    program_args: &[&str],
) -> (i32, String, String) {
    let dir = unique_tmp("jet_jit_run");
    fs::create_dir_all(&dir).unwrap();
    let jet_name = format!("{name}.jet");
    let jet_path = dir.join(&jet_name);
    fs::write(&jet_path, src).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .arg("run")
        .arg(&jet_path)
        .current_dir(&dir)
        // Keep every run out of the shared build cache, which is keyed on the
        // AST hash and would otherwise serve a binary built before this change.
        .env("JET_CACHE_DIR", dir.join("cache"));
    for (key, value) in vars {
        command.env(key, value);
    }
    if !program_args.is_empty() {
        command.arg("--");
        for arg in program_args {
            command.arg(arg);
        }
    }
    let out = command.output().unwrap();
    let _ = fs::remove_dir_all(&dir);
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn interpreter_run(name: &str, src: &str) -> (i32, String, String) {
    let dir = unique_tmp("jet_interpreter_run");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let outcome = jet::Interpreter::dev_iteration(&shown, false, true);
    let _ = fs::remove_dir_all(&dir);
    match outcome {
        jet::Interpreter::RunOutcome::Ran {
            exit_code,
            stdout,
            stderr,
        } => (exit_code, stdout, stderr),
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            panic!("interpreter rejected the tier-comparison source: {diagnostics:?}")
        }
    }
}

/// The same snippet on both tiers, asserting they agree. This is the shape I9
/// actually asks for: not "AOT prints X", but "every tier prints the same X".
pub fn assert_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let (jit_code, jit_out, jit_err) = jit_run(name, src);
    assert_eq!(jit_code, 0, "`jet run` failed:\n{jit_err}");
    assert_eq!(
        jit_out, expected_stdout,
        "`jet run` (Cranelift/interpreter) disagreed:\n{jit_err}"
    );
    let (interpreter_code, interpreter_out, interpreter_err) = interpreter_run(name, src);
    assert_eq!(
        interpreter_code, jit_code,
        "forced interpreter and default JIT exit codes disagree:\ninterpreter stderr: {interpreter_err}\nJIT stderr: {jit_err}"
    );
    assert_eq!(
        interpreter_out, jit_out,
        "forced interpreter and default JIT stdout disagree:\ninterpreter stderr: {interpreter_err}\nJIT stderr: {jit_err}"
    );
    assert_eq!(
        interpreter_err, jit_err,
        "forced interpreter and default JIT stderr disagree"
    );
    if have_rustc() {
        let (aot_code, aot_out, aot_err) = build_and_run_full("jet_tir_test", name, src);
        assert_eq!(aot_code, jit_code, "AOT and `jet run` exit codes disagree:\n{aot_err}");
        assert_eq!(
            aot_out, jit_out,
            "AOT and `jet run` disagree — one tier re-encoded the rule (I9)"
        );
        assert_eq!(
            aot_err, jit_err,
            "AOT and `jet run` stderr disagree — one tier re-encoded the rule (I9)"
        );
        assert_eq!(
            aot_code, interpreter_code,
            "AOT and forced interpreter exit codes disagree"
        );
        assert_eq!(
            aot_out, interpreter_out,
            "AOT and forced interpreter stdout disagree"
        );
        assert_eq!(
            aot_err, interpreter_err,
            "AOT and forced interpreter stderr disagree"
        );
    }
}

pub fn build_and_run_full_with_cfg(
    prefix: &str,
    name: &str,
    src: &str,
    rustc_cfg: &str,
) -> (i32, String, String) {
    build_and_run_full_inner(prefix, name, src, Some(rustc_cfg))
}

fn build_and_run_full_inner(
    prefix: &str,
    name: &str,
    src: &str,
    rustc_cfg: Option<&str>,
) -> (i32, String, String) {
    let dir = unique_tmp(prefix);
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(cfg) = rustc_cfg {
        rustc_cmd.args(["--cfg", cfg]);
    }
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        stderr,
    )
}

/// Build and run a multi-file program from a fresh temporary directory.
#[allow(dead_code)]
pub fn build_and_run_multi(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!(
        "jet_tir_multi_{}_{}",
        name,
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let shown = entry_path.to_string_lossy().into_owned();
    let entry_src = fs::read_to_string(&entry_path).unwrap();
    let out = jet::compile_with_path(&entry_src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, &entry_src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
    )
}

/// Run a multi-file program through the default `jet run` lens.
pub fn run_default_multi(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    let dir = unique_tmp(&format!("jet_jit_multi_{name}"));
    fs::create_dir_all(&dir).unwrap();
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, src).unwrap();
    }
    let run = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", entry, "--trace-tiers"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    (
        run.status.code().unwrap_or(1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

pub fn strip_vetted_prelude_modules(rust_code: &str) -> String {
    fn strip_mod(src: &str, name: &str) -> String {
        let Some(start) = src.find(&format!("mod {name}")) else {
            return src.to_string();
        };
        let bytes = src.as_bytes();
        let Some(open) = bytes[start..]
            .iter()
            .position(|byte| *byte == b'{')
            .map(|offset| start + offset)
        else {
            return src.to_string();
        };
        fn raw_string_start(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
            let raw = if bytes.get(i) == Some(&b'r') {
                i
            } else if bytes.get(i) == Some(&b'b') && bytes.get(i + 1) == Some(&b'r') {
                i + 1
            } else {
                return None;
            };
            let mut quote = raw + 1;
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
            (bytes.get(quote) == Some(&b'"')).then_some((quote, quote - raw - 1))
        }
        #[derive(Clone, Copy)]
        enum State {
            Normal,
            LineComment,
            BlockComment(usize),
            String,
            RawString(usize),
        }
        let mut depth = 1usize;
        let mut i = open + 1;
        let mut end = src.len();
        let mut state = State::Normal;
        while i < bytes.len() {
            match state {
                State::Normal => {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                        state = State::LineComment;
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        state = State::BlockComment(1);
                        i += 2;
                        continue;
                    }
                    if let Some((quote, hashes)) = raw_string_start(bytes, i) {
                        state = State::RawString(hashes);
                        i = quote + 1;
                        continue;
                    }
                    if bytes[i] == b'"' || (bytes[i] == b'b' && bytes.get(i + 1) == Some(&b'"')) {
                        state = State::String;
                        i += usize::from(bytes[i] == b'b') + 1;
                        continue;
                    }
                    if bytes[i] == b'\'' {
                        let mut j = i + 1;
                        let mut escaped = false;
                        let mut closed = false;
                        while j < bytes.len() && bytes[j] != b'\n' {
                            if escaped {
                                escaped = false;
                            } else if bytes[j] == b'\\' {
                                escaped = true;
                            } else if bytes[j] == b'\'' {
                                closed = true;
                                break;
                            }
                            j += 1;
                        }
                        if closed {
                            i = j + 1;
                            continue;
                        }
                        i += 1;
                        continue;
                    }
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                State::LineComment => {
                    if bytes[i] == b'\n' {
                        state = State::Normal;
                    }
                }
                State::BlockComment(comment_depth) => {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        state = State::BlockComment(comment_depth + 1);
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        i += 2;
                        if comment_depth == 1 {
                            state = State::Normal;
                        } else {
                            state = State::BlockComment(comment_depth - 1);
                        }
                        continue;
                    }
                }
                State::String => {
                    if bytes[i] == b'\\' {
                        i += 1;
                    } else if bytes[i] == b'"' {
                        state = State::Normal;
                    }
                }
                State::RawString(hashes) => {
                    if bytes[i] == b'"'
                        && (0..hashes)
                            .all(|offset| bytes.get(i + 1 + offset) == Some(&b'#'))
                    {
                        i += 1 + hashes;
                        state = State::Normal;
                        continue;
                    }
                }
            }
            i += 1;
        }
        format!("{}{}", &src[..start], &src[end..])
    }
    fn strip_jet_cell(src: &str) -> String {
        // Codegen emits this module as one contiguous source part: the opening
        // marker in `CORELIB_KERNEL_PARTS`, `LocalCell.rs`, then this exact
        // closing/re-export marker. Use that source boundary instead of
        // guessing where a Rust token-tree brace belongs.
        const BEGIN: &str = "\nmod jet_cell {\n";
        const END: &str =
            "\n}\npub use self::jet_cell::{JetCell, JetCellEditGuard, JetCellReadGuard};\n";
        let Some(start) = src.find(BEGIN) else {
            return src.to_string();
        };
        let body_start = start + BEGIN.len();
        let Some(relative_end) = src[body_start..].find(END) else {
            return src.to_string();
        };
        let end = body_start + relative_end + END.len();
        format!("{}{}", &src[..start], &src[end..])
    }
    let s = strip_mod(rust_code, "jet_uninit_semantics");
    let s = strip_mod(&s, "jet_mem");
    let s = strip_jet_cell(&s);
    let s = strip_mod(&s, "jet_txn");
    let s = strip_mod(&s, "jet_term_unix");
    let s = strip_mod(&s, "jet_term_windows");
    let s = strip_mod(&s, "jet_process_pty");
    let s = strip_mod(&s, "jet_os_unix");
    let s = strip_mod(&s, "jet_atomic_windows");
    let s = strip_mod(&s, "jet_gtk");
    let s = strip_mod(&s, "jet_crypto_entropy");
    let mut s = strip_scheduler_native(&s);
    s = strip_marked_regions(
        &s,
        "// jet:shared-guard-internal-begin",
        "// jet:shared-guard-internal-end",
    );
    s = strip_vetted_module(&s, "jet_env_windows");
    s = strip_vetted_module(&s, "jet_watch_process_probe");
    s = strip_vetted_module(&s, "ffi_reporter");
    while s.contains("mod __jet___c_") {
        let before = s.clone();
        s = strip_mod(&s, "__jet___c_");
        if s == before {
            break;
        }
    }
    s
}

fn strip_scheduler_native(src: &str) -> String {
    let begin = "// jet:scheduler-native-begin";
    let end = "// jet:scheduler-native-end";
    match (src.find(begin), src.find(end)) {
        (Some(b), Some(e)) if e >= b => {
            let mut s = src[..b].to_string();
            s.push_str(&src[e + end.len()..]);
            s
        }
        _ => src.to_string(),
    }
}

fn strip_marked_regions(src: &str, begin: &str, end: &str) -> String {
    let mut out = src.to_string();
    loop {
        let (Some(start), Some(end_pos)) = (out.find(begin), out.find(end)) else {
            return out;
        };
        if end_pos < start {
            return out;
        }
        let end_offset = end_pos + end.len();
        out = format!("{}{}", &out[..start], &out[end_offset..]);
    }
}

fn strip_vetted_module(src: &str, name: &str) -> String {
    let begin = format!("// JET_VETTED_UNSAFE_BEGIN: {name}");
    let end = format!("// JET_VETTED_UNSAFE_END: {name}");
    let Some(start) = src.find(&begin) else {
        return src.to_string();
    };
    let Some(relative_end) = src[start + begin.len()..].find(&end) else {
        return src.to_string();
    };
    let end_offset = start + begin.len() + relative_end + end.len();
    format!("{}{}", &src[..start], &src[end_offset..])
}

#[test]
fn watcher_process_probe_is_vetted_without_hiding_user_unsafe() {
    let generated = "// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe\nunsafe { ffi() }\n// JET_VETTED_UNSAFE_END: jet_watch_process_probe\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}
