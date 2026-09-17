use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "common/mod.rs"]
mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

#[test]
fn authority_is_a_named_prelude_rights_carrier() {
    let source = r#"
struct Holder {
    authority: Authority
}

fn run() {
    authority :: Authority.workspace()
    print("authority")
}
"#;
    let output = jet::compile(source).expect("Authority type should compile");
    assert!(
        output.rust.contains("pub struct JetAuthority"),
        "{}",
        output.rust
    );
    assert!(
        output
            .rust
            .contains("rights: std::collections::BTreeSet<String>"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
    assert!(
        output.rust.contains("jet_authority_workspace()"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("FS.Read:repo"), "{}", output.rust);
    assert!(
        output.rust.contains("FS.Write:.jet/build"),
        "{}",
        output.rust
    );
}

fn host_executable(name: &str) -> String {
    std::env::split_paths(&std::env::var_os("PATH").expect("PATH should be set"))
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
        .unwrap_or_else(|| std::path::PathBuf::from(format!("/bin/{name}")))
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

const AUTHORITY_VALUE_SOURCE: &str = r#"
fn run() {
    #FX(authority: FS.Read, IO) {
        narrowed :: authority.with("FS.Read")
        _released :: narrowed.without("FS.Read")
        print("authority")
    }
}
"#;

#[test]
fn authority_with_and_without_are_the_only_narrowing_family() {
    let output = jet::compile(AUTHORITY_VALUE_SOURCE).expect("Authority operations should compile");
    assert!(
        output.rust.contains("jet_authority_with"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_authority_without"),
        "{}",
        output.rust
    );
    tir_support::assert_tiers_agree("authority_narrowing", AUTHORITY_VALUE_SOURCE, "authority\n");
}

#[test]
fn authority_with_outside_held_rights_is_e0712() {
    let source = r#"
fn run() {
    authority :: Authority.workspace()
    authority.with("FS.Write")
}
"#;
    let (_, _, stderr) = tir_support::jit_run("authority_outside", source);
    assert!(stderr.contains("E0712"), "JIT must report E0712: {stderr}");
}

#[test]
fn authority_boundary_consumers_take_the_named_value() {
    let source = r#"
use core.process as process

struct SessionHolder {
    authority: Authority
}

fn run() {
    session :: SessionHolder{authority: Authority.workspace()}
    #FX(authority: Exec, IO, Time.Wait) {
        result :: process.run(["echo", "authority"], authority)
        print("boundary")
    }
}
"#;
    let output = jet::compile(source).expect("boundary APIs should accept Authority");
    assert!(
        output.rust.contains("jet_std_process_run_with_authority"),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
    assert!(
        output.rust.contains("jet_authority_to_wire"),
        "{}",
        output.rust
    );
    assert!(!output.rust.contains("let _authority"), "{}", output.rust);
}

#[test]
fn plugin_call_rejects_an_overlarge_argument_list_before_guest_execution() {
    let scratch = common::Scratch::new("plugin_large_argument_frame");
    let plugin_path = write_plugin_component(
        &scratch,
        "large_args.wasm",
        &large_argument_component_wat(1),
    );
    let plugin_path_text = plugin_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let plugin_root = scratch
        .path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let params = (0..1025)
        .map(|_| "1")
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
use core.plugin as plugin

fn run() {{
    policy :: Authority.from_rights(["FS.Read:{plugin_root}"])
    hostile :: plugin.load("{plugin_path}", policy)
    _result :: hostile.scale({params}) ?? {{
        print("rejected")
        return
    }}
    print("guest-executed")
    print(_result)
}}
"#,
        plugin_path = plugin_path_text,
        plugin_root = plugin_root,
        params = params,
    );
    let api = plugin_api_snapshot("large_args", "scale", "a0: Int", "Int");
    let mut tiers = vec!["jit", "interpreter"];
    if common::have_rustc() {
        tiers.push("release");
    }
    for tier in tiers {
        let (code, stdout, stderr) = tir_support::run_plugin_tier(
            "plugin_large_argument_list",
            &source,
            tier,
            &plugin_path,
            &api,
        );
        assert_ne!(code, 0, "{tier} accepted an overlarge plugin argument list");
        assert_eq!(stdout, "", "{tier} emitted output before arity rejection");
        assert!(
            stderr.contains("E0104"),
            "{tier} did not report the standard arity diagnostic: {stderr}"
        );
    }
}

fn write_plugin_component(
    scratch: &common::Scratch,
    name: &str,
    wat_source: &str,
) -> std::path::PathBuf {
    let bytes = wat::parse_str(wat_source)
        .unwrap_or_else(|error| panic!("hostile plugin WAT should parse: {error}"));
    let path = scratch.join(name);
    std::fs::write(&path, bytes)
        .unwrap_or_else(|error| panic!("write hostile plugin fixture {}: {error}", path.display()));
    path
}

fn plugin_api_snapshot(
    package: &str,
    export: &str,
    params: &str,
    result: &str,
) -> String {
    format!(
        "api_version = 1\npackage = plugin__{package}\npublished_version = 0.1.0\nfn {export}({params}) {result}\n"
    )
}

fn large_argument_component_wat(count: usize) -> String {
    large_named_argument_component_wat("scale", count)
}

fn large_zero_argument_component_wat(count: usize) -> String {
    large_named_argument_component_wat("zero", count)
}

fn large_named_argument_component_wat(export: &str, count: usize) -> String {
    let params = std::iter::repeat("i64")
        .take(count)
        .collect::<Vec<_>>()
        .join(" ");
    let component_params = (0..count)
        .map(|index| format!("(param \"a{index}\" s64)"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"(component
  (core module $m
    (func ${export} (export "{export}") (param {params}) (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func {component_params} (result s64)))
  (func ${export} (type $t)
    (canon lift (core func $i "{export}")))
  (export "{export}" (func ${export})))
"#,
        export = export,
        params = params,
        component_params = component_params,
    )
}

fn plugin_failure_source(
    path: &Path,
    export: &str,
    failure_marker: &str,
    success_marker: &str,
) -> String {
    let plugin_path = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let plugin_root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        r#"
use core.plugin as plugin

fn run() {{
    policy :: Authority.from_rights(["FS.Read:{plugin_root}"])
    hostile :: plugin.load("{plugin_path}", policy)
    _result :: hostile.{export}() ?? {{
        print("{failure_marker}")
        print(err)
        return
    }}
    print("{success_marker}")
    print(_result)
}}
"#,
        plugin_path = plugin_path,
        plugin_root = plugin_root,
        export = export,
        failure_marker = failure_marker,
        success_marker = success_marker,
    )
}

fn assert_plugin_failure_result(
    tier: &str,
    result: &(i32, String, String),
    failure_marker: &str,
    required_error_terms: &[&str],
) {
    let (code, stdout, stderr) = result;
    assert_eq!(code, &0, "{tier} plugin witness failed: {stderr}");
    assert_eq!(stderr, "", "{tier} plugin witness wrote diagnostics");
    assert!(
        stdout.starts_with(&format!("{failure_marker}\n")),
        "{tier} did not return the typed failure marker: {stdout}"
    );
    assert!(
        !stdout.contains("guest-returned"),
        "{tier} entered the success path: {stdout}"
    );
    assert!(
        stdout.len() <= 4096,
        "{tier} returned an unbounded plugin failure: {} bytes",
        stdout.len()
    );
    assert!(
        stdout.contains("trapped"),
        "{tier} did not reach a guest trap through the plugin call seam: {stdout}"
    );
    assert!(
        required_error_terms.is_empty()
            || required_error_terms
                .iter()
                .any(|term| stdout.to_ascii_lowercase().contains(term)),
        "{tier} trap did not identify the expected resource guard: {stdout}"
    );
}

const PLUGIN_CHILD_TEST_ENV: &str = "JET_AUTHORITY_PLUGIN_RESOURCE_TEST";
const PLUGIN_CHILD_TIER_ENV: &str = "JET_AUTHORITY_PLUGIN_RESOURCE_TIER";
const PLUGIN_CHILD_SOURCE_ENV: &str = "JET_AUTHORITY_PLUGIN_RESOURCE_SOURCE";
const PLUGIN_CHILD_ARTIFACT_ENV: &str = "JET_AUTHORITY_PLUGIN_RESOURCE_ARTIFACT";
const PLUGIN_CHILD_API_ENV: &str = "JET_AUTHORITY_PLUGIN_RESOURCE_API";
const PLUGIN_CHILD_OK_MARKER: &str = "JET_AUTHORITY_PLUGIN_RESOURCE_CHILD_OK";
const PLUGIN_CHILD_CAPTURE_LIMIT: usize = 16 * 1024;

fn run_plugin_resource_child_if_selected(test_name: &str) -> bool {
    let Ok(requested_test) = std::env::var(PLUGIN_CHILD_TEST_ENV) else {
        return false;
    };
    if requested_test != test_name {
        return false;
    }
    let tier = std::env::var(PLUGIN_CHILD_TIER_ENV)
        .unwrap_or_else(|error| panic!("plugin child tier is required: {error}"));
    let source = std::env::var(PLUGIN_CHILD_SOURCE_ENV)
        .unwrap_or_else(|error| panic!("plugin child source is required: {error}"));
    let artifact = std::path::PathBuf::from(
        std::env::var(PLUGIN_CHILD_ARTIFACT_ENV)
            .unwrap_or_else(|error| panic!("plugin child artifact is required: {error}")),
    );
    let api = std::env::var(PLUGIN_CHILD_API_ENV)
        .unwrap_or_else(|error| panic!("plugin child API is required: {error}"));
    let (failure_marker, required_error_terms): (&str, &[&str]) = match requested_test.as_str() {
        "plugin_call_stops_a_non_terminating_component_on_all_hosted_tiers" => (
            "execution-bound-failure",
            &["fuel", "epoch", "interrupt"],
        ),
        "plugin_call_rejects_linear_memory_growth_beyond_the_cap_on_all_hosted_tiers" => {
            ("memory-bound-failure", &[])
        }
        "plugin_call_rejects_table_growth_beyond_the_cap_on_all_hosted_tiers" => {
            ("table-bound-failure", &[])
        }
        other => panic!("unknown plugin child test `{other}`"),
    };
    let result = tir_support::run_plugin_tier(
        "plugin_resource_child",
        &source,
        &tier,
        &artifact,
        &api,
    );
    assert_plugin_failure_result(
        &format!("isolated {tier}"),
        &result,
        failure_marker,
        required_error_terms,
    );
    println!("{PLUGIN_CHILD_OK_MARKER}");
    true
}

fn bounded_plugin_capture(path: &Path) -> String {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|error| panic!("open bounded plugin child capture: {error}"));
    let mut bytes = Vec::with_capacity(PLUGIN_CHILD_CAPTURE_LIMIT + 1);
    file.take((PLUGIN_CHILD_CAPTURE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| panic!("read bounded plugin child capture: {error}"));
    let truncated = bytes.len() > PLUGIN_CHILD_CAPTURE_LIMIT;
    bytes.truncate(PLUGIN_CHILD_CAPTURE_LIMIT);
    if truncated {
        let suffix = b"\n[output truncated]";
        let start = PLUGIN_CHILD_CAPTURE_LIMIT.saturating_sub(suffix.len());
        bytes[start..].copy_from_slice(&suffix[..PLUGIN_CHILD_CAPTURE_LIMIT - start]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(unix)]
fn kill_plugin_resource_process_group(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    if let Ok(pid) = i32::try_from(pid) {
        let _ = unsafe { kill(-pid, 9) };
    }
}

fn run_plugin_resource_child(
    test_name: &str,
    tier: &str,
    source: &str,
    artifact: &Path,
    api_snapshot: &str,
) -> (bool, String, String) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let prefix = format!(
        "jet_authority_plugin_resource_{}_{}_{}",
        std::process::id(),
        stamp,
        tier
    );
    let stdout_path = std::env::temp_dir().join(format!("{prefix}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{prefix}.stderr"));
    let stdout_file = std::fs::File::create(&stdout_path)
        .unwrap_or_else(|error| panic!("create plugin child stdout capture: {error}"));
    let stderr_file = std::fs::File::create(&stderr_path)
        .unwrap_or_else(|error| panic!("create plugin child stderr capture: {error}"));
    let mut command = Command::new(
        std::env::current_exe().expect("authority test executable should be available"),
    );
    command
        .args(["--exact", test_name, "--nocapture"])
        .env(PLUGIN_CHILD_TEST_ENV, test_name)
        .env(PLUGIN_CHILD_TIER_ENV, tier)
        .env(PLUGIN_CHILD_SOURCE_ENV, source)
        .env(PLUGIN_CHILD_ARTIFACT_ENV, artifact)
        .env(PLUGIN_CHILD_API_ENV, api_snapshot)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn isolated plugin {tier} child: {error}"));
    let timeout = if tier == "release" {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(10)
    };
    let started = Instant::now();
    loop {
        match child
            .try_wait()
            .unwrap_or_else(|error| panic!("poll isolated plugin {tier} child: {error}"))
        {
            Some(status) => {
                let _ = child
                    .wait()
                    .unwrap_or_else(|error| panic!("reap isolated plugin {tier} child: {error}"));
                let stdout = bounded_plugin_capture(&stdout_path);
                let stderr = bounded_plugin_capture(&stderr_path);
                let _ = std::fs::remove_file(&stdout_path);
                let _ = std::fs::remove_file(&stderr_path);
                return (status.success(), stdout, stderr);
            }
            None if started.elapsed() >= timeout => {
                #[cfg(unix)]
                kill_plugin_resource_process_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                let stdout = bounded_plugin_capture(&stdout_path);
                let stderr = bounded_plugin_capture(&stderr_path);
                let _ = std::fs::remove_file(&stdout_path);
                let _ = std::fs::remove_file(&stderr_path);
                panic!(
                    "isolated plugin {tier} child timed out after {timeout:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn assert_plugin_failure_on_all_hosted_tiers(
    test_name: &str,
    source: &str,
    failure_marker: &str,
    artifact: &Path,
    api_snapshot: &str,
) {
    let mut tiers = vec!["jit", "interpreter"];
    if common::have_rustc() {
        tiers.push("release");
    } else {
        eprintln!("note: skipping AOT plugin resource witness (need rustc)");
    }
    for tier in tiers {
        let (success, stdout, stderr) =
            run_plugin_resource_child(test_name, tier, source, artifact, api_snapshot);
        assert!(
            success,
            "isolated plugin {tier} child failed for {failure_marker}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains(PLUGIN_CHILD_OK_MARKER),
            "isolated plugin {tier} child did not execute the selected test"
        );
        assert!(
            stdout.len() <= PLUGIN_CHILD_CAPTURE_LIMIT
                && stderr.len() <= PLUGIN_CHILD_CAPTURE_LIMIT,
            "isolated plugin {tier} child capture exceeded the bound"
        );
    }
}

const PLUGIN_NON_TERMINATING_COMPONENT_WAT: &str = r#"
(component
  (core module $m
    (func $spin (export "spin") (result i64)
      (loop
        (br 0))
      (i64.const 0)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $spin (type $t)
    (canon lift (core func $i "spin")))
  (export "spin" (func $spin)))
"#;

// One initial 64 KiB page plus 256 requested pages exceeds the 16 MiB cap.

const PLUGIN_LINEAR_MEMORY_COMPONENT_WAT: &str = r#"
(component
  (core module $m
    (memory (export "memory") 1 512)
    (func $grow (export "grow") (result i64)
      (local $old i32)
      (local.set $old (memory.grow (i32.const 256)))
      (if
        (i32.eq (local.get $old) (i32.const -1))
        (then (unreachable)))
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $grow (type $t)
    (canon lift (core func $i "grow")))
  (export "grow" (func $grow)))
"#;

// One initial table slot plus 10,000 requested elements exceeds the table cap.

const PLUGIN_TABLE_COMPONENT_WAT: &str = r#"
(component
  (core module $m
    (table 1 20000 funcref)
    (func $grow (export "grow") (result i64)
      (local $old i32)
      (local.set $old (table.grow (ref.null func) (i32.const 10000)))
      (if
        (i32.eq (local.get $old) (i32.const -1))
        (then (unreachable)))
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $grow (type $t)
    (canon lift (core func $i "grow")))
  (export "grow" (func $grow)))
"#;

#[test]
fn plugin_call_stops_a_non_terminating_component_on_all_hosted_tiers() {
    const TEST_NAME: &str = "plugin_call_stops_a_non_terminating_component_on_all_hosted_tiers";
    if run_plugin_resource_child_if_selected(TEST_NAME) {
        return;
    }
    let scratch = common::Scratch::new("plugin_call_non_terminating");
    let path = write_plugin_component(
        &scratch,
        "non_terminating.wasm",
        PLUGIN_NON_TERMINATING_COMPONENT_WAT,
    );
    let source = plugin_failure_source(
        &path,
        "spin",
        "execution-bound-failure",
        "guest-returned",
    );
    let api = plugin_api_snapshot("non_terminating", "spin", "", "Int");
    assert_plugin_failure_on_all_hosted_tiers(
        TEST_NAME,
        &source,
        "execution-bound-failure",
        &path,
        &api,
    );
}

#[test]
fn plugin_call_rejects_linear_memory_growth_beyond_the_cap_on_all_hosted_tiers() {
    const TEST_NAME: &str =
        "plugin_call_rejects_linear_memory_growth_beyond_the_cap_on_all_hosted_tiers";
    if run_plugin_resource_child_if_selected(TEST_NAME) {
        return;
    }
    let scratch = common::Scratch::new("plugin_call_linear_memory");
    let path = write_plugin_component(
        &scratch,
        "linear_memory.wasm",
        PLUGIN_LINEAR_MEMORY_COMPONENT_WAT,
    );
    let source = plugin_failure_source(
        &path,
        "grow",
        "memory-bound-failure",
        "guest-returned",
    );
    let api = plugin_api_snapshot("linear_memory", "grow", "", "Int");
    assert_plugin_failure_on_all_hosted_tiers(
        TEST_NAME,
        &source,
        "memory-bound-failure",
        &path,
        &api,
    );
}

#[test]
fn plugin_call_rejects_table_growth_beyond_the_cap_on_all_hosted_tiers() {
    const TEST_NAME: &str = "plugin_call_rejects_table_growth_beyond_the_cap_on_all_hosted_tiers";
    if run_plugin_resource_child_if_selected(TEST_NAME) {
        return;
    }
    let scratch = common::Scratch::new("plugin_call_table");
    let path = write_plugin_component(&scratch, "table.wasm", PLUGIN_TABLE_COMPONENT_WAT);
    let source = plugin_failure_source(&path, "grow", "table-bound-failure", "guest-returned");
    let api = plugin_api_snapshot("table", "grow", "", "Int");
    assert_plugin_failure_on_all_hosted_tiers(
        TEST_NAME,
        &source,
        "table-bound-failure",
        &path,
        &api,
    );
}


#[test]
fn authority_process_boundary_runs_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    #FX(authority: Exec, IO, Time.Wait) {
        result :: process.run(["echo", "boundary"], authority)
        print("boundary")
    }
}
"#;
    tir_support::assert_tiers_agree("authority_process_boundary", source, "boundary\n");
}

#[test]
fn authority_plan_refuses_before_spawn_on_all_hosted_tiers() {
    let marker =
        std::env::temp_dir().join(format!("jet-authority-plan-marker-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["sh", "-c", "printf spawned > '__MARKER__'"]).cwd("/").under(policy)
    if spec.plan() == {
        .Ok(_) -> print("spawned")
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__MARKER__", &marker.to_string_lossy());
    let output = jet::compile(&source).expect("authority plan API should compile");
    assert!(
        output.rust.contains("jet_std_process_spec_under"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_process_spec_plan"),
        "{}",
        output.rust
    );
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    let expected = if cfg!(any(target_os = "linux", target_os = "macos")) && native_backend {
        "spawned\n"
    } else {
        "refused\n"
    };
    tir_support::assert_tiers_agree("authority_plan_no_spawn", &source, expected);
    assert!(
        !marker.exists(),
        "plan() spawned the authority-bound command"
    );
}

#[test]
fn authority_process_binds_exact_resource_grants() {
    let cargo = host_executable("cargo");
    let source = r#"
use core.process as process

fn run() {
    policy :: Authority.from_rights([
        "FS.Read:repo",
        "FS.Write:.jet/build",
        "Exec:__CARGO__",
    ])
    spec :: process.cmd(["__CARGO__", "test"]).cwd("/tmp").under(policy)
    if spec.plan() == {
        .Ok(_) -> print("planned")
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__CARGO__", &cargo);
    let output = jet::compile(&source).expect("exact process grants should compile");
    assert!(
        output.rust.contains("jet_authority_from_rights"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("jet_std_process_spec_under"),
        "{}",
        output.rust
    );
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    let expected = if cfg!(any(target_os = "linux", target_os = "macos")) && native_backend {
        "planned\n"
    } else {
        "refused\n"
    };
    tir_support::assert_tiers_agree("authority_process_exact_grants", &source, expected);
}

#[test]
fn authority_process_refuses_unenforced_scoped_network() {
    let source = r#"
use core.process as process

fn run() {
    policy :: Authority.from_rights(["Net:example.com"])
    spec :: process.cmd(["/usr/bin/true"]).under(policy)
    if spec.plan() == {
        .Ok(_) -> print("accepted")
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree("authority_process_scoped_network", source, "refused\n");
}

#[cfg(unix)]
#[test]
fn authority_process_receipt_redacts_secret_output_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    spec :: process.cmd(["sh", "-c", "printf '%s' \"$SECRET_TOKEN\""])
        .env_clear()
        .env("SECRET_TOKEN", "receipt-secret")
        .stdout(.Capture)
        .stderr(.Capture)
    receipt :: spec.run() ?? panic("process receipt failed")
    print(receipt.output)
    print(receipt.redacted)
    print(receipt.policy_digest != "")
    print(receipt.descendants)
}
"#;
    let output = jet::compile(source).expect("ProcessReceipt fields should compile");
    assert!(output.rust.contains("ProcessReceipt"), "{}", output.rust);
    tir_support::assert_tiers_agree(
        "authority_process_receipt_redaction",
        source,
        "<redacted>\ntrue\ntrue\ncontained\n",
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn authority_process_plan_and_receipt_share_the_policy_digest() {
    let printf = host_executable("printf");
    let source = r#"
use core.process as process

fn run() {
    policy :: Authority.from_rights([
        "FS.Read:repo",
        "FS.Write:.jet/build",
        "Exec:__PRINTF__",
    ])
    spec :: process.cmd(["__PRINTF__", "receipt"]).cwd("/tmp").under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            receipt :: spec.run() ?? panic("run failed")
            print(plan.policy_digest == receipt.policy_digest)
            print(receipt.redacted)
        }
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__PRINTF__", &printf);
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    tir_support::assert_tiers_agree(
        "authority_process_receipt_digest",
        &source,
        if native_backend {
            "true\ntrue\n"
        } else {
            "refused\n"
        },
    );
}

#[cfg(target_os = "macos")]
#[test]
fn authority_process_macos_seatbelt_runs_and_denies_host_read() {
    let success = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["/usr/bin/printf", "sandboxed"]).cwd("/tmp").under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            print(plan.backend)
            if spec.run_checked() == {
                .Ok(value) -> print(value.output)
                .Err(_) -> print("denied")
            }
            }
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_macos_seatbelt_success",
        success,
        "macos-seatbelt\nsandboxed\n",
    );

    let hostile = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    if process.cmd(["/bin/sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).cwd("/tmp").under(policy).run_checked() == {
        .Ok(_) -> print("blocked")
        .Err(_) -> print("escaped")
    }
}
"#;
    tir_support::assert_tiers_agree("authority_macos_seatbelt_host_read", hostile, "blocked\n");
}

#[cfg(target_os = "linux")]
#[test]
fn authority_process_linux_bwrap_runs_and_denies_host_read() {
    let success = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["printf", "sandboxed"]).cwd("/tmp").under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            if {
                plan.backend == "linux-bwrap" -> {
                    if spec.run_checked() == {
                        .Ok(value) -> print(value.output)
                        .Err(_) -> print("denied")
                    }
                }
                else -> print("wrong-backend")
            }
        }
        .Err(_) -> print("refused")
    }
}
"#;
    let native_backend = jetpack::RuntimePolicy::detect_sandbox().level
        == jetpack::RuntimePolicy::SandboxLevel::Strong;
    tir_support::assert_tiers_agree(
        "authority_linux_bwrap_success",
        success,
        if native_backend {
            "sandboxed\n"
        } else {
            "refused\n"
        },
    );

    let hostile = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    if process.cmd(["sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).cwd("/tmp").under(policy).run_checked() == {
        .Ok(_) -> print("blocked")
        .Err(_) -> print("escaped")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_linux_bwrap_host_read",
        hostile,
        if native_backend {
            "blocked\n"
        } else {
            "refused\n"
        },
    );
}

#[cfg(target_os = "windows")]
#[test]
fn authority_process_windows_appcontainer_runs_and_denies_host_read() {
    let success = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    if process.cmd(["cmd.exe", "/C", "exit", "0"]).cwd("/tmp").under(policy).run_checked() == {
        .Ok(_) -> print("sandboxed")
        .Err(_) -> print("refused")
    }
}
"#;
    tir_support::assert_tiers_agree(
        "authority_windows_appcontainer_success",
        success,
        "sandboxed\n",
    );

    let marker = std::env::temp_dir().join(format!(
        "jet-authority-windows-host-secret-{}.txt",
        std::process::id()
    ));
    std::fs::write(&marker, "host-secret").expect("write Windows host-read marker");
    let marker = marker.to_string_lossy().replace('\\', "/");
    let hostile = format!(
        r#"
use core.process as process

fn run() {{
    policy :: process.workspace()
    if process.cmd(["cmd.exe", "/C", "type \"{marker}\""]).cwd("/tmp").under(policy).run_checked() == {{
        .Ok(_) -> print("escaped")
        .Err(_) -> print("blocked")
    }}
}}
"#
    );
    tir_support::assert_tiers_agree(
        "authority_windows_appcontainer_host_read",
        &hostile,
        "blocked\n",
    );
    let _ = std::fs::remove_file(marker);
}

#[cfg(target_os = "windows")]
#[test]
fn authority_process_windows_plan_and_receipt_share_policy_digest() {
    let source = r#"
use core.process as process

fn run() {
    policy :: process.workspace()
    spec :: process.cmd(["cmd.exe", "/C", "exit", "0"]).under(policy)
    plan :: spec.plan() ?? panic("plan failed")
    receipt :: spec.run() ?? panic("run failed")
    print(plan.policy_digest == receipt.policy_digest)
    print(receipt.backend)
}
"#;
    tir_support::assert_tiers_agree(
        "authority_process_windows_receipt_digest",
        source,
        "true\nwindows-appcontainer\n",
    );
}

#[test]
fn authority_example_runs_on_all_hosted_tiers() {
    tir_support::assert_example_cli_tiers_agree_with_package(
        "types/authority",
        Some(tir_support::TIR_TEST_PACKAGE),
        |actual| assert_eq!(actual, "authority\n"),
    );
}

#[test]
fn authority_is_not_a_type_selection_or_dispatch_input() {
    let source = r#"
fn run() {
        #FX(Authority) {
        print("Authority must remain ordinary data")
    }
}
"#;
    let diagnostics =
        jet::compile(source).expect_err("Authority must not act as an effect/type fact");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0930"),
        "{diagnostics:#?}"
    );
}

#[test]
fn authority_parser_accepts_the_named_value() {
    let (tokens, diagnostics) = jet::Lexer::lex(
        "fn run() { #FX(authority: FS.Read) { value :: authority.with(\"FS.Read\") } }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    jet::Parser::parse(&tokens).expect("parser must accept Authority");
}

#[test]
fn authority_sema_keeps_the_named_type() {
    let output = jet::compile(AUTHORITY_VALUE_SOURCE).expect("sema should accept Authority");
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_tir_runs_the_same_value() {
    tir_support::assert_tiers_agree("authority_tir", AUTHORITY_VALUE_SOURCE, "authority\n");
}

#[test]
fn authority_aot_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::build_and_run_full(
        "jet_authority_aot",
        "authority_aot",
        AUTHORITY_VALUE_SOURCE,
    );
    assert_eq!(code, 0, "AOT failed: {stderr}");
    assert_eq!(stdout, "authority\n");
}

#[test]
fn authority_jit_runs_the_same_value() {
    let (code, stdout, stderr) = tir_support::jit_run("authority_jit", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "JIT failed: {stderr}");
    assert_eq!(stdout, "authority\n");
}

#[test]
fn authority_dev_runs_the_same_value() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("authority_dev", AUTHORITY_VALUE_SOURCE);
    assert_eq!(code, 0, "dev/interpreter failed: {stderr}");
    assert_eq!(stdout, "authority\n");
}

#[test]
fn authority_comptime_uses_the_same_value() {
    let source = "@authority :: Authority.from_rights([\"FS.Read\", \"IO\"])\n@narrowed :: @authority.with(\"FS.Read\")\n@released :: @narrowed.without(\"FS.Read\")\n\nfn run() { print(\"authority\") }\n";
    let output = jet::compile(source).expect("comptime should construct Authority");
    assert!(output.rust.contains("JetAuthority"), "{}", output.rust);
}

#[test]
fn authority_repl_accepts_the_same_value() {
    let transcript = jet::REPL::run_transcript(
        &[
            "authority :: Authority.workspace()",
            "narrowed :: authority.with(\"FS.Read\")",
            "released :: narrowed.without(\"FS.Read\")",
            "#FX(scoped: FS.Read) { inside :: scoped.with(\"FS.Read\") }",
            "print(\"authority\")",
        ],
        None,
    );
    assert!(
        transcript.contains("authority"),
        "REPL changed Authority meaning: {transcript}"
    );
}

#[test]
fn authority_web_accepts_the_same_value() {
    let source = "#Target(Web)\nfn run() {\n    #FX(authority: IO) {\n        narrowed :: authority.with(\"IO\")\n        _released :: narrowed.without(\"IO\")\n        print(\"authority\")\n    }\n}\n";
    let web = jet::compile_web_with_path(source, "tests/fixtures/authority_web.jet")
        .expect("web should accept Authority")
        .web
        .expect("web tier dropped Authority");
    assert!(
        web.wasm_rust.contains("pub extern \"C\" fn jet_entry_"),
        "web run entry missing"
    );
    assert!(
        web.wasm_rust.contains("jet_runtime_boundary(|| {"),
        "web run did not retain the runtime boundary"
    );
    assert!(
        web.wasm_rust.contains("jet_authority_with"),
        "web lost Authority.with"
    );
    assert!(
        web.wasm_rust.contains("jet_authority_without"),
        "web lost Authority.without"
    );
}

const PLUGIN_ZERO_PARAM_COMPONENT_WAT: &str = r#"
(component
  (core module $m
    (func $zero (export "zero") (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $zero (type $t)
    (canon lift (core func $i "zero")))
  (export "zero" (func $zero)))
"#;

#[test]
fn plugin_call_rejects_an_overlarge_frame_for_a_zero_param_export() {
    let scratch = common::Scratch::new("plugin_zero_param_frame");
    let plugin_path = write_plugin_component(
        &scratch,
        "zero_param.wasm",
        PLUGIN_ZERO_PARAM_COMPONENT_WAT,
    );
    let plugin_path_text = plugin_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let plugin_root = scratch
        .path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let params = std::iter::repeat("1")
        .take(1025)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
use core.plugin as plugin

fn run() {{
    policy :: Authority.from_rights(["FS.Read:{plugin_root}"])
    hostile :: plugin.load("{plugin_path}", policy)
    _result :: hostile.zero({params}) ?? {{
        print("rejected")
        return
    }}
    print("guest-executed")
    print(_result)
}}
"#,
        plugin_path = plugin_path_text,
        plugin_root = plugin_root,
        params = params,
    );
    let api = plugin_api_snapshot("zero_param", "zero", "", "Int");
    let mut tiers = vec!["jit", "interpreter"];
    if common::have_rustc() {
        tiers.push("release");
    }
    for tier in tiers {
        let (code, stdout, stderr) = tir_support::run_plugin_tier(
            "plugin_zero_param_overlarge_frame",
            &source,
            tier,
            &plugin_path,
            &api,
        );
        assert_ne!(code, 0, "{tier} unexpectedly ran an overlarge plugin frame");
        assert_eq!(stdout, "", "{tier} emitted output before arity rejection");
        assert!(
            stderr.contains("E0104"),
            "{tier} did not report the standard arity diagnostic: {stderr}"
        );
    }
}

#[cfg(unix)]
#[test]
fn plugin_load_rejects_a_symlink_outside_the_granted_read_root() {
    use std::os::unix::fs::symlink;

    let outside = common::Scratch::new("plugin_outside_read_root");
    let allowed = common::Scratch::new("plugin_allowed_read_root");
    let target = write_plugin_component(
        &outside,
        "target.wasm",
        PLUGIN_ZERO_PARAM_COMPONENT_WAT,
    );
    let link = allowed.join("link.wasm");
    symlink(&target, &link).expect("create hostile plugin symlink");
    let plugin_path = link
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let root = allowed
        .path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = format!(
        r#"
use core.plugin as plugin

fn run() {{
    policy :: Authority.from_rights(["FS.Read:{root}"])
    hostile :: plugin.load("{plugin_path}", policy)
    _result :: hostile.zero() ?? {{
        print("rejected")
        return
    }}
    print("guest-executed")
    print(_result)
}}
"#,
        plugin_path = plugin_path,
        root = root,
    );
    let api = plugin_api_snapshot("link", "zero", "", "Int");
    let mut tiers = vec!["jit", "interpreter"];
    if common::have_rustc() {
        tiers.push("release");
    }
    for tier in tiers {
        let (code, stdout, stderr) = tir_support::run_plugin_tier(
            "plugin_symlink_outside_read_root",
            &source,
            tier,
            &link,
            &api,
        );
        assert_ne!(code, 0, "{tier} followed a plugin symlink outside its root");
        assert_eq!(stdout, "", "{tier} emitted output before symlink rejection");
        assert!(
            stderr.contains("file authority path contains a symlink"),
            "{tier} did not report the symlink rejection: {stderr}"
        );
    }
}

#[test]
fn erased_devtools_publications_do_not_feed_panel_state() {
    let source = r#"
use core.ui as ui
use core.devtools as devtools

struct State {
    selected: String{"none"}
    event_count: Int{0}
}

fn panel_lines(state: State) [UiNode] -> {
    return [UiNode]{
        ui.text(state.selected),
        ui.text("{state.event_count}")
    }
}

#DevPanel
pub fn panel(state: State) UiNode -> {
    return ui.box(panel_lines(state))
}

fn run() {
    selected_value :: "overview"
    #Off {
        devtools.publish(field: .selected, value: selected_value)
        devtools.publish(field: .event_count, value: 1)
    }
    print("ok")
}
"#;
    let diagnostics = tir_support::compile_source("erased_devtools_publications", source)
        .expect_err("erased publications must not satisfy panel state fields");
    let error_codes = diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();
    assert_eq!(
        error_codes,
        vec!["E1414".to_string(), "E1414".to_string()]
    );
}
