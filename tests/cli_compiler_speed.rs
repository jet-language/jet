mod common;

#[cfg(unix)]
mod production_path {
    use super::common::Scratch;
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use jet_foundation::JSON::{json_get, json_int, json_str, parse, JSONValue};

    const COMPILER_SPEED_PLAN: &str = include_str!("../docs/plans/compiler-speed.md");
    const DEFAULT_PROFILE_ROUTING_CANARY: &str = "## #1027 default profile routing canary";
    const DEFAULT_PROFILE_ROUTING_CANARY_REQUIREMENTS: &[&str] = &[
        DEFAULT_PROFILE_ROUTING_CANARY,
        "compiler_speed_default_profile_routing_is_removal_sensitive_and_production_backed",
        "`jet run` and `jet dev` use the fast profile and the default native tier;",
        "`jet build` uses the optimized default profile.",
        "Removing or bypassing this plan section must fail the canary.",
    ];
    const CLOSEOUT_CRITERIA_SECTION: &str = "## #666 criterion evidence and removal checks";
    const CLOSEOUT_CRITERIA_REQUIREMENTS: &[&str] = &[
        CLOSEOUT_CRITERIA_SECTION,
        "tools/perf/dashboard.sh",
        "tests/dev_default_parity.rs::dev_default_matches_compiled_binary",
        "tests/cli_compiler_speed.rs::compiler_speed_named_job_dev_matches_run_and_interpreter",
        "tests/build_entry.rs::compiler_speed_phase_timing_reports_real_release_build",
        "tests/cli_compiler_speed.rs::production_build_reports_missing_explicit_linker_as_tool_error",
        "The checked-corpus dashboard emits six rows per active corpus row:",
        "`jit-clean`, `jit-no-change`, `jit-representative-edit`,",
        "`aot-release-clean`, `aot-release-no-change`, and",
        "`aot-release-representative-edit`.",
        "The current `tools/perf/corpus.tsv` has five active",
        "rows, so the minimum matrix is 30 rows.",
        "Each row uses one warmup and twenty",
        "measured samples.",
        "interquartile spread of 100% or less",
        "five Tukey-fence outliers.",
    ];
    const CACHE_REMOVAL_CANARY: &str = "## #1025 cache removal canary";
    const CACHE_REMOVAL_CANARY_REQUIREMENTS: &[&str] = &[
        CACHE_REMOVAL_CANARY,
        "production_build_reuses_and_repairs_stdlib_objects",
        "an unchanged build restores its final binary from BuildCache",
        "a corrupted final binary is rejected by its digest and rebuilt",
        "a program edit reuses the runtime/Core objects",
        "a corrupted runtime object is rebuilt before link",
        "The test also checks that the native cache log exposes the relevant runtime and",
        "Core digests. Removing or bypassing this plan section must fail the canary.",
    ];

