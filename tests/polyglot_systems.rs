mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Output;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn tool(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .and_then(|path| fs::canonicalize(path).ok())
}

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cpp_clang_ast_binder_links_external_namespace_and_on_demand_surface() {
    let Some(clang) = tool("clang++") else { return };
    let Some(ar) = tool("ar") else { return };
    let Some(_nm) = tool("nm") else { return };
    let Some(rustc) = tool("rustc") else { return };
    let target = env!("JET_BUILD_TARGET");
    let root = std::env::temp_dir().join(format!("jet_cpp_systems_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".jet/bindings/cpp")).unwrap();

    let header = root.join("counter.hpp");
    fs::write(
        &header,
        r#"#pragma once
#include <cstdint>

int64_t decoy(int64_t value);

namespace acme {
class Counter {
public:
    explicit Counter(int64_t start);
    int64_t add(int64_t amount);
    int64_t add(double factor);
    int64_t operator+(int64_t amount);
    int64_t fail_if_negative(int64_t value);
private:
    int64_t value;
};

int64_t apply(int64_t (*callback)(int64_t), int64_t value);
template <typename T> T twice(T value) { return value + value; }
}
"#,
    )
    .unwrap();
    let implementation = root.join("counter.cpp");
    fs::write(
        &implementation,
        r#"#include "counter.hpp"

int64_t decoy(int64_t value) { return value - 1000; }

namespace acme {
Counter::Counter(int64_t start) : value(start) {}
int64_t Counter::add(int64_t amount) { value += amount; return value; }
int64_t Counter::add(double factor) { value += static_cast<int64_t>(factor); return value; }
int64_t Counter::operator+(int64_t amount) { return value + amount; }
int64_t Counter::fail_if_negative(int64_t input) { if (input < 0) throw 1; return input; }
int64_t apply(int64_t (*callback)(int64_t), int64_t input) {
    auto result = callback(input);
    if (result < 0) throw 2;
    return result;
}
}
"#,
    )
    .unwrap();
    let object = root.join("counter.o");
    let library = root.join("libcounter_impl.a");
    run(Command::new(&clang)
        .args(["-std=c++17", "-fPIC", "-c"])
        .arg(&implementation)
        .arg("-target")
        .arg(target)
        .arg("-o")
        .arg(&object));
    run(Command::new(&ar).arg("rcs").arg(&library).arg(&object));

    let cache = root.join(".jet/bindings/cpp");
    let options = jet::CppBind::BindOptions {
        lib: "counter".into(),
        target: target.into(),
        clang: clang.clone(),
        archiver: ar,
        include_dirs: vec![root.clone()],
        library_dirs: vec![root.clone()],
        libraries: vec!["counter_impl".into()],
        namespaces: vec!["acme".into()],
        templates: vec![jet::CppBind::TemplateInstantiation {
            qualified_name: "acme::twice".into(),
            cpp_args: vec!["int64_t".into()],
            jet_name: "twice_int".into(),
        }],
    };
    let result = jet::CppBind::bind(&header, &cache, &options).unwrap();
    fs::write(cache.join("counter.jet"), &result.source).unwrap();
    fs::write(cache.join("counter.provenance"), &result.provenance).unwrap();
    assert!(result.archive.is_file());
    assert_ne!(result.archive.parent(), Some(cache.as_path()));
    assert!(cache.join("libjet_cpp_counter.a").is_file());
    assert!(result
        .provenance
        .contains("schema=jet-ffi-bridge-provenance-v1"));
    assert!(result.provenance.contains("binder-schema=jet-cpp-bind-v3"));
    assert!(result.provenance.contains(&format!("target={target}")));
    assert!(result
        .provenance
        .contains(&format!("clang={}", clang.display())));
    assert!(result.provenance.contains("namespace=acme"));
    assert!(result.provenance.contains("library=counter_impl"));
    assert!(result.provenance.contains("linked-archive="));
    assert!(result.provenance.contains("linked-archive-sha256="));
    assert!(result
        .provenance
        .contains("descriptor=jet-ffi-descriptor-v1;"));
    assert!(!result.bound.iter().any(|name| name.contains("decoy")));
    assert!(result.source.contains("pub fn apply(callback:"));
    assert!(result
        .source
        .contains("// jet-ffi-descriptor=jet-ffi-descriptor-v1;"));
    assert!(result.source.contains("-[FFI.Cpp]>"));
    assert!(!result.source.contains("=>"));

    let changed_target = jet::CppBind::cache_identity_for_test(&header, &options, "other-target");
    let selected_target = jet::CppBind::cache_identity_for_test(&header, &options, target);
    assert_ne!(changed_target, selected_target);

    let main = root.join("main.jet");
    let source = r#"use cpp.counter as cpp

fn increment(value: Int) Int -[]> { return value + 1 }
fn reject(value: Int) Int -[]> { return -value }

fn run() {
    counter := cpp.new_counter(10) ?? panic("constructor")
    print(counter.add_amount(5) ?? panic("method"))
    print(counter.add_factor(2.0) ?? panic("overload"))
    print(counter.add(3) ?? panic("operator"))
    print(cpp.apply(increment, 41) ?? panic("callback"))
    print(cpp.apply(reject, 41) ?? -1)
    print(cpp.twice_int(21) ?? panic("template"))
    print(counter.fail_if_negative(-1) ?? -1)
    cpp.close_counter(^counter)
}
"#;
    fs::write(&main, source).unwrap();
    let output = jet::compile_with_path(source, main.to_str().unwrap()).unwrap_or_else(|diags| {
        panic!(
            "{}",
            jet::render_diagnostics(main.to_str().unwrap(), source, &diags)
        )
    });
    let rust = root.join("main.rs");
    let binary = root.join("main_bin");
    fs::write(&rust, output.rust).unwrap();
    let mut command = Command::new(&rustc);
    command
        .args(["--edition", "2021", "--target", target])
        .arg(&rust)
        .arg("-o")
        .arg(&binary);
    for arg in output
        .clinks
        .into_iter()
        .chain(jet::resolve_c_links(main.to_str().unwrap()).unwrap())
    {
        command.arg(arg);
    }
    run(&mut command);
    let run_output = Command::new(&binary).output().unwrap();
    assert!(run_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run_output.stdout),
        "15\n17\n20\n42\n-1\n42\n-1\n"
    );

    let mut bundle =
        jet::Loader::load_entry_with_overlay(main.to_str().unwrap(), None, false).unwrap();
    assert!(jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run).is_empty());
    let boundary = jet_jit::resident_jit_safe_bundle_detail(&bundle);
    assert!(
        !boundary.is_empty() && boundary.to_ascii_lowercase().contains("foreign"),
        "resident JIT must name its C++ boundary instead of silently falling back: {boundary}"
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn cpp_binder_uses_selected_target_tools_runtime_and_cache_identity() {
    let target = "aarch64-apple-darwin";
    let root = std::env::temp_dir().join(format!(
        "jet_cpp_target_tools_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let header = root.join("probe.hpp");
    fs::write(&header, "long probe(long value);\n").unwrap();
    let canonical = fs::canonicalize(&header).unwrap();
    let clang_log = root.join("clang.log");
    let ar_log = root.join("ar.log");
    let clang = root.join("fake-clang++");
    let ar = root.join("fake-ar");
    fs::write(
        &clang,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = "--version" ]; then printf '%s\n' 'fake clang 1'; exit 0; fi
case "$*" in
  *-ast-dump=json*) printf '%s\n' '{}'; exit 0 ;;
esac
previous=''
output=''
for argument in "$@"; do
  if [ "$previous" = "-o" ]; then output="$argument"; fi
  previous="$argument"
done
: > "$output"
"#,
            clang_log.display(),
            format!(
                r#"{{"kind":"TranslationUnitDecl","inner":[{{"kind":"FunctionDecl","name":"probe","loc":{{"file":"{}"}},"type":{{"qualType":"long (long)"}},"inner":[{{"kind":"ParmVarDecl","name":"value","type":{{"qualType":"long"}}}}]}}]}}"#,
                canonical.display()
            )
        ),
    )
    .unwrap();
    fs::write(
        &ar,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = "--version" ]; then printf '%s\n' 'fake ar 1'; exit 0; fi
: > "$2"
"#,
            ar_log.display()
        ),
    )
    .unwrap();
    for tool in [&clang, &ar] {
        let mut permissions = fs::metadata(tool).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(tool, permissions).unwrap();
    }

    let cache = root.join(".jet/bindings/cpp");
    let options = jet::CppBind::BindOptions {
        lib: "probe".into(),
        target: target.into(),
        clang: clang.clone(),
        archiver: ar.clone(),
        include_dirs: vec![],
        library_dirs: vec![],
        libraries: vec![],
        namespaces: vec![],
        templates: vec![],
    };
    jet::CppBind::bind(&header, &cache, &options).unwrap();
    let clang_commands = fs::read_to_string(&clang_log).unwrap();
    assert!(clang_commands.contains("-target aarch64-apple-darwin"));
    assert!(clang_commands.contains("-Wl,-undefined,error"));
    assert!(clang_commands.contains("proof.dylib"));
    assert!(fs::read_to_string(&ar_log).unwrap().contains("rcs"));
    let link = fs::read_to_string(cache.join("probe.link")).unwrap();
    assert!(link.contains("target\taarch64-apple-darwin"));
    assert!(link.contains("l\tc++"));
    let flags = jet::CFFI::resolve_link_for_target("jet_cpp_probe", &root, target).unwrap();
    assert!(flags.link_names.contains(&"c++".to_string()));
    assert!(!flags.link_names.contains(&"stdc++".to_string()));

    let shared_input = root.join("same-input");
    fs::create_dir(&shared_input).unwrap();
    let mut include_options = options.clone();
    include_options.include_dirs = vec![shared_input.clone()];
    let mut library_options = options.clone();
    library_options.library_dirs = vec![shared_input.clone()];
    let include_identity = jet::CppBind::cache_identity_for_test(&header, &include_options, target);
    let library_identity = jet::CppBind::cache_identity_for_test(&header, &library_options, target);
    assert_ne!(include_identity, library_identity);

    fs::write(&clang_log, "").unwrap();
    let include_result = jet::CppBind::bind(&header, &cache, &include_options).unwrap();
    let include_commands = fs::read_to_string(&clang_log).unwrap();
    let include_proof = include_commands
        .lines()
        .find(|line| line.contains("-shared"))
        .unwrap();
    assert!(!include_proof.contains(&format!("-L {}", shared_input.display())));

    fs::write(&clang_log, "").unwrap();
    let library_result = jet::CppBind::bind(&header, &cache, &library_options).unwrap();
    assert_ne!(
        include_result.archive.parent(),
        library_result.archive.parent()
    );
    let library_commands = fs::read_to_string(&clang_log).unwrap();
    let library_proof = library_commands
        .lines()
        .find(|line| line.contains("-shared"))
        .unwrap();
    assert!(library_proof.contains(&format!("-L {}", shared_input.display())));

    let first_identity = jet::CppBind::cache_identity_for_test(&header, &options, target);
    fs::write(
        &ar,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = "--version" ]; then printf '%s\n' 'fake ar 2'; exit 0; fi
