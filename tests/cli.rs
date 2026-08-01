//! E2-M3 Wave A — developer command UX golden tests.
//!
//! Pins:
//!   - the stable exit-code table (0/1/2/70/101);
//!   - human *and* `--json` diagnostic output for check/build/test;
//!   - CI determinism: output is byte-identical and ANSI-free under `NO_COLOR`
//!     and when piped (not a TTY);
//!   - `jet explain <CODE>` resolves for EVERY registered diagnostic code
//!     (closing the I4 loop: no code without an explain).
//!
//! Snapshots live in `tests/cli/*.txt`; bless with `UPDATE_EXPECT=1`.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

/// D-LENS-RUN1: native execution proof for programs default `jet run` cannot JIT.
fn jet_run_release(file: &str) -> Command {
    let mut cmd = Command::new(jet());
    cmd.args(["run", "--release", file]);
    cmd
}

fn output_with_retry(cmd: &mut Command) -> Output {
    let mut last = None;
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(20 * attempt));
        }
        match cmd.output() {
            Ok(out) => return out,
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy => last = Some(e),
            Err(e) => panic!("CLI command failed: {e}"),
        }
    }
    panic!("CLI command stayed busy: {}", last.unwrap());
}

fn spawn_with_retry(cmd: &mut Command) -> Child {
    let mut last = None;
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(20 * attempt));
        }
        match cmd.spawn() {
            Ok(child) => return child,
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy => last = Some(e),
            Err(e) => panic!("CLI command failed: {e}"),
        }
    }
    panic!("CLI command stayed busy: {}", last.unwrap());
}

#[test]
fn inspect_unsafe_reports_policy_provenance_and_operations() {
    let dir = isolated_cwd("inspect_unsafe");
    fs::write(dir.join("main.jet"), "use core.mem\nfn run() {\n value :: 7\n #Unsafe(\"local\", obligations: .Track) {\n  pointer :: *Int.{ *value }\n  assert no_alias\n  band :: pointer.*..8\n  assert valid_ptr, aligned\n  print(band.start)\n }\n}\n").unwrap();
    let output = Command::new(jet()).args(["inspect", "unsafe", "main.jet", "--json"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"schema_version\":1") && stdout.contains("\"mode\":\"Obligations\"") && stdout.contains("\"kind\":\"raw_pointer\"") && stdout.contains("\"kind\":\"dereference\"") && stdout.contains("\"discharged\":true"), "{stdout}");
}

#[test]
fn configured_organization_unsafe_policy_fails_closed_and_keeps_path() {
    let dir = isolated_cwd("organization_unsafe");
    fs::write(dir.join("main.jet"), "fn run() {}\n").unwrap();
    let configured = dir.join("org-policy.jet");
    let output = Command::new(jet()).args(["check", "main.jet"]).current_dir(&dir).env(jet::Syntax::ENV_ORG_UNSAFE_POLICY, &configured).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("E3109") && stderr.contains(configured.to_str().unwrap()), "{stderr}");
    check_snapshot("unsafe_org_policy.txt", &stderr.replace(configured.to_str().unwrap(), "ORG_POLICY"));
}

#[test]
fn lua_bind_runs_embedded_vm_and_recovers_after_hostile_calls() {
    if Command::new("lua").arg("-v").output().is_err() { return }
    let dir=isolated_cwd("lua_bind_e2e");let root=PathBuf::from(env!("CARGO_MANIFEST_DIR"));let example=root.join("examples/interop/lua");
    fs::copy(example.join("ops.lua"),dir.join("ops.lua")).unwrap();fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","lua","ops.lua","--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"Lua bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    let cache=dir.join(".jet/bindings/lua");assert!(cache.join("libjet_lua_ops.a").is_file());let provenance=fs::read_to_string(cache.join("ops.provenance")).unwrap();assert!(provenance.contains("state=per-session\ntransport=datatree+table-view\ntable-view=zero-copy\nhook=instructions\n"));
    let generated=fs::read_to_string(cache.join("ops.jet")).unwrap();assert!(generated.contains("pub struct TableView")&&generated.contains("pub fn counters_view(session: Session, deadline_ms: Int) => TableView ? LuaError")&&generated.contains("pub fn view_get_int(view: TableView, key: String) => Int ? LuaError")&&generated.contains("pub fn view_set_int(view: TableView, key: String, value: Int) => Bool ? LuaError"));
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"embedded Lua binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::copy(root.join("tests/fixtures/lua_lifecycle.c"),dir.join("lifecycle.c")).unwrap();let lua_dir=fs::read_to_string(cache.join("ops.lua-path")).unwrap();let lua_dir=lua_dir.trim();let link_dir=format!("-L{lua_dir}");let rpath=format!("-Wl,-rpath,{lua_dir}");
    let cc=Command::new("cc").arg("lifecycle.c").args(["-L.jet/bindings/lua","-l:libjet_lua_ops.a"]).arg(link_dir).arg(rpath).args(["-llua","-lpthread","-ldl","-lm","-o","lifecycle"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"Lua lifecycle probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let lifecycle=Command::new(dir.join("lifecycle")).current_dir(&dir).output().unwrap();assert!(lifecycle.status.success(),"Lua lifecycle probe failed: {:?}",lifecycle.status.code());
}

#[test]
fn lua_bind_discovers_without_executing_and_launders_parse_errors() {
    if Command::new("luac").arg("-v").output().is_err() { return }
    let dir=isolated_cwd("lua_bind_static");let script=dir.join("static.lua");fs::write(&script,"error('discovery executed source')\n-- function fake(input) end\nlocal function hidden(input) return input end\nfunction visible(input) return input end\n").unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","lua"]).arg(&script).args(["--pkg","static_ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"static Lua discovery failed:\n{}",String::from_utf8_lossy(&bind.stderr));let generated=fs::read_to_string(dir.join(".jet/bindings/lua/static_ops.jet")).unwrap();assert!(generated.contains("pub fn visible("));assert!(!generated.contains("pub fn fake(")&&!generated.contains("pub fn hidden("));
    let invalid=dir.join("invalid.lua");fs::write(&invalid,"function broken(input)\n  return {\nend\n").unwrap();let failed=Command::new(jet()).args(["inspect","bind","lua"]).arg(&invalid).args(["--pkg","bad"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert_eq!(failed.status.code(),Some(1));let stderr=String::from_utf8_lossy(&failed.stderr);assert!(stderr.contains("Error [E3208]"));assert!(!stderr.contains("invalid.lua:")&&!stderr.contains("near 'end'"),"raw Lua parser detail escaped: {stderr}");
}

#[test]
fn lua_bind_rejects_generated_fixed_abi_names() {
    if Command::new("luac").arg("-v").output().is_err() { return }
    let dir=isolated_cwd("lua_bind_reserved_helpers");for name in ["take_error","view_release"]{let script=dir.join(format!("{name}.lua"));fs::write(&script,format!("function {name}(input) return input end\n")).unwrap();let package=format!("reserved_{name}");let failed=Command::new(jet()).args(["inspect","bind","lua"]).arg(&script).args(["--pkg",&package]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert_eq!(failed.status.code(),Some(1));let stderr=String::from_utf8_lossy(&failed.stderr);assert!(stderr.contains("Error [E3208]")&&stderr.contains(&format!("`{name}` cannot be exported")),"{stderr}");assert!(!stderr.contains("E0105"),"duplicate generated extern escaped binder validation: {stderr}");assert!(!dir.join(format!(".jet/bindings/lua/{package}.jet")).exists());}
}

fn cli_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli")
}

/// Compare `actual` against `tests/cli/<name>`; bless on `UPDATE_EXPECT=1`.
fn check_snapshot(name: &str, actual: &str) {
    let path = cli_dir().join(name);
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(cli_dir()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run UPDATE_EXPECT=1 cargo test",
            path.display()
        )
    });
    assert_eq!(actual, expected, "snapshot mismatch for {}", name);
}

/// Write a tiny source file with a known error and return its path. Each test
/// passes a unique `tag` so concurrent tests never share a path — `fs::write`
/// truncates-then-writes, so a shared path would let one test's write race a
/// sibling's `jet check` read (seeing a momentarily-empty file).
fn bad_file(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_bad_{tag}.jet"));
    fs::write(&p, "fn run() {\n    pirnt(\"hi\");\n}\n").unwrap();
    p
}

/// Replace machine-specific temp paths so snapshots are portable.
fn scrub(s: &str, file: &Path) -> String {
    s.replace(&file.display().to_string(), "BAD.jet")
}

/// A private cwd for a `jet run`/`build`/`bench`/`test` subprocess.
///
/// `jet` writes compiled output to `build/<stem>.rs` + `build/<stem>` *relative
/// to its own cwd* (Source/CmdCompile.rs `bin_path`/`stem`/`build`), keyed only
/// by the source file's stem — not its full path. Two concurrent `jet`
/// processes compiling different files that happen to share a stem (e.g. two
/// `main.jet` fixtures) race on that shared `build/` path if both inherit the
/// test harness's cwd (the repo root). Giving each such test its own cwd
/// removes the shared namespace entirely, regardless of stem.
fn isolated_cwd(tag: &str) -> PathBuf {
    static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    loop {
        let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jet_cli_cwd_{tag}_{}_{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot create isolated CLI fixture {}: {error}", dir.display()),
        }
    }
}

#[test]
fn tasks_lists_documented_scheduled_project_tasks_and_matches_run_outside_projects() {
    let project = isolated_cwd("tasks_project");
    fs::write(
        project.join("pkg.jet"),
        "payload: { name: \"task_runner\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(
        project.join("run.jet"),
        include_str!("../examples/features/devloop/task_runner.jet"),
    )
    .unwrap();

    let listed = Command::new(jet())
        .arg("tasks")
        .current_dir(&project)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        listed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert_eq!(
        String::from_utf8(listed.stdout).unwrap(),
        "greet  Say hello from a project task\nseed   Seed local data (every 5min)\n"
    );

    let unknown = Command::new(jet())
        .args(["run", "--task", "missing", "run.jet"])
        .current_dir(&project)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let unknown_stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(!unknown.status.success(), "{unknown_stderr}");
    assert!(unknown_stderr.contains("E1294"), "{unknown_stderr}");
    assert!(
        unknown_stderr.contains("declared tasks: greet, seed"),
        "{unknown_stderr}"
    );

    let help = Command::new(jet()).arg("help").output().unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("jet tasks"),
        "jet help must list task discovery"
    );

    let outside = isolated_cwd("tasks_outside_project");
    let tasks_error = Command::new(jet())
        .arg("tasks")
        .current_dir(&outside)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let run_error = Command::new(jet())
        .arg("run")
        .current_dir(&outside)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(tasks_error.status.code(), run_error.status.code());
    assert_eq!(tasks_error.stderr, run_error.stderr);

    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/monorepo");
    let workspace = isolated_cwd("tasks_workspace");
    fs::remove_dir_all(&workspace).unwrap();
    copy_dir_all(&fixture, &workspace);
    let hello = workspace.join("packages/hello/hello.jet");
    let mut hello_source = fs::read_to_string(&hello).unwrap();
    hello_source.push_str(
        "\n#[Job, Doc(\"Say hello from this workspace member\")] fn greet() {}\n",
    );
    fs::write(&hello, hello_source).unwrap();

    let ambiguous = Command::new(jet())
        .arg("tasks")
        .current_dir(&workspace)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let ambiguous_stderr = String::from_utf8_lossy(&ambiguous.stderr);
    assert_eq!(ambiguous.status.code(), Some(2), "{ambiguous_stderr}");
    assert!(
        ambiguous_stderr.contains("`jet tasks` is ambiguous"),
        "{ambiguous_stderr}"
    );
    assert!(
        ambiguous_stderr.contains("hello") && ambiguous_stderr.contains("ranker"),
        "{ambiguous_stderr}"
    );
    assert!(
        ambiguous_stderr.contains("jet tasks -p <member>")
            && !ambiguous_stderr.contains("jet run"),
        "{ambiguous_stderr}"
    );

    let selected = Command::new(jet())
        .args(["tasks", "-p", "hello"])
        .current_dir(&workspace)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        selected.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    assert_eq!(
        String::from_utf8(selected.stdout).unwrap(),
        "greet  Say hello from this workspace member\n"
    );
}

#[test]
fn project_parts_lists_skipped_explicit_and_conflicting_modules() {
    let dir = isolated_cwd("project_parts");
    fs::write(dir.join("main.jet"), "module app { }\nfn run() {}\n").unwrap();
    fs::write(dir.join("bench.jet"), "module _bench { }\n").unwrap();

    let skipped = Command::new(jet())
        .args(["project", "parts", "--skipped"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(skipped.status.code(), Some(0));
    let stdout = String::from_utf8(skipped.stdout).unwrap();
    assert!(stdout.contains("skipped") && stdout.contains("_bench"), "{stdout}");
    assert!(!stdout.contains("app"), "{stdout}");

    fs::write(
        dir.join("main.jet"),
        "use project._bench;\nmodule app { }\nfn run() {}\n",
    )
    .unwrap();
    let explicit = Command::new(jet())
        .args(["project", "parts", "--json"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(explicit.status.code(), Some(0));
    let stdout = String::from_utf8(explicit.stdout).unwrap();
    assert!(
        stdout.contains("\"name\":\"_bench\"")
            && stdout.contains("\"state\":\"explicit\""),
        "{stdout}"
    );

    fs::write(dir.join("other.jet"), "module _bench { }\n").unwrap();
    let conflict = Command::new(jet())
        .args(["project", "parts"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&conflict.stderr);
    assert!(stderr.contains("Error [E0606]:"), "{stderr}");
    assert!(stderr.contains(" Why:"), "{stderr}");
    assert!(stderr.contains(" Fix:"), "{stderr}");
}

#[test]
#[cfg(target_os = "linux")]
fn isolated_cwd_child_holds_executable() {
    let Some(ready) = std::env::var_os("JET_CLI_EXECUTABLE_HOLDER_READY") else {
        return;
    };
    fs::write(ready, "ready").unwrap();
    let mut release = [0];
    std::io::stdin().read_exact(&mut release).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn isolated_cwd_never_reuses_executing_fixture_path() {
    let first = isolated_cwd("executing_fixture_collision");
    let executable = first.join("cli-test-holder");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let ready = first.join("ready");
    let mut child = Command::new(&executable)
        .args(["--exact", "isolated_cwd_child_holds_executable"])
        .env("JET_CLI_EXECUTABLE_HOLDER_READY", &ready)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let ready_seen = loop {
        if ready.is_file() {
            break true;
        }
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };

    let second = ready_seen.then(|| isolated_cwd("executing_fixture_collision"));
    let copy_result = second
        .as_ref()
        .map(|dir| fs::copy(std::env::current_exe().unwrap(), dir.join("cli-test-holder")));
    let release_result = child.stdin.as_mut().unwrap().write_all(&[1]);
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();

    let _ = fs::remove_dir_all(&first);
    if let Some(second) = &second {
        let _ = fs::remove_dir_all(second);
    }

    assert!(ready_seen, "holder exited or timed out: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "holder failed: {}", String::from_utf8_lossy(&output.stderr));
    release_result.unwrap();
    let second = second.unwrap();
    assert_ne!(first, second, "fixture path reused while stale executable was running");
    copy_result.unwrap().unwrap();
}

fn budget_project(tag: &str, limit: u64) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    ).unwrap();
    fs::write(
        dir.join("src/main.jet"),
        format!(r#"module perf.package {{
    budgets: [Budget.{{
        name: "public-api",
        scope: .Package,
        metric: .PublicApiItems,
        comparison: .Absolute,
        limit: .AtMost({limit}),
    }}],
}}
pub fn api() {{}}
fn run() {{}}
"#),
    ).unwrap();
    dir
}

fn artifact_budget_project(tag: &str, limit: u64) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    ).unwrap();
    fs::write(
        dir.join("src/main.jet"),
        format!(r#"module perf.package {{
    budgets: [Budget.{{
        name: "binary",
        scope: .Package,
        metric: .BinarySize,
        comparison: .Absolute,
        limit: .AtMost({limit}B),
    }}],
}}
fn run() {{
    print("tiny")
}}
"#),
    ).unwrap();
    dir
}

fn mixed_budget_project(tag: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    ).unwrap();
    fs::write(
        dir.join("src/main.jet"),
        r#"module perf.package {
    budgets: [
        Budget.{
            name: "binary",
            scope: .Package,
            metric: .BinarySize,
            comparison: .Absolute,
            limit: .AtMost(100000000B),
        },
        Budget.{
            name: "public-api",
            scope: .Package,
            metric: .PublicApiItems,
            comparison: .Absolute,
            limit: .AtMost(10),
        },
    ],
}

pub fn api() {}
fn run() {}
"#,
    ).unwrap();
    dir
}

fn benchmark_budget_project(tag: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    fs::write(dir.join("src/main.jet"), r#"module perf.package {
    budgets: [Budget.{
        name: "parse",
        scope: .Bench("parse"),
        metric: .BenchTime(.P50),
        provider: .BenchMeasurement("parse"),
        comparison: .RelativeTo("ci/linux"),
        limit: .RegressionAtMost(100pct),
        enforcement: .Warn,
    }],
}
#Bench("parse") {
    total := 0
    loop value, 0..100 { total = total + value }
    require_eq(total, 4950)
}
fn run() {}
"#).unwrap();
    dir
}

fn allocation_budget_project(tag: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    fs::write(dir.join("src/main.jet"), r#"use core.mem
module perf.package {
    budgets: [
        Budget.{
            name: "arena-count",
            scope: .Bench("arena"),
            metric: .AllocationCount,
            provider: .AllocationProbe("arena"),
            comparison: .AbsoluteFrom("local/arena"),
            limit: .AtMost(2),
            enforcement: .Warn,
        },
        Budget.{
            name: "arena-bytes",
            scope: .Bench("arena"),
            metric: .AllocationBytes,
            provider: .AllocationProbe("arena"),
            comparison: .AbsoluteFrom("local/arena"),
            limit: .AtMost(16B),
            enforcement: .Warn,
        },
    ],
}
#Bench("arena") {
    arena :: mem.Arena.new()
    value :: arena.alloc(42)
    require_eq(value, 42)
}
fn run() {}
"#).unwrap();
    dir
}

#[test]
fn budget_usage_and_preflight_fail_without_artifacts() {
    let dir = budget_project("budget_no_artifact", 10);
    for argv in [
        vec!["budget", "check", "--unknown"],
        vec!["budget", "update", "--baseline", "ci/linux", "--reason", "no gate"],
        vec!["budget", "report"],
        vec!["budget", "check", "--json", "--unknown"],
        vec!["budget", "check", "--unknown", "--json"],
        vec!["budget", "check", "--json", "--json"],
        vec!["budget", "check", "--annotations", "gitlab"],
        vec!["budget", "update", "--baseline", "CI/Linux"],
        vec!["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--accept-regression", "--reason", "invalid"],
        vec!["budget", "update", "--baseline", "ci/linux", "--yes", "-y"],
    ] {
        let out = Command::new(jet()).args(argv).current_dir(&dir).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(!dir.join(".jet").exists(), "usage failure created an artifact");
    }
    fs::write(dir.join("src/main.jet"), "fn run( {\n").unwrap();
    let out = Command::new(jet()).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.join(".jet").exists(), "compiler preflight created an artifact");
}

#[test]
fn budget_check_uses_real_compiler_fact_and_writes_verified_report() {
    let dir = budget_project("budget_check", 10);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let value = jet_foundation::PerformanceBudget::CanonicalJson::parse_canonical(&out.stdout).unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"schema\":\"jet.budget-command\""));
    assert!(text.contains("\"budget_id\":\"package:public-api\""));
    assert!(text.contains("\"num\":1"), "public API count must be measured: {text}");
    let reports = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(reports.len(), 1);
    let bytes = fs::read(reports[0].path()).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let command = match &value { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("command JSON is not an object") };
    let report = match &command["report"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("report is not an object") };
    let content = match &report["content"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("content is not an object") };
    let tool = match &content["toolchain"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("toolchain is not an object") };
    for key in ["compiler_build_id", "stdlib_id", "runner_id"] {
        let jet_foundation::PerformanceBudget::CanonicalJson::String(id) = &tool[key] else { panic!("{key} is not text") };
        assert_eq!(id.len(), 64, "{key} must identify real executable bytes");
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!matches!(id.as_str(), "jet" | "stdlib" | "compiler"));
    }
    let subject = match &content["subject"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("subject is not an object") };
    let jet_foundation::PerformanceBudget::CanonicalJson::String(triple) = &subject["target_triple"] else { panic!("target triple is not text") };
    assert!(triple.split('-').count() >= 3, "target triple must be canonical: {triple}");
    let jet_foundation::PerformanceBudget::CanonicalJson::String(measured_start) = &subject["measured_start"] else { panic!("measurement start is not text") };
    let jet_foundation::PerformanceBudget::CanonicalJson::String(measured_end) = &subject["measured_end"] else { panic!("measurement end is not text") };
    assert!(measured_start < measured_end, "measurement must cover preflight and evidence: {measured_start}..{measured_end}");
    let measurements = match &content["measurements"] { jet_foundation::PerformanceBudget::CanonicalJson::Array(value) => value, _ => panic!("measurements is not an array") };
    let measurement = match &measurements[0] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("measurement is not an object") };
    let provider = match &measurement["provider"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("provider is not an object") };
    for key in ["cpu_model", "kernel", "power_governor"] {
        let jet_foundation::PerformanceBudget::CanonicalJson::String(value) = &provider[key] else { panic!("{key} is not text") };
        assert!(!value.is_empty() && !matches!(value.as_str(), "compiler" | "unknown"));
    }
}

