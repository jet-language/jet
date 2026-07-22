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

pub fn build_and_run_full(prefix: &str, name: &str, src: &str) -> (i32, String, String) {
    build_and_run_full_inner(prefix, name, src, None)
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

pub fn strip_vetted_prelude_modules(rust_code: &str) -> String {
    fn strip_mod(src: &str, name: &str) -> String {
        let Some(start) = src.find(&format!("mod {name}")) else {
            return src.to_string();
        };
        let bytes = src.as_bytes();
        let mut depth = 0usize;
        let mut i = start;
        let mut end = src.len();
        let mut seen_brace = false;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => {
                    depth += 1;
                    seen_brace = true;
                }
                b'}' => {
                    depth -= 1;
                    if seen_brace && depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        format!("{}{}", &src[..start], &src[end..])
    }
    let s = strip_mod(rust_code, "jet_mem");
    let s = strip_mod(&s, "jet_txn");
    let s = strip_mod(&s, "jet_term_unix");
    let s = strip_mod(&s, "jet_term_windows");
    let s = strip_mod(&s, "jet_os_unix");
    let s = strip_mod(&s, "jet_atomic_windows");
    let s = strip_mod(&s, "jet_gtk");
    let s = strip_mod(&s, "jet_crypto_entropy");
    let mut s = strip_scheduler_native(&s);
    s = strip_vetted_module(&s, "jet_env_windows");
    s = strip_vetted_module(&s, "jet_watch_process_probe");
    while s.contains("mod user___c_") {
        let before = s.clone();
        s = strip_mod(&s, "user___c_");
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