: > "$2"
"#,
            ar_log.display()
        ),
    )
    .unwrap();
    let second_identity = jet::CppBind::cache_identity_for_test(&header, &options, target);
    assert_ne!(first_identity, second_identity);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn cpp_link_metadata_requires_exact_selected_target() {
    let target = "aarch64-apple-darwin";
    let root = std::env::temp_dir().join(format!(
        "jet_cpp_link_target_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let dir = root.join(".jet/bindings/cpp");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("libjet_cpp_probe.a"), "archive").unwrap();
    let metadata = dir.join("probe.link");

    for invalid in [
        "l\tc++\n",
        "target\tx86_64-unknown-linux-gnu\nl\tc++\n",
        "target\taarch64-apple-darwin\ntarget\taarch64-apple-darwin\nl\tc++\n",
        "target aarch64-apple-darwin\nl\tc++\n",
    ] {
        fs::write(&metadata, invalid).unwrap();
        assert!(jet::CFFI::resolve_link_for_target("jet_cpp_probe", &root, target).is_err());
    }
    fs::write(&metadata, "target\taarch64-apple-darwin\nl\tc++\n").unwrap();
    assert!(jet::CFFI::resolve_link_for_target("jet_cpp_probe", &root, target).is_ok());

    let _ = fs::remove_dir_all(root);
}

// ---------------------------------------------------------------------------
// Card #1348 — mixed-repository adoption loop.
//
// This is deliberately kept in the existing polyglot target.  It is the
// executable companion to docs/reference/mixed-repo.md and exercises the
// shipped Jet-as-host surfaces in one representative repository shape.  The
// native Jet Library -> C/C++ host direction is covered by
// `tests/library_outputs.rs`; JVM/JS/Python guest exports remain outside the
// current native Library contract.
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn mixed_required_tool(name: &str) -> PathBuf {
    tool(name).unwrap_or_else(|| {
        panic!(
            "mixed-repo production matrix requires `{name}`; install the host toolchain or run the host-gated proof in the provisioned Jet environment"
        )
    })
}

#[cfg(unix)]
fn mixed_jet(cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.current_dir(cwd).env("NO_COLOR", "1");
    command
}

#[cfg(unix)]
fn mixed_jet_output(cwd: &Path, args: &[&str], java_home: Option<&Path>) -> Output {
    let mut command = mixed_jet(cwd);
    if let Some(java_home) = java_home {
        command.env("JAVA_HOME", java_home);
    }
    command
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("could not start jet {args:?}: {error}"))
}

#[cfg(unix)]
fn mixed_output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(unix)]
fn mixed_assert_ok(label: &str, output: &Output) {
    assert!(output.status.success(), "{label} failed\n{}", mixed_output_text(output));
}

