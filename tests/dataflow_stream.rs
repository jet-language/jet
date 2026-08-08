//! D-DATAFLOW1=A: bounded DataStream pull + typed DataError policy.
use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;

fn build_and_run(dir: &PathBuf, name: &str, src: &str) -> (i32, String, String) {
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
    let out = Command::new(&bin).current_dir(dir).output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn data_stream_limits_and_typed_errors() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping dataflow stream test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_dataflow_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let events = dir.join("events.csv");
    let events_path = events.to_str().unwrap();
    let src = format!(
        r#"
use core.data as data
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

#Codable
struct Event {{
    service: String
    latency_ms: Float
}}

fn write_fixture(path: String) {{
    output :: files.create(path) ?? panic("create")
    writer :: csv.writer(^output, encoding.EncodingLimits.safe()) ?? panic("writer")
    writer.write(["service", "latency_ms"]) ?? panic("header")
    writer.write(["api", "10.0"]) ?? panic("r1")
    writer.write(["api", "20.0"]) ?? panic("r2")
    writer.write(["db", "5.0"]) ?? panic("r3")
    writer.write(["api", "30.0"]) ?? panic("r4")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
}}

fn run() {{
    path :: "{events_path}"
    write_fixture(path)
    input :: files.open(path) ?? panic("open")
    limits := data.DataLimits.safe()
    limits.max_groups = 1
    limits.max_output_rows = 10
    reader :: data.csv_reader<Event>(input, limits) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {{
        Val(row) -> print("first:{{row.service}}:{{row.latency_ms}}")
        None -> panic("expected row")
    }}
    groups := data.group_mean(reader, (e) => e.service, (e) => e.latency_ms)
    if groups == {{
        Ok(_) -> print("groups:ok")
        Err(error) -> print("groups:{{error.operation}}:{{error.reason}}")
    }}
    empty := data.mean([Float].{{}})
    if empty == {{
        Ok(_) -> print("mean:ok")
        Err(error) -> print("mean:{{error.operation}}:{{error.reason}}")
    }}
    bad_q := data.quantile([1.0, 2.0], 1.5)
    if bad_q == {{
        Ok(_) -> print("q:ok")
        Err(error) -> print("q:{{error.operation}}:{{error.reason}}")
    }}
    bad_w := data.rolling_mean([1.0, 2.0], 0)
    if bad_w == {{
        Ok(_) -> print("roll:ok")
        Err(error) -> print("roll:{{error.operation}}:{{error.reason}}")
    }}
}}
"#,
        events_path = events_path
    );
    let (code, stdout, stderr) = build_and_run(&dir, "data_stream_bounds", &src);
    assert_eq!(code, 0, "dataflow stream program failed: {stderr}");
    assert_eq!(
        stdout,
        "first:api:10.0\ngroups:group_mean:max_groups 1 exceeded\nmean:mean:mean of empty data is undefined\nq:quantile:quantile q must be a finite value in 0.0 through 1.0\nroll:rolling_mean:rolling width must be positive\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rolling_mean_nonfinite_matches_aot_and_default_dev() {
    if !common::have_rustc() {
        eprintln!("note: skipping rolling_mean parity test (need rustc)");
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_dataflow_rolling_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.data as data

fn run() {
    nonfinite := data.rolling_mean([1.0, Float.NAN, 3.0], 2)
    if nonfinite == {
        .Ok(_) -> print("unexpected ok")
        .Err(error) -> print("{error}")
    }
    compensated := data.rolling_mean([10000000000000000.0, 1.0, -10000000000000000.0], 3) ?? panic("compensated")
    print(compensated[2])
    overflow := data.rolling_mean([Float.MAX, Float.MAX], 2)
    if overflow == {
        .Ok(_) -> print("unexpected overflow ok")
        .Err(error) -> print("{error}")
    }
    zero := data.rolling_mean([-0.0, -0.0], 2) ?? panic("zero")
    print(zero[1])
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "rolling_nonfinite", source);
    assert_eq!(code, 0, "rolling_mean AOT failed: {stderr}");
    assert_eq!(
        stdout,
        "NonFinite rolling_mean, index 1: numeric input must be finite\n0.3333333333333333\nOverflow sum: finite overflow while summing\n0.0\n"
    );
    let path = dir.join("rolling_nonfinite.jet");
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: dev_stdout,
            stderr: dev_stderr,
            exit_code,
        } => assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new())),
        other => panic!("rolling_mean default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}
