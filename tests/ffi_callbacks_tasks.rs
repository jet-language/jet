//! Card #1121: the unified C boundary must compose with callbacks and tasks.
//!
//! The foreign function starts work on a C worker thread and waits for its
//! callback before returning. The Jet call itself runs in a Jet task, so this
//! exercises both directions of the production boundary in one native proof.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_ok(command: &mut Command, label: &str) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("could not start {label}: {error}"));
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn native_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jet_ffi_callbacks_tasks_{name}_{}",
        std::process::id()
    ))
}

fn write_c_library(root: &Path) {
    fs::write(
        root.join("callback.c"),
        r#"
#include <pthread.h>
#include <stdint.h>

typedef int32_t (*callback_t)(int32_t);
typedef struct { callback_t callback; int32_t input; int32_t output; } Job;

static void *run_job(void *raw) {
    Job *job = (Job *)raw;
    job->output = job->callback(job->input);
    return 0;
}

int32_t foreign_callback(callback_t callback, int32_t input) {
    Job job = { callback, input, 0 };
    pthread_t thread;
    if (pthread_create(&thread, 0, run_job, &job) != 0) return -1;
    if (pthread_join(thread, 0) != 0) return -2;
    return job.output;
}

typedef struct { int32_t input; int32_t output; } AsyncJob;

static void *run_async_job(void *raw) {
    AsyncJob *job = (AsyncJob *)raw;
    job->output = job->input + 1;
    return 0;
}

int32_t foreign_async(int32_t input) {
    AsyncJob job = { input, 0 };
    pthread_t thread;
    if (pthread_create(&thread, 0, run_async_job, &job) != 0) return -1;
    if (pthread_join(thread, 0) != 0) return -2;
    return job.output;
}
"#,
    )
    .unwrap();

    let cc = ["cc", "gcc", "clang"]
        .iter()
        .find(|tool| Command::new(tool).arg("--version").output().is_ok())
        .copied()
        .expect("a C compiler is required for the callback/task production proof");
    let mut compile = Command::new(cc);
    compile
        .args(["-pthread", "-c"])
        .arg(root.join("callback.c"))
        .arg("-o")
        .arg(root.join("callback.o"));
    run_ok(&mut compile, "C callback worker compilation");

    let mut archive = Command::new("ar");
    archive
        .args(["rcs"])
        .arg(root.join("libcallback.a"))
        .arg(root.join("callback.o"));
    run_ok(&mut archive, "C callback archive creation");
}

#[test]
fn foreign_callback_reenters_jet_and_completes_on_a_jet_task() {
    let root = native_root("success");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    write_c_library(&root);
    fs::write(
        root.join("package.jet"),
        format!(
            "name: \"ffi-callback-task\"\nversion: \"0.1.0\"\ndeps: {{ callback: c@\"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();

    let source = r#"use c.callback as c

fn increment(value: I32) I32 -[]> {
    return value + 1
}

#Extern module c.callback {
    fn foreign_callback(callback: fn(I32) I32 -[]>, value: I32) I32 = "foreign_callback"
    fn foreign_async(value: I32) I32 = "foreign_async"
}

fn foreign_work() I32 -> {
    return c.foreign_async(41)
}

fn run() {
    print(c.foreign_callback(increment, 41))
    print(c.foreign_callback((value) -> value + 1, 41))
    task.group ffi_workers {
        result :: task foreign_work()
        print(result.join() ?? panic("foreign task failed"))
    }
}
"#;
    let jet_path = root.join("main.jet");
    fs::write(&jet_path, source).unwrap();
    let output =
        jet::compile_with_path(source, jet_path.to_str().unwrap()).unwrap_or_else(|diags| {
            panic!(
                "callback/task source was rejected:\n{}",
                jet::render_diagnostics(jet_path.to_str().unwrap(), source, &diags)
            )
        });
    let callback = output
        .rust
        .find("extern \"C\" fn __jet_increment")
        .expect("named callback must use the stable C trampoline");
    let callback_end = callback
        + output.rust[callback..]
            .find("\n}\n")
            .expect("named callback must have a complete generated body");
    assert!(output.rust[callback..callback_end].contains("jet_ffi_callback_boundary"));
    let inline_callback = output
        .rust
        .find("extern \"C\" fn __jet_c_callback_")
        .expect("inline callback must use the stable C trampoline");
    let inline_callback_end = inline_callback
        + output.rust[inline_callback..]
            .find("\n}\n")
            .expect("inline callback must have a complete generated body");
    assert!(output.rust[inline_callback..inline_callback_end].contains("jet_ffi_callback_boundary"));

    let rust_path = root.join("main.rs");
    let binary = root.join("main");
    fs::write(&rust_path, output.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc
        .args(["--edition", "2021"])
        .arg(&rust_path)
        .arg("-o")
        .arg(&binary)
        .arg("-L")
        .arg(format!("native={}", root.display()))
        .args(["-l", "static=callback", "-l", "pthread"]);
    run_ok(&mut rustc, "generated callback/task Rust link");

    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "generated callback/task program failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n42\n42\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn foreign_task_boundary_rejects_non_sendable_captured_state() {
    let source = r#"use c.callback as c

fn increment(value: I32) I32 -[]> {
    return value + 1
}

#Extern module c.callback {
    fn foreign_callback(callback: fn(I32) I32 -[]>, value: I32) I32 = "foreign_callback"
    fn foreign_async(value: I32) I32 = "foreign_async"
}

fn foreign_work() I32 -> {
    return c.foreign_async(41)
}

fn launch(^cell: Cell<Int>) {
    task.group ffi_workers {
        result :: task {
            _ :: foreign_work()
            _ :: cell
        }
        result.join() ?? panic("foreign task failed")
    }
}

fn run() {}
"#;
    let path = native_root("reject").join("main.jet");
    let root = path.parent().unwrap();
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root).unwrap();
    fs::write(&path, source).unwrap();
    let diagnostics = jet::compile_with_path(source, path.to_str().unwrap())
        .expect_err("a Cell captured across the foreign task boundary must be rejected");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1102"),
        "missing task-boundary rejection: {diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E3203"),
        "the valid callback ABI must not be blamed for the captured state: {diagnostics:?}"
    );
    let _ = fs::remove_dir_all(root);
}
