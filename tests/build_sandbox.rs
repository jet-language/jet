//! T1/T2 (card #99): the build-from-source sandbox contract (D-JPK-ADAPTER1) and
//! the pinned build toolchain (D-JPK-BUILDTOOL1). Drives the internal
//! `BuildRecipe` substrate through the public `jetpack` crate surface so the
//! diagnostic codes are covered by a `tests/` snapshot (invariant I4).

mod common;

use jetpack::Recipe::{self, BuildContext, BuildRecipe, BuildStep};
use jetpack::Toolchain;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(target_os = "windows")]
use jet::Comptime::Build::{
    execute_build_plan, ActionSpec, BuildCapability, BuildContext as HermeticBuildContext,
    TargetSpec,
};

const HOSTILE_CORPUS: &str = include_str!("fixtures/build_sandbox/hostile-corpus.tsv");

static SANDBOX_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn sandbox_test_lock() -> MutexGuard<'static, ()> {
    SANDBOX_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn target_sandbox_manifest_runs() {
    let source = r#"
name: "mathkit"
version: "0.1.0"
packages: {
    mathkit: sandbox { export: "mathkit" },
}
"#;
    let facts = jetpack::Package::PackageFacts::parse(source, "package.jet")
        .expect("sandbox target manifest should parse");
    assert!(matches!(
        facts.packages[0].targets.as_slice(),
        [jetpack::Package::Target::Plugin { export: Some(name) }] if name == "mathkit"
    ));
    let (rewritten, count) =
        jetpack::Package::rewrite_retired_targets(&source.replace("sandbox", "plugin"));
    assert_eq!(count, 1);
    assert!(rewritten.contains("mathkit: sandbox"), "{rewritten}");
}

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "build-sandbox-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn hostile_cases() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let cases = HOSTILE_CORPUS
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 4, "malformed hostile corpus row: {line}");
            (fields[0], fields[1], fields[2], fields[3])
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 7, "hostile corpus lost a required case");
    let mut ids = BTreeSet::new();
    for (id, category, attempt, expected) in &cases {
        assert!(ids.insert(*id), "hostile corpus repeats case {id}");
        assert!(
            matches!(
                *category,
                "escape" | "exfiltration" | "resource-abuse" | "toctou" | "child-process-abuse"
            ),
            "{id} has an unknown hostile category {category}"
        );
        assert!(!attempt.is_empty(), "{id} has no hostile attempt");
        assert_eq!(
            *expected, "blocked-or-unsupported",
            "{id} has a weak result claim"
        );
    }
    for category in [
        "escape",
        "exfiltration",
        "resource-abuse",
        "toctou",
        "child-process-abuse",
    ] {
        assert!(
            cases.iter().any(|(_, found, _, _)| *found == category),
            "hostile corpus lacks category {category}"
        );
    }
    cases
}

#[test]
fn sandbox_denies_ambient_network_e1236() {
    let base = scratch("net");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools: HashMap::new(),
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Fetch {
            url: "https://example.invalid/x.tar".to_string(),
            sha256: String::new(),
        }],
    };
    assert_eq!(Recipe::run(&recipe, &ctx, None).unwrap_err().code, "E1236");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn sandbox_confines_output_e1237() {
    let base = scratch("confine");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("f"), "hi").unwrap();
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools: HashMap::new(),
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Install {
            src: "f".to_string(),
            dest: "../escape".to_string(),
        }],
    };
    assert_eq!(Recipe::run(&recipe, &ctx, None).unwrap_err().code, "E1237");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn sandbox_tool_must_be_a_dep_e1238() {
    let base = scratch("tool");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools: HashMap::new(),
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "gcc".to_string(),
            args: vec![],
        }],
    };
    // `validate` (the `jet inspect audit` read path) flags it without executing.
    assert_eq!(Recipe::validate(&recipe, &ctx).unwrap_err().code, "E1238");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn sandbox_tool_path_must_be_an_absolute_realized_artifact() {
    let base = scratch("relative-tool");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let mut tools = HashMap::new();
    tools.insert("cc".to_string(), PathBuf::from("cc"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "cc".to_string(),
            args: vec![],
        }],
    };
    assert_eq!(Recipe::validate(&recipe, &ctx).unwrap_err().code, "E1238");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(unix)]
