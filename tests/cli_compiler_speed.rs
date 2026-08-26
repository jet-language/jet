mod common;

#[cfg(unix)]
mod production_path {
    use super::common::Scratch;
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const COMPILER_SPEED_PLAN: &str = include_str!("../docs/plans/compiler-speed.md");

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

    fn prepend_path(directory: &Path) -> std::ffi::OsString {
        let mut paths = vec![directory.to_path_buf()];
        let path = env::var_os("PATH").unwrap_or_default();
        paths.extend(env::split_paths(&path));
        env::join_paths(paths).unwrap()
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

    fn build(scratch: &Scratch) -> std::process::Output {
        Command::new(jet())
            .args(["build", "main.jet", "--profile=debug", "--verbose"])
            .current_dir(&scratch.path)
            .env("JET_CACHE_DIR", scratch.join("build-cache"))
            .env("JET_RUNTIME_CACHE_DIR", scratch.join("runtime-cache"))
            .env("JET_RUNTIME_CACHE_STATS", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    #[test]
    fn production_build_follows_compiler_speed_plan_flags_and_linker() {
        for requirement in [
            "Fast linker (mold → lld → system), tuned rustc flags.",
            "Native rustc builds honor explicit `RUSTC_LINKER`/`CC`",
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
}
