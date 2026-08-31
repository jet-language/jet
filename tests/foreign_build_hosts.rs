//! #1347: foreign build hosts keep one checked Jet Library contract.
//!
//! The real host commands are listed in the reference proof matrix. This
//! target stays runnable on a normal Jet checkout: it checks that the adapter
//! files, input declarations, fixture layout, and maintained-line promise are
//! present without treating an absent CMake/Gradle/Bazel/MSBuild installation
//! as a successful integration.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn maintained_lines(path: &Path) -> usize {
    read(path)
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("//")
                && !line.starts_with("<!--")
        })
        .count()
}

#[test]
fn adapters_use_one_jet_library_runner_and_fail_closed_outputs() {
    let root = repo_root().join("tools/foreign-build-hosts");
    let runner = read(&root.join("jet-library.sh"));
    let powershell = read(&root.join("jet-library.ps1"));
    for text in [&runner, &powershell] {
        assert!(text.contains("build"), "runner does not invoke Jet build");
        assert!(text.contains("--lib"), "runner does not select Library export");
        assert!(text.contains("--locked"), "runner does not pin lock provenance");
        assert!(text.contains("--output"), "runner does not select manifest output");
        assert!(!text.contains("fetch --locked"), "runner substitutes a fetch preflight for the locked build");
        assert!(text.contains("package.jet"), "runner does not declare the manifest");
        assert!(text.contains(".jet/lock"), "runner does not declare the lock");
        assert!(text.contains("jet-host.receipt"), "runner does not publish provenance");
        assert!(text.contains("jet-host.stamp"), "runner does not publish completion");
        assert!(
            text.contains("\"schema\":2") || text.contains("schema = 2"),
            "runner does not version its structured receipt"
        );
        assert!(text.contains("sha256"), "runner does not record content digests");
        assert!(text.contains("jet-library.complete"), "runner does not require the complete Jet artifact set");
        assert!(text.contains("timed out"), "runner lock wait is unbounded");
        assert!(text.contains("kill") || text.contains("Stop-Process"), "runner does not clean up cancellation");
        assert!(text.contains("JET-HOST-TOOL"), "runner lacks tool diagnostics");
        assert!(text.contains("JET-HOST-INPUT"), "runner lacks input diagnostics");
        assert!(text.contains("JET-HOST-ABI"), "runner lacks ABI diagnostics");
        assert!(text.contains("cl.exe"), "runner does not reject the MSVC ABI");
    }
    assert!(runner.contains("${CC:-cc}"), "POSIX runner has no host-linker fallback");
    assert!(powershell.contains("taskkill.exe"), "Windows runner does not kill build descendants");
    assert!(powershell.contains("project parts"), "Windows runner does not derive the source closure");
    assert!(powershell.contains("TimeoutSeconds"), "Windows runner has no bounded build timeout");
    for variable in ["RUSTC", "RUSTC_LINKER", "CC", "NO_COLOR", "PATH"] {
        assert!(powershell.contains(&format!("EnvironmentVariables[\"{variable}\"]")), "Windows runner does not pass {variable}");
    }

    let adapters = [
        root.join("cmake/Jet.cmake"),
        root.join("gradle/jet-library.gradle"),
        root.join("bazel/jet_library.bzl"),
        root.join("msbuild/Jet.Library.targets"),
    ];
    for path in adapters {
        let text = read(&path);
        assert!(text.contains("jet-library"), "{} bypasses the shared runner", path.display());
        assert!(!text.contains("rustc"), "{} owns Jet code generation", path.display());
        assert!(!text.contains("cargo"), "{} owns a duplicate build engine", path.display());
    }
}

#[test]
fn representative_projects_declare_exact_inputs_and_stay_under_ten_lines() {
    let root = repo_root().join("tests/fixtures/foreign_build_hosts");
    let projects = [
        ("cmake", "CMakeLists.txt", ["package.jet", ".jet/lock", "library.jet", "extra.jet"]),
        ("gradle", "build.gradle", ["package.jet", ".jet/lock", "library.jet", "extra.jet"]),
        ("bazel", "BUILD.bazel", ["package.jet", ".jet/lock", "library.jet", "extra.jet"]),
        ("msbuild", "host.proj", ["package.jet", ".jet/lock", "library.jet", "extra.jet"]),
    ];
    for (host, build_file, inputs) in projects {
        let project = root.join(host);
        let build = read(&project.join(build_file));
        assert!(maintained_lines(&project.join(build_file)) < 10, "{host} build grew past ten maintained lines");
        for input in inputs {
            assert!(project.join(input).is_file(), "{host} is missing {input}");
            let named = build.contains(input)
                || (input == ".jet/lock" && build.contains(".jet\\lock"));
            assert!(named, "{host} build does not name {input}");
        }
        assert!(project.join("package.jet").is_file());
        assert!(project.join(".jet/lock").is_file());
        assert!(read(&project.join("package.jet")).contains(".Library"));
        assert!(read(&project.join(".jet/lock")).contains("[build.stamp]"));
        assert!(build.contains("loadable"), "{host} does not name the native artifact");
        let host_source = if host == "msbuild" { "host.cpp" } else { "host.c" };
        assert!(read(&project.join(host_source)).contains("on_tick(41) == 42"));
    }
}

