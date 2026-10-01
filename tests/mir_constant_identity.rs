mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::{
    collections::HashSet,
    fs,
    path::Path,
    process::Command,
};

const MAIN_SOURCE: &str = r#"
use alpha.alias_value_list as imported_alpha_values
use beta.[import_value_list as imported_beta_values]
module alpha
module beta

struct PublicValuesBox {
    alias_values: [U8]
}

fn int_head(values: [Int]) -> Int {
    return values[0]
}

fn byte_head(values: [U8]) -> U8 {
    return values[0]
}

fn module_prefix_shadow(beta: PublicValuesBox) -> U8 {
    return beta.alias_values[0]
}

fn run() {
    print(alpha.token_at(0))
    print(alpha.payload_at(0))
    print(beta.token_at(0))
    print(beta.payload_at(0))
    print(int_head(imported_alpha_values()))
    print(byte_head(imported_beta_values()))
    print(byte_head(beta.alias_value_list()))
    print(alpha.shadowed([99]))
    print(module_prefix_shadow(PublicValuesBox{alias_values: [U8]{7}}))
}
"#;

const ALPHA_SOURCE: &str = r#"
TOKEN :: [Int]{65}
PAYLOAD :: [Int]{17}
ALIAS_VALUES :: [Int]{31}

pub fn token_at(index: Int) -> Int {
    return TOKEN[index]
}

pub fn payload_at(index: Int) -> Int {
    return PAYLOAD[index]
}

pub fn alias_value_list() -> [Int] {
    return ALIAS_VALUES
}

pub fn shadowed(token: [Int]) -> Int {
    return token[0]
}
"#;

const BETA_SOURCE: &str = r#"
TOKEN :: [Int]{66}
PAYLOAD :: [U8]{84}
ALIAS_VALUES :: [U8]{42}
IMPORT_VALUES :: [U8]{43}

pub fn token_at(index: Int) -> Int {
    return TOKEN[index]
}

pub fn payload_at(index: Int) -> U8 {
    return PAYLOAD[index]
}

pub fn alias_value_list() -> [U8] {
    return ALIAS_VALUES
}

pub fn import_value_list() -> [U8] {
    return IMPORT_VALUES
}
"#;

const EXPECTED: &str = "65\n17\n66\n84\n31\n43\n42\n99\n7\n";

const FILES: &[(&str, &str)] = &[
    ("main.jet", MAIN_SOURCE),
    ("alpha.jet", ALPHA_SOURCE),
    ("beta.jet", BETA_SOURCE),
];

fn write_fixture(root: &Path) {
    tir_support::write_test_package(root, tir_support::TIR_TEST_PACKAGE);
    for (relative, source) in FILES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, source).unwrap();
    }
}

fn assert_fixture_constants_reach_mir_globals() {
    let scratch = common::Scratch::new("mir_constant_identity_keys");
    write_fixture(&scratch.path);
    let mir = jet::run_compiler_work(|| {
        let entry = scratch.path.join("main.jet");
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
            .expect("constant identity bundle should load");
        let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.severity,
                    jet::Diagnostics::Severity::Error
                )
            })
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "constant identity fixture rejected: {errors:#?}");

        let request = jet_foundation::MIR::MirArtifactRequest::new(
            jet_foundation::MIR::MirArtifactTarget::RustAot,
            jet_foundation::MIR::MirArtifactKind::NativeExecutable,
            jet_foundation::MIR::MirArtifactBuildMode::Dev,
        );
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request)
            .expect("constant identity fixture should lower through TIR");
        jet::Codegen::TIR::lower_tir_to_mir(&tir)
            .expect("constant identity fixture should lower through MIR")
    });

    let expected_keys = mir
        .constants
        .iter()
        .filter(|constant| {
            let Some(module) = mir
                .modules
                .iter()
                .find(|module| module.id == constant.module)
            else {
                return false;
            };
            (module.path.ends_with("alpha.jet") || module.path.ends_with("beta.jet"))
                && matches!(
                    constant.name.as_str(),
                    "TOKEN" | "PAYLOAD" | "ALIAS_VALUES" | "IMPORT_VALUES"
                )
        })
        .map(|constant| constant.key.clone())
        .collect::<HashSet<_>>();
    assert_eq!(
        expected_keys.len(),
        7,
        "fixture must retain duplicate module constants with distinct element types"
    );

    let global_keys = mir
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match &instruction.operation {
            jet_foundation::MIR::MirOperation::Global { name } => Some(name.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let missing = expected_keys
        .difference(&global_keys)
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "runtime fixture constants must reach MIR Globals by their exact definition keys: {missing:?}"
    );
}

