mod common;

#[cfg(unix)]
mod production_path {
    use super::common::Scratch;
    use jet_foundation::DataTree::DataTree;
    use jet_foundation::JSON::{json_get, json_int, json_str, parse};
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;


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

    fn json_field<'a>(value: &'a DataTree, key: &str) -> &'a DataTree {
        json_get(value, key).unwrap_or_else(|| panic!("missing JSON field {key}"))
    }

    fn json_text<'a>(value: &'a DataTree, key: &str) -> &'a str {
        json_str(json_field(value, key)).unwrap_or_else(|| panic!("JSON field {key} is not text"))
    }

    fn json_number(value: &DataTree, key: &str) -> i64 {
        json_int(json_field(value, key))
            .unwrap_or_else(|| panic!("JSON field {key} is not numeric"))
    }

    fn synthetic_v4_baseline() -> String {
        let states = [
            "jit-clean",
            "jit-no-change",
            "jit-representative-edit",
            "aot-release-clean",
            "aot-release-no-change",
            "aot-release-representative-edit",
        ];
        let source_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let expected_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let manifest_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let workload_hash = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let libc_hash = "1111111111111111111111111111111111111111111111111111111111111111";
        let allocator_hash = "2222222222222222222222222222222222222222222222222222222222222222";
        let allocator_environment_hash =
            "3333333333333333333333333333333333333333333333333333333333333333";
        let hardware_hash = "4444444444444444444444444444444444444444444444444444444444444444";
        let topology_hash = "5555555555555555555555555555555555555555555555555555555555555555";
        let toolchain_hash = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let rustc_hash = "6666666666666666666666666666666666666666666666666666666666666666";
        let output_hash = "7777777777777777777777777777777777777777777777777777777777777777";
        let error_hash = "8888888888888888888888888888888888888888888888888888888888888888";
        let linker_hash = "9999999999999999999999999999999999999999999999999999999999999999";
        let compiler_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let rustc_vv_hash = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let jet_env_hash = "abababababababababababababababababababababababababababababababab";
        let peer_contract = "6f9085bde94607c64f688587c06e6fc2f3e02fba10104c960c7488dba341c183";
        let mut runs = String::new();
        let mut peers = String::new();
        for state in states {
            let (stage, profile, backend, linker, linker_identity, cache_state, cache_policy) =
                if state.starts_with("jit-") {
                    (
                        "jit-fast",
                        "fast",
                        "cranelift",
                        "none",
                        "none",
                        if state.ends_with("no-change") {
                            "NoChange"
                        } else if state.ends_with("representative-edit") {
                            "Edit"
                        } else {
                            "Clean"
                        },
                        if state.ends_with("no-change") {
                            "shared-cache-after-warmup"
                        } else if state.ends_with("representative-edit") {
                            "base-cache-snapshot-before-edit"
                        } else {
                            "fresh-cache-per-sample"
                        },
                    )
                } else {
                    (
                        "aot-release",
                        "release",
                        "rustc-llvm",
                        "ld",
                        linker_hash,
                        if state.ends_with("no-change") {
                            "NoChange"
                        } else if state.ends_with("representative-edit") {
                            "Edit"
                        } else {
                            "Clean"
                        },
                        if state.ends_with("no-change") {
                            "shared-cache-after-warmup"
                        } else if state.ends_with("representative-edit") {
                            "base-cache-snapshot-before-edit"
                        } else {
                            "fresh-cache-per-sample"
                        },
                    )
                };
            let phase = format!(
                "parse_us=1;sema_us=1;source=fixture.jet;source_sha256={source_hash};source_bytes=1;expected_sha256={expected_hash};expected_bytes=1;manifest_sha256={manifest_hash};workload_sha256={workload_hash};role=base;profile={profile};backend={backend};linker={linker};linker_path={linker};linker_sha256={linker_identity};linker_backend={linker};linker_backend_path={linker};linker_backend_sha256={linker_identity};cache_state={cache_state};cache_policy={cache_policy};cache_hits=1;cache_misses=0;libc_sha256={libc_hash};allocator_sha256={allocator_hash};allocator_environment_sha256={allocator_environment_hash};hardware_sha256={hardware_hash};topology_sha256={topology_hash};toolchain_sha256={toolchain_hash};rustc_sha256={rustc_hash};top_cause=none;artifact_bytes=0;parity=verified;semantic_parity=verified;diagnostic_parity=verified;effect_parity=verified;tier_parity=verified;dev_profile=dev;aot_profile=release"
            );
            if !runs.is_empty() {
                runs.push(',');
            }
            runs.push_str(&format!(
                r#"{{"program":"fixture.jet","state":"{state}","stage":"{stage}","latency_ns":100,"memory_bytes":100,"variance_pct":0,"stdout_sha256":"{output_hash}","stderr_sha256":"{error_hash}","phase_totals":"{phase}"}}"#
            ));
            for (peer, language, value) in [("rustc", "rust", 100), ("cxx", "cxx", 101)] {
                for metric in ["latency_ns", "memory_bytes"] {
                    if !peers.is_empty() {
                        peers.push(',');
                    }
                    peers.push_str(&format!(
                        r#"{{"peer":"{peer}","language":"{language}","program":"fixture.jet","state":"{state}","metric":"{metric}","value":{value},"workload_sha256":"{workload_hash}","source_sha256":"{source_hash}","expected_sha256":"{expected_hash}","manifest_sha256":"{manifest_hash}","toolchain_sha256":"{toolchain_hash}"}}"#
                    ));
                }
            }
        }
        let corpus_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        format!(
            r#"{{"schema":"jet.compiler-speed","version":4,"corpus_sha256":"{corpus_hash}","manifest_sha256":"{manifest_hash}","stage":"matrix","peer_version":1,"peer_contract_sha256":"{peer_contract}","peer_run_id":"fixture-run","peer_keys":"rustc:rust,cxx:cxx","peer_metrics":"latency_ns,memory_bytes","peer_count":2,"peer_rows":24,"parity":{{"status":"verified","cases":1,"semantic":"verified","diagnostics":"verified","effects":"verified","tiers":"verified"}},"machine":{{"allocator_source_sha256":"{allocator_hash}","allocator_environment_sha256":"{allocator_environment_hash}","arch":"x86_64","compiler_sha256":"{compiler_hash}","cpus":2,"governor":"governor","hardware_sha256":"{hardware_hash}","hostname":"fixture","kernel":"kernel","libc_sha256":"{libc_hash}","load1_start_milli":100,"load1_peak_milli":200,"load1_end_milli":150,"memory_bytes":1024,"os":"Linux","rustc":"rustc","rustc_vv_sha256":"{rustc_vv_hash}","target":"target","toolchain_sha256":"{toolchain_hash}","topology_sha256":"{topology_hash}","jet_env_sha256":"{jet_env_hash}","llvm":"llvm","rustc_sha256":"{rustc_hash}"}},"budgets":{{"latency_regression_pct":15,"memory_regression_pct":15,"samples":20,"variance_pct":100,"warmups":1}},"outliers_discarded":0,"runs":[{runs}],"peers":[{peers}]}}
"#
        )
    }

    fn checker_report_from_baseline(baseline: &str) -> String {
        let root = parse(baseline).expect("generated compiler-speed baseline JSON");
        let machine = json_field(&root, "machine");
        let budgets = json_field(&root, "budgets");
        let parity = json_field(&root, "parity");
        let runs = json_field(&root, "runs")
            .as_array()
            .expect("compiler-speed baseline runs array");
        assert_eq!(
            runs.len() % 6,
            0,
            "baseline runs are not six rows per corpus row"
        );
        let machine_id = format!(
            "{}/{}/cpus={}/host={}",
            json_text(machine, "os"),
            json_text(machine, "arch"),
            json_number(machine, "cpus"),
            json_text(machine, "hostname")
        );
        let peer_contract = json_text(&root, "peer_contract_sha256");
        let peer_run_id = json_text(&root, "peer_run_id");
        let peer_keys = json_text(&root, "peer_keys");
        let peer_metrics = json_text(&root, "peer_metrics");
        let peer_count = json_number(&root, "peer_count");
        let peer_rows = json_number(&root, "peer_rows");
        let mut report = format!(
            "compiler-speed version={} corpus={} corpus_sha256={} manifest_sha256={} stage=matrix machine={} target={} rustc={} llvm={} rustc_vv_sha256={} rustc_sha256={} compiler_sha256={} jet_env_sha256={} libc_sha256={} allocator_sha256={} allocator_environment_sha256={} hardware_sha256={} topology_sha256={} toolchain_sha256={} kernel={} governor={} load1_start_milli={} load1_peak_milli={} load1_end_milli={} memory_bytes={} profiles=jit-fast,aot-release backends=cranelift,rustc-llvm warmups={} samples={} outliers_discarded={} parity={} parity_cases={} peer_contract_sha256={} peer_run_id={} peer_machine={} peer_target={} peer_keys={} peer_metrics={} peer_count={} peer_rows={}\n",
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
            peer_contract,
            peer_run_id,
            machine_id,
            json_text(machine, "target"),
            peer_keys,
            peer_metrics,
            peer_count,
            peer_rows,
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
        report.push_str(&format!(
            "compiler-speed-peer version=1 run_id={} corpus_sha256={} manifest_sha256={} target={} machine={} peers={} metrics={} contract_sha256={} peer_count={} rows={}\n",
            peer_run_id,
            json_text(&root, "corpus_sha256"),
            json_text(&root, "manifest_sha256"),
            json_text(machine, "target"),
            machine_id,
            peer_keys,
            peer_metrics,
            peer_contract,
            peer_count,
            peer_rows,
        ));
        report.push_str("peer\tlanguage\tprogram\tstate\tmetric\tvalue\tworkload_sha256\tsource_sha256\texpected_sha256\tmanifest_sha256\ttoolchain_sha256\n");
        for run in runs {
            for (peer, language, value) in [("rustc", "rust", 100), ("cxx", "cxx", 101)] {
                for metric in ["latency_ns", "memory_bytes"] {
                    report.push_str(&format!(
                        "{peer}\t{language}\t{}\t{}\t{metric}\t{value}\tffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\tbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\tbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\tdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\n",
                        json_text(run, "program"),
                        json_text(run, "state"),
                    ));
                }
            }
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
            .env(
                "JET_CI_CANDIDATE_COMMIT",
                "0123456789abcdef0123456789abcdef01234567",
            )
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
        let baseline = synthetic_v4_baseline();
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
        let baseline = synthetic_v4_baseline();
        let version = json_number(&parse(&baseline).unwrap(), "version");
        let bumped = baseline.replacen(
            &format!("\"version\":{version}"),
            &format!("\"version\":{}", version + 1),
            1,
        );
        let output = run_checker_fixture("compiler-speed-checker-version", &bumped, "");
        let text = output_text(&output);
        assert!(
            !output.status.success(),
            "unsupported version was accepted:\n{text}"
        );
        assert!(
            text.contains(&format!(
                "unsupported compiler-speed baseline version: {}",
                version + 1
            )),
            "version mismatch was not named:\n{text}"
        );
    }

    #[test]
    fn compiler_speed_checker_rejects_changed_machine_rustc_identity() {
        let baseline = synthetic_v4_baseline();
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
        assert!(
            !output.status.success(),
            "changed rustc identity was accepted:\n{text}"
        );
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


    #[test]
    fn compiler_speed_default_profile_routing_is_removal_sensitive_and_production_backed() {
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
                .env("JET_STORE_DIR", scratch.join("store"))
                .env("JET_STORE_CAP_BYTES", "21474836480")
                .env("JET_STORE_RESERVE_BYTES", "2147483648")
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
            .env("JET_STORE_DIR", scratch.join("store"))
            .env("JET_STORE_CAP_BYTES", "21474836480")
            .env("JET_STORE_RESERVE_BYTES", "2147483648")
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
                .env("JET_STORE_DIR", scratch.path.join(format!("{tag}-store")))
                .env("JET_STORE_CAP_BYTES", "21474836480")
                .env("JET_STORE_RESERVE_BYTES", "2147483648")
                .env(
                    "JET_RUN_CACHE_DIR",
                    scratch.path.join(format!("{tag}-run-cache")),
                )
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
        assert_ne!(
            unknown.status.code(),
            Some(0),
            "unknown dev job was accepted"
        );
        assert!(
            stderr.contains("E1294"),
            "unknown dev job lost E1294:\n{stderr}"
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
    fn metadata_value(args: &[String]) -> &str {
        args.windows(2)
            .find_map(|window| {
                (window[0] == "-C")
                    .then(|| window[1].strip_prefix("metadata="))
                    .flatten()
            })
            .expect("rustc invocation has content metadata")
    }

    fn assert_content_metadata_and_remap(args: &[String]) {
        let metadata = metadata_value(args);
        assert!(
            metadata.len() == 64
                && metadata
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "rustc metadata is not a lowercase SHA-256 key: {metadata:?}; args={args:?}"
        );
        assert!(
            args.windows(2).any(|window| {
                window[0] == "--remap-path-prefix" && window[1].ends_with("=/jet/build")
            }),
            "rustc invocation did not remap its build workdir: {args:?}"
        );
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
    fn cache_key(log: &str) -> String {
        log.lines()
            .flat_map(str::split_whitespace)
            .find_map(|part| {
                let value = part.strip_prefix("key=")?;
                (value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
                .then(|| value.to_owned())
            })
            .expect("native cache log has a build key")
    }

    fn has_corelib_identity(line: &str) -> bool {
        line.contains("corelib=/* jet-corelib-r10 ")
            && ["source=", "closure=", "fp="]
                .iter()
                .all(|field| has_digest_field(line, field))
    }

    fn build(scratch: &Scratch) -> std::process::Output {
        Command::new(jet())
            .args(["build", "main.jet", "--profile=debug", "--verbose"])
            .current_dir(&scratch.path)
            .env("JET_STORE_DIR", scratch.join("store"))
            .env("JET_STORE_CAP_BYTES", "21474836480")
            .env("JET_STORE_RESERVE_BYTES", "2147483648")
            .env("JET_RUNTIME_CACHE_STATS", "1")
            .env(
                "JET_DEBUG_NATIVE_CACHE_LOG",
                scratch.join("native-cache.log"),
            )
            .env("JET_RECEIPT_BYPASS", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }
    fn release_build(scratch: &Scratch) -> std::process::Output {
        Command::new(jet())
            .args(["build", "main.jet", "--profile=release", "--verbose"])
            .current_dir(&scratch.path)
            .env("JET_STORE_DIR", scratch.join("store"))
            .env("JET_STORE_CAP_BYTES", "21474836480")
            .env("JET_STORE_RESERVE_BYTES", "2147483648")
            .env("JET_RECEIPT_BYPASS", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    #[test]
    fn production_build_follows_compiler_speed_plan_flags_and_linker() {
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
            .env("JET_STORE_DIR", scratch.join("store"))
            .env("JET_STORE_CAP_BYTES", "21474836480")
            .env("JET_STORE_RESERVE_BYTES", "2147483648")
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

        let recorded = invocations(&log);
        let final_args = recorded
            .iter()
            .find(|args| has_pair(args, "--crate-name", "main"))
            .expect("recorded final rustc invocation");
        assert_content_metadata_and_remap(final_args);
        let runtime_args = recorded
            .iter()
            .filter(|args| {
                has_pair(args, "--crate-name", "jet_runtime")
                    || has_pair(args, "--crate-name", "jet_runtime_core")
            })
            .collect::<Vec<_>>();
        assert!(
            !runtime_args.is_empty(),
            "recorded runtime rlib rustc invocation"
        );
        for args in runtime_args {
            assert_content_metadata_and_remap(args);
        }
        for flag in ["codegen-units=256", "opt-level=0", "lto=off", "debuginfo=2"] {
            assert!(
                final_args.iter().any(|arg| arg == flag),
                "final rustc invocation omitted {flag}: {final_args:?}"
            );
        }
        assert!(
            has_pair(
                final_args,
                "-C",
                &format!("linker={}", real_linker.display())
            ),
            "final rustc invocation omitted explicit linker: {final_args:?}"
        );
    }

    #[test]
    fn production_build_is_reproducible_across_checkout_paths() {
        let left = Scratch::new("compiler-speed-repro-left");
        let right = Scratch::new("compiler-speed-repro-right");
        let package =
            "name: \"deterministic_build\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO] } }\n";
        let source = "fn run() { print(\"deterministic-build\") }\n";
        for scratch in [&left, &right] {
            fs::create_dir_all(scratch.join("src")).unwrap();
            fs::write(scratch.join("package.jet"), package).unwrap();
            fs::write(scratch.join("src/main.jet"), source).unwrap();
        }
        let build = |scratch: &Scratch| {
            Command::new(jet())
                .args(["run", "--release", "src/main.jet"])
                .current_dir(&scratch.path)
                .env("JET_STORE_DIR", scratch.join("store"))
                .env("JET_RECEIPT_BYPASS", "1")
                .env("NO_COLOR", "1")
                .output()
                .unwrap()
        };
        for (name, output) in [("left", build(&left)), ("right", build(&right))] {
            assert_eq!(
                output.status.code(),
                Some(0),
                "{name} reproducibility build failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let left_rust = fs::read(left.join("build/main.rs")).unwrap();
        let right_rust = fs::read(right.join("build/main.rs")).unwrap();
        assert_eq!(
            jet::SHA256::sha256_hex(&left_rust),
            jet::SHA256::sha256_hex(&right_rust),
            "generated Rust SHA-256 changed with checkout path"
        );
        assert_eq!(
            left_rust, right_rust,
            "generated Rust changed with checkout path"
        );

        let left_binary = fs::read(left.join("build/main")).unwrap();
        let right_binary = fs::read(right.join("build/main")).unwrap();
        assert_eq!(
            jet::SHA256::sha256_hex(&left_binary),
            jet::SHA256::sha256_hex(&right_binary),
            "native binary SHA-256 changed with checkout path"
        );
        assert_eq!(
            left_binary, right_binary,
            "native binary changed with checkout path"
        );
    }

    #[test]
    fn explicit_project_link_build_is_reproducible_and_content_sensitive() {
        let left = Scratch::new("compiler-speed-c-link-left");
        let right = Scratch::new("compiler-speed-c-link-right");
        let package =
            "name: \"deterministic_c_build\"\nversion: \"0.1.0\"\ndeps: { answer: c@\"./native\" }\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n";
        let source = "use c.answer as answer\n#Import module c.answer { fn value() I32 = \"answer_value\" }\nfn run() { print(\"{answer.value()}\") }\n";
        for scratch in [&left, &right] {
            fs::create_dir_all(scratch.join("native")).unwrap();
            fs::create_dir_all(scratch.join("src")).unwrap();
            fs::write(scratch.join("package.jet"), package).unwrap();
            fs::write(scratch.join("src/main.jet"), source).unwrap();
        }
        let build_archive = |scratch: &Scratch, value: i32| {
            fs::write(
                scratch.join("native/answer.c"),
                format!("int answer_value(void) {{ return {value}; }}\n"),
            )
            .unwrap();
            let object = scratch.join("native/answer.o");
            let compile = Command::new(path_program("cc"))
                .args([
                    "-c",
                    scratch.join("native/answer.c").to_str().unwrap(),
                    "-o",
                    object.to_str().unwrap(),
                ])
                .output()
                .unwrap();
            assert!(
                compile.status.success(),
                "C archive source failed: {}",
                String::from_utf8_lossy(&compile.stderr)
            );
            let archive = Command::new(path_program("ar"))
                .args(["rcs", "native/libanswer.a", "native/answer.o"])
                .current_dir(&scratch.path)
                .output()
                .unwrap();
            assert!(
                archive.status.success(),
                "C archive creation failed: {}",
                String::from_utf8_lossy(&archive.stderr)
            );
        };
        build_archive(&left, 42);
        build_archive(&right, 42);

        let tools = left.join("tools");
        fs::create_dir_all(&tools).unwrap();
        let real_rustc = path_program("rustc");
        let left_log = left.join("rustc.log");
        write_executable(
            &tools.join("rustc"),
            "#!/bin/sh\n\
             { printf '%s\\n' BEGIN; printf '%s\\n' \"$@\"; printf '%s\\n' END; } >> \"$JET_TEST_RUSTC_LOG\"\n\
             exec \"$JET_TEST_REAL_RUSTC\" \"$@\"\n",
        );
        let run = |scratch: &Scratch, log: &Path| {
            Command::new(jet())
                .args(["run", "--release", "src/main.jet"])
                .current_dir(&scratch.path)
                .env("PATH", prepend_path(&tools))
                .env("JET_TEST_REAL_RUSTC", &real_rustc)
                .env("JET_TEST_RUSTC_LOG", log)
                .env("JET_STORE_DIR", scratch.join("store"))
                .env("JET_STORE_CAP_BYTES", "21474836480")
                .env("JET_STORE_RESERVE_BYTES", "2147483648")
                .env("JET_RECEIPT_BYPASS", "1")
                .env("NO_COLOR", "1")
                .output()
                .unwrap()
        };
        let left_run = run(&left, &left_log);
        assert_eq!(
            left_run.status.code(),
            Some(0),
            "left explicit-link run failed:\n{}",
            String::from_utf8_lossy(&left_run.stderr)
        );
        assert_eq!(left_run.stdout, b"42\n");
        let right_log = right.join("rustc.log");
        let right_run = run(&right, &right_log);
        assert_eq!(
            right_run.status.code(),
            Some(0),
            "right explicit-link run failed:\n{}",
            String::from_utf8_lossy(&right_run.stderr)
        );
        assert_eq!(right_run.stdout, b"42\n");

        let final_args = |log: &Path| {
            let log = fs::read_to_string(log).unwrap();
            invocations(&log)
                .into_iter()
                .find(|args| has_pair(args, "--crate-name", "main"))
                .expect("recorded explicit-link final rustc invocation")
        };
        let left_metadata = {
            let args = final_args(&left_log);
            assert_content_metadata_and_remap(&args);
            metadata_value(&args).to_string()
        };
        let right_metadata = {
            let args = final_args(&right_log);
            assert_content_metadata_and_remap(&args);
            metadata_value(&args).to_string()
        };
        assert_eq!(
            left_metadata, right_metadata,
            "project-local C link metadata changed with checkout path"
        );

        build_archive(&left, 43);
        let changed_log = left.join("rustc-changed.log");
        let changed_run = run(&left, &changed_log);
        assert_eq!(
            changed_run.status.code(),
            Some(0),
            "changed explicit-link run failed:\n{}",
            String::from_utf8_lossy(&changed_run.stderr)
        );
        assert_eq!(changed_run.stdout, b"43\n");
        let changed_args = final_args(&changed_log);
        assert_content_metadata_and_remap(&changed_args);
        assert_ne!(
            left_metadata,
            metadata_value(&changed_args),
            "changed linked archive did not change rustc metadata"
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
            .env("JET_STORE_DIR", scratch.join("store"))
            .env("JET_STORE_CAP_BYTES", "21474836480")
            .env("JET_STORE_RESERVE_BYTES", "2147483648")
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
            "name: \"compiler_speed\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO] } }\n",
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
            String::from_utf8_lossy(&cold.stderr).contains("[build] runtime   ->"),
            "cold build did not report its runtime-store decision:\n{}",
            String::from_utf8_lossy(&cold.stderr)
        );

        let cache_log = fs::read_to_string(scratch.join("native-cache.log")).unwrap();
        assert!(
            cache_log.lines().any(|line| {
                has_digest_field(line, "runtime=")
                    && has_corelib_identity(line)
                    && has_digest_field(line, "key=")
            }),
            "native cache log did not expose relevant runtime/Core digests:\n{cache_log}"
        );
        let cache_key = cache_key(&cache_log);
        let store = jet_store::Store::new(scratch.join("store")).unwrap();
        let initial_status = store.status().unwrap();
        assert!(
            initial_status
                .entries
                .iter()
                .any(|entry| entry.kind == jet_store::EntryKind::Blob),
            "cold build did not publish a blob to the machine-wide store"
        );
        assert!(
            initial_status
                .entries
                .iter()
                .any(|entry| entry.kind == jet_store::EntryKind::Action),
            "cold build did not publish an action record to the machine-wide store"
        );
        let binary_digest = jet::SHA256::sha256_hex(&fs::read(scratch.join("build/main")).unwrap());
        let cached_bin = initial_status
            .entries
            .iter()
            .find(|entry| entry.kind == jet_store::EntryKind::Blob && entry.key == binary_digest)
            .map(|entry| entry.path.clone())
            .expect("cold build published a final binary blob");

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
            String::from_utf8_lossy(&final_repaired.stderr).contains("cache store -> saved binary"),
            "final cache repair did not republish the binary:\n{}",
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
        assert!(
            matches!(
                store.lookup_artifact(&cache_key).unwrap(),
                jet_store::ArtifactLookup::Hit(_)
            ),
            "repaired final binary was not readable from the machine-wide store"
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
            store
                .status()
                .unwrap()
                .entries
                .iter()
                .filter(|entry| entry.kind == jet_store::EntryKind::Blob)
                .count()
                >= 2,
            "changed build did not retain distinct binary blobs in the machine-wide store"
        );

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
            String::from_utf8_lossy(&repaired.stderr).contains("cache store -> saved binary"),
            "changed cache object was not stored visibly:\n{}",
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
            cold_stderr
                .matches("[build] cache miss -> compiling")
                .count(),
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

    /// D-DEVR-TWICE1=A: the second identical `jet build` replays its receipt
    /// instead of compiling again. Each invocation runs under its own Nix
    /// shell temp root (`nix develop` mints a fresh `/tmp/nix-shell.*` per
    /// shell), so the receipt context must not fingerprint that plumbing.
    #[test]
    fn second_identical_build_replays_receipt() {
        let scratch = Scratch::new("compiler-speed-receipt-replay");
        fs::write(
            scratch.join("main.jet"),
            "fn run() { print(\"receipt-replay\") }\n",
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
        let contexts = scratch.join(".jet/receipts/contexts");
        let context_pointers = || -> Vec<(PathBuf, String)> {
            let mut pointers = fs::read_dir(&contexts)
                .map(|entries| {
                    entries
                        .map(|entry| {
                            let path = entry.unwrap().path();
                            let key = fs::read_to_string(&path).unwrap();
                            (path, key)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            pointers.sort();
            pointers
        };

        let build = |shell_temp: &Path| -> std::process::Output {
            fs::create_dir_all(shell_temp).unwrap();
            Command::new(jet())
                .args(["build", "main.jet", "--profile=debug"])
                .current_dir(&scratch.path)
                .env("PATH", prepend_path(&tools))
                .env("RUSTC_LINKER", &real_linker)
                .env("JET_TEST_REAL_RUSTC", &real_rustc)
                .env("JET_TEST_RUSTC_LOG", &rustc_log)
                .env("JET_STORE_DIR", scratch.join("store"))
                .env("JET_STORE_CAP_BYTES", "21474836480")
                .env("JET_STORE_RESERVE_BYTES", "2147483648")
                .env("NIX_BUILD_TOP", shell_temp)
                .env("TMPDIR", shell_temp)
                .env("TMP", shell_temp)
                .env("TEMP", shell_temp)
                .env("TEMPDIR", shell_temp)
                .env("NO_COLOR", "1")
                .output()
                .unwrap()
        };

        let first = build(&scratch.join("nix-shell.first"));
        let first_stderr = String::from_utf8_lossy(&first.stderr);
        assert_eq!(
            first.status.code(),
            Some(0),
            "first debug build failed:\n{first_stderr}"
        );
        assert!(
            !first_stderr.contains("ok: build current"),
            "first build must do the work, not replay a receipt:\n{first_stderr}"
        );
        let first_log = fs::read_to_string(&rustc_log).unwrap();
        assert!(
            invocations(&first_log)
                .iter()
                .any(|args| has_pair(args, "--crate-name", "main")),
            "first build must run rustc for the program:\n{first_log}"
        );
        let binary = scratch.join("build/main");
        let first_binary = fs::read(&binary).unwrap();
        let first_modified = fs::metadata(&binary).unwrap().modified().unwrap();
        let first_pointers = context_pointers();
        assert_eq!(
            first_pointers.len(),
            1,
            "first build must publish exactly one receipt context: {first_pointers:?}"
        );
        let receipt_key = first_pointers[0].1.clone();
        assert_eq!(
            receipt_key.len(),
            64,
            "receipt context pointer must hold one receipt key: {receipt_key:?}"
        );

        let second = build(&scratch.join("nix-shell.second"));
        let second_stderr = String::from_utf8_lossy(&second.stderr);
        assert_eq!(
            second.status.code(),
            Some(0),
            "second debug build failed:\n{second_stderr}"
        );
        let expected = format!("ok: build current (receipt {})", &receipt_key[..12]);
        assert!(
            second_stderr.contains(&expected),
            "second identical build must replay receipt `{expected}`:\n{second_stderr}"
        );
        assert!(
            !second_stderr.contains("receipt: build invalidated"),
            "identical inputs must not invalidate the receipt:\n{second_stderr}"
        );
        assert_eq!(
            second.stdout, first.stdout,
            "replayed build must reproduce the recorded stdout"
        );
        assert_eq!(
            context_pointers(),
            first_pointers,
            "two identical builds under different shell temp roots must share one context digest"
        );
        assert_eq!(
            fs::read_to_string(&rustc_log).unwrap(),
            first_log,
            "a replayed build must not run rustc or the linker"
        );
        assert_eq!(
            fs::metadata(&binary).unwrap().modified().unwrap(),
            first_modified,
            "a replayed build must not rewrite the binary"
        );
        assert_eq!(
            fs::read(&binary).unwrap(),
            first_binary,
            "a replayed build must leave the first binary in place"
        );
    }
    fn cache_cli(scratch: &Scratch, args: &[&str]) -> std::process::Output {
        Command::new(jet())
            .args(args)
            .current_dir(&scratch.path)
            .env("JET_STORE_DIR", scratch.join("store"))
            .env("JET_STORE_CAP_BYTES", "21474836480")
            .env("JET_STORE_RESERVE_BYTES", "2147483648")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    #[test]
    fn cache_cli_status_prune_limit_and_doctor_snapshots() {
        let scratch = Scratch::new("compiler-speed-store-cli");
        let store_root = scratch.join("store");
        let store = jet_store::Store::new(&store_root).unwrap();
        store.publish_blob(&vec![0u8; 2048]).unwrap();

        let status = cache_cli(&scratch, &["cache", "status"]);
        assert_eq!(
            status.status.code(),
            Some(0),
            "cache status failed:\n{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert_eq!(
            String::from_utf8(status.stdout).unwrap(),
            format!(
                "store      {}\n\
                 limit      20 GiB (JET_STORE_CAP_BYTES)\n\
                 reserve    keep 2 GiB free on that filesystem\n\
                 used       2 KiB · 1 blobs · 0 records · 0 ThinLTO caches\n\
                 leases     0 live\n\
                 tiers      local\n",
                store_root.display()
            )
        );

        let prune = cache_cli(&scratch, &["cache", "prune", "--to", "1K"]);
        assert_eq!(
            prune.status.code(),
            Some(0),
            "cache prune failed:\n{}",
            String::from_utf8_lossy(&prune.stderr)
        );
        assert_eq!(
            String::from_utf8(prune.stdout).unwrap(),
            "prune      2 KiB -> 0 B (target 1 KiB)\n\
             removed    1 entries · freed 2 KiB\n\
             pinned     0 B\n"
        );

        let limit = cache_cli(&scratch, &["cache", "limit", "--host", "4G"]);
        assert_eq!(
            limit.status.code(),
            Some(0),
            "cache limit failed:\n{}",
            String::from_utf8_lossy(&limit.stderr)
        );
        assert_eq!(
            String::from_utf8(limit.stdout).unwrap(),
            format!(
                "host limit 4 GiB (persisted)\nstore      {}\n",
                store_root.display()
            )
        );

        let limited_status = cache_cli(&scratch, &["cache", "status"]);
        assert_eq!(
            String::from_utf8(limited_status.stdout).unwrap(),
            format!(
                "store      {}\n\
                 limit      4 GiB (host policy)\n\
                 reserve    keep 2 GiB free on that filesystem\n\
                 used       0 B · 0 blobs · 0 records · 0 ThinLTO caches\n\
                 leases     0 live\n\
                 tiers      local\n",
                store_root.display()
            )
        );

        let doctor = cache_cli(&scratch, &["self", "doctor"]);
        let doctor_stdout = String::from_utf8_lossy(&doctor.stdout);
        assert!(
            doctor_stdout.contains(&format!(
                "artifact store: {} (footprint 0 B; cap 4 GiB; reserve 2 GiB; 0 entries; 0 live leases)",
                store_root.display()
            )),
            "doctor lost the artifact-store footprint row:\n{doctor_stdout}"
        );
    }
}
