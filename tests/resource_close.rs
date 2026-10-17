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
    resource := Resource{ name: "dev" }
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
    assert!(
        common::have_rustc(),
        "resource-close runtime proof needs rustc"
    );
    let compiled = compile(src);
    run_compiled(compiled.rust, tag)
}

fn compile_and_run_with_scheduler_unwind(src: &str, tag: &str) -> Output {
    assert!(
        common::have_rustc(),
        "resource-close runtime proof needs rustc"
    );
    let compiled = compile(src);
    let entry_init = "fn main() {\n    jet_std_env_init();";
    let rust = compiled.rust.replacen(
        entry_init,
        "fn main() {\n    jet_std_env_init();\n    jet_scheduler_task_panic_enter();",
        1,
    );
    assert_ne!(
        rust, compiled.rust,
        "scheduler unwind probe did not attach to entry init"
    );
    run_compiled(rust, tag)
}

fn run_compiled(rust: String, tag: &str) -> Output {
    let root = common::unique_tmp(tag);
    fs::create_dir_all(&root).unwrap();
    let rs = root.join("main.rs");
    let bin = root.join("main");
    fs::write(&rs, rust).unwrap();
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
    assert!(
        out.rust.contains("as __jet_Close>::close(")
            && out.rust.contains(".take().expect(\"MIR local\")"),
        "deferred close must consume its MIR local through the nominal protocol:\n{}",
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

    let non_resource = codes("fn run() {\n    value := 1\n    defer close(^value)\n}");
    assert!(non_resource.contains(&"E0905".into()), "{non_resource:?}");
}

#[test]
fn free_close_function_cannot_shadow_the_nominal_protocol() {
    let src = r#"
struct NotClose { value: Int }
fn close(value: ^NotClose) { print(value.value) }
fn run() {
    value := NotClose{ value: 1 }
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
    resource := Resource{ name: "scope" }
    print("body {resource.name}")
}
fn run() { scoped() }
"#;
    let ran = compile_and_run(src, "jet_resource_close_automatic");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
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
    resource := Resource{ name: "panic" }
    print("body")
    panic("stop")
}
"#;
    let ran = compile_and_run_with_scheduler_unwind(src, "jet_resource_close_automatic_panic");
    assert!(!ran.status.success());
    assert_eq!(ran.status.code(), Some(70));
    assert_eq!(ran.stdout, b"body\nauto panic\n");
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert_eq!(stderr.matches("Stop [E3001]").count(), 1, "{stderr}");
    assert!(stderr.contains("Stop [E3001]: `panic: stop`\n"), "{stderr}");
}

#[test]
fn typed_runtime_stops_keep_cleanup_and_exit_code_through_native_boundary() {
    let cases = [
        (
            "E3010",
            "arithmetic",
            r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("auto {self.name}") }
}
fn run() {
    resource := Resource{ name: "arithmetic" }
    print("body")
    zero :: 0
    print(10 /% zero)
}
"#,
        ),
        (
            "E3011",
            "todo",
            r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("auto {self.name}") }
}
fn missing() -> Int { #Todo }
fn run() {
    resource := Resource{ name: "todo" }
    print("body")
    print(missing())
}
"#,
        ),
        (
            "E3012",
            "stack",
            r#"
struct Resource { name: String }
impl Resource.Close {
    fn close(^self) { print("auto {self.name}") }
}
fn recurse(n: Int) -> Int {
    return recurse(n + 1)
}
fn run() {
    resource := Resource{ name: "stack" }
    print("body")
    print(recurse(0))
}
"#,
        ),
    ];

    for (code, name, src) in cases {
        let ran = compile_and_run_with_scheduler_unwind(src, &format!("jet_resource_close_{code}"));
        assert_eq!(ran.status.code(), Some(70), "{code}: {ran:?}");
        assert_eq!(ran.stdout, format!("body\nauto {name}\n").as_bytes());
        let stderr = String::from_utf8_lossy(&ran.stderr);
        assert!(
            stderr.contains(&format!("Stop [{code}]")),
            "{code}: {stderr}"
        );
        assert_eq!(
            stderr.matches(&format!("Stop [{code}]")).count(),
            1,
            "{code}: {stderr}"
        );
    }
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
    first := Resource{ name: "transfer" }
    second := relay(^first)
    third := second.handoff()
    consume(^third)
}
"#;
    let compiled = compile(src);
    assert!(
        compiled.rust.contains(".take().expect(\"MIR parameter\")")
            && compiled.rust.contains("as __jet_Close>::close("),
        "consuming parameters must transfer their MIR slot and close the final owner:\n{}",
        compiled.rust
    );
    let out = compile_and_run(src, "jet_resource_close_transfer");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "consume transfer\nclose transfer\n"
    );
}