#[cfg(unix)]
fn mixed_assert_failed_with(label: &str, output: &Output, marker: &str) {
    assert!(!output.status.success(), "{label} unexpectedly passed");
    let text = mixed_output_text(output);
    assert!(text.contains(marker), "{label} omitted `{marker}`\n{text}");
}

#[cfg(unix)]
fn mixed_assert_common_provenance(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    assert!(
        text.contains("schema=jet-ffi-bridge-provenance-v1\n"),
        "{} did not use the common bridge provenance schema:\n{text}",
        path.display()
    );
    assert!(
        text.lines().any(|line| line.starts_with("identity=")),
        "{} has no bridge identity:\n{text}",
        path.display()
    );
    text
}

#[cfg(unix)]
fn mixed_write_package(root: &Path, name: &str, deps: Option<String>) {
    let dependency_line = deps
        .map(|deps| format!("deps: .{{ {deps} }}\n"))
        .unwrap_or_default();
    fs::write(
        root.join("package.jet"),
        format!("name: \"{name}\"\nversion: \"0.1.0\"\n{dependency_line}"),
    )
    .unwrap();
}

#[cfg(unix)]
fn mixed_build_archive(cc: &Path, ar: &Path, root: &Path, stem: &str, threaded: bool) {
    let source = root.join(format!("{stem}.c"));
    let object = root.join(format!("{stem}.o"));
    let archive = root.join(format!("lib{stem}.a"));
    let mut compile = Command::new(cc);
    compile.args(["-std=c11", "-fPIC", "-c"]);
    if threaded {
        compile.arg("-pthread");
    }
    compile.arg(&source).arg("-o").arg(&object);
    let output = compile.output().unwrap();
    assert!(
        output.status.success(),
        "C host compile failed\n{}",
        mixed_output_text(&output)
    );
    let mut archive_command = Command::new(ar);
    archive_command.args(["rcs"]).arg(&archive).arg(&object);
    let output = archive_command.output().unwrap();
    assert!(
        output.status.success(),
        "C host archive failed\n{}",
        mixed_output_text(&output)
    );
}