    fn jet() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_jet"))
    }

    fn path_program(name: &str) -> PathBuf {
        let path = env::var_os("PATH").unwrap_or_default();
        env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| panic!("{name} is required for the production-path check"))
    }

    fn write_executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn repository_file(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
    }

    fn json_field<'a>(value: &'a JSONValue, key: &str) -> &'a JSONValue {
        json_get(value, key).unwrap_or_else(|| panic!("missing JSON field {key}"))
    }

    fn json_text<'a>(value: &'a JSONValue, key: &str) -> &'a str {
        json_str(json_field(value, key)).unwrap_or_else(|| panic!("JSON field {key} is not text"))
    }

    fn json_number(value: &JSONValue, key: &str) -> i64 {
        json_int(json_field(value, key)).unwrap_or_else(|| panic!("JSON field {key} is not numeric"))
    }

    fn checker_report_from_baseline(baseline: &str) -> String {
        let root = parse(baseline).expect("generated compiler-speed baseline JSON");
        let machine = json_field(&root, "machine");
        let budgets = json_field(&root, "budgets");
        let parity = json_field(&root, "parity");
        let runs = json_field(&root, "runs")
            .as_array()
            .expect("compiler-speed baseline runs array");
        assert_eq!(runs.len() % 6, 0, "baseline runs are not six rows per corpus row");
        let machine_id = format!(
            "{}/{}/cpus={}/host={}",
            json_text(machine, "os"),
            json_text(machine, "arch"),
            json_number(machine, "cpus"),
            json_text(machine, "hostname")
        );
        let mut report = format!(
            "compiler-speed version={} corpus={} corpus_sha256={} manifest_sha256={} stage=matrix machine={} target={} rustc={} llvm={} rustc_vv_sha256={} rustc_sha256={} compiler_sha256={} jet_env_sha256={} libc_sha256={} allocator_sha256={} allocator_environment_sha256={} hardware_sha256={} topology_sha256={} toolchain_sha256={} kernel={} governor={} load1_start_milli={} load1_peak_milli={} load1_end_milli={} memory_bytes={} profiles=jit-fast,aot-release backends=cranelift,rustc-llvm warmups={} samples={} outliers_discarded={} parity={} parity_cases={}\n",
            json_number(&root, "version"),
            runs.len() / 6,
            json_text(&root, "corpus_sha256"),
            json_text(&root, "manifest_sha256"),
            machine_id,
            json_text(machine, "target"),
            json_text(machine, "rustc"),
            json_text(machine, "llvm"),
            json_text(machine, "rustc_vv_sha256"),
            json_text(machine, "rustc_sha256"),
            json_text(machine, "compiler_sha256"),
            json_text(machine, "jet_env_sha256"),
            json_text(machine, "libc_sha256"),
            json_text(machine, "allocator_source_sha256"),
            json_text(machine, "allocator_environment_sha256"),
            json_text(machine, "hardware_sha256"),
            json_text(machine, "topology_sha256"),
            json_text(machine, "toolchain_sha256"),
            json_text(machine, "kernel"),
            json_text(machine, "governor"),
            json_number(machine, "load1_start_milli"),
            json_number(machine, "load1_peak_milli"),
            json_number(machine, "load1_end_milli"),
            json_number(machine, "memory_bytes"),
            json_number(budgets, "warmups"),
            json_number(budgets, "samples"),
            json_number(&root, "outliers_discarded"),
            json_text(parity, "status"),
            json_number(parity, "cases"),
        );
        report.push_str("program\tstate\tstage\tlatency_ns\tmemory_bytes\tvariance_pct\toutput_sha256:stderr_sha256\tphases\n");
        for run in runs {
            report.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}:{}\tphases={}\n",
                json_text(run, "program"),
                json_text(run, "state"),
                json_text(run, "stage"),
                json_number(run, "latency_ns"),
                json_number(run, "memory_bytes"),
                json_number(run, "variance_pct"),
                json_text(run, "stdout_sha256"),
                json_text(run, "stderr_sha256"),
                json_text(run, "phase_totals"),
            ));
        }
        report
    }

    fn run_checker_fixture(name: &str, baseline: &str, report: &str) -> std::process::Output {
        let scratch = Scratch::new(name);
        let fixture_perf = scratch.join("tools/perf");
        fs::create_dir_all(&fixture_perf).unwrap();
        let checker = fixture_perf.join("ci-perf-check.sh");
        fs::copy(repository_file("tools/perf/ci-perf-check.sh"), &checker).unwrap();
        fs::set_permissions(&checker, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(fixture_perf.join("baseline.json"), baseline).unwrap();
        let report_path = scratch.join("current.report");
        fs::write(&report_path, report).unwrap();
        write_executable(
            &fixture_perf.join("dashboard.sh"),
            "#!/bin/sh\ncat \"$JET_TEST_CURRENT_REPORT\"\n",
        );
        let scratch_root = env::var_os("JET_PERF_SCRATCH_ROOT")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/jet-perf")))
            .expect("disk-backed compiler-speed scratch root")
            .join("rust-checker-fixtures");
        Command::new(checker)
            .current_dir(&scratch.path)
            .env("JET_CI_CANDIDATE_COMMIT", "0123456789abcdef0123456789abcdef01234567")
            .env("JET_TEST_CURRENT_REPORT", report_path)
            .env("JET_PERF_SCRATCH_ROOT", scratch_root)
            .output()
            .unwrap()
    }

    fn output_text(output: &std::process::Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    #[test]
    fn compiler_speed_checker_accepts_generated_baseline() {
        let baseline = fs::read_to_string(repository_file("tools/perf/baseline.json")).unwrap();
        let report = checker_report_from_baseline(&baseline);
        let output = run_checker_fixture("compiler-speed-checker-accept", &baseline, &report);
        assert_eq!(
            output.status.code(),
            Some(0),
            "generated baseline was rejected:\n{}",
            output_text(&output)
        );
        assert!(
            output_text(&output).contains("perf gate OK"),
            "checker did not report success:\n{}",
            output_text(&output)
        );
    }

    #[test]
    fn compiler_speed_checker_rejects_unsupported_baseline_version() {
        let baseline = fs::read_to_string(repository_file("tools/perf/baseline.json")).unwrap();
        let version = json_number(&parse(&baseline).unwrap(), "version");
        let bumped = baseline.replacen(
            &format!("\"version\":{version}"),
            &format!("\"version\":{}", version + 1),
            1,
        );
        let output = run_checker_fixture("compiler-speed-checker-version", &bumped, "");
        let text = output_text(&output);
        assert!(!output.status.success(), "unsupported version was accepted:\n{text}");
        assert!(
            text.contains(&format!("unsupported compiler-speed baseline version: {}", version + 1)),
            "version mismatch was not named:\n{text}"
        );
    }

    #[test]
    fn compiler_speed_checker_rejects_changed_machine_rustc_identity() {
        let baseline = fs::read_to_string(repository_file("tools/perf/baseline.json")).unwrap();
        let parsed = parse(&baseline).unwrap();
        let machine_rustc = json_str(json_field(json_field(&parsed, "machine"), "rustc"))
            .expect("machine rustc identity")
            .to_string();
        let bumped = baseline.replacen(
            &format!("\"rustc\":\"{machine_rustc}\""),
            "\"rustc\":\"changed-rustc\"",
            1,
        );
        assert_ne!(bumped, baseline, "machine rustc mutation did not apply");
        let report = checker_report_from_baseline(&baseline);
        let output = run_checker_fixture("compiler-speed-checker-rustc", &bumped, &report);
        let text = output_text(&output);
        assert!(!output.status.success(), "changed rustc identity was accepted:\n{text}");
        assert!(
            text.contains(&format!("rustc changed: changed-rustc -> {machine_rustc}")),
            "rustc mismatch was not named:\n{text}"
        );
    }

    fn prepend_path(directory: &Path) -> std::ffi::OsString {
        let mut paths = vec![directory.to_path_buf()];
        let path = env::var_os("PATH").unwrap_or_default();
        paths.extend(env::split_paths(&path));
        env::join_paths(paths).unwrap()
    }

    fn default_profile_routing_canary_is_intact(plan: &str) -> bool {
        DEFAULT_PROFILE_ROUTING_CANARY_REQUIREMENTS
            .iter()
            .all(|requirement| plan.contains(*requirement))
    }

    fn closeout_criteria_canary_is_intact(plan: &str) -> bool {
        CLOSEOUT_CRITERIA_REQUIREMENTS
            .iter()
            .all(|requirement| plan.contains(*requirement))
    }

    fn cache_removal_canary_is_intact(plan: &str) -> bool {
        CACHE_REMOVAL_CANARY_REQUIREMENTS
            .iter()
            .all(|requirement| plan.contains(*requirement))
    }

    #[test]
    fn compiler_speed_default_profile_routing_is_removal_sensitive_and_production_backed() {
        assert!(
            default_profile_routing_canary_is_intact(COMPILER_SPEED_PLAN),
            "default profile routing is no longer backed by docs/plans/compiler-speed.md"
        );
        let bypassed = COMPILER_SPEED_PLAN.replacen(
            DEFAULT_PROFILE_ROUTING_CANARY,
            "## #1027 route canary bypassed",
            1,
        );
        assert_ne!(
            bypassed, COMPILER_SPEED_PLAN,
            "default profile routing canary mutation did not apply"
        );
        assert!(
            !default_profile_routing_canary_is_intact(&bypassed),
            "the route proof must fail when the compiler-speed plan canary is bypassed"
        );

        let scratch = Scratch::new("compiler-speed-default-profile-routing");
        fs::write(
            scratch.join("main.jet"),
            "fn run() { print(\"default-profile\") }\n",
        )
        .unwrap();

        for args in [
            vec!["run", "--trace-tiers", "main.jet"],
            vec!["dev", "--watch=off", "--trace-tiers", "main.jet"],
        ] {
            let output = Command::new(jet())
                .args(&args)
                .current_dir(&scratch.path)
                .env("JET_CACHE_DIR", scratch.join("run-cache"))
                .env("JET_RUN_CACHE_DIR", scratch.join("jit-cache"))
                .env("NO_COLOR", "1")
                .output()
                .unwrap();
            let trace = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(0),
                "{} failed:\nstdout: {}\nstderr: {trace}",
                args[0],
                String::from_utf8_lossy(&output.stdout)
            );
            assert_eq!(output.stdout, b"default-profile\n", "{} output", args[0]);
            assert!(
                trace
                    .lines()
                    .any(|line| line.starts_with("run") && line.contains("tier1 native")),
                "{} did not use the default fast production lens:\n{trace}",
                args[0]
            );
            assert!(
                !trace.contains("tier0 interp"),
                "{} default route deoptimized:\n{trace}",
                args[0]
            );
        }

        let tools = scratch.join("tools");
        fs::create_dir_all(&tools).unwrap();
        let rustc_log = scratch.join("default-rustc.log");
        let real_rustc = path_program("rustc");
        let real_linker = path_program("cc");
        write_executable(
            &tools.join("rustc"),
            "#!/bin/sh\n\
             { printf '%s\\n' BEGIN; printf '%s\\n' \"$@\"; printf '%s\\n' END; } >> \"$JET_TEST_RUSTC_LOG\"\n\
             exec \"$JET_TEST_REAL_RUSTC\" \"$@\"\n",
        );
        let build = Command::new(jet())
            .args(["build", "main.jet"])
            .current_dir(&scratch.path)
            .env("PATH", prepend_path(&tools))
            .env("RUSTC_LINKER", &real_linker)
            .env("JET_TEST_REAL_RUSTC", &real_rustc)
            .env("JET_TEST_RUSTC_LOG", &rustc_log)
            .env("JET_CACHE_DIR", scratch.join("build-cache"))
            .env("JET_RUNTIME_CACHE_DIR", scratch.join("runtime-cache"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            build.status.code(),
            Some(0),
            "default production build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(run.status.code(), Some(0));
        assert_eq!(run.stdout, b"default-profile\n");

        let log = fs::read_to_string(rustc_log).unwrap();
        let final_args = invocations(&log)
            .into_iter()
            .find(|args| has_pair(args, "--crate-name", "main"))
            .expect("recorded final rustc invocation");
        for flag in ["opt-level=2", "lto=thin", "strip=symbols"] {
            assert!(
                final_args.iter().any(|arg| arg == flag),
                "default profile omitted {flag}: {final_args:?}"
            );
        }
        assert!(
            !final_args.iter().any(|arg| arg == "opt-level=0")
                && !final_args
                    .iter()
                    .any(|arg| arg.starts_with("codegen-units=")),
            "default profile used fast-build flags: {final_args:?}"
        );
    }

    #[test]
    fn compiler_speed_named_job_dev_matches_run_and_interpreter() {
        let scratch = Scratch::new("compiler-speed-named-job-dev");
        fs::copy(
            repository_file("examples/features/devloop/job_runner.jet"),
            scratch.join("run.jet"),
        )
        .unwrap();
        fs::copy(
            repository_file("tools/perf/package.jet"),
            scratch.join("package.jet"),
        )
        .unwrap();
        let expected = "hello from job\nseeded\n";
        let invoke = |args: &[&str], tag: &str| {
            Command::new(jet())
                .args(args)
                .current_dir(&scratch.path)
                .env("JET_CACHE_DIR", scratch.join(&format!("{tag}-build-cache")))
                .env("JET_RUN_CACHE_DIR", scratch.join(&format!("{tag}-run-cache")))
                .env("NO_COLOR", "1")
                .output()
                .unwrap()
        };

        let run = invoke(&["run", "run.jet", "--", "seed_data"], "run");
        let dev = invoke(
            &[
                "dev",
                "run.jet",
                "--watch=off",
                "--quiet",
                "--",
                "seed_data",
            ],
            "dev",
        );
        let interpreter = invoke(
            &["run", "--interpret", "run.jet", "--", "seed_data"],
            "interpreter",
        );
        for (label, output) in [("run", &run), ("dev", &dev), ("interpreter", &interpreter)] {
            assert_eq!(
                output.status.code(),
                Some(0),
                "{label} named-job execution failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                output.stdout,
                expected.as_bytes(),
                "{label} named-job output diverged"
            );
        }
        assert_eq!(run.stdout, dev.stdout, "JIT/dev named-job output diverged");
        assert_eq!(
            run.stdout, interpreter.stdout,
            "JIT/interpreter named-job output diverged"
        );

        let unknown = invoke(
            &[
                "dev",
                "run.jet",
                "--watch=off",
                "--quiet",
                "--",
                "missing_job",
            ],
            "unknown",
        );
        let stderr = String::from_utf8_lossy(&unknown.stderr);
        assert_ne!(unknown.status.code(), Some(0), "unknown dev job was accepted");
        assert!(stderr.contains("E1294"), "unknown dev job lost E1294:\n{stderr}");
    }

    #[test]
    fn compiler_speed_closeout_is_backed_by_plan() {
        for requirement in [
            "The cross-backend proof runs the same checked example through optimized AOT",
            "the default tiered lens, then compares exit status, stdout, and stderr.",
            "The speed proof uses the existing phase reports and typed `CompilerProbe`",
            "The checked-corpus dashboard emits six rows per active corpus row:",
            "interquartile spread of 100% or less",
            "five Tukey-fence outliers.",
        ] {
            assert!(
                COMPILER_SPEED_PLAN.contains(requirement),
                "compiler-speed closeout proof is no longer backed by docs/plans/compiler-speed.md: missing {requirement:?}"
            );
        }
        assert!(
            closeout_criteria_canary_is_intact(COMPILER_SPEED_PLAN),
            "#666 criterion evidence is no longer backed by docs/plans/compiler-speed.md"
        );
        let bypassed = COMPILER_SPEED_PLAN.replacen(
            CLOSEOUT_CRITERIA_SECTION,
            "## #666 criterion evidence and removal checks (bypassed)",
            1,
        );
        assert_ne!(
            bypassed, COMPILER_SPEED_PLAN,
            "#666 criterion evidence canary mutation did not apply"
        );
        assert!(
            !closeout_criteria_canary_is_intact(&bypassed),
            "#666 criterion evidence must fail when its plan section is bypassed"
        );
    }

    #[test]
    fn compiler_speed_cache_removal_canary_is_backed_by_plan() {
        assert!(
            cache_removal_canary_is_intact(COMPILER_SPEED_PLAN),
            "compiler-speed cache canary is no longer backed by docs/plans/compiler-speed.md"
        );
        let bypassed = COMPILER_SPEED_PLAN.replacen(
            CACHE_REMOVAL_CANARY,
            "## #1025 cache proof bypassed",
            1,
        );
        assert_ne!(
            bypassed, COMPILER_SPEED_PLAN,
            "compiler-speed cache canary mutation did not apply"
        );
        assert!(
            !cache_removal_canary_is_intact(&bypassed),
            "the cache proof must fail when the compiler-speed plan canary is bypassed"
        );
    }

    fn invocations(log: &str) -> Vec<Vec<String>> {
        let mut all = Vec::new();
        let mut current = None;
        for line in log.lines() {
            match line {
                "BEGIN" => current = Some(Vec::new()),
                "END" => {
                    if let Some(args) = current.take() {
                        all.push(args);
                    }
                }
                arg => {
                    if let Some(args) = current.as_mut() {
                        args.push(arg.to_string());
                    }
                }
            }
        }
        all
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2)
            .any(|window| window[0] == flag && window[1] == value)
    }

    fn has_digest_field(line: &str, field: &str) -> bool {
        line.split_whitespace().any(|part| {
            let Some(value) = part.strip_prefix(field) else {
                return false;
            };
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    }

    fn build(scratch: &Scratch) -> std::process::Output {
        Command::new(jet())
            .args(["build", "main.jet", "--profile=debug", "--verbose"])
            .current_dir(&scratch.path)
            .env("JET_CACHE_DIR", scratch.join("build-cache"))
            .env("JET_RUNTIME_CACHE_DIR", scratch.join("runtime-cache"))
            .env("JET_RUNTIME_CACHE_STATS", "1")
            .env(
                "JET_DEBUG_NATIVE_CACHE_LOG",
                scratch.join("native-cache.log"),
            )
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }
    fn release_build(scratch: &Scratch) -> std::process::Output {
        Command::new(jet())
            .args(["build", "main.jet", "--profile=release", "--verbose"])
            .current_dir(&scratch.path)
            .env("JET_CACHE_DIR", scratch.join("build-cache"))
            .env("JET_RUNTIME_CACHE_DIR", scratch.join("runtime-cache"))
            .env("JET_TIMING", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    #[test]
    fn production_build_follows_compiler_speed_plan_flags_and_linker() {
        for requirement in [
            "Fast linker (mold → lld → system), tuned rustc flags.",
            "Native rustc builds honor explicit",
            "`RUSTC_LINKER`/`CC`; otherwise Jet selects mold, then lld",
            "Fast builds pass explicit `opt-level=0`, `codegen-units=256`, and",
            "`lto=off`",
            "optimized AOT passes explicit `opt-level=2`, thin LTO, and strip.",
        ] {
            assert!(
                COMPILER_SPEED_PLAN.contains(requirement),
                "compiler-speed production proof is no longer backed by docs/plans/compiler-speed.md: missing {requirement:?}"
            );
        }

        let scratch = Scratch::new("compiler-speed-production");
        fs::write(
            scratch.join("main.jet"),
            "fn run() { print(\"compiler-speed\") }\n",
        )
        .unwrap();
        let tools = scratch.join("tools");
        fs::create_dir_all(&tools).unwrap();
        let rustc_log = scratch.join("rustc.log");
        let real_rustc = path_program("rustc");
        let real_linker = path_program("cc");
        write_executable(
            &tools.join("rustc"),
            "#!/bin/sh\n\
             { printf '%s\\n' BEGIN; printf '%s\\n' \"$@\"; printf '%s\\n' END; } >> \"$JET_TEST_RUSTC_LOG\"\n\
             exec \"$JET_TEST_REAL_RUSTC\" \"$@\"\n",
        );

        let build = Command::new(jet())
            .args(["build", "main.jet", "--profile=debug"])
            .current_dir(&scratch.path)
            .env("PATH", prepend_path(&tools))
            .env("RUSTC_LINKER", &real_linker)
            .env("JET_TEST_REAL_RUSTC", &real_rustc)
            .env("JET_TEST_RUSTC_LOG", &rustc_log)
            .env("JET_CACHE_DIR", scratch.join("build-cache"))
            .env("JET_RUNTIME_CACHE_DIR", scratch.join("runtime-cache"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            build.status.code(),
            Some(0),
            "production build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let run = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(run.status.code(), Some(0));
        assert_eq!(run.stdout, b"compiler-speed\n");

        let log = fs::read_to_string(rustc_log).unwrap();
        let final_args = invocations(&log)
            .into_iter()
            .find(|args| has_pair(args, "--crate-name", "main"))
            .expect("recorded final rustc invocation");
        for flag in ["codegen-units=256", "opt-level=0", "lto=off", "debuginfo=2"] {
            assert!(
                final_args.iter().any(|arg| arg == flag),
                "final rustc invocation omitted {flag}: {final_args:?}"
            );
        }
        assert!(
            has_pair(
                &final_args,
                "-C",
                &format!("linker={}", real_linker.display())
            ),
            "final rustc invocation omitted explicit linker: {final_args:?}"
        );
    }

    #[test]
    fn production_build_reports_missing_explicit_linker_as_tool_error() {
        let scratch = Scratch::new("compiler-speed-linker-failure");
        fs::write(
            scratch.join("main.jet"),
            "fn run() { print(\"should-not-link\") }\n",
        )
        .unwrap();
        let missing_linker = scratch.join("missing-linker");
        let build = Command::new(jet())
            .args(["build", "main.jet", "--profile=debug"])
            .current_dir(&scratch.path)
            .env("RUSTC_LINKER", &missing_linker)
            .env("JET_CACHE_DIR", scratch.join("build-cache"))
            .env("JET_RUNTIME_CACHE_DIR", scratch.join("runtime-cache"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&build.stderr);
        assert_eq!(
            build.status.code(),
            Some(1),
            "unexpected build result:\n{stderr}"
        );
        assert!(
            stderr.contains("L2101"),
            "missing linker lost tool diagnostic:\n{stderr}"
        );
        assert!(
            stderr.contains(&missing_linker.display().to_string()),
            "missing linker path absent from diagnostic:\n{stderr}"
        );
        assert!(
            !stderr.contains("internal compiler error"),
            "missing linker reached ICE rail:\n{stderr}"
        );
    }

    #[test]
    fn production_build_reuses_and_repairs_stdlib_objects() {
        let scratch = Scratch::new("compiler-speed-runtime-cache");
        fs::write(
            scratch.join("main.jet"),
            r#"use core.math as math

fn run() {
    print("first")
    print(math.abs(Float{-1.0}) == 1.0)
}
"#,
        )
        .unwrap();
        fs::write(
            scratch.join("package.jet"),
            "name: \"compiler_speed\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [IO] } }\n",
        )
        .unwrap();

        let cold = build(&scratch);
        assert_eq!(
            cold.status.code(),
            Some(0),
            "cold production build failed:\n{}",
            String::from_utf8_lossy(&cold.stderr)
        );
        let first = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(first.status.code(), Some(0));
        assert_eq!(first.stdout, b"first\ntrue\n");
        assert!(
            String::from_utf8_lossy(&cold.stderr).contains("jet-runtime-cache store"),
            "cold build did not expose a runtime object store:\n{}",
            String::from_utf8_lossy(&cold.stderr)
        );

        let cache_log = fs::read_to_string(scratch.join("native-cache.log")).unwrap();
        assert!(
            cache_log
                .lines()
                .any(|line| {
                    has_digest_field(line, "runtime=")
                        && has_digest_field(line, "corelib=")
                        && has_digest_field(line, "key=")
                }),
            "native cache log did not expose relevant runtime/Core digests:\n{cache_log}"
        );

        let cached_bin = fs::read_dir(scratch.join("build-cache"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("bin"))
            .find(|path| path.is_file())
            .expect("cold build published a final binary cache entry");
        let unchanged = build(&scratch);
        assert_eq!(
            unchanged.status.code(),
            Some(0),
            "unchanged production build failed:\n{}",
            String::from_utf8_lossy(&unchanged.stderr)
        );
        assert!(
            String::from_utf8_lossy(&unchanged.stderr)
                .contains("cache hit -> reused cached binary"),
            "unchanged production build did not reuse its final binary:\n{}",
            String::from_utf8_lossy(&unchanged.stderr)
        );
        let unchanged_output = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(unchanged_output.status.code(), Some(0));
        assert_eq!(unchanged_output.stdout, b"first\ntrue\n");

        fs::write(&cached_bin, b"corrupt final binary").unwrap();
        let final_repaired = build(&scratch);
        assert_eq!(
            final_repaired.status.code(),
            Some(0),
            "corrupt final-cache build failed:\n{}",
            String::from_utf8_lossy(&final_repaired.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&final_repaired.stderr)
                .contains("cache hit -> reused cached binary"),
            "corrupt final binary was reused:\n{}",
            String::from_utf8_lossy(&final_repaired.stderr)
        );
        assert!(
            String::from_utf8_lossy(&final_repaired.stderr).contains("jet-runtime-cache hit"),
            "final cache repair did not reach the warm runtime cache:\n{}",
            String::from_utf8_lossy(&final_repaired.stderr)
        );
        let final_repaired_output = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(final_repaired_output.status.code(), Some(0));
        assert_eq!(final_repaired_output.stdout, b"first\ntrue\n");
        let repaired_cache_hit = build(&scratch);
        assert_eq!(
            repaired_cache_hit.status.code(),
            Some(0),
            "repaired final-cache build failed:\n{}",
            String::from_utf8_lossy(&repaired_cache_hit.stderr)
        );
        assert!(
            String::from_utf8_lossy(&repaired_cache_hit.stderr)
                .contains("cache hit -> reused cached binary"),
            "repaired final binary was not reusable:\n{}",
            String::from_utf8_lossy(&repaired_cache_hit.stderr)
        );
        let cache_log = fs::read_to_string(scratch.join("native-cache.log")).unwrap();
        assert!(
            cache_log.contains("verify-digest-mismatch")
                && cache_log.contains("copy-unverified"),
            "final cache corruption was not visible in the cache log:\n{cache_log}"
        );

        fs::write(
            scratch.join("main.jet"),
            r#"use core.math as math

fn run() {
    print("changed")
    print(math.abs(Float{-1.0}) == 1.0)
}
"#,
        )
        .unwrap();
        let warm = build(&scratch);
        assert_eq!(
            warm.status.code(),
            Some(0),
            "warm production build failed:\n{}",
            String::from_utf8_lossy(&warm.stderr)
        );
        let changed = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(changed.status.code(), Some(0));
        assert_eq!(changed.stdout, b"changed\ntrue\n");
        assert!(
            String::from_utf8_lossy(&warm.stderr).contains("jet-runtime-cache hit"),
            "changed program did not reuse the stdlib object:\n{}",
            String::from_utf8_lossy(&warm.stderr)
        );

        let runtime_rlib = fs::read_dir(scratch.join("runtime-cache"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("libjet_runtime.rlib"))
            .find(|path| path.is_file())
            .expect("cold build published a runtime object");
        let core_rlib = fs::read_dir(scratch.join("runtime-cache"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("libjet_runtime_core.rlib"))
            .find(|path| path.is_file())
            .expect("cold build published a Core object");
        assert_ne!(
            runtime_rlib.parent(),
            core_rlib.parent(),
            "runtime and Core objects must have independent cache entries"
        );
        fs::write(&runtime_rlib, b"corrupt runtime object").unwrap();
        fs::write(
            scratch.join("main.jet"),
            r#"use core.math as math

fn run() {
    print("repaired")
    print(math.abs(Float{-1.0}) == 1.0)
}
"#,
        )
        .unwrap();
        let repaired = build(&scratch);
        assert_eq!(
            repaired.status.code(),
            Some(0),
            "corrupt-cache build failed:\n{}",
            String::from_utf8_lossy(&repaired.stderr)
        );
        let repaired_output = Command::new(scratch.join("build/main"))
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        assert_eq!(repaired_output.status.code(), Some(0));
        assert_eq!(repaired_output.stdout, b"repaired\ntrue\n");
        assert!(
            String::from_utf8_lossy(&repaired.stderr).contains("jet-runtime-cache store"),
            "corrupt cache was not repaired visibly:\n{}",
            String::from_utf8_lossy(&repaired.stderr)
        );
    }
    #[test]
    fn release_build_timing_runs_inner_cache_and_invalidates_on_source_edit() {
        let scratch = Scratch::new("compiler-speed-release-cache-reuse");
        let source = "fn run() { print(\"release-cache\") }\n";
        fs::write(scratch.join("main.jet"), source).unwrap();

        let cold = release_build(&scratch);
        let cold_stderr = String::from_utf8_lossy(&cold.stderr);
        assert_eq!(
            cold.status.code(),
            Some(0),
            "cold release build failed:\n{cold_stderr}"
        );
        assert_eq!(
            cold_stderr.matches("[build] cache miss -> compiling").count(),
            1,
            "cold release build must compile once:\n{cold_stderr}"
        );
        let first_binary = fs::read(scratch.join("build/main")).unwrap();

        // Rewriting identical bytes changes filesystem metadata, but must not
        // change the content key or turn the warm invocation into a miss.
        fs::write(scratch.join("main.jet"), source).unwrap();
        let unchanged = release_build(&scratch);
        let unchanged_stderr = String::from_utf8_lossy(&unchanged.stderr);
        assert_eq!(
            unchanged.status.code(),
            Some(0),
            "unchanged release build failed:\n{unchanged_stderr}"
        );
        assert_eq!(
            unchanged_stderr
                .matches("[build] cache hit -> reused cached binary")
                .count(),
            1,
            "unchanged release build must report one inner cache hit:\n{unchanged_stderr}"
        );
        assert_eq!(
            unchanged_stderr
                .matches("[build] cache miss -> compiling")
                .count(),
            0,
            "unchanged release build must not report a stale receipt miss:\n{unchanged_stderr}"
        );
        assert_eq!(
            first_binary,
            fs::read(scratch.join("build/main")).unwrap(),
            "a byte-identical release build must reuse the same artifact"
        );

        fs::write(
            scratch.join("main.jet"),
            "fn run() { print(\"release-cache-edited\") }\n",
        )
        .unwrap();
        let edited = release_build(&scratch);
        let edited_stderr = String::from_utf8_lossy(&edited.stderr);
        assert_eq!(
            edited.status.code(),
            Some(0),
            "edited release build failed:\n{edited_stderr}"
        );
        assert_eq!(
            edited_stderr
                .matches("[build] cache miss -> compiling")
                .count(),
            1,
            "edited source must invalidate the release cache exactly once:\n{edited_stderr}"
        );
        assert_eq!(
            edited_stderr
                .matches("[build] cache hit -> reused cached binary")
                .count(),
            0,
            "edited source must not reuse the prior release artifact:\n{edited_stderr}"
        );
    }
}