#[test]
fn scheduled_transfer_reuses_move_checks_for_every_second_use() {
    let use_after = codes(&format!(
        "{SIMPLE}\nfn bad() {{\n    resource := Resource{{ name: \"x\" }}\n    defer close(^resource)\n    print(resource.name)\n}}"
    ));
    assert!(use_after.contains(&"E0121".into()), "{use_after:?}");

    let double_defer = codes(&format!(
        "{SIMPLE}\nfn bad() {{\n    resource := Resource{{ name: \"x\" }}\n    defer close(^resource)\n    defer close(^resource)\n}}"
    ));
    assert!(double_defer.contains(&"E0121".into()), "{double_defer:?}");

    let double_close = codes(&format!(
        "{SIMPLE}\nfn bad() {{\n    resource := Resource{{ name: \"x\" }}\n    close(^resource)\n    close(^resource)\n}}"
    ));
    assert!(double_close.contains(&"E0121".into()), "{double_close:?}");

    let copied = codes(&format!(
        "{SIMPLE}\nfn bad() {{\n    resource := Resource{{ name: \"x\" }}\n    copied := resource.copy()\n    print(copied.name)\n}}"
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

fn fail() -> Int Err! {
    return Err("stop")
}

fn returned() {
    resource := Resource{ name: "return" }
    defer close(^resource)
    print("return body")
    return
}

fn questioned() -> Int Err! {
    resource := Resource{ name: "question" }
    defer close(^resource)
    value := fail()
    return Ok(value)
}

fn looped() {
    loop n in [0, 1] {
        resource := Resource{ name: if n == 0 -> "continue" else -> "break" }
        defer close(^resource)
        if n == 0 { next }
        break
    }
}

fn run() {
    first := Resource{ name: "a" }
    defer close(^first)
    second := Resource{ name: "b" }
    defer close(^second)
    print("body")
    returned()
    questioned().drop("this path intentionally proves `?` cleanup")
    looped()
    unwind := Resource{ name: "unwind" }
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
    automatic := Resource{ name: "automatic" }
    deferred := Resource{ name: "deferred" }
    defer close(^deferred)
    assert(true)
    print("before failure")
    assert(false, "stop")
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
fn maybe() -> Int Err! { return Err("unused") }
fn run() {
    value := Value{ number: 1 }
    print(value.number)
    maybe().drop("best effort remains an explicit value discard")
}
"#;
    let out = compile(src);
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
    fixed :: mem.Fixed.new(size: 64)
    close(^fixed)
    print("closed")
}
"#;
    let ran = compile_and_run(src, "jet_resource_close_allocators");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(ran.stdout, b"closed\n");
}

#[test]
fn source_pool_does_not_inherit_compiler_owned_close() {
    let src = r#"
use core.mem as mem
struct Pool { value: Int }
fn run() {
    pool :: Pool{value: 1}
    close(^pool)
}
"#;
    let diagnostics = codes(src);
    assert!(diagnostics.contains(&"E0905".to_string()), "{diagnostics:?}");
}

#[test]
fn explicit_close_of_a_database_connection_releases_the_provider_handle() {
    // D-DBDRIVER1: a DB connection is a host handle with no source `Close`
    // impl; `close(^conn)` must consume it through the provider's close, not
    // a missing `DBConnection::close` method.
    let open_only = r#"
use core.db as db
fn run() {
    conn := db.open_memory()
    print("open")
}
"#;
    let closed = r#"
use core.db as db
fn run() {
    conn := db.open_memory()
    print("open")
    close(^conn)
}
"#;
    let provider_closes = |src: &str| compile(src).rust.matches("jet_db_close(").count();
    assert!(
        provider_closes(closed) > provider_closes(open_only),
        "explicit close must release the connection through the provider"
    );
}

#[test]
fn deferred_close_agrees_on_interpreter_and_native() {
    use jet::Interpreter::RunOutcome;

    let root = common::unique_tmp("jet_resource_close_dev");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, SIMPLE).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let (diagnostics, _facts, checked_nominals) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics.into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let bundle = jet::Driver::CheckedSourceBundle::new(bundle, facts, nominals);

    let native = compile_and_run(SIMPLE, "jet_resource_close_native_parity");
    assert!(native.status.success());
    assert_eq!(native.stdout, b"body\nclose dev\n");

    let policy = common::development_policy();
    match common::run_interpreted(&bundle, &checked_nominals, &policy) {
        RunOutcome::Ran { stdout, .. } => {
            assert_eq!(stdout, "body\nclose dev\n");
        }
        RunOutcome::Problems(diags) => {
            panic!("typed deferred cleanup failed on the interpreter: {diags:?}");
        }
    }
}

const CLOSE_CONTAINERS: &str =
    include_str!("../Examples/features/memory/resource_close_containers.jet");
const CLOSE_CONTAINERS_EXPECTED: &str =
    include_str!("../Examples/features/expected/memory/resource_close_containers.out");

#[test]
fn generic_container_close_runs_once_on_rust_aot() {
    assert!(common::have_rustc(), "resource-close runtime proof needs rustc");
    let (code, stdout, stderr) =
        common::build_and_run("jet_close_containers_aot", "main", CLOSE_CONTAINERS);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, CLOSE_CONTAINERS_EXPECTED);
}