#[cfg(unix)]
fn mixed_build_cpp_archive(clang: &Path, ar: &Path, root: &Path, target: &str) {
    let source = root.join("counter.cpp");
    let object = root.join("counter.o");
    let archive = root.join("libcounter_impl.a");
    let mut compile = Command::new(clang);
    compile
        .args(["-std=c++17", "-fPIC", "-pthread", "-c", "-target"])
        .arg(target)
        .arg("-I")
        .arg(root)
        .arg(&source)
        .arg("-o")
        .arg(&object);
    let output = compile.output().unwrap();
    assert!(
        output.status.success(),
        "C++ host compile failed\n{}",
        mixed_output_text(&output)
    );
    let mut archive_command = Command::new(ar);
    archive_command.args(["rcs"]).arg(&archive).arg(&object);
    let output = archive_command.output().unwrap();
    assert!(
        output.status.success(),
        "C++ host archive failed\n{}",
        mixed_output_text(&output)
    );
}

#[cfg(unix)]
fn mixed_strip_archive(strip: &Path, archive: &Path, label: &str) {
    let mut command = Command::new(strip);
    if cfg!(target_os = "macos") {
        command.arg("-S");
    } else {
        command.arg("--strip-debug");
    }
    let output = command.arg(archive).output().unwrap();
    assert!(
        output.status.success(),
        "{label} strip failed\n{}",
        mixed_output_text(&output)
    );
}

#[cfg(unix)]
fn mixed_java_home(javac: &Path) -> PathBuf {
    let home = std::env::var_os("JAVA_HOME")
        .map(PathBuf::from)
        .or_else(|| javac.parent().and_then(Path::parent).map(Path::to_path_buf))
        .unwrap_or_else(|| panic!("cannot infer JAVA_HOME from {}", javac.display()));
    let runtime = home.join("lib/server").join(if cfg!(target_os = "macos") {
        "libjvm.dylib"
    } else {
        "libjvm.so"
    });
    assert!(
        runtime.is_file(),
        "JAVA_HOME={} has no embedded JVM runtime at {}",
        home.display(),
        runtime.display()
    );
    home
}

#[cfg(unix)]
fn mixed_bind_c(root: &Path) -> Output {
    mixed_jet_output(
        root,
        &["inspect", "bind", "counter.h", "--pkg", "counter"],
        None,
    )
}

#[cfg(unix)]
fn mixed_bind_cpp(root: &Path, target: &str, clang: &Path, ar: &Path) -> Output {
    let mut command = mixed_jet(root);
    command
        .args(["inspect", "bind", "cpp", "counter.hpp", "--target"])
        .arg(target)
        .args(["--clang"])
        .arg(clang)
        .args(["--ar"])
        .arg(ar)
        .args(["--pkg", "counter", "--namespace", "acme", "-I"])
        .arg(root)
        .args(["-L"])
        .arg(root)
        .args(["-l", "counter_impl", "-l", "pthread"])
        .output()
        .unwrap()
}

#[cfg(unix)]
fn mixed_bind_java(root: &Path, java_home: &Path) -> Output {
    mixed_jet_output(
        root,
        &["inspect", "bind", "java", "Counter.java", "--pkg", "counter"],
        Some(java_home),
    )
}

