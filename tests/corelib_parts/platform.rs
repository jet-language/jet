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

struct Plain {
    value: String
}

fn identity(value: ^Plain) => Plain {
    return value
}

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

#Codable
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
    assert!(
        out.rust.contains("jet_enc_csv_decode::<__jet_Ticket>"),
        "core.data.csv must lower its sema-owned list element type exactly:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("jet_enc_csv_decode::<Vec<__jet_Ticket>>"),
        "core.data.csv nested its list result at the runtime boundary:\n{}",
        out.rust
    );
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
    timeout :: Duration.seconds(2) ?? panic("duration")
    spec :: process.cmd(["{probe}"]).cwd("{work}").env_clear().env("JET_PROCESS_TEST", "ok").stdin(.Capture).stdout(.Capture).stderr(.Capture).timeout(timeout).output_limit(10000)
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
    loop line, child.stdout.lines() {{
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

/// D-PROCESS-SESSION1=A (#1181): `.terminal()` is the one opt-in for a
/// terminal-backed session, and it lives on the same `ProcessSpec`. Argv
/// execution with no terminal stays the default. Unix run/spawn use a real PTY;
/// pipeline stages reject terminal specs rather than coercing them to pipes.
#[cfg(unix)]
#[test]
fn core_process_terminal_uses_unix_pty_for_run_and_spawn() {
    let dir = std::env::temp_dir().join(format!("jet_core_process_terminal_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.process as process

fn run() {
    plain :: process.cmd(["echo", "plain-ok"]).stdout(.Capture).run() ?? panic("default run failed")
    print(plain.output.trim())

    run_result :: process.cmd(["printf", "run-ok"]).terminal().run() ?? panic("terminal run failed")
    print(run_result.output.contains("run-ok"))

    child :: process.cmd(["printf", "spawn-ok"]).terminal().spawn() ?? panic("terminal spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize.{ cols: 100, rows: 30 }) ?? panic("resize failed")
            print("spawn: session")
        }
        .None -> { print("spawn: no session") }
    }
    waited :: child.wait() ?? panic("terminal wait failed")
    print(waited.output.contains("spawn-ok"))

    if process.pipeline([process.cmd(["echo", "a"]), process.cmd(["cat"]).terminal()]) == {
        .Ok(_) -> { print("pipeline: accepted") }
        .Err(_) -> { print("pipeline: refused") }
    }
    if process.cmd([]).terminal().run() == {
        .Ok(_) -> { print("empty: accepted") }
        .Err(e) -> {
            if e == {
                .InvalidInput(_) -> { print("empty: invalid") }
                else -> { print("empty: wrong error") }
            }
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.starts_with("plain-ok\n"), "{stdout}");
    assert!(stdout.contains("true\nspawn: session\ntrue\npipeline: refused\nempty: invalid\n"), "{stdout}");
    // The production path carries the native PTY primitive into the emitted
    // program; this guards against a test-only or pipe fallback.
    let compiled = compile_temp("process_terminal_text.jet", src);
    assert!(
        compiled.rust.contains("posix_openpt"),
        "the Unix terminal path must include the native PTY backend"
    );
}

/// D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D (#1181): the beginner and expert
/// forms share one ProcessSpec. Stable host facts advertise the Unix PTY and a
/// policy controls the initial terminal size and mode.
#[cfg(unix)]
#[test]
fn core_process_terminal_policy_and_capabilities_are_typed_and_resizable() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_terminal_policy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.process as process

fn run() {
    policy :: TerminalPolicy.{
        size: TerminalSize.{ cols: 120, rows: 40 },
        mode: .Raw
    }
    plan :: process.cmd(["echo", "hi"]).terminal(policy)
    facts :: plan.capabilities()
    print(facts.has(TerminalFact.terminal))
    print(facts.has(TerminalFact.resize))
    print(facts.has(TerminalFact.raw))
    print(facts.has("preview_x"))
    if plan.run() == {
        .Ok(_) -> { print("terminal:ok") }
        .Err(_) -> { print("terminal:unavailable") }
    }
    child :: process.cmd(["echo", "plain"]).stdout(.Capture).spawn() ?? panic("spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize.{ cols: 80, rows: 24 }) ?? panic("resize failed")
            print("plain child unexpectedly has terminal")
        }
        .None -> { print("plain child has no terminal") }
    }
    waited :: child.wait() ?? panic("wait failed")
    print(waited.output.trim())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal_policy", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\nfalse\nterminal:ok\nplain child has no terminal\nplain\n"
    );

    let typo = jet::compile(
        r#"use core.process as process
fn run() {
    facts :: process.cmd(["echo", "x"]).capabilities()
    print(facts.has(TerminalFact.reszie))
}
"#,
    )
    .expect_err("stable fact typos must fail in sema");
    assert!(
        typo.iter().any(|diag| {
            diag.code == "E0302"
                && diag.what.contains("`TerminalFact` has no key `reszie`")
                && diag.fix.contains("`TerminalFact.resize`")
        }),
        "{typo:?}"
    );

    let preview_typo = jet::compile(
        r#"use core.process as process
fn run() {
    facts :: process.cmd(["echo", "x"]).capabilities()
    print(facts.has("reszie"))
}
"#,
    )
    .expect_err("close preview-string typos must suggest the stable fact");
    assert!(
        preview_typo.iter().any(|diag| {
            diag.code == "E0302"
                && diag.what.contains("`reszie` looks like `resize`")
                && diag.fix.contains("`TerminalFact.resize`")
        }),
        "{preview_typo:?}"
    );

    let plain_child_terminal = jet::compile(
        r#"use core.process as process
fn run() {
    child :: process.cmd(["echo", "plain"]).spawn() ?? panic("spawn failed")
    child.terminal.resize(TerminalSize.{ cols: 80, rows: 24 })
}
"#,
    )
    .expect_err("a plain child must not expose a TerminalSession");
    assert!(
        plain_child_terminal
            .iter()
            .any(|diag| {
                diag.code == "E0311"
                    && diag.what
                        == "`.resize()` needs `TerminalSession`, not `TerminalSession?`"
                    && diag.fix.contains("session.resize(size)")
            }),
        "{plain_child_terminal:?}"
    );
}

#[cfg(unix)]
#[test]
fn core_process_sh_typed_text_keeps_each_hole_one_argv_item() {
    let dir = std::env::temp_dir().join(format!("jet_core_process_sh_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "process_sh_typed_text",
        r#"
use core.process as process

fn run() {
    hostile :: "two words;*.jet"
    expected :: Sh.{"printf <%s> {hostile}"}
    first :: process.run(expected) ?? panic("typed-head command failed")
    print(first.output)

    second :: process.run(Sh.{"printf [%s] {hostile}"}) ?? panic("second typed-head failed")
    print(second.output)

    audited :: Sh.raw("printf raw")
    third :: process.run(audited) ?? panic("raw command failed")
    print(third.output)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "<two words;*.jet>\n[two words;*.jet]\nraw\n");
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
use core.time.date as date

fn run() {
    zone :: time.zone("America/New_York") ?? panic("missing zone")
    local :: time.zoned_local(date.new(2024, 3, 10), time.time(1, 30, 0), zone)
    print(local.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    civil :: local.add_period(time.period_days(1))
    day :: Duration.hours(24) ?? panic("duration")
    absolute :: local.add_duration(day)
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