#[test]
fn budget_build_artifact_measures_real_selected_binary() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = artifact_budget_project("budget_build_artifact", 100_000_000);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject object") };
    let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("artifact identity") };
    let CanonicalJson::Integer(bytes) = &artifact["bytes"] else { panic!("artifact byte count") };
    let CanonicalJson::String(digest) = &artifact["sha256"] else { panic!("artifact digest") };
    let artifact_path = dir.join("build/main");
    let metadata = fs::metadata(&artifact_path).unwrap();
    assert_eq!(bytes, &metadata.len().to_string());
    assert_eq!(digest, &jet::SHA256::sha256_file_hex(&artifact_path).unwrap());
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    let CanonicalJson::Object(measurement) = &measurements[0] else { panic!("measurement") };
    let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
    let CanonicalJson::Object(sample) = &samples[0] else { panic!("sample") };
    assert_eq!(sample["num"], CanonicalJson::Integer(metadata.len().to_string()));
    let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
    assert_eq!(provider["kind"], CanonicalJson::String("BuildArtifact".into()));
    assert_eq!(measurement["unit"], CanonicalJson::String("Bytes".into()));
}

#[test]
fn budget_report_collects_mixed_providers_measurement_locally() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = mixed_budget_project("budget_mixed_providers");
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 2);
    let mut providers = std::collections::BTreeMap::new();
    for measurement in measurements {
        let CanonicalJson::Object(measurement) = measurement else { panic!("measurement object") };
        let CanonicalJson::String(id) = &measurement["budget_id"] else { panic!("budget id") };
        let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
        let CanonicalJson::String(kind) = &provider["kind"] else { panic!("provider kind") };
        let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
        assert_eq!(samples.len(), 1, "{id} must own its provider sample");
        providers.insert(id.clone(), kind.clone());
    }
    assert_eq!(providers.get("package:binary").map(String::as_str), Some("BuildArtifact"));
    assert_eq!(providers.get("package:public-api").map(String::as_str), Some("CompilerFacts"));
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
    let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("shared artifact provenance") };
    let CanonicalJson::Integer(bytes) = &artifact["bytes"] else { panic!("artifact bytes") };
    assert_eq!(bytes, &fs::metadata(dir.join("build/main")).unwrap().len().to_string());
    let report_path = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().next().unwrap().unwrap().path();
    jet_foundation::PerformanceBudget::verify_budget_report(&fs::read(report_path).unwrap()).unwrap();
}

#[test]
fn build_enforces_deterministic_fail_budgets_and_reuses_relevant_identity() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = mixed_budget_project("build_budget_gates");
    let source_path = dir.join("src/main.jet");
    let passing = fs::read_to_string(&source_path).unwrap();
    let failing = passing.replace(".AtMost(10)", ".AtMost(0)");
    fs::write(&source_path, &failing).unwrap();

    let failed = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(failed.status.code(), Some(1), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&failed.stdout), String::from_utf8_lossy(&failed.stderr));
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("built:"), "failed budget claimed build success");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Error [E2907]: performance budget public-api regressed"), "{}", String::from_utf8_lossy(&failed.stderr));
    let report_dir = dir.join(".jet/perf/reports");
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 1);

    fs::write(&source_path, &passing).unwrap();
    let passed = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(passed.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&passed.stdout), String::from_utf8_lossy(&passed.stderr));
    assert!(String::from_utf8_lossy(&passed.stderr).contains("budgets: 2 budgets passed · report "));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 2, "source/spec change must refresh evidence");

    let reused = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(reused.status.code(), Some(0), "{}", String::from_utf8_lossy(&reused.stderr));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 2, "unchanged relevant identity must reuse canonical report");

    let ci = Command::new(jet()).args(["build", "src/main.jet", "--profile=ci"]).current_dir(&dir).output().unwrap();
    assert_eq!(ci.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&ci.stdout), String::from_utf8_lossy(&ci.stderr));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 3, "CI profile identity must refresh evidence");
    let mut profiles = Vec::new();
    for entry in fs::read_dir(&report_dir).unwrap() {
        let value = CanonicalJson::parse_canonical(&fs::read(entry.unwrap().path()).unwrap()).unwrap();
        let CanonicalJson::Object(report) = value else { panic!("report") };
        let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
        let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
        let CanonicalJson::String(profile) = &subject["profile"] else { panic!("profile") };
        profiles.push(profile.clone());
    }
    assert!(profiles.iter().any(|profile| profile == "dev"));
    assert!(profiles.iter().any(|profile| profile == "ci"));
}

#[test]
#[cfg(target_os = "linux")]
fn perf_report_reuse_ignores_nonsemantic_compiler_bytes_under_parallel_load() {
    let bin_dir = isolated_cwd("perf_report_compiler_identity");
    let compiler = bin_dir.join("jet-semantic-a");
    let padded_compiler = bin_dir.join("jet-semantic-b");
    fs::copy(jet(), &compiler).unwrap();
    fs::copy(jet(), &padded_compiler).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&padded_compiler)
        .unwrap()
        .write_all(b"nonsemantic linker padding")
        .unwrap();
    assert_ne!(
        jet::SHA256::sha256_file_hex(&compiler).unwrap(),
        jet::SHA256::sha256_file_hex(&padded_compiler).unwrap(),
        "controlled compiler copies must have different file bytes",
    );

    let workspace = benchmark_budget_project("perf_report_compiler_identity");
    let seeded = Command::new(&compiler)
        .args(["bench", "src/main.jet"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert_eq!(
        seeded.status.code(), Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&seeded.stdout),
        String::from_utf8_lossy(&seeded.stderr),
    );
    assert_eq!(fs::read_dir(workspace.join(".jet/perf/reports")).unwrap().count(), 1);

    let start = std::sync::Barrier::new(3);
    let outputs = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            Command::new(&compiler)
                .args(["bench", "src/main.jet"])
                .current_dir(&workspace)
                .output()
                .unwrap()
        });
        let second = scope.spawn(|| {
            start.wait();
            Command::new(&padded_compiler)
                .args(["bench", "src/main.jet"])
                .current_dir(&workspace)
                .output()
                .unwrap()
        });
        start.wait();
        [first.join().unwrap(), second.join().unwrap()]
    });

    for output in outputs {
        assert_eq!(
            output.status.code(), Some(0),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("ns/iter"), "compatible report reran benchmark workload");
    }
    assert_eq!(
        fs::read_dir(workspace.join(".jet/perf/reports")).unwrap().count(),
        1,
        "nonsemantic compiler bytes must not invalidate compatible report identity",
    );
}

