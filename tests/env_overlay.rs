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
    build_rust(dir, name, &out.rust)
}

fn build_rust(dir: &Path, name: &str, rust: &str) -> PathBuf {
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, rust).unwrap();
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
    removed_wins :: process.cmd(["/usr/bin/env"])
        .env_clear()
        .env("SAME_KEY", "set-first")
        .env_remove("SAME_KEY")
        .run() ?? panic("precedence child failed")
    print(!removed_wins.output.contains("SAME_KEY="))
}
"#;
    let (dir, out) = compile("mutation", src);
    let bin = build(&dir, "mutation", &out);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "one\ntrue\ntrue\nfalse\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn codegen_is_raw_locked_and_never_mutates_host_env() {
    let (_, out) = compile(
        "shape",
        r#"
use core.env as env
use core.process as process

fn remove(name: String) -> Bool ? env.EnvError {
    return ok(env.unset(name)?)
}

fn run() {
    env.set("MODE", "test")
    removed :: remove("CI")
    names :: env.vars()
    child :: process.run(["worker"])
}
"#,
    );
    assert!(out.rust.contains("std::env::vars_os()"));
    assert!(out.rust.contains("type JetEnvEntries = Vec<(std::ffi::OsString, std::ffi::OsString)>"));
    assert!(out.rust.contains("RwLock<JetEnvEntries>"));
    assert!(out.rust.contains("fn main() {\n    jet_std_env_init();"));
    assert!(out.rust.contains("command.env_clear()"));
    assert!(out.rust.contains("command.envs(child_env)"));
    assert!(out.rust.contains("i32::try_from(left.len())"));
    assert!(out.rust.contains("2 => std::cmp::Ordering::Equal"));
    assert!(out.rust.contains("_ => left.cmp(&right)"));
    assert!(!out.rust.contains("std::env::set_var"));
    assert!(!out.rust.contains("std::env::remove_var"));
}

#[cfg(unix)]
#[test]
fn raw_non_unicode_value_survives_child_snapshot_and_vars_fails_whole_snapshot() {
    use std::os::unix::ffi::OsStringExt;

    let src = r#"
use core.env as env
use core.process as process

fn run() {
    child :: process.run(["./raw_probe"]) ?? panic("raw child failed")
    print(child.output)
    if env.vars() == {
        ok(_) -> { print("unexpected vars success") }
        err(e) -> { print(e) }
    }
}
"#;
    let (dir, out) = compile("raw", src);
    let probe_rs = dir.join("raw_probe.rs");
    fs::write(
        &probe_rs,
        r#"
use std::os::unix::ffi::OsStrExt;
fn main() {
    let raw_value = std::env::var_os("JET_RAW_VALUE").expect("missing raw value");
    println!("{}", raw_value.as_os_str().as_bytes() == [0x66, 0x80, 0x6f]);
    let raw_name = [b'J', b'E', b'T', b'_', 0x81];
    let found = std::env::vars_os().any(|(name, value)| {
        name.as_os_str().as_bytes() == raw_name
            && value.as_os_str().as_bytes() == [0x76, 0x82, 0x6c]
    });
    println!("{}", found);
}
"#,
    )
    .unwrap();
    let probe_build = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&probe_rs)
        .arg("-o")
        .arg(dir.join("raw_probe"))
        .output()
        .unwrap();
    assert!(probe_build.status.success(), "{}", String::from_utf8_lossy(&probe_build.stderr));
    let bin = build(&dir, "raw", &out);
    let run = Command::new(bin)
        .current_dir(&dir)
        .env(
            "JET_RAW_VALUE",
            std::ffi::OsString::from_vec(vec![0x66, 0x80, 0x6f]),
        )
        .env(
            std::ffi::OsString::from_vec(vec![b'J', b'E', b'T', b'_', 0x81]),
            std::ffi::OsString::from_vec(vec![0x76, 0x82, 0x6c]),
        )
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\ntrue\n\nenvironment contains a name or value that is not valid Unicode\n"
    );
}