#[test]
fn build_hook_does_not_inherit_host_credentials() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("clean-env");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();

    let secret_name = "JET_TEST_SECRET_DO_NOT_LEAK";
    let previous = std::env::var_os(secret_name);
    std::env::set_var(secret_name, "sentinel");
    let mut tools = HashMap::new();
    tools.insert("sh".to_string(), host_tool("sh"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!(
                    "test \"${{{secret_name}:-}}\" = \"\" && printf clean > \"$JET_BUILD_OUTPUT/clean\""
                ),
            ],
        }],
    };
    let result = Recipe::run(&recipe, &ctx, None);
    match previous {
        Some(value) => std::env::set_var(secret_name, value),
        None => std::env::remove_var(secret_name),
    }
    result.unwrap();
    assert_eq!(std::fs::read_to_string(out.join("clean")).unwrap(), "clean");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn native_linux_recipe_sandbox_blocks_host_escape_and_records_backend_receipt() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("native-linux");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    let host_marker = base.with_file_name(format!(
        "{}-host-escape",
        base.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("input"), "source").unwrap();

    let mut tools = HashMap::new();
    tools.insert("sh".to_string(), host_tool("sh"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!(
                    "test ! -e /etc/passwd && if printf hostile > '{}'; then exit 41; fi && if printf hostile > input; then exit 42; fi && printf ok > \"$JET_BUILD_OUTPUT/ok\"",
                    host_marker.display()
                ),
            ],
        }],
    };

    let report = Recipe::run(&recipe, &ctx, None).expect("native Linux sandbox should run recipe");
    assert_eq!(report.sandbox_class, "linux-bwrap");
    assert!(report
        .sandbox_policy
        .contains("filesystem=source-readonly,output-private-copy"));
    assert!(report
        .sandbox_policy
        .contains("process=private-pid,parent-death"));
    assert!(report.sandbox_policy.contains("network=isolated"));
    assert_eq!(std::fs::read_to_string(out.join("ok")).unwrap(), "ok");
    assert!(
        !host_marker.exists(),
        "recipe wrote through to the host filesystem"
    );
    assert_eq!(
        std::fs::read_to_string(src.join("input")).unwrap(),
        "source"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
fn windows_command_interpreter() -> PathBuf {
    std::env::var_os("ComSpec")
        .or_else(|| std::env::var_os("COMSPEC"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"))
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_appcontainer_allows_declared_output_and_records_receipt() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("native-windows-output");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("input"), "source").unwrap();
    let secret_name = "JET_WINDOWS_HOST_SECRET";
    let previous_secret = std::env::var_os(secret_name);
    std::env::set_var(secret_name, "must-not-enter-appcontainer");

    let mut tools = HashMap::new();
    tools.insert("cmd".to_string(), windows_command_interpreter());
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "cmd".to_string(),
            args: vec![
                "/C".to_string(),
                "if defined JET_WINDOWS_HOST_SECRET (exit /b 13) else type \"%CD%\\input\" > \"%JET_BUILD_OUTPUT%\\ok\""
                    .to_string(),
            ],
        }],
    };

    let result = Recipe::run(&recipe, &ctx, None);
    match previous_secret {
        Some(value) => std::env::set_var(secret_name, value),
        None => std::env::remove_var(secret_name),
    }
    let report = result.expect("AppContainer build should run");
    assert_eq!(report.sandbox_class, "windows-appcontainer");
    assert!(report
        .sandbox_policy
        .contains("source-readonly,output-readwrite"));
    for policy in [
        "process=appcontainer+job-kill-on-close+active-process=256",
        "network=denied",
        "environment=clear+declared",
        "devices=appcontainer-default-deny",
        "resources=memory-2GiB+active-process-256",
    ] {
        assert!(report.sandbox_policy.contains(policy), "{policy}");
    }
    assert_eq!(
        std::fs::read_to_string(out.join("ok")).unwrap().trim(),
        "source"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_appcontainer_hermetic_build_allows_declared_output() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("windows-hermetic-success");
    let marker = base.join("host-marker");
    let command = format!(
        "echo escaped>\"{}\" || echo blocked>status & if exist \"{}\" (exit /b 70) else echo ok>status",
        marker.display(),
        marker.display()
    );

    let mut context = HermeticBuildContext::new();
    let action = context
        .action(
            "windows-hermetic-success",
            ActionSpec::uncached_phony([
                windows_command_interpreter().to_string_lossy().into_owned(),
                "/C".to_string(),
                command,
            ])
            .with_outputs(["status"])
            .with_cap(BuildCapability::Exec),
        )
        .expect("hermetic Windows action should register");
    let target = context
        .add_executable(
            "windows-hermetic-success",
            TargetSpec::new().with_action(action),
        )
        .expect("hermetic Windows target should register");
    let plan = context
        .plan_with_default(target)
        .expect("hermetic Windows plan should validate");
    let grants = [BuildCapability::Exec].into_iter().collect();

    let result = execute_build_plan(&plan, &base, &grants)
        .expect("AppContainer hermetic build should complete");
    assert_eq!(result.report.metrics.actions_total, 1);
    assert_eq!(result.report.metrics.failed_actions, 0);
    assert_eq!(
        std::fs::read_to_string(base.join("status")).unwrap().trim(),
        "ok"
    );
    assert!(!marker.exists(), "hermetic build wrote outside its output");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_appcontainer_hermetic_build_rejects_host_write() {
    let _sandbox_guard = sandbox_test_lock();
    assert_eq!(
        jetpack::RuntimePolicy::detect_sandbox().level,
        jetpack::RuntimePolicy::SandboxLevel::Strong,
        "Windows hermetic test requires the native AppContainer backend"
    );
    let base = scratch("windows-hermetic-failure");
    let marker = base.join("host-marker");
    let mut context = HermeticBuildContext::new();
    let action = context
        .action(
            "windows-hermetic-failure",
            ActionSpec::uncached_phony([
                windows_command_interpreter().to_string_lossy().into_owned(),
                "/C".to_string(),
                format!("echo escaped>\"{}\" && exit /b 70", marker.display()),
            ])
            .with_cap(BuildCapability::Exec),
        )
        .expect("hermetic Windows action should register");
    let target = context
        .add_executable(
            "windows-hermetic-failure",
            TargetSpec::new().with_action(action),
        )
        .expect("hermetic Windows target should register");
    let plan = context
        .plan_with_default(target)
        .expect("hermetic Windows plan should validate");
    let grants = [BuildCapability::Exec].into_iter().collect();

    let result = execute_build_plan(&plan, &base, &grants);
    assert!(result.is_err(), "host write must fail the hermetic action");
    assert!(!marker.exists(), "hermetic build wrote outside its output");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_appcontainer_rejects_reparse_parent_before_launch() {
    let _sandbox_guard = sandbox_test_lock();
    use std::os::windows::fs::symlink_dir;

    assert_eq!(
        jetpack::RuntimePolicy::detect_sandbox().level,
        jetpack::RuntimePolicy::SandboxLevel::Strong,
        "Windows reparse-point test requires the native AppContainer backend"
    );
    let base = scratch("windows-reparse-parent");
    let real = base.join("real");
    let linked = base.join("linked");
    let source = real.join("source");
    let output = real.join("output");
    let marker = base.join("launched");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    symlink_dir(&real, &linked).expect("Windows reparse-point test needs a directory link");

    let command = format!(
        "echo launched>\"{}\" & if exist \"{}\" (exit /b 70) else exit /b 0",
        marker.display(),
        marker.display()
    );
    let result = jet::Comptime::Build::run_native_sandboxed(
        &windows_command_interpreter(),
        &["/C".to_string(), command],
        &linked.join("source"),
        Some(&linked.join("output")),
        &BTreeMap::new(),
        false,
    );

    assert!(
        result.is_err(),
        "a reparse-point parent must be rejected before launch: {result:?}"
    );
    assert!(
        !marker.exists(),
        "reparse-point rejection launched the tool"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_appcontainer_blocks_sibling_write_before_publication() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("native-windows-escape");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    let sibling = base.join("sibling-escape");
    std::fs::create_dir_all(&src).unwrap();

    let mut tools = HashMap::new();
    tools.insert("cmd".to_string(), windows_command_interpreter());
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "cmd".to_string(),
            args: vec![
                "/C".to_string(),
                format!("echo escaped>{}", sibling.display()),
            ],
        }],
    };

    let error = Recipe::run(&recipe, &ctx, None).unwrap_err();
    assert_eq!(error.code, "E1238");
    assert!(
        !sibling.exists(),
        "AppContainer wrote outside the declared output"
    );
    assert!(
        !out.join("ok").exists(),
        "failed sandbox stage was published"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
#[test]
fn unavailable_windows_appcontainer_refuses_before_recipe_tool_launch() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("windows-unavailable");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    let marker = base.join("host-marker");
    std::fs::create_dir_all(&src).unwrap();

    let previous = std::env::var_os("JETPACK_FAKE_SANDBOX");
    std::env::set_var("JETPACK_FAKE_SANDBOX", "unavailable");
    let mut tools = HashMap::new();
    tools.insert("cmd".to_string(), windows_command_interpreter());
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "cmd".to_string(),
            args: vec![
                "/C".to_string(),
                format!(
                    "echo launched>\"{}\" & echo launched>\"%JET_BUILD_OUTPUT%\\marker\"",
                    marker.display()
                ),
            ],
        }],
    };
    let result = Recipe::run(&recipe, &ctx, None);
    match previous {
        Some(value) => std::env::set_var("JETPACK_FAKE_SANDBOX", value),
        None => std::env::remove_var("JETPACK_FAKE_SANDBOX"),
    }
    let error = result.expect_err("an unavailable AppContainer backend must fail closed");
    assert_eq!(error.code, "E1275");
    assert!(
        !marker.exists(),
        "the recipe tool launched without AppContainer"
    );
    assert!(!out.join("marker").exists());
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_appcontainer_uses_the_shared_hostile_corpus() {
    let _sandbox_guard = sandbox_test_lock();
    use std::net::TcpListener;
    use std::os::windows::fs::symlink_file;
    use std::thread;
    use std::time::{Duration, Instant};

    let cases = hostile_cases();

    for (case_id, _category, _attempt, _expected) in cases {
        let base = scratch(&format!("windows-hostile-{case_id}"));
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        let host_secret = base.join("host-secret");
        let host_marker = base.join("host-marker");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(&host_secret, "host-only-secret").unwrap();
        if case_id == "source-symlink" {
            symlink_file(&host_secret, src.join("link")).expect(
                "Windows hostile corpus needs symlink creation to test the source boundary",
            );
        }

        let mut network_thread = None;
        let (tool, args) = match case_id {
            "host-read" => (
                windows_command_interpreter(),
                vec![
                    "/C".to_string(),
                    format!(
                        "copy /Y \"{}\" \"{}\" >NUL 2>&1 && exit /b 70 || echo blocked>\"%JET_BUILD_OUTPUT%\\status\"",
                        host_secret.display(),
                        host_marker.display()
                    ),
                ],
            ),
            "host-write" => (
                windows_command_interpreter(),
                vec![
                    "/C".to_string(),
                    format!("echo escaped>\"{}\"", host_marker.display()),
                ],
            ),
            "source-symlink" => (
                windows_command_interpreter(),
                vec![
                    "/C".to_string(),
                    "if exist \"%CD%\\link\" (copy /Y \"%CD%\\link\" \"%JET_BUILD_OUTPUT%\\leak\" >NUL 2>&1 && exit /b 70) else echo blocked>\"%JET_BUILD_OUTPUT%\\status\"".to_string(),
                ],
            ),
            "output-symlink" => (
                windows_command_interpreter(),
                vec![
                    "/C".to_string(),
                    format!(
                        "mklink \"%JET_BUILD_OUTPUT%\\link\" \"{}\" >NUL 2>&1 && echo escaped>\"%JET_BUILD_OUTPUT%\\link\" || echo blocked>\"%JET_BUILD_OUTPUT%\\status\"",
                        host_marker.display()
                    ),
                ],
            ),
            "network-exfiltration" => {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener.set_nonblocking(true).unwrap();
                let port = listener.local_addr().unwrap().port();
                network_thread = Some(thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_millis(750);
                    loop {
                        match listener.accept() {
                            Ok((_stream, _address)) => return true,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                if Instant::now() >= deadline {
                                    return false;
                                }
                                thread::sleep(Duration::from_millis(5));
                            }
                            Err(_) => return false,
                        }
                    }
                }));
                let powershell = PathBuf::from(
                    std::env::var_os("WINDIR")
                        .unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
                )
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe");
                (
                    powershell,
                    vec![
                        "-NoProfile".to_string(),
                        "-NonInteractive".to_string(),
                        "-Command".to_string(),
                        format!(
                            "try {{$c=New-Object Net.Sockets.TcpClient('127.0.0.1',{port});$c.Close();exit 70}} catch {{exit 0}}"
                        ),
                    ],
                )
            }
            "child-process" => {
                let interpreter = windows_command_interpreter();
                (
                    interpreter.clone(),
                    vec![
                        "/C".to_string(),
                        format!(
                            "start \"\" /b \"{}\" /C \"echo escaped>\\\"{}\\\"\"",
                            interpreter.display(),
                            host_marker.display()
                        ),
                    ],
                )
            }
            "tmpfs-exhaustion" => {
                let interpreter = windows_command_interpreter();
                (
                    interpreter.clone(),
                    vec![
                        "/C".to_string(),
                        format!(
                            "for /L %i in (1,1,300) do @start \"\" /b \"{}\" /C \"timeout /T 3 /NOBREAK >NUL\" & echo blocked>\"%JET_BUILD_OUTPUT%\\status\"",
                            interpreter.display()
                        ),
                    ],
                )
            }
            other => panic!("unmapped hostile corpus case {other}"),
        };

        let mut tools = HashMap::new();
        tools.insert("hostile-tool".to_string(), tool);
        let ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools,
            fetch_cache: &cache,
            offline: false,
        };
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "hostile-tool".to_string(),
                args,
            }],
        };
        let result = Recipe::run(&recipe, &ctx, None);
        if let Some(error) = result.as_ref().err() {
            assert!(
                ["E1237", "E1238", "E1275"].contains(&error.code.as_str()),
                "{case_id} returned an unrelated diagnostic: {}",
                error.code
            );
        }
        if let Ok(report) = result {
            assert_eq!(report.sandbox_class, "windows-appcontainer");
            for policy in [
                "filesystem=",
                "process=",
                "network=",
                "environment=",
                "devices=",
                "resources=",
            ] {
                assert!(
                    report.sandbox_policy.contains(policy),
                    "{case_id}: {policy}"
                );
            }
        }
        if let Some(network_thread) = network_thread {
            assert!(
                !network_thread.join().unwrap(),
                "{case_id} reached the network"
            );
        }
        assert!(!host_marker.exists(), "{case_id} wrote the host marker");
        std::fs::remove_dir_all(&base).ok();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn unavailable_native_backend_refuses_before_recipe_tool_launch() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("unavailable");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    let marker = base.join("host-marker");
    std::fs::create_dir_all(&src).unwrap();

    let previous = std::env::var_os("JETPACK_FAKE_SANDBOX");
    std::env::set_var("JETPACK_FAKE_SANDBOX", "unavailable");
    let mut tools = HashMap::new();
    tools.insert("sh".to_string(), host_tool("sh"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!(
                    "printf launched > '{}'; printf launched > \"$JET_BUILD_OUTPUT/marker\"",
                    marker.display()
                ),
            ],
        }],
    };
    let result = Recipe::run(&recipe, &ctx, None);
    match previous {
        Some(value) => std::env::set_var("JETPACK_FAKE_SANDBOX", value),
        None => std::env::remove_var("JETPACK_FAKE_SANDBOX"),
    }
    let error = result.expect_err("an unavailable native backend must fail closed");
    assert_eq!(error.code, "E1275");
    assert!(
        !marker.exists(),
        "the recipe tool launched without a sandbox"
    );
    assert!(!out.join("marker").exists());
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(unix)]
#[test]
fn failed_recipe_preserves_previous_output_and_removes_partial_stage() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("rollback");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("old"), "previous").unwrap();

    let mut tools = HashMap::new();
    tools.insert("sh".to_string(), host_tool("sh"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![
            BuildStep::Exec {
                tool: "sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "printf replacement > \"$JET_BUILD_OUTPUT/new\"".to_string(),
                ],
            },
            BuildStep::Exec {
                tool: "sh".to_string(),
                args: vec!["-c".to_string(), "false".to_string()],
            },
        ],
    };

    let error = Recipe::run(&recipe, &ctx, None).unwrap_err();
    assert_eq!(error.code, "E1238");
    assert_eq!(
        std::fs::read_to_string(out.join("old")).unwrap(),
        "previous"
    );
    assert!(!out.join("new").exists());
    assert!(
        std::fs::read_dir(&base)
            .unwrap()
            .flatten()
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".out.jet-stage-")),
        "failed recipe left a partial staged output"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "macos")]