fn assert_private_module_constant_is_rejected() {
    let scratch = common::Scratch::new("mir_constant_identity_private");
    write_fixture(&scratch.path);
    fs::write(
        scratch.path.join("main.jet"),
        r#"
module alpha
fn run() {
    print(alpha.PAYLOAD[0])
}
"#,
    )
    .unwrap();
    let diagnostics = jet::run_compiler_work(|| {
        let entry = scratch.path.join("main.jet");
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
            .expect("private constant bundle should load");
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
    });
    let private = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0605")
        .expect("qualified private module constant should be rejected");
    assert!(
        private.what.contains("PAYLOAD"),
        "private diagnostic must name the inaccessible constant: {private:?}"
    );
}

fn run_without_tier_trace(name: &str, mode: &[&str]) -> (i32, String, String) {
    let scratch = common::Scratch::new(name);
    write_fixture(&scratch.path);
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .args(mode)
        .arg("main.jet")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .env("JET_STORE_DIR", scratch.path.join("cache"))
        .env("JETPACK_ROOT", scratch.path.join("jetpack"))
        .output()
        .expect("Jet run should launch");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn assert_web_output(name: &str) {
    let scratch = common::Scratch::new(name);
    write_fixture(&scratch.path);

    let entry = scratch.path.join("main.jet");
    let shown = entry.to_string_lossy();
    let output = jet::compile_web(&shown).unwrap_or_else(|diagnostics| {
        panic!("web rejected constant identity fixture: {diagnostics:#?}")
    });
    let web = output.web.expect("web compile must produce web artifacts");
    let web_root = scratch.path.join("web");
    let build_dir = web_root.join("build");
    fs::create_dir_all(&build_dir).unwrap();
    fs::write(build_dir.join("web.manifest.json"), &web.manifest_json).unwrap();
    fs::write(build_dir.join("app.js"), &web.js_app).unwrap();
    fs::write(build_dir.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(build_dir.join("app_wasm.rs"), &web.wasm_rust).unwrap();

    let wasm = Command::new("rustc")
        .current_dir(&web_root)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "build/app_wasm.rs",
            "-o",
            "build/app.wasm",
        ])
        .output()
        .expect("rustc is required to compile the web constant-identity regression");
    assert!(
        wasm.status.success(),
        "rustc rejected web constant-identity wasm:\n{}",
        String::from_utf8_lossy(&wasm.stderr)
    );
    assert!(
        build_dir.join("app.wasm").is_file(),
        "web compile must produce app.wasm"
    );

    let node = Command::new("node")
        .current_dir(&build_dir)
        .args([
            "--input-type=module",
            "-e",
            "const { jet_main } = await import('./app.js');\n\
             const result = await jet_main();\n\
             if (result !== undefined) throw new Error('web entry success result must be undefined');",
        ])
        .output()
        .expect("Node is required to execute the web constant-identity regression");
    assert!(
        node.status.success(),
        "web constant-identity fixture failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), EXPECTED);
    assert!(
        node.stderr.is_empty(),
        "valid web program wrote to stderr: {}",
        String::from_utf8_lossy(&node.stderr)
    );
}

#[test]
fn aliased_single_member_import_uses_the_grouped_member_resolution() {
    let scratch = common::Scratch::new("mir_constant_identity_member_import");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    fs::write(
        scratch.path.join("main.jet"),
        "use alpha.value as dotted\nuse alpha.[value as grouped]\n\
         module alpha\nfn run() { print(dotted()); print(grouped()) }\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("alpha.jet"),
        "pub fn value() -> Int { 31 }\n",
    )
    .unwrap();
    let diagnostics = jet::run_compiler_work(|| {
        let entry = scratch.path.join("main.jet");
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
            .expect("both member spellings should load their declared module");
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
    });
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "member imports must resolve to public functions: {diagnostics:#?}"
    );

    fs::write(
        scratch.path.join("alpha.jet"),
        "fn value() -> Int { 31 }\n",
    )
    .unwrap();
    let diagnostics = jet::run_compiler_work(|| {
        let entry = scratch.path.join("main.jet");
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
            .expect("a private member still belongs to a valid module");
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
    });
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0609"),
        "private member import must be rejected: {diagnostics:#?}"
    );
    fs::write(
        scratch.path.join("alpha.jet"),
        "pub fn value() -> Int { 31 }\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("main.jet"),
        "use renamed.[value as grouped]\nuse renamed.value as dotted\n\
         use \"alpha\" as renamed\n\
         fn run() { print(dotted()); print(grouped()) }\n",
    )
    .unwrap();
    let diagnostics = jet::run_compiler_work(|| {
        let entry = scratch.path.join("main.jet");
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
            .expect("a named file import should bind a member-import prefix");
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
    });
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "aliased file imports must share member resolution: {diagnostics:#?}"
    );
}

