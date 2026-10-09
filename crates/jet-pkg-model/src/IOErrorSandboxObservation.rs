// Private cfg-test plumbing. Includes the existing canonical supplier rather
// than copying its implementation or weakening its bounded wait/cleanup.
include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessWindowsSandbox.rs");

use std::io::Write;

const MAX_FIXTURE_COMPONENT_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct FixtureCollection {
    pub(super) output: Result<Output, Error>,
    pub(super) input: Result<(), String>,
}

pub(super) fn run_component_fixture(
    executable: &Path,
    args: &[String],
    source_dir: &Path,
    output_dir: &Path,
    env: &BTreeMap<String, String>,
    mounts: &[ReadOnlyMount],
    component: Vec<u8>,
    timeout: Duration,
) -> FixtureCollection {
    if component.is_empty() || component.len() > MAX_FIXTURE_COMPONENT_BYTES {
        return FixtureCollection {
            output: Err(Error::Io("empty or oversized qualified component input".to_string())),
            input: Err("component input was not sent".to_string()),
        };
    }
    #[cfg(target_os = "windows")]
    {
        // The included Windows supplier keeps its real AppContainer/Job Object
        // implementation and naming. Its current run API creates NUL stdin,
        // not a configurable pipe. Do not skip this case or launch Unix code.
        let status = windows_status();
        return FixtureCollection {
            output: Err(Error::Unsupported(format!(
                "canonical Windows component-stdin fixture is BLOCKED: current supplier creates NUL stdin; mechanism={} available={} reason={}",
                status.mechanism, status.available, status.reason,
            ))),
            input: Err("canonical Windows supplier has no existing piped-stdin configuration".to_string()),
        };
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        return FixtureCollection {
            output: Err(Error::Unsupported("canonical component-stdin fixture has no native supplier for this target".to_string())),
            input: Err("component input was not sent".to_string()),
        };
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let spawned = spawn_with_read_only_mounts(
            executable,
            args,
            source_dir,
            Some(output_dir),
            env,
            true,
            true,
            false,
            mounts,
            None,
            |command| {
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    command.process_group(0);
                }
                Ok(())
            },
        );
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                return FixtureCollection {
                    output: Err(error),
                    input: Err("canonical sandbox spawn failed before input transfer".to_string()),
                };
            }
        };
        let Some(mut stdin) = child.stdin.take() else {
            // Even an invariant failure must use the real finite waiter and
            // tree cleanup, not merely drop the live Child handle.
            return FixtureCollection {
                output: wait_with_limited_output(child, Some(timeout)),
                input: Err("canonical configured sandbox child has no stdin pipe".to_string()),
            };
        };
        let writer = thread::Builder::new()
            .name("io11-fixture-stdin".to_string())
            .spawn(move || {
                let result = stdin
                    .write_all(&component)
                    .map_err(|error| format!("write qualified component stdin: {error}"));
                drop(stdin); // EOF, including write failure.
                result
            });
        // The canonical waiter drains BOTH bounded output streams concurrently
        // with input transfer. Its finite timeout/output failure closes the
        // existing child tree before joining the writer; no private watcher.
        let output = wait_with_limited_output(child, Some(timeout));
        let input = match writer {
            Ok(writer) => match writer.join() {
                Ok(result) => result,
                Err(payload) => {
                    let reason = payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("non-string panic payload");
                    Err(format!("component stdin writer panicked: {reason}"))
                }
            },
            Err(error) => Err(format!("start component stdin writer: {error}")),
        };
        // Keep actual stdout/stderr/status even if input transfer failed; the
        // root test must print/classify them and also require input == Ok(()).
        FixtureCollection { output, input }
    }
}