fn run_macos_attack(tag: &str, command_template: &str) {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch(tag);
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let host = base.join("host-secret");
    let outside = base.join("escape");
    std::fs::write(&host, "host-only").unwrap();
    let command = command_template
        .replace("{HOST}", &host.to_string_lossy())
        .replace("{OUTSIDE}", &outside.to_string_lossy());
    let mut tools = HashMap::new();
    tools.insert("bash".to_string(), PathBuf::from("/bin/bash"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "bash".to_string(),
            args: vec!["-c".to_string(), command],
        }],
    };
    let error = Recipe::run(&recipe, &ctx, None).unwrap_err();
    assert!(matches!(error.code, "E1237" | "E1238"), "{error:?}");
    assert!(
        !outside.exists(),
        "macOS sandbox escape succeeded: {outside:?}"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_native_sandbox_records_seatbelt_policy() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("macos-receipt");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    std::fs::create_dir_all(&src).unwrap();
    let mut tools = HashMap::new();
    tools.insert("bash".to_string(), PathBuf::from("/bin/bash"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "bash".to_string(),
            args: vec![
                "-c".to_string(),
                "printf ok > \"$JET_BUILD_OUTPUT/ok\"".to_string(),
            ],
        }],
    };
    let report = Recipe::run(&recipe, &ctx, None).expect("native macOS sandbox should run");
    assert_eq!(report.sandbox_class, "macos-seatbelt");
    for policy in [
        "filesystem=source-readonly,output-readwrite",
        "process=declared-tool-and-fork",
        "network=denied",
        "environment=clear",
        "devices=denied",
        "resources=",
    ] {
        assert!(report.sandbox_policy.contains(policy), "{policy}");
    }
    assert_eq!(std::fs::read_to_string(out.join("ok")).unwrap(), "ok");
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "macos")]
#[test]
fn unavailable_macos_backend_refuses_before_recipe_tool_launch() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("macos-unavailable");
    let src = base.join("src");
    let out = base.join("out");
    let cache = base.join("cache");
    let marker = base.join("host-marker");
    std::fs::create_dir_all(&src).unwrap();

    let previous = std::env::var_os("JETPACK_FAKE_SANDBOX");
    std::env::set_var("JETPACK_FAKE_SANDBOX", "unavailable");
    let mut tools = HashMap::new();
    tools.insert("bash".to_string(), PathBuf::from("/bin/bash"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "bash".to_string(),
            args: vec![
                "-c".to_string(),
                format!("printf launched > '{}'", marker.display()),
            ],
        }],
    };
    let result = Recipe::run(&recipe, &ctx, None);
    match previous {
        Some(value) => std::env::set_var("JETPACK_FAKE_SANDBOX", value),
        None => std::env::remove_var("JETPACK_FAKE_SANDBOX"),
    }
    let error = result.expect_err("an unavailable macOS backend must fail closed");
    assert_eq!(error.code, "E1275");
    assert!(!marker.exists(), "recipe tool launched without Seatbelt");
    assert!(!out.join("marker").exists());
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_native_sandbox_blocks_host_read() {
    run_macos_attack(
        "macos-host-read",
        "if IFS= read -r value < \"{HOST}\"; then printf '%s' \"$value\" > \"{OUTSIDE}\"; exit 0; else exit 77; fi",
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_native_sandbox_blocks_sibling_write() {
    run_macos_attack(
        "macos-sibling-write",
        "if printf escape > \"{OUTSIDE}\"; then exit 0; else exit 77; fi",
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_native_sandbox_blocks_source_and_symlink_escape() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("macos-symlink");
    let outside = base.join("symlink-result");
    let host = base.join("host-secret");
    let src = base.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(&host, "host-only").unwrap();
    std::os::unix::fs::symlink(&host, src.join("link")).unwrap();
    let command = format!(
        "if IFS= read -r value < \"{}/link\"; then printf '%s' \"$value\" > \"{}\"; exit 0; else exit 77; fi",
        src.display(),
        outside.display()
    );
    let out = base.join("out");
    let cache = base.join("cache");
    let mut tools = HashMap::new();
    tools.insert("bash".to_string(), PathBuf::from("/bin/bash"));
    let ctx = BuildContext {
        source_dir: &src,
        output_root: &out,
        tools,
        fetch_cache: &cache,
        offline: false,
    };
    let recipe = BuildRecipe {
        steps: vec![BuildStep::Exec {
            tool: "bash".to_string(),
            args: vec!["-c".to_string(), command],
        }],
    };
    let error = Recipe::run(&recipe, &ctx, None).unwrap_err();
    assert!(matches!(error.code, "E1237" | "E1238"), "{error:?}");
    assert!(!outside.exists());
    std::fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_native_sandbox_blocks_undeclared_process_network_and_device() {
    for (tag, command) in [
        (
            "macos-process",
            "if /bin/echo escaped >/dev/null; then exit 0; else exit 77; fi",
        ),
        (
            "macos-network",
            "if exec 3<>/dev/tcp/127.0.0.1/9; then exit 0; else exit 77; fi",
        ),
        (
            "macos-device",
            "if test -r /dev/zero; then exit 0; else exit 77; fi",
        ),
    ] {
        run_macos_attack(tag, command);
    }
}

#[test]
fn toolchain_unavailable_is_e1240() {
    let d = Toolchain::e1240();
    assert_eq!(d.code, "E1240");
    assert!(d.fix.contains("jet update jet"));
}

#[cfg(unix)]
#[test]
fn shared_hostile_corpus_uses_the_recipe_production_path() {
    let _sandbox_guard = sandbox_test_lock();
    use std::io::Read;
    use std::net::TcpListener;
    use std::os::unix::fs::symlink;
    use std::thread;
    use std::time::{Duration, Instant};

    let cases = hostile_cases();
    let sandbox_shell = std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH should be available to sandbox tests"),
    )
    .map(|directory| directory.join("bash"))
    .find(|candidate| candidate.is_file())
    .or_else(|| {
        std::env::split_paths(
            &std::env::var_os("PATH").expect("PATH should be available to sandbox tests"),
        )
        .map(|directory| directory.join("sh"))
        .find(|candidate| candidate.is_file())
    })
    .expect("a shell is required for the hostile corpus");

    for (case_id, _category, _attempt, expected) in cases {
        let base = scratch(&format!("hostile-{case_id}"));
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        let host_secret = base.join("host-secret");
        let host_marker = base.join("host-marker");
        let fill_path = PathBuf::from(format!(
            "/tmp/jet-build-hostile-fill-{}-{case_id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(&host_secret, "host-only-secret").unwrap();
        if case_id == "source-symlink" {
            symlink(&host_secret, src.join("link")).unwrap();
        }

        let mut network_thread = None;
        let mut command = match case_id {
            "host-read" => format!(
                "if [ -r {} ]; then printf leaked > \"$JET_BUILD_OUTPUT/status\"; exit 70; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi",
                shell_quote(&host_secret)
            ),
            "host-write" => format!(
                "if printf escaped > {}; then printf escaped > \"$JET_BUILD_OUTPUT/status\"; exit 70; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi",
                shell_quote(&host_marker)
            ),
            "source-symlink" => "if [ -r \"$PWD/link\" ]; then printf leaked > \"$JET_BUILD_OUTPUT/status\"; exit 70; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi".to_string(),
            "output-symlink" => format!(
                "if {} -s {} \"$JET_BUILD_OUTPUT/link\" && printf escaped > \"$JET_BUILD_OUTPUT/link\"; then printf escaped > \"$JET_BUILD_OUTPUT/status\"; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi",
                shell_quote(&host_tool("ln")),
                shell_quote(&host_marker)
            ),
            "network-exfiltration" => {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener
                    .set_nonblocking(true)
                    .expect("test listener should accept nonblocking mode");
                let port = listener.local_addr().unwrap().port();
                network_thread = Some(thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_millis(500);
                    loop {
                        match listener.accept() {
                            Ok((mut stream, _)) => {
                                let mut bytes = Vec::new();
                                let _ = stream.read_to_end(&mut bytes);
                                return bytes == b"escaped";
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                if Instant::now() >= deadline {
                                    return false;
                                }
                                thread::sleep(Duration::from_millis(5));
                            }
                            Err(_) => return false,
                        }
                    }
                }));
                format!(
                    "if printf escaped > /dev/tcp/127.0.0.1/{port} 2>/dev/null; then printf leaked > \"$JET_BUILD_OUTPUT/status\"; exit 70; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi"
                )
            }
            "child-process" => format!(
                "(printf escaped > {}) & child=$!; wait \"$child\"; if [ -e {} ]; then printf escaped > \"$JET_BUILD_OUTPUT/status\"; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi",
                shell_quote(&host_marker),
                shell_quote(&host_marker)
            ),
            "tmpfs-exhaustion" => format!("i=0; while [ \"$i\" -lt 1025 ]; do if ! printf '%65536s' x >> {}; then printf blocked > \"$JET_BUILD_OUTPUT/status\"; break; fi; i=$((i + 1)); done; if [ \"$i\" -ge 1025 ]; then printf escaped > \"$JET_BUILD_OUTPUT/status\"; else printf blocked > \"$JET_BUILD_OUTPUT/status\"; fi", shell_quote(&fill_path)),
            other => panic!("unmapped hostile corpus case {other}"),
        };
        let mut tools = HashMap::new();
        tools.insert("sh".to_string(), sandbox_shell.clone());
        let ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools,
            fetch_cache: &cache,
            offline: false,
        };
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "sh".to_string(),
                args: vec!["-c".to_string(), std::mem::take(&mut command)],
            }],
        };
        let result = Recipe::run(&recipe, &ctx, None);
        match result {
            Ok(report) => {
                assert_ne!(report.sandbox_class, "non-executing");
                assert!(report.sandbox_policy.contains("filesystem="));
                assert!(report.sandbox_policy.contains("process="));
                assert!(report.sandbox_policy.contains("network="));
                assert!(report.sandbox_policy.contains("environment="));
                assert!(report.sandbox_policy.contains("devices="));
                assert!(report.sandbox_policy.contains("resources="));
                assert_eq!(
                    std::fs::read_to_string(out.join("status")).unwrap().trim(),
                    "blocked",
                    "{case_id} corpus result must prove the attempt was blocked"
                );
            }
            Err(error) => assert!(
                ["E1237", "E1275"].contains(&error.code.as_str()),
                "{case_id} returned an executable failure instead of blocked/unsupported: {}",
                error.code
            ),
        }
        if case_id == "child-process" {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(
            !host_marker.exists(),
            "{case_id} escaped the sandbox and wrote the host marker"
        );
        if let Some(network_thread) = network_thread {
            assert!(
                !network_thread.join().unwrap(),
                "{case_id} escaped the network namespace"
            );
        }
        if let Ok(status) = std::fs::read_to_string(out.join("status")) {
            assert_ne!(status, "leaked");
            assert_ne!(status, "escaped");
        }
        std::fs::remove_file(&fill_path).ok();
        std::fs::remove_dir_all(&base).ok();
        assert_eq!(expected, "blocked-or-unsupported");
    }
}

