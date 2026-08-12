#[test]
fn random_and_time_output_pins_with_seed_and_epoch() {
    let have_rustc = common::have_rustc();
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

/// #1788/#1781: an immutable `::` binding of a `core.random` call must read
/// the runtime-seeded PRNG exactly like a mutable `:=` binding does. Before
/// the fix, sema's D-VERDICT-1308-1 implicit fold treated `random.float()` as
/// a foldable pure call and baked its value at compile time from a disjoint
/// ambient interpreter PRNG, so two identical `seed(11); x :: random.float()`
/// pairs never matched and never landed on the seeded stream either.
#[test]
fn immutable_binding_of_random_call_reads_the_seeded_stream() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping immutable-random-binding test (need rustc)");
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_random_immutable_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "random_immutable",
        r#"
use core.random as random

fn run() {
    random.seed(11)
    a :: random.float()
    random.seed(11)
    b :: random.float()
    print(a == b)
    random.seed(11)
    c := random.float()
    print(a == c)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout, "true\ntrue\n",
        "reseeded `::` bindings must match each other and the `:=` binding's seeded draw"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// #1799: an immutable `::` binding of `date.today()` must read the runtime
/// clock. Before the fix, D-VERDICT-1308-1 folded the ambient wall-clock read
/// into the generated literal, so the artifact kept the build date forever.
#[test]
fn immutable_binding_of_date_today_reads_the_runtime_clock() {
    let src = r#"
use core.time.date as date

fn run() {
    a :: date.today()
    b :: date.today()
    print(a == b)
}
"#;
    let compiled = compile_temp("date_today_immutable", src);
    let user_run = compiled
        .rust
        .split_once("pub fn __jet_run() {")
        .and_then(|(_, body)| body.split_once("\n}\n").map(|(body, _)| body))
        .expect("generated Rust must contain the __jet_run body");
    assert_eq!(
        user_run.matches("JetDate::today_utc()").count(),
        2,
        "both immutable date.today() calls must remain runtime reads:\n{user_run}"
    );

    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping immutable-date-today-binding test (need rustc)");
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_date_today_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "date_today_immutable", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "true\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn random_distribution_surface_is_deterministic() {
    let have_rustc = common::have_rustc();
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
    let have_rustc = common::have_rustc();
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
    print(json.canonical(data) ?? panic("value is not canonical JSON"))
    print(json.events(data).contains("object_start $"))
    rows := jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")
    print(rows.len())
    print(jsonl.to_string(rows).contains("\"a\":1"))
    source := "<r xmlns=\"urn:r\" xmlns:h=\"urn:h\" h:a=\"x&amp;y\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><h:c/></r>"
    doc := xml.parse(source) ?? panic("xml")
    print(xml.to_string(doc))
    print((doc.field("$xml") ?? panic("document tag")).text() ?? "bad")
    root := (doc.field("children") ?? panic("document children")).at(0) ?? panic("root")
    name := root.field("name") ?? panic("root name")
    print((name.field("namespace_uri") ?? panic("root namespace")).text() ?? "bad")
    content := root.field("children") ?? panic("root children")
    entity := content.at(1) ?? panic("entity")
    comment := content.at(2) ?? panic("comment")
    cdata := content.at(3) ?? panic("cdata")
    pi := content.at(4) ?? panic("pi")
    print((entity.field("$xml") ?? panic("entity tag")).text() ?? "bad")
    print((comment.field("$xml") ?? panic("comment tag")).text() ?? "bad")
    print((cdata.field("$xml") ?? panic("cdata tag")).text() ?? "bad")
    print((pi.field("$xml") ?? panic("pi tag")).text() ?? "bad")
    encoded := cbor.to_bytes(data) ?? panic("cbor encode")
    print(encoded.len() > 0)
    decoded := cbor.parse(encoded) ?? panic("cbor parse")
    print(json.canonical(decoded) ?? panic("value is not canonical JSON"))
    bytes :: [U8].{ 104, 105 }
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
        "{\"a\":1,\"b\":2}\ntrue\n2\ntrue\n<r xmlns=\"urn:r\" xmlns:h=\"urn:h\" h:a=\"x&amp;y\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><h:c/></r>\ndocument\nurn:r\nentity_ref\ncomment\ncdata\nprocessing_instruction\ntrue\n{\"a\":1,\"b\":2}\naGk\n2\nNBUQ====\n2\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_dual_limits_validate_in_ratified_order_and_fuse_stronger_bounds() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML dual-limits test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_xml_dual_limits_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let probe = dir.join("probe.xml");
    fs::write(&probe, "<a><b><c/></b></a>").unwrap();
    let probe = probe.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn run() {{
    // EncodingLimits fail first even when XMLLimits are also illegal.
    enc_bad := encoding.EncodingLimits.safe()
    enc_bad.buffer_bytes = 1
    xml_bad := xml.XMLParseOptions.safe()
    xml_bad.limits.max_depth = 0
    input1 :: files.open("{probe}") ?? panic("open1")
    if xml.reader(^input1, enc_bad, xml_bad) == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{
            print(error.format == encoding.EncodingFormat.XML)
            print(error.kind == encoding.EncodingErrorKind.Limit)
            print(error.byte_offset)
            print(error.line ?? -1)
            print(error.column ?? -1)
            print(error.path)
            print(error.reason)
        }}
    }}

    // EncodingLimits ok → XMLLimits field order: max_depth before max_nodes.
    enc_ok := encoding.EncodingLimits.safe()
    xml_depth := xml.XMLParseOptions.safe()
    xml_depth.limits.max_depth = 0
    xml_depth.limits.max_nodes = 0
    input2 :: files.open("{probe}") ?? panic("open2")
    if xml.reader(^input2, enc_ok, xml_depth) == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{
            print(error.format == encoding.EncodingFormat.XML)
            print(error.kind == encoding.EncodingErrorKind.Limit)
            print(error.byte_offset)
            print(error.line ?? -1)
            print(error.column ?? -1)
            print(error.path)
            print(error.reason)
        }}
    }}

    // Cross-field XMLLimits after ranges.
    enc_ok2 := encoding.EncodingLimits.safe()
    xml_cross := xml.XMLParseOptions.safe()
    xml_cross.limits.max_depth = 2
    xml_cross.limits.max_entity_depth = 3
    input3 :: files.open("{probe}") ?? panic("open3")
    if xml.reader(^input3, enc_ok2, xml_cross) == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{
            print(error.format == encoding.EncodingFormat.XML)
            print(error.kind == encoding.EncodingErrorKind.Limit)
            print(error.byte_offset)
            print(error.line ?? -1)
            print(error.column ?? -1)
            print(error.path)
            print(error.reason)
        }}
    }}

    // Encoding depth tighter than XML depth: error names the fused bound.
    // XMLLimits must be self-valid before EncodingLimits fusion.
    enc_tight := encoding.EncodingLimits.safe()
    enc_tight.max_depth = 2
    xml_loose := xml.XMLParseOptions.safe()
    xml_loose.limits.max_depth = 8
    xml_loose.limits.max_entity_depth = 8
    input4 :: files.open("{probe}") ?? panic("open4")
    reader :: xml.reader(^input4, enc_tight, xml_loose) ?? panic("fused reader")
    loop true {{
        result :: reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("depth-missed"); break }}
                }}
            }}
            .Err(error) -> {{
                print(error.kind == encoding.EncodingErrorKind.Limit)
                print(error.reason)
                break
            }}
        }}
    }}

    // XML depth tighter than Encoding depth.
    enc_loose := encoding.EncodingLimits.safe()
    xml_tight := xml.XMLParseOptions.safe()
    xml_tight.limits.max_depth = 1
    xml_tight.limits.max_entity_depth = 1
    deep_input :: files.open("{probe}") ?? panic("open deep")
    deep_reader :: xml.reader(^deep_input, enc_loose, xml_tight) ?? panic("xml-tight reader")
    loop true {{
        result :: deep_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("xml-depth-missed"); break }}
                }}
            }}
            .Err(error) -> {{
                print(error.kind == encoding.EncodingErrorKind.Limit)
                print(error.reason)
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_dual_limits", &source, &[], None);
    assert_eq!(code, 0, "XML dual-limits test failed: {stderr}");
    assert_eq!(
        stdout,
        concat!(
            "true\n",
            "true\n",
            "0\n",
            "-1\n",
            "-1\n",
            "\n",
            "buffer_bytes 1 is outside 4096..16777216\n",
            "true\n",
            "true\n",
            "0\n",
            "-1\n",
            "-1\n",
            "\n",
            "XML limit `max_depth` must be between 1 and 4096\n",
            "true\n",
            "true\n",
            "0\n",
            "-1\n",
            "-1\n",
            "\n",
            "XML limit `max_entity_depth` exceeds `max_depth`\n",
            "true\n",
            "XML element nesting exceeds max_depth (2)\n",
            "true\n",
            "XML element nesting exceeds max_depth (1)\n",
        )
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_whole_byte_verbs_match_comptime_aot_and_dev() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML whole-byte parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_xml_whole_bytes_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.xml as xml

fn same_bytes(left: [U8], right: [U8]) => Bool {
    if left.len() != right.len() { return false }
    loop index, 0..<left.len() {
        if left[index] != right[index] { return false }
    }
    return true
}

fn summarize() => String {
    plain :: [U8].{ 60, 114, 62, 111, 107, 60, 47, 114, 62 }
    utf8_bom :: [U8].{ 239, 187, 191, 60, 63, 120, 109, 108, 32, 118, 101, 114, 115, 105, 111, 110, 61, 39, 49, 46, 48, 39, 32, 101, 110, 99, 111, 100, 105, 110, 103, 61, 39, 85, 84, 70, 45, 56, 39, 63, 62, 60, 114, 62, 195, 169, 240, 159, 153, 130, 60, 47, 114, 62 }
    utf16 :: [U8].{ 255, 254, 60, 0, 63, 0, 120, 0, 109, 0, 108, 0, 32, 0, 118, 0, 101, 0, 114, 0, 115, 0, 105, 0, 111, 0, 110, 0, 61, 0, 39, 0, 49, 0, 46, 0, 48, 0, 39, 0, 32, 0, 101, 0, 110, 0, 99, 0, 111, 0, 100, 0, 105, 0, 110, 0, 103, 0, 61, 0, 39, 0, 85, 0, 84, 0, 70, 0, 45, 0, 49, 0, 54, 0, 39, 0, 63, 0, 62, 0, 60, 0, 114, 0, 62, 0, 233, 0, 61, 216, 66, 222, 60, 0, 47, 0, 114, 0, 62, 0 }
    conflict :: [U8].{ 255, 254, 60, 0, 63, 0, 120, 0, 109, 0, 108, 0, 32, 0, 118, 0, 101, 0, 114, 0, 115, 0, 105, 0, 111, 0, 110, 0, 61, 0, 39, 0, 49, 0, 46, 0, 48, 0, 39, 0, 32, 0, 101, 0, 110, 0, 99, 0, 111, 0, 100, 0, 105, 0, 110, 0, 103, 0, 61, 0, 39, 0, 85, 0, 84, 0, 70, 0, 45, 0, 56, 0, 39, 0, 63, 0, 62, 0, 60, 0, 114, 0, 47, 0, 62, 0 }

    plain_doc :: xml.parse_bytes(plain) ?? panic("plain parse")
    plain_out :: xml.to_bytes(plain_doc) ?? panic("plain render")
    utf8_doc :: xml.parse_bytes(utf8_bom) ?? panic("UTF-8 BOM parse")
    utf8_out :: xml.to_bytes(utf8_doc, xml.XMLRenderOptions.{ encoding: .UTF8BOM, lexical: .PreserveValid }) ?? panic("UTF-8 BOM render")
    utf16_doc :: xml.parse_bytes(utf16) ?? panic("UTF-16 parse")
    utf16_out :: xml.to_bytes(utf16_doc, xml.XMLRenderOptions.{ encoding: .UTF16LE, lexical: .PreserveValid }) ?? panic("UTF-16 render")

    conflict_result :: xml.parse_bytes(conflict)
    if conflict_result == {
        .Ok(_) -> return "encoding-conflict-missed"
        .Err(error) -> {
            reason_ok :: error.reason == "XML declaration conflicts with detected input encoding"
            return "{same_bytes(plain_out, plain)}|{same_bytes(utf8_out, utf8_bom)}|{same_bytes(utf16_out, utf16)}|{reason_ok}|{error.byte_offset}|{error.line}|{error.column}|{error.path}|{error.reason}"
        }
    }
    return "unreachable"
}

$expected :: summarize()

fn run() {
    print(expected)
    print(summarize())
}
"#;
    let expected = concat!(
        "true|true|true|true|2|1|1|$|XML declaration conflicts with detected input encoding\n",
        "true|true|true|true|2|1|1|$|XML declaration conflicts with detected input encoding\n",
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_whole_bytes", source, &[], None);
    assert_eq!(code, 0, "XML whole-byte AOT fixture failed: {stderr}");
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");

    let dev_path = dir.join("xml_whole_bytes.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout, stderr), (0, expected.to_string(), String::new()));
        }
        other => panic!("XML whole-byte default-dev fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_10_fifth_edition_char_errors_match_comptime_aot_and_dev() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML character parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_chars_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.xml as xml

fn show(result: DataTree ? XMLError) => String {
    if result == {
        .Ok(_) -> { return "accepted" }
        .Err(error) -> {
            return "{error.byte_offset}|{error.line}|{error.column}|{error.path}|{error.reason}"
        }
    }
    return "unreachable"
}

$numeric :: show(xml.parse("<r>&#0;</r>"))
$attribute :: show(xml.parse("<r a='&#0;'/>"))
$namespace :: show(xml.parse("<r xmlns='&#0;'/>"))

fn run() {
    runtime_numeric :: show(xml.parse("<r>&#0;</r>"))
    runtime_attribute :: show(xml.parse("<r a='&#0;'/>"))
    runtime_namespace :: show(xml.parse("<r xmlns='&#0;'/>"))
    print("{$numeric}|{runtime_numeric}")
    print("{$attribute}|{runtime_attribute}")
    print("{$namespace}|{runtime_namespace}")
}
"#;
    let expected = concat!(
        "3|1|4|$/r|invalid numeric character reference|3|1|4|$/r|invalid numeric character reference\n",
        "6|1|7|$|invalid numeric character reference|6|1|7|$|invalid numeric character reference\n",
        "10|1|11|$|invalid numeric character reference|10|1|11|$|invalid numeric character reference\n",
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_chars", &source, &[], None);
    assert_eq!(code, 0, "XML character AOT fixture failed: {stderr}");
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");

    let dev_path = dir.join("xml_chars.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout, stderr), (0, expected.to_string(), String::new()));
        }
        other => panic!("XML character default-dev fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_attribute_whitespace_normalization_matches_comptime_aot_and_dev() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML attribute normalization parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_xml_attribute_normalization_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.xml as xml

fn summarize(source: String) => String {
    doc := xml.parse(source) ?? panic("xml")
    root := (doc.field("children") ?? panic("document children")).at(0) ?? panic("root")
    namespace := ((root.field("namespaces") ?? panic("namespaces")).at(0) ?? panic("namespace")).field("namespace_uri") ?? panic("namespace URI")
    attributes := root.field("attributes") ?? panic("attributes")
    literal := ((attributes.at(0) ?? panic("literal attribute")).field("normalized_value") ?? panic("literal normalized value")).text() ?? "bad"
    reference := ((attributes.at(1) ?? panic("reference attribute")).field("normalized_value") ?? panic("reference normalized value")).text() ?? "bad"
    namespace_ok := (namespace.text() ?? "bad") == "urn: foo bar"
    literal_ok := literal == "A B C D E"
    lexical_ok := xml.to_string(doc) == source
    return "{namespace_ok}|{literal_ok}|{reference.len()}|{lexical_ok}"
}

$cr :: String.from_bytes([13]) ?? panic("CR")
$close :: "/>"
$source :: "<r xmlns='urn:\tfoo\nbar' a='A\tB\nC{$cr}\nD{$cr}E' b='&#xD;&#xA;&#x9;'{$close}"
$normalized :: summarize($source)

fn run() {
    runtime := summarize($source)
    print("{$normalized}|{runtime}")
}
"#;
    let expected = "true|true|3|true|true|true|3|true\n";
    let (code, stdout, stderr) =
        build_and_run(&dir, "xml_attribute_normalization", source, &[], None);
    assert_eq!(
        code, 0,
        "XML attribute normalization AOT fixture failed: {stderr}"
    );
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");

    let dev_path = dir.join("xml_attribute_normalization.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(
                (exit_code, stdout, stderr),
                (0, expected.to_string(), String::new())
            );
        }
        other => panic!("XML attribute normalization default-dev fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn base_decoders_preserve_2026_union_with_comptime_aot_and_dev_parity() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping base decoder parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_base_decoder_parity_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"corelib_base_decoder\"\nversion: \"0.1.0\"\nedition: \"2026\"\n",
    )
    .unwrap();
    let source = r#"
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn show64(text: String) => String {
    if base64.decode(text) == {
        .Ok(bytes) -> { return "OK:{bytes}" }
        .Err(reason) -> { return "ERR:{reason}" }
    }
    return "unreachable"
}

fn show64url(text: String) => String {
    if base64.decode_url(text) == {
        .Ok(bytes) -> { return "OK:{bytes}" }
        .Err(reason) -> { return "ERR:{reason}" }
    }
    return "unreachable"
}

fn show32(text: String) => String {
    if base32.decode(text) == {
        .Ok(bytes) -> { return "OK:{bytes}" }
        .Err(reason) -> { return "ERR:{reason}" }
    }
    return "unreachable"
}

$standard_ws :: show64("Z g = =\n")
$standard_unpadded :: show64("Zg")
$standard_interior :: show64("Zg=A")
$standard_excess :: show64("Zg====")
$standard_bits :: show64("Zh==")
$standard_padding :: show64("=AAA")
$standard_alphabet :: show64("Zg-=")
$standard_size :: show64("A")
$url_outer_ws :: show64url(" \tZg==\n")
$url_interior :: show64url("Zg=A")
$url_standard_alphabet :: show64url("+w")
$url_bits :: show64url("Zh")
$url_padding :: show64url("=AAA")
$url_size :: show64url("A")
$base32_loose :: show32("m=y======\n")
$base32_bits :: show32("MZ======")
$base32_short :: show32("A")
$base32_alphabet :: show32("M0======")

fn run() {
    r_standard_ws := show64("Z g = =\n")
    r_standard_unpadded := show64("Zg")
    r_standard_interior := show64("Zg=A")
    r_standard_excess := show64("Zg====")
    r_standard_bits := show64("Zh==")
    r_standard_padding := show64("=AAA")
    r_standard_alphabet := show64("Zg-=")
    r_standard_size := show64("A")
    r_url_outer_ws := show64url(" \tZg==\n")
    r_url_interior := show64url("Zg=A")
    r_url_standard_alphabet := show64url("+w")
    r_url_bits := show64url("Zh")
    r_url_padding := show64url("=AAA")
    r_url_size := show64url("A")
    r_base32_loose := show32("m=y======\n")
    r_base32_bits := show32("MZ======")
    r_base32_short := show32("A")
    r_base32_alphabet := show32("M0======")
    print("{$standard_ws}|{r_standard_ws}")
    print("{$standard_unpadded}|{r_standard_unpadded}")
    print("{$standard_interior}|{r_standard_interior}")
    print("{$standard_excess}|{r_standard_excess}")
    print("{$standard_bits}|{r_standard_bits}")
    print("{$standard_padding}|{r_standard_padding}")
    print("{$standard_alphabet}|{r_standard_alphabet}")
    print("{$standard_size}|{r_standard_size}")
    print("{$url_outer_ws}|{r_url_outer_ws}")
    print("{$url_interior}|{r_url_interior}")
    print("{$url_standard_alphabet}|{r_url_standard_alphabet}")
    print("{$url_bits}|{r_url_bits}")
    print("{$url_padding}|{r_url_padding}")
    print("{$url_size}|{r_url_size}")
    print("{$base32_loose}|{r_base32_loose}")
    print("{$base32_bits}|{r_base32_bits}")
    print("{$base32_short}|{r_base32_short}")
    print("{$base32_alphabet}|{r_base32_alphabet}")
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "base_decoder_parity", source, &[], None);
    assert_eq!(code, 0, "base decoder AOT parity fixture failed: {stderr}");
    let expected = concat!(
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "ERR:invalid base64 at byte 0: padding may appear only at the end|ERR:invalid base64 at byte 0: padding may appear only at the end\n",
        "ERR:invalid base64 at byte 2: byte 0x2D is not in the standard base64 alphabet|ERR:invalid base64 at byte 2: byte 0x2D is not in the standard base64 alphabet\n",
        "ERR:invalid base64 at byte 1: encoded length cannot represent whole bytes|ERR:invalid base64 at byte 1: encoded length cannot represent whole bytes\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[251]|OK:[251]\n",
        "OK:[102]|OK:[102]\n",
        "ERR:invalid base64url at byte 0: padding may appear only at the end|ERR:invalid base64url at byte 0: padding may appear only at the end\n",
        "ERR:invalid base64url at byte 1: encoded length cannot represent whole bytes|ERR:invalid base64url at byte 1: encoded length cannot represent whole bytes\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[]|OK:[]\n",
        "ERR:invalid base32 at byte 1: byte 0x30 is not in the base32 alphabet|ERR:invalid base32 at byte 1: byte 0x30 is not in the base32 alphabet\n",
    );
    assert_eq!(stdout, expected);
    let dev_path = dir.join("base_decoder_parity.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => assert_eq!((exit_code, stdout, stderr), (0, expected.to_string(), String::new())),
        other => panic!("base decoder default-dev parity fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_stream_reader_is_incremental_exact_and_terminal() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML stream test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let boundary_paths = (4078..=4110)
        .map(|padding| {
            let path = dir.join(format!("boundary-{padding}.xml"));
            fs::write(
                &path,
                format!(
                    "{}<r xmlns=\"urn:r\" a=\"x&amp;y\">é</r>",
                    " ".repeat(padding)
                ),
            )
            .unwrap();
            format!("\"{}\"", path.to_string_lossy().replace('\\', "\\\\"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let malformed = dir.join("malformed.xml");
    fs::write(&malformed, "<r>").unwrap();
    let invalid_char = dir.join("invalid-char.xml");
    fs::write(&invalid_char, b"<r>\x01</r>").unwrap();
    let limited = dir.join("limited.xml");
    fs::write(&limited, "<root>text</root>").unwrap();
    let encoding_conflict = dir.join("encoding-conflict.xml");
    let mut encoding_conflict_bytes = vec![0xff, 0xfe];
    encoding_conflict_bytes.extend(
        "<?xml version='1.0' encoding='UTF-8'?><r/>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    fs::write(&encoding_conflict, encoding_conflict_bytes).unwrap();
    let malformed = malformed.to_string_lossy().replace('\\', "\\\\");
    let invalid_char = invalid_char.to_string_lossy().replace('\\', "\\\\");
    let limited = limited.to_string_lossy().replace('\\', "\\\\");
    let encoding_conflict = encoding_conflict.to_string_lossy().replace('\\', "\\\\");

    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn run() {{
    paths :: [String].{{ {boundary_paths} }}
    passed := 0
    loop path, paths {{
        input :: files.open(path) ?? panic("open boundary")
        reader :: xml.reader(^input) ?? panic("reader defaults")
        document_start := false
        root_start := false
        document_end := false
        loop true {{
            maybe :: reader.next() ?? panic("boundary next")
            if maybe == {{
                Val(event) -> {{
                    event_kind := (event.field("$xml_event") ?? panic("event tag")).text() ?? ""
                    if event_kind == "document_start" {{
                        wire_encoding := (event.field("encoding") ?? panic("encoding")).text() ?? ""
                        document_start = wire_encoding == "UTF-8"
                    }}
                    if event_kind == "element_start" {{
                        name := event.field("name") ?? panic("name")
                        local := (name.field("local") ?? panic("local")).text() ?? ""
                        namespace := (name.field("namespace_uri") ?? panic("namespace")).text() ?? ""
                        root_start = local == "r" && namespace == "urn:r"
                    }}
                    if event_kind == "document_end" {{ document_end = true }}
                }}
                None -> {{ break }}
            }}
        }}
        eof_again :: reader.next() ?? panic("fused eof")
        if eof_again == {{
            Val(_) -> {{}}
            None -> {{ if document_start && root_start && document_end {{ passed++ }} }}
        }}
    }}
    print(passed)

    malformed_input :: files.open("{malformed}") ?? panic("open malformed")
    malformed_reader :: xml.reader(^malformed_input) ?? panic("malformed reader")
    loop true {{
        result :: malformed_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("malformed-missed"); break }}
                }}
            }}
            .Err(first) -> {{
                again :: malformed_reader.next()
                if again == {{
                    .Ok(_) -> {{ print("terminal-missed") }}
                    .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
                }}
                break
            }}
        }}
    }}

    invalid_char_input :: files.open("{invalid_char}") ?? panic("open invalid character")
    invalid_char_reader :: xml.reader(^invalid_char_input) ?? panic("invalid character reader")
    loop true {{
        result :: invalid_char_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("invalid-character-missed"); break }}
                }}
            }}
            .Err(first) -> {{
                print(first.kind == encoding.EncodingErrorKind.Syntax)
                print(first.byte_offset)
                print(first.line ?? -1)
                print(first.column ?? -1)
                print(first.path)
                print(first.reason)
                again :: invalid_char_reader.next()
                if again == {{
                    .Ok(_) -> {{ print("invalid-character-terminal-missed") }}
                    .Err(second) -> {{ print(first.reason == second.reason) }}
                }}
                break
            }}
        }}
    }}

    total_limits := encoding.EncodingLimits.safe()
    total_limits.max_total_bytes = Val(6)
    total_input :: files.open("{limited}") ?? panic("open total")
    total_reader :: xml.reader(^total_input, total_limits, xml.XMLParseOptions.safe()) ?? panic("total reader")
    loop true {{
        result :: total_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("total-missed"); break }}
                }}
            }}
            .Err(first) -> {{
                again :: total_reader.next()
                if again == {{
                    .Ok(_) -> {{ print("total-terminal-missed") }}
                    .Err(second) -> {{ print(first.byte_offset); print(first.reason == second.reason) }}
                }}
                break
            }}
        }}
    }}

    conflict_input :: files.open("{encoding_conflict}") ?? panic("open encoding conflict")
    conflict_reader :: xml.reader(^conflict_input) ?? panic("encoding conflict reader")
    conflict_start :: conflict_reader.next() ?? panic("encoding conflict document start")
    if conflict_start == None {{ panic("missing document start") }}
    conflict :: conflict_reader.next()
    if conflict == {{
        .Ok(_) -> {{ print("encoding-conflict-missed") }}
        .Err(error) -> {{
            print(error.kind == encoding.EncodingErrorKind.Syntax)
            print(error.byte_offset)
            print(error.reason)
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_stream", &source, &[], None);
    assert_eq!(code, 0, "XML stream test failed: {stderr}");
    assert_eq!(
        stdout,
        "33\ntrue\ntrue\n3\n1\n4\n$/r\nXML contains forbidden character U+0001\ntrue\n7\ntrue\ntrue\n2\nXML declaration conflicts with detected input encoding\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_stream_writer_and_canonical_surface_run_end_to_end() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML writer test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_writer_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.xml");
    let output = dir.join("output.xml");
    let utf16 = dir.join("output-utf16.xml");
    let source = "<?xml version='1.0'?><r xmlns:p='urn:p' p:a='x&amp;y'>z<p:e/></r>";
    fs::write(&input, source).unwrap();
    let source_code = format!(r#"
use core.encoding.xml as xml
use core.encoding as encoding
use core.files as files

fn run() {{
    input :: files.open("{}") ?? panic("open")
    output :: files.create("{}") ?? panic("create")
    reader :: xml.reader(^input) ?? panic("reader")
    writer :: xml.writer(^output) ?? panic("writer")
    loop true {{
        maybe :: reader.next() ?? panic("next")
        if maybe == {{
            Val(event) -> {{ writer.write(event) ?? panic("write") }}
            None -> {{ break }}
        }}
    }}
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("idempotent finish")
    input16 :: files.open("{}") ?? panic("open utf16 source")
    output16 :: files.create("{}") ?? panic("create utf16")
    reader16 :: xml.reader(^input16) ?? panic("reader utf16")
    render16 := xml.XMLRenderOptions.{{ encoding: .UTF16LE, lexical: .Deterministic }}
    writer16 :: xml.writer(^output16, encoding.EncodingLimits.safe(), render16) ?? panic("writer utf16")
    loop true {{
        maybe :: reader16.next() ?? panic("next utf16")
        if maybe == {{
            Val(event) -> {{ writer16.write(event) ?? panic("write utf16") }}
            None -> {{ break }}
        }}
    }}
    writer16.finish() ?? panic("finish utf16")
    tree :: xml.parse("<r xmlns:q='urn:q' q:z='2' a='1'><e/></r>") ?? panic("parse")
    options := xml.XMLCanonical.{{ mode: .Exclusive10, comments: false, inclusive_prefixes: ["q"] }}
    print(xml.canonical(tree, options) ?? panic("canonical"))
}}
"#, input.to_string_lossy().replace('\\', "\\\\"), output.to_string_lossy().replace('\\', "\\\\"), input.to_string_lossy().replace('\\', "\\\\"), utf16.to_string_lossy().replace('\\', "\\\\"));
    let (code, stdout, stderr) = build_and_run(&dir, "xml_writer", &source_code, &[], None);
    assert_eq!(code, 0, "XML writer test failed: {stderr}");
    assert_eq!(stdout, "<r xmlns:q=\"urn:q\" a=\"1\" q:z=\"2\"><e></e></r>\n");
    assert_eq!(fs::read(&output).unwrap(), source.as_bytes());
    let deterministic =
        "<?xml version=\"1.0\"?><r xmlns:p='urn:p' p:a='x&amp;y'>z<p:e/></r>";
    let mut expected16 = vec![0xff, 0xfe];
    expected16.extend(deterministic.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(fs::read(&utf16).unwrap(), expected16);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_reader_writer_hostile_state_and_exclusive_c14n() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML hostile/c14n surface test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_hostile_c14n_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("round.xml");
    fs::write(
        &input,
        "<?xml version='1.0'?>\n<!--c-->\n<root xmlns:a='urn:a' xmlns:b='urn:b'><a:child b:x='1'/></root>\n",
    )
    .unwrap();
    let input = input.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn run() {{
    // Fold/unfold round trip through XMLReader/XMLWriter keeps order + lexical.
    input :: files.open("{input}") ?? panic("open")
    out_path := "{input}.out"
    output :: files.create(out_path) ?? panic("create")
    reader :: xml.reader(^input) ?? panic("reader")
    writer :: xml.writer(^output) ?? panic("writer")
    loop true {{
        maybe :: reader.next() ?? panic("next")
        if maybe == {{
            Val(event) -> {{ writer.write(event) ?? panic("write") }}
            None -> {{ break }}
        }}
    }}
    writer.finish() ?? panic("finish")
    print(files.read(out_path) ?? panic("read out"))

    // Hostile: document_end before document_start → State, no bytes.
    bad_out :: files.create("{input}.bad") ?? panic("bad create")
    bad :: xml.writer(^bad_out) ?? panic("bad writer")
    end := DataTree.Object(["$xml_event": DataTree.Text("document_end")])
    if bad.write(end) == {{
        .Ok(_) -> {{ print("hostile-missed") }}
        .Err(error) -> {{
            print(error.kind == encoding.EncodingErrorKind.State)
            print(error.reason)
        }}
    }}

    // Exclusive C14N omits unused xmlns on ancestors; utilized prefixes move down.
    tree :: xml.parse("<root xmlns:a='urn:a' xmlns:b='urn:b'><a:child b:x='1'/></root>") ?? panic("parse")
    options := xml.XMLCanonical.{{ mode: .Exclusive10, comments: false, inclusive_prefixes: [] }}
    print(xml.canonical(tree, options) ?? panic("canonical"))

    // InclusiveNamespaces PrefixList forces unused b onto the apex.
    forced := xml.XMLCanonical.{{ mode: .Exclusive10, comments: false, inclusive_prefixes: ["b"] }}
    print(xml.canonical(tree, forced) ?? panic("forced"))
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_hostile_c14n", &source, &[], None);
    assert_eq!(code, 0, "XML hostile/c14n surface failed: {stderr}");
    assert_eq!(
        stdout,
        "<?xml version='1.0'?>\n<!--c-->\n<root xmlns:a='urn:a' xmlns:b='urn:b'><a:child b:x='1'/></root>\n\ntrue\nXML writer expects document_start first\n<root><a:child xmlns:a=\"urn:a\" xmlns:b=\"urn:b\" b:x=\"1\"></a:child></root>\n<root xmlns:b=\"urn:b\"><a:child xmlns:a=\"urn:a\" b:x=\"1\"></a:child></root>\n"
    );
    let _ = fs::remove_dir_all(&dir);
}