#[cfg(unix)]
fn mixed_bind_js(root: &Path) -> Output {
    mixed_jet_output(
        root,
        &[
            "inspect",
            "bind",
            "js",
            "ops.d.ts",
            "--runtime",
            "ops.mjs",
            "--pkg",
            "ops",
        ],
        None,
    )
}

#[cfg(unix)]
fn mixed_bind_python(root: &Path) -> Output {
    mixed_jet_output(
        root,
        &["inspect", "bind", "py", "ops.py", "--pkg", "ops"],
        None,
    )
}

#[cfg(unix)]
fn mixed_run_workflow(root: &Path, expected: &str, java_home: Option<&Path>) {
    let run_file = root.join("run.jet");
    let run_path = run_file.to_str().unwrap();
    assert_eq!(jet::Debug::needs_native(run_path), Some(true));

    let checked = mixed_jet_output(root, &["check", "run.jet"], java_home);
    mixed_assert_ok("mixed-repo check", &checked);

    let built = mixed_jet_output(root, &["build", "--profile=debug", "run.jet"], java_home);
    mixed_assert_ok("mixed-repo debug build", &built);

    let tested = mixed_jet_output(
        root,
        &["test", "--coverage", "--profile=debug", "run.jet"],
        java_home,
    );
    mixed_assert_ok("mixed-repo test with coverage", &tested);
    assert!(
        mixed_output_text(&tested).contains("coverage:"),
        "test completed without a coverage report\n{}",
        mixed_output_text(&tested)
    );

    let run = mixed_jet_output(root, &["run", "--profile=debug", "run.jet"], java_home);
    mixed_assert_ok("mixed-repo debug-profile run", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);

    let debug_file = root.join("debug.jet");
    fs::write(
        &debug_file,
        include_str!("fixtures/mixed_repo/debug.jet"),
    )
    .unwrap();
    let debug = jet::Debug::run_session_result(debug_file.to_str().unwrap(), &["s", "c"]);
    assert_eq!(debug.status, jet::Debug::SessionStatus::Finished);
    assert!(debug.transcript.contains("program finished"));
    assert!(debug.transcript.contains("42"));

    let trace = root.join("profile.jettrace");
    let mut perf = mixed_jet(root);
    if let Some(java_home) = java_home {
        perf.env("JAVA_HOME", java_home);
    }
    let perf = perf
        .args(["perf", "run", "run.jet", "--profile=debug", "--out"])
        .arg(&trace)
        .output()
        .unwrap();
    mixed_assert_ok("mixed-repo profile capture", &perf);
    let trace_text = fs::read_to_string(&trace).unwrap();
    assert!(trace_text.contains("\"schema\":\"jet.trace\""));
    assert!(trace_text.contains("run.jet"));
    assert!(trace_text.contains("source_identity"));
    assert!(!trace_text.contains("__jet_"));

    let mut view = mixed_jet(root);
    let view = view
        .args(["perf", "view"])
        .arg(&trace)
        .arg("--json")
        .output()
        .unwrap();
    mixed_assert_ok("mixed-repo profile view", &view);
    let view_text = String::from_utf8_lossy(&view.stdout);
    assert!(view_text.contains("\"kind\":\"jet.trace.view\""));
    assert!(view_text.contains("\"flamegraph\":"));
}

#[cfg(unix)]
fn mixed_recover_stale_descriptor(
    root: &Path,
    cache: &Path,
    expected: &str,
    java_home: Option<&Path>,
    rebind: impl FnOnce() -> Output,
) {
    let original = fs::read_to_string(cache).unwrap();
    let stale = original.replacen(
        "jet-ffi-descriptor=",
        "jet-ffi-descriptor=stale-",
        1,
    );
    assert_ne!(original, stale);
    fs::write(cache, stale).unwrap();
    let rejected = mixed_jet_output(root, &["check", "run.jet"], java_home);
    mixed_assert_failed_with("stale foreign descriptor", &rejected, "E3208");

    let rebound = rebind();
    mixed_assert_ok("foreign descriptor recovery bind", &rebound);
    let recovered = mixed_jet_output(root, &["run", "--profile=debug", "run.jet"], java_home);
    mixed_assert_ok("foreign descriptor recovery run", &recovered);
    assert_eq!(String::from_utf8_lossy(&recovered.stdout), expected);
}

#[cfg(unix)]
fn mixed_assert_bad_bind(root: &Path, args: &[&str], cache: &Path) {
    let rejected = mixed_jet_output(root, args, None);
    mixed_assert_failed_with("malformed foreign source", &rejected, "E3208");
    assert!(!cache.exists(), "malformed foreign source published {cache:?}");
}

