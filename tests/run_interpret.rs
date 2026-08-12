mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn run_interpret_forces_tier_zero_without_watch() {
    let dir = std::env::temp_dir().join(format!("jet_run_interpret_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    let marker = format!("run-interpreter-{}", std::process::id());
    fs::write(&file, format!("fn run() {{\n    print(\"{marker}\")\n}}\n")).unwrap();

    let default = Command::new(jet())
        .args(["run", "--trace-tiers", "main.jet"])
        .current_dir(&dir)
        .env("JET_RUN_CACHE_DIR", dir.join("run-cache"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        default.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&default.stderr)
    );
    assert!(String::from_utf8_lossy(&default.stderr).contains("tier1 native"));

    let forced = Command::new(jet())
        .args(["run", "--trace-tiers", "--interpret", "main.jet"])
        .current_dir(&dir)
        .env("JET_RUN_CACHE_DIR", dir.join("run-cache"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        forced.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(!String::from_utf8_lossy(&forced.stderr).contains("tier1 native"));
    assert_eq!(forced.stdout, default.stdout);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_interpret_keeps_unused_c_member_lists_runnable() {
    let dir = std::env::temp_dir().join(format!("jet_run_interpret_imports_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_interpret_imports\"\nversion: \"0.1.0\"\ndeps: .{ c: c@system, m: c@system }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.math.[abs, min]\nuse core.encoding.[json, csv]\nuse c.[c as libc, m]\nfn run() { print(abs(-8)); print(min(9, 4)) }\n",
    )
    .unwrap();

    let run = |interpret: bool| {
        let cache = dir.join(if interpret { "cache-interpreter" } else { "cache-default" });
        let mut command = Command::new(jet());
        command
            .args(["run", "--trace-tiers"])
            .args(interpret.then_some("--interpret"))
            .arg("main.jet")
            .current_dir(&dir)
            .env("JET_RUN_CACHE_DIR", cache.join("run"))
            .env("JET_CACHE_DIR", cache.join("build"))
            .env("NO_COLOR", "1");
        command.output().unwrap()
    };

    let default = run(false);
    assert_eq!(default.status.code(), Some(0), "{}", String::from_utf8_lossy(&default.stderr));
    let forced = run(true);
    assert_eq!(forced.status.code(), Some(0), "{}", String::from_utf8_lossy(&forced.stderr));
    assert_eq!(forced.stdout, default.stdout);
    assert!(!String::from_utf8_lossy(&forced.stderr).contains("E2201"));
    let _ = fs::remove_dir_all(&dir);
}
