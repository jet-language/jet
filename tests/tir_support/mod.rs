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
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .arg("run")
        .arg(&jet_path)
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

/// The same snippet on both tiers, asserting they agree. This is the shape I9
/// actually asks for: not "AOT prints X", but "every tier prints the same X".
pub fn assert_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let (jit_code, jit_out, jit_err) = jit_run(name, src);
    assert_eq!(jit_code, 0, "`jet run` failed:\n{jit_err}");
    assert_eq!(
        jit_out, expected_stdout,
        "`jet run` (Cranelift/interpreter) disagreed:\n{jit_err}"
    );
    if have_rustc() {
        let (aot_code, aot_out) = build_and_run(name, src);
        assert_eq!(aot_code, 0, "AOT run failed:\n{aot_out}");
        assert_eq!(
            aot_out, jit_out,
            "AOT and `jet run` disagree — one tier re-encoded the rule (I9)"
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

fn rust_skip_space_comments(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while bytes.get(i).is_some_and(|byte| byte.is_ascii_whitespace()) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                if bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes.get(i) == Some(&b'*') && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        return i;
    }
}

fn rust_raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut marker = start;
    if bytes.get(marker) == Some(&b'b') {
        marker += 1;
    }
    if bytes.get(marker) != Some(&b'r') {
        return None;
    }
    let mut quote = marker + 1;
    while bytes.get(quote) == Some(&b'#') {
        quote += 1;
    }
    if bytes.get(quote) != Some(&b'"') {
        return None;
    }
    let hashes = quote - marker - 1;
    let mut i = quote + 1;
    while i < bytes.len() {
        if bytes[i] == b'"'
            && (0..hashes).all(|offset| bytes.get(i + 1 + offset) == Some(&b'#'))
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    None
}

fn rust_quoted_end(bytes: &[u8], quote: usize) -> Option<usize> {
    let mut i = quote + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = i.saturating_add(2);
        } else if bytes[i] == bytes[quote] {
            return Some(i + 1);
        } else if bytes[i] == b'\n' && bytes[quote] == b'\'' {
            return None;
        } else {
            i += 1;
        }
    }
    None
}

fn rust_atom_end(bytes: &[u8], i: usize) -> Option<usize> {
    if let Some(end) = rust_raw_string_end(bytes, i) {
        return Some(end);
    }
    if bytes.get(i) == Some(&b'b')
        && (bytes.get(i + 1) == Some(&b'"') || bytes.get(i + 1) == Some(&b'\''))
    {
        return rust_quoted_end(bytes, i + 1);
    }
    if bytes.get(i) == Some(&b'"') {
        return rust_quoted_end(bytes, i);
    }
    if bytes.get(i) != Some(&b'\'') {
        return None;
    }
    let Some(&next) = bytes.get(i + 1) else {
        return None;
    };
    if next.is_ascii_alphanumeric() || next == b'_' {
        let mut j = i + 2;
        while bytes.get(j).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
            j += 1;
        }
        return Some(if bytes.get(j) == Some(&b'\'') { j + 1 } else { j });
    }
    rust_quoted_end(bytes, i)
}

fn rust_token_spans(src: &str) -> Vec<(usize, usize)> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        i = rust_skip_space_comments(bytes, i);
        if i >= bytes.len() {
            break;
        }
        if let Some(end) = rust_atom_end(bytes, i) {
            i = end;
            continue;
        }
        let start = i;
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            i += 1;
            while bytes
                .get(i)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                i += 1;
            }
        } else {
            i += 1;
        }
        tokens.push((start, i));
    }
    tokens
}