#[cfg(unix)]
#[test]
fn shared_hostile_corpus_uses_agent_executor_production_path() {
    let _sandbox_guard = sandbox_test_lock();
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    let cases = hostile_cases();

    let base = scratch("agent-hostile");
    let host_secret = base.with_file_name("build-sandbox-agent-hostile-secret");
    let host_marker = base.with_file_name("build-sandbox-agent-hostile-marker");
    std::fs::write(&host_secret, "host-only-secret").unwrap();
    let source_link = base.join("link");
    std::os::unix::fs::symlink(&host_secret, &source_link).unwrap();
    let agent_fill_path = format!("/tmp/jet-agent-hostile-fill-{}", std::process::id());

    let mut network_thread = None;
    let commands: Vec<(&str, String)> = cases
        .iter()
        .map(|(case_id, _, _, _)| {
            let command = match *case_id {
                "host-read" => format!(
                    "if test -r {}; then printf escaped; else printf blocked; fi",
                    shell_quote(&host_secret)
                ),
                "host-write" => format!(
                    "if printf escaped > {}; then printf escaped; else printf blocked; fi",
                    shell_quote(&host_marker)
                ),
                "source-symlink" => {
                    "if test -r \"$PWD/link\"; then printf escaped; else printf blocked; fi"
                        .to_string()
                }
                "output-symlink" => format!(
                    "if {} -s {} \"$JET_BUILD_OUTPUT/link\" && printf escaped > \"$JET_BUILD_OUTPUT/link\"; then printf escaped; else printf blocked; fi",
                    shell_quote(&host_tool("ln")),
                    shell_quote(&host_marker)
                ),
                "network-exfiltration" => {
                    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                    listener.set_nonblocking(true).unwrap();
                    let port = listener.local_addr().unwrap().port();
                    network_thread = Some(thread::spawn(move || {
                        let deadline = Instant::now() + Duration::from_millis(750);
                        loop {
                            match listener.accept() {
                                Ok(_) => return true,
                                Err(error)
                                    if error.kind() == std::io::ErrorKind::WouldBlock =>
                                {
                                    if Instant::now() >= deadline {
                                        return false;
                                    }
                                    thread::sleep(Duration::from_millis(5));
                                }
                                Err(_) => return false,
                            }
                        }
                    }));
                    format!(
                        "if printf escaped > /dev/tcp/127.0.0.1/{port} 2>/dev/null; then printf escaped; else printf blocked; fi"
                    )
                }
                "child-process" => format!(
                    "(printf escaped > {}) & printf blocked",
                    shell_quote(&host_marker)
                ),
                "tmpfs-exhaustion" => format!("i=0; while [ \"$i\" -lt 1025 ]; do if ! printf '%65536s' x >> {}; then printf blocked; exit 0; fi; i=$((i + 1)); done; printf escaped", shell_quote(std::path::Path::new(&agent_fill_path))),
                other => panic!("unmapped hostile corpus case {other}"),
            };
            (*case_id, command)
        })
        .collect();

    let mut source = String::from(
        "use core.process as process\n\nfn run() {\n    policy :: process.workspace()\n",
    );
    for (index, (case_id, command)) in commands.iter().enumerate() {
        source.push_str(&format!(
            "    print({})\n    result{index} :: process.cmd([\"bash\", \"-c\", {}]).under(policy).run_checked()\n    if result{index} == {{\n        .Ok(value) -> print(value.output)\n        .Err(_) -> print(\"unsupported\")\n    }}\n",
            jet_string_literal(case_id),
            jet_string_literal(command),
        ));
    }
    source.push_str("}\n");
    let source_path = base.join("agent-hostile.jet");
    std::fs::write(&source_path, source).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--release"])
        .arg(&source_path)
        .current_dir(&base)
        .output()
        .expect("agent executor should launch through the public CLI");
    assert!(
        output.status.success(),
        "agent hostile corpus failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        cases.len() * 2,
        "agent corpus output lost a row"
    );
    let capability = jetpack::RuntimePolicy::detect_sandbox();
    let expected = if capability.level == jetpack::RuntimePolicy::SandboxLevel::Strong {
        "blocked"
    } else {
        "unsupported"
    };
    for (index, (case_id, _, _, _)) in cases.iter().enumerate() {
        assert_eq!(lines[index * 2], *case_id);
        assert_eq!(lines[index * 2 + 1], expected, "{case_id} result");
    }
    if let Some(network_thread) = network_thread {
        assert!(
            !network_thread.join().unwrap(),
            "agent corpus reached the network"
        );
    }
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        std::fs::read_to_string(&host_secret).unwrap(),
        "host-only-secret",
        "agent corpus modified the host secret"
    );
    assert!(!host_marker.exists(), "agent corpus wrote the host marker");
    std::fs::remove_file(&agent_fill_path).ok();
    std::fs::remove_dir_all(&base).ok();
    std::fs::remove_file(&host_secret).ok();
    std::fs::remove_file(&host_marker).ok();
}