#[cfg(unix)]
fn mixed_c_case(root: &Path, cc: &Path, ar: &Path, strip: &Path) {
    let root = root.join("c");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("counter.h"),
        include_str!("fixtures/mixed_repo/c/counter.h"),
    )
    .unwrap();
    fs::write(
        root.join("counter.c"),
        include_str!("fixtures/mixed_repo/c/counter.c"),
    )
    .unwrap();
    fs::write(
        root.join("run.jet"),
        include_str!("fixtures/mixed_repo/c/run.jet"),
    )
    .unwrap();
    mixed_write_package(
        &root,
        "mixed_c",
        Some(format!(
            "counter: c@\"{}\", pthread: c@system",
            root.display()
        )),
    );
    mixed_build_archive(cc, ar, &root, "counter", true);

    let bind = mixed_bind_c(&root);
    mixed_assert_ok("C header bind", &bind);
    let cache = root.join(".jet/bindings/c/counter.jet");
    assert!(fs::read_to_string(&cache)
        .unwrap()
        .contains("jet-ffi-descriptor="));
    assert!(root.join(".jet/bindings/c/counter.hash").is_file());
    let provenance = mixed_assert_common_provenance(
        &root.join(".jet/bindings/c/counter.provenance"),
    );
    assert!(provenance.contains("linked-archive="));
    assert!(provenance.contains("linked-archive-sha256="));

    let archive = root.join("libcounter.a");
    let original_archive = fs::read(&archive).unwrap();
    let mut changed_archive = original_archive.clone();
    changed_archive.push(0);
    fs::write(&archive, changed_archive).unwrap();
    let stale_archive = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("stale C implementation archive", &stale_archive, "E3208");
    fs::write(&archive, &original_archive).unwrap();

    mixed_run_workflow(&root, "42\n42\n", None);
    mixed_strip_archive(strip, &archive, "C host archive");
    let rebound = mixed_bind_c(&root);
    mixed_assert_ok("stripped C rebind", &rebound);
    let release = mixed_jet_output(&root, &["run", "--profile=release", "run.jet"], None);
    mixed_assert_ok("stripped C release run", &release);
    assert_eq!(String::from_utf8_lossy(&release.stdout), "42\n42\n");

    mixed_recover_stale_descriptor(&root, &cache, "42\n42\n", None, || mixed_bind_c(&root));

    fs::write(
        root.join("broken.h"),
        include_str!("fixtures/mixed_repo/c/broken.h"),
    )
    .unwrap();
    mixed_assert_bad_bind(
        &root,
        &["inspect", "bind", "broken.h", "--pkg", "broken"],
        &root.join(".jet/bindings/c/broken.jet"),
    );
    fs::write(
        root.join("broken.h"),
        "#include <stdint.h>\nint64_t repaired(int64_t value);\n",
    )
    .unwrap();
    let repaired = mixed_jet_output(
        &root,
        &["inspect", "bind", "broken.h", "--pkg", "broken"],
        None,
    );
    mixed_assert_ok("C malformed-header recovery", &repaired);
}

