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
    assert!(
        !default.stdout.is_empty(),
        "default tier returned success with empty stdout: {}",
        String::from_utf8_lossy(&default.stderr)
    );
    assert_eq!(default.stdout, format!("{marker}\n").into_bytes());
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
    assert!(!forced.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&forced.stderr).contains("tier1 native"));
    assert_eq!(forced.stdout, default.stdout);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_interpret_rejects_release_profile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(jet())
        .args([
            "run",
            "--interpret",
            "--release",
            "examples/features/errors/result_handler.jet",
        ])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E2102"), "{stderr}");
    assert!(stderr.contains("--interpret"), "{stderr}");
}

/// Card #2250 / I9: one invocation argv and its user-argument projection must
/// survive every native execution adapter. The first item is the tier's
/// program identity, so the observable comparison covers the forwarded
/// program arguments while the same source proves flags, filenames, Unicode,
/// an empty value, and a bare -- survive the CLI split unchanged.
#[test]
fn argv_agrees_on_every_native_tier() {
    let dir = std::env::temp_dir().join(format!(
        "jet_run_interpret_argv_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_interpret_argv\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [Exec, IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.process as process\n\nfn run() {\n    args :: process.argv()\n    user_args :: process.args()\n    print(args.len())\n    print(user_args.len())\n    loop arg in user_args {\n        print(\"<{arg}>\")\n    }\n}\n",
    )
    .unwrap();

    let program_args = ["--flag", "--port=50000", "report file.jet", "Δ", "", "--"];
    let cache = dir.join("cache");
    let run = |label: &str, args: &[&str]| {
        let output = Command::new(jet())
            .args(args)
            .current_dir(&dir)
            .env("JET_RUN_CACHE_DIR", cache.join(label).join("run"))
            .env("JET_CACHE_DIR", cache.join(label).join("build"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("{label} should start: {error}"));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{label} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{label} wrote stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };

    let invocation = |label: &str, command: &str, extra: &[&str]| {
        let mut args = vec![command];
        args.extend_from_slice(extra);
        args.push("main.jet");
        args.push("--");
        args.extend_from_slice(&program_args);
        run(label, &args)
    };
    let default = invocation("default", "run", &[]);
    let interpret = invocation("interpret", "run", &["--interpret"]);
    let release = invocation("release", "run", &["--release"]);
    let dev = {
        let mut args = vec!["dev", "--watch=off", "main.jet", "--"];
        args.extend_from_slice(&program_args);
        run("dev", &args)
    };

    let build = Command::new(jet())
        .args(["build", "main.jet", "--quiet"])
        .current_dir(&dir)
        .env("JET_CACHE_DIR", cache.join("aot-build"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        build.status.code(),
        Some(0),
        "AOT build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let aot = Command::new(dir.join("build/main"))
        .args(program_args)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        aot.status.code(),
        Some(0),
        "AOT binary failed:\n{}",
        String::from_utf8_lossy(&aot.stderr)
    );

    for (label, output) in [
        ("interpret", interpret),
        ("release", release),
        ("dev", dev),
        ("AOT", aot.stdout),
    ] {
        assert_eq!(output, default, "{label} argv output diverged");
    }
    let expected = "7\n6\n<--flag>\n<--port=50000>\n<report file.jet>\n<Δ>\n<>\n<-->\n";
    assert_eq!(
        default.as_slice(),
        expected.as_bytes(),
        "forwarded argv was not preserved"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_interpret_keeps_unused_c_member_lists_runnable() {
    let dir =
        std::env::temp_dir().join(format!("jet_run_interpret_imports_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_interpret_imports\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [IO] } }\ndeps: .{ c: c@system, m: c@system }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.math.[abs, min]\nuse core.encoding.[json, csv]\nuse c.[c as libc, m]\nfn run() { print(abs(-8)); print(min(9, 4)) }\n",
    )
    .unwrap();

    let run = |interpret: bool| {
        let cache = dir.join(if interpret {
            "cache-interpreter"
        } else {
            "cache-default"
        });
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
    assert_eq!(
        default.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&default.stderr)
    );
    let forced = run(true);
    assert_eq!(
        forced.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert_eq!(forced.stdout, default.stdout);
    assert!(!String::from_utf8_lossy(&forced.stderr).contains("E2201"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn c_extern_calls_match_aot_and_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cache = std::env::temp_dir().join(format!("jet_c_extern_parity_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);

    let run = |mode: &str| {
        let mut command = Command::new(jet());
        command.arg("run");
        match mode {
            "release" => {
                command.arg("--release");
            }
            "interpret" => {
                command.arg("--interpret");
            }
            _ => {}
        }
        command
            .arg("examples/features/lowlevel/cbind/run.jet")
            .current_dir(&root)
            .env("JET_RUN_CACHE_DIR", cache.join(mode).join("run"))
            .env("JET_CACHE_DIR", cache.join(mode).join("build"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };

    let release = run("release");
    let default = run("default");
    let interpret = run("interpret");
    for (mode, output) in [
        ("release", &release),
        ("default", &default),
        ("interpret", &interpret),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{mode} C extern run failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"10\n7\n", "{mode} C extern output");
        assert!(
            output.stderr.is_empty(),
            "{mode} C extern stderr: {:?}",
            output.stderr
        );
    }

    let _ = fs::remove_dir_all(&cache);
}

/// Cards #2014/#2015 (I9): an example AOT completes must complete identically
/// on default `jet run` and on the forced tier-0 interpreter — same stdout,
/// same stderr, same exit code — and the forced run must prove tier 0 answered.
/// Without the tier proof a silent deopt hands the test the right bytes from
/// the wrong engine and the interpreter gap stays invisible.
fn assert_example_tier_parity(tag: &str, example: &str, golden: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected = fs::read_to_string(root.join(golden)).expect("example golden output");
    let cache = std::env::temp_dir().join(format!("jet_tier_parity_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);

    let run = |mode: &str, trace: bool| {
        let mut command = Command::new(jet());
        command.arg("run");
        if trace {
            command.arg("--trace-tiers");
        }
        match mode {
            "release" => {
                command.arg("--release");
            }
            "interpret" => {
                command.arg("--interpret");
            }
            _ => {}
        }
        command
            .arg(example)
            .current_dir(&root)
            .env("JET_RUN_CACHE_DIR", cache.join(mode).join("run"))
            .env("JET_CACHE_DIR", cache.join(mode).join("build"))
            .env("NO_COLOR", "1")
            .output()
            .expect("jet run")
    };

    // Byte-exact differential. No `--trace-tiers`, so stderr carries only what
    // the program itself wrote and the three tiers must agree on all of it.
    for mode in ["release", "default", "interpret"] {
        let output = run(mode, false);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{tag}: `{mode}` exited nonzero:\n{stderr}"
        );
        assert_eq!(stderr, "", "{tag}: `{mode}` wrote to stderr");
        assert_eq!(
            stdout, expected,
            "{tag}: `{mode}` stdout diverged from the golden"
        );
    }

    // Tier proof. The forced run must be answered by tier 0 and nothing else,
    // otherwise the differential above measured the wrong engine. Default
    // `jet run` is only required to name `run` in the trace: which of the two
    // tiers claims these stems is Cranelift-coverage work tracked separately,
    // and pinning it here would assert another lane's fix.
    let forced = run("interpret", true);
    let forced_trace = String::from_utf8_lossy(&forced.stderr).into_owned();
    assert!(
        forced_trace
            .lines()
            .any(|line| line.starts_with("run") && line.contains("tier0 interp")),
        "{tag}: forced `--interpret` did not run `run` on tier 0:\n{forced_trace}"
    );
    assert!(
        !forced_trace.contains("tier1 native"),
        "{tag}: forced `--interpret` reached the resident JIT:\n{forced_trace}"
    );

    let default = run("default", true);
    let default_trace = String::from_utf8_lossy(&default.stderr).into_owned();
    assert!(
        default_trace.lines().any(|line| {
            line.starts_with("run")
                && (line.contains("tier1 native") || line.contains("tier0 interp"))
        }),
        "{tag}: default `jet run` reported no tier for `run`:\n{default_trace}"
    );

    let _ = fs::remove_dir_all(&cache);
}

/// Card #2014: `task.all` over children that borrow disjoint places of an owner
/// list. The interpreter used to fail the join outright because the child
/// thread could not name the owner local behind the write window.
#[test]
fn scoped_borrow_bands_agrees_on_every_tier() {
    assert_example_tier_parity(
        "scoped_borrow_bands",
        "examples/features/concurrency/scoped_borrow_bands.jet",
        "examples/features/expected/concurrency/scoped_borrow_bands.out",
    );
}

/// Card #2015: `data.json<T>` into a user struct. The interpreter used to
/// answer from a stub instead of the typed decode AOT calls.
#[test]
fn data_json_agrees_on_every_tier() {
    assert_example_tier_parity(
        "data_json",
        "examples/features/tooling/data_json.jet",
        "examples/features/expected/tooling/data_json.out",
    );
}