#[test]
fn host_descriptors_are_small_and_route_to_native_library_targets() {
    let root = repo_root().join("tests/fixtures/foreign_build_hosts");
    let cmake = read(&root.join("cmake/CMakeLists.txt"));
    assert!(cmake.contains("find_package(Jet REQUIRED)"));
    assert!(!cmake.contains("CMAKE_MODULE_PATH"));
    assert!(cmake.contains("jet_library"));
    assert!(cmake.contains("target_link_libraries"));
    let cmake_adapter = read(&repo_root().join("tools/foreign-build-hosts/cmake/Jet.cmake"));
    assert!(cmake_adapter.contains("set(_kind static)"));
    assert!(cmake_adapter.contains("CMAKE_CXX_COMPILER"));
    assert!(cmake_adapter.contains("add_custom_target(\"${TARGET}_jet\" DEPENDS ${_outputs})"));

        let gradle = read(&root.join("gradle/build.gradle"));
        assert!(gradle.contains("jet-library.gradle"));
        assert!(gradle.contains("dependsOn \"jetLibrary\""));
        assert!(gradle.contains("jetLibrary.staticLibrary"));
        assert!(gradle.contains("inputs.files jetLibrary.staticLibrary()"));

    let bazel = read(&root.join("bazel/BUILD.bazel"));
    assert!(bazel.contains("jet_library"));
    assert!(bazel.contains("cc_binary"));
    assert!(bazel.contains("linkopts"));
    let bazel_adapter = read(&repo_root().join("tools/foreign-build-hosts/bazel/jet_library.bzl"));
    assert!(bazel_adapter.contains("ctx.actions.run"));
    assert!(bazel_adapter.contains("args.add"));
    assert!(bazel_adapter.contains("short_path"));
    assert!(!bazel_adapter.contains("$(location"));

    let msbuild = read(&root.join("msbuild/host.proj"));
    assert!(msbuild.contains("Jet.Library.targets"));
    assert!(msbuild.contains("DependsOnTargets=\"JetLibrary\""));
    assert!(msbuild.contains("JetLibraryStatic"));
    assert!(msbuild.contains("Inputs=\"$(MSBuildProjectDirectory)\\host.cpp"));
    assert!(msbuild.contains("Outputs=\"$(IntermediateOutputPath)host.exe"));
    let msbuild_adapter = read(&repo_root().join("tools/foreign-build-hosts/msbuild/Jet.Library.targets"));
    assert!(msbuild_adapter.contains("$(MSBuildProjectDirectory)\\"));
    assert!(msbuild_adapter.contains(".jet\\lock"));
    assert!(msbuild_adapter.contains("$(MSBuildThisFileDirectory)..\\jet-library.ps1"));
    assert!(msbuild_adapter.contains("$(JetLibraryIntermediate)\\jet-host.stamp"));

    let lifecycle = root.join("msbuild/lifecycle.cpp");
    assert!(lifecycle.is_file());
    let lifecycle_text = read(&lifecycle);
    assert!(lifecycle_text.contains("#ifdef _WIN32"));
    assert!(lifecycle_text.contains("LoadLibraryA"));
    assert!(lifecycle_text.contains("std::thread"));

    let docs = read(&repo_root().join("docs/reference/foreign-build-hosts.md"));
    for term in [
        "CMake",
        "Gradle",
        "Bazel",
        "MSBuild",
        "--locked",
        "jet-host.receipt",
        "jet-host.stamp",
        "jet-library-set-v1",
        "lifecycle.cpp",
        "CMAKE_TOOLCHAIN_FILE",
    ] {
        assert!(docs.contains(term), "reference docs omit {term}");
    }
}

