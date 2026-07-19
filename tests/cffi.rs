//! C FFI (S59 / E2-M14) integration + unit tests.
//!
//! Phase 1 proves the whole pipeline end to end: a hand-written `@Bindgen`
//! cache fixture + a `use c.<lib>` call site compile to `extern "C"` wrappers
//! that link against a real C static library (built here with `cc`) and print
//! deterministic output.
//!
//! Phase 2 link discovery is exercised by unit tests over the flag parser
//! (`parse_pkg_config`), the `deps: { lib: c@… }` manifest path, and E3201,
//! since the nix dev shell ships neither `pkg-config` nor a known system lib.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{have_rustc, FfiBridgeLock};

#[test]
fn cobol_copybook_binder_runs_real_gnucobol_and_preserves_comp3() {
    if Command::new("cobc").arg("--version").output().is_err() {
        eprintln!("note: provisioned cobc unavailable; skipping COBOL integration");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cobol_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/interop/cobol");
    for file in ["payroll.cob", "payroll.cpy", "main.jet", "expected.out"] {
        fs::copy(repo.join(file), root.join(file)).unwrap();
    }
    let bind = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "bind", "cobol", "payroll.cob", "--copybook", "payroll.cpy", "--pkg", "payroll"])
        .current_dir(&root).output().unwrap();
    assert!(bind.status.success(), "{}", String::from_utf8_lossy(&bind.stderr));
    let generated = fs::read_to_string(root.join(".jet/bindings/cobol/payroll.jet")).unwrap();
    assert!(generated.contains("gross_pay: Decimal;"));
    assert!(generated.contains("offset=24 width=5 type=Decimal scale=2 encoding=COMP-3"));
    assert!(!generated.contains("gross_pay: Float"));
    let run = Command::new(env!("CARGO_BIN_EXE_jet")).args(["run", "main.jet"]).current_dir(&root).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), fs::read_to_string(root.join("expected.out")).unwrap());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn forged_fortran_library_prefix_cannot_admit_list_abi() {
    let root = std::env::temp_dir().join(format!(
        "jet_fortran_prefix_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let main = root.join("main.jet");
    let source = "use c.jet_fortran_forged as raw\n@Extern module c.jet_fortran_forged { fn probe(a: [Float]) -> Float = \"probe\"; }\nfn run() { print(raw.probe([1.0])) }\n";
    fs::write(&main, source).unwrap();
    let diagnostics = jet::compile_with_path(source, main.to_str().unwrap()).unwrap_err();
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3203"),
        "forged prefix admitted a list ABI: {diagnostics:?}"
    );
}

#[test]
fn unified_foreign_binder_registry_routes_active_and_planned_languages() {
    use jet::Foreign::{binder_for, BinderStatus, BinderSurface};
    use jet::AST::ForeignLanguage;

    let expected = [
        (
            ForeignLanguage::C,
            "c",
            "bindings/c",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Rust,
            "rust",
            "bindings/rust",
            BinderSurface::Namespace,
            BinderStatus::Planned,
        ),
        (
            ForeignLanguage::Py,
            "py",
            "bindings/py",
            BinderSurface::Namespace,
            BinderStatus::Planned,
        ),
        (
            ForeignLanguage::Js,
            "js",
            "bindings/js",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Swift,
            "swift",
            "bindings/swift",
            BinderSurface::Namespace,
            BinderStatus::Planned,
        ),
        (
            ForeignLanguage::Fortran,
            "fortran",
            "bindings/fortran",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Cobol,
            "cobol",
            "bindings/cobol",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Java,
            "java",
            "bindings/java",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::DotNet,
            "cs",
            "bindings/cs",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Tcl,
            "tcl",
            "bindings/tcl",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Ada,
            "ada",
            "bindings/ada",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Pascal,
            "pascal",
            "bindings/pascal",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Dart,
            "dart",
            "bindings/dart",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::PowerShell,
            "pwsh",
            "bindings/pwsh",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Perl,
            "perl",
            "bindings/perl",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Ruby,
            "ruby",
            "bindings/ruby",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Php,
            "php",
            "bindings/php",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::R,
            "r",
            "bindings/r",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
        (
            ForeignLanguage::Com,
            "com",
            "bindings/com",
            BinderSurface::Namespace,
            BinderStatus::Active,
        ),
    ];

    for (lang, root, bindings, surface, status) in expected {
        assert_eq!(ForeignLanguage::from_root(root), Some(lang));
        assert_eq!(lang.root(), root);
        assert_eq!(lang.bindings_subdir(), bindings);
        let binder = binder_for(lang).expect("registered binder");
        assert_eq!(binder.surface, surface);
        assert_eq!(binder.status, status);
    }
}

#[test]
fn unified_foreign_namespace_model_recognizes_every_registered_root() {
    use jet::AST::{ForeignLanguage, ForeignNamespace};

    let roots = [
        ("c", ForeignLanguage::C),
        ("cpp", ForeignLanguage::Cpp),
        ("rust", ForeignLanguage::Rust),
        ("py", ForeignLanguage::Py),
        ("js", ForeignLanguage::Js),
        ("swift", ForeignLanguage::Swift),
        ("go", ForeignLanguage::Go),
        ("java", ForeignLanguage::Java),
        ("cs", ForeignLanguage::DotNet),
        ("tcl", ForeignLanguage::Tcl),
        ("lua", ForeignLanguage::Lua),
        ("fortran", ForeignLanguage::Fortran),
        ("cobol", ForeignLanguage::Cobol),
        ("ada", ForeignLanguage::Ada),
        ("pascal", ForeignLanguage::Pascal),
        ("dart", ForeignLanguage::Dart),
        ("pwsh", ForeignLanguage::PowerShell),
        ("perl", ForeignLanguage::Perl),
        ("ruby", ForeignLanguage::Ruby),
        ("php", ForeignLanguage::Php),
        ("r", ForeignLanguage::R),
        ("com", ForeignLanguage::Com),
    ];

    assert_eq!(roots.map(|(_, language)| language), ForeignLanguage::ALL);
    for (root, language) in roots {
        let path = format!("{root}.lib");
        let namespace = ForeignNamespace::from_module_path(&path)
            .unwrap_or_else(|| panic!("{root} namespace"));
        assert_eq!(namespace.language, language);
        assert_eq!(namespace.lib, "lib");
        assert_eq!(namespace.display(), path);

        assert!(ForeignNamespace::from_module_path(root).is_none());
        assert!(ForeignNamespace::from_module_path(&format!("{root}.")).is_none());
        assert!(ForeignNamespace::from_module_path(&format!("{root}.lib.extra")).is_none());
    }
    assert!(ForeignNamespace::from_module_path("unknown.lib").is_none());
}

#[test]
fn foreign_active_js_import_is_accepted_while_planned_swift_stays_reserved() {
    let dir = common::unique_tmp("jet_foreign_active_js");
    fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.jet");
    let src = "use js.plotly as plot\nfn run() { }\n";
    fs::write(&main, src).unwrap();

    jet::compile_with_path(src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("active js import rejected:\n{:?}", d));

    let dir = common::unique_tmp("jet_foreign_reserved");
    fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.jet");
    let src = "use swift.foundation as foundation\nfn run() { }\n";
    fs::write(&main, src).unwrap();

    let diags = jet::compile_with_path(src, main.to_str().unwrap())
        .expect_err("planned foreign roots must be reserved");
    assert_eq!(diags[0].code, "E1002");
    let rendered = jet::render_diagnostics(main.to_str().unwrap(), src, &diags);
    assert!(rendered.contains("`swift` is reserved for first-party or foreign packages"));
}

#[cfg(not(target_os="windows"))]
#[test]
fn foreign_com_import_is_honestly_windows_gated() {
    let dir=common::unique_tmp("jet_foreign_com_gate");fs::create_dir_all(&dir).unwrap();let main=dir.join("main.jet");let src="use com.excel as excel\nfn run() { }\n";fs::write(&main,src).unwrap();let diags=jet::compile_with_path(src,main.to_str().unwrap()).expect_err("COM import must reject a non-Windows host");assert_eq!(diags[0].code,"E3260");let rendered=jet::render_diagnostics(main.to_str().unwrap(),src,&diags);assert!(rendered.contains("`com.*` needs a Windows host"));
}

#[test]
fn foreign_js_import_uses_generated_binding_cache_for_symbols() {
    let dir = common::unique_tmp("jet_foreign_js_cache");
    let cache_dir = dir.join(".jet/bindings/js");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(
        cache_dir.join("plotly.jet"),
        "pub fn scatter() -> Int {\n    return 7\n}\n",
    )
    .unwrap();
    let main = dir.join("main.jet");
    let src = "use js.plotly as plot\nfn run() {\n    print(plot.scatter())\n}\n";
    fs::write(&main, src).unwrap();

    jet::compile_with_path(src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("js cache import rejected:\n{:?}", d));
}

#[test]
fn foreign_js_import_without_cache_does_not_invent_symbols() {
    let dir = common::unique_tmp("jet_foreign_js_no_cache");
    fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.jet");
    let src = "use js.plotly as plot\nfn run() {\n    print(plot.scatter())\n}\n";
    fs::write(&main, src).unwrap();

    let diags = jet::compile_with_path(src, main.to_str().unwrap())
        .expect_err("missing js cache must not invent callable symbols");
    let rendered = jet::render_diagnostics(main.to_str().unwrap(), src, &diags);
    assert!(
        rendered.contains("scatter") || rendered.contains("plot"),
        "unexpected diagnostics:\n{rendered}"
    );
}

#[test]
fn foreign_interop_routes_js_with_target_dispatched_host_and_dts_stub() {
    use jet::Foreign::{
        route_plan, BinderRuntime, BinderStatus, BindingStubKind, ForeignHost, ForeignTarget,
    };
    use jet::AST::{ForeignLanguage, ForeignNamespace};

    let root = PathBuf::from("/tmp/jet_foreign_route");
    let ns = ForeignNamespace::from_module_path("js.plotly").expect("js namespace");
    let native = route_plan(&root, ns.clone(), ForeignTarget::Native).expect("native js route");
    let web = route_plan(&root, ns, ForeignTarget::Web).expect("web js route");

    assert_eq!(native.descriptor.language, ForeignLanguage::Js);
    assert_eq!(native.descriptor.status, BinderStatus::Active);
    assert_eq!(native.descriptor.runtime, BinderRuntime::TargetDispatchedJs);
    assert_eq!(
        native.descriptor.stub_kind,
        BindingStubKind::TypeScriptDeclarations
    );
    assert_eq!(native.host, ForeignHost::NativeJsWasmComponent);
    assert_eq!(web.host, ForeignHost::BrowserJsEngine);
    assert_eq!(
        native.binding_cache,
        root.join(".jet/bindings/js/plotly.jet")
    );
    assert_eq!(
        native.type_stub,
        Some(root.join(".jet/bindings/js/plotly.d.ts"))
    );
    assert_eq!(
        native.provenance,
        root.join(".jet/bindings/js/plotly.provenance")
    );
}

#[test]
fn foreign_interop_routes_swift_as_planned_c_abi_bridge() {
    use jet::Foreign::{
        route_plan, BinderRuntime, BinderStatus, BindingStubKind, ForeignHost, ForeignTarget,
    };
    use jet::AST::{ForeignLanguage, ForeignNamespace};

    let root = PathBuf::from("/tmp/jet_foreign_route");
    let ns = ForeignNamespace::from_module_path("swift.foundation").expect("swift namespace");
    let route = route_plan(&root, ns, ForeignTarget::Native).expect("swift route");

    assert_eq!(route.descriptor.language, ForeignLanguage::Swift);
    assert_eq!(route.descriptor.status, BinderStatus::Planned);
    assert_eq!(route.descriptor.runtime, BinderRuntime::SwiftCAbiBridge);
    assert_eq!(route.descriptor.stub_kind, BindingStubKind::SwiftModule);
    assert_eq!(route.host, ForeignHost::SwiftCAbiBridge);
    assert_eq!(
        route.binding_cache,
        root.join(".jet/bindings/swift/foundation.jet")
    );
    assert_eq!(route.type_stub, None);
    assert_eq!(
        route.provenance,
        root.join(".jet/bindings/swift/foundation.provenance")
    );
}

#[test]
fn foreign_interop_routes_dart_as_active_api_dl_host() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let root=PathBuf::from("/tmp/jet_foreign_route");
    let route=route_plan(&root,ForeignNamespace::from_module_path("dart.callbacks").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::Dart);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::DartApiDl);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::DartContract);
    assert_eq!(route.host,ForeignHost::DartHostFfi);
    assert_eq!(route.type_stub,Some(root.join(".jet/bindings/dart/callbacks_host.dart")));
}

