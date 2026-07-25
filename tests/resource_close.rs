//! D-SHAPE-RESOURCE2=A: the sole deferred action is `defer close(^resource)`.

use std::fs;
use std::process::{Command, Output};

mod common;

const SIMPLE: &str = r#"
struct Resource { name: String }

impl Resource.Close {
    fn close(^self) {
        print("close {self.name}")
    }
}

fn run() {
    resource := Resource.{ name: "dev" }
    defer close(^resource)
    print("body")
}
"#;

fn compile(src: &str) -> jet::CompileOutput {
    let root = common::unique_tmp("jet_resource_close_compile");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, src).unwrap();
    jet::compile_with_path(src, path.to_str().unwrap()).unwrap_or_else(|diags| {
        panic!(
            "{}",
            jet::render_diagnostics(path.to_str().unwrap(), src, &diags)
        )
    })
}

fn codes(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

fn compile_and_run(src: &str, tag: &str) -> Output {
    assert!(common::have_rustc(), "resource-close runtime proof needs rustc");
    let compiled = compile(src);
    let root = common::unique_tmp(tag);
    fs::create_dir_all(&root).unwrap();
    let rs = root.join("main.rs");
    let bin = root.join("main");
    fs::write(&rs, compiled.rust).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "rustc rejected deferred-close output:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    Command::new(bin).output().unwrap()
}

#[test]
fn defer_close_has_explicit_tir_codegen_and_stable_formatting() {
    let out = compile(SIMPLE);
    assert!(out.rust.contains("JetDeferredClose"), "{}", out.rust);
    assert!(
        out.rust.contains("user_Close::close(user___jet_resource_resource_")
            && out.rust.contains(".take())"),
        "{}",
        out.rust
    );

    let once = jet::format_source(SIMPLE).expect("deferred close should format");
    assert!(once.contains("defer close(^resource)"), "{once}");
    let twice = jet::format_source(&once).expect("formatted deferred close should parse");
    assert_eq!(once, twice);
}

#[test]
fn defer_rejects_general_actions_blocks_and_values_without_close_capability() {
    let statement = codes("fn run() { defer print(\"not close\") }");
    assert!(statement.contains(&"E0003".into()), "{statement:?}");

    let block = codes("fn run() { defer { print(\"not close\") } }");
    assert!(block.contains(&"E0003".into()), "{block:?}");

    let non_resource = codes("fn run() { value := 1; defer close(^value) }");
    assert!(non_resource.contains(&"E0905".into()), "{non_resource:?}");
}

#[test]
fn free_close_function_cannot_shadow_the_nominal_protocol() {
    let src = r#"
struct NotClose { value: Int }
fn close(value: ^NotClose) { print(value.value) }
fn run() {
    value := NotClose.{ value: 1 }
    defer close(^value)
}
"#;
    let got = codes(src);
    assert!(got.contains(&"E0905".into()), "{got:?}");
}

#[test]
fn nominal_close_is_the_automatic_scope_end_safety_net() {
    let src = r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("auto {self.name}") }
}

fn scoped() {
    resource := Resource.{ name: "scope" }
    print("body {resource.name}")
}
fn run() { scoped() }
"#;
    let ran = compile_and_run(src, "jet_resource_close_automatic");
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(ran.stdout, b"body scope\nauto scope\n");
}

#[test]
fn automatic_cleanup_runs_before_source_panic_exits_the_process() {
    let src = r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("auto {self.name}") }
}
fn run() {
    resource := Resource.{ name: "panic" }
    print("body")
    panic("stop")
}
"#;
    let ran = compile_and_run(src, "jet_resource_close_automatic_panic");
    assert!(!ran.status.success());
    assert_eq!(ran.stdout, b"body\nauto panic\n");
}

#[test]
fn consuming_parameters_and_returns_transfer_the_cleanup_guard() {
    let src = r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("close {self.name}") }
}
impl Resource {
    fn handoff(^self) -> Resource { return self }
}
fn relay(^resource: Resource) -> Resource { return resource }
fn consume(^resource: Resource) { print("consume {resource.name}") }
fn run() {
    first := Resource.{ name: "transfer" }
    second := relay(^first)
    third := second.handoff()
    consume(^third)
}
"#;
    let compiled = compile(src);
    assert!(
        compiled.rust.contains(
            ": JetResource<user_Resource> = JetResource::new(user_resource);"
        ),
        "{}",
        compiled.rust
    );
    assert!(
        compiled
            .rust
            .contains("return user___jet_resource_param_resource.take();"),
        "{}",
        compiled.rust
    );
    let out = compile_and_run(src, "jet_resource_close_transfer");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "consume transfer\nclose transfer\n");
}

#[test]
fn scheduled_transfer_reuses_move_checks_for_every_second_use() {
    let use_after = codes(&format!(
        "{SIMPLE}\nfn bad() {{ resource := Resource.{{ name: \"x\" }}; defer close(^resource); print(resource.name) }}"
    ));
    assert!(use_after.contains(&"E0121".into()), "{use_after:?}");

    let double_defer = codes(&format!(
        "{SIMPLE}\nfn bad() {{ resource := Resource.{{ name: \"x\" }}; defer close(^resource); defer close(^resource) }}"
    ));
    assert!(double_defer.contains(&"E0121".into()), "{double_defer:?}");

    let double_close = codes(&format!(
        "{SIMPLE}\nfn bad() {{ resource := Resource.{{ name: \"x\" }}; close(^resource); close(^resource) }}"
    ));
    assert!(double_close.contains(&"E0121".into()), "{double_close:?}");

    let copied = codes(&format!(
        "{SIMPLE}\nfn bad() {{ resource := Resource.{{ name: \"x\" }}; copied := ~resource; print(copied.name) }}"
    ));
    assert!(copied.contains(&"E0211".into()), "{copied:?}");
}

