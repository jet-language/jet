mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn compile(dir: &Path, name: &str, src: &str) -> PathBuf {
    let source = dir.join(format!("{name}.jet"));
    fs::write(&source, src).unwrap();
    let shown = source.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(&shown, src, &diags))
    });
    let rust = dir.join(format!("{name}.rs"));
    let binary = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    fs::write(&rust, out.rust).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021", rust.to_str().unwrap(), "-o", binary.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(built.status.success(), "rustc failed:\n{}", String::from_utf8_lossy(&built.stderr));
    binary
}

fn wait_bounded(child: &mut Child, what: &str) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not finish within 10 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jet-os-native-{label}-{}-{:?}",
        std::process::id(),
        thread::current().id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn native_os_facts_match_host_and_are_nonempty() {
    let dir = temp_dir("facts");
    let binary = compile(
        &dir,
        "facts",
        r#"
use core.os as os

fn run() {
    print(os.name())
    print(os.family())
    print(os.arch())
    print(os.cpu_count() >= 1)
    print(os.pid() >= 1)
    print(os.hostname().len() > 0)
    print(os.temp_dir().len() > 0)
    print(os.executable().len() > 0)
}
"#,
    );
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "facts child failed: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap().replace("\r\n", "\n");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 8, "unexpected facts output: {stdout:?}");
    assert_eq!(lines[0], std::env::consts::OS);
    assert_eq!(lines[1], std::env::consts::FAMILY);
    assert_eq!(lines[2], std::env::consts::ARCH);
    assert_eq!(&lines[3..], &["true", "true", "true", "true", "true"]);
}

#[cfg(unix)]
#[test]
fn native_interrupt_runs_ordered_handlers_after_first_panics() {
    let dir = temp_dir("interrupt");
    let binary = compile(
        &dir,
        "interrupt",
        r#"
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
"#,
    );
    let mut child = Command::new(binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (lines_tx, lines_rx) = mpsc::channel();
    let child_stdout = child.stdout.take().unwrap();
    thread::spawn(move || {
        for line in BufReader::new(child_stdout).lines() {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    let ready = match lines_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(line) => line.unwrap(),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("interrupt child did not become ready within 10 seconds: {error}");
        }
    };
    assert_eq!(ready, "ready");
    unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let status = wait_bounded(&mut child, "interrupt child");
    assert!(status.success(), "interrupt child failed: {status}");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let second = lines_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second handler produced no output")
        .unwrap();
    assert_eq!(second, "second");
    assert!(lines_rx.try_recv().is_err(), "unexpected handler output");
    assert!(
        stderr.contains("panic: first handler failed"),
        "first handler panic lost its interrupt boundary diagnostic: {stderr:?}"
    );
}
