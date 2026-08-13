#[test]
fn text_unicode_audit_surface_runs() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping text unicode test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_text_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "text_unicode",
        r#"
use core.text as text

fn run() {
    print(text.caseless_eq("Straße", "STRASSE"))
    print(text.nfc("é") == "é")
    print(text.nfkc("ﬃ"))
    print(text.graphemes("é👍").len())
    print(text.words("Hi, κόσμε 123.").len())
    print(text.sentences("One. Two!").len())
    print(text.display_width("表a"))
    print(text.is_alphabetic("Ж"))
    print(text.is_numeric("٣"))
    print(text.pad_start("7", 3, "0"))
    print(text.center("x", 3, "."))
    print(text.starts_any("jetpack", ["jet", "go"]))
    print(text.char_indices("éa")[1])
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "text unicode test failed: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\nffi\n2\n3\n2\n3\ntrue\ntrue\n007\n.x.\ntrue\n2:a\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn db_checked_sql_params_feed_parameterized_execute() {
    assert!(common::have_rustc(), "DB query_one proof requires rustc");
    let dir = std::env::temp_dir().join(format!("jet_corelib_db_sql_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "db_checked_sql",
        r#"
use core.db as db

fn run() {
    conn := db.open_memory()
    policy :: db.policy("person", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "owner")
    created :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT, active INTEGER)"
    ]) ?? panic("migrate")
    skipped :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT, active INTEGER)"
    ]) ?? panic("migrate again")
    id :: 7
    name :: "Ada"
    insert :: SQL.{"INSERT INTO person (id, name, active) VALUES ({id}, {name}, 1)"}
    _inserted :: scoped.execute(insert.template(), db.params(insert)) ?? panic("insert")
    failed :: db.transaction(scoped, "bad batch", [
        "INSERT INTO person (id, name, active) VALUES (8, 'Grace', 1)",
        "INSERT INTO missing_table VALUES (1)"
    ]) ?? 0
    row :: scoped.query_one("SELECT id, name, active FROM person WHERE id = ?", [DBValue.Int(7)]) ?? panic("query")
    found :: row ?? panic("missing")
    missing :: scoped.query_one("SELECT id, name, active FROM person WHERE id = ?", [DBValue.Int(99)]) ?? panic("missing query")
    count :: scoped.query_one("SELECT COUNT(*) AS n FROM person", []) ?? panic("count")
    counted :: count ?? panic("missing count")
    print(created)
    print(skipped)
    print(failed)
    print(db.row_int(found, "id") ?? 0)
    print(db.row_text(found, "name") ?? "bad")
    print(db.row_int(found, "active") ?? 0)
    print(db.row_int(counted, "n") ?? 0)
    if missing == .None { print("absent") } else { panic("unexpected row") }
    _closed :: scoped.close()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "db checked sql test failed: {stderr}");
    assert_eq!(stdout, "1\n0\n0\n7\nAda\n1\n1\nabsent\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_db_implements_driver_trait() {
    assert!(common::have_rustc(), "DB Driver query_one proof requires rustc");
    let dir = std::env::temp_dir().join(format!("jet_corelib_db_driver_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.db as db

fn count_people<T: Driver>(&conn: T) => Int ? DBError {
    row :: conn.query_one("SELECT COUNT(*) AS n FROM person", [])?
    found :: row ?? panic("missing")
    missing :: conn.query_one("SELECT id, name FROM person WHERE id = ?", [DBValue.Int(99)])?
    if missing == .None { print("absent") } else { panic("unexpected row") }
    return .Ok(db.row_int(found, "n") ?? 0)
}

fn run() {
    conn := db.open_memory()
    policy :: db.policy("person", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "owner")
    _ :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT)"
    ]) ?? panic("create")
    _ :: scoped.execute(
        "INSERT INTO person (id, name) VALUES (?, ?)",
        [DBValue.Int(1), DBValue.Text("Ada")]
    ) ?? panic("insert")
    n :: count_people(&scoped) ?? panic("count")
    print(n)
    _closed :: scoped.close()
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "db_driver_trait",
        src,
        &[],
        None,
    );
    assert_eq!(code, 0, "db Driver trait AOT failed: {stderr}");
    assert_eq!(stdout, "absent\n1\n");

    // I9: default `jet run` (Cranelift) must share the same Driver meaning.
    let jet = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/jet");
    let path = dir.join("db_driver_trait_jit.jet");
    fs::write(&path, src).unwrap();
    let out = Command::new(&jet)
        .arg("run")
        .arg(&path)
        .output()
        .expect("spawn jet run for Driver JIT");
    assert!(
        out.status.success(),
        "db Driver trait JIT failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "absent\n1\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_db_query_one_first_row_matches_all_execution_tiers() {
    assert!(common::have_rustc(), "DB query_one parity proof requires rustc");
    assert!(
        jet_jit::cranelift_host_supported(),
        "DB query_one parity proof requires a resident Cranelift host"
    );
    let dir = common::unique_tmp("jet_corelib_db_query_one_parity");
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.db as db

fn run() {
    conn :: db.open_memory()
    policy :: db.policy("person", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "owner")
    _ :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT)"
    ]) ?? panic("create")
    _ :: scoped.execute(
        "INSERT INTO person (id, name) VALUES (?, ?)",
        [DBValue.Int(7), DBValue.Text("Ada")]
    ) ?? panic("insert")
    present :: scoped.query_one(
        "SELECT id, name FROM person WHERE id = ?",
        [DBValue.Int(7)]
    ) ?? panic("present query")
    if present == {
        .Val(_) -> {
            print("present")
        }
        .None -> { panic("present row absent") }
    }
    absent :: scoped.query_one(
        "SELECT id, name FROM person WHERE id = ?",
        [DBValue.Int(99)]
    ) ?? panic("absent query")
    if absent == .None { print("absent") } else { panic("absent row present") }
    _closed :: scoped.close()
}
"#;
    let (code, aot_stdout, stderr) = build_and_run(&dir, "db_query_one_parity", src, &[], None);
    assert_eq!(code, 0, "DB query_one AOT failed: {stderr}");
    let expected = "present\nabsent\n";
    assert_eq!(aot_stdout, expected);

    let file = dir.join("db_query_one_parity.jet");
    fs::write(&file, src).unwrap();
    let path = file.to_string_lossy().into_owned();
    let interpreted = match jet::Interpreter::dev_iteration(&path, false, true) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("DB query_one forced interpreter failed: {diags:?}")
        }
    };
    assert_eq!(interpreted, expected);

    jet_jit::reset_jit_trace_for_test();
    let resident = match jet::Interpreter::dev_iteration(&path, false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(stderr, "");
            assert_eq!(exit_code, 0);
            stdout
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("DB query_one resident JIT failed: {diags:?}")
        }
    };
    assert_eq!(resident, expected);
    assert!(jet_jit::jit_executed_for_test(), "DB query_one must execute in resident JIT");
    assert!(!jet_jit::deopt_invoked_for_test(), "DB query_one must not deopt");
    assert!(!jet_jit::fallback_invoked_for_test(), "DB query_one must not fall back");
    assert_eq!(resident, interpreted, "DB query_one tiers diverged");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_fmt_human_formatting_surface_runs() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.fmt runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_fmt_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "human_format",
        r#"