#[cfg(unix)]
fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(unix)]
fn jet_string_literal(value: &str) -> String {
    let mut literal = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => literal.push_str("\\\\"),
            '"' => literal.push_str("\\\""),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            character => literal.push(character),
        }
    }
    literal.push('"');
    literal
}

#[cfg(unix)]
fn host_tool(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").expect("PATH should be set"))
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
        .unwrap_or_else(|| PathBuf::from(format!("/bin/{name}")))
}

#[test]
fn core_cargo_build_refuses_before_unavailable_sandbox_can_run_build_script() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("core-cargo-unavailable");
    let repo = base.join("repo");
    let root = base.join("root");
    let marker = base.join("host-marker");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        repo.join("package.jet"),
        "name: \"escape\"\nversion: \"0.1.0\"\npackages: { escape: library }\n",
    )
    .unwrap();
    std::fs::write(repo.join("lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"escape\"\nversion = \"0.1.0\"\nbuild = \"build.rs\"\n[lib]\npath = \"lib.rs\"\n",
    )
    .unwrap();
    std::fs::write(repo.join("Cargo.lock"), "# This file is automatically @generated by Cargo.\nversion = 3\n\n[[package]]\nname = \"escape\"\nversion = \"0.1.0\"\n\n").unwrap();
    std::fs::write(
        repo.join("build.rs"),
        format!(
            "fn main() {{ let _ = std::fs::write({:?}, \"escaped\"); }}\n",
            marker.to_string_lossy()
        ),
    )
    .unwrap();

    let table = jetpack::RefSpec::SourceTable::from_decls([(
        "mine".to_string(),
        format!("path:{}", repo.display()),
        jetpack::RefSpec::ProviderKind::Core,
    )]);
    let spec = jetpack::RefSpec::classify_in("escape@mine", &table).unwrap();
    let roots = jetpack::Store::Roots::at(root.clone());
    let store = roots.hangar_dir();
    let ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store,
        offline: false,
        project_dir: None,
    };

    let previous = std::env::var_os("JETPACK_FAKE_SANDBOX");
    std::env::set_var("JETPACK_FAKE_SANDBOX", "unavailable");
    let result = jetpack::Store::realize_verified(
        &roots,
        &ctx,
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    );
    match previous {
        Some(value) => std::env::set_var("JETPACK_FAKE_SANDBOX", value),
        None => std::env::remove_var("JETPACK_FAKE_SANDBOX"),
    }

    let error = match result {
        Ok(_) => panic!("an unavailable build sandbox must refuse Core Cargo"),
        Err(error) => error,
    };
    assert!(
        format!("{error:?}").contains("SandboxUnavailable"),
        "{error:?}"
    );
    assert!(!marker.exists(), "Cargo build.rs ran without the sandbox");
    assert!(jetpack::Store::list(&roots).is_empty());
    std::fs::remove_dir_all(base).ok();
}

