mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    assert!(result.provenance.contains("schema=jet-cpp-bind-v3"));
    assert!(result.provenance.contains(&format!("target={target}")));
    assert!(result.provenance.contains(&format!("clang={}", clang.display())));
    assert!(result.provenance.contains("namespace=acme"));
    assert!(result.provenance.contains("library=counter_impl"));
    assert!(!result.bound.iter().any(|name| name.contains("decoy")));
    assert!(result.source.contains("pub fn apply(callback:"));

    let changed_target = jet::CppBind::cache_identity_for_test(&header, &options, "other-target");
    let selected_target = jet::CppBind::cache_identity_for_test(&header, &options, target);
    assert_ne!(changed_target, selected_target);

    let main = root.join("main.jet");
    let source = r#"use cpp.counter as cpp

fn increment(value: Int) =[]=> Int { return value + 1 }
fn reject(value: Int) =[]=> Int { return -value }

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
        panic!("{}", jet::render_diagnostics(main.to_str().unwrap(), source, &diags))
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
    let include_identity =
        jet::CppBind::cache_identity_for_test(&header, &include_options, target);
    let library_identity =
        jet::CppBind::cache_identity_for_test(&header, &library_options, target);
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
    assert_ne!(include_result.archive.parent(), library_result.archive.parent());
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
