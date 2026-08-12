use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use jet_foundation::JSON::parse_json;
use jet_foundation::PerformanceBudget::CanonicalJson;

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

/// Write a tiny source file with a known error and return its path. Keep each
/// file in its own authority root: concurrent tests creating sibling files
/// directly under the shared temp directory would change that root while
/// `jet` is checking its authority snapshot.
fn bad_file(tag: &str) -> PathBuf {
    let dir = isolated_cwd(&format!("bad_{tag}"));
    let p = dir.join("bad.jet");
    fs::write(&p, "fn run() {\n    pirnt(\"hi\");\n}\n").unwrap();
    p
}

/// Replace machine-specific temp paths so snapshots are portable.
fn scrub(s: &str, file: &Path) -> String {
    let mut scrubbed = s.replace(&file.display().to_string(), "BAD.jet");
    let temp_dir = std::env::temp_dir();
    if let Some(temp_root) = temp_dir.parent() {
        if let Ok(relative) = file.strip_prefix(temp_root) {
            scrubbed = scrubbed.replace(&relative.display().to_string(), "BAD.jet");
        }
    }
    scrubbed
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





fn budget_project(tag: &str, limit: u64) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\n",
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
        dir.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\n",
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
        dir.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\n",
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
    fs::write(dir.join("package.jet"), "name: \"app\"\nversion: \"0.1.0\"\n").unwrap();
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
    fs::write(dir.join("package.jet"), "name: \"app\"\nversion: \"0.1.0\"\n").unwrap();
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



// ── Exit-code table ────────────────────────────────────────────────































// ── Human + JSON golden for one diagnostic ────────────────────────





// ── CI determinism: ANSI-free + identical when piped/NO_COLOR ──────



// ── explain coverage: every registered code resolves ──────────────







fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}

// ── Wave B: greeting, did-you-mean, doctor, completions, fix, externals ──
























































// ── Ext-optional CLI (no syntax decision; pure CLI behavior) ──────────







// ── D-ILE1: implicit executable inference (no package.jet) ───────────────


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





// ── D-BUILDPROFILE1: --release / --profile=<name> ─────────────────────────────





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



fn expand_layout_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/expand_layout_facts.jet")
}


fn expand_effects_layout_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/expand_effects_layout.jet")
}










// ── D-JPK-FILENAME2=B (A2): retired manifest filenames → E1226 ──────







// ── D-ILE1 / D-CLI-BARE1: bare entry resolution (card #497 verifier bounce) ──
//
// `resolve_bare_entry` (Source/main.rs) delegated to a `find_project_entry`
// that only ever checked `main.jet`/`.jet/main.jet`, never the package-named
// D-ILE1 fallback (`<package>.jet`, using `package.jet`'s `payload.name`). The shipped
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




fn scene_probe_project(tag: &str, budget: &str) -> PathBuf {
    let dir = isolated_cwd(tag);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("package.jet"), "name: \"app\"\nversion: \"0.1.0\"\n").unwrap();
    let source = r#"use core.game as game

module perf.game {
    budgets: [
        __SCENE_BUDGET__,
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
"#
    .replace("__SCENE_BUDGET__", budget);
    fs::write(dir.join("src/main.jet"), source).unwrap();
    dir
}

fn scene_probe_run(dir: &Path) -> Output {
    Command::new(jet())
        .args(["self", "devtools", "probe", "src/main.jet"])
        .current_dir(dir)
        .output()
        .unwrap()
}

fn scene_probe_once(tag: &str, budget: &str) -> (PathBuf, CanonicalJson) {
    let dir = scene_probe_project(tag, budget);
    let output = scene_probe_run(&dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = dir.join(".jet/perf/reports");
    let paths = fs::read_dir(&reports)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1, "expected exactly one report; got {paths:?}");
    let bytes = fs::read(&paths[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    (dir, CanonicalJson::parse_canonical(&bytes).unwrap())
}

fn assert_scene_probe_measurement(report: &CanonicalJson, expected_metric: &str) {
    let CanonicalJson::Object(report) = report else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 1, "expected one SceneProbe measurement");
    let CanonicalJson::Object(measurement) = &measurements[0] else { panic!("measurement") };
    let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
    assert_eq!(provider["kind"], CanonicalJson::String("SceneProbe".into()));
    assert_eq!(provider["identity"], CanonicalJson::String("main".into()));
    let CanonicalJson::Object(metric) = &measurement["metric"] else { panic!("metric") };
    assert_eq!(metric["name"], CanonicalJson::String(expected_metric.into()));
    let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
    assert_eq!(samples.len(), 600, "SceneProbe must produce exactly 600 measured samples");
}
