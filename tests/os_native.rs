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
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rust = dir.join(format!("{name}.rs"));
    let binary = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    fs::write(&rust, out.rust).unwrap();
    let built = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rust.to_str().unwrap(),
            "-o",
            binary.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "rustc failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
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

#[cfg(unix)]
fn termios_target_supported() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
    ))
}

#[cfg(unix)]
fn parse_probe_output(output: &[u8]) -> (String, std::collections::BTreeMap<String, usize>) {
    let text = String::from_utf8(output.to_vec()).expect("termios probe emitted non-UTF-8");
    let mut target = None;
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .unwrap_or_else(|| panic!("malformed termios probe row: {line:?}"));
        if key == "target" {
            target = Some(value.to_string());
        } else {
            values.insert(
                key.to_string(),
                value
                    .parse::<usize>()
                    .unwrap_or_else(|_| panic!("non-numeric termios probe row: {line:?}")),
            );
        }
    }
    (
        target.unwrap_or_else(|| panic!("termios probe did not report its target: {text:?}")),
        values,
    )
}

#[cfg(unix)]
fn run_c_header_layout_probe(dir: &Path) -> (String, std::collections::BTreeMap<String, usize>) {
    let source = dir.join("termios_header_probe.c");
    let binary = dir.join("termios_header_probe");
    fs::write(
        &source,
        r#"
#include <stddef.h>
#include <stdio.h>
#include <termios.h>

#define JET_NONE ((size_t)-1)

#if defined(__ANDROID__)
#define JET_TARGET "android"
#elif defined(__ENVIRONMENT_IPHONE_OS_VERSION_MIN_REQUIRED__)
#define JET_TARGET "ios"
#elif defined(__APPLE__)
#define JET_TARGET "macos"
#elif defined(__FreeBSD__)
#define JET_TARGET "freebsd"
#elif defined(__OpenBSD__)
#define JET_TARGET "openbsd"
#elif defined(__NetBSD__)
#define JET_TARGET "netbsd"
#elif defined(__linux__)
#define JET_TARGET "linux"
#else
#define JET_TARGET "unsupported"
#endif

int main(void) {
    printf("target=%s\n", JET_TARGET);
    printf("size=%zu\n", sizeof(struct termios));
    printf("align=%zu\n", _Alignof(struct termios));
    printf("flag_size=%zu\n", sizeof(((struct termios *)0)->c_lflag));
    printf("c_iflag=%zu\n", offsetof(struct termios, c_iflag));
    printf("c_oflag=%zu\n", offsetof(struct termios, c_oflag));
    printf("c_cflag=%zu\n", offsetof(struct termios, c_cflag));
    printf("c_lflag=%zu\n", offsetof(struct termios, c_lflag));
#if defined(__linux__) || defined(__ANDROID__)
    printf("c_line=%zu\n", offsetof(struct termios, c_line));
    printf("line_size=%zu\n", sizeof(((struct termios *)0)->c_line));
#else
    printf("c_line=%zu\n", JET_NONE);
    printf("line_size=%zu\n", JET_NONE);
#endif
    printf("c_cc=%zu\n", offsetof(struct termios, c_cc));
    printf("cc_size=%zu\n", sizeof(((struct termios *)0)->c_cc));
#if defined(__ANDROID__)
    printf("c_ispeed=%zu\n", JET_NONE);
    printf("c_ospeed=%zu\n", JET_NONE);
    printf("speed_size=%zu\n", JET_NONE);
#else
    printf("c_ispeed=%zu\n", offsetof(struct termios, c_ispeed));
    printf("c_ospeed=%zu\n", offsetof(struct termios, c_ospeed));
    printf("speed_size=%zu\n", sizeof(((struct termios *)0)->c_ispeed));
#endif
    printf("nccs=%zu\n", (size_t)NCCS);
    printf("vmin=%zu\n", (size_t)VMIN);
    printf("vtime=%zu\n", (size_t)VTIME);
    printf("tcsanow=%zu\n", (size_t)TCSANOW);
    printf("echo=%zu\n", (size_t)ECHO);
    printf("icanon=%zu\n", (size_t)ICANON);
    return 0;
}
"#,
    )
    .unwrap();

    let mut compilers = Vec::new();
    if let Some(compiler) = std::env::var_os("CC") {
        compilers.push(compiler);
    } else {
        compilers.extend(["cc", "clang", "gcc"].into_iter().map(std::ffi::OsString::from));
    }
    let mut errors = Vec::new();
    for compiler in compilers {
        let built = Command::new(&compiler)
            .args(["-std=c11", source.to_str().unwrap(), "-o", binary.to_str().unwrap()])
            .output()
            .unwrap_or_else(|error| panic!("failed to invoke C compiler {compiler:?}: {error}"));
        if built.status.success() {
            let output = Command::new(&binary)
                .output()
                .expect("termios C-header probe must execute");
            assert!(
                output.status.success(),
                "termios C-header probe failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return parse_probe_output(&output.stdout);
        }
        errors.push(format!(
            "{compiler:?}: {}",
            String::from_utf8_lossy(&built.stderr)
        ));
    }
    panic!("no C compiler accepted the termios header probe: {errors:?}");
}

#[cfg(unix)]
fn run_rust_layout_probe(dir: &Path) -> std::collections::BTreeMap<String, usize> {
    let term = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/jet-codegen/src/Prelude/Term.rs");
    let include_path = format!("{:?}", term.to_string_lossy());
    let source = dir.join("termios_rust_probe.rs");
    let binary = dir.join("termios_rust_probe");
    fs::write(
        &source,
        format!(
            r#"
include!({include_path});

fn main() {{
    println!("target=rust");
    let values = jet_term_layout_fingerprint();
    for (key, value) in [
        ("size", values[0]),
        ("align", values[1]),
        ("flag_size", values[2]),
        ("c_iflag", values[3]),
        ("c_oflag", values[4]),
        ("c_cflag", values[5]),
        ("c_lflag", values[6]),
        ("c_line", values[7]),
        ("c_cc", values[8]),
        ("c_ispeed", values[9]),
        ("c_ospeed", values[10]),
        ("nccs", values[11]),
        ("vmin", values[12]),
        ("vtime", values[13]),
        ("tcsanow", values[14]),
        ("echo", values[15]),
        ("icanon", values[16]),
        ("line_size", values[17]),
        ("speed_size", values[18]),
        ("cc_size", values[19]),
    ] {{
        println!("{{key}}={{value}}");
    }}
}}
"#
        ),
    )
    .unwrap();
    let built = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            source.to_str().unwrap(),
            "-o",
            binary.to_str().unwrap(),
        ])
        .output()
        .expect("rustc must build the Term.rs layout probe");
    assert!(
        built.status.success(),
        "rustc rejected the Term.rs layout probe:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&binary)
        .output()
        .expect("Term.rs layout probe must execute");
    assert!(
        output.status.success(),
        "Term.rs layout probe failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, values) = parse_probe_output(&output.stdout);
    values
}

#[cfg(unix)]
#[test]
fn termios_ffi_layout_matches_target_header_and_shared_by_native_tiers() {
    if !termios_target_supported() {
        eprintln!(
            "not applicable: Term.rs rejects target_os={}",
            std::env::consts::OS
        );
        return;
    }

    let dir = temp_dir("termios-layout");
    let (c_target, c_values) = run_c_header_layout_probe(&dir);
    assert_eq!(
        c_target,
        std::env::consts::OS,
        "the C probe and Rust test must run for the same target"
    );
    assert_ne!(c_target, "unsupported", "C header probe target is unsupported");
    let rust_values = run_rust_layout_probe(&dir);
    let keys = [
        "size",
        "align",
        "flag_size",
        "c_iflag",
        "c_oflag",
        "c_cflag",
        "c_lflag",
        "c_line",
        "c_cc",
        "c_ispeed",
        "c_ospeed",
        "nccs",
        "vmin",
        "vtime",
        "tcsanow",
        "echo",
        "icanon",
        "line_size",
        "speed_size",
        "cc_size",
    ];
    assert_eq!(c_values.len(), keys.len(), "C probe metric set drifted");
    assert_eq!(rust_values.len(), keys.len(), "Rust probe metric set drifted");
    for key in keys {
        assert_eq!(
            c_values.get(key),
            rust_values.get(key),
            "Term.rs disagrees with <termios.h> for {key}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
unsafe extern "C" {
    fn openpty(
        master: *mut i32,
        slave: *mut i32,
        name: *mut i8,
        termp: *const u8,
        winp: *const u8,
    ) -> i32;
    fn tcgetattr(fd: i32, termios: *mut u8) -> i32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}

#[cfg(unix)]
enum PtyRead {
    Bytes(Vec<u8>),
    End,
}

#[cfg(unix)]
fn open_test_pty() -> (std::fs::File, std::fs::File) {
    use std::os::fd::FromRawFd;
    let (mut master_fd, mut slave_fd) = (-1, -1);
    assert_eq!(
        unsafe {
            openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0,
        "openpty must provide a real PTY"
    );
    assert!(master_fd >= 0 && slave_fd >= 0, "openpty returned invalid fds");
    // openpty may leave descriptors inheritable. Close-on-exec keeps the
    // reader's EIO/EOF boundary tied to the child, not an accidental clone.
    for fd in [master_fd, slave_fd] {
        let flags = unsafe { fcntl(fd, 1) };
        assert!(flags >= 0, "fcntl(F_GETFD) failed for PTY fd {fd}");
        assert_eq!(unsafe { fcntl(fd, 2, flags | 1) }, 0, "fcntl(FD_CLOEXEC) failed");
    }
    (
        unsafe { std::fs::File::from_raw_fd(master_fd) },
        unsafe { std::fs::File::from_raw_fd(slave_fd) },
    )
}

#[cfg(unix)]
fn read_pty_termios(
    slave: &std::fs::File,
    layout: &std::collections::BTreeMap<String, usize>,
) -> Vec<u8> {
    use std::os::fd::AsRawFd;
    let size = *layout.get("size").expect("termios size metric");
    assert!(
        size > 0 && size <= 256,
        "termios size outside probe buffer: {size}"
    );
    let mut words = vec![0u64; size.div_ceil(std::mem::size_of::<u64>())];
    let rc = unsafe { tcgetattr(slave.as_raw_fd(), words.as_mut_ptr().cast::<u8>()) };
    assert_eq!(rc, 0, "tcgetattr failed for PTY inspection");
    unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), size) }.to_vec()
}

#[cfg(unix)]
fn termios_lflag(
    bytes: &[u8],
    layout: &std::collections::BTreeMap<String, usize>,
) -> u64 {
    let offset = *layout.get("c_lflag").expect("termios c_lflag metric");
    let width = *layout.get("flag_size").expect("termios flag size metric");
    assert!(width > 0 && width <= 8 && offset + width <= bytes.len());
    let mut raw = [0u8; 8];
    raw[..width].copy_from_slice(&bytes[offset..offset + width]);
    u64::from_ne_bytes(raw)
}

#[cfg(unix)]
fn pty_contains(bytes: &[u8], marker: &[u8]) -> bool {
    !marker.is_empty() && bytes.windows(marker.len()).any(|window| window == marker)
}

#[cfg(unix)]
fn wait_for_pty_marker(
    child: &mut Child,
    reader: &std::sync::mpsc::Receiver<PtyRead>,
    transcript: &mut Vec<u8>,
    marker: &[u8],
    what: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    while !pty_contains(transcript, marker) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "{what} did not arrive within {:?}; child={:?}; transcript={:?}",
            timeout,
            child.try_wait().ok().flatten(),
            String::from_utf8_lossy(transcript)
        );
        match reader.recv_timeout(remaining) {
            Ok(PtyRead::Bytes(bytes)) => transcript.extend(bytes),
            Ok(PtyRead::End) => panic!(
                "PTY reader reached EOF before {what}; child={:?}; transcript={:?}",
                child.try_wait().ok().flatten(),
                String::from_utf8_lossy(transcript)
            ),
            Err(error) => panic!(
                "PTY reader stopped before {what}: {error}; child={:?}; transcript={:?}",
                child.try_wait().ok().flatten(),
                String::from_utf8_lossy(transcript)
            ),
        }
    }
}