#[cfg(unix)]
fn mixed_cpp_case(root: &Path, clang: &Path, ar: &Path, strip: &Path, target: &str) {
    let root = root.join("cpp");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("counter.hpp"),
        include_str!("fixtures/mixed_repo/cpp/counter.hpp"),
    )
    .unwrap();
    fs::write(
        root.join("counter.cpp"),
        include_str!("fixtures/mixed_repo/cpp/counter.cpp"),
    )
    .unwrap();
    fs::write(
        root.join("run.jet"),
        include_str!("fixtures/mixed_repo/cpp/run.jet"),
    )
    .unwrap();
    mixed_write_package(&root, "mixed_cpp", None);
    mixed_build_cpp_archive(clang, ar, &root, target);

    let bind = mixed_bind_cpp(&root, target, clang, ar);
    mixed_assert_ok("C++ bind", &bind);
    let cache = root.join(".jet/bindings/cpp/counter.jet");
    let link = root.join(".jet/bindings/cpp/counter.link");
    assert!(fs::read_to_string(&cache)
        .unwrap()
        .contains("jet-ffi-descriptor="));
    let provenance = mixed_assert_common_provenance(
        &root.join(".jet/bindings/cpp/counter.provenance"),
    );
    assert!(provenance.contains("linked-archive="));
    assert!(provenance.contains("linked-archive-sha256="));
    assert!(link.is_file());

    let archive = root.join("libcounter_impl.a");
    let original_archive = fs::read(&archive).unwrap();
    let mut changed_archive = original_archive.clone();
    changed_archive.push(0);
    fs::write(&archive, changed_archive).unwrap();
    let stale_archive = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("stale C++ implementation archive", &stale_archive, "E3208");
    fs::write(&archive, &original_archive).unwrap();

    let header = root.join("counter.hpp");
    let original_header = fs::read_to_string(&header).unwrap();
    fs::write(&header, format!("{original_header}\n// stale binding source\n")).unwrap();
    let stale_source = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("stale C++ binding source", &stale_source, "E3208");
    fs::write(&header, original_header).unwrap();

    mixed_run_workflow(&root, "15\n17\n42\n42\n-1\n", None);
    mixed_strip_archive(strip, &archive, "C++ host archive");
    let rebound = mixed_bind_cpp(&root, target, clang, ar);
    mixed_assert_ok("stripped C++ rebind", &rebound);
    let release = mixed_jet_output(&root, &["run", "--profile=release", "run.jet"], None);
    mixed_assert_ok("stripped C++ release run", &release);
    assert_eq!(String::from_utf8_lossy(&release.stdout), "15\n17\n42\n42\n-1\n");

    mixed_recover_stale_descriptor(
        &root,
        &cache,
        "15\n17\n42\n42\n-1\n",
        None,
        || mixed_bind_cpp(&root, target, clang, ar),
    );

    let link_original = fs::read_to_string(&link).unwrap();
    let link_stale = link_original.replacen(
        &format!("target\t{target}"),
        "target\tmixed-invalid-target",
        1,
    );
    assert_ne!(link_original, link_stale);
    fs::write(&link, link_stale).unwrap();
    let abi_rejected = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("C++ ABI-target mismatch", &abi_rejected, "E3201");
    let rebound = mixed_bind_cpp(&root, target, clang, ar);
    mixed_assert_ok("C++ ABI-target recovery bind", &rebound);
    let recovered = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_ok("C++ ABI-target recovery run", &recovered);
    assert_eq!(
        String::from_utf8_lossy(&recovered.stdout),
        "15\n17\n42\n42\n-1\n"
    );

    let link_original = fs::read_to_string(&link).unwrap();
    let link_stale = link_original.replacen(
        "l\tcounter_impl\n",
        "l\tcounter_impl_stale\n",
        1,
    );
    assert_ne!(link_original, link_stale);
    fs::write(&link, link_stale).unwrap();
    let stale_link = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("stale C++ link provenance", &stale_link, "E3208");
    let link_rebound = mixed_bind_cpp(&root, target, clang, ar);
    mixed_assert_ok("C++ link provenance recovery bind", &link_rebound);
    let link_recovered = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_ok("C++ link provenance recovery run", &link_recovered);
}

#[cfg(unix)]
fn mixed_java_case(root: &Path, java_home: &Path) {
    let root = root.join("jvm");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("Counter.java"),
        include_str!("fixtures/mixed_repo/jvm/Counter.java"),
    )
    .unwrap();
    fs::write(
        root.join("run.jet"),
        include_str!("fixtures/mixed_repo/jvm/run.jet"),
    )
    .unwrap();
    mixed_write_package(&root, "mixed_jvm", None);

    let bind = mixed_bind_java(&root, java_home);
    mixed_assert_ok("JVM bind", &bind);
    let cache = root.join(".jet/bindings/java/counter.jet");
    assert!(cache.is_file());
    mixed_assert_common_provenance(&root.join(".jet/bindings/java/counter.provenance"));
    assert!(root
        .join(".jet/bindings/java/counter.jvm-path")
        .is_file());

    mixed_run_workflow(&root, "42\n5.0\n-7\n", Some(java_home));

    let source = root.join("Counter.java");
    let original_source = fs::read_to_string(&source).unwrap();
    fs::write(&source, format!("{original_source}\n// stale binding source\n")).unwrap();
    let stale_source = mixed_jet_output(&root, &["run", "run.jet"], Some(java_home));
    mixed_assert_failed_with("stale JVM binding source", &stale_source, "E3208");
    fs::write(&source, original_source).unwrap();

    let edited = fs::read_to_string(&source)
        .unwrap()
        .replace("value += amount", "value += amount + 1");
    fs::write(&source, edited).unwrap();
    let rebound = mixed_bind_java(&root, java_home);
    mixed_assert_ok("edited JVM rebind", &rebound);
    let edited_run = mixed_jet_output(
        &root,
        &["run", "--profile=debug", "run.jet"],
        Some(java_home),
    );
    mixed_assert_ok("edited JVM run", &edited_run);
    assert_eq!(String::from_utf8_lossy(&edited_run.stdout), "43\n5.0\n-7\n");
}

