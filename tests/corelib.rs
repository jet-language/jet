use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

mod dns_resolver_policy {
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/DnsResolverPolicy.rs");

    pub fn resolv_conf(text: &str) -> Vec<String> {
        jet_net_dns_parse_resolv_conf(text)
    }

    pub fn scutil(text: &str) -> Vec<String> {
        jet_net_dns_parse_scutil(text)
    }

    pub fn windows(text: &str) -> Vec<String> {
        jet_net_dns_parse_windows_addresses(text)
    }
}

#[test]
fn encoding_stream_foundation_types_are_real_jet_values() {
    let dir = std::env::temp_dir().join(format!("jet_encoding_foundation_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "encoding_foundation",
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.xml as xml
use core.encoding.cbor as cbor

fn keep_error(v: ^encoding.EncodingError) -> encoding.EncodingError { return v }
fn keep_cause(v: ^encoding.EncodingCause) -> encoding.EncodingCause { return v }
fn keep_event(v: ^encoding.DataEvent) -> encoding.DataEvent { return v }
fn keep_format(v: ^encoding.EncodingFormat) -> encoding.EncodingFormat { return v }
fn keep_kind(v: ^encoding.EncodingErrorKind) -> encoding.EncodingErrorKind { return v }
fn keep_json_reader(v: ^json.JSONReader) -> json.JSONReader { return v }
fn keep_json_writer(v: ^json.JSONWriter) -> json.JSONWriter { return v }
fn keep_jsonl_reader(v: ^jsonl.JSONLReader) -> jsonl.JSONLReader { return v }
fn keep_jsonl_writer(v: ^jsonl.JSONLWriter) -> jsonl.JSONLWriter { return v }
fn keep_csv_reader(v: ^csv.CSVReader) -> csv.CSVReader { return v }
fn keep_csv_writer(v: ^csv.CSVWriter) -> csv.CSVWriter { return v }
fn keep_xml_reader(v: ^xml.XMLReader) -> xml.XMLReader { return v }
fn keep_xml_writer(v: ^xml.XMLWriter) -> xml.XMLWriter { return v }
fn keep_cbor_reader(v: ^cbor.CBORReader) -> cbor.CBORReader { return v }
fn keep_cbor_writer(v: ^cbor.CBORWriter) -> cbor.CBORWriter { return v }

fn run() {
    limits: encoding.EncodingLimits := encoding.EncodingLimits.safe()
    print("{limits.buffer_bytes}:{limits.max_depth}:{limits.max_item_bytes}:{limits.max_expansion_depth}:{limits.max_expansion_bytes}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "65536:256:16777216:32:8388608\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_reader_writer_are_real_incremental_handles() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("events.json");
    let path_text = path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    limits :: encoding.EncodingLimits.safe()
    output :: files.create("{path_text}") ?? panic("create")
    writer :: json.writer(^output, limits, false) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("write")
    writer.write(encoding.DataEvent.Key("message")) ?? panic("write")
    writer.write(encoding.DataEvent.Text("hi ☺")) ?? panic("write")
    writer.write(encoding.DataEvent.Key("values")) ?? panic("write")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("write")
    writer.write(encoding.DataEvent.Int(7)) ?? panic("write")
    writer.write(encoding.DataEvent.Bool(true)) ?? panic("write")
    writer.write(encoding.DataEvent.Null) ?? panic("write")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("write")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("write")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish")

    input :: files.open("{path_text}") ?? panic("open")
    reader :: json.reader(^input, encoding.EncodingLimits.safe()) ?? panic("reader")
    count := 0
    loop count < 11 {{
        maybe_event :: reader.next() ?? panic("next")
        if maybe_event == None {{ print("eof") }} else {{ print("event") }}
        count++
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "event\nevent\nevent\nevent\nevent\nevent\nevent\nevent\nevent\nevent\neof\n"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"message":"hi ☺","values":[7,true,null]}"#);
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_defaults_paths_limits_and_terminal_errors_are_stable() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_hostile_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("hostile.json");
    let default_path = dir.join("default.json");
    let limited_path = dir.join("limited.json");
    fs::write(&input_path, r#"{"o":[0,{"i":"\u263a"}]}"#).unwrap();
    let input_text = input_path.to_string_lossy().replace('\\', "\\\\");
    let default_text = default_path.to_string_lossy().replace('\\', "\\\\");
    let limited_text = limited_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    default_output :: files.create("{default_text}") ?? panic("create")
    default_writer :: json.writer(^default_output) ?? panic("default writer")
    default_writer.write(encoding.DataEvent.Null) ?? panic("default write")
    default_writer.finish() ?? panic("default finish")
    default_input :: files.open("{default_text}") ?? panic("default open")
    default_reader :: json.reader(^default_input) ?? panic("default reader")
    if default_reader.next() == {{
        ok(_) -> {{ print(true) }}
        err(_) -> {{ print(false) }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 2
    input :: files.open("{input_text}") ?? panic("open")
    reader :: json.reader(^input, limits) ?? panic("reader")
    count := 0
    loop count < 8 {{
        result :: reader.next()
        if result == {{
            ok(_) -> {{ count++ }}
            err(first) -> {{
                again :: reader.next()
                if again == {{
                    ok(_) -> {{ print("reader-not-latched") }}
                    err(second) -> {{
                        print(first.path)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}

    finished_output :: files.create("{limited_text}") ?? panic("create")
    finished_writer :: json.writer(^finished_output) ?? panic("writer")
    finished_writer.write(encoding.DataEvent.Null) ?? panic("write")
    finished_writer.finish() ?? panic("finish")
    after_finish :: finished_writer.write(encoding.DataEvent.Null)
    if after_finish == {{
        ok(_) -> {{ print("finish-missed") }}
        err(first) -> {{
            after_flush :: finished_writer.flush()
            if after_flush == {{
                ok(_) -> {{ print("finish-not-latched") }}
                err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    escaped_limits := encoding.EncodingLimits.safe()
    escaped_limits.max_item_bytes = 1
    escaped_output :: files.create("{limited_text}") ?? panic("create")
    escaped_writer :: json.writer(^escaped_output, escaped_limits) ?? panic("writer")
    escaped_result :: escaped_writer.write(encoding.DataEvent.Text("\n"))
    if escaped_result == {{
        ok(_) -> {{ print("escape-missed") }}
        err(first) -> {{
            escaped_again :: escaped_writer.finish()
            if escaped_again == {{
                ok(_) -> {{ print("escape-not-latched") }}
                err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream_hostile", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\n$[\"o\"][1][\"i\"]\ntrue\ntrue\ntrue\n");
    assert_eq!(fs::read_to_string(&default_path).unwrap(), "null");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

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

#[test]
fn invariant_refinement_proves_fixed_array_index() {
    let src = r#"
#Invariant("value >= 0 && value < 4")
Index4 :: distinct Int

fn pick(xs: [String#4], i: Index4) -> String {
    return xs[i]
}

fn run() {
    words: [String#4] :: ["zero", "one", "two", "three"]
    print(pick(words, Index4(2)))
}
"#;
    let out = compile_temp("refinement_index.jet", src);
    assert!(
        !out.rust.contains("jet_index_vec(&"),
        "proof-carrying fixed-array index should not emit runtime list bounds helper:\n{}",
        out.rust
    );
}

#[test]
fn comptime_find_glob_records_sorted_lock_inputs() {
    let dir = std::env::temp_dir().join(format!(
        "jet_comptime_find_{}_{}",
        std::process::id(),
        "lock"
    ));
    fs::create_dir_all(dir.join("inputs/nested")).unwrap();
    fs::write(dir.join("inputs/alpha-1.txt"), "alpha").unwrap();
    fs::write(dir.join("inputs/nested/beta-2.txt"), "beta").unwrap();
    fs::write(dir.join("inputs/nested/gamma-3.txt"), "gamma").unwrap();
    fs::write(dir.join("inputs/nested/beta-2.md"), "skip").unwrap();
    let src = r#"
comptime PATHS = find("inputs/**/{{alpha,beta}}-[0-9].t?t")

fn run() {
    print(PATHS.join("|"))
}
"#;
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected find fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let paths: Vec<&str> = out
        .comptime_inputs
        .iter()
        .map(|input| input.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["inputs/alpha-1.txt", "inputs/nested/beta-2.txt"]
    );
    assert!(out
        .comptime_inputs
        .iter()
        .all(|input| input.hash.len() == 64));
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn core_args_audit_surface_runs_and_reports_suggestions() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_args_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.args as args

fn run() {
    spec :: args.spec()
        .flag_short("verbose", "v", "print extra detail")
        .option_env("profile", "config profile", "NAME", "JET_ARGS_PROFILE")
        .option_int("jobs", "worker count", "N")
        .repeat("tag", "classification tag", "TAG")
    parsed :: spec.parse(["tool", "-vv", "--jobs", "8", "--tag", "a", "--tag=b"]) ?? panic("parse failed")
    print(parsed.flag("verbose"))
    print(parsed.option("profile") ?? "")
    print(parsed.option_int("jobs") ?? 0)
    print(parsed.options("tag").len())
    if spec.parse(["tool", "--verbse"]) == {
        ok(_) -> {
            print("unexpected")
        }
        err(e) -> {
            print(e)
        }
    }
}
"#;
    let (_code, stdout, stderr) = build_and_run(
        &dir,
        "args_audit",
        src,
        &[("JET_ARGS_PROFILE", "dev")],
        None,
    );
    assert!(
        stdout.contains("unknown option `--verbse`")
            && stdout.contains("did you mean `--verbose`?"),
        "core.args suggestion missing:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.starts_with("true\ndev\n8\n2\n"));
}

#[test]
fn core_os_facts_and_interrupt_hook_compile() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_os_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os

fn run() {
    os.on_interrupt(() => {
        print("interrupted")
    })
    print(os.name().len() > 0)
    print(os.family().len() > 0)
    print(os.arch().len() > 0)
    print(os.cpu_count() >= 1)
    print(os.pid() >= 1)
    print(os.hostname().len() > 0)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "os_facts", src, &[], None);
    assert_eq!(code, 0, "core.os program failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn core_os_interrupt_prelude_is_emitted_only_when_used() {
    let facts_only = compile_temp(
        "os_facts_only.jet",
        r#"
use core.os as os

fn run() {
    print(os.name())
}
"#,
    );
    assert!(
        !facts_only.rust.contains("mod jet_os_interrupt")
            && !facts_only.rust.contains("SetConsoleCtrlHandler")
            && !facts_only.rust.contains("jet_std_os_on_interrupt"),
        "ordinary core.os facts should not inherit signal FFI"
    );
    assert!(
        facts_only.rust.contains("JET_INTERRUPT_HANDLER_DEPTH")
            && facts_only.rust.contains("fn jet_runtime_should_unwind()"),
        "safe central panic-boundary state must remain available without signal FFI"
    );

    let with_interrupt = compile_temp(
        "os_interrupt.jet",
        r#"
use core.os as os

fn run() {
    os.on_interrupt(() => {
        print("interrupted")
    })
}
"#,
    );
    assert!(
        with_interrupt.rust.contains("mod jet_os_interrupt")
            && with_interrupt.rust.contains("SetConsoleCtrlHandler")
            && with_interrupt.rust.contains("CTRL_C_EVENT")
            && with_interrupt.rust.contains("AtomicUsize")
            && with_interrupt.rust.contains("catch_unwind")
            && with_interrupt.rust.contains("struct PanicBoundary")
            && with_interrupt.rust.contains("impl Drop for PanicBoundary")
            && with_interrupt.rust.contains("#[cfg(not(any(unix, windows)))]")
            && with_interrupt.rust.contains("interrupt handling is unavailable on this target")
            && with_interrupt.rust.contains("jet_std_os_on_interrupt")
            && !with_interrupt.rust.contains("let _ = handler"),
        "on_interrupt should keep its Unix/Windows dispatcher and no silent no-op"
    );
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_handlers_are_additive_and_ordered() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let dir = std::env::temp_dir().join(format!("jet_corelib_interrupt_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os
use core.process as process

fn run() {
    os.on_interrupt(() => { panic("first handler failed") })
    os.on_interrupt(() => {
        print("second")
        process.exit(0)
    })
    print("ready")
    loop { }
}

"#;
    let out = compile_temp("os_interrupt_runtime.jet", src);
    let rs = dir.join("main.rs");
    let bin = dir.join("interrupt-runtime");
    fs::write(&rs, out.rust).unwrap();
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
    assert!(rustc.status.success(), "rustc failed:\n{}", String::from_utf8_lossy(&rustc.stderr));

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n", "registration was not ready before run continued");
    unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let status = child.wait().unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(status.success(), "interrupt child failed: {status}");
    assert_eq!(rest, "second\n");
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_deadline_diagnostic_unwinds_inside_handler_boundary() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_interrupt_deadline_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os
use core.process as process
use core.time as time

fn run() {
    os.on_interrupt(() => {
        #Context(deadline: time.now()) {
            time.sleep(5)
        }
    })
    os.on_interrupt(() => {
        print("second")
        process.exit(0)
    })
    print("ready")
    loop { }
}
"#;
    let out = compile_temp("os_interrupt_deadline.jet", src);
    assert!(
        out.rust.contains("jet_interrupt_handler_panic_enter")
            && out.rust.contains("jet_interrupt_handler_panic_leave"),
        "interrupt handlers need a boundary distinct from scheduler-task identity"
    );
    let rs = dir.join("main.rs");
    let bin = dir.join("interrupt-deadline");
    fs::write(&rs, out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc failed:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let output = child.wait_with_output().unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(
        output.status.success(),
        "interrupt child failed: {}",
        output.status
    );
    assert_eq!(rest, "second\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3003]: deadline exceeded while waiting in time sleep"));
    assert!(stderr.contains("Why: this wait point observed the task context deadline"));
    assert!(stderr.contains("Fix: raise the deadline budget or shorten the work"));
}

#[test]
fn core_os_interrupt_runtime_failures_use_the_boundary_aware_helpers() {
    let task_mem = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs");
    let time = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs");
    let scheduler = include_str!("../crates/jet-codegen/src/Prelude/Scheduler.rs");
    assert!(!task_mem.contains("process::exit(70)"));
    assert!(!time.contains("process::exit(70)"));
    assert_eq!(scheduler.matches("process::exit(70)").count(), 1);
    assert!(task_mem.contains("super::jet_panic(\"<core.tasks>\""));
    // The deadline helper binds the rendered E3003 to a local so it can feed
    // both the interrupt-handler unwind branch and the boundary-aware diagnostic
    // (D-fatal-waits containment, commit 40a9fca8); it still routes its fatal
    // path through `jet_runtime_diagnostic`, never `process::exit`.
    assert!(time.contains("jet_runtime_diagnostic(rendered)"));
    assert!(scheduler.contains("fn jet_scheduler_fatal(msg: &str) -> !"));
    let core = include_str!("../crates/jet-codegen/src/Prelude/Core.rs");
    assert!(core.contains("fn jet_runtime_should_unwind() -> bool"));
    assert!(core.contains("jet_scheduler_in_task() || jet_interrupt_handler_should_unwind()"));
    assert!(core.contains("if jet_runtime_should_unwind()"));
    assert!(core.contains("if jet_interrupt_handler_should_unwind()"));
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
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        if link.deps_dir.is_dir() {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", link.deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
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

fn build_and_run_multi(
    dir: &PathBuf,
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let src = fs::read_to_string(&entry_path).unwrap();
    let shown = entry_path.to_string_lossy();
    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected multi-file fixture:\n{}",
            jet::render_diagnostics(&shown, &src, &diags)
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
        "rustc rejected generated multi-file code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).current_dir(dir).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn jet_string_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn dns_name_wire(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for label in name.trim_end_matches('.').split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out
}

fn dns_fixture_response(query: &[u8]) -> Vec<u8> {
    let mut pos = 12usize;
    while pos < query.len() && query[pos] != 0 {
        pos += query[pos] as usize + 1;
    }
    pos += 1;
    let qtype = u16::from_be_bytes([query[pos], query[pos + 1]]);
    let question_end = pos + 4;
    let mut resp = Vec::new();
    resp.extend_from_slice(&query[0..2]);
    resp.extend_from_slice(&0x8180u16.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&query[12..question_end]);
    resp.extend_from_slice(&[0xc0, 0x0c]);
    resp.extend_from_slice(&qtype.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&0u32.to_be_bytes());
    let rdata = match qtype {
        16 => {
            let mut r = Vec::new();
            r.push(3);
            r.extend_from_slice(b"jet");
            r
        }
        33 => {
            let mut r = Vec::new();
            r.extend_from_slice(&10u16.to_be_bytes());
            r.extend_from_slice(&20u16.to_be_bytes());
            r.extend_from_slice(&443u16.to_be_bytes());
            r.extend_from_slice(&dns_name_wire("srv.example.test"));
            r
        }
        _ => Vec::new(),
    };
    resp.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    resp.extend_from_slice(&rdata);
    resp
}

fn dns_question_end(query: &[u8]) -> usize {
    let mut pos = 12usize;
    while pos < query.len() && query[pos] != 0 {
        pos += query[pos] as usize + 1;
    }
    pos + 5
}

fn dns_truncated_response(query: &[u8]) -> Vec<u8> {
    let end = dns_question_end(query);
    let mut response = Vec::new();
    response.extend_from_slice(&query[0..2]);
    response.extend_from_slice(&0x8380u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&query[12..end]);
    response
}

fn dns_cname_additional_response(query: &[u8]) -> Vec<u8> {
    let end = dns_question_end(query);
    let alias = dns_name_wire("alias.example.test");
    let mut response = Vec::new();
    response.extend_from_slice(&query[0..2]);
    response.extend_from_slice(&0x8180u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&query[12..end]);
    response.extend_from_slice(&[0xc0, 0x0c]);
    response.extend_from_slice(&5u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(&(alias.len() as u16).to_be_bytes());
    response.extend_from_slice(&alias);
    response.extend_from_slice(&alias);
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(&4u16.to_be_bytes());
    response.extend_from_slice(&[192, 0, 2, 42]);
    response
}

#[test]
fn core_net_dns_txt_and_srv_are_real_udp_queries() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut buf = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut buf).unwrap();
            let resp = dns_fixture_response(&buf[..n]);
            socket.send_to(&resp, peer).unwrap();
        }
    });
    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    txts :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("txt")
    print(txts[0])
    srvs :: net.dns_srv_at("{}", "_jet._tcp.example.test", 1000) ?? panic("srv")
    print("{{net.dns_srv_target(srvs[0])}}:{{net.dns_srv_port(srvs[0])}}")
}}
"#,
        addr, addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_fixture", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "jet\nsrv.example.test:443\n");
}

#[test]
fn core_net_dns_udp_truncation_retries_tcp_and_reads_cname_additional() {
    use std::io::{Read, Write};

    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = tcp.local_addr().unwrap();
    let udp = std::net::UdpSocket::bind(addr).unwrap();
    let server = std::thread::spawn(move || {
        let mut udp_query = [0u8; 512];
        let (n, peer) = udp.recv_from(&mut udp_query).unwrap();
        udp.send_to(&dns_truncated_response(&udp_query[..n]), peer)
            .unwrap();

        let (mut stream, _) = tcp.accept().unwrap();
        let mut prefix = [0u8; 2];
        stream.read_exact(&mut prefix).unwrap();
        let mut tcp_query = vec![0u8; u16::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut tcp_query).unwrap();
        let response = dns_cname_additional_response(&tcp_query);
        stream
            .write_all(&(response.len() as u16).to_be_bytes())
            .unwrap();
        stream.write_all(&response).unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_tcp_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    ips :: net.dns_a_at("{}", "service.example.test", 1000) ?? panic("dns")
    print(net.ip_to_string(ips[0]))
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_tcp_cname", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "192.0.2.42\n");
}

#[test]
fn core_net_dns_timeout_is_one_budget_across_udp_and_tcp() {
    use std::io::Read;

    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = tcp.local_addr().unwrap();
    let udp = std::net::UdpSocket::bind(addr).unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = udp.recv_from(&mut query).unwrap();
        let started = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(70));
        udp.send_to(&dns_truncated_response(&query[..n]), peer).unwrap();
        let (mut stream, _) = tcp.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(500))).unwrap();
        let mut prefix = [0u8; 2];
        stream.read_exact(&mut prefix).unwrap();
        let mut request = vec![0u8; u16::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut request).unwrap();
        let mut closed = [0u8; 1];
        let _ = stream.read(&mut closed);
        started.elapsed()
    });
    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_budget_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(r#"
use core.net as net

fn run() {{
    _ :: net.dns_a_at("{}", "service.example.test", 120) ?? panic("dns total timeout")
}}
"#, addr);
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_total_budget", &src, &[], None);
    let elapsed = server.join().unwrap();
    assert_ne!(code, 0, "stalled DNS TCP fallback unexpectedly succeeded");
    assert!(stderr.contains("dns total timeout"), "{stderr}");
    assert!(elapsed < std::time::Duration::from_millis(190), "UDP and TCP each received a fresh timeout: {elapsed:?}");
}

#[test]
fn core_net_dns_platform_resolver_policy_uses_native_sources() {
    let net = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/NetHttp.rs");
    assert!(net.contains("#[cfg(target_os = \"linux\")]"));
    assert!(net.contains("read_to_string(\"/etc/resolv.conf\")"));
    assert!(net.contains("#[cfg(target_os = \"macos\")]"));
    assert!(net.contains("Command::new(\"scutil\").arg(\"--dns\")"));
    assert!(net.contains("#[cfg(windows)]"));
    assert!(net.contains("Get-DnsClientServerAddress"));
    assert!(net.contains("$_.ServerAddresses"));
    assert!(!net.contains("Command::new(\"ipconfig\")"));
    assert!(!net.contains("1.1.1.1"));
}

#[test]
fn core_net_dns_platform_resolver_parsers_accept_native_fixtures() {
    assert_eq!(
        dns_resolver_policy::resolv_conf(
            "# generated\nnameserver 192.0.2.53 # vpn\nnameserver 2001:db8::53\nsearch example.test\n"
        ),
        ["192.0.2.53:53", "[2001:db8::53]:53"]
    );
    assert_eq!(
        dns_resolver_policy::scutil(
            "resolver #1\n  nameserver[0] : 192.0.2.54\n  nameserver[1] : 2001:db8::54\n  search domain[0] : example.test\n"
        ),
        ["192.0.2.54:53", "[2001:db8::54]:53"]
    );
    assert_eq!(
        dns_resolver_policy::windows("{192.0.2.55, 2001:db8::55}\r\n\r\n"),
        ["192.0.2.55:53", "[2001:db8::55]:53"]
    );
}

#[test]
fn core_net_dns_platform_resolver_parsers_reject_noise_and_malformed_entries() {
    assert!(dns_resolver_policy::resolv_conf(
        "nameserver nope\nnot-nameserver 192.0.2.1\nnameserver [broken\n"
    )
    .is_empty());
    assert!(dns_resolver_policy::scutil(
        "nameserver[x] : 192.0.2.1\nnameserver[0 : 192.0.2.2\nnameserver[] : 192.0.2.3\n"
    )
    .is_empty());
    assert!(dns_resolver_policy::windows(
        "InterfaceAlias Ethernet\nServerAddresses nope, 999.1.1.1\n"
    )
    .is_empty());
}

#[test]
fn core_net_dns_rejects_wrong_transaction_id() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = socket.recv_from(&mut query).unwrap();
        let mut response = dns_fixture_response(&query[..n]);
        let wrong = u16::from_be_bytes([response[0], response[1]]).wrapping_add(1);
        response[0..2].copy_from_slice(&wrong.to_be_bytes());
        socket.send_to(&response, peer).unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_bad_id_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    _ :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("forged DNS accepted")
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_bad_id", &src, &[], None);
    server.join().unwrap();
    assert_ne!(code, 0, "forged transaction ID was accepted");
    assert!(stderr.contains("forged DNS accepted"), "{stderr}");
}

#[test]
fn core_net_dns_transaction_ids_are_not_a_fixed_sequence() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..9 {
            let mut query = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut query).unwrap();
            ids.insert(u16::from_be_bytes([query[0], query[1]]));
            socket
                .send_to(&dns_fixture_response(&query[..n]), peer)
                .unwrap();
        }
        ids
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_ids_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    loop _i in 0..8 {{
        _ :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("dns")
    }}
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_ids", &src, &[], None);
    let ids = server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert!(ids.len() > 1, "all nine DNS queries reused one transaction ID");
}

fn run_rejected_dns_response(tag: &str, make_response: fn(&[u8]) -> Vec<u8>) -> String {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = socket.recv_from(&mut query).unwrap();
        socket
            .send_to(&make_response(&query[..n]), peer)
            .unwrap();
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_reject_{}_{}",
        tag,
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    _ :: net.dns_a_at("{}", "service.example.test", 1000) ?? panic("invalid DNS accepted")
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, tag, &src, &[], None);
    server.join().unwrap();
    assert_ne!(code, 0, "invalid DNS response was accepted: {tag}");
    stderr
}

#[test]
fn core_net_dns_rejects_non_response_and_cyclic_compression() {
    fn non_response(query: &[u8]) -> Vec<u8> {
        let mut response = dns_fixture_response(query);
        response[2] &= 0x7f;
        response
    }
    fn cyclic_compression(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let record_start = end;
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response.push(0xc0 | ((record_start >> 8) as u8 & 0x3f));
        response.push(record_start as u8);
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u32.to_be_bytes());
        response.extend_from_slice(&4u16.to_be_bytes());
        response.extend_from_slice(&[192, 0, 2, 1]);
        response
    }

    let qr = run_rejected_dns_response("dns_not_response", non_response);
    assert!(qr.contains("invalid DNS accepted"), "{qr}");
    let cycle = run_rejected_dns_response("dns_pointer_cycle", cyclic_compression);
    assert!(cycle.contains("invalid DNS accepted"), "{cycle}");
}

#[test]
fn core_net_dns_nxdomain_is_an_error() {
    fn nxdomain(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8183u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response
    }

    let stderr = run_rejected_dns_response("dns_nxdomain", nxdomain);
    assert!(stderr.contains("invalid DNS accepted"), "{stderr}");
}

#[test]
fn core_net_tcp_read_uses_scheduler_and_returns_typed_cancellation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_task_cancel_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_task_cancel",
        r#"
use core.net as net
use core.tasks as tasks

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    server :: tasks.spawn(take(listener, ready_tx) () => {
        stream := net.tcp_accept(listener) ?? panic("accept")
        ready_tx.send(1)
        if stream.read(1) == {
            ok(_) -> print("unexpected read")
            err(error) -> print(net.error_message(error))
        }
    })
    _client :: net.tcp_connect(address) ?? panic("connect")
    _ready :: ready_rx.receive() ?? panic("ready")
    server.cancel()
    server.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp read cancelled\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_read_persistent_timeout_uses_scheduler_budget() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_task_timeout_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_task_timeout",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    client :: tasks.spawn(take(address) () => {
        stream := net.tcp_connect(address) ?? panic("connect")
        time.sleep(100)
        stream.close() ?? panic("close")
    })
    stream := net.tcp_accept(listener) ?? panic("accept")
    net.set_read_timeout(&stream, 20) ?? panic("timeout")
    if stream.read(1) == {
        ok(_) -> print("unexpected read")
        err(error) -> print(net.error_message(error))
    }
    client.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("deadline exceeded while waiting in tcp read"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_expired_deadlines_return_typed_timeouts() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_expired_deadlines_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_expired_deadlines",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: tasks.spawn(take(listener) () => {
        first := net.tcp_accept(listener) ?? return
        time.sleep(100)
        first.close() ?? return
        second := net.tcp_accept(listener) ?? return
        time.sleep(100)
        second.close() ?? return
    })

    first := net.tcp_connect(address) ?? panic("first connect")
    net.set_read_timeout(&first, 0) ?? panic("zero timeout")
    if first.read(1) == {
        ok(_) -> print("unexpected first read")
        err(error) -> print(net.error_message(error))
    }
    first.close() ?? panic("first close")

    second := net.tcp_connect(address) ?? panic("second connect")
    #Context(deadline: time.now() - 1) {
        if second.read(1) == {
            ok(_) -> print("unexpected second read")
            err(error) -> print(net.error_message(error))
        }
    }
    second.close() ?? panic("second close")
    server.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "deadline exceeded while waiting in tcp read\ndeadline exceeded while waiting in tcp read\n"
    );
    assert!(!stderr.contains("E3003"), "typed timeout escaped as runtime deadline: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_write_all_uses_one_absolute_deadline() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_write_deadline_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_write_deadline",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: tasks.spawn(take(listener) () => {
        stream := net.tcp_accept(listener) ?? return
        loop {
            chunk := stream.read(65536) ?? return
            if chunk.len() == 0 {
                return
            }
            time.sleep(15)
        }
    })
    client := net.tcp_connect(address) ?? panic("connect")
    net.set_write_timeout(&client, 80) ?? panic("timeout")
    started := time.now()
    if client.write_text("x".repeat(16000000)) == {
        ok(_) -> print("unexpected write")
        err(error) -> print(net.error_message(error))
    }
    elapsed := time.now() - started
    print(elapsed < 300)
    client.close() ?? panic("close")
    server.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "deadline exceeded while waiting in tcp write\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_tls_byte_stream_runs_real_local_handshake_and_close_notify() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_surface_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca_cert = root.join("tests/fixtures/tls/localhost.cert.pem");
    let ca_key = root.join("tests/fixtures/tls/localhost.key.pem");
    let cert = dir.join("leaf.cert.pem");
    let key = dir.join("leaf.key.pem");
    let csr = dir.join("leaf.csr.pem");
    let extensions = dir.join("leaf.ext");
    fs::write(&extensions, "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n").unwrap();
    let req = Command::new("openssl").args(["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost", "-keyout"])
        .arg(&key).arg("-out").arg(&csr).output().unwrap();
    assert!(req.status.success(), "{}", String::from_utf8_lossy(&req.stderr));
    let sign = Command::new("openssl").args(["x509", "-req", "-days", "1", "-set_serial", "2", "-CA"])
        .arg(&ca_cert).arg("-CAkey").arg(&ca_key).arg("-extfile").arg(&extensions)
        .arg("-in").arg(&csr).arg("-out").arg(&cert).output().unwrap();
    assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
    let mut server = Command::new("openssl")
        .args(["s_server", "-quiet", "-www", "-accept", &port.to_string(), "-cert"])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let src = r#"
use core.net as net
use core.tls as tls

fn run() {
    tcp :: net.tcp_connect("127.0.0.1:$PORT") ?? panic("tcp")
    if tls.client(tcp, "localhost") == {
        ok(secure) -> {
            request: [U8] :: [71, 69, 84, 32, 47, 32, 72, 84, 84, 80, 47, 49, 46, 48, 13, 10, 13, 10]
            tls.write_all(&secure, request) ?? panic("write bytes")
            response :: tls.read(&secure, 4096) ?? panic("read bytes")
            print(response.len() > 0)
            tls.close(&secure) ?? panic("close notify")
            tls.close(&secure) ?? panic("idempotent close")
        }
        err(error) -> panic(net.error_message(error))
    }
}
"#.replace("$PORT", &port.to_string());
    let cert_text = ca_cert.to_string_lossy().into_owned();
    let (code, stdout, stderr) = build_and_run(&dir, "tls_byte_surface", &src, &[("SSL_CERT_FILE", &cert_text)], None);
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "true\n");
}

#[test]
fn canonical_core_import_resolves() {
    let out = compile_temp(
        "core_imports.jet",
        r#"
use core.files as fs

fn run() {
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
    );
    assert!(!out.rust.contains("mod jet_std"));
    assert!(!out.rust.contains("jet_std_fs_read"));
    assert!(out.rust.contains("fn main()"));
}

#[test]
fn core_data_import_and_codegen_resolve() {
    let out = compile_temp(
        "core_data_import.jet",
        r#"
use core.data as data

@Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    rows :: data.csv<Ticket>("team,minutes\nCore,4.0") ?? panic("bad csv")
    print(data.count(rows))
}
"#,
    );
    assert!(out.rust.contains("jet_enc_csv_decode"));
    assert!(out.rust.contains("jet_data_count"));
}

#[test]
fn core_files_depth_example_runs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let out = Command::new(&jet)
        .args(["run", "examples/features/io/files_depth.jet"])
        .output()
        .expect("run files_depth");
    assert!(
        out.status.success(),
        "files_depth failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string("examples/features/expected/io/files_depth.out").unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn core_watcher_example_runs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let out = Command::new(&jet)
        .args(["run", "examples/features/io/watcher.jet"])
        .output()
        .expect("run watcher");
    assert!(
        out.status.success(),
        "watcher failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string("examples/features/expected/io/watcher.out").unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[cfg(unix)]
#[test]
fn core_process_builder_pipeline_and_spawn_run() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_process_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();

    let probe = dir.join("probe.sh");
    let emit = dir.join("emit.sh");
    let cat = dir.join("cat.sh");
    let lines = dir.join("lines.sh");
    write_executable(
        &probe,
        "#!/bin/sh\nprintf 'env=%s\\n' \"$JET_PROCESS_TEST\"\nprintf 'cwd=%s\\n' \"$(pwd)\"\nread line\nprintf 'stdin=%s\\n' \"$line\"\n",
    );
    write_executable(&emit, "#!/bin/sh\nprintf 'pipe-ok\\n'\n");
    write_executable(&cat, "#!/bin/sh\ncat\n");
    write_executable(&lines, "#!/bin/sh\nprintf 'line-one\\nline-two\\n'\n");

    let src = format!(
        r#"
use core.process as process
use core.time as time

fn run() {{
    spec :: process.cmd(["{probe}"]).cwd("{work}").env_clear().env("JET_PROCESS_TEST", "ok").stdin(.Capture).stdout(.Capture).stderr(.Capture).timeout(time.seconds(2)).output_limit(10000)
    probe_child :: spec.spawn() ?? panic("spawn failed")
    probe_child.stdin.write("from-stdin\n") ?? panic("write failed")
    result :: probe_child.wait() ?? panic("wait failed")
    print(result.success)
    print(result.code)
    print(result.timed_out)
    print(result.output)

    piped :: process.pipeline([process.cmd(["{emit}"]), process.cmd(["{cat}"])]) ?? panic("pipeline failed")
    print(piped.success)
    print(piped.output)

    child :: process.cmd(["{lines}"]).stdout(.Stream).spawn() ?? panic("spawn failed")
    loop line in child.stdout.lines() {{
        print(line)
    }}
    waited :: child.wait() ?? panic("wait failed")
    print(waited.success)
}}
"#,
        probe = jet_string_path(&probe),
        work = jet_string_path(&work),
        emit = jet_string_path(&emit),
        cat = jet_string_path(&cat),
        lines = jet_string_path(&lines)
    );

    let (code, stdout, stderr) = build_and_run(&dir, "process_api", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("true\n0\nfalse\n"), "{stdout}");
    assert!(stdout.contains("env=ok\n"), "{stdout}");
    assert!(
        stdout.contains(&format!("cwd={}\n", work.display())),
        "{stdout}"
    );
    assert!(stdout.contains("stdin=from-stdin\n"), "{stdout}");
    assert!(stdout.contains("pipe-ok\n"), "{stdout}");
    assert!(stdout.contains("line-one\n"), "{stdout}");
}

#[test]
fn core_time_calendar_zone_and_dst_run() {
    let source_zone = std::env::var_os("TZDIR")
        .map(|dir| PathBuf::from(dir).join("America/New_York"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("/usr/share/zoneinfo/America/New_York"));
    if !source_zone.exists() {
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_time_calendar_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let tzdb = dir.join("tzdb");
    fs::create_dir_all(tzdb.join("America")).unwrap();
    fs::copy(&source_zone, tzdb.join("America/New_York")).unwrap();
    let src = r#"
use core.time as time
use core.time.date as Date

fn run() {
    zone :: time.zone("America/New_York") ?? panic("missing zone")
    local :: time.zoned_local(Date.new(2024, 3, 10), time.local_time(1, 30, 0), zone)
    print(local.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    civil :: local.add_period(time.period_days(1))
    absolute :: local.add_duration(time.hours(24))
    print(civil.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    print(absolute.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    print(local.to_datetime().format_rfc3339())
    parsed :: time.parse_rfc3339("2024-03-10T06:30:00Z") ?? panic("bad parse")
    print(parsed.in_zone(zone).format("yyyy-MM-dd HH:mm:ss VV XXX"))
}
"#;
    let tzdb_env = tzdb.to_string_lossy().into_owned();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "time_calendar",
        src,
        &[("JET_TZDB_DIR", &tzdb_env)],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "2024-03-10 01:30:00 America/New_York -05:00\n2024-03-11 01:30:00 America/New_York -04:00\n2024-03-11 02:30:00 America/New_York -04:00\n2024-03-10T06:30:00Z\n2024-03-10 01:30:00 America/New_York -05:00\n"
    );
}

#[test]
fn core_url_mime_parse_join_query_and_http_url_run() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_url_mime_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.mime as mime
use core.url as url

fn run() {
    base :: url.parse("https://Bücher.example:443/a/./b/../c?x=1#frag") ?? panic("bad url")
    print(base.to_string())
    print(base.host() ?? "none")
    print(base.path())
    print(base.query_pairs()[0][0])
    print(base.query_pairs()[0][1])
    rel :: base.join("../notify?user=ada lovelace&user=grace#done") ?? panic("bad join")
    print(rel.to_string())
    print(rel.path_segments().join("|"))
    print(rel.fragment() ?? "none")
    print(url.query([["user", "ada lovelace"], ["user", "grace"], ["empty", ""]]))
    print(url.percent_encode("a b/c"))
    print(url.percent_decode("a%20b%2Fc") ?? "bad")
    html :: mime.parse("Text/HTML; charset=UTF-8") ?? panic("bad mime")
    print(html.essence())
    print(html.param("charset") ?? "none")
    print(mime.from_extension("png") ?? "none")
    print(mime.extension("image/png") ?? "none")
    png :: mime.parse("image/png") ?? panic("bad mime")
    print(url.data(png, "<h1>Hi</h1>").to_string())
    print(url.file("/tmp/a b.txt").to_string())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "url_mime", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "https://xn--bcher-kva.example:443/a/c?x=1#frag\nxn--bcher-kva.example\n/a/c\nx\n1\nhttps://xn--bcher-kva.example:443/notify?user=ada%20lovelace&user=grace#done\nnotify\ndone\nuser=ada%20lovelace&user=grace&empty=\na%20b%2Fc\na b/c\ntext/html\nUTF-8\nimage/png\npng\ndata:image/png,%3Ch1%3EHi%3C%2Fh1%3E\nfile:///tmp/a%20b.txt\n"
    );
}

#[test]
fn core_http_client_accepts_typed_url_in_codegen() {
    let out = compile_temp(
        "http_url.jet",
        r#"
use core.http.client as http
use core.url as url

fn run() {
    u :: url.parse("https://example.com/a") ?? panic("bad url")
    req :: http.request("GET", u).timeout(1)
}
"#,
    );
    assert!(
        out.rust.contains(".to_string_value()"),
        "typed Url should render to String at HTTP boundary:\n{}",
        out.rust
    );
}

#[test]
fn core_data_typed_csv_group_stats_status_and_plot() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping core.data runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_core",
        r#"
use core.data as data

@Codable
struct Ticket {
    team: String
    minutes: Float
}

@Codable
struct Budget {
    team: String
    owner: String
}

fn run() {
    raw :: "team,minutes\nCore,4.0\nTools,5.0\nCore,8.0\nTools,7.0"
    rows :: data.csv<Ticket>(raw) ?? panic("bad csv")
    budget_raw :: "team,owner\nCore,Ada\nTools,Grace"
    budgets :: data.csv<Budget>(budget_raw) ?? panic("bad budget")
    print(data.count(rows))
    table :: data.table(rows)
    lazy :: data.lazy(table)
    planned :: data.lazy_sort_by(data.lazy_filter(lazy, (t) => t.minutes >= 6.0), (t) => t.team)
    collected :: data.collect(planned)
    print(data.count(table))
    print(data.count(planned))
    print(data.count(data.rows(collected)))
    print(data.plan(planned)[2])
    none: Float? :: None
    maybe_minutes: [Float?] :: [Val(2.0), none, Val(6.0), none]
    series :: data.series(maybe_minutes)
    print(data.count(series))
    print(data.missing_count(series))
    groups :: data.group_mean(rows, (t) => t.team, (t) => t.minutes)
    loop g in groups {
        print("{g.key}:{g.count}:{g.sum}:{g.mean}")
    }
    values :: [2.0, 4.0, 6.0]
    print(data.sum(values))
    print(data.mean(values))
    joined :: data.inner_join(rows, budgets, (t) => t.team, (b) => b.team)
    print(data.bar_text(joined))
    pivot :: data.pivot_sum(rows, (t) => t.team, (t) => if t.minutes >= 6.0 { "long" } else { "short" }, (t) => t.minutes)
    print(data.bar_text(pivot))
    rolling :: data.rolling_mean([2.0, 4.0, 6.0], 2)
    print(rolling[2])
    counts :: data.group_count(rows, (t) => t.team)
    print(data.bar_text(counts))
    print(data.bar_svg(counts).len())
    status :: data.status()
    print("{status[0].step}:{status[0].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data program failed: {stderr}");
    assert_eq!(
        stdout,
        "4\n4\n2\n2\nsort_by\n4\n2\nCore:2:12.0:6.0\nTools:2:12.0:6.0\n12.0\n4.0\nCore | ## 2\nTools | ## 2\nCore|long | # 1\nCore|short | # 1\nTools|long | # 1\nTools|short | # 1\n5.0\nCore | ## 2\nTools | ## 2\n531\ncore.data.csv:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
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

fn run() {
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

fn run() {
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
fn random_distribution_surface_is_deterministic() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping random distribution test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_random_dist_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "random_dist",
        r#"
use core.random as random

fn run() {
    random.seed(7)
    print(random.bool(1.0))
    print(random.float_range(10.0, 20.0) >= 10.0)
    random.seed(11)
    a := random.normal(0.0, 1.0)
    random.seed(11)
    b := random.normal(0.0, 1.0)
    print(a == b)
    print(random.exponential(2.0) >= 0.0)
    items := ["red", "green", "blue"]
    weights := [0.0, 1.0, 0.0]
    print(random.weighted_pick(items, weights) ?? "none")
    print(random.sample(items, 2).len())
    print(random.bytes(4).len())
    rng := random.rng(99)
    print(rng.float_range(1.0, 2.0) >= 1.0)
    print(rng.bool(1.0))
    print(rng.weighted_pick(items, weights) ?? "none")
    print(rng.sample(items, 2).len())
    print(rng.bytes(3).len())
    child := rng.split()
    print(child.int(1, 1))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "random distribution test failed: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\ntrue\ngreen\n2\n4\ntrue\ntrue\ngreen\n2\n3\n1\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_breadth_codecs_share_data_tree() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping encoding breadth test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_encoding_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "encoding_breadth",
        r#"
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn run() {
    data := json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json")
    print(json.canonical(data))
    print(json.events(data).contains("object_start $"))
    rows := jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")
    print(rows.len())
    print(jsonl.to_string(rows).contains("\"a\":1"))
    doc := xml.parse("<r xmlns:h=\"urn:h\"><h:c id=\"7\">ok</h:c></r>") ?? panic("xml")
    print(xml.to_string(doc))
    encoded := cbor.encode(data)
    print(encoded.len() > 0)
    decoded := cbor.decode(encoded) ?? panic("cbor")
    print(json.canonical(decoded))
    bytes: [U8] :: [104, 105]
    u := base64.encode_url(bytes)
    print(u)
    print((base64.decode_url(u) ?? panic("base64url")).len())
    b32 := base32.encode(bytes)
    print(b32)
    print((base32.decode(b32) ?? panic("base32")).len())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "encoding breadth test failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"a\":1,\"b\":2}\ntrue\n2\ntrue\n<r xmlns:h=\"urn:h\"><h:c id=\"7\">ok</h:c></r>\ntrue\n{\"a\":1,\"b\":2}\naGk\n2\nNBUQ====\n2\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn text_unicode_audit_surface_runs() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
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
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping db checked sql test (need rustc)");
        return;
    }
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
    created :: db.migrate(conn, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT, active INTEGER)"
    ]) ?? panic("migrate")
    skipped :: db.migrate(conn, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT, active INTEGER)"
    ]) ?? panic("migrate again")
    id :: 7
    name :: "Ada"
    insert :: sql"INSERT INTO person (id, name, active) VALUES ({id}, {name}, 1)"
    _inserted :: conn.execute(insert.template(), db.params(insert)) ?? panic("insert")
    failed :: db.transaction(conn, "bad batch", [
        "INSERT INTO person (id, name, active) VALUES (8, 'Grace', 1)",
        "INSERT INTO missing_table VALUES (1)"
    ]) ?? 0
    row :: conn.query_one("SELECT id, name, active FROM person WHERE id = ?", [DbValue.Int(7)]) ?? panic("query")
    found :: row ?? panic("missing")
    count :: conn.query_one("SELECT COUNT(*) AS n FROM person", []) ?? panic("count")
    counted :: count ?? panic("missing count")
    print(created)
    print(skipped)
    print(failed)
    print(db.row_int(found, "id") ?? 0)
    print(db.row_text(found, "name") ?? "bad")
    print(db.row_int(found, "active") ?? 0)
    print(db.row_int(counted, "n") ?? 0)
    _closed :: conn.close()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "db checked sql test failed: {stderr}");
    assert_eq!(stdout, "1\n0\n0\n7\nAda\n1\n1\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_fmt_human_formatting_surface_runs() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
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
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
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
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
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
    print(testing.bench_budget("parse", 5000000, () => {}))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.testing helpers failed: {stderr}");
    assert_eq!(
        stdout,
        "fixture\ntrue\ntrue\n2\ntrue\n99\ntrue\ntrue\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("__snapshots__/case.snap")).unwrap(),
        "snap"
    );
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
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
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
fn run() {
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
fn run() {
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
fn run() {
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
fn run() {
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

fn run() {
(sender, ch) : tasks.channel<Int>()
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

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i in 1..1000 {
        dup :: copy sender
        tasks.spawn(take(dup) () => {
            dup.send(1)
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

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i in 1..10000 {
        dup :: copy sender
        tasks.spawn(take(dup) () => {
            dup.send(1)
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

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i in 1..100000 {
        dup :: copy sender
        tasks.spawn(take(dup) () => {
            dup.send(1)
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

fn run() {
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
        .find("pub(crate) fn core_module_items(")
        .expect("core_module_items function not found in CheckerCoreLib/module_items.rs");
    // Find the closing `}` at top-level indent (just after the last arm).
    let fn_body = &src[fn_start..];
    // Collect ALL string literals from match arm heads (handles `"a" | "b" => &[` form too).
    let mut items_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in fn_body.lines() {
        let trimmed = line.trim();
        // A match arm head: `"core.files" => &[` or `"core.log" | "jet.log" => &[`
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
    // internal dispatch keys. Some modules are already canonical end-to-end.
    let ring_names = ["log", "crypto", "http", "regex", "reactive", "db", "plugin"];
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
struct Tagged<K> {
    raw: Int
    #[Skip] marker: K?
}

fn run() {
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
    // (D-MEM1 S6: struct renamed `Id<K>` -> `Tagged<K>` — `Id<T>` is now the
    // reserved `Pool<T>` handle type.)
    assert!(
        rs.contains("impl<K: Clone> user_Encode for user_Tagged<K>"),
        "Tagged's Encode impl must NOT bound K with user_Encode (phantom param)\n{rs}"
    );
    assert!(
        rs.contains("impl<K: Clone> user_Decode for user_Tagged<K>"),
        "Tagged's Decode impl must NOT bound K with user_Decode (phantom param)\n{rs}"
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
struct Tagged<K> {
    raw: Int
    #[Skip] marker: K?
}

fn run() {
    wi :: Wrap<Int>.{ value: 7 }
    print(json.to_string(wi))
    back :: json.decode<Wrap<Int>>("{{\"value\":42}}") ?? panic("bad")
    print(back.value)
    id :: Tagged<Wrap<Int>>.{ raw: 9, marker: None }
    print(json.to_string(id))
    rid :: json.decode<Tagged<Wrap<Int>>>("{{\"raw\":3}}") ?? panic("bad id")
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
fn run() {
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
fn run() {
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

// ── card #131 S1-bridge: hand-written `impl T.Encode` / `impl T.Decode` (D-SERDE2) ──
// A hand codec passes sema and MUST produce Rust rustc accepts (I2). The Jet-facing
// verbs `encode`/`decode` bridge internally to the Rust `user_Encode`/`user_Decode`
// traits' `jet_encode(&self) -> DataTree` / `jet_decode(&DataTree) -> Result<Self, …>`.
// The impl uses a custom wire key (`"email"`, not the field name `addr`) so the round
// trip can only succeed through the HAND methods, never a derive.
#[test]
fn hand_written_encode_decode_round_trips() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping hand_written_encode_decode_round_trips (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_hand_codec_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "hand_codec",
        r#"
use core.encoding.json as json

struct Email { addr: String }

impl Email.Encode {
    fn encode(self) -> DataTree {
        m: [String: DataTree] :: ["email": DataTree.Text(copy self.addr)]
        return DataTree.Object(m)
    }
}

impl Email.Decode {
    fn decode(tree: DataTree) -> Email ? DecodeError {
        f := tree.field("email") ?? DataTree.Text("")
        s := f.text() ?? ""
        return ok(Email.{addr: s})
    }
}

fn run() {
    e := Email.{addr: "a@b.com"}
    s := json.to_string(e)
    print(s)
    back := json.decode<Email>(s) ?? panic("decode failed")
    print(back.addr)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "hand codec round trip failed: {stderr}");
    // Custom wire key proves the hand `encode` ran; `back.addr` proves hand `decode` ran.
    assert_eq!(stdout, "{\"email\":\"a@b.com\"}\na@b.com\n");
    let _ = fs::remove_dir_all(&dir);
}

/// card #131: `DataTree.decode<T>()` dispatches primitive, container,
/// generated, and hand-written Decode implementations through one spelling.
#[test]
fn datatree_decode_dispatches_all_decode_impl_kinds() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_datatree_decode_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
@[Codable]
struct Point { x: Int }
struct Email { addr: String }
impl Email.Decode {
    fn decode(tree: DataTree) -> Email ? DecodeError {
        value := tree.field("address") ?? DataTree.Text("")
        return ok(Email.{ addr: value.text() ?? "" })
    }
}

fn run() {
    i_tree: DataTree := DataTree.Int(41)
    xs_tree: DataTree := DataTree.Array([DataTree.Int(1), DataTree.Int(2)])
    p_tree: DataTree := DataTree.Object(["x": DataTree.Int(7)])
    e_tree: DataTree := DataTree.Object(["address": DataTree.Text("a@b")])
    i := i_tree.decode<Int>() ?? panic("primitive")
    xs := xs_tree.decode<[Int]>() ?? panic("list")
    p := p_tree.decode<Point>() ?? panic("derive")
    e := e_tree.decode<Email>() ?? panic("hand")
    print(i + xs[1] + p.x)
    print(e.addr)
}
"#;
    let out = compile_temp("datatree_decode.jet", src);
    assert!(out.rust.contains("<i64 as user_Decode>::jet_decode"));
    assert!(out.rust.contains("<user_Point as user_Decode>::jet_decode"));
    assert!(out.rust.contains("<user_Email as user_Decode>::jet_decode"));
    let (code, stdout, stderr) = build_and_run(&dir, "datatree_decode", src, &[], None);
    assert_eq!(code, 0, "DataTree.decode dispatch failed: {stderr}");
    assert_eq!(stdout, "50\na@b\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generated_enum_codecs_reenter_jet_pipeline() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_enum_serde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json
@[Codable]
enum Event {
    Idle
    Count(Int)
    Named(name: String, enabled: Bool)
}
fn run() {
    a: Event := .Idle
    b: Event := .Count(3)
    c: Event := .Named.{ name: "x", enabled: true }
    print(json.to_string(a))
    print(json.to_string(b))
    print(json.to_string(c))
    back := json.decode<Event>("{{\"Count\":7}}") ?? panic("decode")
    if back == .Count(n) { print(n) }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "enum_serde", src, &[], None);
    assert_eq!(code, 0, "generated enum codec failed: {stderr}");
    assert_eq!(stdout, "\"Idle\"\n{\"Count\":3}\n{\"Named\":{\"name\":\"x\",\"enabled\":true}}\n7\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SERDE7: internal tags apply uniformly to unit, single-payload, and
/// named-payload variants. Exact JSON plus decode proves the AOT contract.
#[test]
fn generated_internal_tagged_enum_round_trips_every_variant_shape() {
    let dir = std::env::temp_dir().join(format!("jet_tagged_enum_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

@[Codable]
#[Tag("type")]
enum Event {
    Idle
    Count(Int)
    Named(name: String, enabled: Bool)
}

fn run() {
    unit: Event := .Idle
    tuple: Event := .Count(3)
    named: Event := .Named.{ name: "x", enabled: true }
    print(json.to_string(unit))
    print(json.to_string(tuple))
    print(json.to_string(named))
    a := json.decode<Event>("{{\"type\":\"Idle\"}}") ?? panic("unit")
    b := json.decode<Event>("{{\"type\":\"Count\",\"value\":7}}") ?? panic("tuple")
    c := json.decode<Event>("{{\"type\":\"Named\",\"name\":\"y\",\"enabled\":false}}") ?? panic("named")
    print(json.to_string(a))
    print(json.to_string(b))
    print(json.to_string(c))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "tagged_enum", src, &[], None);
    assert_eq!(code, 0, "generated internally tagged enum failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"type\":\"Idle\"}\n{\"type\":\"Count\",\"value\":3}\n{\"type\":\"Named\",\"name\":\"x\",\"enabled\":true}\n{\"type\":\"Idle\"}\n{\"type\":\"Count\",\"value\":7}\n{\"type\":\"Named\",\"name\":\"y\",\"enabled\":false}\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn builtin_codec_expansion_has_no_ast_transplant_or_rust_fallback() {
    let registration = include_str!("../crates/jet-sema/src/Sema/Registration.rs");
    let items = include_str!("../crates/jet-codegen/src/Codegen/Items.rs");
    assert!(registration.contains("impl {}.Encode"));
    assert!(registration.contains("impl {}.Decode"));
    assert!(registration.contains("Some(trigger_span)"));
    assert!(!registration.contains("__JetSerdeCarrier"));
    assert!(!registration.contains("__JetSerdeGenerated"));
    assert!(!registration.contains("trait_impls.extend"));
    assert!(!items.contains("emit_struct_serde"));
    assert!(!items.contains("emit_enum_serde"));
}

/// Card #131: generated struct codecs preserve field-policy behavior while
/// running through ordinary Jet bodies: absent options stay off the wire and
/// computed fields encode through their getter without becoming decode slots.
#[test]
fn generated_struct_codecs_preserve_option_and_computed_fields() {
    let dir = std::env::temp_dir().join(format!("jet_struct_serde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

@[Codable]
struct Record {
    base: Int
    note: String?
    doubled: Int => base * 2
}

fn run() {
    value := Record.{ base: 4, note: None }
    print(json.to_string(value))
    back := json.decode<Record>("{{\"base\":5,\"doubled\":999}}") ?? panic("decode")
    print(back.base)
    print(back.doubled)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_serde", src, &[], None);
    assert_eq!(code, 0, "generated struct codec failed: {stderr}");
    assert_eq!(stdout, "{\"base\":4,\"doubled\":8}\n5\n10\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Derived objects keep declaration order across renamed fields and optional
/// omission. Ordinary maps retain their independent key-ordering behavior.
#[test]
fn generated_struct_encode_preserves_order_with_rename_and_option() {
    let dir = std::env::temp_dir().join(format!("jet_struct_serde_order_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

@[Encode]
struct Wire {
    first: String
    #[Rename("wireSecond")] second: String
    maybe: String?
    last: Int
}

fn run() {
    absent := Wire.{ first: "a", second: "b", maybe: None, last: 4 }
    present := Wire.{ first: "a", second: "b", maybe: Val("c"), last: 4 }
    arbitrary: [String: Int] := ["z": 1, "a": 2]
    print(json.to_string(absent))
    print(json.to_string(present))
    print(json.to_string(arbitrary))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_serde_order", src, &[], None);
    assert_eq!(code, 0, "ordered generated struct codec failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"first\":\"a\",\"wireSecond\":\"b\",\"last\":4}\n{\"first\":\"a\",\"wireSecond\":\"b\",\"maybe\":\"c\",\"last\":4}\n{\"a\":2,\"z\":1}\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131: flatten, rename, and decode defaults are behavior of generated
/// Jet codec bodies, not a hidden Rust-only derive path.
#[test]
fn generated_struct_codecs_preserve_flatten_rename_and_default() {
    let dir = std::env::temp_dir().join(format!("jet_struct_markers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

@[Codable]
struct Inner { x: Int  y: Bool }

@[Codable]
#[RenameAll(camel)]
struct Outer {
    display_name: String
    #[Flatten] inner: Inner
    #[Default(4 + 5)] count: Int
}

fn run() {
    value := Outer.{ display_name: "n", inner: Inner.{ x: 1, y: true }, count: 2 }
    print(json.to_string(value))
    back := json.decode<Outer>("{{\"displayName\":\"m\",\"x\":3,\"y\":false}}") ?? panic("decode")
    print(back.display_name)
    print(back.inner.x)
    print(back.count)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_markers", src, &[], None);
    assert_eq!(code, 0, "generated marker codec failed: {stderr}");
    assert_eq!(stdout, "{\"count\":2,\"displayName\":\"n\",\"x\":1,\"y\":true}\nm\n3\n9\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131 / D-SERDE8: strict unknown-key rejection is emitted as ordinary
/// Jet control flow and carries the offending wire path plus E2412 reason.
#[test]
fn generated_struct_decode_denies_unknown_fields() {
    let dir = std::env::temp_dir().join(format!("jet_struct_deny_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

@[Codable]
#[DenyUnknownFields]
struct Strict { name: String }

fn run() {
    result := json.decode<Strict>("{{\"name\":\"x\",\"extra\":1}}")
    if result == err(e) {
        print(e.path)
        print(e.reason)
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_deny", src, &[], None);
    assert_eq!(code, 0, "generated strict codec failed: {stderr}");
    assert_eq!(stdout, "extra\nE2412: unknown field `extra`\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131: built-in Decode fragments live beside their source type, so a
/// consumer can dispatch through an imported type without entry-local aliases.
/// The nested List/Option/Map fields also prove D-SERDE16's public dispatch.
#[test]
fn generated_decode_dispatches_across_module_boundaries() {
    let dir = std::env::temp_dir().join(format!("jet_serde_module_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lib = r#"
@[Codable]
pub struct Address { pub city: String }

@[Codable]
pub struct Order {
    pub shipping: Address
    pub quantities: [Int]
    pub coupon: String?
    pub labels: [String: Int]
}

"#;
    let main = r#"
use core.encoding.json as json
use orders

fn run() {
    order := json.decode<orders.Order>("{{\"shipping\":{{\"city\":\"Paris\"}},\"quantities\":[2,3],\"coupon\":null,\"labels\":{{\"fragile\":1}}}}") ?? panic("decode")
    print(json.to_string(order))
}
"#;
    let (code, stdout, stderr) = build_and_run_multi(
        &dir,
        "serde_module",
        "main.jet",
        &[("main.jet", main), ("orders.jet", lib)],
    );
    assert_eq!(code, 0, "cross-module generated decode failed: {stderr}");
    assert_eq!(stdout, "{\"shipping\":{\"city\":\"Paris\"},\"quantities\":[2,3],\"labels\":{\"fragile\":1}}\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-METADERIVE1 orphan law: expansion is legal when either derive provider
/// or target type is entry-local. Both directions must generate usable code.
#[test]
fn user_derive_orphan_rule_allows_either_local_side() {
    let dir = std::env::temp_dir().join(format!("jet_derive_orphan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lib = r#"
derive T.RemoteLabel {
    info :: T.reflect()
    name :: info.name
    emit("impl $name {{ fn remote_label(self) -> String {{ return \"remote:$name\" }} }}")
}

@LocalLabel
pub struct RemoteType { pub value: Int }

pub fn remote_type_label() -> String {
    value := RemoteType.{ value: 2 }
    return value.local_label()
}
"#;
    let main = r#"
use labels

derive T.LocalLabel {
    info :: T.reflect()
    name :: info.name
    emit("impl $name {{ pub fn local_label(self) -> String {{ return \"local:$name\" }} }}")
}

@RemoteLabel
struct LocalType { value: Int }

fn run() {
    local := LocalType.{ value: 1 }
    print(local.remote_label())
    print(labels.remote_type_label())
}
"#;
    let (code, stdout, stderr) = build_and_run_multi(
        &dir,
        "derive_orphan",
        "main.jet",
        &[("main.jet", main), ("labels.jet", lib)],
    );
    assert_eq!(code, 0, "local-orphan derive dispatch failed: {stderr}");
    assert_eq!(stdout, "remote:LocalType\nlocal:RemoteType\n");
    let _ = fs::remove_dir_all(&dir);
}

/// #495 / I2: a field read from a bare (`Read`) parameter is still rooted in
/// the borrowed parameter. The explicit `copy` required by E0209 must produce
/// owned values for both shallow and nested fields, compile through rustc, and
/// run with the expected data.
#[test]
fn consuming_core_constructor_copies_borrowed_field_explicitly() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_borrowed_field_copy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "core_borrowed_field_copy",
        r#"
use core.encoding.json as json

struct Address { text: String }
struct Email { addr: String, nested: Address, items: [Address] }

fn pick() -> Int {
    return 0
}

fn encoded(e: Email, i: Int) -> String {
    shallow := DataTree.Text(copy e.addr)
    nested := DataTree.Text(copy e.nested.text)
    indexed := DataTree.Text(copy e.items[0].text)
    computed := DataTree.Text(copy e.items[i + 1].text)
    called := DataTree.Text(copy e.items[pick()].text)
    parenthesized := DataTree.Text(copy e.items[-(-i)].text)
    conditional := DataTree.Text(copy e.items[if i == 0 { 0 } else { 1 }].text)
    return "{json.to_string(shallow)}|{json.to_string(nested)}|{json.to_string(indexed)}|{json.to_string(computed)}|{json.to_string(called)}|{json.to_string(parenthesized)}|{json.to_string(conditional)}"
}

fn slice_data(xs: [DataTree]) -> DataTree {
    return DataTree.Array(xs[0..1])
}

fn run() {
    e := Email.{addr: "a@b.com", nested: Address.{text: "inside"}, items: [Address.{text: "zero"}, Address.{text: "item"}]}
    sliced := slice_data([DataTree.Text("slice0"), DataTree.Text("slice1")])
    print("{encoded(e, 0)}|{json.to_string(sliced)}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "explicit field copy failed to compile/run: {stderr}");
    assert_eq!(
        stdout,
        "\"a@b.com\"|\"inside\"|\"zero\"|\"item\"|\"zero\"|\"zero\"|\"zero\"|[\"slice0\",\"slice1\"]\n"
    );
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
fn run() {
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
fn run() {
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

// ── D-MIGRATE3=A / D-MIGRATE4=A: decode-time migration transparency ──────────
// `decode_traced<T>` sits beside `decode<T>` on every codec that shares the
// decode machinery. `MigrationStatus.migrated` is false and `.from`/`.steps`
// are empty both for a plain type (no `@PublishedSchema`) and for a
// `@PublishedSchema` type decoding data already shaped like the current
// struct (the "fresh" case). This test covers those non-migrated cases; the
// migrated paths (D-MIGRATE4 runtime chain) are `decode_traced_migration_*`
// below.
#[test]
fn decode_traced_json_plain_and_published_fresh() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_json_plain_and_published_fresh (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_decode_traced_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "decode_traced_json",
        r#"
use core.encoding.json as json

@[Codable]
struct Point { x: Int  y: Int }

@[PublishedSchema, Codable]
struct UserRecord { id: Int  display_name: String }

migration UserRecord {
    rename name -> display_name
}

fn run() {
    // Plain (non-@PublishedSchema) type: decode_traced still works.
    p :: json.decode_traced<Point>("{{\"x\":1,\"y\":2}}") ?? panic("bad point")
    print(p.value.x)
    print(p.migration.migrated)
    print(p.migration.from)
    print(p.migration.steps.len())

    // @PublishedSchema type, fresh data (matches the current shape exactly):
    // still reports migrated: false — nothing runtime-converted it.
    r :: json.decode_traced<UserRecord>("{{\"id\":1,\"display_name\":\"Ada\"}}") ?? panic("bad user")
    print(r.value.display_name)
    print(r.migration.migrated)

    // decode<T> (untraced) is untouched: same call, no DecodeResult wrapper.
    plain :: json.decode<Point>("{{\"x\":3,\"y\":4}}") ?? panic("bad plain")
    print(plain.x)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "decode_traced json program failed: {stderr}");
    assert_eq!(stdout, "1\nfalse\n\n0\nAda\nfalse\n3\n");
    let _ = fs::remove_dir_all(&dir);
}

// A second codec exercising the same DecodeResult/MigrationStatus plumbing —
// proves the traced method isn't a json-only special case (D-ENC1 shares the
// decode machinery across json/csv/toml/yaml).
#[test]
fn decode_traced_toml_and_csv() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_toml_and_csv (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_decode_traced_toml_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "decode_traced_toml",
        r#"
use core.encoding.toml as toml
use core.encoding.csv as csv

@[Codable]
struct Config { port: Int }

fn run() {
    r :: toml.decode_traced<Config>("port = 8080\n") ?? panic("bad toml")
    print(r.value.port)
    print(r.migration.migrated)

    cr :: csv.decode_traced<Config>("port\n8080\n9090\n") ?? panic("bad csv")
    print(cr.value.len())
    print(cr.value[0].port)
    print(cr.migration.migrated)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "decode_traced toml/csv program failed: {stderr}");
    assert_eq!(stdout, "8080\nfalse\n2\n8080\nfalse\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── D-MIGRATE4=A: the runtime migration chain ────────────────────────────────
// Decoding a `@PublishedSchema` type tries the current shape first; on
// mismatch it detects which historical shape the data's field-name set
// matches (newest matching version preferred) and walks the migration blocks
// forward, oldest→current. `decode_traced` reports `from` + `steps`
// ("v1->v2" style, one per block applied); plain `decode` applies the same
// chain silently. Data matching no shape keeps the ordinary decode error.
// This covers: a two-block chain (v1→v3: remove + rename + `change … via`),
// the newest-match rule (v2 data walks one step, not two), the silent plain
// `decode`, and garbage still erroring.
#[test]
fn decode_traced_migration_chain() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_migration_chain (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_migrate_chain_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "migrate_chain",
        r#"
use core.encoding.json as json

@Codable
struct Rank { value: Int }

// v1: { legacy_id, name, score: Int }
// v2: { name, score: Int }     (block 1: remove legacy_id)
// v3: { title, score: Rank }   (block 2: rename + change via)
@[PublishedSchema, Codable]
struct Profile {
    title: String
    score: Rank
}

migration Profile {
    remove legacy_id
}

migration Profile {
    rename name -> title
    change score: Int -> Rank via { (n) => Rank.{ value: n } }
}

fn run() {
    // v1 data walks both steps.
    v1 :: "{{\"legacy_id\": 9, \"name\": \"Ada\", \"score\": 95}}"
    r :: json.decode_traced<Profile>(v1) ?? panic("bad v1")
    print(r.value.title)
    print(r.value.score.value)
    print(r.migration.migrated)
    print(r.migration.from)
    print(r.migration.steps.len())
    print(r.migration.steps[0])
    print(r.migration.steps[1])

    // v2 data matches the newer historical shape — one step, not two.
    v2 :: "{{\"name\": \"Grace\", \"score\": 7}}"
    r2 :: json.decode_traced<Profile>(v2) ?? panic("bad v2")
    print(r2.migration.from)
    print(r2.migration.steps.len())

    // Plain decode applies the same chain silently.
    p :: json.decode<Profile>(v1) ?? panic("bad plain")
    print(p.title)
    print(p.score.value)

    // Data matching no shape keeps the ordinary decode error.
    g :: json.decode<Profile>("{{\"nonsense\": 1}}") ?? Profile.{ title: "rejected", score: Rank.{ value: 0 } }
    print(g.title)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "migration chain program failed: {stderr}");
    assert_eq!(
        stdout,
        "Ada\n95\ntrue\nv1\n2\nv1->v2\nv2->v3\nv2\n1\nAda\n95\nrejected\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

// D-MIGRATE4 across codecs (D-ENC1: one decode machinery): an `add … = default`
// migration fills old records in toml and csv exactly as in json. The csv case
// also proves per-row application (every row of an old-header file migrates,
// the batch-level status reports it once).
#[test]
fn decode_traced_migration_toml_and_csv() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_migration_toml_and_csv (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_migrate_codecs_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "migrate_codecs",
        r#"
use core.encoding.toml as toml
use core.encoding.csv as csv

@[PublishedSchema, Codable]
struct Config {
    port: Int
    host: String
}

migration Config {
    add host: String = "localhost"
}

fn run() {
    t :: toml.decode_traced<Config>("port = 8080\n") ?? panic("bad toml")
    print(t.value.host)
    print(t.migration.migrated)
    print(t.migration.from)

    c :: csv.decode_traced<Config>("port\n1\n2\n") ?? panic("bad csv")
    print(c.value.len())
    print(c.value[1].host)
    print(c.migration.migrated)
    print(c.migration.steps[0])
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "migration codec program failed: {stderr}");
    assert_eq!(stdout, "localhost\ntrue\nv1\n2\nlocalhost\ntrue\nv1->v2\n");
    let _ = fs::remove_dir_all(&dir);
}

// D-MIGRATE4 zero cost: a type without migration blocks — published or not —
// gets NO runtime chain code: no step functions, no per-type
// `jet_decode_traced` override. Compile-only (asserts on the generated Rust).
#[test]
fn migration_free_types_emit_no_runtime_chain() {
    let src = r#"
use core.encoding.json as json

@Codable
struct Point { x: Int  y: Int }

@[PublishedSchema, Codable]
struct UserRecord { id: Int  display_name: String }

fn run() {
    p :: json.decode<Point>("{{\"x\":1,\"y\":2}}") ?? panic("bad")
    print(p.x)
    u :: json.decode_traced<UserRecord>("{{\"id\":1,\"display_name\":\"Ada\"}}") ?? panic("bad")
    print(u.value.id)
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_migrate_free_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("migration_free.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !out.rust.contains("jet_migrate_step_"),
        "no step functions may be emitted for migration-free types"
    );
    // The only `jet_decode_traced` definitions are the prelude's (the trait
    // default) — no per-type override in the user section.
    let user_section = out
        .rust
        .split("impl user_Decode for user_")
        .skip(1)
        .collect::<String>();
    assert!(
        !user_section.contains("fn jet_decode_traced"),
        "no per-type jet_decode_traced override may be emitted for migration-free types"
    );
}

#[test]
fn perf_static_api_lowers_to_core_helpers() {
    let out = compile_temp(
        "perf_static.jet",
        r#"
fn run() -> Void ? {
    print(Perf.default_fidelity())
    Perf.override_fidelity(0.25)?
    print(Perf.fidelity())
    Perf.reset_fidelity()
}
"#,
    );
    assert!(out.rust.contains("jet_perf_default_fidelity()"));
    assert!(out.rust.contains("jet_perf_override_fidelity(0.25"));
    assert!(out.rust.contains("jet_perf_fidelity()"));
    assert!(out.rust.contains("jet_perf_reset_fidelity()"));
}

#[test]
fn perf_set_fidelity_alias_is_not_exported() {
    let src = r#"
use core.perf as Perf

fn run() -> Void ? {
    Perf.set_fidelity(0.25)?
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_corelib_perf_alias_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("perf_alias.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let diags = jet::compile_with_path(src, &shown).expect_err("set_fidelity alias must not exist");
    let rendered = jet::render_diagnostics(&shown, src, &diags);
    assert!(
        rendered.contains("set_fidelity"),
        "diagnostic should name retired alias, got:\n{rendered}"
    );
    assert!(
        rendered.contains("has no item"),
        "diagnostic should reject retired alias, got:\n{rendered}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn perf_override_is_range_checked_and_resettable() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping perf runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_perf_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "perf_runtime",
        r#"
use core.perf as Perf

fn run() -> Void ? {
    print(Perf.default_fidelity())
    Perf.override_fidelity(0.25)?
    print(Perf.fidelity())
    Perf.reset_fidelity()
    print(Perf.fidelity())
    Perf.override_fidelity(1.25)?
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 1, "out-of-range override should fail");
    assert_eq!(stdout, "1.0\n0.25\n1.0\n");
    assert!(
        stderr.contains("core.perf.Perf.override_fidelity needs 0.0 through 1.0"),
        "range error should be in Jet runtime terms, got {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn option_zip_and_lift2_combinators() {
    // D-HOLE1: `.zip`/`Option.lift2` — both present -> a present result; either
    // absent -> `None`. No general "hole" type; these are plain library combinators
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
fn run() {
    both_a: Float? :: Val(2.0)
    both_b: Float? :: Val(5.0)
    print(both_a.zip(both_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_a, both_b))

    a_only: Float? :: Val(2.0)
    b_missing: Float? :: None
    print(a_only.zip(b_missing).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, a_only, b_missing))

    both_missing_a: Float? :: None
    both_missing_b: Float? :: None
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

#[test]
fn event_scope_subscribe_once_priority_and_hook_run() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping event runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_event_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "event_runtime",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    ev :: event.with_policy<Int>(event.policy_async(2))
    sub :: ev.on(scope, (n) => { print("low {n}") })
    ev.on_priority(scope, 10, (n) => { print("high {n}") })
    ev.once(scope, (n) => { print("once {n}") })
    print(ev.emit_async(1).summary())
    sub.unsubscribe()
    print(ev.emit(2).summary())
    print(scope.active_count())

    hook :: event.hook<Int, String>("base")
    hook.on(scope, (n) => "seen {n}")
    print(hook.run(7, "fallback"))
    scope.cancel()
    print(hook.run(8, "fallback"))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "event runtime failed: {stderr}");
    assert_eq!(
        stdout,
        "high 1\nlow 1\nonce 1\nevent delivered=3 queued=1 dropped=0\nhigh 2\nevent delivered=1 queued=1 dropped=0\n1\nseen 7\nfallback\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn event_sync_dispatch_handles_mutation_reentrancy_and_owner_drop() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_event_hostile_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "event_sync_hostile",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    ev :: event.new<Int>()
    late :: ev.on(scope, (n) => { print("late {n}") })
    ev.on_priority(scope, 10, (n) => { print("killer {n}"); late.unsubscribe() })
    print(ev.emit(1).summary())
    print("listeners={ev.listener_count()}")

    additions :: event.scope()
    growing :: event.new<Int>()
    growing.on(additions, (n) => {
        print("root {n}")
        _ :: growing.on(additions, (m: Int) => { print("added {m}") })
    })
    print(growing.emit(1).summary())
    print(growing.emit(2).summary())

    nested_scope :: event.scope()
    nested :: event.new<Int>()
    nested.once(nested_scope, (n) => {
        print("once {n}")
        if n == 1 { nested.emit(2) }
    })
    print(nested.emit(1).summary())
    print("nested-listeners={nested.listener_count()}")

    owned :: event.new<Int>()
    if true {
        owner :: event.scope()
        owned.on(owner, (n) => { print("leaked {n}") })
    }
    print(owned.emit(9).summary())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "hostile event runtime failed: {stderr}");
    assert_eq!(
        stdout,
        "killer 1\nevent delivered=1 queued=0 dropped=0\nlisteners=1\nroot 1\nevent delivered=1 queued=0 dropped=0\nroot 2\nadded 2\nevent delivered=2 queued=0 dropped=0\nonce 1\nevent delivered=1 queued=0 dropped=0\nnested-listeners=0\nevent delivered=0 queued=0 dropped=0\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn solve_solver_records_bool_constraints_in_order() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping solver runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_solve_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "solve_runtime",
        r#"
use core.solve as Solve

fn run() {
    solver := Solve.Solver.new(42)
    solver.require(1 + 1 == 2)
    solver.require(2 * 3 == 5)
    solver.require(true)
    print(solver.status())
    print(solver.failure_count())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "failed\n1\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn solve_require_needs_mutable_solver() {
    let src = r#"
use core.solve as Solve

fn run() {
    solver :: Solve.Solver.new(1)
    solver.require(true)
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "solver.require on immutable solver must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn solve_solver_type_name_is_reserved() {
    let src = r#"
struct Solver { value: Int }

fn run() {}
"#;
    let diags = jet::compile(src).expect_err("Solver is a reserved Core handle name");
    assert!(
        diags.iter().any(|d| d.code == "E0106"),
        "expected E0106, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn solve_constructor_is_static_only() {
    let src = r#"
use core.solve as Solve

fn run() {
    solver := Solve.Solver.new(1)
    solver.new(2)
}
"#;
    let diags = jet::compile(src).expect_err("solver.new must not be an instance method");
    assert!(
        !diags.is_empty(),
        "expected a diagnostic for instance constructor"
    );
}

#[test]
fn game_scene_asset_registration_needs_mutable_scene() {
    let src = r#"
use core.game as game

fn run() {
    scene :: game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
}
"#;
    let diags = jet::compile(src).expect_err("asset registration must need edit access");
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_run_needs_mutable_scene_lvalue() {
    let src = r#"
use core.game as game

fn run() {
    print(game.run(game.Scene.new("arcade")))
}
"#;
    let diags = jet::compile(src).expect_err("game.run must reject temporary scene");
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_run_rejects_transposed_labels() {
    let src = r#"
use core.game as game

fn run() {
    scene := game.Scene.new("arcade")
    replay :: game.Replay.record("runs/demo.jreplay")
    backend :: game.Backend.headless()
    print(game.run(scene, backend: backend, replay))
}
"#;
    let diags = jet::compile(src).expect_err("game.run labels must match positional shape");
    assert!(
        diags.iter().any(|d| d.code == "E0125"),
        "expected E0125, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_headless_scene_replay_transcript_is_deterministic() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_game_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "game_headless",
        r#"
use core.game as game

struct Position { x: Int }
struct Velocity { dx: Int }

fn run() {
    scene := game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
    scene.assets.sound("assets/jump.wav") ?? panic("sound")
    scene.input.bind("jump", "Space")
    scene.budgets.set(game.Budgets.new(16, 96, 256, 4))
    scene.component<Position>()
    scene.component<Velocity>()
    print("query {scene.query<Position, Velocity>().len()}")
    scene.on_frame((frame) => {
        if frame.input.pressed("jump") {
            print("hook jump {frame.index}")
        }
    })
    replay :: game.Replay.record("runs/demo.jreplay")
    print(game.run(scene, replay: replay))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "query 1\nhook jump 1\nscene:arcade\nbackend:headless/none/none\nreplay:runs/demo.jreplay\nassets:image:assets/player.png,sound:assets/jump.wav\ninput:jump=Space\ncomponents:Position,Velocity\nbudgets:frame=16ms,memory=96mb,assets=256kb,draws=4\nframe:0 input:none\nframe:1 input:jump\nframe:2 input:none\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}