#[test]
fn generic_container_close_runs_once_on_interpreter_mir() {
    use jet::Interpreter::RunOutcome;

    let root = common::unique_tmp("jet_close_containers_interpreter");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, CLOSE_CONTAINERS).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    match common::run_interpreted(&bundle, &common::development_policy()) {
        RunOutcome::Ran { stdout, .. } => assert_eq!(stdout, CLOSE_CONTAINERS_EXPECTED),
        RunOutcome::Problems(diags) => {
            panic!("instantiated Close cleanup failed on the interpreter: {diags:?}");
        }
    }
}

#[test]
fn projected_move_from_close_owner_is_explicitly_refused() {
    let src = r#"
struct ClosePair { first: String, second: String }
impl ClosePair.Close {
    fn close(^self) { print(self.second) }
}
fn run() {
    held :: ClosePair{first: "first", second: "second"}
    taken :: ^held.first
    print(taken)
}
"#;
    let diagnostics = match jet::compile(src) {
        Err(diagnostics) => diagnostics,
        Ok(_) => panic!("a non-receiver Close owner was partially moved"),
    };
    let rendered = jet::render_diagnostics("close_pair.jet", src, &diagnostics);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "E0222"
            && diagnostic.why.contains("ClosePair")
            && diagnostic.why.contains("own Close implementation")
    }), "{rendered}");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.span.is_some_and(|span| {
        src[span.start..span.end].contains("held.first")
    })), "refusal must point at the projected move, not the binding: {rendered}");
}

#[test]
fn projected_move_below_atomic_close_leaf_is_explicitly_refused() {
    let src = r#"
struct ClosePair { first: String, second: String }
impl ClosePair.Close {
    fn close(^self) { print(self.second) }
}
struct Holder { value: ClosePair, spare: String }
fn run() {
    held :: Holder{value: ClosePair{first: "first", second: "second"}, spare: "kept"}
    taken :: ^held.value.first
    print(taken)
}
"#;
    let diagnostics = match jet::compile(src) {
        Err(diagnostics) => diagnostics,
        Ok(_) => panic!("a move below an atomic Close leaf was silently lowered"),
    };
    let rendered = jet::render_diagnostics("close_leaf.jet", src, &diagnostics);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "E0222"
            && diagnostic.why.contains("ClosePair")
            && diagnostic.why.contains("own Close implementation")
    }), "{rendered}");
}

#[test]
fn consuming_close_receiver_does_not_exempt_nested_close_owners() {
    let src = include_str!("ui/E0222_close_receiver_nested.jet");
    let diagnostics = jet::compile(src)
        .expect_err("the Close receiver exemption must not extend into a nested Close owner");
    let rendered = jet::render_diagnostics("close_receiver_nested.jet", src, &diagnostics);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "E0222"
            && diagnostic.what.contains("self.value.first")
            && diagnostic.why.contains("ClosePair")
    }), "{rendered}");
}

#[test]
fn borrowed_close_owner_keeps_the_borrowed_move_diagnostic() {
    let src = r#"
struct ClosePair { first: String, second: String }
impl ClosePair.Close {
    fn close(^self) { print(self.second) }
}
fn borrowed(held: ClosePair) {
    taken :: ^held.first
    print(taken)
}
fn run() {}
"#;
    let diagnostics = jet::compile(src).expect_err("a borrowed field cannot move");
    let rendered = jet::render_diagnostics("borrowed_close.jet", src, &diagnostics);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0201"), "{rendered}");
    assert!(!diagnostics.iter().any(|diagnostic| diagnostic.code == "E0222"), "{rendered}");
}
