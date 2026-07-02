use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn compile_temp(name: &str, src: &str) -> jet::CompileOutput {
    let dir = std::env::temp_dir().join(format!("jet_corelib_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    })
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn build_and_run(
    dir: &PathBuf,
    name: &str,
    src: &str,
    env: &[(&str, &str)],
    stdin: Option<&str>,
) -> (i32, String, String) {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
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
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut cmd = Command::new(&bin);
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(text) = stdin {
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if let Some(mut input) = child.stdin.take() {
            use std::io::Write;
            input.write_all(text.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        return (
            out.status.code().unwrap_or(0),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        );
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn canonical_core_import_resolves() {
    let out = compile_temp(
        "core_imports.jet",
        r#"
use core.fs as fs

fn main() {
    print(fs.exists("/tmp"))
}
"#,
    );
    assert!(out.rust.contains("jet_std_fs_exists"));
}

#[test]
fn importing_core_without_calls_is_free_in_codegen() {
    let out = compile_temp(
        "core_import_only.jet",
        r#"
use core.fs as fs
use core.io as io
use core.env as env
use core.process as process
use core.math as math
use core.random as random
use core.time as time
use core.encoding.json as json

fn main() {
    print("ok")
}
"#,
    );
    assert!(!out.rust.contains("mod jet_std"));
    assert!(!out.rust.contains("jet_std_fs_read"));
    assert!(out.rust.contains("fn main()"));
}

#[test]
fn io_input_reads_a_line_from_stdin() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping io.input test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_input_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "input_demo",
        r#"
use core.io as io

fn main() {
    name :: io.input("name? ") ?? panic("read failed")
    print("hello, {name}")
}
"#,
        &[],
        Some("Ada\n"),
    );
    assert_eq!(code, 0, "stdin demo failed");
    assert!(
        stdout.contains("hello, Ada"),
        "expected greeting on stdout, got stdout={stdout:?} stderr={stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn random_and_time_output_pins_with_seed_and_epoch() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping random/time pin test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_time_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, _stderr) = build_and_run(
        &dir,
        "time_random",
        r#"
use core.random as random
use core.time as time

fn main() {
    random.seed(42)
    print(random.int(1, 100))
    print(random.float())
    print(time.now())
}
"#,
        &[("LEX_TEST_EPOCH", "1700000000000")],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "9\n0.05534409481976061\n1700000000000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deadline_context_exceed_reports_e3003() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping deadline runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_deadline_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, _stdout, stderr) = build_and_run(
        &dir,
        "deadline_exceeded",
        r#"
use core.time as time

fn main() {
    #Context(deadline: time.now()) {
        time.sleep(5)
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 70, "deadline exceed should stop with runtime code 70");
    assert!(
        stderr.contains("Error [E3003]"),
        "deadline exceed should report E3003, got: {stderr:?}"
    );
    assert!(
        stderr.contains("E3003"),
        "deadline exceed should carry code E3003, got: {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// SL9 / R10: importing every core module without calling it must not bloat the binary.
#[test]
fn importing_all_core_modules_without_calls_stays_hello_world_sized() {
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping core use size test (need jet + rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_corelib_size_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    fs::write(
        dir.join("hello.jet"),
        "fn main() {\n    print(\"hello, world\");\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("core_import_only.jet"),
        r#"
use core.fs as fs
use core.io as io
use core.env as env
use core.process as process
use core.math as math
use core.random as random
use core.time as time
use core.encoding.json as json

fn main() {
    print("ok")
}
"#,
    )
    .unwrap();

    let hello = Command::new(&jet)
        .args(["build", "--small", "hello.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(hello.status.success(), "hello build failed");
    let imports = Command::new(&jet)
        .args(["build", "--small", "core_import_only.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(imports.status.success(), "import-only build failed");

    let hello_size = fs::metadata(dir.join("build/hello")).unwrap().len();
    let import_size = fs::metadata(dir.join("build/core_import_only"))
        .unwrap()
        .len();
    assert!(
        import_size <= hello_size.saturating_add(4096),
        "import-only binary ({import_size} bytes) should stay within 4 KiB of hello ({hello_size} bytes)"
    );

    let _ = fs::remove_dir_all(&dir);
}

// D-JSON3=B: lenient decode (core.encoding.json.decode) surfaces coercions via log lines.
// Probes: (a) string→number coercion line + plain value; (b) clean JSON = no log lines;
// (c) multiple coercions = one line each.
#[test]
fn json_decode_lenient_surfaces_coercions() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping json_decode_lenient_surfaces_coercions (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_json_decode_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Probe (a): string→number coercion appears in stderr; value is usable in arithmetic.
    let (code_a, stdout_a, stderr_a) = build_and_run(
        &dir,
        "json_coerce_a",
        r#"
use core.encoding.json as json
fn main() {
    data :: json.decode("{{\"port\":\"8080\"}}") ?? panic("bad json")
    if data == Object(m) {
        if m["port"] == Int(n) {
            print(n + 1)
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_a, 0, "probe (a) failed: {stderr_a}");
    assert_eq!(
        stdout_a, "8081\n",
        "probe (a): decoded value should be plain number + 1"
    );
    assert!(
        stderr_a.contains("json coerce")
            && stderr_a.contains("port")
            && stderr_a.contains("number"),
        "probe (a): coercion log line missing or malformed; got: {stderr_a}"
    );

    // Probe (b): clean JSON (no string values that look like numbers/bools) → no coercion lines.
    let (code_b, stdout_b, stderr_b) = build_and_run(
        &dir,
        "json_coerce_b",
        r#"
use core.encoding.json as json
fn main() {
    data :: json.decode("{{\"port\":8080,\"name\":\"api\"}}") ?? panic("bad json")
    if data == Object(m) {
        if m["port"] == Int(n) {
            print(n)
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_b, 0, "probe (b) failed: {stderr_b}");
    assert_eq!(stdout_b, "8080\n", "probe (b): value should be 8080");
    assert!(
        !stderr_b.contains("json coerce"),
        "probe (b): spurious coercion line emitted for clean JSON; got: {stderr_b}"
    );

    // Probe (c): multiple coercions → one log line each.
    let (code_c, stdout_c, stderr_c) = build_and_run(
        &dir,
        "json_coerce_c",
        r#"
use core.encoding.json as json
fn main() {
    data :: json.decode("{{\"port\":\"8080\",\"enabled\":\"true\"}}") ?? panic("bad json")
    if data == Object(m) {
        if m["port"] == Int(n) {
            print(n)
        }
        if m["enabled"] == Bool(b) {
            print(b)
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_c, 0, "probe (c) failed: {stderr_c}");
    assert_eq!(
        stdout_c, "8080\ntrue\n",
        "probe (c): both coerced values should come back plain"
    );
    let coerce_lines: Vec<&str> = stderr_c
        .lines()
        .filter(|l| l.contains("json coerce"))
        .collect();
    assert_eq!(
        coerce_lines.len(),
        2,
        "probe (c): expected 2 coercion lines, got {}; stderr: {stderr_c}",
        coerce_lines.len()
    );
    // Each line names its field.
    assert!(
        coerce_lines.iter().any(|l| l.contains("port")),
        "probe (c): no coercion line for 'port'"
    );
    assert!(
        coerce_lines.iter().any(|l| l.contains("enabled")),
        "probe (c): no coercion line for 'enabled'"
    );

    let _ = fs::remove_dir_all(&dir);
}

// D-PARSE-1: the user-facing JSON parser is full RFC 8259 — exponents,
// `\uXXXX` (with surrogate pairs), every escape — and rejects invalid input
// (bad escapes, raw control chars) with a clear line/message.
#[test]
fn json_parser_is_rfc8259_complete() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping json_parser_is_rfc8259_complete (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_json_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Probe (a): exponent number, BMP `\u` escape, a surrogate pair, and a `\t`
    // escape — all parsed, then re-serialized (keys sort, `\t` re-escaped).
    let (code_a, stdout_a, stderr_a) = build_and_run(
        &dir,
        "json_full_a",
        r#"
use core.encoding.json as json
fn main() {
    raw :: "{{\"big\":1.5e3,\"acc\":\"caf\\u00e9\",\"grin\":\"\\uD83D\\uDE00\",\"tab\":\"a\\tb\"}}"
    data :: json.parse(raw) ?? panic("bad json")
    print(json.to_string(data))
}
"#,
        &[],
        None,
    );
    assert_eq!(code_a, 0, "probe (a) failed: {stderr_a}");
    // D-ENC-DYN1=A+: `json.parse` yields the `Data` value; an integral-valued number
    // (`1.5e3` == 1500) collapses to `Int`, so it re-serializes as `1500` (not `1500.0`).
    assert_eq!(
        stdout_a, "{\"acc\":\"café\",\"big\":1500,\"grin\":\"😀\",\"tab\":\"a\\tb\"}\n",
        "probe (a): full parse + re-serialize"
    );

    // Probe (b): an invalid escape is rejected with a clear message.
    let (code_b, stdout_b, stderr_b) = build_and_run(
        &dir,
        "json_full_b",
        r#"
use core.encoding.json as json
fn main() {
    if json.parse("{{\"x\":\"a\\qb\"}}") == {
        ok(_) -> { print("OK") }
        err(e) -> { print("ERR: {e.message}") }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_b, 0, "probe (b) failed: {stderr_b}");
    assert_eq!(
        stdout_b, "ERR: invalid escape in string\n",
        "probe (b): bad escape rejected"
    );

    // Probe (c): a raw control character (literal tab) inside a string is rejected.
    let (code_c, stdout_c, stderr_c) = build_and_run(
        &dir,
        "json_full_c",
        "
use core.encoding.json as json
fn main() {
    if json.parse(\"{{\\\"x\\\":\\\"a\tb\\\"}}\") == {
        ok(_) -> { print(\"OK\") }
        err(e) -> { print(\"ERR: {e.message}\") }
    }
}
",
        &[],
        None,
    );
    assert_eq!(code_c, 0, "probe (c) failed: {stderr_c}");
    assert_eq!(
        stdout_c, "ERR: control character in string\n",
        "probe (c): raw control char rejected"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn channel_stress_1000_messages() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping channel stress test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_channel_stress_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "channel_stress",
        r#"
use core.tasks as tasks

fn main() {
ch: Channel<Int> : tasks.channel()
sender: Sender<Int> : ch.sender()
    producer :: tasks.spawn(take(sender) () => {
        loop i in 1..1000 {
            sender.send(i)
        }
    })
    producer.join()
    total: Int = 0
    loop i in 1..1000 {
        total = total + (ch.receive() ?? panic("channel closed"))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "channel stress failed: {stderr}");
    assert_eq!(stdout, "500500\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scheduler_spawn_1000_tasks() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping scheduler spawn test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_scheduler_spawn_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "scheduler_spawn",
        r#"
use core.tasks as tasks

fn main() {
ch: Channel<Int> :: tasks.channel()
sender: Sender<Int> :: ch.sender()
    loop i in 1..1000 {
        copy :: sender.clone()
        tasks.spawn(take(copy) () => {
            copy.send(1)
        })
    }
    total: Int := 0
    loop i in 1..1000 {
        total = (total + (ch.receive() ?? panic("channel closed")))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "scheduler spawn stress failed: {stderr}");
    assert_eq!(stdout, "1000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scheduler_spawn_10000_tasks() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping 10k scheduler spawn test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_scheduler_10k_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "scheduler_spawn_10k",
        r#"
use core.tasks as tasks

fn main() {
ch: Channel<Int> :: tasks.channel()
sender: Sender<Int> :: ch.sender()
    loop i in 1..10000 {
        copy :: sender.clone()
        tasks.spawn(take(copy) () => {
            copy.send(1)
        })
    }
    total: Int := 0
    loop i in 1..10000 {
        total = (total + (ch.receive() ?? panic("channel closed")))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "10k scheduler spawn failed: {stderr}");
    assert_eq!(stdout, "10000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "local 100k parked-task stress; run with --ignored"]
fn scheduler_spawn_100000_tasks_bench() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping 100k scheduler bench (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_scheduler_100k_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "scheduler_spawn_100k",
        r#"
use core.tasks as tasks

fn main() {
ch: Channel<Int> :: tasks.channel()
sender: Sender<Int> :: ch.sender()
    loop i in 1..100000 {
        copy :: sender.clone()
        tasks.spawn(take(copy) () => {
            copy.send(1)
        })
    }
    total: Int := 0
    loop i in 1..100000 {
        total = (total + (ch.receive() ?? panic("channel closed")))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "100k scheduler bench failed: {stderr}");
    assert_eq!(stdout, "100000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn race_cancels_losing_task() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping race cancel test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_race_cancel_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "race_cancel",
        r#"
use core.tasks as tasks
use core.time as time

fn fast_nine() -> Int {
    return 9
}

fn slow_one() -> Int {
    time.sleep(300)
    return 1
}

fn main() {
    taskgroup g {
        slow :: g.task { slow_one() }
        fast :: g.task { fast_nine() }
        winner :: g.race([slow, fast])
        print(winner)
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "race cancel test failed: {stderr}");
    assert_eq!(stdout, "9\n");
    let _ = fs::remove_dir_all(&dir);
}

/// c45 drift-guard: `core_module_items` in Sema/CheckerCoreLib.rs must cover
/// every module in `Loader::KNOWN_CORE_MODULES` (and no extras).
///
/// `core_module_items` is `pub(crate)` so we can't call it directly from here.
/// Instead we parse the source file and extract the string literals used as
/// match arm heads — the same technique used in tests/decisions.rs for
/// Source/Syntax.rs. This breaks if the match arm format changes, which is
/// exactly the right tripwire: a format change must be mirrored here.
#[test]
fn core_module_items_covers_known_core_modules() {
    let src = fs::read_to_string("crates/jet-sema/src/Sema/CheckerCoreLib.rs")
        .expect("Source/Sema/CheckerCoreLib.rs must exist");

    // Extract the `core_module_items` function body.
    let fn_start = src
        .find("pub(crate) fn core_module_items(")
        .expect("core_module_items function not found in CheckerCoreLib.rs");
    // Find the closing `}` at top-level indent (just after the last arm).
    let fn_body = &src[fn_start..];
    // Collect ALL string literals from match arm heads (handles `"a" | "b" => &[` form too).
    let mut items_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in fn_body.lines() {
        let trimmed = line.trim();
        // A match arm head: `"core.fs" => &[` or `"core.log" | "jet.log" => &[`
        if trimmed.starts_with('"') && trimmed.contains("=>") {
            let arm_head = trimmed.split("=>").next().unwrap_or("");
            let mut rest = arm_head;
            while let Some(start) = rest.find('"') {
                rest = &rest[start + 1..];
                if let Some(end) = rest.find('"') {
                    items_keys.insert(rest[..end].to_string());
                    rest = &rest[end + 1..];
                } else {
                    break;
                }
            }
        }
        // Stop when we reach the wildcard arm or the closing brace of the function.
        if trimmed == "_ => &[]," || trimmed == "_ => &[]" {
            break;
        }
    }

    // D-CORENS-CANON1: most ring packages still normalize to legacy `jet.*`
    // internal dispatch keys. `core.archive` is already canonical end-to-end.
    let ring_names = ["log", "crypto", "http", "regex", "reactive", "db"];
    let known_raw = jet::Loader::KNOWN_CORE_MODULES;
    let known: std::collections::BTreeSet<String> = known_raw
        .iter()
        .map(|s| {
            if let Some(ring) = s.strip_prefix("core.") {
                if ring_names.contains(&ring) {
                    return format!("jet.{ring}");
                }
            }
            s.to_string()
        })
        .collect();

    let missing_from_items: Vec<&String> =
        known.iter().filter(|m| !items_keys.contains(*m)).collect();
    let extra_in_items: Vec<&String> = items_keys.iter().filter(|m| !known.contains(*m)).collect();

    assert!(
        missing_from_items.is_empty(),
        "core_module_items is missing arms for modules in KNOWN_CORE_MODULES: {:?}\n\
         Add a match arm in Source/Sema/CheckerCoreLib.rs for each.",
        missing_from_items
    );
    assert!(
        extra_in_items.is_empty(),
        "core_module_items has arms for modules NOT in KNOWN_CORE_MODULES: {:?}\n\
         Either add to KNOWN_CORE_MODULES in Source/Loader.rs or remove the arm.",
        extra_in_items
    );
}

/// c136 / D-SERDE9-12: generic `@[Codable]` is first-class. The derive injects
/// `T: Encode`/`T: Decode` on exactly the wire-reaching params (D-SERDE9/10); a
/// phantom/skip-only param gets no serde bound (it still gets structural Clone).
/// E2413 is retired (D-SERDE12).
#[test]
fn generic_codable_injects_wire_param_bounds() {
    let out = compile_temp(
        "generic_serde.jet",
        r#"
use core.encoding.json as json

@[Codable]
struct Wrap<T> {
    value: T
}

@[Codable]
struct Id<K> {
    raw: Int
    #[Skip] marker: K?
}

fn main() {
    print("x")
}
"#,
    );
    let rs = &out.rust;
    // D-SERDE9: the wire-reaching param T carries `user_Encode`/`user_Decode`.
    assert!(
        rs.contains("impl<T: user_Encode") && rs.contains("user_Encode for user_Wrap<T>"),
        "Wrap's Encode impl must bound T: user_Encode\n{rs}"
    );
    assert!(
        rs.contains("impl<T: user_Decode") && rs.contains("user_Decode for user_Wrap<T>"),
        "Wrap's Decode impl must bound T: user_Decode\n{rs}"
    );
    // D-SERDE10: the phantom param K gets NO Encode/Decode bound (only Clone).
    assert!(
        rs.contains("impl<K: Clone> user_Encode for user_Id<K>"),
        "Id's Encode impl must NOT bound K with user_Encode (phantom param)\n{rs}"
    );
    assert!(
        rs.contains("impl<K: Clone> user_Decode for user_Id<K>"),
        "Id's Decode impl must NOT bound K with user_Decode (phantom param)\n{rs}"
    );
    assert!(
        !rs.contains("K: user_Encode") && !rs.contains("K: user_Decode"),
        "phantom param K must never get a serde bound\n{rs}"
    );
}

/// c136: a generic `@[Codable]` value round-trips through json encode/decode, and
/// a phantom-param type serializes regardless of its phantom argument (D-SERDE10).
#[test]
fn generic_codable_round_trips() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping generic serde round-trip (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_gserde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, _stderr) = build_and_run(
        &dir,
        "gserde",
        r#"
use core.encoding.json as json

@[Codable]
struct Wrap<T> {
    value: T
}

@[Codable]
struct Id<K> {
    raw: Int
    #[Skip] marker: K?
}

fn main() {
    wi :: Wrap<Int>.{ value: 7 }
    print(json.to_string(wi))
    back :: json.decode<Wrap<Int>>("{{\"value\":42}}") ?? panic("bad")
    print(back.value)
    id :: Id<Wrap<Int>>.{ raw: 9, marker: null }
    print(json.to_string(id))
    rid :: json.decode<Id<Wrap<Int>>>("{{\"raw\":3}}") ?? panic("bad id")
    print(rid.raw)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "generic serde program should run cleanly");
    assert_eq!(stdout, "{\"value\":7}\n42\n{\"raw\":9}\n3\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── c152: full TOML adapter (D-ENC-DYN1=A+) ──────────────────────────────────
// Nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars decode into
// nested `@[Codable]` structs, and the rich tree round-trips through `to_string`.
#[test]
fn toml_full_nested_decode_and_round_trip() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping toml_full_nested_decode_and_round_trip (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_toml_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Typed decode into nested structs + array-of-tables.
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "toml_typed",
        r#"
use core.encoding.toml as toml
@[Codable]
struct Server { host: String  port: Int }
@[Codable]
struct Config { title: String  server: Server  ports: [Int] }
fn main() {
    raw :: "title = \"jet\"\nports = [80, 443]\n\n[server]\nhost = \"db.local\"\nport = 5432\n"
    cfg :: toml.decode<Config>(raw) ?? panic("bad toml")
    print(cfg.title)
    print(cfg.server.host)
    print(cfg.server.port)
    print(cfg.ports.len())
    print(toml.to_string(cfg))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "toml typed decode failed: {stderr}");
    assert_eq!(
        stdout,
        "jet\ndb.local\n5432\n2\ntitle = \"jet\"\nports = [80, 443]\n\n[server]\nhost = \"db.local\"\nport = 5432\n"
    );

    // Dynamic parse → rich tree → round-trip identity for a nested document.
    let (code2, stdout2, stderr2) = build_and_run(
        &dir,
        "toml_dyn",
        r#"
use core.encoding.toml as toml
fn main() {
    raw :: "name = \"a\"\n\n[db]\nhost = \"h\"\nport = 1\n"
    d :: toml.parse(raw) ?? panic("bad")
    print(toml.to_string(d))
}
"#,
        &[],
        None,
    );
    assert_eq!(code2, 0, "toml dynamic parse failed: {stderr2}");
    assert_eq!(stdout2, "name = \"a\"\n\n[db]\nhost = \"h\"\nport = 1\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── c152: full YAML adapter (D-ENC-YAML1 = A) ────────────────────────────────
// Block mappings + sequences, flow collections, typed scalars, block scalars,
// comments, document markers, and anchors/aliases.
#[test]
fn yaml_full_nested_decode_and_features() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping yaml_full_nested_decode_and_features (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_yaml_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Typed decode of a nested document with a block sequence of mappings.
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "yaml_typed",
        r#"
use core.encoding.yaml as yaml
@[Codable]
struct Service { name: String  port: Int }
@[Codable]
struct Config { app: String  services: [Service] }
fn main() {
    raw :: "app: myapp\nservices:\n  - name: web\n    port: 80\n  - name: db\n    port: 5432\n"
    cfg :: yaml.decode<Config>(raw) ?? panic("bad yaml")
    print(cfg.app)
    print(cfg.services.len())
    print(cfg.services[0].name)
    print(cfg.services[1].port)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "yaml typed decode failed: {stderr}");
    assert_eq!(stdout, "myapp\n2\nweb\n5432\n");

    // Advanced features: flow collections, comments, `---`, anchors/aliases, block scalar.
    let (code2, stdout2, stderr2) = build_and_run(
        &dir,
        "yaml_adv",
        r#"
use core.encoding.yaml as yaml
fn main() {
    raw :: "---\n# a config\nflowlist: [1, 2, 3]\nbase: &b\n  host: local\n  port: 80\nuse: *b\nnote: |\n  one\n  two\n"
    d :: yaml.parse(raw) ?? panic("bad yaml")
    if d == Object(top) {
        if top["flowlist"] == Array(xs) {
            print(xs.len())
        }
        if top["use"] == Object(u) {
            print(u.len())
        }
        if top["note"] == Text(s) {
            print(s.contains("one"))
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code2, 0, "yaml advanced features failed: {stderr2}");
    assert_eq!(stdout2, "3\n2\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn option_zip_and_lift2_combinators() {
    // D-HOLE1: `.zip`/`Option.lift2` — both present -> a present result; either
    // absent -> `null`. No general "hole" type; these are plain library combinators
    // on `T?`.
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping option combinator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_option_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "option_combinators",
        r#"
fn main() {
    both_a: Float? :: value(2.0)
    both_b: Float? :: value(5.0)
    print(both_a.zip(both_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_a, both_b))

    a_only: Float? :: value(2.0)
    b_missing: Float? :: null
    print(a_only.zip(b_missing).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, a_only, b_missing))

    both_missing_a: Float? :: null
    both_missing_b: Float? :: null
    print(both_missing_a.zip(both_missing_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_missing_a, both_missing_b))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "option combinator fixture failed: {stderr}");
    assert_eq!(
        stdout, "10.0\n10.0\nnull\nnull\nnull\nnull\n",
        "unexpected option combinator output: {stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}