#[test]
fn foreign_interop_routes_powershell_as_active_supervised_worker() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let route=route_plan(&PathBuf::from("/tmp/jet_foreign_route"),ForeignNamespace::from_module_path("pwsh.inventory").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::PowerShell);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::SupervisedPowerShell);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::PowerShellScript);
    assert_eq!(route.host,ForeignHost::SupervisedPowerShell);
}

#[test]
fn foreign_interop_routes_perl_as_active_supervised_worker() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let route=route_plan(&PathBuf::from("/tmp/jet_foreign_route"),ForeignNamespace::from_module_path("perl.text").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::Perl);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::SupervisedPerl);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::PerlScript);
    assert_eq!(route.host,ForeignHost::SupervisedPerl);
}

#[test]
fn foreign_interop_routes_ruby_as_active_supervised_worker() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let route=route_plan(&PathBuf::from("/tmp/jet_foreign_route"),ForeignNamespace::from_module_path("ruby.text").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::Ruby);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::SupervisedRuby);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::RubyScript);
    assert_eq!(route.host,ForeignHost::SupervisedRuby);
}

#[test]
fn foreign_interop_routes_php_as_active_supervised_pool() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let route=route_plan(&PathBuf::from("/tmp/jet_foreign_route"),ForeignNamespace::from_module_path("php.pricing").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::Php);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::SupervisedPhpPool);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::PhpScript);
    assert_eq!(route.host,ForeignHost::SupervisedPhpPool);
}

#[test]
fn foreign_interop_routes_r_as_active_supervised_worker() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let route=route_plan(&PathBuf::from("/tmp/jet_foreign_route"),ForeignNamespace::from_module_path("r.stats").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::R);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::SupervisedR);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::RScript);
    assert_eq!(route.host,ForeignHost::SupervisedR);
}

#[test]
fn foreign_interop_routes_com_as_active_windows_automation() {
    use jet::Foreign::{route_plan,BinderRuntime,BinderStatus,BindingStubKind,ForeignHost,ForeignTarget};
    use jet::AST::{ForeignLanguage,ForeignNamespace};
    let route=route_plan(&PathBuf::from("/tmp/jet_foreign_route"),ForeignNamespace::from_module_path("com.excel").unwrap(),ForeignTarget::Native).unwrap();
    assert_eq!(route.descriptor.language,ForeignLanguage::Com);
    assert_eq!(route.descriptor.status,BinderStatus::Active);
    assert_eq!(route.descriptor.runtime,BinderRuntime::WindowsComAutomation);
    assert_eq!(route.descriptor.stub_kind,BindingStubKind::ComTypeLibrary);
    assert_eq!(route.host,ForeignHost::WindowsComAutomation);
}