#[cfg(unix)]
fn mixed_js_case(root: &Path) {
    let root = root.join("js");
    fs::create_dir_all(&root).unwrap();
    for (name, fixture) in [
        (
            "ops.d.ts",
            include_str!("fixtures/mixed_repo/js/ops.d.ts"),
        ),
        (
            "ops.mjs",
            include_str!("fixtures/mixed_repo/js/ops.mjs"),
        ),
        (
            "run.jet",
            include_str!("fixtures/mixed_repo/js/run.jet"),
        ),
        (
            "broken.d.ts",
            include_str!("fixtures/mixed_repo/js/broken.d.ts"),
        ),
    ] {
        fs::write(root.join(name), fixture).unwrap();
    }
    mixed_write_package(&root, "mixed_js", None);

    let bind = mixed_bind_js(&root);
    mixed_assert_ok("JavaScript bind", &bind);
    let cache = root.join(".jet/bindings/js/ops.jet");
    assert!(cache.is_file());
    mixed_assert_common_provenance(&root.join(".jet/bindings/js/ops.provenance"));
    mixed_run_workflow(&root, "5\n99\n123\n", None);

    let runtime = root.join("ops.mjs");
    let original_runtime = fs::read_to_string(&runtime).unwrap();
    fs::write(&runtime, format!("{original_runtime}\n// stale binding source\n")).unwrap();
    let stale_source = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("stale JavaScript binding source", &stale_source, "E3208");
    fs::write(&runtime, original_runtime).unwrap();

    mixed_recover_stale_descriptor(&root, &cache, "5\n99\n123\n", None, || {
        mixed_bind_js(&root)
    });
    mixed_assert_bad_bind(
        &root,
        &[
            "inspect",
            "bind",
            "js",
            "broken.d.ts",
            "--runtime",
            "ops.mjs",
            "--pkg",
            "broken",
        ],
        &root.join(".jet/bindings/js/broken.jet"),
    );

    let edited = fs::read_to_string(&runtime)
        .unwrap()
        .replace("return left + right", "return left + right + 1");
    fs::write(&runtime, edited).unwrap();
    let rebound = mixed_bind_js(&root);
    mixed_assert_ok("edited JavaScript rebind", &rebound);
    let edited_run = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_ok("edited JavaScript run", &edited_run);
    assert_eq!(String::from_utf8_lossy(&edited_run.stdout), "6\n99\n123\n");
}

#[cfg(unix)]
fn mixed_python_case(root: &Path) {
    let root = root.join("python");
    fs::create_dir_all(&root).unwrap();
    for (name, fixture) in [
        ("ops.py", include_str!("fixtures/mixed_repo/python/ops.py")),
        (
            "run.jet",
            include_str!("fixtures/mixed_repo/python/run.jet"),
        ),
        (
            "broken.py",
            include_str!("fixtures/mixed_repo/python/broken.py"),
        ),
    ] {
        fs::write(root.join(name), fixture).unwrap();
    }
    mixed_write_package(&root, "mixed_python", None);

    let bind = mixed_bind_python(&root);
    mixed_assert_ok("Python bind", &bind);
    let cache = root.join(".jet/bindings/py/ops.jet");
    assert!(cache.is_file());
    mixed_assert_common_provenance(&root.join(".jet/bindings/py/ops.provenance"));
    mixed_run_workflow(&root, "5\nfalse\n99\n123\n", None);

    let source = root.join("ops.py");
    let original_source = fs::read_to_string(&source).unwrap();
    fs::write(&source, format!("{original_source}\n# stale binding source\n")).unwrap();
    let stale_source = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_failed_with("stale Python binding source", &stale_source, "E3208");
    fs::write(&source, original_source).unwrap();

    mixed_recover_stale_descriptor(&root, &cache, "5\nfalse\n99\n123\n", None, || {
        mixed_bind_python(&root)
    });
    mixed_assert_bad_bind(
        &root,
        &["inspect", "bind", "py", "broken.py", "--pkg", "broken"],
        &root.join(".jet/bindings/py/broken.jet"),
    );

    let edited = fs::read_to_string(&source)
        .unwrap()
        .replace("return left + right", "return left + right + 1");
    fs::write(&source, edited).unwrap();
    let rebound = mixed_bind_python(&root);
    mixed_assert_ok("edited Python rebind", &rebound);
    let edited_run = mixed_jet_output(&root, &["run", "run.jet"], None);
    mixed_assert_ok("edited Python run", &edited_run);
    assert_eq!(
        String::from_utf8_lossy(&edited_run.stdout),
        "6\nfalse\n99\n123\n"
    );
}

#[cfg(unix)]
#[test]
fn mixed_repo_edit_test_debug_profile_and_recovery_workflows() {
    let cc = mixed_required_tool("cc");
    let ar = mixed_required_tool("ar");
    let clang = mixed_required_tool("clang++");
    let strip = mixed_required_tool("strip");
    let _nm = mixed_required_tool("nm");
    let javac = mixed_required_tool("javac");
    let _javap = mixed_required_tool("javap");
    let _node = mixed_required_tool("node");
    let _python = mixed_required_tool("python3");
    let java_home = mixed_java_home(&javac);
    let root = common::Scratch::new("mixed-repo");

    mixed_c_case(&root.path, &cc, &ar, &strip);
    mixed_cpp_case(&root.path, &clang, &ar, &strip, env!("JET_BUILD_TARGET"));
    mixed_java_case(&root.path, &java_home);
    mixed_js_case(&root.path);
    mixed_python_case(&root.path);
}