#[cfg(unix)]
fn wait_for_pty_mode(
    slave: &std::fs::File,
    layout: &std::collections::BTreeMap<String, usize>,
    predicate: impl Fn(u64) -> bool,
    what: &str,
) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let bytes = read_pty_termios(slave, layout);
        if predicate(termios_lflag(&bytes, layout)) {
            return bytes;
        }
        assert!(
            Instant::now() < deadline,
            "{what} did not become observable within 10 seconds"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
const TERM_PTY_PACKAGE: &str = r#"
name: "termios_pty"
version: "0.1.0"
authority: { holds: { allow: [IO, Mem.Alloc] } }
"#;

#[cfg(unix)]
const TERM_PTY_SUCCESS_SOURCE: &str = r#"
use core.term as term

fn run() {
    print("ready")
    #Live {
        _pressed :: term.read_key()
        print("raw-complete")
    }
    secret :: term.input_secret("secret> ")
    print("secret-ok:{secret}")
}
"#;

#[cfg(unix)]
const TERM_PTY_FAILURE_SOURCE: &str = r#"
use core.term as term

fn run() {
    print("ready")
    #Live {
        _pressed :: term.read_key()
        panic("injected terminal failure")
    }
}
"#;

#[cfg(unix)]
fn run_termios_pty_case(
    mode: &str,
    source: &str,
    failure: bool,
    layout: &std::collections::BTreeMap<String, usize>,
) {
    use std::io::{Read as _, Write as _};
    use std::process::Stdio;
    let dir = temp_dir(&format!(
        "termios-pty-{mode}-{}",
        if failure { "fail" } else { "ok" }
    ));
    let entry = dir.join("main.jet");
    fs::write(&entry, source).unwrap();
    fs::write(dir.join("package.jet"), TERM_PTY_PACKAGE).unwrap();

    let (mut master, slave) = open_test_pty();
    let inspect = slave.try_clone().expect("PTY inspection clone");
    let baseline = read_pty_termios(&inspect, layout);
    let baseline_lflag = termios_lflag(&baseline, layout);
    let echo = *layout.get("echo").expect("termios ECHO metric") as u64;
    let icanon = *layout.get("icanon").expect("termios ICANON metric") as u64;
    assert_ne!(baseline_lflag & echo, 0, "openpty baseline must have ECHO set");
    assert_ne!(
        baseline_lflag & icanon,
        0,
        "openpty baseline must have ICANON set"
    );

    let mut args = vec!["run", "--quiet"];
    match mode {
        "release" => args.push("--release"),
        "default" => args.push("--trace-tiers"),
        "interpret" => args.push("--interpret"),
        other => panic!("unknown PTY mode {other}"),
    }
    args.push(entry.to_str().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("JET_STORE_DIR", dir.join("cache"))
        .env("JET_RUN_CACHE_DIR", dir.join("run-cache"))
        .env("JETPACK_ROOT", dir.join("jetpack"))
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn()
        .unwrap_or_else(|error| panic!("{mode} PTY child must start: {error}"));

    let (tx, rx) = mpsc::channel();
    let mut reader = master.try_clone().expect("PTY reader clone");
    let reader_thread = thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    let _ = tx.send(PtyRead::End);
                    break;
                }
                Ok(count) => {
                    if tx.send(PtyRead::Bytes(chunk[..count].to_vec())).is_err() {
                        break;
                    }
                }
                Err(error) if error.raw_os_error() == Some(5) => {
                    let _ = tx.send(PtyRead::End);
                    break;
                }
                Err(error) => panic!("PTY reader failed: {error}"),
            }
        }
    });
    let mut transcript = Vec::new();
    wait_for_pty_marker(
        &mut child,
        &rx,
        &mut transcript,
        b"ready",
        "PTY ready marker",
        Duration::from_secs(60),
    );
    let _raw = wait_for_pty_mode(
        &inspect,
        layout,
        |flags| flags & echo == 0 && flags & icanon == 0,
        "raw mode",
    );
    master.write_all(b"x").expect("write PTY raw key");

    if failure {
        let status = wait_bounded(&mut child, &format!("{mode} failure PTY child"));
        let restored = read_pty_termios(&inspect, layout);
        assert_eq!(
            restored, baseline,
            "{mode} injected failure did not restore the complete termios state"
        );
        drop(inspect);
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(PtyRead::Bytes(bytes)) => transcript.extend(bytes),
                Ok(PtyRead::End) | Err(_) => break,
            }
        }
        reader_thread.join().expect("PTY reader thread");
        assert!(
            !status.success(),
            "{mode} injected failure unexpectedly succeeded"
        );
        let output = String::from_utf8_lossy(&transcript);
        assert!(
            output.contains("injected terminal failure"),
            "{mode} failure did not reach the injected runtime stop: {output:?}"
        );
        let _ = fs::remove_dir_all(&dir);
        return;
    }

    wait_for_pty_marker(
        &mut child,
        &rx,
        &mut transcript,
        b"raw-complete",
        "raw completion marker",
        Duration::from_secs(10),
    );
    let _secret = wait_for_pty_mode(
        &inspect,
        layout,
        |flags| flags & echo == 0 && flags & icanon == baseline_lflag & icanon,
        "secret echo-off mode",
    );
    master.write_all(b"answer\n").expect("write PTY secret");
    wait_for_pty_marker(
        &mut child,
        &rx,
        &mut transcript,
        b"secret-ok:answer",
        "secret success marker",
        Duration::from_secs(10),
    );
    let status = wait_bounded(&mut child, &format!("{mode} success PTY child"));
    let restored = read_pty_termios(&inspect, layout);
    assert_eq!(
        restored, baseline,
        "{mode} successful terminal use did not restore the complete termios state"
    );
    drop(inspect);
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(PtyRead::Bytes(bytes)) => transcript.extend(bytes),
            Ok(PtyRead::End) | Err(_) => break,
        }
    }
    reader_thread.join().expect("PTY reader thread");
    assert!(
        status.success(),
        "{mode} successful PTY child failed: {status}"
    );
    if mode == "default" {
        let output = String::from_utf8_lossy(&transcript);
        assert!(
            output.contains("tier1 native"),
            "default run did not execute resident JIT: {output:?}"
        );
        assert!(
            !output.contains("tier0 interp"),
            "default run deopted to interpreter: {output:?}"
        );
    }
    let output = String::from_utf8_lossy(&transcript);
    assert!(
        output.contains("secret-ok:answer"),
        "{mode} PTY output missing success: {output:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn termios_pty_mode_and_restore_are_hostile_proven_across_native_tiers() {
    if !termios_target_supported() {
        eprintln!(
            "not applicable: Term.rs rejects target_os={}",
            std::env::consts::OS
        );
        return;
    }
    let dir = temp_dir("termios-pty-layout");
    let (_, layout) = run_c_header_layout_probe(&dir);
    let _ = fs::remove_dir_all(&dir);
    for mode in ["release", "default", "interpret"] {
        run_termios_pty_case(mode, TERM_PTY_SUCCESS_SOURCE, false, &layout);
        run_termios_pty_case(mode, TERM_PTY_FAILURE_SOURCE, true, &layout);
    }
}