#[test]
fn budget_bench_measurement_bootstraps_then_consumes_compatible_history() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("budget_bench_measurement");
    let bootstrap = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","initial benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(bootstrap.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&bootstrap.stdout),String::from_utf8_lossy(&bootstrap.stderr));
    let CanonicalJson::Object(first)=CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap() else{panic!("command")};
    let CanonicalJson::Object(report)=&first["report"] else{panic!("report")};let CanonicalJson::Object(content)=&report["content"] else{panic!("content")};let CanonicalJson::Array(measurements)=&content["measurements"] else{panic!("measurements")};let CanonicalJson::Object(measurement)=&measurements[0] else{panic!("measurement")};
    let CanonicalJson::Array(samples)=&measurement["samples"] else{panic!("samples")};assert_eq!(samples.len(),20);assert!(matches!(measurement["statistics"],CanonicalJson::Object(_)));assert!(matches!(measurement["policy"],CanonicalJson::Object(_)));assert_eq!(measurement["history"],CanonicalJson::Null);assert_eq!(measurement["baseline"],CanonicalJson::Null);
    let CanonicalJson::Object(provider)=&measurement["provider"] else{panic!("provider")};assert_eq!(provider["kind"],CanonicalJson::String("BenchMeasurement".into()));assert_eq!(provider["identity"],CanonicalJson::String("parse".into()));
    let first_id=match &report["report_id"]{CanonicalJson::String(value)=>value.clone(),_=>panic!("report id")};

    let check=Command::new(jet()).args(["budget","check","--json"]).current_dir(&dir).output().unwrap();
    assert!(matches!(check.status.code(),Some(0)|Some(1)),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&check.stdout),String::from_utf8_lossy(&check.stderr));
    let CanonicalJson::Object(second)=CanonicalJson::parse_canonical(&check.stdout).unwrap() else{panic!("command")};
    let CanonicalJson::Object(report)=&second["report"] else{panic!("report")};let CanonicalJson::Object(content)=&report["content"] else{panic!("content")};let CanonicalJson::Array(measurements)=&content["measurements"] else{panic!("measurements")};let CanonicalJson::Object(measurement)=&measurements[0] else{panic!("measurement")};let CanonicalJson::Object(history)=&measurement["history"] else{panic!("history")};let CanonicalJson::Array(ids)=&history["report_ids"] else{panic!("ids")};assert_eq!(ids, &vec![CanonicalJson::String(first_id.clone())]);let CanonicalJson::Object(baseline)=&measurement["baseline"] else{panic!("baseline")};let CanonicalJson::Array(pooled)=&baseline["pooled_samples"] else{panic!("pooled")};assert_eq!(pooled.len(),20);let CanonicalJson::Object(decision)=&measurement["decision"] else{panic!("decision")};assert_ne!(decision["evidence"],CanonicalJson::String("unavailable".into()));
    let CanonicalJson::Array(results)=&second["results"] else{panic!("results")};let CanonicalJson::Object(result)=&results[0] else{panic!("result")};
    assert_eq!(result["baseline_report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id)]));
    assert_eq!(result["metric"],measurement["metric"]);
    assert_eq!(result["lower95"],decision["lower95"]);assert_eq!(result["upper95"],decision["upper95"]);assert_eq!(result["trend"],decision["trend"]);assert_eq!(result["reason"],decision["reason"]);
}

#[test]
fn bench_owns_canonical_refresh_and_dossier_only_projects_it() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("bench_owned_budget_refresh");
    let run = || Command::new(jet()).args(["bench", "src/main.jet"]).current_dir(&dir).output().unwrap();
    let first = run();
    assert_eq!(first.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    assert!(String::from_utf8_lossy(&first.stderr).contains("report "));
    let reports = dir.join(".jet/perf/reports");
    let report_paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = report_paths();
    assert_eq!(initial.len(), 1);
    let bytes = fs::read(&initial[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
    assert_eq!(subject["profile"], CanonicalJson::String("bench".into()));

    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&second.stdout).contains("ns/iter"), "unchanged relevant identity reran measurement harness");
    assert_eq!(report_paths(), initial, "unchanged relevant identity must reuse report");

    let before = fs::read_dir(&reports).unwrap().map(|entry| { let path=entry.unwrap().path();(path.clone(),fs::metadata(&path).unwrap().modified().unwrap()) }).collect::<Vec<_>>();
    let dossier = Command::new(jet()).args(["inspect", "dossier", "src/main.jet", "run", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(dossier.status.code(), Some(0), "{}", String::from_utf8_lossy(&dossier.stderr));
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(dossier.contains("\"performance_budgets\":{\"mode\":\"read_only\""), "{dossier}");
    assert!(dossier.contains("\"budget_id\":\"package:parse\""), "{dossier}");
    let after = fs::read_dir(&reports).unwrap().map(|entry| { let path=entry.unwrap().path();(path.clone(),fs::metadata(&path).unwrap().modified().unwrap()) }).collect::<Vec<_>>();
    assert_eq!(before, after, "dossier projection must not rewrite reports");

    fs::OpenOptions::new().append(true).open(dir.join("src/main.jet")).unwrap().write_all(b"\n// relevant source digest change\n").unwrap();
    let third = run();
    assert_eq!(third.status.code(), Some(0), "{}", String::from_utf8_lossy(&third.stderr));
    assert_eq!(report_paths().len(), 2, "source digest change must refresh canonical report");
}

#[test]
fn allocation_probe_uses_real_bench_boundaries_and_rejects_forged_cache() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = allocation_budget_project("allocation_probe_runtime");
    let run = || Command::new(jet()).args(["bench", "src/main.jet"]).current_dir(&dir).output().unwrap();

    let first = run();
    assert_eq!(first.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    let reports = dir.join(".jet/perf/reports");
    let paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = paths();
    assert_eq!(initial.len(), 1);
    let bytes = fs::read(&initial[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 2);
    for measurement in measurements {
        let CanonicalJson::Object(measurement) = measurement else { panic!("measurement") };
        let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
        assert_eq!(provider["kind"], CanonicalJson::String("AllocationProbe".into()));
        assert_eq!(provider["identity"], CanonicalJson::String("arena".into()));
        assert_eq!(provider["isolation"], CanonicalJson::String("benchmark-process-counter-reset-per-trial".into()));
        assert_eq!(provider["version"], CanonicalJson::String("jet-arena-events-v1-warmup-auto-trials-20".into()));
        let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
        assert_eq!(samples.len(), 20);
        assert!(samples.windows(2).all(|pair| pair[0] == pair[1]), "reset boundary leaked calibration or a prior trial");
        let CanonicalJson::Object(metric) = &measurement["metric"] else { panic!("metric") };
        let expected = match &metric["name"] {
            CanonicalJson::String(name) if name == "AllocationCount" => "1",
            CanonicalJson::String(name) if name == "AllocationBytes" => "8",
            other => panic!("unexpected metric: {other:?}"),
        };
        for sample in samples {
            let CanonicalJson::Object(sample) = sample else { panic!("sample") };
            assert_eq!(sample["den"], CanonicalJson::Integer("1".into()));
            assert_eq!(sample["num"], CanonicalJson::Integer(expected.into()));
        }
    }

    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&second.stdout).contains("ns/iter"), "compatible report reran allocation workload");
    assert_eq!(paths(), initial);

    fs::OpenOptions::new().append(true).open(&initial[0]).unwrap().write_all(b"forged").unwrap();
    let third = run();
    assert_eq!(third.status.code(), Some(0), "{}", String::from_utf8_lossy(&third.stderr));
    assert_eq!(paths().len(), 2, "forged report must not satisfy compatible cache identity");
}

fn age_budget_baseline(dir: &Path, baseline: &str) -> (String, String) {
    use jet_foundation::PerformanceBudget::{stable_id, CanonicalJson};
    let path = dir.join(format!(".jet/perf/baselines/names/{baseline}.json"));
    let mut manifest = CanonicalJson::parse_canonical(&fs::read(&path).unwrap()).unwrap();
    let CanonicalJson::Object(wrapper) = &mut manifest else { panic!("manifest") };
    let report_id = {
        let CanonicalJson::Object(content) = &wrapper["content"] else { panic!("content") };
        let CanonicalJson::String(id) = &content["head_report_id"] else { panic!("head") };
        id.clone()
    };
    {
        let CanonicalJson::Object(content) = wrapper.get_mut("content").unwrap() else { panic!("content") };
        let CanonicalJson::Array(generations) = content.get_mut("generations").unwrap() else { panic!("generations") };
        let CanonicalJson::Object(generation) = &mut generations[0] else { panic!("generation") };
        let CanonicalJson::Object(audit) = generation.get_mut("audit").unwrap() else { panic!("audit") };
        audit.insert("accepted_at".into(), CanonicalJson::String("2000-01-01T00:00:00.000000000Z".into()));
        let mut body = audit.clone();
        body.remove("audit_id").unwrap();
        audit.insert("audit_id".into(), CanonicalJson::String(stable_id(&CanonicalJson::Object(body))));
    }
    let state_id = stable_id(&wrapper["content"]);
    wrapper.insert("manifest_id".into(), CanonicalJson::String(state_id.clone()));
    fs::write(path, manifest.bytes()).unwrap();
    (report_id, state_id)
}

#[test]
fn budget_stale_history_is_persisted_rendered_and_bootstrap_appends() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("budget_stale_history");
    let source = fs::read_to_string(dir.join("src/main.jet")).unwrap().replace("enforcement: .Warn", "enforcement: .Fail");
    fs::write(dir.join("src/main.jet"), source).unwrap();
    let first = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","initial benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(first.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&first.stdout),String::from_utf8_lossy(&first.stderr));
    let (first_id, stale_state_id) = age_budget_baseline(&dir, "ci/linux");

    let check = Command::new(jet()).args(["budget","check","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(check.status.code(),Some(1),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&check.stdout),String::from_utf8_lossy(&check.stderr));
    let CanonicalJson::Object(check) = CanonicalJson::parse_canonical(&check.stdout).unwrap() else { panic!("command") };
    assert_eq!(check["status"],CanonicalJson::String("stale".into()));
    assert_eq!(check["failure_kind"],CanonicalJson::String("evidence".into()));
    let CanonicalJson::Array(results)=&check["results"] else { panic!("results") };
    let CanonicalJson::Object(result)=&results[0] else { panic!("result") };
    assert_eq!(result["stale"],CanonicalJson::Bool(true));
    assert_eq!(result["status"],CanonicalJson::String("stale".into()));
    assert_eq!(result["evidence"],CanonicalJson::String("unavailable".into()));
    assert_eq!(result["baseline_report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));
    let CanonicalJson::String(reason)=&result["reason"] else { panic!("reason") };
    assert!(reason.contains("compatible history is stale"),"{reason}");
    assert!(reason.contains("policy limit is 2592000 seconds"),"{reason}");
    let CanonicalJson::Object(report)=&check["report"] else { panic!("report") };
    jet_foundation::PerformanceBudget::verify_budget_report(&CanonicalJson::Object(report.clone()).bytes()).unwrap();
    let CanonicalJson::Object(content)=&report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements)=&content["measurements"] else { panic!("measurements") };
    let CanonicalJson::Object(measurement)=&measurements[0] else { panic!("measurement") };
    let CanonicalJson::Object(history)=&measurement["history"] else { panic!("history") };
    assert_eq!(history["state_id"],CanonicalJson::String(stale_state_id.clone()));
    assert_eq!(history["report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));
    assert_eq!(measurement["baseline"],CanonicalJson::Null,"stale samples must not be pooled");
    let CanonicalJson::Object(decision)=&measurement["decision"] else { panic!("decision") };
    let CanonicalJson::Object(trend)=&decision["trend"] else { panic!("trend") };
    assert_eq!(trend["report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id.clone())]));

    let human = Command::new(jet()).args(["budget","check","--annotations","none"]).current_dir(&dir).output().unwrap();
    assert_eq!(human.status.code(),Some(1));
    let human = String::from_utf8(human.stderr).unwrap();
    assert!(human.contains("Error [E2906]: performance budget parse has no usable evidence"),"{human}");
    assert!(human.contains("compatible history is stale"),"{human}");
    assert!(human.contains("budgets stale: 1 baseline stale · report "),"{human}");

    let bootstrap = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","refresh stale benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(bootstrap.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&bootstrap.stdout),String::from_utf8_lossy(&bootstrap.stderr));
    let CanonicalJson::Object(bootstrap)=CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap() else { panic!("bootstrap") };
    assert_eq!(bootstrap["applied"],CanonicalJson::Bool(true));
    assert_eq!(bootstrap["status"],CanonicalJson::String("stale".into()));
    let CanonicalJson::Object(report)=&bootstrap["report"] else { panic!("report") };
    let CanonicalJson::String(second_id)=&report["report_id"] else { panic!("report id") };
    let manifest = CanonicalJson::parse_canonical(&fs::read(dir.join(".jet/perf/baselines/names/ci/linux.json")).unwrap()).unwrap();
    let CanonicalJson::Object(wrapper)=manifest else { panic!("manifest") };
    let CanonicalJson::Object(content)=&wrapper["content"] else { panic!("content") };
    let CanonicalJson::Array(generations)=&content["generations"] else { panic!("generations") };
    assert_eq!(generations.len(),2);
    let CanonicalJson::Object(second)=&generations[1] else { panic!("generation") };
    let CanonicalJson::Object(audit)=&second["audit"] else { panic!("audit") };
    assert_eq!(audit["kind"],CanonicalJson::String("bootstrap".into()));
    assert_eq!(audit["prior_state_id"],CanonicalJson::String(stale_state_id));
    assert_eq!(audit["prior_head_report_id"],CanonicalJson::String(first_id));
    assert_eq!(second["report_id"],CanonicalJson::String(second_id.clone()));
}

#[test]
fn budget_effect_count_uses_solved_effects_not_import_count() {
    let dir = budget_project("budget_effect_truth", 10);
    fs::write(dir.join("src/main.jet"), r#"use core.files as files
module perf.package {
    budgets: [Budget.{ name: "effects", scope: .Package, metric: .EffectCount, comparison: .Absolute, limit: .AtMost(0) }],
}
fn run() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(json.contains("\"budget_id\":\"package:effects\""));
    assert!(json.contains("\"point\":{\"den\":1,\"num\":0}"), "unused core import must not fabricate an effect: {json}");
}

#[test]
fn budget_generated_unsafe_rejects_proxy_before_artifact() {
    let dir = budget_project("budget_unsafe_truth", 10);
    fs::write(dir.join("src/main.jet"), r#"use core.mem as mem
module perf.package {
    budgets: [Budget.{ name: "unsafe", scope: .Package, metric: .GeneratedUnsafe, comparison: .Absolute, limit: .AtMost(0) }],
}
fn run() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("has no exact checked front-end fact; refusing proxy measurement"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "unsupported fact emitted an artifact");
}

#[test]
#[cfg(unix)]
fn budget_path_tools_cannot_forge_provenance() {
    use std::os::unix::fs::PermissionsExt;
    let dir = budget_project("budget_hostile_path", 10);
    let fake = dir.join("fake-bin");
    fs::create_dir(&fake).unwrap();
    for (name, body) in [
        ("rustc", "#!/bin/sh\necho 'host: fake-forged-triple'\n"),
        ("sha256sum", "#!/bin/sh\necho 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  fake'\n"),
        ("shasum", "#!/bin/sh\necho 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff  fake'\n"),
    ] {
        let path = fake.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = Command::new(jet()).args(["budget", "check", "--json"]).env("PATH", &fake).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let json = String::from_utf8(out.stdout).unwrap();
    assert!(!json.contains("fake-forged-triple"), "PATH rustc forged target identity");
    assert!(!json.contains("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"), "PATH digest tool forged compiler identity");
}

#[test]
#[cfg(unix)]
fn budget_unreadable_compiler_identity_rejects_before_artifact() {
    use std::os::unix::fs::PermissionsExt;
    let dir = artifact_budget_project("budget_missing_compiler_identity", 100_000_000);
    let copied = dir.join("jet-unreadable");
    fs::copy(jet(), &copied).unwrap();
    fs::set_permissions(&copied, fs::Permissions::from_mode(0o111)).unwrap();
    let out = output_with_retry(Command::new(&copied).args(["budget", "check"]).current_dir(&dir));
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("cannot hash running compiler executable"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "missing compiler identity emitted an artifact");
    assert!(!dir.join("build/main").exists(), "missing compiler identity started the selected artifact build");
}

#[test]
#[cfg(target_os = "linux")]
fn budget_parallel_child_builds_survive_running_compiler_unlink() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    use std::os::unix::fs::MetadataExt;
    let dirs = [
        artifact_budget_project("budget_unlinked_compiler_identity_a", 100_000_000),
        artifact_budget_project("budget_unlinked_compiler_identity_b", 100_000_000),
    ];
    let bin_dir = isolated_cwd("budget_unlinked_compiler_binary");
    let copied = bin_dir.join("jet-running-unlinked");
    let cache = bin_dir.join("cache");
    fs::copy(jet(), &copied).unwrap();
    // Pin the artifact before unlink: independent rustc links have different
    // build IDs, and this test owns compiler replacement rather than linking.
    let primed = output_with_retry(Command::new(&copied)
        .arg("build")
        .arg(dirs[0].join("src/main.jet"))
        .current_dir(&dirs[0])
        .env("JET_CACHE_DIR", &cache));
    assert_eq!(primed.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&primed.stdout), String::from_utf8_lossy(&primed.stderr));
    let seed_artifact = dirs[0].join("build/main");
    let expected_artifact = (
        CanonicalJson::String(jet::SHA256::sha256_file_hex(&seed_artifact).unwrap()),
        CanonicalJson::Integer(fs::metadata(seed_artifact).unwrap().len().to_string()),
    );
    let expected_compiler = env!("JET_COMPILER_BUILD_ID").to_string();
    let expected_stdlib = env!("JET_STDLIB_BUILD_ID").to_string();
    let expected_runner = env!("JET_RUNNER_BUILD_ID").to_string();
    let children = dirs.iter().map(|dir| {
        spawn_with_retry(Command::new(&copied)
            .args(["budget", "check", "--json"])
            .current_dir(dir)
            .env("JET_CACHE_DIR", &cache)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped()))
    }).collect::<Vec<_>>();
    let running_inode = fs::metadata(&copied).unwrap().ino();
    fs::remove_file(&copied).unwrap();
    fs::write(&copied, "replacement compiler inode\n").unwrap();
    assert_ne!(running_inode, fs::metadata(&copied).unwrap().ino());
    for (child, dir) in children.into_iter().zip(&dirs) {
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.stderr.is_empty());
        let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
        let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
        jet_foundation::PerformanceBudget::verify_budget_report(&CanonicalJson::Object(report.clone()).bytes()).unwrap();
        let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
        let CanonicalJson::Object(toolchain) = &content["toolchain"] else { panic!("toolchain object") };
        assert_eq!(toolchain["compiler_build_id"], CanonicalJson::String(expected_compiler.clone()));
        assert_eq!(toolchain["runner_id"], CanonicalJson::String(expected_runner.clone()));
        assert_eq!(toolchain["stdlib_id"], CanonicalJson::String(expected_stdlib.clone()));
        let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject object") };
        let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("artifact object") };
        let artifact_path = dir.join("build/main");
        assert_eq!(artifact["sha256"], CanonicalJson::String(jet::SHA256::sha256_file_hex(&artifact_path).unwrap()));
        assert_eq!(artifact["bytes"], CanonicalJson::Integer(fs::metadata(artifact_path).unwrap().len().to_string()));
        assert_eq!((&artifact["sha256"], &artifact["bytes"]), (&expected_artifact.0, &expected_artifact.1), "parallel builds produced different artifact identities");
    }
}

#[test]
fn budget_failure_has_human_github_projection_and_exit_one() {
    let dir = budget_project("budget_failure", 0);
    let out = Command::new(jet()).args(["budget", "check", "--annotations", "github"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Error [E2907]: performance budget public-api regressed"), "{stderr}");
    assert!(stderr.contains("::error file=src/main.jet"), "{stderr}");
    assert!(stderr.contains("performance budget public-api regressed%0AWhy: measured estimator"), "{stderr}");
    assert!(stderr.contains("%0AFix: improve the measured behavior, inspect `jet budget check --verbose`, or record an explicit exception"), "{stderr}");
    assert!(stderr.contains("budgets failed: 1 budget failed · report "), "{stderr}");
}

#[test]
fn budget_imported_declaration_reports_owning_module_location() {
    let dir = budget_project("budget_imported_source", 10);
    fs::write(dir.join("src/main.jet"), "module perf_defs;\nfn run() {}\n").unwrap();
    fs::write(dir.join("src/perf_defs.jet"), r#"module perf.package {
    budgets: [Budget.{ name: "imported-api", scope: .Package, metric: .PublicApiItems, comparison: .Absolute, limit: .AtMost(0) }],
}
pub fn imported() {}
"#).unwrap();
    let out = Command::new(jet()).args(["budget", "check", "--annotations", "github"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains(" --> src/perf_defs.jet:2:15"), "{stderr}");
    assert!(stderr.contains("::error file=src/perf_defs.jet,line=2,col=15,title=Jet E2907::"), "{stderr}");
    let report = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().next().unwrap().unwrap().path();
    let report = fs::read_to_string(report).unwrap();
    assert!(report.contains("\"source\":\"src/perf_defs.jet:2\""), "{report}");
}

#[test]
fn budget_update_is_plan_first_and_yes_applies_once() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = budget_project("budget_update", 10);
    let args = ["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence", "--json"];
    let plan = Command::new(jet()).args(args).current_dir(&dir).output().unwrap();
    assert_eq!(plan.status.code(), Some(0), "{}", String::from_utf8_lossy(&plan.stderr));
    let CanonicalJson::Object(plan)=jet_foundation::PerformanceBudget::CanonicalJson::parse_canonical(&plan.stdout).unwrap() else{panic!("command")};
    assert_eq!(plan["applied"],CanonicalJson::Bool(false));
    let CanonicalJson::Object(plan)=&plan["plan"] else{panic!("plan")};assert_eq!(plan["requires_confirmation"],CanonicalJson::Bool(false));let CanonicalJson::Array(rows)=&plan["rows"] else{panic!("rows")};assert_eq!(rows.len(),2);let CanonicalJson::Object(report)=&rows[0] else{panic!("report row")};let CanonicalJson::Object(baseline)=&rows[1] else{panic!("baseline row")};assert_eq!(report["operation"],CanonicalJson::String("create".into()));assert_eq!(report["artifact"],CanonicalJson::String("report".into()));assert_eq!(baseline["operation"],CanonicalJson::String("advance".into()));assert_eq!(baseline["artifact"],CanonicalJson::String("baseline".into()));
    assert!(!dir.join(".jet").exists(),"JSON plan-only mutated workspace");

    let applied = Command::new(jet()).args(args).arg("--yes").current_dir(&dir).output().unwrap();
    assert_eq!(applied.status.code(), Some(0), "{}", String::from_utf8_lossy(&applied.stderr));
    let applied = String::from_utf8(applied.stdout).unwrap();
    assert!(applied.contains("\"applied\":true"));
    assert!(dir.join(".jet/perf/baselines/names/ci/linux.json").is_file());
}

#[test]
fn budget_json_projection_is_exact_and_tool_failure_uses_null_report_fields() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = budget_project("budget_json_exact", 10);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    assert_eq!(command.keys().map(String::as_str).collect::<Vec<_>>(), ["applied","command","diagnostics","exit_code","failure_kind","plan","report","report_path","results","schema","status","version"]);
    let CanonicalJson::Array(results) = &command["results"] else { panic!("results") };
    let CanonicalJson::Object(result) = &results[0] else { panic!("result") };
    assert_eq!(result.keys().map(String::as_str).collect::<Vec<_>>(), ["baseline_report_ids","budget_id","comparison","diagnostic_code","direction","enforcement","evidence","lower95","metric","point","reason","source","stale","status","trend","unit","upper95"]);
    let CanonicalJson::Object(source) = &result["source"] else { panic!("source") };
    assert_eq!(source.keys().map(String::as_str).collect::<Vec<_>>(), ["column","line","path"]);
    let CanonicalJson::Object(comparison) = &result["comparison"] else { panic!("comparison") };
    assert_eq!(comparison.keys().map(String::as_str).collect::<Vec<_>>(), ["direction","kind","limit"]);

    fs::write(dir.join("src/main.jet"), "fn run( {\n").unwrap();
    let invalid = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stderr.is_empty());
    let CanonicalJson::Object(invalid) = CanonicalJson::parse_canonical(&invalid.stdout).unwrap() else { panic!("compiler failure object") };
    assert_eq!(invalid["failure_kind"], CanonicalJson::String("compiler".into()));
    let CanonicalJson::Array(diagnostics) = &invalid["diagnostics"] else { panic!("diagnostics") };
    let CanonicalJson::Object(diagnostic) = &diagnostics[0] else { panic!("diagnostic") };
    let CanonicalJson::Object(source) = &diagnostic["source"] else { panic!("diagnostic source") };
    assert_eq!(source.keys().map(String::as_str).collect::<Vec<_>>(), ["column","end_column","end_line","line","path"]);
    assert_eq!(source["path"], CanonicalJson::String("src/main.jet".into()));
    assert!(matches!(source["end_line"], CanonicalJson::Integer(_)));
    assert!(matches!(source["end_column"], CanonicalJson::Integer(_)));

    let empty = isolated_cwd("budget_json_tool_failure");
    let failed = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&empty).output().unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stderr.is_empty());
    let CanonicalJson::Object(failure) = CanonicalJson::parse_canonical(&failed.stdout).unwrap() else { panic!("failure object") };
    assert_eq!(failure["status"], CanonicalJson::String("fail".into()));
    assert_eq!(failure["failure_kind"], CanonicalJson::String("tool".into()));
    assert_eq!(failure["report"], CanonicalJson::Null);
    assert_eq!(failure["report_path"], CanonicalJson::Null);
    assert_eq!(failure["plan"], CanonicalJson::Null);
}

#[test]
fn budget_non_tty_plan_only_creates_no_artifact_or_baseline() {
    let dir = budget_project("budget_non_tty_cancel", 10);
    let out = Command::new(jet()).args(["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("plan only; pass -y or --yes to apply in a non-interactive shell"), "{stderr}");
    assert!(!dir.join(".jet").exists(), "plan-only update mutated workspace");
}

#[cfg(unix)]
fn run_budget_update_pty(dir: &Path, answer: &[u8]) -> (i32, String) {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::process::Stdio;
    unsafe extern "C" { fn openpty(master: *mut i32, slave: *mut i32, name: *mut i8, termp: *const u8, winp: *const u8) -> i32; }
    let (mut master_fd, mut slave_fd) = (-1, -1);
    assert_eq!(unsafe { openpty(&mut master_fd, &mut slave_fd, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) }, 0);
    let mut master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    let mut child = Command::new(jet())
        .args(["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--reason", "initial evidence"])
        .current_dir(dir)
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn().unwrap();
    master.write_all(answer).unwrap();
    let status = child.wait().unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop { match master.read(&mut buffer) { Ok(0) => break, Ok(n) => bytes.extend_from_slice(&buffer[..n]), Err(error) if error.raw_os_error() == Some(5) => break, Err(error) => panic!("PTY read: {error}") } }
    (status.code().unwrap(), String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

#[test]
#[cfg(unix)]
fn budget_tty_confirmation_cancel_and_yes_control_mutation() {
    let cancelled = budget_project("budget_tty_no", 10);
    let (code, transcript) = run_budget_update_pty(&cancelled, b"n\n");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.contains("Apply? [y/N]"), "{transcript}");
    assert!(transcript.contains("plan cancelled; no baseline changed"), "{transcript}");
    assert!(!cancelled.join(".jet").exists(), "TTY cancel mutated workspace");

    let applied = budget_project("budget_tty_yes", 10);
    let (code, transcript) = run_budget_update_pty(&applied, b"yes\n");
    assert_eq!(code, 0, "{transcript}");
    assert!(transcript.contains("Apply? [y/N]"), "{transcript}");
    assert_eq!(transcript.matches("+ report ").count(), 1, "plan/apply duplicated report row: {transcript}");
    assert_eq!(transcript.matches("~ baseline ").count(), 1, "plan/apply duplicated baseline row: {transcript}");
    assert!(applied.join(".jet/perf/baselines/names/ci/linux.json").is_file(), "TTY yes did not apply");
    assert!(applied.join(".jet/perf/reports").read_dir().unwrap().next().is_some(), "TTY yes omitted report artifact");
}

#[test]
fn budget_surface_is_generated_into_help_completions_and_man() {
    let help = Command::new(jet()).arg("help").output().unwrap();
    assert!(String::from_utf8(help.stdout).unwrap().contains("budget"));
    let completions = Command::new(jet()).args(["self", "completions", "bash"]).output().unwrap();
    let completions = String::from_utf8(completions.stdout).unwrap();
    assert!(completions.contains("budget"));
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let man = String::from_utf8(man.stdout).unwrap();
    assert!(man.contains("budget"));
    for flag in ["--annotations","--baseline","--bootstrap","--accept-regression","--reason","--yes","-y"] {
        assert!(completions.contains(flag),"completion omitted {flag}");
        assert!(man.contains(flag),"man page omitted {flag}");
    }
}

// ── Exit-code table ────────────────────────────────────────────────

#[test]
fn exit_code_ok_check() {
    let p = std::env::temp_dir().join("jet_cli_ok.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "clean check should exit 0");
}

#[test]
fn exit_code_user_error_check() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "a program error should exit 1");
}

#[test]
fn exit_code_no_args_starts_repl() {
    // c6vz465: bare `jet` starts the REPL — exit 0 after EOF on piped stdin.
    let out = Command::new(jet()).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "no args should start REPL (exit 0)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("interactive REPL"),
        "bare jet should print REPL banner:\n{}",
        stdout
    );
}

#[test]
fn exit_code_unknown_subcommand_is_usage() {
    // A typo'd subcommand is a usage error (exit 2) and teaches E2101.
    let out = Command::new(jet()).arg("buld").output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown subcommand should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2101"), "should cite E2101:\n{}", stderr);
    assert!(
        stderr.contains("build"),
        "should suggest `build`:\n{}",
        stderr
    );
}

#[test]
fn gc_report_missing_trace_has_registered_human_and_json_diagnostics() {
    let root = std::env::temp_dir().join(format!(
        "jet_gc_report_missing_{}_{}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let human = Command::new(jet())
        .args(["gc", "report"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert!(stderr.contains("Error [E2110]: GC trace cannot be reported"), "{stderr}");
    assert!(stderr.contains("run `jet run --gc-trace <file.jet>`"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
    check_snapshot(
        "gc_report_missing_trace_e2110.txt",
        &stderr.replace(root.to_str().unwrap(), "WORKSPACE"),
    );

    let json = Command::new(jet())
        .args(["gc", "report", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&json.stdout);
    assert!(stdout.contains("\"code\":\"E2110\""), "{stdout}");
    assert!(stdout.contains("\"severity\":\"error\""), "{stdout}");
    assert!(!stdout.contains('\u{1b}'), "{stdout}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn frequency_ring_groups_execute_real_handlers() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains(".TH JET 1"));

    let out = Command::new(jet()).args(["inspect", "semindex"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "group must reach semindex handler");
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs an entry file"));

    let out = Command::new(jet()).args(["hangar", "generations"]).output().unwrap();
    assert_ne!(out.status.code(), Some(2), "existing grouped handler must remain live");
}

#[test]
fn shape6_groups_inspect_and_registry_while_rejecting_bare_actions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hello = root.join("examples/features/basics/hello.jet");
    let dossier = Command::new(jet())
        .args(["inspect", "dossier"])
        .arg(&hello)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "grouped dossier did not reach its handler: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    assert!(String::from_utf8_lossy(&dossier.stdout).contains("run"));

    let empty = isolated_cwd("shape6_registry_publish");
    let publish = Command::new(jet())
        .args(["registry", "publish"])
        .current_dir(&empty)
        .output()
        .unwrap();
    assert_eq!(publish.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&publish.stderr).contains("no `pkg.jet` found"),
        "grouped publish did not reach its handler: {}",
        String::from_utf8_lossy(&publish.stderr)
    );

    for (bare, canonical) in [
        ("dossier", "jet inspect dossier"),
        ("publish", "jet registry publish"),
    ] {
        let out = Command::new(jet()).arg(bare).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101") && stderr.contains(canonical), "{stderr}");
    }
}

#[test]
fn shape_cli_entry_type_drives_shell_inputs_but_remains_optional() {
    let dir = isolated_cwd("shape_cli_entry_source");
    fs::write(
        dir.join("typed.jet"),
        r#"#CLI
struct RunArgs {
    #Doc("person to greet") name: String
    retries: Int = 2
    verbose: Bool
}

fn run(args: RunArgs) {
    print(args.name)
    print(args.retries)
    print(args.verbose)
}
"#,
    )
    .unwrap();
    let typed = Command::new(jet())
        .args(["run", "--release", "typed.jet", "--", "--name", "Ada", "--verbose"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        typed.status.success(),
        "typed entry failed: {}",
        String::from_utf8_lossy(&typed.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&typed.stdout), "Ada\n2\ntrue\n");

    let help = Command::new(jet())
        .args(["run", "--release", "typed.jet", "--", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    for field_fact in ["--name", "person to greet", "--retries", "--verbose"] {
        assert!(help.contains(field_fact), "typed help missing {field_fact}: {help}");
    }
    assert_eq!(
        help.lines()
            .filter(|line| line.trim_start().starts_with("--help"))
            .count(),
        1,
        "generated and Core help both claimed --help:\n{help}"
    );

    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "typed.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "typed command dossier failed: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for projected in [
        "\"entry_type\":\"RunArgs\"",
        "\"flag\":\"--name\"",
        "\"value_type\":\"String\"",
        "\"required\":true",
        "\"help\":\"person to greet\"",
        "\"flag\":\"--retries\"",
        "\"default\":\"2\"",
        "\"flag\":\"--verbose\"",
        "\"shape\":\"flag\"",
        "\"completion_words\":[\"--help\",\"name\",\"--name\",\"--retries\",\"--verbose\"]",
    ] {
        assert!(
            dossier.contains(projected),
            "typed command dossier omitted {projected}: {dossier}"
        );
    }

    for shell in ["bash", "zsh", "fish", "powershell"] {
        let completion = Command::new(jet())
            .args(["self", "completions", shell, "--for", "build/typed"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            completion.status.success(),
            "{shell} external completion failed: {}",
            String::from_utf8_lossy(&completion.stderr)
        );
        let script = String::from_utf8(completion.stdout).unwrap();
        for flag in ["help", "name", "retries", "verbose"] {
            assert!(script.contains(flag), "{shell} script omitted {flag}: {script}");
        }
        assert!(!script.contains("Ada"), "completion queried a live value: {script}");
        check_snapshot(&format!("shape_cli_for_{shell}.txt"), &script);
    }

    fs::write(dir.join("plain.jet"), "fn run() { print(\"plain\") }\n").unwrap();
    let plain = Command::new(jet())
        .args(["run", "--release", "plain.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        plain.status.success(),
        "plain fn run() became invalid: {}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&plain.stdout), "plain\n");
    let plain_completion = Command::new(jet())
        .args(["self", "completions", "bash", "--for", "build/plain"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(plain_completion.status.success());
    let plain_script = String::from_utf8(plain_completion.stdout).unwrap();
    assert!(plain_script.contains("--help"));
    assert!(!plain_script.contains("--name"));
    check_snapshot("shape_cli_for_plain.txt", &plain_script);
}

#[test]
fn typed_cli_field_markers_add_short_and_env_inputs_with_pinned_precedence() {
    let dir = isolated_cwd("typed_cli_short_env");
    fs::write(
        dir.join("typed.jet"),
        r#"#CLI
struct RunArgs {
    #[Doc("print extra detail"), Short("v")] verbose: Bool
    #[Doc("port to listen on"), Short("p"), Env("JET_TYPED_PORT")] port: Int = 3000
}

fn run(args: RunArgs) {
    print(args.verbose)
    print(args.port)
}
"#,
    )
    .unwrap();

    let run = |args: &[&str], env: Option<&str>, release: bool| {
        let mut command = Command::new(jet());
        command.arg("run");
        if release {
            command.arg("--release");
        }
        command.arg("typed.jet").arg("--").args(args).current_dir(&dir);
        match env {
            Some(value) => {
                command.env("JET_TYPED_PORT", value);
            }
            None => {
                command.env_remove("JET_TYPED_PORT");
            }
        }
        command.output().unwrap()
    };

    for release in [false, true] {
        let env_fallback = run(&["-v"], Some("4100"), release);
        assert!(
            env_fallback.status.success(),
            "typed CLI env fallback failed: {}",
            String::from_utf8_lossy(&env_fallback.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&env_fallback.stdout), "true\n4100\n");

        let long_wins = run(&["--port", "4200"], Some("4100"), release);
        assert!(long_wins.status.success());
        assert_eq!(String::from_utf8_lossy(&long_wins.stdout), "false\n4200\n");

        let short_wins = run(&["-p", "4300"], Some("4100"), release);
        assert!(short_wins.status.success());
        assert_eq!(String::from_utf8_lossy(&short_wins.stdout), "false\n4300\n");

        let default = run(&[], None, release);
        assert!(default.status.success());
        assert_eq!(String::from_utf8_lossy(&default.stdout), "false\n3000\n");
    }

    let help = run(&["--help"], None, true);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for fact in [
        "-v, --verbose",
        "-p, --port PORT",
        "[env: JET_TYPED_PORT]",
        "[default: 3000]",
    ] {
        assert!(help.contains(fact), "typed CLI help omitted {fact}: {help}");
    }
}

#[test]
fn typed_cli_entry_accepts_an_imported_argument_type() {
    let dir = isolated_cwd("shape_cli_imported_entry_type");
    fs::write(
        dir.join("args.jet"),
        r#"#CLI
pub struct RunArgs {
    #Doc("person to greet") pub name: String
    pub retries: Int = 2
    pub verbose: Bool
}
"#,
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        r#"use "args"

fn run(args: RunArgs) {
    print(args.name)
    print(args.retries)
    print(args.verbose)
}
"#,
    )
    .unwrap();

    let run = Command::new(jet())
        .args(["run", "--release", "run.jet", "--", "--name", "Ada", "--verbose"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "imported typed entry failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Ada\n2\ntrue\n");

    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "run.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "imported typed command dossier failed: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "\"entry_type\":\"RunArgs\"",
        "\"flag\":\"--name\"",
        "\"default\":\"2\"",
        "\"flag\":\"--verbose\"",
    ] {
        assert!(dossier.contains(fact), "imported CLI dossier omitted {fact}: {dossier}");
    }
}

#[test]
fn typed_cli_entry_accepts_an_imported_subcommand_type() {
    let dir = isolated_cwd("shape_cli_imported_subcommand_type");
    fs::write(
        dir.join("commands.jet"),
        r#"#CLI
pub struct ServeArgs { pub port: Int }

#CLI
pub struct ImportArgs { pub file: String }

pub enum Cmd { Serve(ServeArgs) Import(ImportArgs) }
"#,
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        r#"use "commands"

fn run(cmd: Cmd) {}
"#,
    )
    .unwrap();

    let run = Command::new(jet())
        .args(["run", "--release", "run.jet", "--", "serve", "--port", "8080"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "imported subcommand entry failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "run.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "\"entry_type\":\"Cmd\"",
        "\"name\":\"serve\"",
        "\"flag\":\"--port\"",
        "\"name\":\"import\"",
        "\"flag\":\"--file\"",
    ] {
        assert!(dossier.contains(fact), "imported subcommand dossier omitted {fact}: {dossier}");
    }
}

#[test]
fn colliding_imported_cli_type_resolution_stays_in_codegen_sync() {
    let dir = isolated_cwd("shape_cli_ambiguous_imported_type");
    fs::write(
        dir.join("cli.jet"),
        "#CLI\npub struct RunArgs { pub name: String }\n",
    )
    .unwrap();
    fs::write(
        dir.join("plain.jet"),
        "pub struct RunArgs { pub name: String }\n",
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        "use \"plain\"\nuse \"cli\"\nfn run(args: RunArgs) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "run.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert_ne!(build.status.code(), Some(101), "type ambiguity reached rustc: {stderr}");
    if !build.status.success() {
        assert!(stderr.contains("E1308"), "wrong frontend diagnostic: {stderr}");
    }
    assert!(!stderr.contains("internal compiler error"), "type ambiguity reached rustc: {stderr}");
}

#[test]
fn local_cli_type_wins_over_same_named_import() {
    let dir = isolated_cwd("shape_cli_local_type_precedence");
    fs::write(
        dir.join("other.jet"),
        "pub struct RunArgs { pub name: String }\n",
    )
    .unwrap();
    fs::write(
        dir.join("run.jet"),
        r#"use "other"

#CLI
struct RunArgs { name: String }

fn run(args: RunArgs) { print(args.name) }
"#,
    )
    .unwrap();

    let run = Command::new(jet())
        .args(["run", "--release", "run.jet", "--", "--name", "Ada"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "local CLI type did not win: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Ada\n");
}

#[test]
fn bare_project_run_prefers_run_jet() {
    let dir = isolated_cwd("run_jet_default_entry");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"entry-default\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(dir.join("run.jet"), "fn run() { print(\"run.jet\") }\n").unwrap();
    fs::write(
        dir.join("src/main.jet"),
        "fn run() { print(\"legacy main.jet\") }\n",
    )
    .unwrap();

    let run = Command::new(jet())
        .arg("run")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "bare run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "run.jet\n");
}

#[test]
fn external_completion_metadata_errors_fail_closed() {
    let dir = isolated_cwd("shape_cli_metadata_error");
    fs::write(dir.join("not-a-program"), b"not an executable").unwrap();
    let out = Command::new(jet())
        .args(["self", "completions", "bash", "--for", "not-a-program"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Error [E2103]:"));
    assert!(stderr.contains("Why:") && stderr.contains("Fix:"));
    check_snapshot("shape_cli_metadata_error_e2103.txt", &stderr);
}

#[test]
fn external_completion_rejects_hostile_files_and_names() {
    let dir = isolated_cwd("shape_cli_hostile_artifacts");
    let oversized = dir.join("oversized");
    fs::File::create(&oversized).unwrap().set_len(512 * 1024 * 1024 + 1).unwrap();
    let oversized_out = Command::new(jet())
        .args(["self", "completions", "bash", "--for", "oversized"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(oversized_out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&oversized_out.stderr).contains("larger than the 512 MiB"));

    #[cfg(unix)]
    {
        let device = Command::new(jet())
            .args(["self", "completions", "bash", "--for", "/dev/null"])
            .output()
            .unwrap();
        assert_eq!(device.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&device.stderr).contains("not a regular file"));

        let fifo = dir.join("program-fifo");
        let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        let fifo_out = Command::new(jet())
            .args(["self", "completions", "bash", "--for", "program-fifo"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(fifo_out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&fifo_out.stderr).contains("not a regular file"));
    }

    let hostile = "safe\nINJECT_COMMAND\nnext";
    fs::write(dir.join(hostile), b"not an executable").unwrap();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = Command::new(jet())
            .args(["self", "completions", shell, "--for", hostile])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty(), "{shell} emitted attacker-controlled script bytes");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("contains a control character"));
        assert!(!stderr.contains("\nINJECT_COMMAND\n"), "{shell} exposed an executable line: {stderr:?}");
        assert!(stderr.contains("safe\\nINJECT_COMMAND\\nnext"));
    }
}

#[test]
fn external_completion_preserves_checked_subcommands() {
    let dir = isolated_cwd("shape_cli_subcommands");
    fs::write(dir.join("commands.jet"), r#"#CLI
struct ServeArgs {
    #[Doc("port to listen on"), Short("p"), Env("JET_SERVE_PORT")] port: Int = 3000
}
#CLI
struct ImportArgs {
    #Doc("file to import") file: String
}
enum Cmd { Serve(ServeArgs) Import(ImportArgs) }
fn run(cmd: Cmd) {}
"#).unwrap();
    let build = Command::new(jet()).args(["build", "commands.jet"]).current_dir(&dir).output().unwrap();
    assert!(build.status.success(), "subcommand build failed: {}", String::from_utf8_lossy(&build.stderr));
    let help = Command::new(dir.join("build/commands"))
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(help.status.success(), "subcommand root help failed: {}", String::from_utf8_lossy(&help.stderr));
    let help = String::from_utf8(help.stdout).unwrap();
    assert_eq!(help, "Usage: commands <command> [options]\n\nCommands:\n  serve\n  import\n");
    assert!(!help.contains("Serve") && !help.contains("Import"));

    for shell in ["bash", "zsh", "fish", "powershell"] {
        let completion = Command::new(jet())
            .args(["self", "completions", shell, "--for", "build/commands"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(completion.status.success(), "{shell} subcommand completion failed: {}", String::from_utf8_lossy(&completion.stderr));
        let script = String::from_utf8(completion.stdout).unwrap();
        let expected = match shell {
            "bash" => ["serve", "import", "--port -p", "file --file"],
            "zsh" => ["serve", "import", "{-p,--port}", ":file:file to import"],
            "fish" => ["serve", "import", "-l port -s p", "-l file"],
            "powershell" => ["serve", "import", "'--port','-p'", "'file','--file'"],
            _ => unreachable!(),
        };
        for fragment in expected {
            assert!(script.contains(fragment), "{shell} external completion omitted {fragment}: {script}");
        }
        check_snapshot(&format!("shape_cli_enum_{shell}.txt"), &script);
    }
    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "commands.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(dossier.contains("\"completion_words\":[\"--help\",\"serve\",\"import\"]"), "dossier flattened enum flags: {dossier}");
    for fact in ["\"commands\":[", "\"name\":\"serve\"", "\"name\":\"import\"", "\"flag\":\"--port\"", "\"flag\":\"--file\""] {
        assert!(dossier.contains(fact), "dossier omitted {fact}: {dossier}");
    }
    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "commands.jet", "run"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "command serve",
        "-p / --port: Int (optional, default 3000, env JET_SERVE_PORT) — port to listen on",
        "command import",
        "--file: String (required) — file to import",
    ] {
        assert!(dossier.contains(fact), "text dossier omitted {fact}: {dossier}");
    }
}

#[test]
fn derived_help_uses_program_basename_for_compiled_and_jet_run_paths() {
    let dir = isolated_cwd("shape_cli_help_program_name");
    fs::write(
        dir.join("typed.jet"),
        "#CLI\nstruct RunArgs { verbose: Bool }\nfn run(args: RunArgs) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "typed.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "typed build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built = dir.join("build/typed").canonicalize().unwrap();
    let compiled_help = Command::new(&built)
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        compiled_help.status.success(),
        "compiled typed help failed: {}",
        String::from_utf8_lossy(&compiled_help.stderr)
    );

    let run_help = Command::new(jet())
        .args(["run", "typed.jet", "--", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run_help.status.success(),
        "jet run typed help failed: {}",
        String::from_utf8_lossy(&run_help.stderr)
    );

    let program_names = format!(
        "compiled: {}\njet run: {}\n",
        String::from_utf8(compiled_help.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
        String::from_utf8(run_help.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
    );
    check_snapshot("shape_cli_help_program_names.txt", &program_names);
}

#[test]
fn derived_enum_help_uses_program_basename_for_compiled_and_jet_run_paths() {
    let dir = isolated_cwd("shape_cli_enum_help_program_name");
    fs::write(
        dir.join("commands.jet"),
        "#CLI\nstruct ServeArgs { verbose: Bool }\nenum Cmd { Serve(ServeArgs) }\nfn run(cmd: Cmd) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "commands.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "enum build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built = dir.join("build/commands").canonicalize().unwrap();
    let compiled_root = Command::new(&built)
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    let compiled_sub = Command::new(&built)
        .args(["serve", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let run_root = Command::new(jet())
        .args(["run", "commands.jet", "--", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let run_sub = Command::new(jet())
        .args(["run", "commands.jet", "--", "serve", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    for (label, output) in [
        ("compiled root", &compiled_root),
        ("compiled subcommand", &compiled_sub),
        ("jet run root", &run_root),
        ("jet run subcommand", &run_sub),
    ] {
        assert!(
            output.status.success(),
            "{label} help failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let first_line = |output: Output| {
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    let program_names = format!(
        "compiled root: {}\ncompiled subcommand: {}\njet run root: {}\njet run subcommand: {}\n",
        first_line(compiled_root),
        first_line(compiled_sub),
        first_line(run_root),
        first_line(run_sub)
    );
    check_snapshot("shape_cli_enum_help_program_names.txt", &program_names);
}

#[test]
fn moved_bare_commands_are_teaching_errors_not_aliases() {
    for (verb, replacement) in [
        ("publish", "jet registry publish"),
        ("semindex", "jet inspect semindex"),
        ("doctor", "jet self doctor"),
        ("lsp", "jet self lsp"),
        ("push", "jet os push"),
    ] {
        let out = Command::new(jet()).arg(verb).arg("sentinel").output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{verb} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{verb}: {stderr}");
        assert!(stderr.contains(replacement), "{verb}: {stderr}");
    }

    let out = Command::new(jet()).args(["lsp", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"code\":\"E2101\""));
    assert!(stdout.contains("jet self lsp --json"));
}

/// D-CLI-STORE2=A / D-CLI-DEVSERVE1=A / D-CLI-SURFACE3=B: words retired with
/// **no** `jet <group> <same-word>` rename — `teach_retired`'s bespoke path
/// (`RETIRED_BARE` in `crates/jet-cli/src/CLI.rs`), not the generic `moved_command` one.
#[test]
fn retired_bespoke_words_teach_real_spelling() {
    for (argv, replacement) in [
        (vec!["gc"], "jet clean"),
        (vec!["store", "verify"], "jet hangar verify"),
        (vec!["store", "generations"], "jet hangar generations"),
        (vec!["store", "gc"], "jet clean"),
        (vec!["store", "fetch"], "jet fetch"),
        (vec!["serve", "main.jet"], "jet dev main.jet --swap"),
        (vec!["lock", "stats.jet"], "jet fetch --lock stats.jet"),
    ] {
        let out = Command::new(jet()).args(&argv).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{argv:?} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{argv:?}: {stderr}");
        assert!(stderr.contains(replacement), "{argv:?}: {stderr}");
    }
}

#[test]
fn every_moved_bare_action_is_e2101_in_human_and_json_modes() {
    for group in jet::CLI::COMMAND_GROUPS {
        for action in group.actions {
            // `import` names both the canonical source translator and a
            // physical hangar action. Only actions without a canonical
            // top-level meaning are moved bare commands.
            if jet::CLI::is_canonical_top_level(action.name) {
                continue;
            }
            let owner = jet::CLI::moved_command_group(action.name).unwrap_or(group.name);
            let replacement = format!("jet {} {}", owner, action.name);
            let out = Command::new(jet()).arg(action.name).arg("sentinel").output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {}", action.name);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("E2101") && stderr.contains(&replacement), "{}: {stderr}", action.name);

            let out = Command::new(jet()).args([action.name, "sentinel\\\"quoted", "--json"]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {} --json", action.name);
            assert!(out.stderr.is_empty(), "JSON diagnostic leaked stderr for {}", action.name);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("\"code\":\"E2101\"") && stdout.contains(&replacement), "{}: {stdout}", action.name);
            assert!(stdout.contains("sentinel\\\\\\\"quoted"), "replacement was not JSON escaped: {stdout}");
        }
    }
}

#[test]
fn invalid_nested_action_is_e2101_and_json_escaped() {
    let bad = "bad\\\"action";
    // D-CLI-SURFACE3=B: `os` is not exhaustive (see `CommandGroup::exhaustive`)
    // — an unmodeled subword falls through to the real `jet os` dispatcher,
    // which teaches its own (non-E2101) "not a jetos verb" error, not this
    // registry's generic invalid-action path.
    for group in jet::CLI::COMMAND_GROUPS.iter().filter(|g| g.exhaustive) {
        let out = Command::new(jet()).args([group.name, bad]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101") && stderr.contains(bad), "{stderr}");

        let out = Command::new(jet()).args([group.name, bad, "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("bad\\\\\\\"action"), "invalid JSON escaping: {stdout}");
        assert!(!stdout.contains("`bad\\\"action`"), "raw quote leaked into JSON: {stdout}");
    }
}

#[test]
fn grouped_e2101_human_and_json_goldens() {
    let moved = Command::new(jet()).args(["publish", "sentinel"]).output().unwrap();
    assert_eq!(moved.status.code(), Some(2));
    assert!(moved.stdout.is_empty());
    check_snapshot("moved_bare_e2101_human.txt", &String::from_utf8_lossy(&moved.stderr));

    let moved_json = Command::new(jet()).args(["publish", "sentinel\\\"quoted", "--json"]).output().unwrap();
    assert_eq!(moved_json.status.code(), Some(2));
    assert!(moved_json.stderr.is_empty());
    check_snapshot("moved_bare_e2101_json.txt", &String::from_utf8_lossy(&moved_json.stdout));

    let invalid = Command::new(jet()).args(["inspect", "bad\\\"action"]).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    check_snapshot("invalid_nested_e2101_human.txt", &String::from_utf8_lossy(&invalid.stderr));

    let invalid_json = Command::new(jet()).args(["inspect", "bad\\\"action", "--json"]).output().unwrap();
    assert_eq!(invalid_json.status.code(), Some(2));
    assert!(invalid_json.stderr.is_empty());
    check_snapshot("invalid_nested_e2101_json.txt", &String::from_utf8_lossy(&invalid_json.stdout));
}

#[test]
fn group_help_and_man_inventory_every_nested_description() {
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(man.status.code(), Some(0));
    let man = String::from_utf8_lossy(&man.stdout);
    for group in jet::CLI::COMMAND_GROUPS {
        // D-CLI-SURFACE3=B: a non-exhaustive group (`os`) doesn't own its bare
        // `help` output — that stays the real `jet os` dispatcher's, which
        // this registry can't predict — so only the *static* man-page
        // inventory is checked for it. An exhaustive group's `help` is
        // CLI-owned and must list every action.
        if group.exhaustive {
            let out = Command::new(jet()).args([group.name, "help"]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let help = String::from_utf8_lossy(&out.stdout);
            assert!(help.contains(group.summary), "{} help missing summary", group.name);
            for action in group.actions {
                assert!(help.contains(action.name) && help.contains(action.summary), "{} help missing {}", group.name, action.name);
            }
        }
        for action in group.actions {
            assert!(man.contains(&format!(".B {} {}", group.name, action.name)), "man missing {} {}", group.name, action.name);
            assert!(man.contains(action.summary), "man missing summary for {} {}", group.name, action.name);
        }
    }
}

#[test]
fn palette_uses_canonical_nested_routes() {
    for group in jet::CLI::COMMAND_GROUPS {
        for action in group.actions {
            let route = format!("{} {}", group.name, action.name);
            let out = Command::new(jet()).args(["?", &route]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains(&route), "palette missing {route}: {stdout}");
            assert!(!stdout.contains(&format!("jet {}   ", action.name)), "palette advertised bare moved action {}", action.name);
        }
    }
}

#[test]
fn jet_install_teaches_jet_fetch() {
    // `jet install` is not a Jet command; the compiler emits E0043 pointing to `jet fetch`.
    let out = Command::new(jet()).arg("install").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0043"),
        "`jet install` should emit E0043 teaching error:\n{stderr}"
    );
    assert!(
        stderr.contains("jet fetch"),
        "`jet install` error should mention `jet fetch`:\n{stderr}"
    );
}

#[test]
fn exit_code_explain_unknown() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E9999")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "unknown code should exit 1");
}

// ── Human + JSON golden for one diagnostic ────────────────────────

#[test]
fn check_human_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_human.txt", &stderr);
}

#[test]
fn check_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_json.txt", &stderr);
}

#[test]
fn build_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("build")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("build_json.txt", &stderr);
}

#[test]
fn test_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("test")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("test_json.txt", &stderr);
}

// ── CI determinism: ANSI-free + identical when piped/NO_COLOR ──────

#[test]
fn ci_output_is_ansi_free_when_piped() {
    let p = bad_file(&line!().to_string());
    // Default (piped, not a TTY): must be plain.
    let piped = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains('\x1b'),
        "piped output must be ANSI-free:\n{}",
        s
    );

    // NO_COLOR explicitly set: also plain.
    let no_color = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let nc = String::from_utf8_lossy(&no_color.stderr);
    assert!(!nc.contains('\x1b'), "NO_COLOR output must be ANSI-free");

    // And the two must be byte-identical (determinism).
    assert_eq!(s, nc, "piped and NO_COLOR output must match exactly");
}

#[test]
fn color_always_adds_ansi_but_flag_wins_over_no_color() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--color=always")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(
        s.contains('\x1b'),
        "--color=always must win over NO_COLOR and emit ANSI"
    );
}

// ── explain coverage: every registered code resolves ──────────────

#[test]
fn every_registered_code_has_an_explain_entry() {
    let md = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/spec/diagnostics.md"),
    )
    .unwrap();

    // Pull every E####/L#### that appears as the first cell of a table row —
    // i.e. a registered code, not an in-prose mention.
    let mut codes: Vec<String> = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with("| E") && !line.starts_with("| L") {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if is_code(first) && !codes.contains(&first.to_string()) {
            codes.push(first.to_string());
        }
    }
    assert!(
        codes.len() > 150,
        "expected the full code registry, found {}",
        codes.len()
    );

    let index = jet::Explain::index();
    for code in &codes {
        assert!(
            index.contains_key(code),
            "code {} is registered in diagnostics.md but has no explain entry",
            code
        );
        // And `jet explain <code>` must succeed at the CLI for every code.
        let out = Command::new(jet())
            .arg("explain")
            .arg(code)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "`jet explain {}` should succeed",
            code
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(code.as_str()),
            "`jet explain {}` output should name the code",
            code
        );
    }
}

#[test]
fn explain_golden() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E2001")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("explain_E2001.txt", &stdout);
}

#[test]
fn explain_e2211_golden() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E2211")
        .output()
        .unwrap();
    assert!(out.status.success(), "jet explain E2211 should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("This code is retired"), "{stdout}");
    check_snapshot("explain_E2211.txt", &stdout);
}

#[test]
fn default_jet_run_deopts_jit_gap_silently() {
    let dir = isolated_cwd("jit_gap_run");
    let file = dir.join("env.jet");
    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let output = Command::new(jet())
        .args(["run", file.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}\nstdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("E2211"), "E2211 retired: {stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).is_empty(),
        "deopted env.current_dir() should print a path"
    );
}

#[test]
fn malformed_advisory_database_is_e2607_snapshot() {
    let dir = isolated_cwd("audit_e2607");
    fs::write(dir.join("pkg.jet"), "package app 0.1.0\n").unwrap();
    fs::create_dir(dir.join(".jet")).unwrap();
    fs::write(dir.join(".jet/lock"), "version = 1\n").unwrap();
    let advisory_db = dir.join("advisories.txt");
    fs::write(&advisory_db, "missing|fields|only\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "audit", "--advisory-db"])
        .arg(&advisory_db)
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    check_snapshot(
        "audit_malformed_e2607.txt",
        &String::from_utf8(output.stderr).unwrap(),
    );
}

#[test]
fn jetpack_missing_build_log_golden() {
    let cwd = isolated_cwd(&line!().to_string());
    let root = cwd.join("jetpack-root");
    let out = Command::new(jet())
        .args(["inspect", "logs", "definitely_missing", "--no-color"])
        .current_dir(&cwd)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing log is usage-class error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("e1274_missing_build_log.txt", &stderr);
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}

// ── Wave B: greeting, did-you-mean, doctor, completions, fix, externals ──

#[test]
fn no_args_repl_banner_golden() {
    let out = Command::new(jet()).env("NO_COLOR", "1").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("no_args_repl_banner.txt", &stdout);
}

#[test]
fn question_mark_is_help_golden() {
    let out = Command::new(jet())
        .arg("?")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ?` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("question_mark_help.txt", &stdout);
}

/// D-FE-HELP1=D: `jet ? <query>` (piped, i.e. non-TTY) is the non-interactive
/// floor — best matches for the query, printed once, no raw mode.
#[test]
fn question_mark_query_prints_matches_non_interactively() {
    let out = Command::new(jet())
        .args(["?", "run"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ? run` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet run"), "expected a `run` match, got:\n{}", stdout);
}

#[test]
fn question_mark_language_symbol_uses_shared_semantic_index() {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["?", "List.filter"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run jet ? List.filter");
    assert!(output.status.success(), "status: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("List.filter(f: fn(T) => Bool) -> List<T>"), "signature missing: {stdout}");
    assert!(stdout.contains("Keeps items where f(item) is true."), "summary missing: {stdout}");
    assert!(stdout.contains("Example:"), "example missing: {stdout}");
    assert!(stdout.contains("core.collections"), "provenance missing: {stdout}");
}

/// A query that looks like a diagnostic code renders the verbatim I4 essay —
/// byte-identical to `jet explain <CODE>`, since both go through
/// `jet::Explain::render` over the same registry (single source of truth).
#[test]
fn question_mark_code_query_matches_explain_verbatim() {
    let via_help = Command::new(jet())
        .args(["?", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let via_explain = Command::new(jet())
        .args(["explain", "E0102"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(via_help.status.code(), Some(0));
    assert_eq!(via_explain.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&via_help.stdout),
        String::from_utf8_lossy(&via_explain.stdout),
        "`jet ? E0102` must render the same verbatim essay as `jet explain E0102` (I4)"
    );
}

/// A multi-word task/outcome phrase still resolves to a real command line —
/// the owner-modified default (2026-07-08): keywords are aliases on command
/// entries, never a separate goal menu, but they must still be findable.
#[test]
fn question_mark_task_phrase_resolves_to_a_real_command() {
    let out = Command::new(jet())
        .args(["?", "add", "a", "dependency"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet add"), "expected `add` to surface, got:\n{}", stdout);
}

#[test]
fn file_sugar_runs_without_run_subcommand() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"file-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&file).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <file> sugar should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("file-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_ext_optional() {
    let stem = std::env::temp_dir().join("jet_cli_file_sugar_extopt");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ext-sugar\");\n}\n").unwrap();
    let out = Command::new(jet()).arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "jet <stem> sugar should resolve .jet; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ext-sugar"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn file_sugar_missing_jet_file_errors() {
    let missing = std::env::temp_dir().join("jet_cli_file_sugar_absent.jet");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        combined.contains("jet_cli_file_sugar_absent"),
        "missing file should be named in output: {combined}"
    );
}

#[test]
fn did_you_mean_golden() {
    let out = Command::new(jet())
        .arg("buld")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("did_you_mean.txt", &stderr);
}

#[test]
fn unknown_flag_is_e2102() {
    let p = std::env::temp_dir().join("jet_cli_ok2.jet");
    fs::write(&p, "fn run() {\n    print(\"hi\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--jsn")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "unknown flag should exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{}", stderr);
    assert!(
        stderr.contains("--json"),
        "should suggest --json:\n{}",
        stderr
    );
}

#[test]
fn doctor_ok_golden() {
    // On a CI/dev box rustc is present; the report is deterministic except for
    // machine-specific paths and the rustc version, which we scrub.
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Doctor must never emit ANSI when piped.
    assert!(
        !s.contains('\x1b'),
        "doctor output must be ANSI-free when piped"
    );
    // Structural assertions (a full golden would be machine-specific).
    assert!(s.contains("doctor"), "missing header:\n{}", s);
    assert!(s.contains("rustc"), "missing rustc check:\n{}", s);
    assert!(s.contains("pkg-config"), "missing C-FFI section:\n{}", s);
    assert!(s.contains("hangar"), "missing hangar check:\n{}", s);
}

#[test]
fn doctor_failure_is_l2101_snapshot() {
    let out = Command::new(jet())
        .args(["self", "doctor"])
        .env("PATH", "")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout.find("Warning [L2101]:").expect("L2101 diagnostic");
    check_snapshot("doctor_l2101.txt", &stdout[start..]);
}

#[test]
fn fetch_without_git_is_e1203_snapshot() {
    let dir = isolated_cwd("fetch_no_git");
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\", jet: \">=0.1.0\", description: \"\", license: \"MIT\" }\npackages: { app: executable }\ndeps: { tool: { git: \"https://example.invalid/tool.git\", tag: \"v1\" } }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .args(["fetch"])
        .current_dir(&dir)
        .env("PATH", "")
        .env("HOME", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    let start = stderr.find("Error [E1203]:").expect("E1203 diagnostic");
    check_snapshot("fetch_no_git_e1203.txt", &stderr[start..]);
}

#[test]
fn bind_missing_header_is_e3208() {
    let missing = std::env::temp_dir().join("jet_missing_bind_header.h");
    let _ = fs::remove_file(&missing);
    let out = Command::new(jet())
        .args(["inspect", "bind"])
        .arg(&missing)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3208]:"), "missing bind diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3208 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3208 fix:\n{stderr}");
    check_snapshot("bind_missing_e3208.txt", &scrub(&stderr, &missing));
}

#[test]
fn fortran_bind_compiles_and_runs_iso_c_binding_scalar() {
    let dir = isolated_cwd("fortran_bind_scalar");
    let source = dir.join("scalar.f90");
    fs::write(
        &source,
        r#"module scalar_math
  use iso_c_binding
contains
  function add_i64(a, b) result(value) bind(C, name="add_i64")
    integer(c_int64_t), value :: a
    integer(c_int64_t), value :: b
    integer(c_int64_t) :: value
    value = a + b
  end function add_i64
end module scalar_math
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "scalar"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Fortran bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    assert!(dir.join(".jet/bindings/fortran/scalar.jet").is_file());
    assert!(dir.join(".jet/bindings/fortran/libjet_fortran_scalar.a").is_file());

    fs::write(
        dir.join("main.jet"),
        "use fortran.scalar as scalar\n\nfn run() { print(scalar.add_i64(20, 22)) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Fortran binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn fortran_bind_runs_checked_column_major_array() {
    let dir = isolated_cwd("fortran_bind_array");
    let source = dir.join("matrix.f90");
    fs::write(
        &source,
        r#"module matrix_math
  use iso_c_binding
contains
  function probe(a) result(value) bind(C, name="probe_column_major")
    real(c_double), intent(in) :: a(2,3)
    real(c_double) :: value
    value = 100.0_c_double * a(1,2) + a(2,1)
  end function probe
end module matrix_math
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "matrix"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Fortran array bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(dir.join(".jet/bindings/fortran/matrix.jet")).unwrap();
    assert!(generated.contains("fortran-layout probe.a: column-major 2x3"));
    assert!(generated.contains("a.len() != 6"));
    assert!(generated.contains("=[Fortran]=>"));
    assert!(String::from_utf8_lossy(&bind.stdout).contains("layout: probe.a column-major 2x3"));

    fs::write(
        dir.join("main.jet"),
        "use fortran.matrix as matrix\n\nfn run() { print(matrix.probe([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Fortran array binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    // Fortran sees the flat input in column-major order: a(1,2)=3, a(2,1)=2.
    assert_eq!(String::from_utf8_lossy(&run.stdout), "302.0\n");

    fs::write(
        dir.join("bad.jet"),
        "use fortran.matrix as matrix\n\nfn run() { print(matrix.probe([1.0, 2.0, 3.0, 4.0, 5.0])) }\n",
    )
    .unwrap();
    let bad = Command::new(jet())
        .args(["run", "--release", "bad.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr)
            .contains("a must contain exactly 6 column-major values"),
        "missing checked array length failure:\n{}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

#[test]
fn go_bind_compiles_and_runs_c_archive_scalar() {
    let dir = isolated_cwd("go_bind_scalar");
    let source = dir.join("scalar.go");
    fs::write(
        &source,
        r#"package main

/*
#include <stdint.h>
*/
import "C"

//export add_i64
func add_i64(a int64, b int64) int64 {
    return a + b
}

func main() {}
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "scalar"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Go bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    assert!(dir.join(".jet/bindings/go/scalar.jet").is_file());
    assert!(dir.join(".jet/bindings/go/libjet_go_scalar.a").is_file());

    fs::write(
        dir.join("main.jet"),
        "use go.scalar as scalar\n\nfn run() { print(scalar.add_i64(20, 22)) }\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Go binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_compiles_and_runs_move_only_cgo_handle() {
    let dir = isolated_cwd("go_bind_handle");
    let source = dir.join("handles.go");
    fs::write(
        &source,
        r#"package main

/*
#include <stdint.h>
*/
import "C"
import "runtime/cgo"

//export new_handle
func new_handle(value int64) uintptr {
    return uintptr(cgo.NewHandle(value))
}

//export consume_handle
func consume_handle(handle uintptr) int64 {
    owned := cgo.Handle(handle)
    value := owned.Value().(int64)
    owned.Delete()
    return value
}

func main() {}
"#,
    )
    .unwrap();

    let bind = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "handles"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        bind.status.success(),
        "Go handle bind failed:\n{}",
        String::from_utf8_lossy(&bind.stderr)
    );
    let generated = fs::read_to_string(dir.join(".jet/bindings/go/handles.jet")).unwrap();
    assert!(generated.contains("pub struct Handle { value: Int }"));
    assert!(generated.contains("pub fn new_handle(value: Int) => Handle"));
    assert!(generated.contains("pub fn consume_handle(handle: Handle) => Int"));

    fs::write(
        dir.join("main.jet"),
        "use go.handles as handles\n\nfn run() =[Go, IO]=> {\n    handle :: handles.new_handle(42)\n    print(handles.consume_handle(handle))\n}\n",
    )
    .unwrap();
    let run = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated Go handle binding did not run:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
}

#[test]
fn go_bind_launders_foreign_compiler_failure_as_e3208() {
    let dir = isolated_cwd("go_bind_failure");
    let source = dir.join("broken.go");
    fs::write(
        &source,
        r#"package main

import "C"

//export broken
func broken(a int64) int64 {
    return a +
}

func main() {}
"#,
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["inspect", "bind", "go"])
        .arg(&source)
        .args(["--pkg", "broken"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"), "missing Jet diagnostic:\n{stderr}");
    assert!(stderr.contains(" Why:"), "missing reason:\n{stderr}");
    assert!(stderr.contains(" Fix:"), "missing fix:\n{stderr}");
    assert!(!stderr.contains("broken.go:"), "raw Go location leaked:\n{stderr}");
    check_snapshot("bind_go_invalid_e3208.txt", &scrub(&stderr, &source));
}

#[test]
fn java_bind_embeds_jvm_handles_methods_and_exceptions() {
    let dir = isolated_cwd("java_bind_embedded");
    let source = dir.join("Counter.java");
    fs::write(&source, r#"public class Counter {
    private long value;
    public Counter(long value) { this.value = value; }
    public long add(long amount) { value += amount; return value; }
    public long explode(long code) { if (code < 0) throw new IllegalStateException("hidden foreign detail"); return code; }
    public static double twice(double value) { return value * 2.0; }
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","java"]).arg(&source).args(["--pkg","counter"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Java bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/java/libjet_java_counter.a").is_file());
    assert!(dir.join(".jet/bindings/java/counter.classes/Counter.class").is_file());
    assert!(dir.join(".jet/bindings/java/counter.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use java.counter as counter

fn run() =[Java, IO]=> {
    handle :: counter.new(40) ?? panic("JVM create failed")
    print(counter.add(handle, 2) ?? -1)
    print(counter.twice(2.5) ?? -1.0)
    print(counter.explode(handle, -1) ?? -7)
    counter.close(^handle)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"embedded JVM binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n5.0\n-7\n");
    assert!(!String::from_utf8_lossy(&run.stderr).contains("hidden foreign detail"));
}

#[test]
fn java_bind_launders_javac_failure_as_e3208() {
    let dir=isolated_cwd("java_bind_failure"); let source=dir.join("Broken.java");
    fs::write(&source,"public class Broken { public Broken(long n) { this. = n; } public long value() { return 1; } }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","java"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success()); let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:")); assert!(stderr.contains(" Why:")); assert!(stderr.contains(" Fix:"));
    assert!(!stderr.contains("Broken.java:"),"raw javac location leaked:\n{stderr}");
    check_snapshot("bind_java_invalid_e3208.txt", &scrub(&stderr, &source));
}

#[test]
fn dotnet_bind_embeds_coreclr_state_calls_and_errors(){
    let dir=isolated_cwd("dotnet_bind_embedded");let source=dir.join("Counter.cs");fs::write(&source,r#"public class Counter {
    private long value;
    public Counter(long value) { this.value = value; }
    public long add(long amount) { value += amount; return value; }
    public long explode(long code) { if (code < 0) throw new System.InvalidOperationException("hidden managed detail"); return code; }
    public static double twice(double value) { return value * 2.0; }
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","cs"]).arg(&source).args(["--pkg","counter"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),".NET bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/cs/libjet_cs_counter.a").is_file());assert!(dir.join(".jet/bindings/cs/counter.dotnet/JetBinding.dll").is_file());assert!(dir.join(".jet/bindings/cs/counter.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use cs.counter as counter

fn run() =[DotNet, IO]=> {
    handle :: counter.new(40) ?? panic("CoreCLR create failed")
    print(counter.add(handle, 2) ?? -1)
    print(counter.twice(2.5) ?? -1.0)
    print(counter.explode(handle, -1) ?? -7)
    counter.close(^handle)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"embedded CoreCLR binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n5.0\n-7\n");assert!(!String::from_utf8_lossy(&run.stderr).contains("hidden managed detail"));
}

#[test]
fn dotnet_bind_launders_compiler_failure_as_e3208(){let dir=isolated_cwd("dotnet_bind_failure");let source=dir.join("Broken.cs");fs::write(&source,"public class Broken { public Broken(long n) { this. = n; } public long value() => 1; }\n").unwrap();let output=Command::new(jet()).args(["inspect","bind","cs"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));assert!(!stderr.contains("Broken.cs:"),"raw C# source frame leaked:\n{stderr}");check_snapshot("bind_dotnet_invalid_e3208.txt",&scrub(&stderr,&source));}

#[test]
fn tcl_bind_runs_one_shot_and_persistent_typed_sessions() {
    let dir=isolated_cwd("tcl_bind_session");let source=dir.join("eda.tcl");
    fs::write(&source,"set counter 40\n").unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","tcl"]).arg(&source).args(["--pkg","eda"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Tcl bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/tcl/libjet_tcl_eda.a").is_file());
    assert!(dir.join(".jet/bindings/tcl/eda.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use tcl.eda as tcl

fn run() =[Tcl, IO]=> {
    session :: tcl.open() ?? panic("Tcl open failed")
    print(tcl.eval_int(session, "incr counter 2") ?? -1)
    print(tcl.eval_int(session, "incr counter 1") ?? -1)
    print(tcl.eval_once("expr 6 * 7") ?? "bad")
    print(tcl.eval_float(session, "expr 5.0 / 2") ?? -1.0)
    print(tcl.eval(session, "error \"foreign stack secret\"") ?? "tcl-error")
    tcl.close(^session)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"embedded Tcl binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n43\n42\n2.5\ntcl-error\n");
    assert!(!String::from_utf8_lossy(&run.stderr).contains("foreign stack secret"));
}

#[test]
fn ada_bind_compiles_runs_and_rejects_range_before_call() {
    let dir=isolated_cwd("ada_bind_range");let spec=dir.join("geodesy.ads");let body=dir.join("geodesy.adb");
    fs::write(&spec,r#"with Interfaces.C;
use type Interfaces.C.double;
package Geodesy is
   subtype Latitude is Interfaces.C.double range -90.0 .. 90.0;
   function Double_Lat (Lat : Latitude) return Interfaces.C.double
     with Export, Convention => C, External_Name => "geo_double";
   function Calls (Unused : Interfaces.C.long_long) return Interfaces.C.long_long
     with Export, Convention => C, External_Name => "geo_calls";
end Geodesy;
"#).unwrap();
    fs::write(&body,r#"with Interfaces.C;
use type Interfaces.C.double;
use type Interfaces.C.long_long;
package body Geodesy is
   Calls_Count := Interfaces.C.long_long.{ 0 };
   function Double_Lat (Lat : Latitude) return Interfaces.C.double is
   begin
      Calls_Count := Calls_Count + 1;
      return Lat * 2.0;
   end Double_Lat;
   function Calls (Unused : Interfaces.C.long_long) return Interfaces.C.long_long is
   begin
      return Calls_Count + Unused;
   end Calls;
end Geodesy;
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","ada"]).arg(&spec).args(["--pkg","geodesy"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Ada bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    assert!(dir.join(".jet/bindings/ada/libjet_ada_geodesy.a").is_file());
    assert!(dir.join(".jet/bindings/ada/geodesy.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use ada.geodesy as geo

fn run() =[Ada, IO]=> {
    print(geo.double_lat(95.0) ?? -1.0)
    print(geo.calls(0) ?? -1)
    print(geo.double_lat(21.0) ?? -1.0)
    print(geo.calls(0) ?? -1)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"generated Ada binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout),"-1.0\n0\n42.0\n1\n");
}

#[test]
fn pascal_bind_runs_scalar_and_owned_class_lifecycle() {
    let dir=isolated_cwd("pascal_bind_lifecycle");let source=dir.join("inventory.pas");
    fs::write(&source,r#"library inventory;
type
  TCounter = class
  private
    FValue: Int64;
  public
    constructor Create(Value: Int64);
    function Add(Delta: Int64): Int64;
    destructor Destroy; override;
  end;
var Destroyed: Int64 = 0;
constructor TCounter.Create(Value: Int64);
begin inherited Create; FValue := Value; end;
function TCounter.Add(Delta: Int64): Int64;
begin FValue := FValue + Delta; Result := FValue; end;
destructor TCounter.Destroy;
begin Destroyed := Destroyed + 1; inherited Destroy; end;
function add_scalar(A, B: Int64): Int64; cdecl;
begin Result := A + B; end;
function counter_new(Value: Int64): Pointer; cdecl;
begin Result := Pointer(TCounter.Create(Value)); end;
function counter_add(Handle: Pointer; Delta: Int64): Int64; cdecl;
begin Result := TCounter(Handle).Add(Delta); end;
procedure counter_free(Handle: Pointer); cdecl;
begin TCounter(Handle).Free; end;
function destroyed_count(): Int64; cdecl;
begin Result := Destroyed; end;
exports add_scalar, counter_new, counter_add, counter_free, destroyed_count;
begin
end.
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","pascal"]).arg(&source).args(["--pkg","inventory"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Pascal bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    let cache=dir.join(".jet/bindings/pascal");assert!(cache.join("libjet_pascal_inventory.a").is_file());assert!(cache.join("libjet_pascal_inventory_runtime.so").is_file());assert!(cache.join("inventory.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use pascal.inventory as inv

fn run() =[Pascal, IO]=> {
    print(inv.add_scalar(20, 22))
    handle :: inv.counter_new(40) ?? panic("Pascal constructor failed")
    print(inv.counter_add(handle, 2) ?? -1)
    print(inv.destroyed_count())
    inv.close(^handle)
    print(inv.destroyed_count())
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(run.status.success(),"generated Pascal binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n42\n0\n1\n");
    fs::write(dir.join("stale.c"),r#"#include <stdint.h>
extern int64_t jet_pascal_inventory_counter_new(int64_t);
extern void jet_pascal_inventory_counter_close(int64_t);
extern int64_t jet_pascal_inventory_take_error(void);
extern int64_t jet_pascal_inventory_destroyed_count(void);
int main(void){int64_t h=jet_pascal_inventory_counter_new(1);if(!h)return 1;jet_pascal_inventory_counter_close(h);if(jet_pascal_inventory_destroyed_count()!=1)return 2;jet_pascal_inventory_counter_close(h);if(jet_pascal_inventory_take_error()!=1)return 3;if(jet_pascal_inventory_destroyed_count()!=1)return 4;return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("stale.c").args(["-L.jet/bindings/pascal","-Wl,-rpath,.jet/bindings/pascal","-l:libjet_pascal_inventory.a","-ljet_pascal_inventory_runtime","-lpthread","-ldl","-o","stale"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"stale-handle probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let stale=Command::new(dir.join("stale")).current_dir(&dir).output().unwrap();assert!(stale.status.success(),"stale close reached Pascal destructor twice: {:?}",stale.status.code());
}

#[test]
fn pascal_bind_launders_fpc_failure_as_e3208() {
    let dir=isolated_cwd("pascal_bind_failure");let source=dir.join("broken.pas");
    fs::write(&source,"library broken; type TCounter = class end; function counter_new(Value: Int64): Pointer; cdecl; begin Result := ; end; function counter_add(Handle: Pointer; Delta: Int64): Int64; cdecl; begin Result := 0; end; procedure counter_free(Handle: Pointer); cdecl; begin end; exports counter_new, counter_add, counter_free; begin end.\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","pascal"]).arg(&source).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));assert!(!stderr.contains("broken.pas("));check_snapshot("bind_pascal_invalid_e3208.txt",&scrub(&stderr,&source));
}

#[test]
fn dart_bind_runs_jet_compute_and_dart_callback_in_process() {
    if Command::new("dart").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("dart_bind_round_trip");let contract=dir.join("callbacks.dart");let compute=dir.join("compute.jet");
    fs::write(&contract,"@pragma('vm:entry-point')\nint dartDouble(int value) => value * 2;\n").unwrap();
    fs::write(&compute,"use dart.callbacks as callbacks\n\npub fn compute(value: Int) =[Dart]=> Int {\n    return callbacks.dart_double(value) ?? -1\n}\n").unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","dart"]).arg(&contract).args(["--jet",compute.to_str().unwrap(),"--pkg","callbacks"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(bind.status.success(),"Dart bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    let cache=dir.join(".jet/bindings/dart");let native=cache.join(if cfg!(target_os="macos"){"libjet_dart_callbacks_compute.dylib"}else if cfg!(target_os="windows"){"libjet_dart_callbacks_compute.dll"}else{"libjet_dart_callbacks_compute.so"});
    assert!(cache.join("libjet_dart_callbacks.a").is_file());assert!(native.is_file());assert!(cache.join("callbacks_host.dart").is_file());assert!(cache.join("callbacks.provenance").is_file());
    let native_path=native.to_string_lossy().replace('\\',"\\\\").replace('\'',"\\'");
    fs::write(dir.join("host.dart"),format!("import 'dart:ffi';\nimport '.jet/bindings/dart/callbacks_host.dart';\ntypedef ComputeNative = Int64 Function(Int64);\ntypedef ComputeDart = int Function(int);\nvoid main() {{ initializeJetDart('{native_path}'); final compute = jetDartLibrary.lookupFunction<ComputeNative, ComputeDart>('compute'); print(compute(21)); shutdownJetDart(); }}\n")).unwrap();
    let run=Command::new("dart").args(["run","host.dart"]).current_dir(&dir).output().unwrap();assert!(run.status.success(),"Dart host failed:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n");
}

#[test]
fn dart_bind_rejects_untyped_contract_as_e3208() {
    let dir=isolated_cwd("dart_bind_invalid");let contract=dir.join("broken.dart");fs::write(&contract,"@pragma('vm:entry-point')\nString greet(String value) => value;\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","dart"]).arg(&contract).args(["--jet","compute.jet","--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));check_snapshot("bind_dart_invalid_e3208.txt",&scrub(&stderr,&contract));
}

#[test]
fn powershell_bind_round_trips_datatree_state_and_cleans_workers() {
    if Command::new("pwsh").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("powershell_bind_round_trip");let script=dir.join("ops.ps1");
    fs::write(&script,r#"$script:Counter = 0
function Get-Stateful {
  param($InputObject)
  $script:Counter += 1
  [ordered]@{
    count = $script:Counter
    nested = $InputObject.nested
    list = @($InputObject.list)
    scalar = $InputObject.scalar
    nothing = $null
  }
}
function Fail { param($InputObject) throw 'raw secret failure detail' }
function Sleep { param($InputObject) Start-Sleep -Seconds 30; return $InputObject }
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","pwsh"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"PowerShell bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/pwsh");assert!(cache.join("libjet_pwsh_ops.a").is_file());assert!(cache.join("ops_worker.ps1").is_file());assert!(cache.join("ops.provenance").is_file());
    fs::write(dir.join("main.jet"),r#"use pwsh.ops as ops
use core.encoding.json as json

fn run() =[PowerShell, IO]=> {
    session :: ops.open() ?? panic("PowerShell open failed")
    input :: DataTree.Object(["nested": DataTree.Object(["ok": DataTree.Bool(true)]), "list": DataTree.Array([DataTree.Int(1), DataTree.Text("two")]), "scalar": DataTree.Float(3.5), "nothing": DataTree.Null])
    first :: ops.get_stateful(session, ~input, 5000) ?? panic("first call failed")
    second :: ops.get_stateful(session, ~input, 5000) ?? panic("second call failed")
    print(json.canonical(first))
    print(json.canonical(second))
    failed :: ops.fail(session, DataTree.Null, 5000) ?? DataTree.Text("failed")
    print(json.canonical(failed))
    timed :: ops.sleep(session, DataTree.Int(1), 100) ?? DataTree.Text("timeout")
    print(json.canonical(timed))
    ops.close(^session)
}
"#).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated PowerShell binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),"{\"count\":1,\"list\":[1,\"two\"],\"nested\":{\"ok\":true},\"nothing\":null,\"scalar\":3.5}\n{\"count\":2,\"list\":[1,\"two\"],\"nested\":{\"ok\":true},\"nothing\":null,\"scalar\":3.5}\n\"failed\"\n\"timeout\"\n");
    fs::write(dir.join("cancel.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_pwsh_ops_open(void);
extern const char* jet_pwsh_ops_invoke_sleep(int64_t,const char*,int64_t);
extern void jet_pwsh_ops_cancel(int64_t);
extern void jet_pwsh_ops_close(int64_t);
extern int64_t jet_pwsh_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_pwsh_ops_invoke_sleep(handle,"null",60000);code=jet_pwsh_ops_take_error();return 0;}
int main(void){handle=jet_pwsh_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_pwsh_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_pwsh_ops_open();if(!fresh)return 4;jet_pwsh_ops_close(fresh);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("cancel.c").args(["-L.jet/bindings/pwsh","-l:libjet_pwsh_ops.a","-lpthread","-o","cancel"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"PowerShell cancellation probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let cancel=Command::new(dir.join("cancel")).current_dir(&dir).output().unwrap();assert!(cancel.status.success(),"PowerShell cancellation did not clean the worker: {:?}",cancel.status.code());
}

#[test]
fn powershell_bind_launders_parse_failure_as_e3208() {
    if Command::new("pwsh").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("powershell_bind_invalid");let script=dir.join("broken.ps1");fs::write(&script,"function Broken { param($InputObject) if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","pwsh"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("Unexpected token"));assert!(!stderr.contains("broken.ps1:"));check_snapshot("bind_powershell_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn perl_bind_round_trips_datatree_state_timeout_and_cancellation() {
    if Command::new("perl").arg("-v").output().is_err(){return}
    let dir=isolated_cwd("perl_bind_round_trip");let script=dir.join("ops.pl");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/perl");fs::copy(example.join("ops.pl"),&script).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","perl"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"Perl bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/perl");assert!(cache.join("libjet_perl_ops.a").is_file());assert!(cache.join("ops_worker.pl").is_file());assert!(cache.join("ops.provenance").is_file());
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated Perl binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("cancel.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_perl_ops_open(void);
extern const char* jet_perl_ops_invoke_sleep(int64_t,const char*,int64_t);
extern void jet_perl_ops_cancel(int64_t);
extern void jet_perl_ops_close(int64_t);
extern int64_t jet_perl_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_perl_ops_invoke_sleep(handle,"null",60000);code=jet_perl_ops_take_error();return 0;}
int main(void){handle=jet_perl_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_perl_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_perl_ops_open();if(!fresh)return 4;jet_perl_ops_close(fresh);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("cancel.c").args(["-L.jet/bindings/perl","-l:libjet_perl_ops.a","-lpthread","-o","cancel"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"Perl cancellation probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let cancel=Command::new(dir.join("cancel")).current_dir(&dir).output().unwrap();assert!(cancel.status.success(),"Perl cancellation did not clean the worker: {:?}",cancel.status.code());
}

#[test]
fn perl_bind_launders_parse_failure_as_e3208() {
    if Command::new("perl").arg("-v").output().is_err(){return}
    let dir=isolated_cwd("perl_bind_invalid");let script=dir.join("broken.pl");fs::write(&script,"sub Broken { if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","perl"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("syntax error at"));assert!(!stderr.contains("broken.pl line"));check_snapshot("bind_perl_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn ruby_bind_round_trips_datatree_state_timeout_and_cancellation() {
    if Command::new("ruby").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("ruby_bind_round_trip");let script=dir.join("ops.rb");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/ruby");fs::copy(example.join("ops.rb"),&script).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","ruby"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"Ruby bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/ruby");assert!(cache.join("libjet_ruby_ops.a").is_file());assert!(cache.join("ops_worker.rb").is_file());assert!(cache.join("ops.provenance").is_file());
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated Ruby binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("cancel.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_ruby_ops_open(void);
extern const char* jet_ruby_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern void jet_ruby_ops_cancel(int64_t);
extern void jet_ruby_ops_close(int64_t);
extern int64_t jet_ruby_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_ruby_ops_invoke_sleep_call(handle,"null",60000);code=jet_ruby_ops_take_error();return 0;}
int main(void){handle=jet_ruby_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_ruby_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_ruby_ops_open();if(!fresh)return 4;jet_ruby_ops_close(fresh);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("cancel.c").args(["-L.jet/bindings/ruby","-l:libjet_ruby_ops.a","-lpthread","-o","cancel"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"Ruby cancellation probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let cancel=Command::new(dir.join("cancel")).current_dir(&dir).output().unwrap();assert!(cancel.status.success(),"Ruby cancellation did not clean the worker: {:?}",cancel.status.code());
}

#[test]
fn ruby_bind_launders_parse_failure_as_e3208() {
    if Command::new("ruby").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("ruby_bind_invalid");let script=dir.join("broken.rb");fs::write(&script,"def broken(input)\n  if input\nend\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","ruby"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("syntax error"));assert!(!stderr.contains("broken.rb:"));check_snapshot("bind_ruby_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn php_bind_runs_a_persistent_bounded_worker_pool() {
    if Command::new("php").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("php_bind_pool");let script=dir.join("ops.php");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/php");fs::copy(example.join("ops.php"),&script).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","php"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"PHP bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/php");assert!(cache.join("libjet_php_ops.a").is_file());assert!(cache.join("ops_worker.php").is_file());let provenance=fs::read_to_string(cache.join("ops.provenance")).unwrap();assert!(provenance.contains("pool_workers=4"));
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated PHP binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("pool.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>
extern int64_t jet_php_ops_open(void);
extern const char* jet_php_ops_invoke_pooled_sleep(int64_t,const char*,int64_t);
extern const char* jet_php_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern const char* jet_php_ops_invoke_transform(int64_t,const char*,int64_t);
extern void jet_php_ops_cancel(int64_t);
extern void jet_php_ops_close(int64_t);
extern int64_t jet_php_ops_take_error(void);
static int64_t pool;static int64_t codes[4];
static void* parallel_call(void*arg){intptr_t i=(intptr_t)arg;jet_php_ops_invoke_pooled_sleep(pool,"null",5000);codes[i]=jet_php_ops_take_error();return 0;}
static void* cancel_call(void*unused){(void)unused;jet_php_ops_invoke_sleep_call(pool,"null",60000);codes[0]=jet_php_ops_take_error();return 0;}
static int64_t millis(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return (int64_t)t.tv_sec*1000+t.tv_nsec/1000000;}
int main(void){const char*valid="{\"nested\":{},\"list\":[],\"scalar\":1,\"nothing\":null}";pool=jet_php_ops_open();if(!pool)return 1;pthread_t threads[4];int64_t start=millis();for(intptr_t i=0;i<4;i++)if(pthread_create(&threads[i],0,parallel_call,(void*)i))return 2;for(int i=0;i<4;i++)pthread_join(threads[i],0);if(millis()-start>2500)return 3;for(int i=0;i<4;i++)if(codes[i])return 4;jet_php_ops_invoke_sleep_call(pool,"null",100);if(jet_php_ops_take_error()!=2)return 5;jet_php_ops_invoke_transform(pool,valid,5000);int64_t recovery=jet_php_ops_take_error();if(recovery)return 20+(int)recovery;pthread_t cancelled;if(pthread_create(&cancelled,0,cancel_call,0))return 7;usleep(100000);jet_php_ops_cancel(pool);pthread_join(cancelled,0);if(codes[0]!=3)return 8;for(int i=0;i<4;i++){jet_php_ops_invoke_transform(pool,valid,5000);int64_t code=jet_php_ops_take_error();if(code)return 30+(int)code;}jet_php_ops_close(pool);int64_t pools[8];for(int i=0;i<8;i++)if(!(pools[i]=jet_php_ops_open()))return 40+i;if(jet_php_ops_open()!=0||jet_php_ops_take_error()!=1)return 49;for(int i=0;i<8;i++)jet_php_ops_close(pools[i]);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("pool.c").args(["-L.jet/bindings/php","-l:libjet_php_ops.a","-lpthread","-o","pool"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"PHP pool probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let pool=Command::new(dir.join("pool")).current_dir(&dir).output().unwrap();assert!(pool.status.success(),"PHP worker-pool probe failed: {:?}",pool.status.code());
}

#[test]
fn php_bind_launders_parse_failure_as_e3208() {
    if Command::new("php").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("php_bind_invalid");let script=dir.join("broken.php");fs::write(&script,"<?php function broken($input) { if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","php"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("Parse error"));assert!(!stderr.contains("broken.php on line"));check_snapshot("bind_php_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[test]
fn r_bind_round_trips_datatree_state_and_worker_lifecycle() {
    if Command::new("Rscript").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("r_bind_round_trip");let script=dir.join("ops.R");
    let example=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/r");fs::copy(example.join("ops.R"),&script).unwrap();
    fs::OpenOptions::new().append(true).open(&script).unwrap().write_all(br#"
replace_plot <- function(value) {
  device <- dev.cur()
  dev.off(device)
  writeChar(value, file.path(Sys.getenv("JET_BIND_TEMP"), "plot.svg"), eos = NULL, useBytes = TRUE)
}
hostile_plot <- function(input) {
  kind <- input$kind
  value <- switch(kind,
    script = '<svg xmlns="http://www.w3.org/2000/svg"><script>raw secret script</script></svg>',
    event = '<svg xmlns="http://www.w3.org/2000/svg" onload="raw secret event"><path d="M0 0"/></svg>',
    foreign = '<svg xmlns="http://www.w3.org/2000/svg"><foreignObject>raw secret foreign</foreignObject></svg>',
    external = '<svg xmlns="http://www.w3.org/2000/svg"><use href="https://evil.invalid/raw-secret"/></svg>',
    css = '<svg xmlns="http://www.w3.org/2000/svg"><path style="fill:url(https://evil.invalid/raw-secret)"/></svg>',
    doctype = '<!DOCTYPE svg [<!ENTITY xxe SYSTEM "file:///raw-secret">]><svg xmlns="http://www.w3.org/2000/svg">&xxe;</svg>',
    malformed = '<svg xmlns="http://www.w3.org/2000/svg"><path></svg>',
    oversize = paste0('<svg xmlns="http://www.w3.org/2000/svg"><desc>', strrep('x', 524288), '</desc></svg>'),
    stop('unknown hostile plot'))
  replace_plot(value)
}
"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","r"]).arg(&script).args(["--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"R bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));let cache=dir.join(".jet/bindings/r");assert!(cache.join("libjet_r_ops.a").is_file());assert!(cache.join("ops_worker.R").is_file());let provenance=fs::read_to_string(cache.join("ops.provenance")).unwrap();assert!(provenance.contains("workers_per_session=1\nmax_sessions=32\ntransport=jsonlite\n"));assert!(!provenance.to_ascii_lowercase().contains("cran"));
    fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"generated R binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::write(dir.join("lifecycle.c"),r#"#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
extern int64_t jet_r_ops_open(void);
extern const char* jet_r_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_sleep_call_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_plot_scores_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_hostile_plot_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_transform(int64_t,const char*,int64_t);
extern void jet_r_ops_cancel(int64_t);
extern void jet_r_ops_close(int64_t);
extern int64_t jet_r_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_r_ops_invoke_sleep_call_plot(handle,"1",60000);code=jet_r_ops_take_error();return 0;}
static int hostile(int64_t h,const char*kind){char input[64];snprintf(input,sizeof(input),"{\"kind\":\"%s\"}",kind);const char*response=jet_r_ops_invoke_hostile_plot_plot(h,input,5000);if(jet_r_ops_take_error()!=0||!response||!strstr(response,"\"ok\":false")||strstr(response,"secret"))return 1;return 0;}
int main(void){handle=jet_r_ops_open();if(!handle)return 1;const char*svg=jet_r_ops_invoke_plot_scores_plot(handle,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true")||!strstr(svg,"<svg height=\\\"")||strstr(svg,"<?xml")||strstr(svg,"<script"))return 2;const char*kinds[]={"script","event","foreign","external","css","doctype","malformed","oversize"};for(int i=0;i<8;i++)if(hostile(handle,kinds[i]))return 10+i;const char*recovered=jet_r_ops_invoke_transform(handle,"{\"nested\":{},\"vector\":[1,2],\"scalar\":1,\"nothing\":null}",5000);if(jet_r_ops_take_error()!=0||!recovered||!strstr(recovered,"\"ok\":true"))return 20;jet_r_ops_invoke_sleep_call_plot(handle,"1",100);if(jet_r_ops_take_error()!=2)return 21;int64_t timed=jet_r_ops_open();if(!timed)return 22;svg=jet_r_ops_invoke_plot_scores_plot(timed,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true"))return 23;jet_r_ops_close(timed);handle=jet_r_ops_open();if(!handle)return 24;pthread_t thread;if(pthread_create(&thread,0,call,0))return 25;usleep(100000);jet_r_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 26;int64_t fresh=jet_r_ops_open();if(!fresh)return 27;svg=jet_r_ops_invoke_plot_scores_plot(fresh,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true"))return 28;jet_r_ops_close(fresh);int64_t sessions[32];for(int i=0;i<32;i++)if(!(sessions[i]=jet_r_ops_open()))return 40+i;if(jet_r_ops_open()!=0||jet_r_ops_take_error()!=1)return 72;for(int i=0;i<32;i++)jet_r_ops_close(sessions[i]);return 0;}
"#).unwrap();
    let cc=Command::new("cc").arg("lifecycle.c").args(["-L.jet/bindings/r","-l:libjet_r_ops.a","-lpthread","-o","lifecycle"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"R lifecycle probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let lifecycle=Command::new(dir.join("lifecycle")).current_dir(&dir).output().unwrap();assert!(lifecycle.status.success(),"R lifecycle probe failed: {:?}",lifecycle.status.code());
}

#[test]
fn r_bind_discovers_functions_without_executing_source() {
    if Command::new("Rscript").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("r_bind_static_discovery");let script=dir.join("static.R");fs::write(&script,r#"stop("discovery executed source")
# fake <- function(input) input
text <- "also_fake <- function(input) input"
outer <- function(input) {
  nested <- function(input) input
  input
}

"#).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","r"]).arg(&script).args(["--pkg","static_ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"static R discovery failed:\n{}",String::from_utf8_lossy(&bind.stderr));let generated=fs::read_to_string(dir.join(".jet/bindings/r/static_ops.jet")).unwrap();assert!(generated.contains("pub fn outer("));assert!(!generated.contains("pub fn fake("));assert!(!generated.contains("pub fn also_fake("));assert!(!generated.contains("pub fn nested("));
}

#[test]
fn r_bind_launders_parse_failure_as_e3208() {
    if Command::new("Rscript").arg("--version").output().is_err(){return}
    let dir=isolated_cwd("r_bind_invalid");let script=dir.join("broken.R");fs::write(&script,"broken <- function(input) { if ( }\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","r"]).arg(&script).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);assert!(stderr.contains("Error [E3208]:"));assert!(!stderr.contains("unexpected '}'"));assert!(!stderr.contains("broken.R:"));check_snapshot("bind_r_invalid_e3208.txt",&scrub(&stderr,&script));
}

#[cfg(not(target_os="windows"))]
#[test]
fn com_bind_rejects_non_windows_before_reading_input() {
    let output=Command::new(jet()).args(["inspect","bind","com","missing.tlb","--pkg","excel"]).env("NO_COLOR","1").output().unwrap();assert_eq!(output.status.code(),Some(1));assert!(output.stdout.is_empty());check_snapshot("bind_com_non_windows_e3260.txt",&String::from_utf8_lossy(&output.stderr));
}

#[test]
fn ada_bind_launders_gnat_failure_as_e3208() {
    let dir=isolated_cwd("ada_bind_failure");let spec=dir.join("broken.ads");
    fs::write(&spec,"package Broken is function Value (N : Long_Long_Integer) return Long_Long_Integer with Export, Convention => C, External_Name => \"broken_value\"; end Broken;\n").unwrap();
    fs::write(dir.join("broken.adb"),"package body Broken is function Value (N : Long_Long_Integer) return Long_Long_Integer is begin return N +; end Value; end Broken;\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","ada"]).arg(&spec).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));assert!(!stderr.contains("broken.adb:"));
    check_snapshot("bind_ada_invalid_e3208.txt",&scrub(&stderr,&spec));
}

#[test]
fn tcl_bind_missing_source_is_laundered_e3208() {
    let dir=isolated_cwd("tcl_bind_missing");let source=dir.join("missing.tcl");
    let output=Command::new(jet()).args(["inspect","bind","tcl"]).arg(&source).args(["--pkg","missing"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success());let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:"));assert!(stderr.contains(" Why:"));assert!(stderr.contains(" Fix:"));
    check_snapshot("bind_tcl_missing_e3208.txt",&scrub(&stderr,&source));
}

#[test]
fn fortran_bind_launders_foreign_compiler_failure_as_e3208() {
    let dir = isolated_cwd("fortran_bind_failure");
    let source = dir.join("broken.f90");
    fs::write(
        &source,
        r#"module broken_math
  use iso_c_binding
contains
  function broken(a) result(value) bind(C, name="broken")
    integer(c_int64_t), value :: a
    integer(c_int64_t) :: value
    value = a +
  end function broken
end module broken_math
"#,
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["inspect", "bind", "fortran"])
        .arg(&source)
        .args(["--pkg", "broken"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error [E3208]:"),
        "missing Jet diagnostic:\n{stderr}"
    );
    assert!(stderr.contains(" Why:"), "missing reason:\n{stderr}");
    assert!(stderr.contains(" Fix:"), "missing fix:\n{stderr}");
    assert!(
        !stderr.contains("broken.f90:"),
        "raw gfortran location leaked:\n{stderr}"
    );
    assert!(
        !stderr.contains("    7 |"),
        "raw gfortran source frame leaked:\n{stderr}"
    );
    check_snapshot(
        "bind_fortran_invalid_e3208.txt",
        &scrub(&stderr, &source),
    );
}

#[test]
fn cobol_bind_launders_foreign_compiler_failure_as_e3208() {
    if Command::new("cobc").arg("--version").output().is_err() { return; }
    let dir=isolated_cwd("cobol_bind_failure"); let source=dir.join("broken.cob"); let copybook=dir.join("record.cpy");
    fs::write(&source,"       IDENTIFICATION DIVISION.\n       PROGRAM-ID. BROKEN.\n       THIS IS NOT COBOL.\n").unwrap();
    fs::write(&copybook,"       01 RECORD.\n          05 AMOUNT PIC S9(7)V99 COMP-3.\n").unwrap();
    let output=Command::new(jet()).args(["inspect","bind","cobol"]).arg(&source).args(["--copybook"]).arg(&copybook).args(["--pkg","broken"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();
    assert!(!output.status.success()); let stderr=String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3208]:")); assert!(stderr.contains(" Why:")); assert!(stderr.contains(" Fix:"));
    assert!(!stderr.contains("broken.cob:"),"raw cobc location leaked:\n{stderr}");
    check_snapshot("bind_cobol_invalid_e3208.txt",&scrub(&stderr,&source));
}

#[test]
fn unknown_cross_target_is_e3302() {
    let src = std::env::temp_dir().join("jet_unknown_cross_target.jet");
    fs::write(&src, "fn run() { print(\"target\") }\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg(&src)
        .arg("--target=definitely-not-a-rust-target")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E3302]:"), "missing target diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E3302 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E3302 fix:\n{stderr}");
    check_snapshot("unknown_target_e3302.txt", &stderr);
}

#[test]
fn prove_unknown_lens_is_e2941() {
    let root = std::env::temp_dir().join("jet_cli_prove_unknown_lens");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("plain.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .current_dir(&root)
        .args(["prove", "plain.jet", "--lens", "test"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E2941]:"), "missing lens diagnostic:\n{stderr}");
    assert!(stderr.contains("Why:"), "missing E2941 reason:\n{stderr}");
    assert!(stderr.contains("Fix:"), "missing E2941 fix:\n{stderr}");
    check_snapshot("prove_unknown_lens_e2941.txt", &stderr);
}

#[test]
fn completions_generate_for_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = Command::new(jet())
            .args(["self", "completions"])
            .arg(shell)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "completions {} should exit 0",
            shell
        );
        let s = String::from_utf8_lossy(&out.stdout);
        for flag in ["structural", "out", "report", "repo"] {
            let spelling = if shell == "fish" {
                format!("-l {flag}")
            } else {
                format!("--{flag}")
            };
            assert!(
                s.contains(&spelling),
                "{shell} completion missing {spelling}"
            );
        }
        check_snapshot(&format!("completions_{}.txt", shell), &s);
    }
}

#[test]
fn man_page_golden() {
    let out = Command::new(jet()).args(["self", "man"]).output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    // Scrub the version so the snapshot is stable across releases.
    s = s.replace(env!("CARGO_PKG_VERSION"), "VERSION");
    for flag in ["--structural", "--out", "--report", "--repo"] {
        assert!(s.contains(flag), "man page missing {flag}");
    }
    check_snapshot("man.txt", &s);
}

#[test]
fn retired_emit_rust_flag_teaches_canonical_command() {
    let out = Command::new(jet())
        .args(["run", "examples/features/basics/hello.jet", "--emit-rust"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "unexpected stderr:\n{stderr}");
    assert!(stderr.contains("Error [E2102]: `--emit-rust` isn't a flag"));
    assert!(stderr.contains("Fix: run `jet emit --rust <file.jet>`"));
}

#[test]
fn fix_dry_run_does_not_write() {
    // A file with an autofixable diagnostic. S14 teaching fixes are paused, so
    // use the still-live Core habit fix (`println` -> `print`).
    let p = std::env::temp_dir().join("jet_cli_fix.jet");
    let original = "fn run() {\n    println(\"hi\")\n}\n";
    fs::write(&p, original).unwrap();
    let out = Command::new(jet())
        .arg("fix")
        .arg(&p)
        .arg("--dry-run")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("dry run"), "dry-run should say so:\n{}", s);
    assert!(s.contains("print"), "diff should show the fix:\n{}", s);
    // The file on disk is unchanged.
    assert_eq!(
        fs::read_to_string(&p).unwrap(),
        original,
        "dry-run must not write"
    );

    // And a real fix DOES write.
    let out2 = Command::new(jet()).arg("fix").arg(&p).output().unwrap();
    assert_eq!(out2.status.code(), Some(0));
    assert!(
        fs::read_to_string(&p).unwrap().contains("print(\"hi\")"),
        "fix should rewrite the file"
    );
}

#[test]
fn external_subcommand_is_discovered() {
    // A fake `jet-greet` on a temp PATH should be invokable as `jet greet`.
    let dir = std::env::temp_dir().join("jet_ext_test_bin");
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("jet-greet");
    fs::write(&script, "#!/bin/sh\necho \"hi from plugin $1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&script, perm).unwrap();
    }
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(jet())
        .arg("greet")
        .arg("world")
        .env("PATH", path)
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("hi from plugin world"),
        "external subcommand not forwarded:\n{}",
        s
    );
}

#[test]
fn osc8_hyperlinks_only_when_forced_on() {
    let p = bad_file(&line!().to_string());
    // Piped + NO_COLOR: never an OSC 8 link (existing snapshots stay clean).
    let piped = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains("\x1b]8;;"),
        "piped output must have no OSC 8 links:\n{:?}",
        s
    );
    // The hyperlink layer is gated behind a real TTY; since tests run piped,
    // we exercise the renderer directly to prove the escape appears when asked.
    let src = "fn run() {}\n";
    let d = jet::Diagnostics::Diagnostic::error(
        "E0001",
        "x".into(),
        "y".into(),
        "z".into(),
        Some(jet::Diagnostics::Span::new(3, 7)),
    );
    let linked = d.render_linked("a.jet", src, true, true);
    assert!(
        linked.contains("\x1b]8;;"),
        "render_linked(hyperlinks=true) should emit OSC 8"
    );
    let plain = d.render_linked("a.jet", src, true, false);
    assert!(
        !plain.contains("\x1b]8;;"),
        "render_linked(hyperlinks=false) must not"
    );
}

// ── Ext-optional CLI (no syntax decision; pure CLI behavior) ──────────

#[test]
fn ext_optional_check_resolves_dot_jet() {
    // `jet check <path-without-.jet>` resolves to `<path>.jet` when the bare
    // path does not exist but the .jet file does.
    let stem = std::env::temp_dir().join("jet_cli_extopt_check");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"ok\");\n}\n").unwrap();
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional check should resolve {}.jet and exit 0; stderr: {}",
        stem.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_rejected_collection_body_reports_only_frontend_diagnostics() {
    let dir = isolated_cwd("run_rejected_collection_body");
    fs::write(
        dir.join("main.jet"),
        r#"use core.files as fs

struct Row {
    name: String
    count: Int
}

fn run() {
    fs.write("/tmp/jet_1271.csv", "alpha,1\n") ?? panic("write failed")
    text :: fs.read("/tmp/jet_1271.csv") ?? ""
    rows := [Row].{}
    loop line, text.split("\n") {
        parts :: line.split(",")
        rows.push(Row.{ name: parts.get(0), count: missing })
    }
    rows.sort_by((row: Row) => row.name)
}
"#,
    )
    .unwrap();

    let rejected = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stdout.is_empty(), "{:?}", rejected.stdout);
    let stderr = String::from_utf8(rejected.stderr).unwrap();
    assert_eq!(
        stderr,
        concat!(
            "Error [E0102]: `Iter` has no method `get`\n",
            "  --> main.jet:14:37\n",
            "    |\n",
            " 14 |         rows.push(Row.{ name: parts.get(0), count: missing })\n",
            "    |                                     ^^^\n",
            " Why: check the method name on this type\n",
            " Fix: call `.to_list()` first\n",
            "\n",
            "Error [E0107]: nothing named `missing` exists here\n",
            "  --> main.jet:14:52\n",
            "    |\n",
            " 14 |         rows.push(Row.{ name: parts.get(0), count: missing })\n",
            "    |                                                    ^^^^^^^\n",
            " Why: a name must be declared before it's used\n",
            " Fix: declare it first: `missing :: ...`\n",
            "\n",
            "2 problems found\n",
            "run `jet explain E0102` to learn more\n",
        )
    );
}

#[test]
fn check_fixed_dynamic_size_reports_e0103_without_internal_failure() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let invalid = root.join("tests/fuzz/sema/invalid/ui_fixed_dynamic_size.E0103.jet");
    let rejected = Command::new(jet())
        .args(["check", invalid.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("Error [E0103]"), "{stderr}");
    for leaked in [
        "panicked at",
        "entered unreachable code",
        "internal error",
        "generated Rust",
    ] {
        assert!(!stderr.contains(leaked), "`{leaked}` leaked:\n{stderr}");
    }

    let dir = isolated_cwd("check_fixed_comptime_size");
    fs::write(
        dir.join("mixed.jet"),
        "use core.mem\nfn fixed_size() => Int { return 32 }\nfn bad(size: Int) {\n fixed :: mem.Fixed.new(size: size)\n close(^fixed)\n}\nfn run() {\n fixed :: mem.Fixed.new(size: fixed_size())\n close(^fixed)\n}\n",
    )
    .unwrap();
    let mixed = Command::new(jet())
        .args(["check", "mixed.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(mixed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&mixed.stderr);
    assert!(stderr.contains("Error [E0103]"), "{stderr}");
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("internal error"),
        "{stderr}"
    );

    fs::write(
        dir.join("compare_chain.jet"),
        "fn helper() => Int { return 1 }\nfn run() {\n #Known if 0 < helper() < 2 {\n  print(\"reachable\")\n }\n}\n",
    )
    .unwrap();
    let compare_chain = Command::new(jet())
        .args(["check", "compare_chain.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        compare_chain.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&compare_chain.stderr)
    );

    fs::write(
        dir.join("higher_order_valid.jet"),
        "use core.mem\nfn apply(f: fn() => Int) => Int { return f() }\nfn fixed_size() => Int { return 32 }\nfn run() {\n fixed :: mem.Fixed.new(size: apply(fixed_size))\n close(^fixed)\n}\n",
    )
    .unwrap();
    let higher_order_valid = Command::new(jet())
        .args(["check", "higher_order_valid.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        higher_order_valid.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&higher_order_valid.stderr)
    );

    fs::write(
        dir.join("higher_order_isolation.jet"),
        "use core.mem\nfn apply(f: fn() => Int) => Int { return f() }\nfn fixed_size() => Int { return 32 }\nfn bad(size: Int) {\n fixed :: mem.Fixed.new(size: size)\n close(^fixed)\n}\nfn run() {\n fixed :: mem.Fixed.new(size: apply(fixed_size))\n close(^fixed)\n}\n",
    )
    .unwrap();
    let higher_order_isolation = Command::new(jet())
        .args(["check", "higher_order_isolation.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(higher_order_isolation.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&higher_order_isolation.stderr);
    assert_eq!(stderr.matches("Error [E0103]").count(), 1, "{stderr}");
    for leaked in ["panicked at", "entered unreachable code", "internal error"] {
        assert!(!stderr.contains(leaked), "`{leaked}` leaked:\n{stderr}");
    }

    fs::write(
        dir.join("lambda_value.jet"),
        "fn run() {\n #Known callback :: () => print(\"not called\")\n print(\"ok\")\n}\n",
    )
    .unwrap();
    let lambda_value = Command::new(jet())
        .args(["check", "lambda_value.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        lambda_value.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&lambda_value.stderr)
    );

    fs::write(
        dir.join("helper.jet"),
        "use core.mem\nfn fixed_size() => Int { return 32 }\nfn run() {\n fixed :: mem.Fixed.new(size: fixed_size())\n close(^fixed)\n}\n",
    )
    .unwrap();
    let helper = Command::new(jet())
        .args(["check", "helper.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(helper.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&helper.stderr);
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("internal error"),
        "{stderr}"
    );

    fs::write(
        dir.join("main.jet"),
        "use core.mem\nfn run() {\n fixed :: mem.Fixed.new(size: 16 + 16)\n close(^fixed)\n}\n",
    )
    .unwrap();
    let accepted = Command::new(jet())
        .args(["check", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        accepted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let stderr = String::from_utf8_lossy(&accepted.stderr);
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("internal error"),
        "{stderr}"
    );
}

#[test]
fn check_reports_soft_public_lints_without_failing() {
    let dir = isolated_cwd("check_soft_public");
    fs::write(
        dir.join("library.jet"),
        "pub fn _legacy() => Int { return 1 }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use \"library\"\nfn run() { print(library._legacy()) }\n",
    )
    .unwrap();
    let output = Command::new(jet())
        .args(["check", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("[L0601]").count(), 1, "{stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("has no problems"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn ext_optional_run_resolves_dot_jet() {
    // Same resolution for `jet run`.
    let stem = std::env::temp_dir().join("jet_cli_extopt_run");
    let file = stem.with_extension("jet");
    fs::write(&file, "fn run() {\n    print(\"hello-extopt\");\n}\n").unwrap();
    let out = Command::new(jet()).arg("run").arg(&stem).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "ext-optional run should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello-extopt"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn ext_optional_missing_path_keeps_original_name() {
    // Neither `<path>` nor `<path>.jet` exists: the original name must surface
    // in the file-not-found error (resolution returns it unchanged).
    let stem = std::env::temp_dir().join("jet_cli_extopt_absent_xyz");
    let out = Command::new(jet())
        .arg("check")
        .arg(&stem)
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("jet_cli_extopt_absent_xyz"),
        "error should name the original path; stderr: {err}"
    );
}

// ── D-ILE1: implicit executable inference (no pkg.jet) ───────────────

#[test]
fn simple_exec_runs_without_a_manifest() {
    // A single file with a top-level `fn run` and no pkg.jet runs as an
    // executable with zero ceremony (R9 / D-ILE1).
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_exec/main.jet");
    // Isolated cwd: this fixture's stem is `main`, a common stem other tests
    // and examples also use — see `isolated_cwd`.
    let out = Command::new(jet())
        .arg("run")
        .arg(&path)
        .current_dir(isolated_cwd("simple_exec"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("simple exec, no manifest"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ── D-CLI1: `--` separator passthrough (c11) ──────────────────────────────

/// Write a Jet fixture that prints its argument count via `io.args()`.
fn args_fixture(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("jet_cli_args_{tag}.jet"));
    fs::write(
        &p,
        "use core.io as io\nfn run() {\n    args :: io.args()\n    print(args.len())\n}\n",
    )
    .unwrap();
    p
}

#[test]
fn passthrough_forwards_tokens_after_separator() {
    // `jet run file.jet -- --port 8080 x` — program sees 4 args: argv[0] +
    // three forwarded tokens. io.args().len() == 4.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", "--release", p.to_str().unwrap(), "--", "--port", "8080", "x"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "4",
        "expected 4 args (argv[0] + 3 forwarded), got: {stdout}"
    );
}

#[test]
fn bare_separator_gives_empty_passthrough() {
    // `jet run file.jet --` — bare `--` with nothing after; program sees 1 arg.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", "--release", p.to_str().unwrap(), "--"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "1",
        "expected 1 arg (just argv[0]), got: {stdout}"
    );
}

#[test]
fn no_separator_positional_regression() {
    // Plain positional words with no `--` still reach the program (regression
    // guard). `jet run file.jet hello` → len == 2.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", "--release", p.to_str().unwrap(), "hello"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "2",
        "expected 2 args (argv[0] + hello), got: {stdout}"
    );
}

#[test]
fn unknown_flag_before_separator_is_e2102_with_passthrough_hint() {
    // `jet run file.jet --port` (no `--`) — unknown flag before `--` is E2102
    // and the Fix line teaches the `--` form (D-CLI1=A).
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--port"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown flag before -- should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{stderr}");
    assert!(
        stderr.contains("--"),
        "Fix should mention `--` separator:\n{stderr}"
    );
}

// ── D-BUILDPROFILE1: --release / --profile=<name> ─────────────────────────────

#[test]
fn profile_unknown_name_emits_e1219() {
    // D-BUILDPROFILE1: `--profile=<unknown>` with no pkg.jet defining that name
    // must emit E1219 and exit 1 (user error).
    let p = std::env::temp_dir().join("jet_cli_profile_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=staging"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown --profile should exit 1 (user error)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1219"),
        "unknown profile should cite E1219:\n{stderr}"
    );
    assert!(
        stderr.contains("staging"),
        "E1219 should name the unknown profile:\n{stderr}"
    );
}

#[test]
fn profile_release_flag_is_accepted() {
    // `--release` is valid (blessed profile) and must not emit E1219.
    let p = std::env::temp_dir().join("jet_cli_release_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    // We can't guarantee rustc is in PATH for the binary build, but `jet check`
    // doesn't accept --release yet, so test that `jet build --release` at least
    // doesn't emit E1219. We check that the exit code is NOT 1-with-E1219.
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--release"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--release must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_ci_flag_is_accepted() {
    let p = std::env::temp_dir().join("jet_cli_ci_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=ci"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--profile=ci must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_custom_name_from_pkg_jet() {
    let dir = std::env::temp_dir().join(format!(
        "jet_cli_custom_profile_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        r#"payload: { name: "p", version: "0.1.0" }
build: { staging: Build.{ optimize: basic } }
"#,
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(&main, "fn run() { print(\"ok\") }\n").unwrap();
    // Isolated cwd: this fixture's stem is `main` — see `isolated_cwd`. Also
    // the semantically correct place for `build/` to land, since it's this
    // fixture's own project directory.
    let out = Command::new(jet())
        .args(["build", main.to_str().unwrap(), "--profile=staging"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "pkg.jet-defined profile must resolve:\n{stderr}"
    );
}

// ── D-EXPANDCLI1 (card #183): `jet inspect expand` transparency command ────

/// Fixture exercising the `inline` lens: an `#Inline` fn and an
/// `#Inline(Always)` method.
fn expand_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/expand_facts.jet")
}

/// Replace the fixture's machine-specific absolute path with a stable token.
fn scrub_fixture(s: &str, fixture: &Path) -> String {
    s.replace(&fixture.display().to_string(), "FIXTURE.jet")
}

#[test]
fn expand_inline_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "inline"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "expand --facts inline should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_inline.txt", &s);
}

#[test]
fn expand_all_golden() {
    let p = expand_fixture();
    // Bare `jet inspect expand <file>`: every lens, grouped, magic default.
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare expand should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_all.txt", &s);
}

#[test]
fn expand_unknown_lens_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "bogus"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown lens should exit 1 (USER_ERROR), listing available lenses"
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("expand_unknown_lens.txt", &s);
}

#[test]
fn expand_missing_file_is_user_error() {
    let out = Command::new(jet()).args(["inspect", "expand"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "missing entry file is USER_ERROR"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs an entry file"),
        "should explain the missing file:\n{}",
        stderr
    );
}

#[test]
fn expand_compile_error_reports_ordinary_diagnostics() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "a program that fails to compile can't print facts (USER_ERROR)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0102"),
        "should render the ordinary front-end diagnostic:\n{}",
        stderr
    );
}

// ── D-JPK-FILENAME2=B (A2): retired manifest filenames → E1226 ──────

#[test]
fn stale_manifest_name_pack_jet_is_e1226() {
    let dir = isolated_cwd("stale_pack_jet");
    fs::write(
        dir.join("pack.jet"),
        "payload: { name: \"x\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("add")
        .arg("dep")
        .arg("--path")
        .arg("../dep")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("pack.jet"),
        "names the found file:\n{stderr}"
    );
    assert!(
        stderr.contains("pkg.jet"),
        "names the fix target:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_jet_toml_is_e1226() {
    let dir = isolated_cwd("stale_jet_toml");
    fs::write(dir.join("jet.toml"), "").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("jet.toml"),
        "names the found file:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_payload_jet_is_e1226() {
    let dir = isolated_cwd("stale_payload_jet");
    fs::write(dir.join("payload.jet"), "").unwrap();
    let out = Command::new(jet())
        .args(["inspect", "schema"])
        .arg("status")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("payload.jet"),
        "names the found file:\n{stderr}"
    );
}

/// `jetpack.toml` is a different, still-live file (D-JPK-FILES repo
/// metadata) — it must NOT be mistaken for a retired manifest name.
#[test]
fn jetpack_toml_alone_is_not_e1226() {
    let dir = isolated_cwd("jetpacktoml_not_stale");
    fs::write(dir.join("jetpack.toml"), "[repo]\nname = \"x\"\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1226"),
        "jetpack.toml is a different live file, not a retired manifest name:\n{stderr}"
    );
    assert!(
        stderr.contains("no file given and no `pkg.jet` found") || stderr.contains("E1225"),
        "should fall back to the generic no-manifest message:\n{stderr}"
    );
}

/// D-PLUGIN1=B (c81): a `target: plugin` package is deny-by-default — its own
/// code using any effect (here `core.env`) must fail cleanly at build time
/// (E1258), not defer to a runtime instantiation failure. This check lives in
/// the CLI's post-compile effect-budget pass (`Source/CmdCompile.rs`), so it
/// needs the real subprocess (not the `jet::compile_plugin` library call the
/// `tests/ui` `@plugin_target` harness drives).
#[test]
fn plugin_using_an_effect_is_e1258() {
    let dir = isolated_cwd("plugin_effect_denied");
    fs::write(
        dir.join("main.jet"),
        "use core.env as env\n\npub fn get_secret() => Int {\n    _ :: env.get(\"SECRET\")\n    return 1\n}\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1258"),
        "expected E1258 (plugin capability-denied) in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Env"),
        "should name the offending effect:\n{stderr}"
    );
}

/// D-DEP-WASM1=A (c81): `jet build --target=plugin` shells out to
/// `wasm-tools` to lift the rustc-built core wasm module into a Component. A
/// PATH without `wasm-tools` on it (but with `rustc` still reachable, so the
/// core-module half of the build succeeds) must fail as a clean E1259, never
/// a raw "No such file or directory" panic (I2).
#[test]
fn plugin_missing_wasm_tools_is_e1259() {
    let which = |tool: &str| -> Option<String> {
        Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let (Some(rustc_path), Some(lld_path)) = (which("rustc"), which("lld")) else {
        eprintln!("note: skipping plugin_missing_wasm_tools_is_e1259 (no `rustc`/`lld` on PATH to re-expose)");
        return;
    };

    let dir = isolated_cwd("plugin_no_wasmtools");
    fs::write(
        dir.join("main.jet"),
        "pub fn scale(a: Float, b: Float) => Float {\n    return a * b\n}\n",
    )
    .unwrap();

    // A minimal PATH exposing only `rustc` + `lld` (via symlinks), so the
    // core-wasm-module half of the build still works but `wasm-tools`
    // resolves to nothing.
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink(&rustc_path, bin_dir.join("rustc"));
        let _ = symlink(&lld_path, bin_dir.join("lld"));
    }

    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("PATH", &bin_dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1259"),
        "expected E1259 (missing wasm-tools) in stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "must never panic, only report a clean diagnostic (I2):\n{stderr}"
    );
}

// ── D-ILE1 / D-CLI-BARE1: bare entry resolution (card #497 verifier bounce) ──
//
// `resolve_bare_entry` (Source/main.rs) delegated to a `find_project_entry`
// that only ever checked `main.jet`/`.jet/main.jet`, never the package-named
// D-ILE1 fallback (`<package>.jet`, using `pkg.jet`'s `payload.name`). The shipped
// `examples/features/packages/monorepo` fixture (members `hello.jet` /
// `ranker.jet`, neither named `main.jet`) exposed it end to end: bare `jet
// run` at the workspace root couldn't see either member as runnable, `-p
// hello` said "no workspace member named `hello`", and `cd`-ing into a
// member and running bare failed too.

/// Recursively copy a directory tree — sandboxes the shipped monorepo
/// fixture into an isolated cwd so `jet run`'s `build/` output never lands in
/// the checked-in example and concurrent test runs never collide.
fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let dest_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_all(&entry.path(), &dest_path);
        } else {
            fs::copy(entry.path(), &dest_path).unwrap();
        }
    }
}

#[test]
fn monorepo_bare_entry_honors_d_ile1_search_order() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/monorepo");
    let root = isolated_cwd("monorepo_d_ile1");
    fs::remove_dir_all(&root).ok();
    copy_dir_all(&fixture, &root);

    let run = |dir: &Path, extra_args: &[&str]| -> std::process::Output {
        Command::new(jet())
            .arg("run")
            .args(extra_args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };

    // 1. Bare `jet run` at the workspace root: both members resolve via
    //    D-ILE1 (`<package>.jet`, since neither has `run.jet`/`src/run.jet`), so the
    //    result is the D-CLI-BARE1 ambiguity error naming both.
    let out = run(&root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "ambiguous bare run is USAGE:\n{stderr}"
    );
    assert!(
        stderr.contains("ambiguous"),
        "expected the D-CLI-BARE1 ambiguity error:\n{stderr}"
    );
    assert!(
        stderr.contains("hello") && stderr.contains("ranker"),
        "ambiguity error should list both runnable members by their real pkg.jet name:\n{stderr}"
    );
    assert!(
        !stderr.contains("hello\"") && !stderr.contains("ranker\""),
        "member names must not carry a stray trailing quote:\n{stderr}"
    );

    // 2. `-p hello` picks the member unambiguously and actually runs it.
    let out = run(&root, &["-p", "hello"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "-p hello should run: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello from the monorepo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 3. `-p ranker` likewise.
    let out = run(&root, &["-p", "ranker"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "-p ranker should run: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ranker: #1 monorepo demo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 4. `cd packages/hello && jet run` (bare, single-package convention):
    //    the member directory's own `pkg.jet` names it `hello`, so D-ILE1
    //    resolves `hello.jet` directly — no workspace ambiguity from inside.
    let member_dir = root.join("packages/hello");
    let out = run(&member_dir, &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare run inside a member should run its own entry: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello from the monorepo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 5. Outside any package or workspace, the bare-form usage error is
    //    unchanged (D-CLI-BARE1: "outside a package the bare form stays the
    //    current usage error").
    let outside = isolated_cwd("monorepo_d_ile1_outside");
    let out = run(&outside, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr.contains("no file given and no `pkg.jet` found"),
        "outside-package bare error text must stay the current usage error:\n{stderr}"
    );
}

fn scene_probe_project(tag: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    fs::write(dir.join("src/main.jet"), r#"use core.game as game

module perf.game {
    budgets: [
        Budget.{
            name: "frame",
            scope: .Scene("main"),
            metric: .FrameTime(.P99),
            provider: .SceneProbe("main"),
            comparison: .AbsoluteFrom("local/main"),
            limit: .AtMost(16ms),
            enforcement: .Warn,
        },
        Budget.{
            name: "memory",
            scope: .Scene("main"),
            metric: .MemoryHighWater,
            provider: .SceneProbe("main"),
            comparison: .AbsoluteFrom("local/main"),
            limit: .AtMost(256MiB),
            enforcement: .Warn,
        },
        Budget.{
            name: "assets",
            scope: .Scene("main"),
            metric: .SceneAssetBytes,
            provider: .SceneProbe("main"),
            comparison: .AbsoluteFrom("local/main"),
            limit: .AtMost(1MiB),
            enforcement: .Warn,
        },
        Budget.{
            name: "draws",
            scope: .Scene("main"),
            metric: .DrawCalls(.P99),
            provider: .SceneProbe("main"),
            comparison: .AbsoluteFrom("local/main"),
            limit: .AtMost(10),
            enforcement: .Warn,
        },
    ]
}

fn run() {
    scene := game.Scene.new("main")
    counter := 0
    scene.on_frame((frame) => {
        counter = counter + 1
    })
    transcript :: game.run(scene)
    print(transcript)
}
"#).unwrap();
    dir
}

#[test]
fn scene_probe_produces_real_frame_time_samples_and_rejects_forged_cache() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = scene_probe_project("scene_probe_runtime");
    let run = || Command::new(jet())
        .args(["self", "devtools", "probe", "src/main.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();

    let first = run();
    assert_eq!(
        first.status.code(), Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let reports = dir.join(".jet/perf/reports");
    let paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = paths();
    assert_eq!(initial.len(), 1, "expected exactly one report; got {:?}", initial);
    let bytes = fs::read(&initial[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 4, "expected 4 SceneProbe measurements");
    for measurement in measurements {
        let CanonicalJson::Object(m) = measurement else { panic!("measurement") };
        let CanonicalJson::Object(provider) = &m["provider"] else { panic!("provider") };
        assert_eq!(provider["kind"], CanonicalJson::String("SceneProbe".into()));
        assert_eq!(provider["identity"], CanonicalJson::String("main".into()));
        let CanonicalJson::Array(samples) = &m["samples"] else { panic!("samples") };
        assert_eq!(samples.len(), 600, "SceneProbe must produce exactly 600 measured samples");
    }

    // Second run should reuse cached report (compatible identity → no new report).
    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(paths(), initial, "compatible report should be reused");

    // Forge the report — third run must produce a new one.
    fs::OpenOptions::new().append(true).open(&initial[0]).unwrap().write_all(b"forged").unwrap();
    let third = run();
    assert_eq!(third.status.code(), Some(0), "{}", String::from_utf8_lossy(&third.stderr));
    assert_eq!(paths().len(), 2, "forged report must not satisfy compatible cache identity");
}

#[test]
fn service_probe_unavailable_without_dev_reports_diagnostic() {
    let dir = isolated_cwd("service_probe_no_env");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pkg.jet"), "payload: { name: \"app\", version: \"0.1.0\" }\n").unwrap();
    fs::write(dir.join("src/main.jet"), r#"module env.dev {
    services: { mydb: { enable: true, init: "echo mydb", ready: "true" } }
}

module perf.package {
    budgets: [Budget.{
        name: "readiness",
        scope: .Service("mydb"),
        metric: .ServiceReadiness,
        provider: .ServiceProbe("mydb"),
        comparison: .AbsoluteFrom("local/mydb"),
        limit: .AtMost(500ms),
        enforcement: .Warn,
    }],
}
fn run() {}
"#).unwrap();
    // jet budget check: ServiceProbe stub should report "unavailable", not 101.
    let out = Command::new(jet())
        .args(["budget", "check"])
        .current_dir(&dir)
        .output()
        .unwrap();
    // Exit code must not be 101 (I2 — rustc never speaks to user).
    assert_ne!(out.status.code(), Some(101), "rustc must never speak to user (I2)");
    assert_ne!(out.status.code(), Some(2), "usage error unexpected");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("ServiceProbe") || combined.contains("unavailable") || combined.contains("jet dev"),
        "expected unavailability message; got:\n{combined}"
    );
}

#[test]
fn service_probe_uses_jetpack_lifecycle_and_produces_twenty_samples() {
    use jet_foundation::PerformanceBudget::CanonicalJson;

    let dir = isolated_cwd("service_probe_runtime");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(
        dir.join("env.jet"),
        r#"module env.dev {
    services: { mydb: { enable: true, init: "sleep 30", ready: "true" } }
}
"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/main.jet"),
        r#"use project.env.dev

module perf.package {
    budgets: [Budget.{
        name: "readiness",
        scope: .Service("mydb"),
        metric: .ServiceReadiness,
        provider: .ServiceProbe("mydb"),
        comparison: .AbsoluteFrom("local/mydb"),
        limit: .AtMost(500ms),
        enforcement: .Warn,
    }],
}
fn run() {}
"#,
    )
    .unwrap();

    let out = Command::new(jet())
        .args(["self", "devtools", "probe", "src/main.jet"])
        .current_dir(&dir)
        .env("JETPACK_ROOT", dir.join("jetpack-root"))
        .env("HOME", dir.join("home"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let reports = fs::read_dir(dir.join(".jet/perf/reports"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(reports.len(), 1, "expected one report; got {reports:?}");
    let bytes = fs::read(&reports[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else {
        panic!("report")
    };
    let CanonicalJson::Object(content) = &report["content"] else {
        panic!("content")
    };
    let CanonicalJson::Array(measurements) = &content["measurements"] else {
        panic!("measurements")
    };
    assert_eq!(measurements.len(), 1);
    let CanonicalJson::Object(measurement) = &measurements[0] else {
        panic!("measurement")
    };
    let CanonicalJson::Object(provider) = &measurement["provider"] else {
        panic!("provider")
    };
    assert_eq!(provider["kind"], CanonicalJson::String("ServiceProbe".into()));
    assert_eq!(provider["identity"], CanonicalJson::String("mydb".into()));
    let CanonicalJson::Array(samples) = &measurement["samples"] else {
        panic!("samples")
    };
    assert_eq!(samples.len(), 20, "ServiceProbe must produce exactly 20 samples");
}
