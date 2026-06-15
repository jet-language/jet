use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn compile_temp(name: &str, src: &str) -> jet::CompileOutput {
    let dir = std::env::temp_dir().join(format!("jet_stdlib_test_{}", std::process::id()));
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
fn canonical_and_short_std_imports_resolve() {
    let out = compile_temp(
        "std_imports.jet",
        r#"
import std.fs as fs;
import jet.std.fs as files;

fn main() {
    print(fs.exists("/tmp"));
    print(files.exists("/tmp"));
}
"#,
    );
    assert!(out.rust.contains("jet_std_fs_exists"));
}

#[test]
fn importing_std_without_calls_is_free_in_codegen() {
    let out = compile_temp(
        "std_import_only.jet",
        r#"
import std.fs as fs;
import std.io as io;
import std.env as env;
import std.process as process;
import std.math as math;
import std.random as random;
import std.time as time;
import std.json as json;

fn main() {
    print("ok");
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
    let dir = std::env::temp_dir().join(format!("jet_stdlib_input_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "input_demo",
        r#"
import std.io as io;

fn main() {
    val name = io.input("name? ") or panic("read failed");
    print("hello, {name}");
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
    let dir = std::env::temp_dir().join(format!("jet_stdlib_time_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, _stderr) = build_and_run(
        &dir,
        "time_random",
        r#"
import std.random as random;
import std.time as time;

fn main() {
    random.seed(42);
    print(random.int(1, 100));
    print(random.float());
    print(time.now());
}
"#,
        &[("LEX_TEST_EPOCH", "1700000000000")],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "9\n0.05534409481976061\n1700000000000\n");
    let _ = fs::remove_dir_all(&dir);
}

/// SL9 / R10: importing every std module without calling it must not bloat the binary.
#[test]
fn importing_all_std_modules_without_calls_stays_hello_world_sized() {
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping std import size test (need jet + rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_stdlib_size_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    fs::write(
        dir.join("hello.jet"),
        "fn main() {\n    print(\"hello, world\");\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("std_import_only.jet"),
        r#"
import std.fs as fs;
import std.io as io;
import std.env as env;
import std.process as process;
import std.math as math;
import std.random as random;
import std.time as time;
import std.json as json;

fn main() {
    print("ok");
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
        .args(["build", "--small", "std_import_only.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(imports.status.success(), "import-only build failed");

    let hello_size = fs::metadata(dir.join("build/hello")).unwrap().len();
    let import_size = fs::metadata(dir.join("build/std_import_only"))
        .unwrap()
        .len();
    assert!(
        import_size <= hello_size.saturating_add(4096),
        "import-only binary ({import_size} bytes) should stay within 4 KiB of hello ({hello_size} bytes)"
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
import std.tasks as tasks;

fn main() {
    val ch: Channel<Int> = tasks.channel();
    val sender = ch.sender();
    val producer = tasks.spawn(take(sender) () => {
        for i in 1..1000 {
            sender.send(i);
        }
    });
    producer.join();
    var total = 0;
    for i in 1..1000 {
        total = total + (ch.receive() or panic("channel closed"));
    }
    print(total);
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "channel stress failed: {stderr}");
    assert_eq!(stdout, "500500\n");
    let _ = fs::remove_dir_all(&dir);
}