#[test]
fn eager_snapshot_and_host_getenv_isolation_run_in_process() {
    let src = r#"
use core.env as env

fn run() {
    print(env.get("JET_HOST_ISOLATION") ?? "missing")
    env.set("JET_HOST_ISOLATION", "jet-owned")
    print(env.get("JET_HOST_ISOLATION") ?? "missing")
}
"#;
    let (dir, out) = compile("host_isolation", src);
    // Test-only vetted probe: mutate the real host block after main's mandatory
    // snapshot and verify both directions of isolation in this same process.
    let rust = out.rust.replacen(
        "    jet_std_env_init();\n    user_run();",
        "    jet_std_env_init();\n    std::env::set_var(\"JET_HOST_ISOLATION\", \"host-after-snapshot\");\n    user_run();\n    assert_eq!(std::env::var(\"JET_HOST_ISOLATION\").as_deref(), Ok(\"host-after-snapshot\"));",
        1,
    );
    assert_ne!(rust, out.rust, "test probe did not attach after eager init");
    let bin = build_rust(&dir, "host_isolation", &rust);
    let run = Command::new(bin)
        .current_dir(&dir)
        .env("JET_HOST_ISOLATION", "host-before-snapshot")
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "host-before-snapshot\njet-owned\n"
    );
}

#[cfg(unix)]
#[test]
fn concurrent_mutation_and_real_child_spawns_take_untorn_snapshots() {
    let (dir, out) = compile("atomic_spawn", "fn run() {}\n");
    let probe = r#"    jet_std_env_init();
    let key = "JET_ATOMIC_SNAPSHOT".to_string();
    let a = "A".repeat(4096);
    let b = "B".repeat(4096);
    jet_std_env_set(&key, &a).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer_key = key.clone();
    let writer_a = a.clone();
    let writer_b = b.clone();
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        for i in 0..4000 {
            let value = if i % 2 == 0 { &writer_b } else { &writer_a };
            jet_std_env_set(&writer_key, value).unwrap();
        }
    });
    barrier.wait();
    for _ in 0..12 {
        let result = jet_std_process_run(&vec!["/usr/bin/env".to_string()]).unwrap();
        let value = result.output.lines()
            .find_map(|line| line.strip_prefix("JET_ATOMIC_SNAPSHOT="))
            .expect("child lost snapshot key");
        assert!(value == a || value == b, "child observed torn environment value");
    }
    writer.join().unwrap();
    user_run();"#;
    let rust = out.rust.replacen(
        "    jet_std_env_init();\n    user_run();",
        probe,
        1,
    );
    assert_ne!(rust, out.rust, "atomicity probe did not attach");
    let bin = build_rust(&dir, "atomic_spawn", &rust);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
}

#[test]
fn fallible_set_runtime_hook_is_typed_for_next_edition() {
    let (dir, out) = compile("fallible_set_hook", "fn run() {}\n");
    let probe = r#"    jet_std_env_init();
    let invalid_name: Result<(), jet_std::EnvError> =
        jet_std_env_set(&"".to_string(), &"value".to_string());
    assert!(matches!(invalid_name, Err(jet_std::EnvError::InvalidName)));
    let invalid_value: Result<(), jet_std::EnvError> =
        jet_std_env_set(&"name".to_string(), &"bad\0value".to_string());
    assert!(matches!(invalid_value, Err(jet_std::EnvError::InvalidValue)));
    user_run();"#;
    let rust = out.rust.replacen(
        "    jet_std_env_init();\n    user_run();",
        probe,
        1,
    );
    assert_ne!(rust, out.rust, "fallible-set probe did not attach");
    let bin = build_rust(&dir, "fallible_set_hook", &rust);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
}

#[cfg(windows)]
#[test]
fn windows_casefold_last_spelling_and_child_inheritance_run_natively() {
    let src = r#"
use core.env as env
use core.process as process

fn run() {
    env.set("Jet_Win_Case", "first")
    print(env.get("jET_wIN_cASE") ?? "missing")
    env.set("JET_WIN_CASE", "last-✓")
    print(env.get("jet_win_case") ?? "missing")
    names :: env.vars() ?? panic("vars failed")
    print(names.join(",").contains("JET_WIN_CASE"))
    child :: process.run(["cmd.exe", "/C", "echo %jet_win_case%"])
        ?? panic("child failed")
    print(child.output.contains("last-✓"))
}
"#;
    let (dir, out) = compile("windows_native", src);
    let bin = build(&dir, "windows_native", &out);
    let run = Command::new(bin).current_dir(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "first\nlast-✓\ntrue\ntrue\n");
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
