mod common;

#[cfg(unix)]
mod production_path {
    use super::common::Scratch;
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

    #[test]
    fn production_build_uses_clean_rustc_flags_and_explicit_linker() {
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
}