#[test]
fn process_pty_reuses_the_canonical_termios_abi() {
    let term = include_str!("../crates/jet-codegen/src/Prelude/Term.rs");
    let pty = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/ProcessPty.rs");
    assert_eq!(
        term.matches("struct Termios").count(),
        1,
        "the canonical terminal Prelude must own the only Termios definition"
    );
    assert!(
        !pty.contains("Termios")
            && !pty.contains("tcgetattr")
            && !pty.contains("tcsetattr")
            && !pty.contains("cfmakeraw"),
        "the PTY backend must not carry a second termios ABI"
    );
    assert!(
        term.contains("pub(super) fn configure_fd(fd: i32, raw: bool)")
            && term.contains("fn cfmakeraw(termios: *mut u8)")
            && pty.contains("super::super::jet_term_configure_fd(slave.as_raw_fd(), raw)?;"),
        "PTY configuration must use the canonical Termios seam"
    );
}

#[test]
fn native_os_facts_match_host_and_are_nonempty() {
    let dir = temp_dir("facts");
    let binary = compile(
        &dir,
        "facts",
        r#"
use core.sys as os

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
    assert!(
        output.status.success(),
        "facts child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n");
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
use core.sys as os
use core.process as process

fn run() {
    os.on_interrupt(() -> { panic("first handler failed") })
    os.on_interrupt(() -> {
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
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
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
