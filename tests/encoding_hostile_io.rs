//! D-ENCSTREAM-SURFACE1 hostile streaming I/O matrix (card #711).
//! Exercises the std-only encoding FileReader/FileWriter seam across JSON,
//! JSONL, CSV, and CBOR without changing the public Jet surface.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn build_and_run(
    dir: &PathBuf,
    name: &str,
    src: &str,
    env: &[(&str, &str)],
) -> (i32, String, String) {
    let path = dir.join(format!("{name}.jet"));
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
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
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
    let run = cmd.output().unwrap();
    (
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

fn hostile_env(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![("JET_ENC_HOSTILE_IO".to_string(), "1".to_string())];
    env.extend(
        extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string())),
    );
    env
}

fn run_with_env(
    dir: &PathBuf,
    name: &str,
    src: &str,
    env: &[(String, String)],
) -> (i32, String, String) {
    let path = dir.join(format!("{name}.jet"));
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
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
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
    let run = cmd.output().unwrap();
    (
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

fn json_reader_fixture(input_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    input :: files.open("{input_path}") ?? panic("open")
    reader :: json.reader(^input) ?? panic("reader")
    count := 0
    loop count < 32 {{
        result :: reader.next()
        if result == {{
            Ok(maybe) -> {{
                if maybe == None {{ break }}
                count++
            }}
            Err(first) -> {{
                again :: reader.next()
                if again == {{
                    Ok(_) -> {{ print("not-latched") }}
                    Err(second) -> {{
                        print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
    print(count)
    print("eof")
}}
"#
    )
}

fn json_writer_fixture(output_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    output :: files.create("{output_path}") ?? panic("create")
    writer :: json.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("end")
    if writer.finish() == {{
        Ok(_) -> {{ print("finish-ok") }}
        Err(first) -> {{
            again :: writer.finish()
            if again == {{
                Ok(_) -> {{ print("finish-not-latched") }}
                    Err(second) -> {{
                        print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                    }}
            }}
        }}
    }}
    print(files.read("{output_path}") ?? panic("read"))
}}
"#
    )
}

fn jsonl_reader_fixture(input_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    input :: files.open("{input_path}") ?? panic("open")
    reader :: jsonl.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("first")
    if first == {{
        Val(v) -> print(v.text() ?? "bad")
        None -> print("none")
    }}
    second :: reader.next() ?? panic("second")
    if second == {{
        Val(v) -> print(v.int() ?? -1)
        None -> print("none2")
    }}
    eof :: reader.next() ?? panic("eof")
    if eof == None {{ print("eof") }} else {{ print("bad-eof") }}
}}
"#
    )
}

fn malformed_reader_fixture(codec: &str, input_path: &str) -> String {
    let reader = match codec {
        "json" => "json.reader(^input)",
        "jsonl" => "jsonl.reader(^input)",
        "csv" => "csv.reader(^input)",
        "cbor" => "cbor.reader(^input)",
        _ => unreachable!(),
    };
    format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    input :: files.open("{input_path}") ?? panic("open")
    reader :: {reader} ?? panic("reader")
    loop {{
        result :: reader.next()
        if result == {{
            Ok(maybe) -> {{
                if maybe == None {{ print("eof-missed"); break }}
            }}
            Err(first) -> {{
                again :: reader.next()
                if again == {{
                    Ok(_) -> {{ print("not-latched") }}
                    Err(second) -> {{
                        print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    )
}

fn jsonl_writer_fixture(output_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    output :: files.create("{output_path}") ?? panic("create")
    writer :: jsonl.writer(^output) ?? panic("writer")
    writer.write(DataTree.Text("alpha")) ?? panic("write")
    writer.write(DataTree.Int(7)) ?? panic("write2")
    writer.finish() ?? panic("finish")
    print(files.read("{output_path}") ?? panic("read"))
}}
"#
    )
}

fn csv_reader_fixture(input_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn run() {{
    input :: files.open("{input_path}") ?? panic("open")
    reader :: csv.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("first")
    if first == {{ Val(row) -> {{ print(row[0]); print(row[1]) }} None -> print("none") }}
    eof :: reader.next() ?? panic("eof")
    if eof == None {{ print("eof") }} else {{ print("bad") }}
}}
"#
    )
}

fn csv_writer_fixture(output_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn run() {{
    output :: files.create("{output_path}") ?? panic("create")
    writer :: csv.writer(^output) ?? panic("writer")
    writer.write(["x", "y"]) ?? panic("write")
    writer.finish() ?? panic("finish")
    print(files.read("{output_path}") ?? panic("read"))
}}
"#
    )
}

fn cbor_reader_fixture(input_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    input :: files.open("{input_path}") ?? panic("open")
    reader :: cbor.reader(^input) ?? panic("reader")
    count := 0
    loop count < 16 {{
        result :: reader.next()
        if result == {{
            Ok(maybe) -> {{
                if maybe == None {{ break }}
                count++
            }}
            Err(first) -> {{
                again :: reader.next()
                if again == {{
                    Ok(_) -> {{ print("not-latched") }}
                    Err(second) -> {{
                        print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
    print(count)
    print("eof")
}}
"#
    )
}

fn cbor_writer_fixture(output_path: &str) -> String {
    format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    output :: files.create("{output_path}") ?? panic("create")
    writer :: cbor.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(3)) ?? panic("int")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("end")
    if writer.finish() == {{
        Ok(_) -> {{ print("finish-ok") }}
        Err(first) -> {{
            again :: writer.finish()
            if again == {{
                Ok(_) -> {{ print("not-latched") }}
                    Err(second) -> {{
                        print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                    }}
            }}
        }}
    }}
    writer.finish() ?? panic("finish")
    print("done")
}}
"#
    )
}

#[test]
fn encoding_hostile_one_byte_reads_match_baseline_for_all_codecs() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping hostile one-byte read matrix (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_read1_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let json_in = dir.join("json.in");
    fs::write(&json_in, r#"{"a":[1,true]}"#).unwrap();
    let jsonl_in = dir.join("jsonl.in");
    fs::write(&jsonl_in, "\"one\"\n7\n").unwrap();
    let csv_in = dir.join("csv.in");
    fs::write(&csv_in, "p,q\r\n").unwrap();
    let cbor_in = dir.join("cbor.in");
    fs::write(&cbor_in, [0x82, 0x01, 0x02]).unwrap();

    let cases: [(&str, String, &str); 4] = [
        ("json", json_reader_fixture(&json_in.to_string_lossy().replace('\\', "\\\\")), "7\neof\n"),
        ("jsonl", jsonl_reader_fixture(&jsonl_in.to_string_lossy().replace('\\', "\\\\")), "one\n7\neof\n"),
        ("csv", csv_reader_fixture(&csv_in.to_string_lossy().replace('\\', "\\\\")), "p\nq\neof\n"),
        ("cbor", cbor_reader_fixture(&cbor_in.to_string_lossy().replace('\\', "\\\\")), "4\neof\n"),
    ];

    for (name, source, expected) in cases {
        let (base_code, base_out, base_err) = build_and_run(&dir, &format!("{name}_base"), &source, &[]);
        assert_eq!((base_code, base_err.as_str()), (0, ""), "{name} baseline stderr");
        assert_eq!(base_out, expected, "{name} baseline stdout");

        let (host_code, host_out, host_err) = run_with_env(
            &dir,
            &format!("{name}_read1"),
            &source,
            &hostile_env(&[("JET_ENC_HOSTILE_READ_ONE", "1")]),
        );
        assert_eq!((host_code, host_err.as_str()), (0, ""), "{name} hostile stderr");
        assert_eq!(host_out, expected, "{name} one-byte read drift");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_hostile_short_writes_match_baseline_for_all_codecs() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping hostile short-write matrix (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_write1_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let json_out = dir.join("json.out");
    let jsonl_out = dir.join("jsonl.out");
    let csv_out = dir.join("csv.out");
    let cbor_out = dir.join("cbor.out");

    let cases: [(&str, String, &str); 4] = [
        (
            "json",
            json_writer_fixture(&json_out.to_string_lossy().replace('\\', "\\\\")),
            "finish-ok\n{\"a\":1}\n",
        ),
        (
            "jsonl",
            jsonl_writer_fixture(&jsonl_out.to_string_lossy().replace('\\', "\\\\")),
            "\"alpha\"\n7\n\n",
        ),
        (
            "csv",
            csv_writer_fixture(&csv_out.to_string_lossy().replace('\\', "\\\\")),
            "x,y\r\n\n",
        ),
        (
            "cbor",
            cbor_writer_fixture(&cbor_out.to_string_lossy().replace('\\', "\\\\")),
            "finish-ok\ndone\n",
        ),
    ];

    for (name, source, expected) in cases {
        let (base_code, base_out, base_err) = build_and_run(&dir, &format!("{name}_base"), &source, &[]);
        assert_eq!((base_code, base_err.as_str()), (0, ""), "{name} baseline");
        assert_eq!(base_out, expected, "{name} baseline stdout");

        let (host_code, host_out, host_err) = run_with_env(
            &dir,
            &format!("{name}_chunk1"),
            &source,
            &hostile_env(&[("JET_ENC_HOSTILE_WRITE_MAX", "1")]),
        );
        assert_eq!((host_code, host_err.as_str()), (0, ""), "{name} hostile stderr");
        assert_eq!(host_out, expected, "{name} short-write drift");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_hostile_io_failures_latch_terminal_for_read_and_write() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping hostile interrupt matrix (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_intr_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let directory = dir.to_string_lossy().replace('\\', "\\\\");
    let source_read = format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    directory_input :: files.open("{directory}") ?? panic("open dir")
    directory_reader := cbor.reader(^directory_input) ?? panic("reader")
    result :: directory_reader.next()
    if result == {{
        Ok(_) -> {{ print("read-ok-missed") }}
        Err(first) -> {{
            again :: directory_reader.next()
            if again == {{
                Ok(_) -> {{ print("not-latched") }}
                Err(second) -> {{
                    print(first.kind == encoding.EncodingErrorKind.IO)
                    print(first.reason == second.reason)
                }}
            }}
        }}
    }}
}}
"#
    );
    let json_out = dir.join("json.out");
    let _ = json_out;
    let (code, stdout, stderr) = run_with_env(
        &dir,
        "json_intr_read",
        &source_read,
        &hostile_env(&[]),
    );
    assert_eq!((code, stderr.as_str()), (0, ""));
    assert!(stdout.contains("true"), "expected IO error, got: {stdout}");

    let source_write = format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn writer_io(writer: &cbor.CBORWriter) => Bool {{
    repeated :: writer.flush()
    if repeated == {{
        Err(error) -> return error.kind == encoding.EncodingErrorKind.IO
        Ok(_) -> return false
    }}
    return false
}}

fn run() {{
    full_output :: files.create("/dev/full") ?? panic("full open")
    full_writer := cbor.writer(^full_output) ?? panic("full writer")
    write_result :: full_writer.write(encoding.DataEvent.Null)
    if write_result == {{
        Err(first) -> {{
            print(first.kind == encoding.EncodingErrorKind.IO)
            flush_again :: full_writer.flush()
            if flush_again == {{
                Err(second) -> print(first.reason == second.reason)
                Ok(done) -> print(false)
            }}
        }}
        Ok(_) -> {{
            flush_result :: full_writer.flush()
            if flush_result == {{
                Err(io_error) -> print(writer_io(&full_writer))
                Ok(done) -> print(false)
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = run_with_env(
        &dir,
        "json_intr_write",
        &source_write,
        &hostile_env(&[("JET_ENC_HOSTILE_WRITE_MAX", "1")]),
    );
    assert_eq!((code, stderr.as_str()), (0, ""));
    assert!(stdout.contains("true"), "expected IO on write, got: {stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_hostile_fail_after_write_preserves_prefix_without_duplication() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping hostile fail-after write test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_failw_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("partial.json");
    let output_text = output.to_string_lossy().replace('\\', "\\\\");
    let limits_source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 1
    output :: files.create("{output_text}") ?? panic("create")
    writer :: json.writer(^output, limits) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    result :: writer.write(encoding.DataEvent.Text("ab"))
    if result == {{
        Ok(_) -> {{ print("missed-limit") }}
        Err(first) -> {{
            again :: writer.finish()
            if again == {{
                Ok(_) -> {{ print("finish-after-limit-missed") }}
                Err(second) -> {{
                    print(first.kind == encoding.EncodingErrorKind.Limit)
                    print(first.reason == second.reason)
                }}
            }}
        }}
    }}
    wire := files.read("{output_text}") ?? panic("read")
    print(wire)
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "fail_limit", &limits_source, &[]);
    assert_eq!((code, stderr.as_str()), (0, ""));
    assert!(stdout.contains("true"), "limit latch: {stdout}");
    let wire = fs::read_to_string(&output).unwrap();
    assert_eq!(wire, "[", "prefix only: {wire}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_hostile_malformed_and_truncated_corpora_latch_under_chunked_io() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping hostile malformed corpus (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_mal_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let corpora: [(&str, &str, Vec<u8>, &str); 8] = [
        ("json_trunc", "json", b"{\"a\":\"".to_vec(), "Truncated"),
        ("json_bad", "json", b"{\"a\":}".to_vec(), "Syntax"),
        ("jsonl_bad", "jsonl", b"{\"a\":1}\n[2,]\n".to_vec(), "Syntax"),
        ("csv_bad", "csv", b"\"unclosed\r\n".to_vec(), "Truncated"),
        ("csv_utf8", "csv", vec![b'a', b',', 0xff], "Syntax"),
        ("cbor_trunc", "cbor", vec![0x81], "Truncated"),
        ("cbor_tag", "cbor", vec![0xC0, 0x00], "Unsupported"),
        ("cbor_dup", "cbor", vec![0xA2, 0x61, 0x61, 0x01, 0x61, 0x61, 0x02], "Unsupported"),
    ];

    for (label, codec, bytes, _kind) in corpora {
        let path = dir.join(format!("{label}.in"));
        fs::write(&path, bytes).unwrap();
        let path_text = path.to_string_lossy().replace('\\', "\\\\");
        let source = malformed_reader_fixture(codec, &path_text);
        let (base_code, base_out, _) = build_and_run(&dir, &format!("{label}_base"), &source, &[]);
        assert_eq!(base_code, 0, "{label} baseline failed");
        let (host_code, host_out, _) = run_with_env(
            &dir,
            &format!("{label}_hostile"),
            &source,
            &hostile_env(&[("JET_ENC_HOSTILE_READ_ONE", "1")]),
        );
        assert_eq!(host_code, 0, "{label} hostile failed");
        assert!(
            base_out.contains("true") && host_out.contains("true"),
            "{label}: base={base_out} host={host_out} expected terminal latch"
        );
        assert!(
            !base_out.contains("not-latched") && !host_out.contains("not-latched"),
            "{label}: latch failure base={base_out} host={host_out}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_hostile_drop_scope_emits_no_false_success() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping hostile drop test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_drop_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let partial = dir.join("partial.jsonl");
    let partial_text = partial.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn write_partial(path: String) {{
    output :: files.create(path) ?? panic("create")
    writer :: jsonl.writer(^output) ?? panic("writer")
    writer.write(DataTree.Text("kept")) ?? panic("write")
    writer.flush() ?? panic("flush")
}}

fn run() {{
    write_partial("{partial_text}")
    leftover :: files.read("{partial_text}") ?? panic("read")
    print(leftover == "\"kept\"")
    print(leftover.contains("\n") == false)
}}
"#
    );
    let (code, stdout, stderr) = run_with_env(
        &dir,
        "drop_scope",
        &source,
        &hostile_env(&[("JET_ENC_HOSTILE_WRITE_MAX", "1")]),
    );
    assert_eq!((code, stderr.as_str()), (0, ""));
    assert_eq!(stdout, "true\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn encoding_hostile_retains_real_file_and_dev_full_probes() {
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("note: skipping /dev/full probe (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_enc_hostile_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let directory = dir.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn reader_io(reader: &cbor.CBORReader) => Bool {{
    repeated :: reader.next()
    if repeated == {{
        Err(error) -> return error.kind == encoding.EncodingErrorKind.IO
        Ok(_) -> return false
    }}
    return false
}}

fn writer_io(writer: &cbor.CBORWriter) => Bool {{
    repeated :: writer.flush()
    if repeated == {{
        Err(error) -> return error.kind == encoding.EncodingErrorKind.IO
        Ok(_) -> return false
    }}
    return false
}}

fn run() {{
    directory_input :: files.open("{directory}") ?? panic("open dir")
    directory_reader := cbor.reader(^directory_input) ?? panic("reader")
    print(reader_io(&directory_reader))

    full_output :: files.create("/dev/full") ?? panic("full")
    full_writer := cbor.writer(^full_output) ?? panic("writer")
    write_result :: full_writer.write(encoding.DataEvent.Null)
    if write_result == {{
        Err(io_error) -> print(writer_io(&full_writer))
        Ok(wrote) -> {{
            flush_result :: full_writer.flush()
            if flush_result == {{
                Err(io_error) -> print(writer_io(&full_writer))
                Ok(done) -> print(false)
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "full_probe", &source, &[]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "true\ntrue\n", ""));
    let (host_code, host_stdout, host_stderr) = run_with_env(
        &dir,
        "full_probe_hostile",
        &source,
        &hostile_env(&[("JET_ENC_HOSTILE_WRITE_MAX", "1")]),
    );
    assert_eq!((host_code, host_stderr.as_str()), (0, ""));
    assert_eq!(host_stdout, "true\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}