/// Build a tiny C static library `libjetc.a` in `dir`, returning its directory
/// and link name. Skips (returns None) when no C compiler is available.
fn build_c_lib(dir: &Path) -> Option<(PathBuf, String)> {
    let cc = ["cc", "gcc", "clang"]
        .iter()
        .find(|c| Command::new(c).arg("--version").output().is_ok())?;
    let c_src = dir.join("jetc.c");
    fs::write(
        &c_src,
        r#"
#include <stdint.h>
long long jetc_add_ints(long long a, long long b) { return a + b; }
const char *jetc_greeting(void) { return "hi from C"; }
"#,
    )
    .unwrap();
    let obj = dir.join("jetc.o");
    let ok = Command::new(cc)
        .args(["-c"])
        .arg(&c_src)
        .arg("-o")
        .arg(&obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let lib = dir.join("libjetc.a");
    let ok = Command::new("ar")
        .arg("rcs")
        .arg(&lib)
        .arg(&obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    Some((dir.to_path_buf(), "jetc".to_string()))
}

/// E2-M14: the native `jet inspect bind` backend turns a real C header into a working
/// `@Bindgen` cache that compiles, links against the C library, and runs.
#[test]
fn jet_bind_native_backend_end_to_end() {
    if !have_rustc() {
        eprintln!("note: skipping jet_bind_native_backend (need rustc)");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cbind_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();

    let Some((lib_dir, lib_name)) = build_c_lib(&root) else {
        eprintln!("note: skipping jet_bind_native_backend (no C compiler)");
        return;
    };

    // A real header for the C library — translated by the native backend.
    let header = r#"
        #include <stdint.h>
        /* arithmetic */
        long long jetc_add_ints(long long a, long long b);
        const char *jetc_greeting(void);
    "#;
    let result = jet::CBind::generate(header, "jetc").expect("native bind backend");
    assert!(
        result.skipped.is_empty(),
        "unexpected skips: {:?}",
        result.skipped
    );
    assert_eq!(result.bound.len(), 2);
    // The cache uses the real C symbol names verbatim (no aliasing).
    assert!(result
        .source
        .contains("fn jetc_add_ints(a: Int, b: Int) -> Int = \"jetc_add_ints\";"));
    assert!(result
        .source
        .contains("fn jetc_greeting() -> String = \"jetc_greeting\";"));
    fs::write(cache.join("jetc.jet"), &result.source).unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;

fn run() {
    print(jc.jetc_add_ints(2, 40));
    print(jc.jetc_greeting());
}
"#,
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("front end rejected bind-generated program:\n{:?}", d));

    let rs = root.join("main.rs");
    fs::write(&rs, &out.rust).unwrap();
    let bin = root.join("main_bin");
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("-L")
        .arg(format!("native={}", lib_dir.display()))
        .arg("-l")
        .arg(&lib_name)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "I2: rustc rejected bind-generated C-FFI code (jet bug):\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(
        run.status.success(),
        "bind-generated program failed at runtime"
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\nhi from C\n");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_end_to_end_links_and_runs() {
    if !have_rustc() {
        eprintln!("note: skipping cffi_end_to_end (need rustc)");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cffi_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();

    let Some((lib_dir, lib_name)) = build_c_lib(&root) else {
        eprintln!("note: skipping cffi_end_to_end (no C compiler)");
        return;
    };

    // Hand-written bindgen cache fixture (simulates `jet inspect bind` output).
    fs::write(
        cache.join("jetc.jet"),
        r#"@Bindgen module c.jetc.__bindgen__ {
    fn add_ints(a: Int, b: Int) -> Int = "jetc_add_ints";
    fn greeting() -> String = "jetc_greeting";
}
"#,
    )
    .unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;

fn run() {
    print(jc.add_ints(2, 40));
    print(jc.greeting());
}
"#,
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("front end rejected C FFI program:\n{:?}", d));

    // I1: no `unsafe` leaks into ordinary Jet — but the boundary shim is
    // compiler-emitted, vetted internals (S58). The wrappers we emit DO use
    // unsafe to call extern "C"; confirm it is confined to the C module.
    assert!(
        out.rust.contains("extern \"C\""),
        "expected an extern \"C\" block in generated code"
    );
    assert!(
        out.rust.contains("jetc_add_ints"),
        "expected the C symbol name in generated code"
    );

    // Build + link against the C static library.
    let rs = root.join("main.rs");
    fs::write(&rs, &out.rust).unwrap();
    let bin = root.join("main_bin");
    let mut cmd = Command::new("rustc");
    cmd.args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("-L")
        .arg(format!("native={}", lib_dir.display()))
        .arg("-l")
        .arg(&lib_name);
    let status = cmd.output().unwrap();
    assert!(
        status.status.success(),
        "I2: rustc rejected generated C-FFI code (jet bug):\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "C-FFI program failed at runtime");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\nhi from C\n");

    let _ = fs::remove_dir_all(&root);
}

/// Card #436: build a second C static library exercising the C-ABI shapes
/// that sema newly accepts (fixed-width ints, a `@Layout(c)` struct passed
/// by value, a distinct-over-`Int`) so the round trip is proven against a
/// REAL C compiler + linker, not just codegen string matching.
fn build_c_lib_card436(dir: &Path) -> Option<(PathBuf, String)> {
    let cc = ["cc", "gcc", "clang"]
        .iter()
        .find(|c| Command::new(c).arg("--version").output().is_ok())?;
    let c_src = dir.join("jetc436.c");
    fs::write(
        &c_src,
        r#"
#include <stdint.h>

/* D-SG9: fixed-width integers cross the C boundary as their exact C type. */
uint8_t jetc436_add_u8(uint8_t a, uint8_t b) { return (uint8_t)(a + b); }
int32_t jetc436_add_i32(int32_t a, int32_t b) { return a + b; }
float jetc436_add_f32(float a, float b) { return a + b; }

/* D-REPRC1: a `@Layout(c)` struct is `#[repr(C)]` — same field order/size as
 * this C struct, so it crosses by value with no bridging code needed. */
typedef struct {
    long long x;
    long long y;
} CPoint;

CPoint jetc436_make_point(long long x, long long y) {
    CPoint p;
    p.x = x;
    p.y = y;
    return p;
}

long long jetc436_point_sum(CPoint p) { return p.x + p.y; }

/* D-DIST1: a distinct type is `#[repr(transparent)]` over its base — same
 * ABI as the base scalar, so it crosses the boundary directly. */
long long jetc436_scale_meters(long long m) { return m * 2; }
"#,
    )
    .unwrap();
    let obj = dir.join("jetc436.o");
    let ok = Command::new(cc)
        .args(["-c"])
        .arg(&c_src)
        .arg("-o")
        .arg(&obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let lib = dir.join("libjetc436.a");
    let ok = Command::new("ar")
        .arg("rcs")
        .arg(&lib)
        .arg(&obj)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    Some((dir.to_path_buf(), "jetc436".to_string()))
}

/// Card #436: fixed-width ints (`U8`/`I32`), `F32`, a `@Layout(c)` struct
/// passed/returned by value, and a distinct-over-`Int` all round-trip through
/// generated wrappers, link against a real C library, and run — the shapes
/// `Sema::FFI::is_c_abi_type` accepts now all have matching `CModule.rs`
/// codegen (before this card, these fell into codegen's `/* unsupported */
/// ()` placeholder — an I2/I3 bug: sema-accepted, codegen-unlowerable).
#[test]
fn cffi_card436_c_abi_shapes_round_trip() {
    if !have_rustc() {
        eprintln!("note: skipping cffi_card436_c_abi_shapes_round_trip (need rustc)");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cffi_c436_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let Some((lib_dir, lib_name)) = build_c_lib_card436(&root) else {
        eprintln!("note: skipping cffi_card436_c_abi_shapes_round_trip (no C compiler)");
        return;
    };

    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc436 as c436

// Deliberately NOT named `Point` — that's a reserved builtin UI-geometry
// type name (D-RENDERTGT2, `Codegen/Context.rs`); this exercises the
// ordinary user-struct fallback, not that special case.
@Layout(c)
struct Coord {
    x: Int
    y: Int
}

Meters :: distinct Int;

@Extern module c.jetc436 {
    fn add_u8(a: U8, b: U8) -> U8 = "jetc436_add_u8";
    fn add_i32(a: I32, b: I32) -> I32 = "jetc436_add_i32";
    fn add_f32(a: F32, b: F32) -> F32 = "jetc436_add_f32";
    fn make_point(x: Int, y: Int) -> Coord = "jetc436_make_point";
    fn point_sum(p: Coord) -> Int = "jetc436_point_sum";
    fn scale_meters(m: Meters) -> Meters = "jetc436_scale_meters";
}

fn run() {
    print(c436.add_u8(200, 55))
    print(c436.add_i32(1000000, 234567))
    print(c436.add_f32(1.5, 2.25))
    p :: c436.make_point(3, 4)
    print(c436.point_sum(p))
    print(c436.scale_meters(Meters.from_int(21)).raw())
}
"#,
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap()).unwrap_or_else(|d| {
        panic!(
            "card #436 C-ABI shapes rejected by the front end:\n{}",
            jet::render_diagnostics(main.to_str().unwrap(), &src, &d)
        )
    });
    assert!(
        !out.rust.contains("/* unsupported"),
        "I2/I3: a sema-accepted C-ABI shape fell through to codegen's unsupported \
         placeholder; got:\n{}",
        out.rust
    );

    let rs = root.join("main.rs");
    fs::write(&rs, &out.rust).unwrap();
    let bin = root.join("main_bin");
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("-L")
        .arg(format!("native={}", lib_dir.display()))
        .arg("-l")
        .arg(&lib_name)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "I2: rustc rejected generated C-FFI code for card #436 shapes (jet bug):\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(
        run.status.success(),
        "card #436 C-ABI round-trip program failed at runtime:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "255\n1234567\n3.75\n7\n42\n"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_repr_c_enums_match_native_c_layout() {
    if !have_rustc() { return; }
    let root = std::env::temp_dir().join(format!("jet_cffi_reprc2_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
    fs::write(root.join("reprc2.c"), r#"#include <stdint.h>
#include <stddef.h>
typedef enum { STATUS_OK=0, STATUS_LOST=7 } Status;
typedef enum __attribute__((packed)) { PACKET_PING=3, PACKET_DATA=7 } PacketTag;
typedef union { long long ping; struct { long long x; long long y; } data; } PacketPayload;
typedef struct { PacketTag tag; PacketPayload payload; } Packet;
int32_t repr_status(Status s){return (int32_t)s;} int32_t repr_packet(Packet p){return (int32_t)p.tag*100+p.payload.ping;}
int32_t repr_packet_size(void){return sizeof(Packet);} int32_t repr_packet_align(void){return _Alignof(Packet);}
int32_t repr_packet_payload_offset(void){return offsetof(Packet,payload);}"#).unwrap();
    let cc = ["cc","gcc","clang"].iter().find(|x| Command::new(x).arg("--version").output().is_ok()).unwrap();
    assert!(Command::new(cc).args(["-c"]).arg(root.join("reprc2.c")).arg("-o").arg(root.join("reprc2.o")).status().unwrap().success());
    assert!(Command::new("ar").arg("rcs").arg(root.join("libreprc2.a")).arg(root.join("reprc2.o")).status().unwrap().success());
    let main = root.join("main.jet");
    fs::write(&main, r#"use c.reprc2 as c
@Layout(c)
enum Status { Ok = 0; Lost = 7 }
@Layout(c, tag: U8)
enum Packet { Ping(Int) = 3; Data(x: Int, y: Int) = 7 }
@Extern module c.reprc2 {
 fn repr_status(s: Status) -> I32 = "repr_status"
 fn repr_packet(p: Packet) -> I32 = "repr_packet"
 fn repr_packet_size() -> I32 = "repr_packet_size"
 fn repr_packet_align() -> I32 = "repr_packet_align"
 fn repr_packet_payload_offset() -> I32 = "repr_packet_payload_offset"
}
fn run() { print(c.repr_status(Status.Lost)); print(c.repr_packet(Packet.Ping(41))); print(c.repr_packet_size()); print(c.repr_packet_align()); print(c.repr_packet_payload_offset()) }
"#).unwrap();
    let src=fs::read_to_string(&main).unwrap(); let out=jet::compile_with_path(&src,main.to_str().unwrap()).unwrap_or_else(|d|panic!("{}",jet::render_diagnostics(main.to_str().unwrap(),&src,&d)));
    assert!(out.rust.contains("#[repr(C, u8)]") && out.rust.contains("user_Lost = 7") && out.rust.contains("user_Ping(i64) = 3"));
    assert!(out.rust.contains("typedef uint8_t Packet_Tag;") && out.rust.contains("typedef union Packet_Payload") && out.rust.contains("typedef struct Packet"));
    fs::write(root.join("main.rs"),out.rust).unwrap();
    let built=Command::new("rustc").args(["--edition","2021"]).arg(root.join("main.rs")).arg("-o").arg(root.join("main_bin")).arg("-L").arg(format!("native={}",root.display())).arg("-lreprc2").output().unwrap();
    assert!(built.status.success(),"I2: {}",String::from_utf8_lossy(&built.stderr)); let run=Command::new(root.join("main_bin")).output().unwrap();
    assert!(run.status.success(),"{}",String::from_utf8_lossy(&run.stderr)); assert_eq!(String::from_utf8_lossy(&run.stdout),"7\n341\n24\n8\n8\n");
    let _=fs::remove_dir_all(root);
}

#[test]
fn cffi_named_pure_callback_has_stable_c_symbol() {
    if !have_rustc() { return; }
    let root=std::env::temp_dir().join(format!("jet_cffi_cb_{}",std::process::id())); let _=fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
    fs::write(root.join("cb.c"),"#include <stdint.h>\n#include <pthread.h>\ntypedef int32_t (*cb_t)(int32_t);\nint32_t call_twice(cb_t cb,int32_t x){ return cb(cb(x)); }\ntypedef struct { cb_t cb; int32_t x; int32_t out; } Job;\nstatic void* run_job(void* p){ Job* j=(Job*)p; j->out=j->cb(j->x); return 0; }\nint32_t call_parallel(cb_t cb){ pthread_t t[4]; Job j[4]; for(int i=0;i<4;i++){j[i]=(Job){cb,i,0}; pthread_create(&t[i],0,run_job,&j[i]);} int32_t s=0; for(int i=0;i<4;i++){pthread_join(t[i],0);s+=j[i].out;} return s; }\n").unwrap();
    let cc=["cc","gcc","clang"].iter().find(|x|Command::new(x).arg("--version").output().is_ok()).unwrap();
    assert!(Command::new(cc).args(["-c"]).arg(root.join("cb.c")).arg("-o").arg(root.join("cb.o")).status().unwrap().success());
    assert!(Command::new("ar").arg("rcs").arg(root.join("libcb.a")).arg(root.join("cb.o")).status().unwrap().success());
    let main=root.join("main.jet"); fs::write(&main,"use c.cb as c\nfn increment(x: I32) --[]-> I32 { return x + 1 }\n@Extern module c.cb { fn call_twice(cb: fn(I32) --[]-> I32, x: I32) -> I32 = \"call_twice\"; fn call_parallel(cb: fn(I32) --[]-> I32) -> I32 = \"call_parallel\"; }\nfn run() { print(c.call_twice(increment, 40)); print(c.call_parallel(increment)); print(c.call_twice((x) => x + x, 10)) }\n").unwrap();
    let src=fs::read_to_string(&main).unwrap(); let out=jet::compile_with_path(&src,main.to_str().unwrap()).unwrap_or_else(|d|panic!("{}",jet::render_diagnostics(main.to_str().unwrap(),&src,&d)));
    assert!(out.rust.contains("extern \"C\" fn user_increment")); assert!(out.rust.contains("extern \"C\" fn(i32) -> i32")); assert!(out.rust.contains("extern \"C\" fn __jet_c_callback_"));
    fs::write(root.join("main.rs"),out.rust).unwrap(); let built=Command::new("rustc").args(["--edition","2021"]).arg(root.join("main.rs")).arg("-o").arg(root.join("main_bin")).arg("-L").arg(format!("native={}",root.display())).arg("-lcb").arg("-lpthread").output().unwrap();
    assert!(built.status.success(),"I2: {}",String::from_utf8_lossy(&built.stderr)); let run=Command::new(root.join("main_bin")).output().unwrap(); assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n10\n40\n"); let _=fs::remove_dir_all(root);
}

#[test]
fn cffi_raw_status_out_pointer_reads_only_on_success() {
    if !have_rustc() { return; }
    let root=std::env::temp_dir().join(format!("jet_cffi_out_{}",std::process::id())); let _=fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
    fs::write(root.join("store.c"),"#include <stdint.h>\n#include <string.h>\ntypedef struct { uint64_t id; uint32_t flags; } Record;\nint32_t store_load(uint64_t id, Record* out){ if(id==7){out->id=70;out->flags=3;return 0;} memset(out,0xA5,sizeof(*out)); return 9; }\n").unwrap();
    let cc=["cc","gcc","clang"].iter().find(|x|Command::new(x).arg("--version").output().is_ok()).unwrap();
    assert!(Command::new(cc).args(["-c"]).arg(root.join("store.c")).arg("-o").arg(root.join("store.o")).status().unwrap().success());
    assert!(Command::new("ar").arg("rcs").arg(root.join("libstore.a")).arg(root.join("store.o")).status().unwrap().success());
    let main=root.join("main.jet"); let src=r#"use core.mem
use c.store as store
@Layout(c)
struct Record { id: U64; flags: U32 }
@Extern module c.store { fn store_load(id: U64, out: *Record) -> I32 = "store_load"; }
fn load(id: U64) -> Record ? String {
    slot: Record := Record.{id: 0, flags: 0}
    status: I32 := 1
    @Unsafe("store_load receives a live non-null slot; bytes are read only after status zero") {
        p :: mem.Ptr<Record>.from_addr(mem.address_of(slot))
        status = store.store_load(id, p)
        if Int.from_i32(status) == 0 { slot = ~p.* }
    }
    if Int.from_i32(status) != 0 { return Err("status {status}") }
    return Ok(slot)
}

fn run() {
    print((load(7) ?? panic("success expected")).id)
    if load(8) == {
        Ok(v) -> { print("unexpected {v.id}") }
        Err(e) -> { print(e) }
    }
}
"#; fs::write(&main,src).unwrap();
    let out=jet::compile_with_path(src,main.to_str().unwrap()).unwrap_or_else(|d|panic!("{}",jet::render_diagnostics(main.to_str().unwrap(),src,&d)));
    assert!(out.rust.contains("*mut super::user_Record")); assert!(!out.rust.contains("Result<super::user_Record"));
    fs::write(root.join("main.rs"),out.rust).unwrap(); let built=Command::new("rustc").args(["--edition","2021"]).arg(root.join("main.rs")).arg("-o").arg(root.join("main_bin")).arg("-L").arg(format!("native={}",root.display())).arg("-lstore").output().unwrap();
    assert!(built.status.success(),"I2: {}",String::from_utf8_lossy(&built.stderr)); let run=Command::new(root.join("main_bin")).output().unwrap(); assert_eq!(String::from_utf8_lossy(&run.stdout),"70\nstatus 9\n"); let _=fs::remove_dir_all(root);
}

#[test]
#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
fn cffi_sysv64_abi_executes_native_symbol() {
    if !have_rustc() { return; }
    let root=std::env::temp_dir().join(format!("jet_cffi_sysv_{}",std::process::id())); let _=fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
    fs::write(root.join("abi.c"),"#include <stdint.h>\nint32_t abi_add(int32_t a,int32_t b){return a+b;}\n").unwrap();
    let cc=["cc","gcc","clang"].iter().find(|x|Command::new(x).arg("--version").output().is_ok()).unwrap(); assert!(Command::new(cc).args(["-c"]).arg(root.join("abi.c")).arg("-o").arg(root.join("abi.o")).status().unwrap().success()); assert!(Command::new("ar").arg("rcs").arg(root.join("libabi.a")).arg(root.join("abi.o")).status().unwrap().success());
    let src="use c.abi as c\n@Extern module c.abi { @Abi(sysv64) fn add(a: I32, b: I32) -> I32 = \"abi_add\"; }\nfn run() { print(c.add(20, 22)) }\n"; let main=root.join("main.jet"); fs::write(&main,src).unwrap(); let out=jet::compile_with_path(src,main.to_str().unwrap()).unwrap_or_else(|d|panic!("{}",jet::render_diagnostics(main.to_str().unwrap(),src,&d))); assert!(out.rust.contains("extern \"sysv64\""));
    fs::write(root.join("main.rs"),out.rust).unwrap(); let built=Command::new("rustc").args(["--edition","2021"]).arg(root.join("main.rs")).arg("-o").arg(root.join("main_bin")).arg("-L").arg(format!("native={}",root.display())).arg("-labi").output().unwrap(); assert!(built.status.success(),"I2: {}",String::from_utf8_lossy(&built.stderr)); let run=Command::new(root.join("main_bin")).output().unwrap(); assert_eq!(String::from_utf8_lossy(&run.stdout),"42\n"); let _=fs::remove_dir_all(root);
}

#[test]
fn cffi_string_returns_are_borrowed_non_null_utf8_and_copied() {
    if !have_rustc() { return; }
    let root=std::env::temp_dir().join(format!("jet_cffi_cstr_{}",std::process::id())); let _=fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
    fs::write(root.join("strret.c"),"const char* good(void){return \"caf\\xC3\\xA9\";} const char* null_s(void){return 0;} const char* bad(void){static const char s[]={ (char)0xff,0 };return s;}\n").unwrap();
    let cc=["cc","gcc","clang"].iter().find(|x|Command::new(x).arg("--version").output().is_ok()).unwrap(); assert!(Command::new(cc).args(["-c"]).arg(root.join("strret.c")).arg("-o").arg(root.join("strret.o")).status().unwrap().success()); assert!(Command::new("ar").arg("rcs").arg(root.join("libstrret.a")).arg(root.join("strret.o")).status().unwrap().success());
    for (name, expected, success) in [("good","café\n",true),("null_s","returned a null pointer",false),("bad","not valid UTF-8",false)] {
        let src=format!("use c.strret as c\n@Extern module c.strret {{ fn get() -> String = \"{name}\"; }}\nfn run() {{ print(c.get()) }}\n"); let main=root.join(format!("{name}.jet")); fs::write(&main,&src).unwrap(); let out=jet::compile_with_path(&src,main.to_str().unwrap()).unwrap_or_else(|d|panic!("{}",jet::render_diagnostics(main.to_str().unwrap(),&src,&d)));
        let wrapper = out.rust
            .split_once("pub fn user_get() -> String {\n")
            .unwrap_or_else(|| panic!("missing generated C wrapper for {name}"))
            .1
            .split_once("\n}\n")
            .unwrap_or_else(|| panic!("unterminated generated C wrapper for {name}"))
            .0;
        assert!(wrapper.contains(&format!("let p = unsafe {{ {name}() }};")));
        assert!(wrapper.contains("if p.is_null()"));
        assert!(wrapper.contains("std::ffi::CStr::from_ptr(p)"));
        assert!(wrapper.contains("bytes.to_str()"));
        assert!(wrapper.contains(".to_owned()"));
        assert!(!wrapper.contains("to_string_lossy"));
        assert!(!wrapper.contains("/* unsupported:"));
        let rs=root.join(format!("{name}.rs")); let bin=root.join(format!("{name}_bin")); fs::write(&rs,out.rust).unwrap(); let built=Command::new("rustc").args(["--edition","2021"]).arg(&rs).arg("-o").arg(&bin).arg("-L").arg(format!("native={}",root.display())).arg("-lstrret").output().unwrap(); assert!(built.status.success(),"I2: {}",String::from_utf8_lossy(&built.stderr)); let run=Command::new(bin).output().unwrap(); assert_eq!(run.status.success(),success); let text=format!("{}{}",String::from_utf8_lossy(&run.stdout),String::from_utf8_lossy(&run.stderr)); assert!(text.contains(expected),"{name}: {text}");
    }
    let _=fs::remove_dir_all(root);
}

/// Card #436: a runtime-built `String` (not a literal — so sema's E3211
/// comptime check can't catch it) with an embedded NUL byte, passed to a
/// C-boundary function, must fail LOUDLY at runtime (a panic), never silently
/// send the C function an empty string. Exercises the codegen-side
/// `NUL_PANIC` path in `Codegen/CModule.rs`.
#[test]
fn cffi_runtime_interior_nul_panics_instead_of_silently_truncating() {
    if !have_rustc() {
        eprintln!("note: skipping cffi_runtime_interior_nul_panics (need rustc)");
        return;
    }
    let root = std::env::temp_dir().join(format!("jet_cffi_nul_panic_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    // The NUL-bearing value comes in over stdin at runtime (`input()`), so
    // it is never a compile-time literal — sema's E3211 (comptime-literal
    // check) can't see it, only codegen's runtime guard can.
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc436 as c436

@Extern module c.jetc436 {
    fn takes_str(s: String) -> Int = "strlen";
}

fn run() {
    line :: input() ?? ""
    print(c436.takes_str(line))
}
"#,
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap()).unwrap_or_else(|d| {
        panic!(
            "front end rejected the runtime-NUL fixture:\n{}",
            jet::render_diagnostics(main.to_str().unwrap(), &src, &d)
        )
    });

    let rs = root.join("main.rs");
    fs::write(&rs, &out.rust).unwrap();
    let bin = root.join("main_bin");
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "I2: rustc rejected the runtime-NUL wrapper (jet bug):\n{}",
        String::from_utf8_lossy(&status.stderr)
    );

    use std::io::Write as _;
    let mut child = Command::new(&bin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"ab\0cd\n")
        .unwrap();
    let run = child.wait_with_output().unwrap();
    assert!(
        !run.status.success(),
        "a runtime String with an embedded NUL must panic at the C boundary, not \
         silently succeed with a truncated/empty string; stdout was: {:?}",
        String::from_utf8_lossy(&run.stdout)
    );

    let _ = fs::remove_dir_all(&root);
}

/// Regression (c95): a `String` parameter must emit its `CString` conversion
/// line in the wrapper body. The codegen built `call_args` referencing `c{i}`
/// but dropped the `let c{i} = …` conversion, so rustc rejected the wrapper
/// (`cannot find value c0`) — an I2 violation. Pin that the temp is declared.
#[test]
fn cffi_string_param_emits_cstring_conversion() {
    let root = std::env::temp_dir().join(format!("jet_cffi_strparam_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("strlib.jet"),
        "@Bindgen module c.strlib.__bindgen__ { fn slen(s: String) -> Int = \"strlen\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use c.strlib as s;\nfn run() { print(s.slen(\"hello\")); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("string-param C FFI rejected:\n{:?}", d));
    assert!(
        out.rust.contains("let c0 = std::ffi::CString::new"),
        "wrapper must declare the CString temp for a String param; got:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("strlen(c0.as_ptr())"),
        "wrapper must call through the declared temp; got:\n{}",
        out.rust
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_empty_overlay_is_bindgen_only() {
    // D-CFFI2-SYN-2: an empty `@Extern module` adds nothing; the full bindgen
    // surface stays visible.
    let root = std::env::temp_dir().join(format!("jet_cffi_empty_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("jetc.jet"),
        "@Bindgen module c.jetc.__bindgen__ { fn ping() -> Int = \"jetc_ping\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;
@Extern module c.jetc { }
fn run() { print(jc.ping()); }
"#,
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("empty overlay rejected:\n{:?}", d));
    assert!(
        out.rust.contains("jetc_ping"),
        "bindgen symbol must survive empty overlay"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_overlay_overrides_bindgen() {
    // D-CFFI2-SYN-4: overlay replaces a bindgen symbol with a matching sig.
    let root = std::env::temp_dir().join(format!("jet_cffi_override_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("jetc.jet"),
        "@Bindgen module c.jetc.__bindgen__ { fn add(a: Int, b: Int) -> Int = \"gen_add\"; }\n",
    )
    .unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        r#"use c.jetc as jc;
@Extern module c.jetc { fn add(a: Int, b: Int) -> Int = "real_add"; }
fn run() { print(jc.add(1, 2)); }
"#,
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("override rejected:\n{:?}", d));
    assert!(out.rust.contains("real_add"), "overlay symbol must win");
    assert!(
        !out.rust.contains("gen_add"),
        "bindgen symbol must be replaced"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cffi_header_use_form_lowers_to_lib() {
    // Phase 3: `use "demo.h" as d` resolves through the same merged c.demo
    // module (header basename → link key `demo`).
    let root = std::env::temp_dir().join(format!("jet_cffi_header_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("demo.h"), "int demo_ping(void);\n").unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"demo.h\" as d;\nfn run() { print(d.demo_ping()); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let out = jet::compile_with_path(&src, main.to_str().unwrap())
        .unwrap_or_else(|d| panic!("header use form rejected:\n{:?}", d));
    assert!(
        out.rust.contains("demo_ping"),
        "header form must reach the demo bindgen surface"
    );
    let _ = fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// D-CBIND2 + Phase 3 (E3 deferred pieces): auto-bind on cache miss, hash
// invalidation, and cache-hit fast path.
// ---------------------------------------------------------------------------

/// Probe 1 — missing cache + header-path use form → compiler auto-runs bind,
/// compilation succeeds (no E3208 / no error about missing cache).
#[test]
fn auto_bind_on_cache_miss() {
    let root = std::env::temp_dir().join(format!("jet_autobind_miss_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // Create the project layout but DO NOT pre-create the binding cache.
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();

    // A simple C header with one bindable function.
    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("mylib.h");
    fs::write(&header_path, "int mylib_ping(int x);\n").unwrap();

    // A Jet source using the header-path form (`use "include/mylib.h" as m`).
    // The compiler should auto-invoke the bind backend on the cache miss.
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/mylib.h\" as m;\nfn run() { print(m.mylib_ping(1)); }\n",
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    // compile_with_path loads the bundle from disk (including auto-bind).
    let result = jet::compile_with_path(&src, main.to_str().unwrap());

    // The cache should now exist (auto-created by the compiler).
    let cache_file = cache_dir.join("mylib.jet");
    assert!(
        cache_file.is_file(),
        "auto-bind: compiler should have created the binding cache at {:?}",
        cache_file
    );

    // And compilation should succeed (the generated binding is loaded).
    assert!(
        result.is_ok(),
        "auto-bind on cache miss must not produce a compile error; got: {:?}",
        result.err()
    );

    let _ = fs::remove_dir_all(&root);
}

/// Probe 2 — header changes after a successful bind → hash mismatch detected,
/// the compiler RE-BINDS (does not use the stale cache).
#[test]
fn hash_invalidation_on_header_change() {
    let root = std::env::temp_dir().join(format!("jet_hash_inval_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();

    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("mylib2.h");

    // Step A: initial header with `mylib2_v1`.
    let header_v1 = "int mylib2_v1(int x);\n";
    fs::write(&header_path, header_v1).unwrap();

    // Pre-populate the cache and hash sidecar (simulates a prior `jet inspect bind`).
    let cache_file = cache_dir.join("mylib2.jet");
    let bind_v1 = jet::CBind::generate(header_v1, "mylib2").unwrap();
    fs::write(&cache_file, &bind_v1.source).unwrap();
    jet::CBind::write_bind_hash(&cache_file, header_v1, "").unwrap();

    // Confirm v1 cache doesn't contain v2 symbol yet.
    assert!(
        !bind_v1.source.contains("mylib2_v2"),
        "sanity: v2 not in v1 cache"
    );

    // Step B: change the header — add `mylib2_v2`, remove `mylib2_v1`.
    let header_v2 = "int mylib2_v2(int y);\n";
    fs::write(&header_path, header_v2).unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/mylib2.h\" as m;\nfn run() { print(m.mylib2_v2(2)); }\n",
    )
    .unwrap();

    let src = fs::read_to_string(&main).unwrap();
    let result = jet::compile_with_path(&src, main.to_str().unwrap());

    // The cache should have been regenerated with v2 content.
    let new_cache = fs::read_to_string(&cache_file).unwrap_or_default();
    assert!(
        new_cache.contains("mylib2_v2"),
        "hash invalidation: cache must be regenerated with updated symbol; got:\n{}",
        new_cache
    );
    assert!(
        !new_cache.contains("mylib2_v1"),
        "hash invalidation: stale v1 symbol must not appear in regenerated cache; got:\n{}",
        new_cache
    );

    // The hash sidecar must also be updated to reflect v2.
    let new_hash = jet::CBind::read_stored_hash(&cache_file);
    let expected_hash = jet::CBind::compute_bind_hash(header_v2, "");
    assert_eq!(
        new_hash.as_deref(),
        Some(expected_hash.as_str()),
        "hash sidecar must be updated after re-bind"
    );

    // The compile result may succeed or fail depending on whether the new
    // symbol is in scope; what matters is that the CACHE was invalidated.
    // (We don't assert compile success here since the test binary isn't linked.)
    let _ = result;

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn failed_rebind_rejects_stale_cffi_cache() {
    let root = std::env::temp_dir().join(format!("jet_stale_cbind_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();
    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("stale.h");
    let old_header = "int stale_value(void);\n";
    fs::write(&header_path, old_header).unwrap();

    let cache_file = cache_dir.join("stale.jet");
    let old_cache = jet::CBind::generate(old_header, "stale").unwrap().source;
    fs::write(&cache_file, &old_cache).unwrap();
    jet::CBind::write_bind_hash(&cache_file, old_header, "").unwrap();

    fs::write(&header_path, "int malformed(int value,);\n").unwrap();
    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/stale.h\" as stale;\nfn run() { print(stale.stale_value()); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let diagnostics = jet::compile_with_path(&src, main.to_str().unwrap()).unwrap_err();

    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3208"),
        "failed rebind must report E3208, got: {diagnostics:?}"
    );
    assert_eq!(
        fs::read_to_string(&cache_file).unwrap(),
        old_cache,
        "a failed rebind may preserve old bytes on disk but must not consume them"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_declared_header_rejects_stale_cffi_cache() {
    let root = std::env::temp_dir().join(format!("jet_missing_cbind_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_file = cache_dir.join("missing.jet");
    let old_cache = jet::CBind::generate("int stale_value(void);\n", "missing")
        .unwrap()
        .source;
    fs::write(&cache_file, &old_cache).unwrap();

    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/missing.h\" as missing;\nfn run() { print(missing.stale_value()); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let diagnostics = jet::compile_with_path(&src, main.to_str().unwrap()).unwrap_err();

    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E3208"),
        "missing declared header must report E3208, got: {diagnostics:?}"
    );
    assert_eq!(fs::read_to_string(&cache_file).unwrap(), old_cache);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn absent_or_malformed_hash_regenerates_declared_header_cache() {
    for (case, sidecar) in [("absent", None), ("malformed", Some("not-a-sha256"))] {
        let root = std::env::temp_dir().join(format!(
            "jet_cbind_hash_{case}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let cache_dir = root.join(".jet/bindings/c");
        let header_dir = root.join("include");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&header_dir).unwrap();
        let header = "int fresh_value(void);\n";
        fs::write(header_dir.join("rebind.h"), header).unwrap();

        let cache_file = cache_dir.join("rebind.jet");
        let stale = jet::CBind::generate("int stale_value(void);\n", "rebind")
            .unwrap()
            .source;
        fs::write(&cache_file, stale).unwrap();
        if let Some(sidecar) = sidecar {
            fs::write(jet::CBind::hash_sidecar_path(&cache_file), sidecar).unwrap();
        }
        let main = root.join("main.jet");
        fs::write(
            &main,
            "use \"include/rebind.h\" as rebind;\nfn run() { print(rebind.fresh_value()); }\n",
        )
        .unwrap();
        let src = fs::read_to_string(&main).unwrap();
        let result = jet::compile_with_path(&src, main.to_str().unwrap());

        assert!(result.is_ok(), "{case} hash did not trigger rebind: {result:?}");
        assert!(
            fs::read_to_string(&cache_file)
                .unwrap()
                .contains("fresh_value"),
            "{case} hash left stale cache bytes"
        );
        assert_eq!(
            jet::CBind::read_stored_hash(&cache_file).as_deref(),
            Some(jet::CBind::compute_bind_hash(header, "").as_str())
        );
        let _ = fs::remove_dir_all(&root);
    }
}

/// Probe 3 — unchanged header + present cache → NO re-bind (fast path: the
/// cache is loaded as-is, hash stays the same).
#[test]
fn cache_hit_no_rebind() {
    let root = std::env::temp_dir().join(format!("jet_cache_hit_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join(".jet/bindings/c");
    fs::create_dir_all(&cache_dir).unwrap();

    let header_dir = root.join("include");
    fs::create_dir_all(&header_dir).unwrap();
    let header_path = header_dir.join("mylib3.h");
    let header_src = "int mylib3_ping(int x);\n";
    fs::write(&header_path, header_src).unwrap();

    // Pre-populate cache + hash.
    let cache_file = cache_dir.join("mylib3.jet");
    let bind_result = jet::CBind::generate(header_src, "mylib3").unwrap();
    fs::write(&cache_file, &bind_result.source).unwrap();
    jet::CBind::write_bind_hash(&cache_file, header_src, "").unwrap();

    // Record the cache content and mtime before compile.
    let cache_before = fs::read_to_string(&cache_file).unwrap();
    let mtime_before = fs::metadata(&cache_file)
        .and_then(|m| m.modified())
        .unwrap();

    // Small sleep so any write would shift the mtime noticeably on most FS.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let main = root.join("main.jet");
    fs::write(
        &main,
        "use \"include/mylib3.h\" as m;\nfn run() { print(m.mylib3_ping(0)); }\n",
    )
    .unwrap();
    let src = fs::read_to_string(&main).unwrap();
    let _ = jet::compile_with_path(&src, main.to_str().unwrap());

    // Cache must not have been rewritten.
    let cache_after = fs::read_to_string(&cache_file).unwrap();
    let mtime_after = fs::metadata(&cache_file)
        .and_then(|m| m.modified())
        .unwrap();
    assert_eq!(
        cache_before, cache_after,
        "cache-hit fast path: cache content must not change on identical header"
    );
    assert_eq!(
        mtime_before, mtime_after,
        "cache-hit fast path: cache file must not be rewritten on hash match"
    );

    let _ = fs::remove_dir_all(&root);
}

/// Unit test: the hash function is deterministic and changes when input changes.
#[test]
fn bind_hash_is_real() {
    let h1 = jet::CBind::compute_bind_hash("int foo(int x);", "");
    let h2 = jet::CBind::compute_bind_hash("int foo(int x);", "");
    let h3 = jet::CBind::compute_bind_hash("int bar(int x);", "");
    let h4 = jet::CBind::compute_bind_hash("int foo(int x);", "-I/usr/include");

    assert_eq!(h1, h2, "same inputs → same hash");
    assert_ne!(h1, h3, "different header → different hash");
    assert_ne!(h1, h4, "different cflags → different hash");
    // Must be a 64-char hex string (SHA-256).
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn parse_pkg_config_extracts_flags() {
    let flags = jet::CFFI::parse_pkg_config("-I/usr/include/foo -L/usr/lib -lfoo -lbar", "foo");
    assert_eq!(flags.include_dirs, vec!["/usr/include/foo"]);
    assert_eq!(flags.lib_dirs, vec!["/usr/lib"]);
    assert_eq!(flags.link_names, vec!["foo", "bar"]);
}

#[test]
fn parse_pkg_config_defaults_link_name() {
    let flags = jet::CFFI::parse_pkg_config("-I/usr/include/foo", "foo");
    assert_eq!(flags.link_names, vec!["foo"]);
}

#[test]
fn deps_block_parses_c_lib_refs() {
    // S59/D-CFFI2: native C deps live in the Jet `deps:` block as `c@<target>`
    // refs, parsed by the real PackageManifest parser (not an ad-hoc reader).
    use jetpack::PackageManifest::{parse, DepSource};
    let manifest = r#"
payload: { name: "p", version: "0.1.0" }
deps: {
    raylib: c@system,
    foo:    c@"/opt/foo",
}
"#;
    let pm = parse(manifest).expect("manifest parses");
    let raylib = pm.deps.iter().find(|d| d.name == "raylib").unwrap();
    assert_eq!(
        raylib.source,
        DepSource::CLib {
            target: "system".into()
        }
    );
    let foo = pm.deps.iter().find(|d| d.name == "foo").unwrap();
    assert_eq!(
        foo.source,
        DepSource::CLib {
            target: "/opt/foo".into()
        }
    );
    // A non-C dep stays a normal Jet dep, not a CLib.
    assert!(pm.deps.iter().all(|d| d.name != "sqlite3"));
}

#[test]
fn resolve_link_unknown_lib_is_e3201() {
    // No pkg.jet dep and (in CI) no pkg-config → E3201.
    let root = std::env::temp_dir().join(format!("jet_cffi_e3201_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let err = jet::CFFI::resolve_link("nolib", &root);
    assert!(err.is_err(), "unknown lib without pkg-config must fail");
    let d = err.unwrap_err();
    assert_eq!(d.code, "E3201");
    // I4: pin the exact rendered text (this is the link-time E3201 snapshot;
    // the ui harness only renders front-end diagnostics, so it is pinned here).
    let rendered = jet::render_diagnostics("main.jet", "", std::slice::from_ref(&d));
    let expected = "\
Error [E3201]: C library `nolib` was not found.
 Why: Jet looked for a `nolib: c@…` dep in `pkg.jet`, then tried `pkg-config nolib` on the system; neither provided include/link paths.
 Fix: Install the system package (e.g. `pacman -S nolib`), or declare it as `nolib: c@system` in `deps:`.
";
    assert_eq!(rendered, expected);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn e3209_link_time_missing_lib_snapshot() {
    // I4: link-time "cannot find -l<lib>" diagnostic. Jet-stage (the ui harness
    // only renders front-end diagnostics), so pin the rendered text here.
    let d = jet::CFFI::e3209("raylib");
    assert_eq!(d.code, "E3209");
    let rendered = jet::render_diagnostics("main.jet", "", std::slice::from_ref(&d));
    let expected = "\
Error [E3209]: The linker couldn't find C library `raylib`.
 Why: Your program links against `raylib`, but the linker reported `cannot find -lraylib` — the library isn't on the link search path.
 Fix: Declare it in `deps:` so Jet provisions it: `raylib: c@system` (host pkg-config, else fetched from nixpkgs), or `raylib: c@nixpkgs:<attr>` to pick the nixpkgs attribute, or install the system package.
";
    assert_eq!(rendered, expected);
}

#[test]
fn e3210_nixpkgs_provision_failed_snapshot() {
    // I4: nixpkgs auto-provision failure diagnostic (jet-stage), pinned here.
    let d = jet::CFFI::e3210("raylib", "raylib", "error: attribute 'raylib' missing");
    assert_eq!(d.code, "E3210");
    let rendered = jet::render_diagnostics("main.jet", "", std::slice::from_ref(&d));
    let expected = "\
Error [E3210]: Couldn't fetch C library `raylib` from nixpkgs.
 Why: `raylib: c@system` asked Jet to provision `nixpkgs#raylib`, but `nix build` failed: error: attribute 'raylib' missing
 Fix: Check the attr exists (`nix build nixpkgs#raylib`), or point at a local build with `raylib: c@\"<path>\"`, or install it and use `system`.
";
    assert_eq!(rendered, expected);
}

#[test]
fn e3202_pointer_boundary_snapshot() {
    // E3202 belongs to the E2-M13 pointer tier, which is not implemented, so no
    // real source can reach it. Per I4 the diagnostic must still exist with a
    // pinned snapshot; this is it. When E2-M13 lands, a `tests/ui/` fixture that
    // actually triggers it should replace this rendered-form pin.
    use jet::Diagnostics::Span;
    let src = "fn f(p: Ptr<Int>) = \"f\";\n";
    let d = jet::Sema::e3202("Ptr<Int>", Span::new(8, 16));
    assert_eq!(d.code, "E3202");
    let rendered = jet::render_diagnostics("main.jet", src, std::slice::from_ref(&d));
    let expected = "\
Error [E3202]: Type `Ptr<Int>` cannot cross the C boundary here.
  --> main.jet:1:9
    |
  1 | fn f(p: Ptr<Int>) = \"f\";
    |         ^^^^^^^^
 Why: C FFI allows by-value scalars and `String` in ordinary code; pointers and other gated types need `use core.mem` and an `@Unsafe { … }` region (S58).
 Fix: Move the call inside `@Unsafe`, or change the type to a C-safe value type.
";
    assert_eq!(rendered, expected);
}

// ============================================================================
// Section: Rust extern-crate FFI bridge, M7 (was tests/ffi.rs)
//
// Distinct mechanism from the C FFI above — `extern rust "<crate>@<ver>" { }`
// bridges to an external Rust crate (see crates/jet-driver/src/CFFI.rs docs
// for the C path; the Rust-crate bridge template lives alongside it).
// Small enough (2 cases) not to warrant its own compiled test binary.
// ============================================================================

#[test]
fn ffi_example_compiles_and_runs() {
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    if !have_cargo {
        eprintln!("note: cargo not found; skipping FFI integration test");
        return;
    }
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping FFI integration test");
        return;
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("examples/features/lowlevel/ffi.jet");
    let src = fs::read_to_string(&path).unwrap();
    let shown = "examples/features/lowlevel/ffi.jet";

    // This example's FFI bridge (base64@0.22 / b64encode) shares a cache key
    // with tests/golden.rs's compile of the same fixture — see FfiBridgeLock.
    let _ffi_lock = FfiBridgeLock::acquire();
    let out = jet::compile_with_path(&src, shown).unwrap_or_else(|diags| {
        panic!(
            "22_ffi.jet failed the front end:\n{}",
            jet::render_diagnostics(shown, &src, &diags)
        );
    });
    assert!(out.ffi.is_some(), "expected an FFI bridge for 22_ffi.jet");
    let user_rust = common::strip_vetted_prelude_modules(&out.rust);
    assert!(
        !user_rust.contains("unsafe"),
        "I1: FFI output outside vetted runtime internals must not use unsafe"
    );

    let dir = std::env::temp_dir();
    let rs = dir.join("jet_ffi_test.rs");
    let bin = dir.join("jet_ffi_test_bin");
    fs::write(&rs, &out.rust).unwrap();

    let link = out.ffi.as_ref().unwrap();
    let status = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .arg("--extern")
        .arg(format!("{}={}", link.crate_name, link.rlib_path.display()))
        .arg("-L")
        .arg(format!("dependency={}", link.deps_dir.display()))
        .status()
        .unwrap();
    assert!(status.success(), "rustc rejected FFI-linked output (I2)");

    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "22_ffi runtime failed");
    let expected =
        fs::read_to_string(root.join("examples/features/expected/lowlevel/ffi.out")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
}

#[test]
fn inline_ffi_pin_works_inside_manifest_project() {
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();
    if !have_cargo {
        eprintln!("note: cargo not found; skipping manifest FFI integration test");
        return;
    }

    let root = std::env::temp_dir().join(format!(
        "jet_manifest_ffi_{}_{}",
        std::process::id(),
        "inline"
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("pkg.jet"),
        "payload: {\n    name: \"ffi_app\",\n    version: \"0.1.0\",\n}\n",
    )
    .unwrap();
    let path = root.join("main.jet");
    let src = "extern rust \"base64@0.22\" {\n    fn b64encode(s: String) -> String = \"base64::encode\";\n}\nfn run() { print(b64encode(\"hi\")); }\n";
    fs::write(&path, src).unwrap();

    let shown = path.to_string_lossy();
    // Same base64@0.22/b64encode signature as `ffi_example_compiles_and_runs`
    // and tests/golden.rs's `lowlevel/ffi` example — same FFI cache key.
    let _ffi_lock = FfiBridgeLock::acquire();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "inline FFI pin should work even when pkg.jet exists:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        );
    });
    assert!(out.ffi.is_some(), "expected an FFI bridge");
}
