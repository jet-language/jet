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
        output.rust.contains("JetAuthority::workspace()"),
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
use core.plugin as plugin

struct SessionHolder {
    authority: Authority
}

fn run() {
    session :: SessionHolder{authority: Authority.workspace()}
    #FX(authority: Exec, IO) {
        result :: process.run(["echo", "authority"], authority)
        plugin :: plugin.load("missing.wasm", session.authority)
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
    assert!(output.rust.contains("jet_plugin_load"), "{}", output.rust);
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
    let plugin_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/sandbox_mathkit/mathkit.wasm")
        .canonicalize()
        .expect("plugin fixture should exist")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let params = std::iter::repeat("1.0")
        .take(1025)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
use core.plugin as plugin

fn run() {{
    policy :: Authority.from_rights(["FS.Read:repo"])
    mathkit :: plugin.load("{plugin_path}", policy)
    greeting :: mathkit.call_text("greet", ["Ada"]) ?? panic("plugin fixture")
    print(greeting)
    _result :: mathkit.call("scale", [{params}]) ?? {{
        print(err)
        return
    }}
    print("guest-executed")
}}
"#,
        plugin_path = plugin_path,
        params = params
    );
    let expected = "hello, Ada!\n`scale` expects 2 argument(s), got 0\n";
    let (interpreter_code, interpreter_stdout, interpreter_stderr) =
        tir_support::interpreter_run("jet_plugin_limits_interpreter", &source);
    assert_eq!(
        interpreter_code, 0,
        "forced interpreter plugin resource test failed: {interpreter_stderr}"
    );
    assert_eq!(interpreter_stdout, expected);
    assert!(
        !interpreter_stdout.contains("guest-executed"),
        "forced interpreter entered the guest: {interpreter_stdout}"
    );
    assert_eq!(interpreter_stderr, "");

    if common::have_rustc() {
        let (aot_code, aot_stdout, aot_stderr) =
            common::build_and_run("jet_plugin_limits", "wire_limit", &source);
        assert_eq!(aot_code, 0, "AOT plugin resource test failed: {aot_stderr}");
        assert_eq!(aot_stdout, expected);
        assert!(
            !aot_stdout.contains("guest-executed"),
            "AOT entered the guest: {aot_stdout}"
        );
        assert_eq!(aot_stderr, "");
        assert_eq!(
            aot_stdout, interpreter_stdout,
            "AOT and forced interpreter plugin errors differ"
        );
        assert_eq!(
            aot_stderr, interpreter_stderr,
            "AOT and forced interpreter diagnostics differ"
        );
    } else {
        eprintln!("note: skipping AOT plugin resource witness (need rustc)");
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
    _result :: hostile.call_int("{export}", []) ?? {{
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
    let result = match tier.as_str() {
        "jit" => tir_support::jit_run("plugin_resource_child_jit", &source),
        "interpreter" => {
            tir_support::interpreter_run("plugin_resource_child_interpreter", &source)
        }
        "aot" => {
            assert!(common::have_rustc(), "AOT child requires rustc");
            common::build_and_run("jet_plugin_resource_child", "resource", &source)
        }
        other => panic!("unknown plugin child tier `{other}`"),
    };
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
    let timeout = if tier == "aot" {
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
) {
    let mut tiers = vec!["jit", "interpreter"];
    if common::have_rustc() {
        tiers.push("aot");
    } else {
        eprintln!("note: skipping AOT plugin resource witness (need rustc)");
    }
    for tier in tiers {
        let (success, stdout, stderr) = run_plugin_resource_child(test_name, tier, source);
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
    assert_plugin_failure_on_all_hosted_tiers(TEST_NAME, &source, "execution-bound-failure");
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
    assert_plugin_failure_on_all_hosted_tiers(TEST_NAME, &source, "memory-bound-failure");
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
    assert_plugin_failure_on_all_hosted_tiers(TEST_NAME, &source, "table-bound-failure");
}


#[test]
fn authority_process_boundary_runs_on_all_hosted_tiers() {
    let source = r#"
use core.process as process

fn run() {
    #FX(authority: Exec, IO) {
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
    spec :: process.cmd(["sh", "-c", "printf spawned > '__MARKER__'"]).under(policy)
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
    spec :: process.cmd(["__CARGO__", "test"]).under(policy)
    if spec.plan() == {
        .Ok(_) -> print("planned")
        .Err(_) -> print("refused")
    }
}
"#
    .replace("__CARGO__", &cargo);
    let output = jet::compile(&source).expect("exact process grants should compile");
    assert!(
        output.rust.contains("JetAuthority::__jet_from_rights"),
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
    spec :: process.cmd(["__PRINTF__", "receipt"]).under(policy)
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
    spec :: process.cmd(["/usr/bin/printf", "sandboxed"]).under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            print(plan.backend)
            result :: spec.run_checked()
            if result == {
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
    result :: process.cmd(["/bin/sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).under(policy).run_checked()
    if result == {
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
    spec :: process.cmd(["printf", "sandboxed"]).under(policy)
    if spec.plan() == {
        .Ok(plan) -> {
            if {
                plan.backend == "linux-bwrap" -> {
                    result :: spec.run_checked()
                    if result == {
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
    result :: process.cmd(["sh", "-c", "if test -r /etc/passwd; then exit 41; else exit 0; fi"]).under(policy).run_checked()
    if result == {
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
    result :: process.cmd(["cmd.exe", "/C", "exit", "0"]).under(policy).run_checked()
    if result == {
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
    result :: process.cmd(["cmd.exe", "/C", "type \"{marker}\""]).under(policy).run_checked()
    if result == {{
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
    tir_support::assert_example_cli_tiers_agree("types/authority", "authority\n");
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
    let source = "@authority :: Authority.from_rights([\"FS.Read\", \"IO\"])\n@narrowed :: authority.with(\"FS.Read\")\n@released :: narrowed.without(\"FS.Read\")\n\nfn run() { print(\"authority\") }\n";
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
    let source = "#Target(Web)\nfn run() {\n    #FX(authority: IO) {\n        narrowed :: authority.with(\"IO\")\n        released :: narrowed.without(\"IO\")\n        value :: released\n    }\n}\n";
    let web = jet::compile_web_with_path(source, "tests/fixtures/authority_web.jet")
        .expect("web should accept Authority")
        .web
        .expect("web tier dropped Authority");
    assert!(
        web.wasm_rust
            .contains("pub extern \"C\" fn jet_export_run() -> i32"),
        "web run export missing"
    );
    assert!(
        web.wasm_rust.contains("jet_authority_with"),
        "web lost Authority.with"
    );
    assert!(
        web.wasm_rust.contains("jet_authority_without"),
        "web lost Authority.without"
    );
    assert!(
        !web.wasm_rust.contains("struct Authority"),
        "web handle leaked into emission"
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
    )
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
    _result :: hostile.call_int("zero", [{params}]) ?? {{
        print("rejected")
        return
    }}
    print("guest-executed")
    print(_result)
}}
"#,
        plugin_path = plugin_path,
        plugin_root = plugin_root,
        params = params,
    );
    tir_support::assert_tiers_agree(
        "plugin_zero_param_overlarge_frame",
        &source,
        "rejected\n",
    );
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
    _result :: hostile.call_int("zero", []) ?? {{
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
    tir_support::assert_tiers_agree(
        "plugin_symlink_outside_read_root",
        &source,
        "rejected\n",
    );
}