#[cfg(unix)]
#[test]
fn shared_runner_publishes_only_after_a_successful_jet_export() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = common::Scratch::new("foreign-host-runner");
    fs::create_dir_all(scratch.path.join(".jet")).unwrap();
    fs::write(scratch.path.join("package.jet"), "name: \"runner\"\nversion: \"0.1.0\"\n").unwrap();
    fs::write(
        scratch.path.join(".jet/lock"),
        "version = 1\n\n[build.stamp]\ngit = \"runner\"\ndirty = false\ntoolchain = \"1.0.0\"\nat = \"2026-08-29T00:00:00.000000000Z\"\n\n[root]\ndependencies = []\n",
    )
    .unwrap();
    fs::write(scratch.path.join("library.jet"), "pub fn on_tick(dt: Int) Int -> dt + 1\n").unwrap();
    let fake_jet = scratch.path.join("fake-jet");
    fs::write(
        &fake_jet,
        "#!/bin/sh\nif [ \"$1\" = project ] && [ \"$2\" = parts ]; then printf 'automatic __project.library library.jet\\n'; exit 0; fi\nif [ \"$1\" = build ] && [ \"${FAKE_JET_FAIL:-0}\" = 1 ]; then exit 17; fi\nif [ \"$1\" = build ]; then mkdir -p target; printf '%s\\n' \"$@\" > target/args; printf archive > target/libloadable.a; printf header > target/loadable.h; printf 'jet-jetlib-v3\\0' > target/loadable.jetlib; a=$(sha256sum target/libloadable.a | cut -d' ' -f1); h=$(sha256sum target/loadable.h | cut -d' ' -f1); j=$(sha256sum target/loadable.jetlib | cut -d' ' -f1); printf 'jet-library-set-v1\\nlibloadable.a\\tsha256-%s\\nloadable.h\\tsha256-%s\\nloadable.jetlib\\tsha256-%s\\n' \"$a\" \"$h\" \"$j\" > target/.loadable.jet-library.complete; fi\n",
    )
    .unwrap();
    fs::set_permissions(&fake_jet, fs::Permissions::from_mode(0o755)).unwrap();

    let destination = scratch.path.join("host-build");
    let runner = repo_root().join("tools/foreign-build-hosts/jet-library.sh");
    let args = [
        "--jet",
        fake_jet.to_str().unwrap(),
        "--project",
        scratch.path.to_str().unwrap(),
        "--entry",
        "library.jet",
        "--output",
        "core",
        "--library",
        "loadable",
        "--dest",
        destination.to_str().unwrap(),
        "--kind",
        "static",
        "--loadable",
        "--input",
        "library.jet",
    ];
    let first = Command::new("bash").arg(&runner).args(args).status().unwrap();
    assert!(first.success(), "the fake Jet Library export should publish");
    assert!(destination.join("libloadable.a").is_file());
    assert!(destination.join("loadable.h").is_file());
    assert!(destination.join("loadable.jetlib").is_file());
    assert!(destination.join("jet-host.receipt").is_file());
    assert!(destination.join("jet-host.stamp").is_file());
    let receipt = read(&destination.join("jet-host.receipt"));
    assert!(receipt.contains("\"schema\":2"));
    assert!(receipt.contains("\"output\":\"core\""));
    assert!(receipt.contains("\"lock\":{\"path\":\".jet/lock\",\"digest\":\"sha256-"));
    assert!(receipt.contains("\"command\":[\"build\",\"--lib\",\"--locked\",\"--output\",\"core\",\"--profile=dev\",\"library.jet\"]"));
    assert!(scratch.path.join("target/.loadable.jet-library.complete").is_file());
    let invoked = read(&scratch.path.join("target/args"));
    assert!(invoked.lines().any(|line| line == "--lib"));
    assert!(invoked.lines().any(|line| line == "--locked"));
    assert!(invoked.lines().any(|line| line == "--output"));
    assert!(invoked.lines().any(|line| line == "core"));
    assert!(invoked.lines().any(|line| line == "library.jet"));

    let failed = Command::new("bash")
        .arg(&runner)
        .args(args)
        .env("FAKE_JET_FAIL", "1")
        .status()
        .unwrap();
    assert!(!failed.success(), "a failed Jet export must fail the host action");
    assert!(!destination.join("libloadable.a").exists());
    assert!(!destination.join("loadable.h").exists());
    assert!(!destination.join("loadable.jetlib").exists());
    assert!(!destination.join("jet-host.receipt").exists());
    assert!(!destination.join("jet-host.stamp").exists());
    assert!(!fs::read_dir(&destination)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".jet-host-stage.")));
}