fn rust_matching_brace(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 1usize;
    let mut i = open + 1;
    while i < bytes.len() {
        i = rust_skip_space_comments(bytes, i);
        if i >= bytes.len() {
            break;
        }
        if let Some(end) = rust_atom_end(bytes, i) {
            i = end;
            continue;
        }
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn rust_module_span(src: &str, name: &str) -> Option<(usize, usize)> {
    let tokens = rust_token_spans(src);
    for (index, &(mod_start, _)) in tokens.iter().enumerate() {
        if &src[mod_start..tokens[index].1] != "mod" || index + 2 >= tokens.len() {
            continue;
        }
        let candidate = &src[tokens[index + 1].0..tokens[index + 1].1];
        let matches = if name == "__jet___c_" {
            candidate.starts_with(name)
        } else {
            candidate == name
        };
        if !matches || &src[tokens[index + 2].0..tokens[index + 2].1] != "{" {
            continue;
        }
        let start = if index > 0
            && &src[tokens[index - 1].0..tokens[index - 1].1] == "pub"
        {
            tokens[index - 1].0
        } else {
            mod_start
        };
        let end = rust_matching_brace(src, tokens[index + 2].0)?;
        return Some((start, end));
    }
    None
}

fn strip_rust_module(src: &str, name: &str) -> String {
    let Some((start, end)) = rust_module_span(src, name) else {
        return src.to_string();
    };
    format!("{}{}", &src[..start], &src[end..])
}

fn strip_rust_reexport(src: &str, name: &str) -> String {
    let tokens = rust_token_spans(src);
    let prefix = ["pub", "use", "self", ":", ":", name, ":", ":"];
    for index in 0..tokens.len() {
        if index + prefix.len() >= tokens.len()
            || !prefix.iter().enumerate().all(|(offset, expected)| {
                &src[tokens[index + offset].0..tokens[index + offset].1] == *expected
            })
        {
            continue;
        }
        let open = index + prefix.len();
        if &src[tokens[open].0..tokens[open].1] != "{" {
            continue;
        }
        let Some(close) = rust_matching_brace(src, tokens[open].0) else {
            continue;
        };
        let Some(semicolon) = tokens
            .iter()
            .position(|&(start, end)| start >= close && &src[start..end] == ";")
        else {
            continue;
        };
        return format!("{}{}", &src[..tokens[index].0], &src[tokens[semicolon].1..]);
    }
    src.to_string()
}

pub fn strip_vetted_prelude_modules(rust_code: &str) -> String {
    let s = strip_rust_module(rust_code, "jet_uninit_semantics");
    let s = strip_rust_module(&s, "jet_mem");
    let s = strip_rust_module(&s, "jet_cell");
    let s = strip_rust_reexport(&s, "jet_cell");
    let s = strip_rust_module(&s, "jet_txn");
    let s = strip_rust_module(&s, "jet_term_unix");
    let s = strip_rust_module(&s, "jet_term_windows");
    let s = strip_rust_module(&s, "jet_process_pty");
    let s = strip_rust_module(&s, "jet_os_unix");
    let s = strip_rust_module(&s, "jet_atomic_windows");
    let s = strip_rust_module(&s, "jet_gtk");
    let s = strip_rust_module(&s, "jet_crypto_entropy");
    let mut s = strip_scheduler_native(&s);
    s = strip_marked_regions(
        &s,
        "// jet:shared-guard-internal-begin",
        "// jet:shared-guard-internal-end",
    );
    s = strip_vetted_module(&s, "jet_env_windows");
    s = strip_vetted_module(&s, "jet_watch_process_probe");
    s = strip_vetted_module(&s, "ffi_reporter");
    while rust_module_span(&s, "__jet___c_").is_some() {
        let before = s.clone();
        s = strip_rust_module(&s, "__jet___c_");
        if s == before {
            break;
        }
    }
    s
}

pub fn assert_user_unsafe_is_gated(src: &str) {
    let tokens = rust_token_spans(src);
    for (index, &(start, end)) in tokens.iter().enumerate() {
        if &src[start..end] != "unsafe" {
            continue;
        }
        let next = tokens
            .get(index + 1)
            .map(|&(next_start, next_end)| &src[next_start..next_end]);
        assert!(
            matches!(next, Some("{") | Some("fn")),
            "I1: ungated `unsafe` in generated code at byte {start}"
        );
    }
}

fn strip_scheduler_native(src: &str) -> String {
    strip_marked_regions(src, "// jet:scheduler-native-begin", "// jet:scheduler-native-end")
}

fn strip_marked_regions(src: &str, begin: &str, end: &str) -> String {
    fn is_marker(line: &str, marker: &str) -> bool {
        let Some(suffix) = line.strip_prefix(marker) else {
            return false;
        };
        suffix.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|character| character.is_whitespace() || character == '—')
    }

    let mut out = String::with_capacity(src.len());
    let mut removing = false;
    let mut found_pair = false;
    for line in src.split_inclusive('\n') {
        let marker = line.trim();
        if !removing && is_marker(marker, begin) {
            removing = true;
            continue;
        }
        if removing {
            if is_marker(marker, end) {
                removing = false;
                found_pair = true;
            }
            continue;
        }
        out.push_str(line);
    }
    if removing || !found_pair {
        src.to_string()
    } else {
        out
    }
}

fn strip_vetted_module(src: &str, name: &str) -> String {
    let begin = format!("// JET_VETTED_UNSAFE_BEGIN: {name}");
    let end = format!("// JET_VETTED_UNSAFE_END: {name}");
    strip_marked_regions(src, &begin, &end)
}

#[test]
fn watcher_process_probe_is_vetted_without_hiding_user_unsafe() {
    let generated = "// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe\nunsafe { ffi() }\n// JET_VETTED_UNSAFE_END: jet_watch_process_probe\nunsafe { user_pointer() }";
    let stripped = strip_vetted_prelude_modules(generated);
    assert!(!stripped.contains("ffi()"));
    assert!(stripped.contains("unsafe { user_pointer() }"));
}