#[test]
#[ignore = "requires qualified canonical component bytes on stdin and an admitted bridge proof"]
fn canonical_plugin_positive9_component_uses_verified_bridge() {
    const TEST: &str = "__jet_plugin::io_error_ownership_tests::guest_already_exists_positive_context_buffers_release_once";
    let mut component = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_FIXTURE_COMPONENT_BYTES as u64 + 1)
        .read_to_end(&mut component)
        .expect("read bounded qualified writer bytes on root stdin");
    assert!(!component.is_empty() && component.len() <= MAX_FIXTURE_COMPONENT_BYTES);
    let component_digest = crate::SHA256::sha256_hex(&component);
    println!("io11 root component bytes={} sha256={component_digest}", component.len());
    let env = super::cargo_bridge_sandbox_env();
    assert_eq!(env.get("CARGO_BUILD_JOBS").map(String::as_str), Some("1"));
    let target = super::host_target();
    assert_ne!(target, "unknown-rustc-target");
    let link = super::build_bridge(&[], false, false, false, false, false, false, true, false)
        .expect("existing canonical mandatory-sandbox bridge producer");
    let source = super::bridge_crate_dir(&link);
    let paths = super::bridge_paths(&link.cache_identity, &target, &[]);
    assert_eq!(source, paths.cache_root);
    assert_eq!(link.rlib_path, paths.rlib);
    assert_eq!(link.provenance_path, paths.provenance);
    let cargo = super::command_path("cargo").expect("canonical Cargo executable");
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::dirs_home().join(".cargo"));
    let registry = cargo_home.join("registry").canonicalize().expect("canonical registry");
    let mut mounts = vec![jet_sema::Comptime::Build::ReadOnlyMount::new(
        registry,
        "/work/output/cargo-home/registry",
    )];
    let foundation = super::jet_foundation_sources().expect("canonical Foundation closure");
    mounts.extend(
        super::jet_foundation_mounts(&source, &foundation).expect("canonical source/include mounts"),
    );
    let args = [
        "test".to_string(),
        "--release".to_string(),
        "--target".to_string(),
        target.clone(),
        "--manifest-path".to_string(),
        "/work/source/Cargo.toml".to_string(),
        "--lib".to_string(),
        "--no-run".to_string(),
        "--no-fail-fast".to_string(),
        "--message-format=json".to_string(),
    ];
    let built = jet_sema::Comptime::Build::run_native_sandboxed_with_mounts(
        &cargo, &args, &source, Some(&paths.target_dir), &env, true, &mounts,
    )
    .expect("real generated-runtime test build in the canonical sandbox");
    println!("io11 generated no-run stdout bytes: {:?}", built.output.stdout);
    println!("io11 generated no-run stderr bytes: {:?}", built.output.stderr);
    assert!(built.output.status.success(), "actual bridge test build failed");
    let stdout = std::str::from_utf8(&built.output.stdout).expect("actual Cargo JSON is UTF-8");
    let mut executable = None;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let artifact = crate::JSON::parse_json_with_limit(line, 16 * 1024 * 1024)
            .expect("actual Cargo compiler-artifact JSON");
        if artifact.get("reason").expect("Cargo reason").as_str().expect("reason text")
            != "compiler-artifact"
        {
            continue;
        }
        if artifact.get("target").expect("Cargo target").get("name").expect("target name")
            .as_str().expect("target name text") != link.crate_name
        {
            continue;
        }
        let Ok(binary) = artifact.get("executable").expect("Cargo executable").as_str() else {
            continue;
        };
        assert!(executable.is_none(), "one actual generated library test executable");
        let relative = Path::new(binary).strip_prefix("/work/output").expect("own sandbox output");
        assert!(relative.components().all(|part| matches!(part, std::path::Component::Normal(_))));
        executable = Some(paths.target_dir.join(relative));
    }
    let executable = executable.expect("actual nonzero generated test executable");
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let identity_paths = [
        package.join("src/FFI.rs"),
        package.join("src/IOErrorSandboxObservation.rs"),
        package.join("src/Prelude/Plugin.rs"),
        package.join("src/Prelude/PluginIOErrorOwnershipTests.rs"),
        package.join("../jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs"),
        package.join("../jet-codegen/src/Prelude/CoreLib/Top/ProcessWindowsSandbox.rs"),
        source.join("Cargo.toml"),
        source.join("Cargo.lock"),
        source.join("src/lib.rs"),
        link.provenance_path.clone(),
        executable.clone(),
    ];
    let identities = identity_paths.map(|path| {
        let digest = crate::SHA256::sha256_file_hex(&path).expect("qualified source/provider/artifact");
        println!("io11 bridge identity={} file={} sha256={digest}", link.cache_identity, path.display());
        (path, digest)
    });
    let parent = executable.parent().expect("actual generated executable directory");
    mounts.push(jet_sema::Comptime::Build::ReadOnlyMount::new(parent, "/work/io11-test-bin"));
    let inventory_args = [TEST.to_string(), "--ignored".to_string(), "--exact".to_string(), "--list".to_string()];
    let inventory = jet_sema::Comptime::Build::run_native_sandboxed_with_mounts(
        &executable, &inventory_args, &source, Some(&paths.target_dir), &env, true, &mounts,
    )
    .expect("real generated exact ignored inventory in canonical sandbox");
    println!("io11 generated inventory stdout bytes: {:?}", inventory.output.stdout);
    println!("io11 generated inventory stderr bytes: {:?}", inventory.output.stderr);
    assert!(inventory.output.status.success());
    let listed = std::str::from_utf8(&inventory.output.stdout).expect("inventory UTF-8");
    let mut named_rows = listed.lines().filter(|line| line.ends_with(": test"));
    let expected_row = format!("{TEST}: test");
    assert_eq!(named_rows.next(), Some(expected_row.as_str()));
    assert!(named_rows.next().is_none(), "exactly one selected ignored test");
    for (path, expected) in &identities {
        assert_eq!(crate::SHA256::sha256_file_hex(path).expect("pre-run identity"), *expected);
    }
    let mounts = mounts.into_iter()
        .map(|mount| ReadOnlyMount::new(mount.source, mount.destination))
        .collect::<Vec<_>>();
    let args = [TEST.to_string(), "--ignored".to_string(), "--exact".to_string(), "--show-output".to_string(), "--test-threads=1".to_string()];
    let collected = run_component_fixture(
        &executable, &args, &source, &paths.target_dir, &env, &mounts, component,
        Duration::from_secs(120),
    );
    println!("io11 component input-transfer result: {:?}", collected.input);
    let output = collected.output.expect("canonical finite bounded collection and child-tree cleanup");
    println!("io11 component test stdout bytes: {:?}", output.stdout);
    println!("io11 component test stderr bytes: {:?}", output.stderr);
    for (path, expected) in &identities {
        assert_eq!(crate::SHA256::sha256_file_hex(path).expect("post-run identity"), *expected);
    }
    let stdout = std::str::from_utf8(&output.stdout).expect("component stdout UTF-8");
    let stderr = std::str::from_utf8(&output.stderr).expect("component stderr UTF-8");
    assert!(collected.input.is_ok(), "actual concurrent input-transfer/join must succeed");
    let completion = direct_test_completion(TEST, output.status.code(), stdout, stderr);
    assert_eq!(completion, Ok(true), "one actual completed named PASS is required");
    let expected_digest = format!("sha256={component_digest}");
    assert!(stdout.lines().any(|line| line.ends_with(&expected_digest)),
        "generated runtime consumed the same raw qualified component bytes");
}

