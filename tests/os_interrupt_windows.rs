#![cfg(windows)]

mod common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

unsafe extern "system" {
    fn FreeConsole() -> i32;
    fn AllocConsole() -> i32;
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, process_group: u32) -> i32;
}

fn compile_interrupt_program(dir: &PathBuf) -> PathBuf {
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
    let source = dir.join("interrupt.jet");
    fs::write(&source, src).unwrap();
    let shown = source.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(&shown, src, &diags))
    });
    let rust = dir.join("interrupt.rs");
    let binary = dir.join("interrupt.exe");
    fs::write(&rust, out.rust).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021", rust.to_str().unwrap(), "-o", binary.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(built.status.success(), "rustc failed:\n{}", String::from_utf8_lossy(&built.stderr));
    binary
}

#[test]
fn windows_console_ctrl_c_runs_all_handlers_in_order() {
    const CTRL_C_EVENT: u32 = 0;
    let dir = std::env::temp_dir().join(format!("jet-os-interrupt-windows-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();

    // Give this test and its child a private console. Ignore Ctrl-C in the
    // test process; the generated child clears the inherited flag when it
    // installs Jet's real SetConsoleCtrlHandler callback.
    unsafe {
        FreeConsole();
        assert_ne!(AllocConsole(), 0, "could not allocate private test console");
        assert_ne!(SetConsoleCtrlHandler(None, 1), 0, "could not protect test driver");
    }

    let binary = compile_interrupt_program(&dir);
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
            panic!("generated child did not become ready within 10 seconds: {error}");
        }
    };
    assert_eq!(ready, "ready");
    assert_ne!(unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) }, 0);

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("generated child did not handle CTRL_C_EVENT within 10 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "generated child failed: {status}");
    let second = lines_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second handler produced no output")
        .unwrap();
    assert_eq!(second, "second");
    assert!(lines_rx.try_recv().is_err(), "unexpected handler output");
}