#[test]
fn aliased_dotted_module_import_retains_module_precedence_and_ambiguity() {
    let scratch = common::Scratch::new("mir_constant_identity_module_import");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    fs::write(
        scratch.path.join("main.jet"),
        "use alpha.value as selected\nuse alpha.tools.value as deep\nmodule alpha\n\
         fn run() { print(selected.answer()); print(deep.answer()) }\n",
    )
    .unwrap();
    fs::write(scratch.path.join("alpha.jet"), "pub fn value() -> Int { 31 }\n").unwrap();
    fs::write(
        scratch.path.join("alpha.value.jet"),
        "pub fn answer() -> Int { 42 }\n",
    )
    .unwrap();
    fs::write(
        scratch.path.join("alpha.tools.value.jet"),
        "pub fn answer() -> Int { 43 }\n",
    )
    .unwrap();
    let entry = scratch.path.join("main.jet");
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap())
        .expect("a real dotted module must take precedence over a same-named member");
    let imported = &bundle.modules[bundle.entry].imports[0];
    assert!(
        matches!(&imported.kind, jet::AST::ImportKind::Module(path, _) if path == "alpha.value"),
        "a real module path must retain its namespace: {imported:?}"
    );
    let deep_import = &bundle.modules[bundle.entry].imports[1];
    assert!(
        matches!(&deep_import.kind, jet::AST::ImportKind::Module(path, _) if path == "alpha.tools.value"),
        "a deep module path must retain its namespace: {deep_import:?}"
    );
    let diagnostics = jet::run_compiler_work(|| {
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
    });
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "a real dotted module must resolve: {diagnostics:#?}"
    );

    fs::create_dir_all(scratch.path.join("other")).unwrap();
    fs::write(
        scratch.path.join("other/alpha.value.jet"),
        "pub fn answer() -> Int { 7 }\n",
    )
    .unwrap();
    let errors = jet::Loader::load_entry(entry.to_str().unwrap())
        .expect_err("ambiguous real modules must not turn into member imports");
    assert!(
        errors.iter().any(|diagnostic| diagnostic.code == "E0606"),
        "ambiguous dotted modules must retain E0606: {errors:#?}"
    );
}