fn direct_test_completion(
    name: &str,
    status: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<bool, String> {
    let expected_pass = format!("test {name} ... ok");
    let expected_failure = format!("test {name} ... FAILED");
    let completed_pass = stdout.lines().filter(|line| *line == expected_pass).count() == 1
        && stdout.contains("test result: ok. 1 passed; 0 failed; 0 ignored;");
    let completed_failure = stdout.lines().filter(|line| *line == expected_failure).count() == 1
        && stdout.contains("test result: FAILED. 0 passed; 1 failed; 0 ignored;");
    let child_diagnostic = stderr.lines().find(|line| {
        line.trim_start().starts_with("process didn't exit successfully:")
    });
    if child_diagnostic.is_none() {
        if status == Some(0) && completed_pass {
            return Ok(true);
        }
        // The qualified direct libtest returns 101 for an ordinary assertion.
        // Require its completed named packet; a Cargo child diagnostic is unsafe.
        if status == Some(101) && completed_failure {
            return Ok(false);
        }
    }
    Err(format!(
        "unsafe direct child or missing named completion: test={name} status={status:?} child={child_diagnostic:?}",
    ))
}

#[test]
fn direct_completion_classifies_all_seven_packets() {
    const PASS: &str =
        "test fixture ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n";
    const FAIL: &str =
        "test fixture ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored;\n";
    const SIGNAL: &str =
        "process didn't exit successfully: fixture (signal: 11, SIGSEGV: invalid memory reference)";
    let cases = [
        ("completed-pass", Some(0), PASS, "", Some(true)),
        ("completed-direct-fail-101", Some(101), FAIL, "", Some(false)),
        (
            "cargo-child-diagnostic-not-direct-completion",
            Some(101),
            FAIL,
            "process didn't exit successfully: fixture (exit status: 101)",
            None,
        ),
        ("cargo-masked-fatal-101", Some(101), FAIL, SIGNAL, None),
        ("direct-signal", None, FAIL, "", None),
        ("zero-tests", Some(0), "test result: ok. 0 passed; 0 failed; 0 ignored;\n", "", None),
        ("wrong-name", Some(0), "test other ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n", "", None),
    ];
    let mut all_passed = true;
    for (case, status, stdout, stderr, expected) in cases {
        let actual = direct_test_completion("fixture", status, stdout, stderr);
        let passed = actual.as_ref().ok().copied() == expected
            && (stderr != SIGNAL
                || actual.as_ref().err().is_some_and(|reason| reason.contains(SIGNAL)));
        println!(
            "io11 direct completion case={case} result={} actual={actual:?}",
            if passed { "PASS" } else { "FAIL" },
        );
        all_passed &= passed;
    }
    assert!(all_passed, "all independent direct completion packets are attempted");
}

#[test]
fn canonical_sandbox_env_forwards_only_explicit_jobs() {
    let env = super::cargo_bridge_sandbox_env();
    let actual_jobs = std::env::var("CARGO_BUILD_JOBS").ok();
    assert_eq!(env.get("CARGO_BUILD_JOBS"), actual_jobs.as_ref());
    assert!(env.keys().all(|key| matches!(
        key.as_str(),
        "CARGO_HOME" | "CARGO_TARGET_DIR" | "HOME" | "SOURCE_DATE_EPOCH" | "PATH" | "CARGO_BUILD_JOBS",
    )), "the production construction must not restore other ambient keys");
    println!("io11 actual worker-bound input={actual_jobs:?} forwarded={:?}", env.get("CARGO_BUILD_JOBS"));
}
