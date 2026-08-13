#[test]
fn encoding_stream_foundation_types_are_real_jet_values() {
    let dir = std::env::temp_dir().join(format!("jet_encoding_foundation_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("foundation.json");
    let bad_path = dir.join("bad-limits.json");
    let path_text = path.to_string_lossy().replace('\\', "\\\\");
    let bad_text = bad_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.files as files

fn keep_error(v: ^encoding.EncodingError) => encoding.EncodingError {{ return v }}
fn keep_cause(v: ^encoding.EncodingCause) => encoding.EncodingCause {{ return v }}
fn keep_event(v: ^encoding.DataEvent) => encoding.DataEvent {{ return v }}
fn keep_format(v: ^encoding.EncodingFormat) => encoding.EncodingFormat {{ return v }}
fn keep_kind(v: ^encoding.EncodingErrorKind) => encoding.EncodingErrorKind {{ return v }}
fn keep_json_reader(v: ^json.JSONReader) => json.JSONReader {{ return v }}
fn keep_json_writer(v: ^json.JSONWriter) => json.JSONWriter {{ return v }}
fn keep_jsonl_reader(v: ^jsonl.JSONLReader) => jsonl.JSONLReader {{ return v }}
fn keep_jsonl_writer(v: ^jsonl.JSONLWriter) => jsonl.JSONLWriter {{ return v }}
fn keep_csv_reader(v: ^csv.CSVReader) => csv.CSVReader {{ return v }}
fn keep_csv_writer(v: ^csv.CSVWriter) => csv.CSVWriter {{ return v }}
fn keep_xml_reader(v: ^xml.XMLReader) => xml.XMLReader {{ return v }}
fn keep_xml_writer(v: ^xml.XMLWriter) => xml.XMLWriter {{ return v }}
fn keep_cbor_reader(v: ^cbor.CBORReader) => cbor.CBORReader {{ return v }}
fn keep_cbor_writer(v: ^cbor.CBORWriter) => cbor.CBORWriter {{ return v }}

fn run() {{
    limits := encoding.EncodingLimits.safe()
    print("{{limits.buffer_bytes}}:{{limits.max_depth}}:{{limits.max_item_bytes}}:{{limits.max_expansion_depth}}:{{limits.max_expansion_bytes}}")
    if limits.max_total_bytes == None {{ print(true) }} else {{ print(false) }}
    print(keep_format(^encoding.EncodingFormat.JSON) == encoding.EncodingFormat.JSON)
    print(keep_kind(^encoding.EncodingErrorKind.Limit) == encoding.EncodingErrorKind.Limit)

    cause := encoding.EncodingCause.{{ kind: "io", os_code: None, message: "nope" }}
    kept_cause := keep_cause(^cause)
    print(kept_cause.kind)
    print(kept_cause.message)

    err := encoding.EncodingError.{{
        format: encoding.EncodingFormat.JSON,
        kind: encoding.EncodingErrorKind.Limit,
        byte_offset: 0,
        line: None,
        column: None,
        path: "",
        reason: "buffer_bytes 1 is outside 4096..16777216",
        cause: None,
    }}
    kept_err := keep_error(^err)
    print(kept_err.format == encoding.EncodingFormat.JSON)
    print(kept_err.kind == encoding.EncodingErrorKind.Limit)
    print(kept_err.byte_offset)
    if kept_err.cause == None {{ print(true) }} else {{ print(false) }}
    print("{{kept_err}}")

    event := encoding.DataEvent.Null
    kept_event := keep_event(^event)
    print(true)

    output :: files.create("{path_text}") ?? panic("create")
    writer :: json.writer(^output, limits, false) ?? panic("writer")
    kept_writer := keep_json_writer(^writer)
    kept_writer.write(encoding.DataEvent.Null) ?? panic("write")
    kept_writer.finish() ?? panic("finish")

    bad := encoding.EncodingLimits.safe()
    bad.buffer_bytes = 1
    bad_output :: files.create("{bad_text}") ?? panic("bad create")
    if json.writer(^bad_output, bad, false) == {{
        .Ok(_) -> {{ print("limits-missed") }}
        .Err(reject) -> {{
            print("{{reject}}")
            print(reject.format == encoding.EncodingFormat.JSON)
            print(reject.kind == encoding.EncodingErrorKind.Limit)
            print(reject.reason)
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "encoding_foundation", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        concat!(
            "65536:256:16777216:32:8388608\n",
            "true\n",
            "true\n",
            "true\n",
            "io\n",
            "nope\n",
            "true\n",
            "true\n",
            "0\n",
            "true\n",
            "JSON Limit at byte 0: buffer_bytes 1 is outside 4096..16777216\n",
            "true\n",
            "JSON Limit at byte 0, line 1, column 1: buffer_bytes 1 is outside 4096..16777216\n",
            "true\n",
            "true\n",
            "buffer_bytes 1 is outside 4096..16777216\n",
        )
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "null");
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
        .Ok(_) -> {{ print(true) }}
        .Err(_) -> {{ print(false) }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 2
    input :: files.open("{input_text}") ?? panic("open")
    reader :: json.reader(^input, limits) ?? panic("reader")
    count := 0
    loop count < 8 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("reader-not-latched") }}
                    .Err(second) -> {{
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
        .Ok(_) -> {{ print("finish-missed") }}
        .Err(first) -> {{
            after_flush :: finished_writer.flush()
            if after_flush == {{
                .Ok(_) -> {{ print("finish-not-latched") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    escaped_limits := encoding.EncodingLimits.safe()
    escaped_limits.max_item_bytes = 1
    escaped_output :: files.create("{limited_text}") ?? panic("create")
    escaped_writer :: json.writer(^escaped_output, escaped_limits) ?? panic("writer")
    escaped_result :: escaped_writer.write(encoding.DataEvent.Text("\n"))
    if escaped_result == {{
        .Ok(_) -> {{ print("escape-missed") }}
        .Err(first) -> {{
            escaped_again :: escaped_writer.finish()
            if escaped_again == {{
                .Ok(_) -> {{ print("escape-not-latched") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
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

#[test]
fn json_stream_rejects_whole_events_and_records_before_partial_output() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_atomic_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let text_path = dir.join("text.json");
    let key_path = dir.join("key.json");
    let depth_path = dir.join("depth.json");
    let jsonl_path = dir.join("record.jsonl");
    let nonfinite_path = dir.join("nonfinite.jsonl");
    let text = text_path.to_string_lossy().replace('\\', "\\\\");
    let key = key_path.to_string_lossy().replace('\\', "\\\\");
    let depth = depth_path.to_string_lossy().replace('\\', "\\\\");
    let jsonl = jsonl_path.to_string_lossy().replace('\\', "\\\\");
    let nonfinite = nonfinite_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    text_limits := encoding.EncodingLimits.safe()
    text_limits.max_total_bytes = Val(5)
    text_output :: files.create("{text}") ?? panic("create text")
    text_writer :: json.writer(^text_output, text_limits) ?? panic("text writer")
    text_writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    text_error :: text_writer.write(encoding.DataEvent.Text("abcd"))
    if text_error == {{
        .Ok(_) -> {{ print("text-limit-missed") }}
        .Err(first) -> {{
            again :: text_writer.finish()
            if again == {{
                .Ok(_) -> {{ print("text-terminal-missed") }}
                .Err(second) -> {{ print(first.reason == second.reason) }}
            }}
        }}
    }}

    key_limits := encoding.EncodingLimits.safe()
    key_limits.max_total_bytes = Val(5)
    key_output :: files.create("{key}") ?? panic("create key")
    key_writer :: json.writer(^key_output, key_limits) ?? panic("key writer")
    key_writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    key_result :: key_writer.write(encoding.DataEvent.Key("abc"))
    if key_result == {{ .Ok(_) -> {{ print("key-limit-missed") }} .Err(_) -> {{ print(true) }} }}

    depth_limits := encoding.EncodingLimits.safe()
    depth_limits.max_depth = 1
    depth_output :: files.create("{depth}") ?? panic("create depth")
    depth_writer :: json.writer(^depth_output, depth_limits) ?? panic("depth writer")
    depth_writer.write(encoding.DataEvent.ArrayStart) ?? panic("outer")
    depth_result :: depth_writer.write(encoding.DataEvent.ArrayStart)
    if depth_result == {{ .Ok(_) -> {{ print("depth-limit-missed") }} .Err(_) -> {{ print(true) }} }}

    record_limits := encoding.EncodingLimits.safe()
    record_limits.max_total_bytes = Val(5)
    record_output :: files.create("{jsonl}") ?? panic("create record")
    record_writer :: jsonl.writer(^record_output, record_limits) ?? panic("record writer")
    record_result :: record_writer.write(DataTree.Array([DataTree.Int(1), DataTree.Text("abcd")]))
    if record_result == {{
        .Ok(_) -> {{ print("record-limit-missed") }}
        .Err(first) -> {{
            again :: record_writer.flush()
            if again == {{ .Ok(_) -> {{ print("record-terminal-missed") }} .Err(second) -> {{ print(first.reason == second.reason) }} }}
        }}
    }}

    nonfinite_output :: files.create("{nonfinite}") ?? panic("create nonfinite")
    nonfinite_writer :: jsonl.writer(^nonfinite_output) ?? panic("nonfinite writer")
    nonfinite_result :: nonfinite_writer.write(DataTree.Array([DataTree.Int(1), DataTree.Float(0.0 / 0.0)]))
    if nonfinite_result == {{ .Ok(_) -> {{ print("nonfinite-missed") }} .Err(_) -> {{ print(true) }} }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream_atomic", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\ntrue\n");
    assert_eq!(fs::read_to_string(&text_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&key_path).unwrap(), "{");
    assert_eq!(fs::read_to_string(&depth_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&jsonl_path).unwrap(), "");
    assert_eq!(fs::read_to_string(&nonfinite_path).unwrap(), "");
    assert_eq!(stderr, "");
    let dev_path = dir.join("json_stream_atomic.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("JSON stream atomic default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&text_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&key_path).unwrap(), "{");
    assert_eq!(fs::read_to_string(&depth_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&jsonl_path).unwrap(), "");
    assert_eq!(fs::read_to_string(&nonfinite_path).unwrap(), "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_canonical_stream_sorts_nested_objects_and_latches_rejections() {
    let dir = std::env::temp_dir().join(format!("jet_json_canonical_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output_path = dir.join("canonical.json");
    let duplicate_path = dir.join("duplicate.json");
    let limited_path = dir.join("limited.json");
    let output = output_path.to_string_lossy().replace('\\', "\\\\");
    let duplicate = duplicate_path.to_string_lossy().replace('\\', "\\\\");
    let limited = limited_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    output :: files.create("{output}") ?? panic("create")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), true) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    writer.write(encoding.DataEvent.Key("z")) ?? panic("key")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("nested object")
    writer.write(encoding.DataEvent.Key("b")) ?? panic("key")
    writer.write(encoding.DataEvent.Int(2)) ?? panic("int")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Text("x")) ?? panic("text")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("nested end")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Bool(true)) ?? panic("bool")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("object end")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")

    data := DataTree.Object([
        "z": DataTree.Array([DataTree.Int(1), DataTree.Object(["b": DataTree.Int(2), "a": DataTree.Text("x")])]),
        "a": DataTree.Bool(true),
    ])
    print(json.canonical(data) ?? panic("value is not canonical JSON"))

    duplicate_output :: files.create("{duplicate}") ?? panic("duplicate create")
    duplicate_writer :: json.writer(^duplicate_output, encoding.EncodingLimits.safe(), true) ?? panic("duplicate writer")
    duplicate_writer.write(encoding.DataEvent.ObjectStart) ?? panic("duplicate object")
    duplicate_writer.write(encoding.DataEvent.Key("same")) ?? panic("first key")
    duplicate_writer.write(encoding.DataEvent.Int(1)) ?? panic("first value")
    duplicate_result :: duplicate_writer.write(encoding.DataEvent.Key("same"))
    if duplicate_result == {{
        .Ok(_) -> {{ print("duplicate-missed") }}
        .Err(first) -> {{
            again :: duplicate_writer.finish()
            if again == {{
                .Ok(_) -> {{ print("terminal-missed") }}
                .Err(second) -> {{ print(first.reason == second.reason) }}
            }}
        }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 8
    limited_output :: files.create("{limited}") ?? panic("limited create")
    limited_writer :: json.writer(^limited_output, limits, true) ?? panic("limited writer")
    limited_writer.write(encoding.DataEvent.ObjectStart) ?? panic("limited object")
    limited_writer.write(encoding.DataEvent.Key("long")) ?? panic("limited key")
    limited_writer.write(encoding.DataEvent.Text("value")) ?? panic("limited value")
    limited_result :: limited_writer.write(encoding.DataEvent.ObjectEnd)
    if limited_result == {{
        .Ok(_) -> {{ print("limit-missed") }}
        .Err(first) -> {{
            again :: limited_writer.flush()
            if again == {{
                .Ok(_) -> {{ print("limit-terminal-missed") }}
                .Err(second) -> {{ print(first.reason == second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_canonical_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    let expected = r#"{"a":true,"z":[1,{"a":"x","b":2}]}"#;
    assert_eq!(stdout, format!("{expected}\ntrue\ntrue\n"));
    assert_eq!(fs::read_to_string(&output_path).unwrap(), expected);
    assert_eq!(fs::read_to_string(&duplicate_path).unwrap(), "");
    assert_eq!(fs::read_to_string(&limited_path).unwrap(), "");
    assert_eq!(stderr, "");
    // No quick-run (default `jet run`) leg here: `core.files.create` isn't
    // supported by the shared deopt/interpreter ambient evaluator yet
    // (E0956, card #1583) — matches the AOT-only pattern already used by
    // `json_canonical_stream_matches_rfc8785_numbers_key_order_and_domain`
    // just below, the sibling file-IO-heavy stream test.
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_canonical_stream_matches_rfc8785_numbers_key_order_and_domain() {
    let dir = std::env::temp_dir().join(format!("jet_json_jcs_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let canonical_path = dir.join("canonical.json");
    let int_path = dir.join("int.json");
    let bytes_path = dir.join("bytes.json");
    let nonfinite_path = dir.join("nonfinite.json");
    let duplicate_path = dir.join("duplicate.json");
    let path = |path: &std::path::Path| path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    output :: files.create("{}") ?? panic("create canonical")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), true) ?? panic("writer canonical")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    writer.write(encoding.DataEvent.Key("𐀀")) ?? panic("astral key")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("astral value")
    writer.write(encoding.DataEvent.Key("")) ?? panic("bmp key")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Float(1e30)) ?? panic("positive exponent")
    writer.write(encoding.DataEvent.Float(1e20)) ?? panic("decimal cutover")
    writer.write(encoding.DataEvent.Float(1e-7)) ?? panic("negative exponent")
    writer.write(encoding.DataEvent.Float(-0.0)) ?? panic("negative zero")
    writer.write(encoding.DataEvent.Int(9007199254740992)) ?? panic("exact Int boundary")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("object end")
    writer.finish() ?? panic("finish")

    int_output :: files.create("{}") ?? panic("create int")
    int_writer :: json.writer(^int_output, encoding.EncodingLimits.safe(), true) ?? panic("int writer")
    if int_writer.write(encoding.DataEvent.Int(9007199254740993)) == {{
        .Ok(_) -> print("int accepted")
        .Err(error) -> print(error.reason)
    }}

    bytes_output :: files.create("{}") ?? panic("create bytes")
    bytes_writer :: json.writer(^bytes_output, encoding.EncodingLimits.safe(), true) ?? panic("bytes writer")
    bytes :: [U8].{{ U8.from_int(1) ?? panic("byte") }}
    if bytes_writer.write(encoding.DataEvent.Bytes(bytes)) == {{
        .Ok(_) -> print("bytes accepted")
        .Err(error) -> print(error.reason)
    }}

    nonfinite_output :: files.create("{}") ?? panic("create nonfinite")
    nonfinite_writer :: json.writer(^nonfinite_output, encoding.EncodingLimits.safe(), true) ?? panic("nonfinite writer")
    if nonfinite_writer.write(encoding.DataEvent.Float(0.0 / 0.0)) == {{
        .Ok(_) -> print("nonfinite accepted")
        .Err(error) -> print(error.reason)
    }}

    duplicate_output :: files.create("{}") ?? panic("create duplicate")
    duplicate_writer :: json.writer(^duplicate_output, encoding.EncodingLimits.safe(), true) ?? panic("duplicate writer")
    duplicate_writer.write(encoding.DataEvent.ObjectStart) ?? panic("duplicate object")
    duplicate_writer.write(encoding.DataEvent.Key("same")) ?? panic("first key")
    duplicate_writer.write(encoding.DataEvent.Null) ?? panic("first value")
    if duplicate_writer.write(encoding.DataEvent.Key("same")) == {{
        .Ok(_) -> print("duplicate accepted")
        .Err(error) -> print(error.reason)
    }}
}}
"#,
        path(&canonical_path),
        path(&int_path),
        path(&bytes_path),
        path(&nonfinite_path),
        path(&duplicate_path),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_jcs_stream", &source, &[], None);
    assert_eq!(code, 0, "RFC 8785 stream test failed: {stderr}");
    assert_eq!(
        fs::read_to_string(&canonical_path).unwrap(),
        "{\"𐀀\":1,\"\":[1e+30,100000000000000000000,1e-7,0,9007199254740992]}"
    );
    assert_eq!(
        stdout,
        "JCS requires Int exactly representable as IEEE 754 binary64; encode this integer as Text\nJSON cannot encode Bytes; encode bytes as Text explicitly\nJCS cannot encode a non-finite Float\nJCS requires unique object keys\n"
    );
    for rejected in [&int_path, &bytes_path, &nonfinite_path, &duplicate_path] {
        assert_eq!(fs::read(rejected).unwrap(), b"");
    }
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rfc8785_corpus_manifest_hashes_and_provenance_are_pinned() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/encoding/rfc8785");
    let manifest = fs::read_to_string(root.join("MANIFEST.tsv")).unwrap();
    let mut count = 0;
    for line in manifest.lines().filter(|line| !line.starts_with('#') && !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "bad corpus manifest row: {line}");
        let bytes = fs::read(root.join(fields[0])).unwrap();
        assert_eq!(jet::SHA256::sha256_hex(&bytes), fields[1], "hash drift: {}", fields[0]);
        assert!(fields[2].starts_with("https://www.rfc-editor.org/rfc/rfc8785.html#"));
        assert_eq!(fields[3], "IETF-Trust-Legal-Provisions");
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn json_canonical_stream_matches_every_finite_rfc8785_appendix_b_vector() {
    if !common::have_rustc() {
        eprintln!("note: skipping RFC 8785 Appendix B stream corpus (need rustc)");
        return;
    }
    let corpus = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/encoding/rfc8785/appendix-b.tsv"),
    )
    .unwrap();
    let cases = corpus
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let (bits, expected) = line.split_once('\t').unwrap();
            (u64::from_str_radix(bits, 16).unwrap() as i64, expected)
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 24);
    let writes = cases
        .iter()
        .map(|(bits, _)| {
            let bits = if *bits == i64::MIN {
                "(-9223372036854775807 - 1)".to_string()
            } else {
                bits.to_string()
            };
            format!(
                "    writer.write(encoding.DataEvent.Float(math.from_bits({bits}))) ?? panic(\"Appendix B value\")"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = format!(
        "[{}]",
        cases.iter().map(|(_, expected)| *expected).collect::<Vec<_>>().join(",")
    );
    let dir = std::env::temp_dir().join(format!("jet_json_jcs_appendix_b_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("appendix-b.json");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files
use core.math as math

fn run() {{
    output :: files.create("{}") ?? panic("create")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), true) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
{}
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    writer.finish() ?? panic("finish")
}}
"#,
        output.to_string_lossy().replace('\\', "\\\\"),
        writes,
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_jcs_appendix_b", &source, &[], None);
    assert_eq!(code, 0, "RFC 8785 Appendix B corpus failed: {stderr}");
    assert_eq!(stdout, "");
    assert_eq!(fs::read_to_string(&output).unwrap(), expected);
    // No quick-run (default `jet run`) leg here: `core.files.create` isn't
    // supported by the shared deopt/interpreter ambient evaluator yet
    // (E0956, card #1583) — matches the AOT-only pattern already used by
    // `json_canonical_stream_matches_rfc8785_numbers_key_order_and_domain`,
    // the sibling file-IO-heavy stream test.
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jsonl_stream_records_are_incremental_bounded_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_jsonl_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output_path = dir.join("out.jsonl");
    let limited_path = dir.join("limited.jsonl");
    let input_path = dir.join("input.jsonl");
    let malformed_path = dir.join("malformed.jsonl");
    fs::write(&input_path, "\r\n  \r\n\"first\"\r\n[2,\"second\"]\n").unwrap();
    fs::write(&malformed_path, "{\"ok\":1}\n{\"bad\":[2,]}\n").unwrap();
    let output = output_path.to_string_lossy().replace('\\', "\\\\");
    let limited = limited_path.to_string_lossy().replace('\\', "\\\\");
    let input = input_path.to_string_lossy().replace('\\', "\\\\");
    let malformed = malformed_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    output :: files.create("{output}") ?? panic("create")
    writer :: jsonl.writer(^output) ?? panic("writer")
    writer.write(DataTree.Text("alpha")) ?? panic("write")
    writer.write(DataTree.Array([DataTree.Int(1), DataTree.Text("beta")])) ?? panic("write")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")
    after_finish :: writer.write(DataTree.Null)
    if after_finish == {{
        .Ok(_) -> {{ print("write-after-finish-missed") }}
        .Err(first) -> {{
            after_terminal :: writer.flush()
            if after_terminal == {{
                .Ok(_) -> {{ print("terminal-not-latched") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    input :: files.open("{input}") ?? panic("open")
    reader :: jsonl.reader(^input) ?? panic("reader")
    first_result :: reader.next() ?? panic("first")
    if first_result == {{
        Val(value) -> {{ print(value.text() ?? "bad") }}
        None -> {{ print("missing-first") }}
    }}
    second_result :: reader.next() ?? panic("second")
    if second_result == {{
        Val(value) -> {{
            first :: value.at(0) ?? DataTree.Int(-1)
            second :: value.at(1) ?? DataTree.Text("bad")
            print(first.int() ?? -1)
            print(second.text() ?? "bad")
        }}
        None -> {{ print("missing-second") }}
    }}
    eof_result :: reader.next() ?? panic("eof")
    if eof_result == None {{ print("eof") }} else {{ print("bad-eof") }}
    eof_again :: reader.next() ?? panic("eof again")
    if eof_again == None {{ print("eof-again") }} else {{ print("bad-eof-again") }}

    malformed_input :: files.open("{malformed}") ?? panic("open malformed")
    malformed_reader :: jsonl.reader(^malformed_input) ?? panic("reader malformed")
    first_malformed :: malformed_reader.next() ?? panic("first malformed record")
    malformed_result :: malformed_reader.next()
    if malformed_result == {{
        .Ok(_) -> {{ print("malformed-missed") }}
        .Err(first) -> {{
            malformed_again :: malformed_reader.next()
            if malformed_again == {{
                .Ok(_) -> {{ print("malformed-not-latched") }}
                .Err(second) -> {{
                    print(first.line ?? -1)
                    print(first.path)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 2
    limited_output :: files.create("{limited}") ?? panic("limited create")
    limited_writer :: jsonl.writer(^limited_output, limits) ?? panic("limited writer")
    limited_result :: limited_writer.write(DataTree.Text("three"))
    if limited_result == {{
        .Ok(_) -> {{ print("limit-missed") }}
        .Err(first) -> {{
            limited_again :: limited_writer.finish()
            if limited_again == {{
                .Ok(_) -> {{ print("limit-not-latched") }}
                .Err(second) -> {{
                    print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "jsonl_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\nfirst\n2\nsecond\neof\neof-again\n2\n$[1][\"bad\"][1]\ntrue\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "\"alpha\"\n[1,\"beta\"]\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jsonl_fold_heap_budget_rejects_growth_before_large_record_allocation() {
    let dir = std::env::temp_dir().join(format!("jet_jsonl_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let array_path = dir.join("array.jsonl");
    let object_path = dir.join("object.jsonl");
    let valid_path = dir.join("valid.jsonl");
    let scalar_path = dir.join("scalar.jsonl");
    let near_string_path = dir.join("near-string.jsonl");
    let array = format!("[{}]\n", std::iter::repeat("0").take(256).collect::<Vec<_>>().join(","));
    let object = format!(
        "{{{}}}\n",
        (0..256)
            .map(|index| format!(r#""key{index:04}":"""#))
            .collect::<Vec<_>>()
            .join(",")
    );
    fs::write(&array_path, array).unwrap();
    fs::write(&object_path, object).unwrap();
    fs::write(&valid_path, format!("[{}]\n", std::iter::repeat("0").take(32).collect::<Vec<_>>().join(","))).unwrap();
    fs::write(&scalar_path, "1\n").unwrap();
    fs::write(&near_string_path, format!("{{\"{}\":0}}\n", "k".repeat(100_000))).unwrap();
    let array_path = array_path.to_string_lossy().replace('\\', "\\\\");
    let object_path = object_path.to_string_lossy().replace('\\', "\\\\");
    let valid_path = valid_path.to_string_lossy().replace('\\', "\\\\");
    let scalar_path = scalar_path.to_string_lossy().replace('\\', "\\\\");
    let near_string_path = near_string_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    near_limits := encoding.EncodingLimits.safe()
    near_limits.buffer_bytes = 4096
    near_limits.max_depth = 1
    near_limits.max_item_bytes = 100000
    near_limits.max_expansion_bytes = 0
    near_input :: files.open("{near_string_path}") ?? panic("near string open")
    near_reader :: jsonl.reader(^near_input, near_limits) ?? panic("near string reader")
    near_result :: near_reader.next()
    if near_result == {{
        .Ok(_) -> {{ print("near-string-limit-missed") }}
        .Err(first) -> {{
            near_again :: near_reader.next()
            if near_again == {{
                .Ok(_) -> {{ print("near-string-terminal-missed") }}
                .Err(second) -> {{
                    print(first.byte_offset == 100003)
                    print(first.path)
                    print(first.reason)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}

    valid_limits := encoding.EncodingLimits.safe()
    valid_limits.max_item_bytes = 512
    valid_input :: files.open("{valid_path}") ?? panic("valid open")
    valid_reader :: jsonl.reader(^valid_input, valid_limits) ?? panic("valid reader")
    valid_record :: valid_reader.next() ?? panic("valid next")
    if valid_record == {{
        Val(value) -> {{ last :: value.at(31) ?? DataTree.Int(-1); print(last.int() ?? -1) }}
        None -> {{ print("valid-missing") }}
    }}

    scalar_limits := encoding.EncodingLimits.safe()
    scalar_limits.max_item_bytes = 7
    scalar_input :: files.open("{scalar_path}") ?? panic("scalar open")
    scalar_reader :: jsonl.reader(^scalar_input, scalar_limits) ?? panic("scalar reader")
    scalar_result :: scalar_reader.next()
    if scalar_result == {{
        .Ok(_) -> {{ print("scalar-limit-missed") }}
        .Err(first) -> {{
            scalar_again :: scalar_reader.next()
            if scalar_again == {{
                .Ok(_) -> {{ print("scalar-terminal-missed") }}
                .Err(second) -> {{ print(first.path); print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason) }}
            }}
        }}
    }}

    array_limits := encoding.EncodingLimits.safe()
    array_limits.max_item_bytes = 512
    array_input :: files.open("{array_path}") ?? panic("array open")
    array_reader :: jsonl.reader(^array_input, array_limits) ?? panic("array reader")
    array_result :: array_reader.next()
    if array_result == {{
        .Ok(_) -> {{ print("array-limit-missed") }}
        .Err(first) -> {{
            array_again :: array_reader.next()
            if array_again == {{
                .Ok(_) -> {{ print("array-terminal-missed") }}
                .Err(second) -> {{
                    print(first.byte_offset < 256)
                    print(first.path)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}

    object_limits := encoding.EncodingLimits.safe()
    object_limits.max_item_bytes = 512
    object_input :: files.open("{object_path}") ?? panic("object open")
    object_reader :: jsonl.reader(^object_input, object_limits) ?? panic("object reader")
    object_result :: object_reader.next()
    if object_result == {{
        .Ok(_) -> {{ print("object-limit-missed") }}
        .Err(first) -> {{
            object_again :: object_reader.next()
            if object_again == {{
                .Ok(_) -> {{ print("object-terminal-missed") }}
                .Err(second) -> {{
                    print(first.byte_offset < 2048)
                    print(first.path)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "jsonl_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\n$[0]\nJSON string allocation exceeded the bounded codec heap ceiling\ntrue\n0\n$[0]\ntrue\ntrue\n$[0][63]\ntrue\ntrue\n$[0][\"key0073\"]\ntrue\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.json");
    let heap_path = dir.join("heap.json");
    let key = "k".repeat(100_000);
    fs::write(&heap_path, format!("{{\"{key}\":0}}")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: json.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(7)) ?? panic("int")
    writer.flush() ?? panic("flush")
    // no finish — Drop must close the handle without claiming success
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: unfinished bytes still on this path.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "[7")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: json.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(encoding.DataEvent.Null) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    print(finished == "null")

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 100000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: json.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\n100003\n$\nJSON string allocation exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null");
    assert_eq!(stderr, "");
    let dev_path = dir.join("json_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("JSON stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_number_token_stays_under_counting_allocator_ceiling() {
    if !common::have_rustc() {
        eprintln!("note: skipping JSON counting-allocator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_json_counted_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("number.json");
    fs::write(&input_path, format!("1{}", "0".repeat(149_999))).unwrap();
    let input = input_path.to_string_lossy().replace('\\', "\\\\");
    let malformed_path = dir.join("malformed.json");
    fs::write(&malformed_path, format!("1{}+", "0".repeat(199_998))).unwrap();
    let malformed = malformed_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{input}") ?? panic("open number")
    reader :: json.reader(^input, limits) ?? panic("create reader")
    result :: reader.next()
    if result == {{
        .Ok(_) -> {{ panic("oversized number allocation accepted") }}
        .Err(first) -> {{
            print(first.reason)
            again :: reader.next()
            if again == {{
                .Ok(_) -> {{ panic("number allocation error not terminal") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    malformed_limits := encoding.EncodingLimits.safe()
    malformed_limits.buffer_bytes = 4096
    malformed_limits.max_depth = 1
    malformed_limits.max_item_bytes = 200000
    malformed_limits.max_expansion_bytes = 0
    malformed_input :: files.open("{malformed}") ?? panic("open malformed number")
    malformed_reader :: json.reader(^malformed_input, malformed_limits) ?? panic("create malformed reader")
    malformed_result :: malformed_reader.next()
    if malformed_result == {{
        .Ok(_) -> {{ panic("malformed number accepted") }}
        .Err(first) -> {{
            print(first.reason)
            again :: malformed_reader.next()
            if again == {{
                .Ok(_) -> {{ panic("malformed number error not terminal") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let path = dir.join("counted.jet");
    fs::write(&path, &source).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(&source, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(&shown, &source, &diags))
    });
    let renamed = out.rust.replacen(
        "fn jet_enc_json_reader_next(",
        "fn jet_enc_json_reader_next_inner(",
        1,
    );
    assert_ne!(renamed, out.rust, "generated JSON reader seam changed");
    let allocator = r#"
mod jet_json_alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub struct CountingAlloc;
    static COUNTING: AtomicBool = AtomicBool::new(false);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    fn add(size: usize) {
        let live = LIVE.fetch_add(size, Ordering::SeqCst) + size;
        PEAK.fetch_max(live, Ordering::SeqCst);
    }
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = System.alloc(layout);
            if !ptr.is_null() && COUNTING.load(Ordering::SeqCst) { add(layout.size()); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if COUNTING.load(Ordering::SeqCst) { LIVE.fetch_sub(layout.size(), Ordering::SeqCst); }
            System.dealloc(ptr, layout);
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
            let counting = COUNTING.load(Ordering::SeqCst);
            if counting { LIVE.fetch_sub(old.size(), Ordering::SeqCst); }
            let next = System.realloc(ptr, old, new_size);
            if counting { if next.is_null() { add(old.size()); } else { add(new_size); } }
            next
        }
    }
    pub fn begin() {
        LIVE.store(0, Ordering::SeqCst);
        PEAK.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
    }
    pub fn finish() -> usize {
        COUNTING.store(false, Ordering::SeqCst);
        PEAK.load(Ordering::SeqCst)
    }
}
#[global_allocator]
static JET_JSON_ALLOC: jet_json_alloc_probe::CountingAlloc = jet_json_alloc_probe::CountingAlloc;
fn jet_enc_json_reader_next(reader: &mut jet_std::JSONReader) -> Result<JetOutcome<jet_std::DataEvent, JetAbsent>, jet_std::EncodingError> {
    let ceiling = jet_encoding_codec_heap_ceiling(&reader.limits);
    jet_json_alloc_probe::begin();
    let result = jet_enc_json_reader_next_inner(reader);
    let peak = jet_json_alloc_probe::finish();
    assert!(peak <= ceiling, "JSON requested allocation peak {peak} exceeded {ceiling}");
    result
}
"#;
    let rs = dir.join("counted.rs");
    let bin = dir.join("counted");
    let generated = renamed.replacen("#![allow(warnings)]", "", 1);
    assert_ne!(generated, renamed, "generated crate attribute changed");
    let rust = format!("#![allow(warnings)]\n{allocator}\n{generated}");
    let mut command = Command::new("rustc");
    common::add_generated_rust(&mut command, &rs, &rust, false, &[]);
    let rustc = command.arg("-o").arg(&bin).output().unwrap();
    assert!(rustc.status.success(), "rustc rejected counted JSON program:\n{}", String::from_utf8_lossy(&rustc.stderr));
    let run = Command::new(&bin).current_dir(&dir).output().unwrap();
    assert!(run.status.success(), "counted JSON program failed:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "JSON number allocation exceeded the bounded codec heap ceiling\ntrue\ninvalid JSON number\ntrue\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jsonl_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_jsonl_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.jsonl");
    let heap_path = dir.join("heap.jsonl");
    let key = "k".repeat(100_000);
    fs::write(&heap_path, format!("{{\"{key}\":0}}\n")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: jsonl.writer(^output) ?? panic("writer")
    writer.write(DataTree.Text("alpha")) ?? panic("record")
    writer.flush() ?? panic("flush")
    // no finish — Drop must leave the record LF unwritten (incomplete wire)
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete bytes (no record LF) still here.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "\"alpha\"")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: jsonl.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(DataTree.Null) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    print(finished == "null\n")
    // Honesty: unfinished Drop wire ≠ finished wire for the same value.
    print(leftover != "\"alpha\"\n")

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 100000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: jsonl.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "jsonl_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\n100003\n$[0]\nJSON string allocation exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null\n");
    assert_eq!(stderr, "");
    let dev_path = dir.join("jsonl_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("JSONL stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn csv_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_csv_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.csv");
    let heap_path = dir.join("heap.csv");
    // Capacity doubles to 131072; the next byte charges past the shared codec
    // heap ceiling while still under max_item_bytes (same counting allocator).
    let field = "x".repeat(131_072);
    fs::write(&heap_path, format!("{field}y")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: csv.writer(^output) ?? panic("writer")
    writer.write(["alpha", "beta"]) ?? panic("record")
    writer.flush() ?? panic("flush")
    // no finish — Drop must leave the record CRLF unwritten (incomplete wire)
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete bytes (no record CRLF) still here.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "alpha,beta")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: csv.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(["done"]) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    // finished is "done\\r\\n"; Jet has no \\r escape — prove via length + prefix.
    print(finished.starts_with("done") && finished.len() == 6)
    // Honesty: unfinished Drop wire ≠ finished row terminator.
    print(leftover.len() == 10)

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: csv.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "csv_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\n131073\n$[0][0]\nCSV record heap exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "done\r\n");
    assert_eq!(stderr, "");
    let dev_path = dir.join("csv_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CSV stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "done\r\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_xml_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.xml");
    let heap_path = dir.join("heap.xml");
    // Attribute text under max_item_bytes; raw_bytes→Array<Int> DataTree slots
    // charge past the shared codec heap ceiling (same counting allocator).
    // Keep modest: ByteLexer retains per-scalar units, so 128KiB hung the suite.
    let attr = "x".repeat(8_192);
    fs::write(&heap_path, format!("<r a=\"{attr}\"/>")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn xml_name(local: String) => DataTree {{
    return DataTree.Object([
        "raw": DataTree.Text(~local),
        "prefix": DataTree.Null,
        "local": DataTree.Text(~local),
        "namespace_uri": DataTree.Null,
    ])
}}

fn document_start() => DataTree {{
    return DataTree.Object([
        "$xml_event": DataTree.Text("document_start"),
        "encoding": DataTree.Null,
        "bom": DataTree.Array([]),
    ])
}}

fn document_end() => DataTree {{
    return DataTree.Object(["$xml_event": DataTree.Text("document_end")])
}}

fn element_start(empty_style: String) => DataTree {{
    return DataTree.Object([
        "$xml_event": DataTree.Text("element_start"),
        "name": xml_name("r"),
        "namespaces": DataTree.Array([]),
        "attributes": DataTree.Array([]),
        "empty_style": DataTree.Text(~empty_style),
        "open_lexical": DataTree.Object([
            "raw_text": DataTree.Null,
            "raw_bytes": DataTree.Null,
            "semantic": DataTree.Object([
                "name": xml_name("r"),
                "namespaces": DataTree.Array([]),
                "attributes": DataTree.Array([]),
                "empty_style": DataTree.Text(~empty_style),
            ]),
        ]),
    ])
}}

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: xml.writer(^output) ?? panic("writer")
    writer.write(document_start()) ?? panic("document_start")
    writer.write(element_start("explicit")) ?? panic("open root")
    writer.flush() ?? panic("flush")
    // no element_end / document_end / finish — Drop leaves incomplete open tag
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete open element still here.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "<r>")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: xml.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(document_start()) ?? panic("reopen start")
    reopen_writer.write(element_start("empty")) ?? panic("reopen empty root")
    reopen_writer.write(document_end()) ?? panic("reopen end")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    print(finished == "<r/>")
    // Honesty: unfinished Drop wire ≠ finished complete document.
    print(leftover != finished)

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: xml.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 8 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        format!(
            "true\ntrue\ntrue\n{}\n$\nXML event heap exceeded the bounded codec heap ceiling\ntrue\n",
            fs::metadata(&heap_path).unwrap().len()
        )
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "<r/>");
    assert_eq!(stderr, "");
    let dev_path = dir.join("xml_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("XML stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "<r/>");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.cbor");
    let heap_path = dir.join("heap.cbor");
    // Capacity doubles to 131072; the next byte charges past the shared codec
    // heap ceiling while still under max_item_bytes (same counting allocator).
    let text = vec![b'x'; 131_073];
    let mut heap_bytes = Vec::new();
    heap_bytes.push(0x7a); // text, 4-byte length
    heap_bytes.extend_from_slice(&(131_073u32).to_be_bytes());
    heap_bytes.extend_from_slice(&text);
    fs::write(&heap_path, &heap_bytes).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: cbor.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(7)) ?? panic("int")
    writer.flush() ?? panic("flush")
    // no ArrayEnd / finish — Drop leaves buffered items unwritten (incomplete)
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete leftover still here (empty wire).
    leftover :: files.read_bytes("{partial}") ?? panic("same-path read after Drop")
    empty :: [U8].{{}}
    print(leftover == empty)
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: cbor.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(encoding.DataEvent.Null) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read_bytes("{partial}") ?? panic("same-path read after finish")
    null_wire :: [U8].{{ 246 }}
    print(finished == null_wire)
    // Honesty: unfinished Drop wire ≠ finished complete root.
    print(leftover != finished)

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: cbor.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    // Header is 5 bytes (0x7a + u32 length); fail when doubling past 131072 payload bytes.
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\n131077\n$\nCBOR stream heap exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read(&partial_path).unwrap(), [0xf6]);
    assert_eq!(stderr, "");
    let dev_path = dir.join("cbor_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CBOR stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read(&partial_path).unwrap(), [0xf6]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn csv_whole_value_handles_multiline_quotes_crlf_and_typed_decode() {
    let dir = std::env::temp_dir().join(format!("jet_csv_whole_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.csv as csv

#Codable
struct Note { name: String, note: String }

fn run() {
    raw :: "name,note\nAda,\"line1\nline2\"\nLin,\"said \"\"hi\"\"\"\n"
    rows :: csv.parse(raw) ?? panic("parse")
    print(rows.len())
    print(rows[1][1])
    print(rows[2][1])
    print(csv.to_string(rows).replace("\n", "|"))

    notes :: csv.decode<Note>(raw) ?? panic("decode")
    print(notes.len())
    print(notes[0].name)
    print(notes[0].note)

    if csv.parse("a,\"unterminated") == {
        .Ok(_) -> { print("unterminated-missed") }
        .Err(message) -> { print(message.contains("quoted field ended before its closing quote")) }
    }
    if csv.parse("a,\"ok\"junk") == {
        .Ok(_) -> { print("closing-junk-missed") }
        .Err(message) -> { print(message.contains("may follow a closing quote")) }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "csv_whole", source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "3\nline1\nline2\nsaid \"hi\"\nname,note|Ada,\"line1|line2\"|Lin,\"said \"\"hi\"\"\"\n2\nAda\nline1\nline2\ntrue\ntrue\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn csv_stream_records_are_incremental_rfc4180_bounded_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_csv_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("input.csv");
    let output_path = dir.join("output.csv");
    let malformed_path = dir.join("malformed.csv");
    let invalid_utf8_path = dir.join("invalid-utf8.csv");
    let item_limit_path = dir.join("item-limit.csv");
    let total_limit_path = dir.join("total-limit.csv");
    fs::write(&input_path, "a,\"b,b\",\"c\"\"c\",\"line1\nline2\"\r\nlast,,tail").unwrap();
    fs::write(&malformed_path, "\"bad").unwrap();
    fs::write(&invalid_utf8_path, [b'a', b',', 0xff]).unwrap();
    fs::write(&item_limit_path, "\"abcd\"\r\n").unwrap();
    fs::write(&total_limit_path, "a,b\r\n").unwrap();
    let input = input_path.to_string_lossy().replace('\\', "\\\\");
    let output = output_path.to_string_lossy().replace('\\', "\\\\");
    let malformed = malformed_path.to_string_lossy().replace('\\', "\\\\");
    let invalid_utf8 = invalid_utf8_path.to_string_lossy().replace('\\', "\\\\");
    let item_limit = item_limit_path.to_string_lossy().replace('\\', "\\\\");
    let total_limit = total_limit_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn run() {{
    output :: files.create("{output}") ?? panic("create")
    writer :: csv.writer(^output) ?? panic("writer")
    writer.write(["a", "b,b", "c\"c", "line1\nline2"]) ?? panic("write first")
    writer.write(["last", "", "tail"]) ?? panic("write second")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")
    after_finish :: writer.write(["late"])
    if after_finish == {{
        .Ok(_) -> {{ print("write-after-finish-missed") }}
        .Err(writer_first) -> {{
            after_terminal :: writer.flush()
            if after_terminal == {{
                .Ok(_) -> {{ print("writer-terminal-missed") }}
                .Err(writer_second) -> {{ print(writer_first.byte_offset == writer_second.byte_offset && writer_first.reason == writer_second.reason) }}
            }}
        }}
    }}

    input :: files.open("{input}") ?? panic("open")
    reader :: csv.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("first")
    if first == {{
        Val(row) -> {{ print(row[0]); print(row[1]); print(row[2]); print(row[3]) }}
        None -> {{ print("first-missing") }}
    }}
    second :: reader.next() ?? panic("second")
    if second == {{
        Val(row) -> {{ print(row[0]); print(row[1] == ""); print(row[2]) }}
        None -> {{ print("second-missing") }}
    }}
    eof :: reader.next() ?? panic("eof")
    if eof == {{ Val(_) -> {{ print(false) }} None -> {{ print(true) }} }}
    eof_again :: reader.next() ?? panic("eof again")
    if eof_again == {{ Val(_) -> {{ print(false) }} None -> {{ print(true) }} }}

    malformed_input :: files.open("{malformed}") ?? panic("malformed open")
    malformed_reader :: csv.reader(^malformed_input) ?? panic("malformed reader")
    malformed_result :: malformed_reader.next()
    if malformed_result == {{
        .Ok(_) -> {{ print("malformed-missed") }}
        .Err(malformed_first) -> {{
            malformed_again :: malformed_reader.next()
            if malformed_again == {{
                .Ok(_) -> {{ print("malformed-terminal-missed") }}
                .Err(malformed_second) -> {{ print(malformed_first.path); print(malformed_first.byte_offset == malformed_second.byte_offset && malformed_first.reason == malformed_second.reason) }}
            }}
        }}
    }}

    invalid_utf8_input :: files.open("{invalid_utf8}") ?? panic("invalid utf8 open")
    invalid_utf8_reader :: csv.reader(^invalid_utf8_input) ?? panic("invalid utf8 reader")
    invalid_utf8_result :: invalid_utf8_reader.next()
    if invalid_utf8_result == {{
        .Ok(_) -> {{ print("invalid-utf8-missed") }}
        .Err(error) -> {{
            print(error.byte_offset)
            print(error.line ?? 0)
            print(error.column ?? 0)
            print(error.path)
        }}
    }}

    item_limits := encoding.EncodingLimits.safe()
    item_limits.max_item_bytes = 3
    item_input :: files.open("{item_limit}") ?? panic("item open")
    item_reader :: csv.reader(^item_input, item_limits) ?? panic("item reader")
    item_result :: item_reader.next()
    if item_result == {{
        .Ok(_) -> {{ print("item-limit-missed") }}
        .Err(item_first) -> {{
            item_again :: item_reader.next()
            if item_again == {{
                .Ok(_) -> {{ print("item-terminal-missed") }}
                .Err(item_second) -> {{ print(item_first.path); print(item_first.byte_offset == item_second.byte_offset && item_first.reason == item_second.reason) }}
            }}
        }}
    }}

    total_limits := encoding.EncodingLimits.safe()
    total_limits.max_total_bytes = Val(3)
    total_input :: files.open("{total_limit}") ?? panic("total open")
    total_reader :: csv.reader(^total_input, total_limits) ?? panic("total reader")
    total_result :: total_reader.next()
    if total_result == {{
        .Ok(_) -> {{ print("total-limit-missed") }}
        .Err(total_first) -> {{
            total_again :: total_reader.next()
            if total_again == {{
                .Ok(_) -> {{ print("total-terminal-missed") }}
                .Err(total_second) -> {{ print(total_first.byte_offset); print(total_first.path); print(total_first.reason == total_second.reason) }}
            }}
        }}
    }}

    writer_limits := encoding.EncodingLimits.safe()
    writer_limits.max_item_bytes = 3
    limited_output :: files.create("{output}.limited") ?? panic("limited create")
    limited_writer :: csv.writer(^limited_output, writer_limits) ?? panic("limited writer")
    limited_result :: limited_writer.write(["abcd"])
    if limited_result == {{
        .Ok(_) -> {{ print("writer-limit-missed") }}
        .Err(limited_first) -> {{
            limited_again :: limited_writer.finish()
            if limited_again == {{
                .Ok(_) -> {{ print("writer-limit-terminal-missed") }}
                .Err(limited_second) -> {{ print(limited_first.path); print(limited_first.reason == limited_second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "csv_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\na\nb,b\nc\"c\nline1\nline2\nlast\ntrue\ntail\ntrue\ntrue\n$[0][0]\ntrue\n3\n1\n4\n$[0][1]\n$[0][0]\ntrue\n3\n$[0][1]\ntrue\n$[0][0]\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "a,\"b,b\",\"c\"\"c\",\"line1\nline2\"\r\nlast,,tail\r\n");
    assert_eq!(stderr, "");
    let dev_path = dir.join("csv_stream.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CSV stream default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "a,\"b,b\",\"c\"\"c\",\"line1\nline2\"\r\nlast,,tail\r\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_is_incremental_bounded_deterministic_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("output.cbor");
    let float_output = dir.join("float.cbor");
    let indefinite = dir.join("indefinite.cbor");
    let truncated = dir.join("truncated.cbor");
    let half = dir.join("half.cbor");
    let non_shortest = dir.join("non-shortest.cbor");
    let nested = dir.join("nested.cbor");
    fs::write(&indefinite, [0x9f, 0x01, 0x7f, 0x61, b'a', 0xff, 0x42, 0x01, 0x02, 0xff]).unwrap();
    fs::write(&truncated, [0x63, b'a']).unwrap();
    fs::write(&half, [0xf9, 0x3c, 0x00]).unwrap();
    fs::write(&non_shortest, [0x18, 0x01]).unwrap();
    fs::write(&nested, [0x81, 0x80]).unwrap();
    let output_text = output.to_string_lossy().replace('\\', "\\\\");
    let float_output_text = float_output.to_string_lossy().replace('\\', "\\\\");
    let indefinite_text = indefinite.to_string_lossy().replace('\\', "\\\\");
    let truncated_text = truncated.to_string_lossy().replace('\\', "\\\\");
    let half_text = half.to_string_lossy().replace('\\', "\\\\");
    let non_shortest_text = non_shortest.to_string_lossy().replace('\\', "\\\\");
    let nested_text = nested.to_string_lossy().replace('\\', "\\\\");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    output :: files.create("{output_text}") ?? panic("create")
    writer :: cbor.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("start")
    writer.write(encoding.DataEvent.Key("b")) ?? panic("key")
    writer.write(encoding.DataEvent.Text("xy")) ?? panic("text")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("end")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")
    float_file :: files.create("{float_output_text}") ?? panic("float create")
    float_writer :: cbor.writer(^float_file) ?? panic("float writer")
    float_writer.write(encoding.DataEvent.ArrayStart) ?? panic("float array")
    float_writer.write(encoding.DataEvent.Float(1.0)) ?? panic("float write")
    float_writer.write(encoding.DataEvent.Float(100000.0)) ?? panic("float32 write")
    float_writer.write(encoding.DataEvent.Float(1.1)) ?? panic("float64 write")
    float_writer.write(encoding.DataEvent.Float(0.0 / 0.0)) ?? panic("nan write")
    float_writer.write(encoding.DataEvent.Float(-0.0)) ?? panic("negative zero write")
    float_writer.write(encoding.DataEvent.ArrayEnd) ?? panic("float array end")
    float_writer.finish() ?? panic("float finish")
    whole_tree :: DataTree.Object(["b": DataTree.Text("xy"), "a": DataTree.Int(1)])
    expected_whole :: [U8].{{ 162, 97, 97, 1, 97, 98, 98, 120, 121 }}
    print((cbor.to_bytes_canonical(whole_tree) ?? panic("whole encode")) == expected_whole)
    after :: writer.write(encoding.DataEvent.Null)
    if after == {{
        .Ok(_) -> print(false)
        .Err(writer_first) -> {{
            again :: writer.flush()
            if again == {{
                .Ok(_) -> print(false)
                .Err(writer_second) -> print(writer_first.reason == writer_second.reason)
            }}
        }}
    }}

    input :: files.open("{output_text}") ?? panic("open")
    reader :: cbor.reader(^input) ?? panic("reader")
    count := 0
    loop count < 6 {{
        event :: reader.next() ?? panic("next")
        if event == {{
            Val(_) -> count++
            None -> print("early")
        }}
    }}
    eof :: reader.next() ?? panic("eof")
    if eof == {{
        None -> print(count)
        Val(_) -> print("late")
    }}

    indef_input :: files.open("{indefinite_text}") ?? panic("indef open")
    indef_reader :: cbor.reader(^indef_input) ?? panic("indef reader")
    indef_count := 0
    loop indef_count < 5 {{
        indef_event :: indef_reader.next() ?? panic("indef next")
        if indef_event == {{
            Val(_) -> indef_count++
            None -> print("indef early")
        }}
    }}
    print(indef_count)

    half_input :: files.open("{half_text}") ?? panic("half open")
    half_reader :: cbor.reader(^half_input) ?? panic("half reader")
    if half_reader.next() == {{
        .Ok(_) -> print(true)
        .Err(_) -> print(false)
    }}

    short_input :: files.open("{non_shortest_text}") ?? panic("short open")
    short_reader :: cbor.reader(^short_input) ?? panic("short reader")
    if short_reader.next() == {{
        .Ok(_) -> print(true)
        .Err(_) -> print(false)
    }}

    depth_limits := encoding.EncodingLimits.safe()
    depth_limits.max_depth = 1
    nested_input :: files.open("{nested_text}") ?? panic("nested open")
    nested_reader :: cbor.reader(^nested_input, depth_limits) ?? panic("nested reader")
    root_event :: nested_reader.next() ?? panic("root array")
    if nested_reader.next() == {{
        .Ok(_) -> print(false)
        .Err(depth_error) -> print(depth_error.reason == "max_depth 1 exceeded")
    }}

    bad_input :: files.open("{truncated_text}") ?? panic("bad open")
    bad_reader :: cbor.reader(^bad_input) ?? panic("bad reader")
    first_bad :: bad_reader.next()
    if first_bad == {{
        .Ok(_) -> print("missed")
        .Err(bad_first) -> {{
            second_bad :: bad_reader.next()
            if second_bad == {{
                .Ok(_) -> print("unlatched")
                .Err(bad_second) -> {{
                    print(bad_first.byte_offset)
                    print(bad_first.path)
                    print(bad_first.reason == bad_second.reason)
                }}
            }}
        }}
    }}
}}
"#);
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\ntrue\n6\n5\ntrue\ntrue\ntrue\n2\n$\ntrue\n");
    assert_eq!(fs::read(&output).unwrap(), [0xa2, 0x61, b'a', 0x01, 0x61, b'b', 0x62, b'x', b'y']);
    assert_eq!(fs::read(&float_output).unwrap(), [
        0x85, 0xf9, 0x3c, 0x00, 0xfa, 0x47, 0xc3, 0x50, 0x00,
        0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a,
        0xf9, 0x7e, 0x00, 0xf9, 0x80, 0x00,
    ]);
    assert_eq!(stderr, "");
    let dev_path = dir.join("cbor_stream.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CBOR stream default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_hostile_inputs_and_replacement_limits_are_exact() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_hostile_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixtures: &[(&str, &[u8])] = &[
        ("indef-map.cbor", &[0xbf, 0x61, b'a', 0x7f, 0x61, b'x', 0x61, b'y', 0xff, 0xff]),
        ("indef-bytes.cbor", &[0x5f, 0x42, 1, 2, 0x41, 3, 0xff]),
        ("duplicate.cbor", &[0xbf, 0x61, b'a', 1, 0x61, b'a', 2, 0xff]),
        ("nontext.cbor", &[0xa1, 1, 2]),
        ("tag.cbor", &[0xc0, 1]),
        ("range.cbor", &[0x1b, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        ("trunc-int.cbor", &[0x1a, 0]),
        ("trunc-float.cbor", &[0xfa, 0, 0]),
        ("trunc-indef.cbor", &[0x7f, 0x62, b'a']),
        ("trailing.cbor", &[1, 2]),
        ("nested.cbor", &[0x81, 0xa1, 0x61, b'x', 0x1a, 0]),
    ];
    for (name, bytes) in fixtures { fs::write(dir.join(name), bytes).unwrap(); }
    let path = |name: &str| dir.join(name).to_string_lossy().replace('\\', "\\\\");
    let array_ok = path("array-ok.cbor");
    let array_fail = path("array-fail.cbor");
    let object_ok = path("object-ok.cbor");
    let object_fail = path("object-fail.cbor");
    let incomplete = path("incomplete.cbor");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn reader_terminal(reader: &cbor.CBORReader, reason: String) => Bool {{
    repeated :: reader.next()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn writer_terminal(writer: &cbor.CBORWriter, reason: String) => Bool {{
    repeated :: writer.flush()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn run() {{
    map_limits := encoding.EncodingLimits.safe()
    map_limits.max_item_bytes = 3
    map_input :: files.open("{}") ?? panic("map open")
    map_reader :: cbor.reader(^map_input, map_limits) ?? panic("map reader")
    map_count := 0
    loop map_count < 4 {{
        map_event :: map_reader.next() ?? panic("map error")
        if map_event == {{
            Val(_) -> map_count++
            None -> panic("map eof")
        }}
    }}
    print(map_count)

    tight_limits := encoding.EncodingLimits.safe()
    tight_limits.max_item_bytes = 2
    tight_input :: files.open("{}") ?? panic("tight open")
    tight_reader := cbor.reader(^tight_input, tight_limits) ?? panic("tight reader")
    tight_object :: tight_reader.next() ?? panic("tight object")
    tight_key :: tight_reader.next() ?? panic("tight key")
    tight_first :: tight_reader.next()
    if tight_first == {{
        .Ok(_) -> panic("combined key/chunk budget missed")
        .Err(first) -> {{
            print(first.path == "$[\"a\"]" && first.byte_offset == 6 && reader_terminal(&tight_reader, ~first.reason))
        }}
    }}

    bytes_input :: files.open("{}") ?? panic("bytes open")
    bytes_reader :: cbor.reader(^bytes_input) ?? panic("bytes reader")
    bytes_event :: bytes_reader.next() ?? panic("bytes event")
    if bytes_event == {{
        Val(_) -> print(true)
        None -> print(false)
    }}

    short_input :: files.open("{}") ?? panic("short open")
    short_reader :: cbor.reader(^short_input) ?? panic("short reader")
    short_event :: short_reader.next() ?? panic("short event")
    if short_event == {{
        Val(_) -> print(true)
        None -> print(false)
    }}

    duplicate_input :: files.open("{}") ?? panic("duplicate open")
    duplicate_reader := cbor.reader(^duplicate_input) ?? panic("duplicate reader")
    duplicate_object :: duplicate_reader.next() ?? panic("duplicate object")
    duplicate_key :: duplicate_reader.next() ?? panic("duplicate key")
    duplicate_value :: duplicate_reader.next() ?? panic("duplicate value")
    duplicate_first :: duplicate_reader.next()
    if duplicate_first == {{
        .Err(first) -> {{
            print(first.byte_offset == 4 && first.path == "$" && reader_terminal(&duplicate_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    nontext_input :: files.open("{}") ?? panic("nontext open")
    nontext_reader :: cbor.reader(^nontext_input) ?? panic("nontext reader")
    nontext_object :: nontext_reader.next() ?? panic("nontext object")
    if nontext_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 1 && e.path == "$" && e.reason == "CBOR map key must be text")
        .Ok(_) -> print(false)
    }}

    tag_input :: files.open("{}") ?? panic("tag open")
    tag_reader :: cbor.reader(^tag_input) ?? panic("tag reader")
    if tag_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 0 && e.path == "$" && e.reason == "CBOR tags are outside DataEvent")
        .Ok(_) -> print(false)
    }}

    range_input :: files.open("{}") ?? panic("range open")
    range_reader :: cbor.reader(^range_input) ?? panic("range reader")
    if range_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR integer is outside Jet Int")
        .Ok(_) -> print(false)
    }}

    int_input :: files.open("{}") ?? panic("int open")
    int_reader := cbor.reader(^int_input) ?? panic("int reader")
    if int_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 2 && reader_terminal(&int_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    float_input :: files.open("{}") ?? panic("float open")
    float_reader := cbor.reader(^float_input) ?? panic("float reader")
    if float_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 3 && reader_terminal(&float_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    indef_input :: files.open("{}") ?? panic("indef open")
    indef_reader := cbor.reader(^indef_input) ?? panic("indef reader")
    if indef_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 3 && reader_terminal(&indef_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    trailing_input :: files.open("{}") ?? panic("trailing open")
    trailing_reader := cbor.reader(^trailing_input) ?? panic("trailing reader")
    trailing_root :: trailing_reader.next() ?? panic("root")
    if trailing_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 1 && reader_terminal(&trailing_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    nested_input :: files.open("{}") ?? panic("nested open")
    nested_reader :: cbor.reader(^nested_input) ?? panic("nested reader")
    nested_array :: nested_reader.next() ?? panic("nested array")
    nested_object :: nested_reader.next() ?? panic("nested object")
    nested_key :: nested_reader.next() ?? panic("nested key")
    if nested_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 6 && e.path == "$[0][\"x\"]")
        .Ok(_) -> print(false)
    }}

    array_limits := encoding.EncodingLimits.safe()
    array_limits.max_item_bytes = 2
    array_output :: files.create("{array_ok}") ?? panic("array output")
    array_writer :: cbor.writer(^array_output, array_limits) ?? panic("array writer")
    array_writer.write(encoding.DataEvent.ArrayStart) ?? panic("array start")
    array_writer.write(encoding.DataEvent.Null) ?? panic("array null")
    array_writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    array_writer.finish() ?? panic("array finish")

    array_tight := encoding.EncodingLimits.safe()
    array_tight.max_item_bytes = 1
    array_fail_output :: files.create("{array_fail}") ?? panic("array fail output")
    array_fail_writer := cbor.writer(^array_fail_output, array_tight) ?? panic("array fail writer")
    array_fail_writer.write(encoding.DataEvent.ArrayStart) ?? panic("array fail start")
    array_fail_writer.write(encoding.DataEvent.Null) ?? panic("array fail null")
    if array_fail_writer.write(encoding.DataEvent.ArrayEnd) == {{
        .Err(first) -> {{
            print(writer_terminal(&array_fail_writer, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    object_limits := encoding.EncodingLimits.safe()
    object_limits.max_item_bytes = 4
    object_output :: files.create("{object_ok}") ?? panic("object output")
    object_writer :: cbor.writer(^object_output, object_limits) ?? panic("object writer")
    object_writer.write(encoding.DataEvent.ObjectStart) ?? panic("object start")
    object_writer.write(encoding.DataEvent.Key("a")) ?? panic("object key")
    object_writer.write(encoding.DataEvent.Null) ?? panic("object null")
    object_writer.write(encoding.DataEvent.ObjectEnd) ?? panic("object end")
    object_writer.finish() ?? panic("object finish")

    object_tight := encoding.EncodingLimits.safe()
    object_tight.max_item_bytes = 3
    object_fail_output :: files.create("{object_fail}") ?? panic("object fail output")
    object_fail_writer :: cbor.writer(^object_fail_output, object_tight) ?? panic("object fail writer")
    object_fail_writer.write(encoding.DataEvent.ObjectStart) ?? panic("object fail start")
    object_fail_writer.write(encoding.DataEvent.Key("a")) ?? panic("object fail key")
    object_fail_writer.write(encoding.DataEvent.Null) ?? panic("object fail null")
    if object_fail_writer.write(encoding.DataEvent.ObjectEnd) == {{
        .Err(_) -> print(true)
        .Ok(_) -> print(false)
    }}

    incomplete_output :: files.create("{incomplete}") ?? panic("incomplete output")
    incomplete_writer := cbor.writer(^incomplete_output) ?? panic("incomplete writer")
    incomplete_writer.write(encoding.DataEvent.ArrayStart) ?? panic("incomplete start")
    incomplete_writer.flush() ?? panic("incomplete flush")
    if incomplete_writer.finish() == {{
        .Err(first) -> {{
            print(writer_terminal(&incomplete_writer, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}
}}
"#,
        path("indef-map.cbor"), path("indef-map.cbor"), path("indef-bytes.cbor"),
        path("non-shortest.cbor"), path("duplicate.cbor"), path("nontext.cbor"),
        path("tag.cbor"), path("range.cbor"), path("trunc-int.cbor"),
        path("trunc-float.cbor"), path("trunc-indef.cbor"), path("trailing.cbor"),
        path("nested.cbor"),
    );
    fs::write(dir.join("non-shortest.cbor"), [0x18, 0x01]).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_hostile", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}\nsource:\n{source}");
    assert_eq!(stdout, "4\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n");
    assert_eq!(fs::read(&array_ok).unwrap(), [0x81, 0xf6]);
    assert_eq!(fs::read(&object_ok).unwrap(), [0xa1, 0x61, b'a', 0xf6]);
    assert!(fs::read(&incomplete).unwrap().is_empty());
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_workspace_growth_is_prospective_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_workspace_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let success = dir.join("success.cbor").to_string_lossy().replace('\\', "\\\\");
    let rejected = dir.join("rejected.cbor").to_string_lossy().replace('\\', "\\\\");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn terminal(writer: &cbor.CBORWriter, reason: String) => Bool {{
    repeated :: writer.finish()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn close_array(writer: &cbor.CBORWriter) {{
    result :: writer.write(encoding.DataEvent.ArrayEnd)
    if result == {{
        .Err(error) -> panic("{{error.reason}}")
        .Ok(_) -> return
    }}
}}

fn run() {{
    roomy := encoding.EncodingLimits.safe()
    roomy.max_item_bytes = 9
    output :: files.create("{success}") ?? panic("create")
    writer := cbor.writer(^output, roomy) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("start")
    loop _, 0..7 {{ writer.write(encoding.DataEvent.Null) ?? panic("null") }}
    close_array(&writer)
    writer.finish() ?? panic("finish")

    tight := encoding.EncodingLimits.safe()
    tight.max_item_bytes = 7
    rejected_output :: files.create("{rejected}") ?? panic("create rejected")
    rejected_writer := cbor.writer(^rejected_output, tight) ?? panic("rejected writer")
    rejected_writer.write(encoding.DataEvent.ArrayStart) ?? panic("rejected start")
    loop _, 0..6 {{ rejected_writer.write(encoding.DataEvent.Null) ?? panic("accepted null") }}
    if rejected_writer.write(encoding.DataEvent.Null) == {{
        .Err(first) -> {{
            print(first.reason == "max_item_bytes 7 exceeded")
            print(terminal(&rejected_writer, ~first.reason))
        }}
        .Ok(_) -> {{ print(false); print(false) }}
    }}
}}
"#);
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_workspace", &source, &[], None);
    assert_eq!(code, 0, "CBOR workspace program failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\n");
    assert_eq!(fs::read(dir.join("success.cbor")).unwrap(), [0x88, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6]);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn cbor_stream_io_errors_latch_in_aot_and_default_dev() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_io_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let directory = dir.to_string_lossy().replace('\\', "\\\\");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn reader_terminal(reader: &cbor.CBORReader, reason: String) => Bool {{
    repeated :: reader.next()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn writer_terminal(writer: &cbor.CBORWriter, reason: String) => Bool {{
    repeated :: writer.flush()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn run() {{
    directory_input :: files.open("{directory}") ?? panic("directory open")
    directory_reader := cbor.reader(^directory_input) ?? panic("directory reader")
    if directory_reader.next() == {{
        .Err(first) -> print(reader_terminal(&directory_reader, ~first.reason))
        .Ok(_) -> print(false)
    }}
    full_output :: files.create("/dev/full") ?? panic("full open")
    full_writer := cbor.writer(^full_output) ?? panic("full writer")
    full_writer.write(encoding.DataEvent.Null) ?? panic("full buffered write")
    if full_writer.flush() == {{
        .Err(first) -> print(writer_terminal(&full_writer, ~first.reason))
        .Ok(_) -> print(false)
    }}
}}
"#);
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_io", &source, &[], None);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "true\ntrue\n", ""));
    let path = dir.join("cbor_io.jet");
    fs::write(&path, &source).unwrap();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, "true\ntrue\n", ""));
        }
        other => panic!("CBOR default-dev fallback failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_whole_codable_bytes_and_original_wire_canonical_validation() {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(cbor_whole_codable_bytes_and_original_wire_canonical_validation_inner)
        .expect("spawn CBOR parity worker")
        .join()
        .expect("CBOR parity worker must not panic");
}

fn cbor_whole_codable_bytes_and_original_wire_canonical_validation_inner() {
    if !common::have_rustc() {
        eprintln!("note: skipping cbor whole-value test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_whole_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding as encoding
use core.encoding.cbor as cbor

#Codable
struct Packet { id: Int, payload: [U8] }

fn run() {
    packet := Packet.{ id: 7, payload: [222, 173] }
    wire := cbor.to_bytes(packet) ?? panic("encode")
    stable := cbor.to_bytes_canonical(packet) ?? panic("canonical encode")
    back := Packet.{ cbor.decode<Packet>(wire) ?? panic("decode") }
    raw_wire := cbor.to_bytes([1, 2, 255]) ?? panic("byte encode")
    raw := [U8].{ cbor.decode<[U8]>(raw_wire) ?? panic("byte decode") }
    print(wire)
    print(stable == wire)
    print(back.id)
    print(back.payload)
    print(raw)

    strict := cbor.CBOROptions.{
        max_depth: 256,
        max_items: 1000000,
        max_bytes: 1073741824,
        require_canonical: true,
    }
    // 0x18 0x01 is valid CBOR for 1, but not shortest/Core deterministic.
    rejected := cbor.parse([24, 1], strict) ?? DataTree.Int(-1)
    print(rejected.int() ?? -2)
    strict_decode := cbor.CBOROptions.{
        max_depth: 256,
        max_items: 1000000,
        max_bytes: 1073741824,
        require_canonical: true,
    }
    if cbor.decode<[Int]>([129, 97, 120], strict_decode) == {
        .Ok(_) -> print("unexpected success")
        .Err(error) -> print("{error[0].path}|{error[0].reason}")
    }
    if cbor.decode<Int>([65, 0]) == {
        .Ok(_) -> print("unexpected success")
        .Err(error) -> print("{error[0].path}|{error[0].reason}")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_whole", source, &[], None);
    assert_eq!(code, 0, "CBOR whole-value program failed: {stderr}");
    assert_eq!(
        stdout,
        "[162, 98, 105, 100, 7, 103, 112, 97, 121, 108, 111, 97, 100, 66, 222, 173]\ntrue\n7\n[222, 173]\n[1, 2, 255]\n-1\n[0]|expected Int, found text \"x\"\n|expected Int, found Bytes\n"
    );
    let path = dir.join("cbor_whole.jet");
    fs::write(&path, source).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).expect("CBOR fixture loads");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diag| !matches!(diag.severity, jet::Diagnostics::Severity::Error)),
        "CBOR fixture must type-check: {diagnostics:?}"
    );
    jet_jit::try_compile_bundle(&bundle).expect("CBOR fixture must compile for resident JIT");
    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: dev_stdout,
            stderr: dev_stderr,
            exit_code,
        } => {
            assert_eq!(
                (exit_code, dev_stdout, dev_stderr),
                (0, stdout.clone(), String::new())
            );
            assert!(
                jet_jit::jit_executed_for_test(),
                "CBOR whole-value fixture must execute resident JIT"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "CBOR whole-value fixture must not silently deopt"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test(),
                "CBOR whole-value fixture must not fall back"
            );
        }
        other => panic!("CBOR whole-value default-dev failed: {other:?}"),
    }
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: interpreter_stdout,
            stderr: interpreter_stderr,
            exit_code,
        } => assert_eq!(
            (exit_code, interpreter_stdout, interpreter_stderr),
            (0, stdout, String::new()),
            "CBOR whole-value interpreter drifted from AOT/JIT"
        ),
        other => panic!("CBOR whole-value forced interpreter failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_whole_live_allocation_and_preferred_float_validation() {
    if !common::have_rustc() {
        eprintln!("note: skipping cbor whole-value limits test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_whole_limits_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.cbor as cbor

fn run() {
    strict := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1024, require_canonical: true }
    if cbor.parse([249, 62, 0], ~strict) == {
        .Ok(value) -> print(value.float() ?? -1.0)
        .Err(_) -> print(-2.0)
    }
    if cbor.parse([250, 63, 192, 0, 0], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR Float does not use its preferred shortest encoding")
    }
    if cbor.parse([249, 126, 1], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR NaN is not the canonical 0xf97e00 encoding")
    }
    if cbor.parse([249, 126, 0], ~strict) == {
        .Ok(_) -> print(true)
        .Err(_) -> print(false)
    }

    tiny := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 3, require_canonical: false }
    if cbor.parse([130, 1, 2], tiny) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR array allocation exceeds max_bytes 3")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_whole_limits", source, &[], None);
    assert_eq!(code, 0, "CBOR whole-value limits program failed: {stderr}");
    assert_eq!(stdout, "1.5\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn cbor_whole_indefinite_values_obey_normal_canonical_and_limit_laws() {
    if !common::have_rustc() {
        eprintln!("note: skipping CBOR indefinite-value test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_indefinite_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding as encoding
use core.encoding.cbor as cbor

#Codable
struct Packet { name: String, data: [U8] }

fn run() {
    array := [Int].{ cbor.decode<[Int]>([159, 1, 2, 255]) ?? panic("indefinite array") }
    text := cbor.parse([127, 97, 97, 98, 98, 99, 255]) ?? panic("indefinite text")
    print(array)
    print(text.text() ?? "bad")

    // {_ "name": (_ "J", "et"), "data": (_ h'0102', h'03')}
    packet := Packet.{ cbor.decode<Packet>([191, 100, 110, 97, 109, 101, 127, 97, 74, 98, 101, 116, 255, 100, 100, 97, 116, 97, 95, 66, 1, 2, 65, 3, 255, 255]) ?? panic("typed indefinite decode") }
    print(packet.name)
    print(packet.data)

    strict := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1073741824, require_canonical: true }
    if cbor.parse([159, 1, 255], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.path == "$" && e.reason == "indefinite-length CBOR is not Core deterministic")
    }
    if cbor.parse([129, 127, 97, 120, 255], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 1 && e.path == "$[0]")
    }

    item_limited := cbor.CBOROptions.{ max_depth: 256, max_items: 2, max_bytes: 1024, require_canonical: false }
    if cbor.parse([159, 1, 2, 255], item_limited) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 2 && e.path == "$[1]" && e.reason == "max_items 2 exceeded")
    }
    chunk_limited := cbor.CBOROptions.{ max_depth: 256, max_items: 2, max_bytes: 1024, require_canonical: false }
    if cbor.parse([127, 97, 97, 97, 98, 255], chunk_limited) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 3 && e.path == "$" && e.reason == "max_items 2 exceeded")
    }
    depth_limited := cbor.CBOROptions.{ max_depth: 1, max_items: 100, max_bytes: 64, require_canonical: false }
    if cbor.parse([159, 127, 97, 120, 255, 255], depth_limited) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 1 && e.path == "$[0]" && e.reason == "max_depth 1 exceeded")
    }

    if cbor.parse([127, 65, 120, 255]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 1 && e.reason == "indefinite CBOR string contains a wrong or indefinite chunk")
    }
    if cbor.parse([159, 1]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 2 && e.reason == "indefinite CBOR array ended before its break")
    }
    if cbor.parse([191, 97, 107, 255]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 3 && e.reason == "indefinite CBOR map break appears where a value is required")
    }
    if cbor.parse([255]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR break outside an indefinite container")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_indefinite", source, &[], None);
    assert_eq!(code, 0, "CBOR indefinite-value program failed: {stderr}");
    assert_eq!(
        stdout,
        "[1, 2]\nabc\nJet\n[1, 2, 3]\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn cbor_whole_hostile_byte_corpus_matches_aot_and_default_dev() {
    if !common::have_rustc() {
        eprintln!("note: skipping CBOR hostile whole-value corpus (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_whole_corpus_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.cbor as cbor

fn wire(values: [Int]) => [U8] {
    bytes := [U8].{}
    loop value, values {
        bytes.push(U8.from_int(value) ?? panic("corpus byte outside U8"))
    }
    return bytes
}

fn accepted(values: [Int]) => Bool {
    if cbor.parse(wire(values)) == {
        .Ok(_) -> return true
        .Err(_) -> return false
    }
    return false
}

fn rejected(values: [Int], offset: Int, path: String, reason: String) => Bool {
    if cbor.parse(wire(values)) == {
        .Ok(_) -> return false
        .Err(error) -> return error.byte_offset == offset && error.path == path && error.reason == reason
    }
    return false
}

fn canonical_rejected(values: [Int], offset: Int, path: String, reason: String) => Bool {
    strict := cbor.CBOROptions.{
        max_depth: 256,
        max_items: 1000000,
        max_bytes: 1073741824,
        require_canonical: true,
    }
    if cbor.parse(wire(values), strict) == {
        .Ok(_) -> return false
        .Err(error) -> return error.byte_offset == offset && error.path == path && error.reason == reason
    }
    return false
}

fn run() {
    empty := [Int].{}
    // RFC 8949 argument widths, scalar families, nested containers, preferred
    // floats, and every supported normal-mode indefinite family.
    print(accepted([0]))
    print(accepted([23]))
    print(accepted([24, 24]))
    print(accepted([25, 1, 0]))
    print(accepted([26, 0, 1, 0, 0]))
    print(accepted([27, 0, 0, 0, 1, 0, 0, 0, 0]))
    print(accepted([32]))
    print(accepted([56, 24]))
    print(accepted([96]))
    print(accepted([99, 226, 130, 172]))
    print(accepted([131, 1, 130, 2, 3, 161, 97, 107, 245]))
    print(accepted([246]))
    print(accepted([249, 62, 0]))
    print(accepted([250, 71, 195, 80, 0]))
    print(accepted([127, 97, 97, 98, 98, 99, 255]))
    print(accepted([159, 1, 2, 255]))
    print(accepted([191, 97, 107, 1, 255]))

    // Truncation at each structural layer, reserved heads, invalid text,
    // closed DataTree byte identity, duplicate/non-text keys, tags/simple
    // values, stray breaks, trailing roots, and signed-range overflow.
    print(rejected(empty, 0, "$", "CBOR value is missing"))
    print(rejected([28], 0, "$", "indefinite/reserved CBOR length is unsupported by whole-value decoding"))
    print(rejected([26, 0], 2, "$", "CBOR length argument is truncated"))
    print(rejected([27, 128, 0, 0, 0, 0, 0, 0, 0], 0, "$", "CBOR integer is outside Jet Int"))
    print(rejected([59, 128, 0, 0, 0, 0, 0, 0, 0], 0, "$", "CBOR integer is outside Jet Int"))
    print(rejected([65, 0], 0, "$", "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>"))
    print(rejected([98, 97], 2, "$", "CBOR byte/text string is truncated"))
    print(rejected([97, 255], 0, "$", "CBOR text is not UTF-8"))
    print(rejected([127, 98, 97], 3, "$", "CBOR byte/text string chunk is truncated"))
    print(rejected([130, 1], 2, "$[1]", "CBOR value is missing"))
    print(rejected([161, 1, 2], 1, "$", "CBOR map key must be text"))
    print(rejected([162, 97, 97, 1, 97, 97, 2], 4, "$", "duplicate CBOR text map key"))
    print(rejected([161, 97, 97], 3, "$[\"a\"]", "CBOR value is missing"))
    print(rejected([192, 1], 0, "$", "CBOR tags are unsupported"))
    print(rejected([247], 0, "$", "unsupported CBOR simple value 23"))
    print(rejected([255], 0, "$", "CBOR break outside an indefinite container"))
    print(rejected([249, 0], 2, "$", "CBOR Float16 is truncated"))
    print(rejected([1, 2], 1, "$", "trailing CBOR data after root value"))

    // Original wire, not a normalized tree, determines strict acceptance.
    print(canonical_rejected([24, 1], 0, "$", "CBOR argument does not use its shortest form"))
    print(canonical_rejected([120, 1, 97], 0, "$", "CBOR argument does not use its shortest form"))
    print(canonical_rejected([162, 97, 98, 1, 97, 97, 2], 4, "$", "CBOR map keys are not in Core deterministic bytewise order"))
    print(canonical_rejected([250, 63, 192, 0, 0], 0, "$", "CBOR Float does not use its preferred shortest encoding"))
    print(canonical_rejected([159, 1, 255], 0, "$", "indefinite-length CBOR is not Core deterministic"))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_whole_corpus", source, &[], None);
    assert_eq!(code, 0, "CBOR hostile whole-value corpus failed: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 40, "hostile corpus case count drifted: {stdout}");
    assert!(lines.iter().all(|line| *line == "true"), "hostile corpus mismatch: {stdout}");
    assert_eq!(stderr, "");

    let path = dir.join("cbor_whole_corpus.jet");
    fs::write(&path, source).unwrap();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: dev_stdout,
            stderr: dev_stderr,
            exit_code,
        } => assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new())),
        other => panic!("CBOR hostile corpus default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_whole_requested_allocation_stays_under_counting_allocator_ceiling() {
    if !common::have_rustc() {
        eprintln!("note: skipping cbor counting-allocator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_counted_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.cbor as cbor

fn run() {
    options := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 100, require_canonical: false }
    value := cbor.parse([130, 97, 120, 97, 121], ~options) ?? panic("definite parse")
    indefinite := cbor.parse([159, 97, 120, 97, 121, 255], ~options) ?? panic("indefinite parse")
    if cbor.parse([130, 97, 120], options) == {
        .Ok(_) -> panic("truncated array accepted")
        .Err(e) -> print(e.path == "$[1]" && e.reason == "CBOR value is missing")
    }

    roomy := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 256, require_canonical: false }
    if cbor.parse([129, 130, 97, 120], ~roomy) == {
        .Ok(_) -> panic("nested truncation accepted")
        .Err(e) -> print(e.path == "$[0][1]" && e.reason == "CBOR value is missing")
    }
    if cbor.parse([162, 97, 97, 1, 97, 97, 2], ~roomy) == {
        .Ok(_) -> panic("duplicate key accepted")
        .Err(e) -> print(e.path == "$" && e.reason == "duplicate CBOR text map key")
    }
    if cbor.decode<[Int]>([129, 97, 120], roomy) == {
        .Ok(_) -> panic("typed mismatch accepted")
        .Err(e) -> print(e[0].path == "[0]" && e[0].reason.contains("expected Int"))
    }
    print(true)
}
"#;
    let path = dir.join("counted.jet");
    fs::write(&path, source).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(source, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(&shown, source, &diags))
    });
    let parse_renamed = out.rust.replacen("fn jet_enc_cbor_parse(", "fn jet_enc_cbor_parse_inner(", 1);
    assert_ne!(parse_renamed, out.rust, "generated CBOR parser seam changed");
    let renamed = parse_renamed.replacen("fn jet_enc_cbor_decode<T: __jet_Decode>(", "fn jet_enc_cbor_decode_inner<T: __jet_Decode>(", 1);
    assert_ne!(renamed, parse_renamed, "generated CBOR typed decoder seam changed");
    let allocator = r#"
mod jet_cbor_alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub struct CountingAlloc;
    static COUNTING: AtomicBool = AtomicBool::new(false);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    fn add(size: usize) {
        let live = LIVE.fetch_add(size, Ordering::SeqCst) + size;
        let mut peak = PEAK.load(Ordering::SeqCst);
        while live > peak {
            match PEAK.compare_exchange(peak, live, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(next) => peak = next,
            }
        }
    }
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = System.alloc(layout);
            if !ptr.is_null() && COUNTING.load(Ordering::SeqCst) { add(layout.size()); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if COUNTING.load(Ordering::SeqCst) { LIVE.fetch_sub(layout.size(), Ordering::SeqCst); }
            System.dealloc(ptr, layout);
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
            let counting = COUNTING.load(Ordering::SeqCst);
            if counting { LIVE.fetch_sub(old.size(), Ordering::SeqCst); }
            let next = System.realloc(ptr, old, new_size);
            if counting { if next.is_null() { add(old.size()); } else { add(new_size); } }
            next
        }
    }
    pub fn begin() { LIVE.store(0, Ordering::SeqCst); PEAK.store(0, Ordering::SeqCst); COUNTING.store(true, Ordering::SeqCst); }
    pub fn finish() -> usize { COUNTING.store(false, Ordering::SeqCst); PEAK.load(Ordering::SeqCst) }
}
#[global_allocator]
static JET_CBOR_ALLOC: jet_cbor_alloc_probe::CountingAlloc = jet_cbor_alloc_probe::CountingAlloc;
fn jet_enc_cbor_parse(bytes: &Vec<u8>, options: jet_std::CBOROptions) -> Result<jet_std::DataTree, jet_std::CBORError> {
    let ceiling = options.max_bytes as usize;
    jet_cbor_alloc_probe::begin();
    let result = jet_enc_cbor_parse_inner(bytes, options);
    let peak = jet_cbor_alloc_probe::finish();
    assert!(peak <= ceiling, "CBOR requested allocation peak {peak} exceeded {ceiling}");
    result
}
fn jet_enc_cbor_decode<T: __jet_Decode>(bytes: &Vec<u8>, options: jet_std::CBOROptions) -> Result<T, Vec<jet_std::FieldError>> {
    let ceiling = options.max_bytes as usize;
    jet_cbor_alloc_probe::begin();
    let result = jet_enc_cbor_decode_inner(bytes, options);
    let peak = jet_cbor_alloc_probe::finish();
    assert!(peak <= ceiling, "CBOR typed requested allocation peak {peak} exceeded {ceiling}");
    result
}
"#;
    let rs = dir.join("counted.rs");
    let bin = dir.join("counted");
    let generated = renamed.replacen("#![allow(warnings)]", "", 1);
    assert_ne!(generated, renamed, "generated crate attribute changed");
    let rust = format!("#![allow(warnings)]\n{allocator}\n{generated}");
    let mut command = Command::new("rustc");
    common::add_generated_rust(&mut command, &rs, &rust, false, &[]);
    let rustc = command.arg("-o").arg(&bin).output().unwrap();
    assert!(rustc.status.success(), "rustc rejected counted CBOR program:\n{}", String::from_utf8_lossy(&rustc.stderr));
    let run = Command::new(&bin).current_dir(&dir).output().unwrap();
    assert!(run.status.success(), "counted CBOR program failed:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\ntrue\ntrue\ntrue\n");
}