use core.fmt as fmt

fn run() {
    print(fmt.number(1204331))
    print(fmt.decimal(1234.5678, 2))
    print(fmt.percent(0.1234, 1))
    print(fmt.bytes(1500000000))
    print(fmt.duration(222000))
    print(fmt.ordinal(21))
    print(fmt.plural(2, "row", "rows"))
    print(fmt.pad_left("7", 3, "0"))
    print(fmt.pad_right("go", 4, "."))
    print(fmt.pad_center("x", 3, "."))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.fmt program failed: {stderr}");
    assert_eq!(
        stdout,
        "1,204,331\n1,234.57\n12.3%\n1.5 GB\n3m 42s\n21st\n2 rows\n007\ngo..\n.x.\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_log_structured_file_sink_runs() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.log file sink test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_log_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "log_file",
        r#"
use core.log as log

fn run() {
    log.set_sink("jsonl", "service.log")
    s :: log.span("request")
    log.enter(s)
    log.info_fields("served", [log.field("route", "/"), log.int("status", 200), log.redact("token")])
    log.close(s)
    print("done")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.log file sink failed: {stderr}");
    assert_eq!(stdout, "done\n");
    let log = fs::read_to_string(dir.join("service.log")).expect("service.log must be written");
    assert!(log.contains("\"body\":\"served\""), "log: {log}");
    assert!(log.contains("\"route\":\"/\""), "log: {log}");
    assert!(log.contains("\"status\":200"), "log: {log}");
    assert!(log.contains("\"token\":\"[redacted]\""), "log: {log}");
    assert!(log.contains("\"spans\":[\"request\"]"), "log: {log}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_testing_helpers_run_against_files() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.testing helper test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_testing_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("corpus")).unwrap();
    fs::write(dir.join("fixture.txt"), "fixture").unwrap();
    fs::write(dir.join("golden.txt"), "gold").unwrap();
    fs::write(dir.join("corpus/a.txt"), "alpha").unwrap();
    fs::write(dir.join("corpus/b.txt"), "beta").unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "testing_helpers",
        r#"
use core.testing as testing

module perf.testing {
    budgets: [Budget.{
        name: "parse",
        scope: .Bench("parse"),
        metric: .BenchTime(.P50),
        provider: .BenchMeasurement("parse"),
        comparison: .AbsoluteFrom("local/testing-helpers"),
        limit: .AtMost(5ms),
        enforcement: .Warn,
    }]
}

fn run() {
    print(testing.fixture("fixture.txt"))
    print(testing.golden("golden.txt", "gold"))
    print(testing.snap("case", "snap"))
    print(testing.corpus("corpus").len())
    print(testing.temp_dir("case").len() > 0)
    clock :: testing.fake_clock(99)
    rng := testing.fake_rng(5)
    print(clock.now())
    print(rng.int(1, 4) >= 1)
}

#Bench("parse") {}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.testing helpers failed: {stderr}");
    assert_eq!(
        stdout,
        "fixture\ntrue\ntrue\n2\ntrue\n99\ntrue\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("__snapshots__/case.snap")).unwrap(),
        "snap"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deadline_context_exceed_reports_e3003() {
    let have_rustc = common::have_rustc();
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

fn run() {
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

#[cfg(unix)]
#[test]
fn process_wait_observes_inherited_deadline() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping process deadline runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_process_deadline_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let sleeper = dir.join("sleep.sh");
    write_executable(&sleeper, "#!/bin/sh\nsleep 2\n");
    let source = format!(
        r#"
use core.process as process
use core.time as time

fn run() {{
    child :: process.cmd(["{sleeper}"]).spawn() ?? panic("spawn failed")
    #Context(deadline: time.now() + 20) {{
        child.wait() ?? panic("wait failed")
    }}
}}
"#,
        sleeper = jet_string_path(&sleeper)
    );
    let (code, _stdout, stderr) =
        build_and_run(&dir, "process_wait_deadline", &source, &[], None);
    assert_eq!(code, 70, "process wait deadline should stop with runtime code 70");
    assert!(
        stderr.contains("Error [E3003]") && stderr.contains("process wait"),
        "process wait should report its compiler-owned deadline boundary: {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// SL9 / R10: importing every core module without calling it must not bloat the binary.
#[test]
fn importing_all_core_modules_without_calls_stays_hello_world_sized() {
    let jet = jet_bin();
    let have_rustc = common::have_rustc();
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
        "fn run() {\n    print(\"hello, world\");\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("core_import_only.jet"),
        r#"
use core.files as fs
use core.io as io
use core.env as env
use core.process as process
use core.math as math
use core.random as random
use core.time as time
use core.encoding.json as json

fn run() {
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
    let have_rustc = common::have_rustc();
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
fn run() {
    data :: json.decode("{{\"port\":\"8080\"}}") ?? panic("bad json")
    if data == .Object(m) {
        if m["port"] == .Int(n) {
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
fn run() {
    data :: json.decode("{{\"port\":8080,\"name\":\"api\"}}") ?? panic("bad json")
    if data == .Object(m) {
        if m["port"] == .Int(n) {
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
fn run() {
    data :: json.decode("{{\"port\":\"8080\",\"enabled\":\"true\"}}") ?? panic("bad json")
    if data == .Object(m) {
        if m["port"] == .Int(n) {
            print(n)
        }
        if m["enabled"] == .Bool(b) {
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
    let have_rustc = common::have_rustc();
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
fn run() {
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
fn run() {
    if json.parse("{{\"x\":\"a\\qb\"}}") == {
        .Ok(_) -> { print("OK") }
        .Err(e) -> { print("ERR: {e.message}") }
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
fn run() {
    if json.parse(\"{{\\\"x\\\":\\\"a\tb\\\"}}\") == {
        .Ok(_) -> { print(\"OK\") }
        .Err(e) -> { print(\"ERR: {e.message}\") }
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
    let have_rustc = common::have_rustc();
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

fn run() {
(sender, ch) : tasks.channel<Int>()
    producer :: task {
        loop i, 1..1000 {
            sender.send(i)
        }
    }
    producer.join() ?? 0
    total: Int = 0
    loop i, 1..1000 {
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
    let have_rustc = common::have_rustc();
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

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i, 1..1000 {
        dup :: ~sender
        task {
            dup.send(1)
        }
    }
    total := 0
    loop i, 1..1000 {
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
    let have_rustc = common::have_rustc();
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

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i, 1..10000 {
        dup :: ~sender
        task {
            dup.send(1)
        }
    }
    total := 0
    loop i, 1..10000 {
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
    let have_rustc = common::have_rustc();
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

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i, 1..100000 {
        dup :: ~sender
        task {
            dup.send(1)
        }
    }
    total := 0
    loop i, 1..100000 {
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
    let have_rustc = common::have_rustc();
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

fn fast_nine() => Int {
    return 9
}

fn slow_one() => Int {
    time.sleep(300)
    return 1
}

fn run() {
    task.group g {
        winner :: (task.race {
            slow_one(),
            fast_nine()
        }) ?? panic("race failed")
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

/// c45 drift-guard: `core_module_items` in Sema/CheckerCoreLib must cover
/// every module in `Loader::KNOWN_CORE_MODULES` (and no extras).
///
/// `core_module_items` is `pub(crate)` so we can't call it directly from here.
/// Instead we parse the source file and extract the string literals used as
/// match arm heads — the same technique used in tests/decisions.rs for
/// Source/Syntax.rs. This breaks if the match arm format changes, which is
/// exactly the right tripwire: a format change must be mirrored here.
#[test]
fn core_module_items_covers_known_core_modules() {
    let src = fs::read_to_string("crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs")
        .expect("CheckerCoreLib/module_items.rs must exist");

    // Extract the `core_module_items` function body.
    let fn_start = src
        .find("fn core_module_items(")
        .expect("core_module_items function not found in CheckerCoreLib/module_items.rs");
    // Find the closing `}` at top-level indent (just after the last arm).
    let fn_body = &src[fn_start..];
    // Collect ALL string literals from match arm heads (handles `"a" | "b" => &[` form too).
    let mut items_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // `core.lang` is generated from the marker registry and returns before the
    // static match table, so it has no ordinary arm to extract.
    if fn_body.contains("if module == \"core.lang\"") {
        items_keys.insert("core.lang".to_string());
    }
    if fn_body.contains("module == Syntax::CORE_MEM_MODULE") {
        items_keys.insert("core.mem".to_string());
    }
    if fn_body.contains("Syntax::CORE_MOD_MODULE =>") {
        items_keys.insert("core.mod".to_string());
    }
    for line in fn_body.lines() {
        let trimmed = line.trim();
        // A match arm head: `"core.files" => &[` or `"core.log" => &[`
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

    // D-CORENS1 / D-CORENS-CANON1: every Core module keeps its canonical
    // `core.*` key through the checker tables. No internal `jet.*` rewrite is
    // allowed to hide a missing or extra module arm.
    let known: std::collections::BTreeSet<String> = jet::Loader::KNOWN_CORE_MODULES
        .iter()
        .map(|s| s.to_string())
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

#[test]
fn compiler_sources_reject_retired_jet_ring_keys() {
    // D-CORENS-CANON1: keep the registry guard broader than one table. A new
    // quoted `jet.<ring>` dispatch key in any compiler source must fail this
    // test instead of silently restoring a second internal namespace.
    let roots = [
        "Source",
        "crates/jet-foundation/src",
        "crates/jet-driver/src",
        "crates/jet-sema/src",
        "crates/jet-codegen/src",
        "crates/jet-comptime/src",
        "crates/jet-jit/src",
        "crates/jet-repl/src",
    ];
    let retired = [
        "\"jet.log\"",
        "\"jet.crypto\"",
        "\"jet.http\"",
        "\"jet.regex\"",
        "\"jet.reactive\"",
        "\"jet.db\"",
        "\"jet.plugin\"",
        "\"jet.time\"",
    ];
    let mut pending = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    while let Some(path) = pending.pop() {
        let metadata = fs::metadata(&path).unwrap_or_else(|error| {
            panic!("failed to inspect compiler source {}: {error}", path.display())
        });
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).unwrap_or_else(|error| {
                panic!("failed to read compiler source {}: {error}", path.display())
            }) {
                pending.push(entry.unwrap().path());
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read compiler source {}: {error}", path.display())
        });
        for &key in &retired {
            assert!(
                !source.contains(key),
                "retired internal module key {key} found in {}",
                path.display()
            );
        }
    }
}

#[test]
fn core_reference_lists_every_built_core_module() {
    let docs = fs::read_to_string("docs/reference/core-library.md")
        .expect("core library reference must exist");
    let missing: Vec<&str> = jet::Loader::KNOWN_CORE_MODULES
        .iter()
        .copied()
        .filter(|module| *module != "core")
        .filter(|module| !docs.contains(&format!("`{module}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/reference/core-library.md must list every built Core module from KNOWN_CORE_MODULES: {:?}",
        missing
    );
}

#[test]
fn jet_raylib_namespace_is_not_a_core_module_alias() {
    assert!(jet::Syntax::is_known_core_module("core.raylib"));
    assert!(!jet::Syntax::is_known_core_module("jet.raylib"));

    let src = r#"
use jet.raylib as rl

fn run() {
    print("nope")
}
"#;
    let diags = jet::compile(src).expect_err("jet.raylib must be rejected");
    assert!(
        diags.iter().any(|d| d.code == "E0341"),
        "expected E0341 for retired namespace, got: {:?}",
        diags.iter().map(|d| d.code.to_string()).collect::<Vec<_>>()
    );
}

#[test]
fn jet_time_namespace_uses_retirement_teaching() {
    let source = concat!(
        "use jet", ".time as time\n\n",
        "fn run() {\n",
        "    time.now()\n",
        "}\n"
    );
    let diagnostics = jet::compile(source).expect_err("the retired time namespace must fail");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0341"),
        "expected E0341 for the retired time namespace, got: {:?}",
        diagnostics.iter().map(|diagnostic| diagnostic.code.to_string()).collect::<Vec<_>>()
    );
    assert_eq!(
        jet::Syntax::rename_target(concat!("jet", ".time", ".now")),
        Some("core.time.now")
    );
}