#[test]
fn cleanup_runs_once_in_lifo_order_on_all_scope_exits() {
    let src = r#"
struct Resource { name: String }

impl Resource.Close {
    fn close(^self) {
        print("close {self.name}")
    }
}

fn fail() -> Int ? String {
    return Err("stop")
}

fn returned() {
    resource := Resource.{ name: "return" }
    defer close(^resource)
    print("return body")
    return
}

fn questioned() -> Int ? String {
    resource := Resource.{ name: "question" }
    defer close(^resource)
    value := fail()?
    return Ok(value)
}

fn looped() {
    loop n; [0, 1] {
        resource := Resource.{ name: if n == 0 { "continue" } else { "break" } }
        defer close(^resource)
        if n == 0 { next }
        break
    }
}

fn run() {
    first := Resource.{ name: "a" }
    defer close(^first)
    second := Resource.{ name: "b" }
    defer close(^second)
    print("body")
    returned()
    questioned().drop("this path intentionally proves `?` cleanup")
    looped()
    unwind := Resource.{ name: "unwind" }
    defer close(^unwind)
    panic("boom")
}
"#;
    let ran = compile_and_run(src, "jet_resource_close_exits");
    assert!(!ran.status.success(), "panic path must unwind");
    assert_eq!(
        String::from_utf8(ran.stdout).unwrap(),
        "body\nreturn body\nclose return\nclose question\nclose continue\nclose break\nclose unwind\nclose b\nclose a\n"
    );
}

#[test]
fn failed_require_drains_resources_only_on_the_failure_path() {
    let src = r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("close {self.name}") }
}
fn run() {
    automatic := Resource.{ name: "automatic" }
    deferred := Resource.{ name: "deferred" }
    defer close(^deferred)
    require(true)
    print("before failure")
    require(false, "stop")
    print(automatic.name)
}
"#;
    let ran = compile_and_run(src, "jet_resource_close_require");
    assert!(!ran.status.success());
    assert_eq!(
        ran.stdout,
        b"before failure\nclose deferred\nclose automatic\n"
    );
}

#[test]
fn ordinary_scope_drop_and_reasoned_drop_remain_separate() {
    let src = r#"
struct Value { number: Int }
fn maybe() -> Int ? String { return Err("unused") }
fn run() {
    value := Value.{ number: 1 }
    print(value.number)
    maybe().drop("best effort remains an explicit value discard")
}
"#;
    let out = compile(src);
    assert!(out.rust.contains("user_maybe();"), "{}", out.rust);
    assert_eq!(
        out.rust.matches("JetDeferredClose::new").count(),
        0,
        "ordinary scope cleanup must not become explicit deferred close"
    );
    let ran = compile_and_run(src, "jet_resource_close_ordinary_drop");
    assert!(ran.status.success());
    assert_eq!(ran.stdout, b"1\n");
}

#[test]
fn allocator_handles_participate_in_nominal_close() {
    let src = r#"
use core.mem as mem

fn run() {
    arena := mem.Arena.new()
    close(^arena)
    bump := mem.Bump.new(capacity: 64)
    defer close(^bump)
    pool := mem.Pool.new(slots: 2)
    close(^pool)
    print("closed")
}
"#;
    let ran = compile_and_run(src, "jet_resource_close_allocators");
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(ran.stdout, b"closed\n");
}

#[test]
fn built_in_resource_impls_follow_referenced_core_families() {
    let src = r#"
use core.files as files
use core.net as net
use core.db as db
use core.mem as mem
fn run() {
    conn := db.open_memory()
    close(^conn)
}
"#;
    let rust = compile(src).rust;
    assert_eq!(
        rust.matches("impl user_Close for JetDbConnection").count(),
        1,
        "the referenced database family needs its Close implementation"
    );
    for ty in [
        "JetFileReader",
        "JetFileWriter",
        "jet_std::FileLock",
        "JetTcpStream",
        "JetUnixStream",
        "JetTlsStream",
        "jet_mem::JetArena",
        "jet_mem::JetBump",
        "jet_mem::JetPool",
        "jet_mem::JetFixed",
    ] {
        assert_eq!(
            rust.matches(&format!("impl user_Close for {ty}")).count(),
            0,
            "an import-only resource family must not emit an unused Close implementation"
        );
    }
}

#[test]
fn default_dev_deopts_or_interpreter_gaps_on_deferred_close() {
    use jet::Interpreter::RunOutcome;
    use jet::JitBackend::JitBackend;

    let root = common::unique_tmp("jet_resource_close_dev");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, SIMPLE).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");

    if jet_jit::cranelift_host_supported() {
        let gap = jet_jit::try_compile_bundle(&bundle).expect_err("JIT must name its cleanup gap");
        assert!(
            gap.contains("automatic resource cleanup") || gap.contains("jit "),
            "{gap}"
        );
    }

    let native = compile_and_run(SIMPLE, "jet_resource_close_native_parity");
    assert!(native.status.success());
    assert_eq!(native.stdout, b"body\nclose dev\n");

    let mut dev = jet_jit::CraneliftBackend::new();
    jet_jit::reset_jit_trace_for_test();
    match dev.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => {
            assert!(
                jet_jit::deopt_invoked_for_test(),
                "tiered JIT must deopt on cleanup gap"
            );
            assert_eq!(stdout, "body\nclose dev\n");
        }
        RunOutcome::Problems(diags) => {
            assert!(
                !jet_jit::is_e2211(&diags),
                "E2211 retired: {diags:?}"
            );
        }
    }
}
