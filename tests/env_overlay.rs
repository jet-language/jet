use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(name: &str, src: &str) -> (PathBuf, jet::CompileOutput) {
    let dir = std::env::temp_dir().join(format!("jet_env_overlay_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    (dir, out)
}

fn build(dir: &Path, name: &str, out: &jet::CompileOutput) -> PathBuf {
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    bin
}

#[cfg(unix)]
#[test]
fn mutation_enumeration_and_child_snapshot_run() {
    let src = r#"
use core.env as env
use core.process as process

fn run() {
    env.set("JET_OVERLAY_A", "one")
    env.set("JET_OVERLAY_B", "two")
    print(env.get("JET_OVERLAY_A") ?? "missing")
    names :: env.vars() ?? panic("vars failed")
    print(names.join(",").contains("JET_OVERLAY_A,JET_OVERLAY_B"))
    print(env.unset("JET_OVERLAY_A") ?? false)
    print(env.unset("JET_OVERLAY_A") ?? true)
    inherited :: process.cmd(["/usr/bin/env"]).run() ?? panic("child failed")
    print(inherited.output.contains("JET_OVERLAY_B=two"))
    print(!inherited.output.contains("JET_OVERLAY_A="))
    cleared :: process.cmd(["/usr/bin/env"])
        .env_clear()
        .env("ONLY_THIS", "yes")
        .env_remove("JET_OVERLAY_B")
        .run() ?? panic("clear child failed")
    print(cleared.output == "ONLY_THIS=yes\n")
}
"#;
    let (dir, out) = compile("mutation", src);
    let bin = build(&dir, "mutation", &out);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "one\ntrue\ntrue\nfalse\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn codegen_is_raw_locked_and_never_mutates_host_env() {
    let (_, out) = compile(
        "shape",
        r#"
use core.env as env
use core.process as process

fn run() {
    env.set("MODE", "test")
    _ :: env.unset("CI")
    _ :: env.vars()
    _ :: process.run(["worker"])
}
"#,
    );
    assert!(out.rust.contains("std::env::vars_os()"));
    assert!(out.rust.contains("RwLock<Vec<(std::ffi::OsString, std::ffi::OsString)>>"));
    assert!(out.rust.contains("command.env_clear()"));
    assert!(out.rust.contains("command.envs(child_env)"));
    assert!(!out.rust.contains("std::env::set_var"));
    assert!(!out.rust.contains("std::env::remove_var"));
}

#[test]
fn old_edition_invalid_set_uses_exact_runtime_error() {
    let src = r#"
use core.env as env

fn run() {
    env.set("", "value")
}
"#;
    let (dir, out) = compile("invalid", src);
    let bin = build(&dir, "invalid", &out);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(run.status.code(), Some(70));
    assert!(stderr.contains("panic: core.env.set: invalid environment variable name"));
    assert!(stderr.contains("invalid.jet:5 in run"), "missing call-site frame: {stderr}");
}