#[cfg(target_os = "linux")]
#[test]
fn core_cargo_build_is_private_and_not_published_to_shared_cache() {
    let _sandbox_guard = sandbox_test_lock();
    let base = scratch("core-cargo-private");
    let repo = base.join("repo");
    let root = base.join("root");
    let mirror = base.join("mirror");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&mirror).unwrap();
    std::fs::write(
        repo.join("package.jet"),
        "name: \"private\"\nversion: \"0.1.0\"\npackages: { private: library }\n",
    )
    .unwrap();
    std::fs::write(repo.join("lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"private\"\nversion = \"0.1.0\"\n[lib]\npath = \"lib.rs\"\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\nversion = 3\n\n[[package]]\nname = \"private\"\nversion = \"0.1.0\"\n\n",
    )
    .unwrap();

    let table = jetpack::RefSpec::SourceTable::from_decls([(
        "mine".to_string(),
        format!("path:{}", repo.display()),
        jetpack::RefSpec::ProviderKind::Core,
    )]);
    let spec = jetpack::RefSpec::classify_in("private@mine", &table).unwrap();
    let roots = jetpack::Store::Roots::at(root.clone());
    jetpack::Store::bind_cache(
        &roots,
        "public",
        vec![mirror.display().to_string()],
        None,
        None,
        true,
    )
    .unwrap();
    let store = roots.hangar_dir();
    let ctx = jetpack::Provider::Ctx {
        fixtures: None,
        store_dir: &store,
        offline: false,
        project_dir: None,
    };

    let realized = jetpack::Store::realize_verified(
        &roots,
        &ctx,
        jetpack::Store::RealizeRequest::Package {
            spec: &spec,
            table: &table,
        },
    )
    .expect("native Core Cargo build should produce a private lease");
    assert_eq!(
        realized.source_state(),
        jetpack::Provider::SourceState::Built
    );
    assert_eq!(
        realized.consumption_status(),
        &jetpack::Store::ConsumptionStatus::Consumable
    );
    let producer =
        jetpack::Store::ProducerRecord::decode(&realized.metadata().producer_record).unwrap();
    assert_eq!(
        producer.facts.get("build.trust").map(String::as_str),
        Some("private-untrusted")
    );
    assert_eq!(
        producer.plan.facts().get("build.trust").map(String::as_str),
        Some("private-untrusted")
    );
    assert!(std::fs::read_dir(&mirror)
        .unwrap()
        .flatten()
        .all(|item| item.path().extension().and_then(|ext| ext.to_str()) != Some("narinfo")));
    drop(realized);
    std::fs::remove_dir_all(base).ok();
}