#[test]
fn checked_private_constant_identity_survives_cross_module_calls_imports_and_shadows() {
    assert_fixture_constants_reach_mir_globals();
    assert_private_module_constant_is_rejected();

    let (code, stdout, stderr) =
        tir_support::build_release_and_run_multi("mir_constant_identity_aot", "main.jet", FILES);
    assert_eq!(code, 0, "AOT fixture failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
    assert!(
        stderr.is_empty(),
        "valid AOT program wrote to stderr: {stderr}"
    );

    let (code, stdout, trace) =
        tir_support::run_default_multi("mir_constant_identity_jit", "main.jet", FILES);
    assert_eq!(code, 0, "default JIT fixture failed: {trace}");
    assert_eq!(stdout, EXPECTED);
    assert!(
        trace
            .lines()
            .any(|line| {
                let mut fields = line.split_whitespace();
                fields.next() == Some(".::main.jet::run")
                    && fields.next() == Some("tier1")
                    && fields.next() == Some("native")
            }),
        "default run did not execute the entry natively: {trace}"
    );
    assert!(
        !trace.contains("tier0 interp") && !trace.contains("deopt"),
        "default run fell back from native execution: {trace}"
    );

    let (code, stdout, stderr) =
        run_without_tier_trace("mir_constant_identity_jit_stderr", &[]);
    assert_eq!(code, 0, "default JIT fixture failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
    assert!(
        stderr.is_empty(),
        "valid JIT program wrote to stderr: {stderr}"
    );

    let (code, stdout, stderr) =
        run_without_tier_trace("mir_constant_identity_interpreter", &["--interpret"]);
    assert_eq!(code, 0, "interpreter fixture failed: {stderr}");
    assert_eq!(stdout, EXPECTED);
    assert!(
        stderr.is_empty(),
        "valid interpreter program wrote to stderr: {stderr}"
    );

    assert_web_output("mir_constant_identity_web");
}

const PURE_FOLD_SOURCE: &str = r#"
fn square(value: Int) -> Int {
    value * value
}

fn greet(name: String) -> String {
    "hello {name}"
}

fn noisy() -> Int {
    print("noisy")
    5
}

fn run() {
    squared :: square(12)
    greeting :: greet("jet")
    total :: [4, 5, 6].reduce(0, (acc: Int, n: Int) -> acc + n)
    a :: 7
    b :: 3
    larger :: if a > b -> a * 2 else -> b
    side :: noisy()
    print("{squared} {greeting} {total} {larger} {side}")
}
"#;

/// Compile time is explicit in the checker, so an ordinary immutable binding
/// over pure work reaches MIR as a runtime computation. The MIR pure-call
/// fold must turn it back into a literal for every tier, while work with an
/// effect still runs, once, at run time.
#[test]
fn immutable_bindings_over_pure_work_fold_to_literals() {
    let scratch = common::Scratch::new("mir_pure_fold");
    fs::write(scratch.path.join("main.jet"), PURE_FOLD_SOURCE).unwrap();
    let jet = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(args)
            .arg("main.jet")
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .env("JET_STORE_DIR", scratch.path.join("cache"))
            .env("JETPACK_ROOT", scratch.path.join("jetpack"))
            .output()
            .expect("jet should launch");
        assert!(
            output.status.success(),
            "jet {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let rust = jet(&["emit", "--rust"]);
    for literal in [
        "JetInt::from_i64(144)",
        "\"hello jet\".to_string()",
        "JetInt::from_i64(15)",
        "JetInt::from_i64(14)",
    ] {
        assert!(
            rust.contains(literal),
            "a pure immutable binding did not fold to `{literal}`"
        );
    }
    assert!(
        !rust.contains("jet_list_reduce(__jet_v_"),
        "the pure reduce over a literal list still runs at run time"
    );
    for mode in [&["run"][..], &["run", "--interpret"][..]] {
        assert_eq!(jet(mode), "noisy\n144 hello jet 15 14 5\n", "jet {mode:?}");
    }
}
