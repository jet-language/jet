//! TIR modules and enums integration tests.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{
    assert_tiers_agree, build_and_run, build_and_run_multi, have_rustc, run_default_multi,
};

#[test]
fn grouped_nested_core_import_binds_dotted_leaf() {
    let source = r#"
use core.[encoding.json]

fn run() {
    print(json.to_string(json.parse("[42]") ?? panic("bad")))
}
"#;
    assert_tiers_agree("grouped_nested_core_import", source, "[42]\n");
}

#[test]
fn source_core_enum_constructors_retain_the_declared_owner() {
    let scratch = common::Scratch::new("source_core_enum_owner");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let path = scratch.path.join("main.jet");
    fs::write(
        &path,
        r#"
use core.encoding as encoding
use core.encoding.[DataEvent as Event]
fn take(value: encoding.DataEvent) {}
fn run() {
    take(encoding.DataEvent.ArrayStart)
    take(encoding.DataEvent.Key("qualified"))
    take(Event.ArrayEnd)
    take(Event.Key("member"))
}
"#,
    )
    .unwrap();
    let (diagnostics, bundle, _, nominals) =
        jet::Driver::check_file_with_effect_facts(path.to_str().unwrap(), None, false);
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    let bundle = bundle.expect("checked source enum fixture");
    let declared_variants = bundle.modules.iter().flat_map(|module| &module.items)
        .find_map(|item| match item {
            jet::AST::Item::Enum(definition) if definition.name == "DataEvent" => {
                Some(definition.variants.iter().map(|variant| variant.name.clone()).collect::<Vec<_>>())
            }
            _ => None,
        }).expect("original source DataEvent declaration");
    let request = jet_foundation::MIR::MirArtifactRequest::new(
        jet_foundation::MIR::MirArtifactTarget::Cranelift,
        jet_foundation::MIR::MirArtifactKind::NativeExecutable,
        jet_foundation::MIR::MirArtifactBuildMode::Dev,
    );
    let (program, _) = jet::Codegen::TIR::lower_checked_mir_program_for(&bundle, &nominals, request)
        .expect("source enum constructors lower with their original declaration projection");
    program.validate().expect("source enum nominal ownership validates");
    let definitions = program.types.iter().filter(|definition| definition.name == "DataEvent")
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 1, "one original DataEvent declaration");
    assert_eq!(
        definitions[0].variants.iter().map(|variant| variant.name.clone()).collect::<Vec<_>>(),
        declared_variants,
        "the source declaration owns every emitted variant",
    );
}

#[test]
fn user_same_leaf_enum_keeps_its_own_payload_projection() {
    let scratch = common::Scratch::new("user_encoding_enum_owner");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let local = scratch.path.join("local.jet");
    fs::write(&local, "enum DataEvent { Local(String) }\nfn make() -> DataEvent { return DataEvent.Local(\"local\") }\n").unwrap();
    let path = scratch.path.join("main.jet");
    fs::write(&path, r#"
use "local"
use core.encoding as encoding
fn take_local(value: local.DataEvent) {}
fn take_core(value: encoding.DataEvent) {}
fn run() {
    take_local(local.DataEvent.Local("local"))
    take_local(local.make())
    take_core(encoding.DataEvent.Key("core"))
}
"#).unwrap();
    let (diagnostics, bundle, _, nominals) =
        jet::Driver::check_file_with_effect_facts(path.to_str().unwrap(), None, false);
    assert!(diagnostics.iter().all(|d| d.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}");
    let bundle = bundle.expect("checked distinct nominal owners");
    let owners = nominals.iter().flat_map(|facts| facts.enums.iter())
        .filter(|(_, definition)| definition.name == "DataEvent")
        .map(|(identity, _)| identity.clone()).collect::<std::collections::HashSet<_>>();
    assert_eq!(owners.len(), 2, "same leaf names have distinct original owners");
    let local_module = bundle.modules.iter().position(|module| module.path == local.to_string_lossy())
        .expect("original local enum module");
    let local_decl = bundle.modules[local_module].items.iter().find_map(|item| match item {
        jet::AST::Item::Enum(definition) if definition.name == "DataEvent" => Some(definition),
        _ => None,
    }).expect("original local enum declaration");
    assert_eq!(local_decl.variants.len(), 1);
    assert_eq!(local_decl.variants[0].name, "Local");
}

#[test]
fn native_only_core_enum_keeps_its_intrinsic_constructor_identity() {
    assert!(jet_foundation::CoreModuleExports::core_source_module("core.term").is_none(),
        "this fixture must exercise an actual no-source Core namespace");
    let scratch = common::Scratch::new("native_only_enum_owner");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let path = scratch.path.join("main.jet");
    fs::write(&path, r#"
use core.term as terminal
fn take(value: terminal.Key) {}
fn run() {
    take(terminal.Key.Escape)
}
"#).unwrap();
    let (diagnostics, bundle, _, _) =
        jet::Driver::check_file_with_effect_facts(path.to_str().unwrap(), None, false);
    assert!(bundle.is_some(), "native-only constructor fixture retains its checked bundle");
    assert!(diagnostics.iter().all(|d| d.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}");
}

#[test]
fn soft_public_imports_warn_once_per_outside_use() {
    let dir = std::env::temp_dir().join(format!("jet_soft_public_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("note.jet"),
        "struct Note {\n    _title: String\n}\nimpl Note {\n    fn _length(self) -> Int { return 1 }\n}\nfn _legacy() -> Int { return 2 }\n",
    )
    .unwrap();
    let source = "use \"note\"\nuse note._legacy\nfn run() {\n    n :: note.Note{ _title: \"hi\" }\n    print(n._title)\n    print(n._length())\n    print(note._legacy())\n    print(_legacy())\n}\n";
    let main = dir.join("main.jet");
    fs::write(&main, source).unwrap();

    let diagnostics = jet::check_with_path(main.to_str().unwrap());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "L0601")
            .count(),
        5,
        "{diagnostics:#?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn soft_public_reexports_warn_on_the_exported_spelling_once() {
    let dir = std::env::temp_dir().join(format!("jet_soft_public_reexport_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("api")).unwrap();
    fs::write(
        dir.join("api/module.jet"),
        "pub use implementation.[_raw as stable, supported as _preview]\nmodule implementation\n",
    )
    .unwrap();
    fs::write(
        dir.join("api/implementation.jet"),
        "fn _raw() -> Int { return 1 }\nfn supported() -> Int { return 2 }\n",
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(
        &main,
        "module api\nfn run() {\n    print(api.stable())\n    print(api._preview())\n}\n",
    )
    .unwrap();

    let diagnostics = jet::check_with_path(main.to_str().unwrap());
    let lints = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "L0601")
        .collect::<Vec<_>>();
    assert_eq!(lints.len(), 1, "{diagnostics:#?}");
    assert_eq!(lints[0].what, "`_preview` is a soft-public API");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn imported_soft_public_declared_types_warn_once_per_occurrence() {
    let dir = std::env::temp_dir().join(format!("jet_soft_public_types_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("models.jet"),
        "struct _Cell<T> { value: T }\ntrait _Readable { fn read(self) -> Int }\n",
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(
        &main,
        "use \"models\"\nfn adapt<T: _Readable>(value: ^models._Cell<Int>) -> models._Cell<Int> { return value }\nfn local(value: ^models._Cell<Int>) { cell := value }\nfn run() {}\n",
    )
    .unwrap();

    let diagnostics = jet::check_with_path(main.to_str().unwrap());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "L0601")
            .count(),
        4,
        "{diagnostics:#?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn local_soft_public_types_win_over_imported_name_collisions() {
    let dir = std::env::temp_dir().join(format!(
        "jet_soft_public_type_collision_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("library.jet"),
        "struct _Thing { value: Int }\ntrait _Shape { fn size(self) -> Int }\n",
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(
        &main,
        "use \"library\"\nstruct _Thing { value: Int }\ntrait _Shape { fn size(self) -> Int }\nfn keep<T: _Shape>(value: ^_Thing) -> _Thing { return value }\nfn run() {}\n",
    )
    .unwrap();

    let diagnostics = jet::check_with_path(main.to_str().unwrap());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "L0601")
            .count(),
        0,
        "{diagnostics:#?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Imported public adapters may carry core.data containers across a module
/// boundary. Their signatures are ordinary TIR types; codegen must not fall out
/// of the typed-IR seam merely because the row is dynamic or generic.
#[test]
fn imported_public_table_adapters_stay_in_tir() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module adapter
fn run() {
    print(\"ok\")
}
";
    let adapter_src = "\
fn concrete(table: ^Table<Dyn>) -> Table<Dyn> {
    return table
}
fn generic<T>(table: ^Table<T>) -> Table<T> {
    return table
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_imported_table_adapter",
        "main.jet",
        &[("main.jet", main_src), ("adapter.jet", adapter_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn explicit_internal_project_module_alias_runs() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run_multi(
        "tir_internal_project_alias",
        "main.jet",
        &[
            (
                "main.jet",
                "use project._bench as bench\nfn run() { print(bench.fixture()) }\n",
            ),
            (
                "arbitrary.jet",
                "module _bench { }\nfn fixture() -> Int { return 42 }\n",
            ),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

/// c109 Phase 14: a qualified inline code-module call `math.double(5)` (D-MOD2).
/// `main` routes through the TIR (`ModuleCall::InlineMangled` → `__jet_math__double`),
/// as do the module's own functions. rustc accepting proves byte-parity.
#[test]
fn inline_code_module_qualified_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
module math {
    fn double(n: Int) -> Int {
        return (n * 2)
    }
    fn add(a: Int, b: Int) -> Int {
        return (a + b)
    }
}
fn run() {
    print(math.double(5))
    print(math.add(b: 4, a: 3))
}
";
    let (code, stdout) = build_and_run("tir_inline_mod", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n7\n");
}

/// c109 Phase 14: an unqualified inline-module import `use math.double` (D-MOD3).
/// The bare `double(7)` lowers via `emit_call`'s `unqualified_inline` arm
/// (`ModuleCall::InlineMangled`). `main` routes through the TIR.
#[test]
fn unqualified_inline_module_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
use math.double
module math {
    fn double(n: Int) -> Int {
        return (n * 2)
    }
}
fn run() {
    print(double(7))
}
";
    let (code, stdout) = build_and_run("tir_unqual_inline", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "14\n");
}

/// D-NAME-WALK1=A: imports inside an inline module use that module's scope.
/// The grouped list, alias, public re-export, AOT, default `jet run`, and the
/// forced interpreter all exercise the same source meaning.
#[test]
fn inline_module_use_list_and_pub_reexport_match_all_tiers() {
    let src = "\
module math {
    fn double(n: Int) -> Int {
        return (n * 2)
    }
}
module api {
    use math.[double as twice]
    pub use math.[double as exported]
    fn local(n: Int) -> Int {
        return twice(n)
    }
}
fn run() {
    print(api.local(4))
    print(api.exported(5))
}
";
    if have_rustc() {
        let (code, stdout) = build_and_run("tir_inline_use_list", src);
        assert_eq!(code, 0);
        assert_eq!(stdout, "8\n10\n");
    }
    let (code, stdout, stderr) =
        run_default_multi("inline_use_list", "main.jet", &[("main.jet", src)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "8\n10\n");

    let dir = std::env::temp_dir().join(format!("jet_inline_use_interp_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let outcome = crate::common::dev_run(path.to_str().unwrap(), false, true);
    match outcome {
        jet::Interpreter::RunOutcome::Ran {
            stdout, exit_code, ..
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout, "8\n10\n");
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("forced interpreter rejected inline imports: {diags:?}")
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// D-NAME-WALK1=A: an inline module gets the same Core member walk as the
/// enclosing file. This covers both dotted-prefix body lists and a Core item
/// re-exported through the inline module's public surface.
#[test]
fn inline_module_core_use_list_and_pub_reexport_match_all_tiers() {
    let src = "\
use core.math.[abs, min, max, clamp]
module api {
    use core.[math.abs]
    pub use core.[math.abs as exported]
    fn local(n: Int) -> Int {
        return abs(n)
    }
}
fn run() {
    print(abs(-3))
    print(min(9, 4))
    print(max(9, 4))
    print(clamp(12, 0, 10))
    print(min(9.0, 4.0))
    print(max(9.0, 4.0))
    print(clamp(12.0, 0.0, 10.0))
    print(api.local(-7))
    print(api.exported(-5))
}
";
    if have_rustc() {
        let (code, stdout) = build_and_run("tir_inline_core_use_list", src);
        assert_eq!(code, 0);
        assert_eq!(stdout, "3\n4\n9\n10\n4.0\n9.0\n10.0\n7\n5\n");
    }
    let (code, stdout, stderr) =
        run_default_multi("inline_core_use_list", "main.jet", &[("main.jet", src)]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "3\n4\n9\n10\n4.0\n9.0\n10.0\n7\n5\n");

    let dir =
        std::env::temp_dir().join(format!("jet_inline_core_use_interp_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let outcome = crate::common::dev_run(path.to_str().unwrap(), false, true);
    match outcome {
        jet::Interpreter::RunOutcome::Ran {
            stdout, exit_code, ..
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout, "3\n4\n9\n10\n4.0\n9.0\n10.0\n7\n5\n");
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("forced interpreter rejected inline Core imports: {diags:?}")
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The same body-local import law applies when the prefix is a file module.
/// This catches codegen's local signature/return maps, not only sema lookup.
#[test]
fn inline_module_file_use_list_matches_all_tiers() {
    let main_src = r#"
module math
module api {
    use math.[label as decorate]
    pub use math.[label as exported]
    fn local(n: Int) -> String {
        return decorate("x", n)
    }
}
fn run() {
    print(api.local(7))
    print(api.exported("y", 8))
}
"#;
    let math_src = r#"
fn label(prefix: String, n: Int) -> String {
    return "{prefix}:{n}"
}
"#;
    let files = [("main.jet", main_src), ("math.jet", math_src)];
    if have_rustc() {
        let (code, stdout) = build_and_run_multi("tir_inline_file_use", "main.jet", &files);
        assert_eq!(code, 0);
        assert_eq!(stdout, "x:7\ny:8\n");
    }
    let (code, stdout, stderr) = run_default_multi("inline_file_use", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "x:7\ny:8\n");

    let dir = std::env::temp_dir().join(format!("jet_inline_file_interp_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("main.jet"), main_src).unwrap();
    fs::write(dir.join("math.jet"), math_src).unwrap();
    let outcome =
        crate::common::dev_run(dir.join("main.jet").to_str().unwrap(), false, true);
    match outcome {
        jet::Interpreter::RunOutcome::Ran {
            stdout, exit_code, ..
        } => {
            assert_eq!(exit_code, 0);
            assert_eq!(stdout, "x:7\ny:8\n");
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("forced interpreter rejected inline file imports: {diags:?}")
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// c109 Phase 14: a qualified file-module call `math.clamp(...)` (D-MOD1). `main`
/// routes through the TIR (`ModuleCall::Qualified` → `__jet_math::__jet_clamp`); the
/// imported module's `clamp` routes too. A String-arg module call also exercises the
/// `&(...)` Read-borrow arg form.
#[test]
fn file_module_qualified_call() {
    let main_src = "\
module math
fn run() {
    print(math.clamp(15, 0, 10))
    print(math.label(n: 5, prefix: \"x\"))
    print(math.checked(7) ?? -1)
    print(math.checked(-1) ?? -2)
    print(math.ready() ?? false)
}
";
    let math_src = "\
fn clamp(x: Int, lo: Int, hi: Int) -> Int {
    if (x < lo) {
        return lo
    }
    if (x > hi) {
        return hi
    }
    return x
}
fn label(prefix: String, n: Int) -> String {
    return \"{prefix}:{n}\"
}
fn checked(value: Int) -> Int Err! {
    if value < 0 {
        return Err(message(value))
    }
    return Ok(value)
}
fn message(value: Int) -> String {
    return \"bad {value}\"
}
fn ready() -> Bool { true }
";
    let files = [("main.jet", main_src), ("math.jet", math_src)];
    let expected = "10\nx:5\n7\n-2\ntrue\n";
    if have_rustc() {
        let (code, stdout) = build_and_run_multi("tir_file_mod", "main.jet", &files);
        assert_eq!(code, 0);
        assert_eq!(stdout, expected);
    }
    let (code, stdout, stderr) = run_default_multi("tir_file_mod", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected, "{stderr}");
    let (code, stdout, stderr) = run_default_multi("tir_file_mod", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected, "{stderr}");
}

/// A direct result handler must preserve the imported module's Result carrier
/// and canonical error identity on every execution tier.
#[test]
fn imported_module_direct_result_match_uses_error_identity() {
    let main_src = "\
module worker
fn run() {
    if worker.checked(-1) == {
        .Ok(value) -> print(\"unexpected:{value}\")
        .Err(error) -> print(\"failure:{error.message}\")
    }
    if worker.checked(7) == {
        .Ok(value) -> print(\"success:{value}\")
        .Err(error) -> print(\"unexpected:{error.message}\")
    }
}
";
    let worker_src = "\
#Error
struct WorkerError {
    message: String
}
fn checked(value: Int) -> Int WorkerError! {
    if value < 0 {
        return Err(WorkerError{message: \"negative\"})
    }
    return Ok(value)
}
";
    let files = [("main.jet", main_src), ("worker.jet", worker_src)];
    let expected = "failure:negative\nsuccess:7\n";
    let (code, stdout, stderr) =
        run_default_multi("imported_direct_result_match", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected, "{stderr}");
    let (code, stdout, stderr) =
        run_default_multi("imported_direct_result_match", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected, "{stderr}");
    if have_rustc() {
        let (code, stdout) =
            build_and_run_multi("tir_imported_direct_result_match", "main.jet", &files);
        assert_eq!(code, 0);
        assert_eq!(stdout, expected);
    }
}

/// Returning a second fallible call from a `?? {}` handler must flatten that
/// call's carrier. Otherwise the caller can select `.Ok` with an `Err` payload
/// and fail later as an invalid struct field receiver.
#[test]
fn imported_fallible_handler_return_flattens_before_result_match() {
    let main_src = "\
module worker
fn wire(value: worker.Value) -> String {
    return value.id
}
fn run() {
    if worker.checked() == {
        .Err(error) -> print(\"failure:{error.message}\")
        .Ok(value) -> print(wire(value))
    }
}
";
    let worker_src = "\
#Error
struct WorkerError {
    message: String
}
struct Value {
    id: String
}
fn inner() -> Bool WorkerError! {
    return Err(WorkerError{message: \"inner\"})
}
fn fail_after(error: WorkerError) -> Value WorkerError! {
    return Err(error)
}
fn checked() -> Value WorkerError! {
    inner() ?? {
        return fail_after(err)
    }
    return Ok(Value{id: \"unreachable\"})
}
";
    let files = [("main.jet", main_src), ("worker.jet", worker_src)];
    let expected = "failure:inner\n";
    let (code, stdout, stderr) =
        run_default_multi("imported_fallible_handler_return", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected, "{stderr}");
    let (code, stdout, stderr) =
        run_default_multi("imported_fallible_handler_return", "main.jet", &files);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected, "{stderr}");
    if have_rustc() {
        let (code, stdout) =
            build_and_run_multi("tir_imported_fallible_handler_return", "main.jet", &files);
        assert_eq!(code, 0);
        assert_eq!(stdout, expected);
    }
}

/// Imported-module AOT must qualify generated root helpers from the nested
/// emitter scope. This combines String trimming, outcome fallback/recovery,
/// and an assertion inside a helper with a declared Jet error family.
#[test]
fn imported_module_aot_root_helpers() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module helper
struct Count { value: Int }
impl Count.Add {
    fn add(self, rhs: Count) -> Count {
        total :: helper.sum(self.value, rhs.value)
        return Count{value: total}
    }
}
fn run() {
    trimmed :: helper.trimmed(\"  ok  \")
    print(trimmed)
    print(helper.checked(7) ?? -1)
    print(helper.failed(-1) ?? -2)
    recovered :: helper.recover()
    print(recovered)
    empty :: helper.empty_strings()
    print(empty.len())
    reset :: helper.reset_strings()
    print(reset.len())
    total :: Count{value: 1} + Count{value: 2}
    print(total.value)
}
";
    let helper_src = "\
fn sum(left: Int, right: Int) -> Int { left + right }
#Error
enum HelperError {
    Negative
}
fn trimmed(value: String) -> String {
    return value.trim()
}
fn checked(value: Int) -> Int HelperError! {
    assert(value >= 0)
    return Ok(value)
}
fn failed(value: Int) -> Int HelperError! {
    if value < 0 {
        return Err(HelperError.Negative)
    }
    return Ok(value)
}
fn fallback_message() -> String {
    return \"recovered\"
}
fn recover() -> String {
    values := \"\".split(\",\").to_list()
    return values.get(1) ?? fallback_message()
}
fn empty_strings() -> [String] {
    return [String]{}
}
fn reset_strings() -> [String] {
    values := [String]{}
    values = [String]{}
    return values
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_imported_module_aot_root_helpers",
        "main.jet",
        &[("main.jet", main_src), ("helper.jet", helper_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n7\n-2\nrecovered\n0\n0\n3\n");
}

/// A taken `?? panic(...)` in an imported module must compile with the nested
/// emitter's root-helper prefix and reach Jet's runtime stop.
#[test]
fn imported_module_aot_panic_fallback_uses_root_helper() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module helper
fn run() {
    print(helper.panic_fallback())
}
";
    let helper_src = "\
fn panic_fallback() -> String {
    values := \"\".split(\",\").to_list()
    return values.get(1) ?? panic(\"imported fallback\")
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_imported_module_aot_panic_fallback",
        "main.jet",
        &[("main.jet", main_src), ("helper.jet", helper_src)],
    );
    assert_eq!(code, 70);
    assert!(stdout.is_empty());
}

#[test]
fn fallible_string_call_chain_dispatches_split() {
    let source = r#"
fn text() -> String Err! {
    return Ok("a,b")
}
fn run() Err! {
    print(text().split(",").to_list())
}
"#;
    assert_tiers_agree("tir_fallible_string_split_chain", source, "[a, b]\n");
}

#[test]
fn fallible_string_explicit_try_chain_dispatches_split() {
    let source = r#"
fn text() -> String Err! {
    return Ok("a,b")
}
fn run() Err! {
    print((text()?).split(",").to_list())
}
"#;
    assert_tiers_agree(
        "tir_fallible_string_explicit_try_split_chain",
        source,
        "[a, b]\n",
    );
}

/// c109 Phase 14: an unqualified file-module import `use mathlib.[clamp, lo, hi]`
/// (D-MOD3). The bare calls lower via `emit_call`'s `unqualified_file` arm
/// (`ModuleCall::Qualified` → `__jet_mathlib::__jet_*`). `main` routes through the TIR.
#[test]
fn unqualified_file_module_call() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
use mathlib.clamp
use mathlib.[lo, hi]
module mathlib
fn run() {
    print(clamp(200, lo(), hi()))
}
";
    let mathlib_src = "\
fn clamp(n: Int, lo: Int, hi: Int) -> Int {
    if (n < lo) {
        return lo
    }
    if (n > hi) {
        return hi
    }
    return n
}
fn lo() -> Int {
    return 0
}
fn hi() -> Int {
    return 100
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_unqual_file",
        "main.jet",
        &[("main.jet", main_src), ("mathlib.jet", mathlib_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "100\n");
}

/// c109 Phase 14: a `pub use` re-export (D-MOD4). `text.wrap(...)` resolves through
/// the directory module's `pub use wrap.wrap` and lowers via `reexport_calls`
/// (`ModuleCall::Qualified` → `__jet_wrap::__jet_wrap`). The String arg exercises the
/// Read-borrow form (`&(...)`). `main` + the submodule fns route through the TIR.
#[test]
fn reexport_module_call() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
module text
fn run() {
    print(text.wrap(\"hi\"))
}
";
    let module_src = "\
pub use wrap.wrap
module wrap
";
    let wrap_src = "\
fn wrap(s: String) -> String {
    return \"[{decorate(s)}]\"
}
fn decorate(s: String) -> String {
    return \"{s}\"
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_reexport",
        "main.jet",
        &[
            ("main.jet", main_src),
            ("text/module.jet", module_src),
            ("text/wrap.jet", wrap_src),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "[hi]\n");
}

/// c109 Phase 15: a resolved comptime-if (`@if … { } else { }`). Sema selects
/// the branch; codegen emits ONLY that branch's statements inline (no `if`). Here
/// `debug` is `false`, so the `else` branch is emitted; the `then` branch is dropped.
#[test]
fn comptime_if_selected_branch() {
    if !have_rustc() {
        return;
    }
    let src = "\
DEBUG :: comptime { false }
fn pick(x: Int) -> Int {
    comptime if DEBUG {
        return x + 100
    } else {
        return x + 1
    }
    return 0
}
fn run() {
    print(\"{pick(5)}\")
}
";
    let (code, stdout) = build_and_run("tir_comptime_if", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

/// c109 Phase 15 / D-IF3: a MIXED value+range dispatch — the general `emit_mixed_switch`
/// `if/else if … else` chain (shape D). A bare-value arm (`100 ->` ≡ `score == 100`)
/// sits next to range arms (`90..99 ->`), which lower to `score >= lo && score <= hi`;
/// the body picks the first matching band. (Q4 retired free-predicate arm heads; a
/// value+range mix is the surviving way to reach the mixed if/else chain.)
#[test]
fn mixed_comparison_switch() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn grade(score: Int) -> String {
    if score == {
        100 -> { return \"A+\" }
        90..99 -> { return \"A\" }
        80..89 -> { return \"B\" }
        else -> { return \"F\" }
    }
    return \"?\"
}
fn run() {
    print(grade(85))
    print(grade(95))
    print(grade(100))
    print(grade(50))
}
";
    let (code, stdout) = build_and_run("tir_mixed_switch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "B\nA\nA+\nF\n");
}

/// c109 Phase 15: a DELEGATION trait method (`impl T.Trait using field`). The
/// forwarding method routes through the TIR's `Delegation` kind — `(self).<field>.<m>(…)`
/// with the bare trait method name.
#[test]
fn delegation_trait_method() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Speaker {
    fn say(self, msg: String) -> String
}
struct Voice {
    prefix: String
}
impl Voice.Speaker {
    fn say(self, msg: String) -> String {
        p :: self.prefix
        return \"{p}: {msg}\"
    }
}
struct Megaphone {
    inner: Voice
}
impl Megaphone.Speaker using inner
fn run() {
    v := Voice{ prefix: \"HEY\" }
    m := Megaphone{ inner: v }
    print(m.say(\"go\"))
}
";
    let (code, stdout) = build_and_run("tir_delegation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "HEY: go\n");
}

/// c109 Phase 15: the `a ?? panic(…)` fallback form. The panic message + the sorted
/// scalar-locals snapshot (`safe_locals_expr`) is reproduced from the lexical lowering
/// environment. On the success path the fallback is never taken; the program returns the
/// unwrapped value.
#[test]
fn or_fallback_panic_form() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn maybe(n: Int) -> Int? {
    if n > 0 {
        return Val(n)
    }
    return None
}
fn risky(count: Int, ratio: Float) -> Int {
    base := count + 1
    got :: maybe(count) ?? panic(\"no value at {count}\")
    return got + base
}
fn run() {
    print(\"{risky(5, 1.5)}\")
}
";
    let (code, stdout) = build_and_run("tir_panic_fallback", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "11\n");
}

/// c109 Phase 16: a String-payload enum. The construction `Msg.Text(s.copy())` explicitly
/// copies the read-only String parameter into the owning payload; the match binds the
/// payload value and returns it (owning).
#[test]
fn string_payload_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Msg {
    Text(String)
    Code(Int)
}
fn wrap(s: String) -> Msg {
    return Msg.Text(s.copy())
}
fn render(m: Msg) -> String {
    if m == {
        .Text(s) -> { return s }
        .Code(n) -> { return \"code\" }
    }
    return \"?\"
}
fn run() {
    m :: wrap(\"hi\")
    print(render(m))
}
";
    let (code, stdout) = build_and_run("tir_string_payload_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "hi\n");
}

/// c109 Phase 16: a recursive (boxed) enum. Constructing `Tree.Node(inner.copy())` explicitly
/// copies the read-only `inner: Tree` parameter before the recursive boxed edge. The
/// match traverses it (Rust auto-derefs the `Box`).
#[test]
fn recursive_boxed_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Tree {
    Leaf(Int)
    Node(Tree)
}
fn wrap(inner: Tree) -> Tree {
    return Tree.Node(inner.copy())
}
fn leaf_val(t: Tree) -> Int {
    if t == {
        .Leaf(n) -> { return n }
        .Node(inner) -> { return 0 }
    }
    return 0
}
fn run() {
    a :: Tree.Leaf(7)
    b :: wrap(a)
    print(\"{leaf_val(b)}\")
}
";
    let (code, stdout) = build_and_run("tir_recursive_boxed_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

/// A recursive enum may use named payload fields at both construction and
/// pattern sites. TIR keeps `left`/`right` canonical until MIRRust mangles
/// those names for Rust.
#[test]
fn recursive_named_payload_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Kind {
    Leaf
    Pair(left: Kind, right: Kind)
}
fn run() {
    k :: Kind.Pair{ left: Kind.Leaf, right: Kind.Leaf }
    if k == .Pair(left, right) -> print(\"yes\")
}
";
    let (code, stdout) = build_and_run("tir_recursive_named_payload_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "yes\n");
}

/// c109 Phase 16: an enum variant carrying a covered struct payload. The struct flows
/// through the variant construction and the pattern binding; reading `p.x` from the
/// bound struct is an owning field read (sema rewrites it to `.clone()`).
#[test]
fn struct_payload_enum() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point {
    x: Int
    y: Int
}
enum Shape {
    Dot(Point)
    Line(Int)
}
fn mk(p: Point) -> Shape {
    return Shape.Dot(p.copy())
}
fn first(s: Shape) -> Int {
    if s == {
        .Dot(p) -> { return p.x }
        .Line(n) -> { return n }
    }
    return 0
}
fn run() {
    pt :: Point{ x: 3, y: 4 }
    sh :: mk(pt)
    print(\"{first(sh)}\")
}
";
    let (code, stdout) = build_and_run("tir_struct_payload_enum", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// c109 Phase 16: a struct with a covered collection field, and an enum variant
/// carrying a covered collection payload. The payload is copied explicitly from
/// the read-only parameter before it fills the owning enum slot.
#[test]
fn collection_field_and_payload() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Crate {
    items: [Int]
    label: String
}
enum Holder {
    Nums([Int])
    One(Int)
}
fn mk(xs: [Int]) -> Holder {
    return Holder.Nums(xs.copy())
}
fn run() {
    b :: Crate{ items: [1, 2, 3], label: \"x\" }
    d :: mk([4, 5])
    print(b.label)
}
";
    let (code, stdout) = build_and_run("tir_collection_field_payload", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\n");
}

/// c109 Phase 17: a GENERIC free function. The `<T: Clone>` clause renders at lowering
/// (every type param carries an extra `Clone` bound, exactly `emit_func`), a type-var
/// param/return is by-value (`__jet_x: T`), and the body returns the type-var value. A
/// generic `[T]` list param/return is covered too (`Vec<T>`).
#[test]
fn generic_free_fns() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn id<T>(x: ^T) -> T {
    return x
}
fn pick<T>(a: ^T, b: ^T, first: Bool) -> T {
    if first {
        return a
    }
    return b
}
fn firstof<T>(xs: ^[T]) -> T {
    return xs[0].copy()
}
fn wrap<T>(x: ^T) -> [T] {
    return [x]
}
fn run() {
    print(\"{id(5)}\")
    print(\"{pick(1, 2, true)}\")
    print(\"{firstof([10, 20, 30])}\")
    ys :: wrap(7)
    print(\"{ys[0]}\")
}
";
    let (code, stdout) = build_and_run("tir_generic_free_fns", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n1\n10\n7\n");
}

/// Source HTTP constructors reach their reserved resource transfer rather than
/// a public native policy row; the client remains a typed transport constructor.
#[test]
fn prelude_struct_construction() {
    let src = "\
use core.http.client as client
use core.http.server as server
fn build_resp(body: String) -> HTTPResponse {
    return server.response(200, body)
}
fn build_req() -> HTTPRequest {
    return client.request(\"GET\", \"http://localhost/\")
}
fn run() {
    r :: build_resp(\"hi\")
    q :: build_req()
    print(\"built\")
}
";
    // `compile_with_path` loads the entry from disk, so write the .jet first.
    let dir = std::env::temp_dir().join(format!("jet_tir_prelude_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("prelude.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    assert!(
        out.rust.contains("jet_http_srv_raw_response_parts("),
        "source HTTPResponse resource transfer missing:\n{}",
        out.rust
    );
    // HTTPRequest: the shared client request helper.
    assert!(
        out.rust.contains(r#"return jet_http_client_request_new(&("GET".to_string()), &("http://localhost/".to_string()));"#),
        "HTTPRequest constructor helper missing:\n{}",
        out.rust
    );
}

#[test]
fn package_namespace_uses_owner_indices_without_sibling_imports() {
    let scratch = common::Scratch::new("namespace_owner_indices");
    let files = [
        ("package.jet", "name: \"namespace_owner_indices\"\nversion: \"0.1.0\"\noutputs: { app: .Executable{ entry: run } }\n"),
        ("main.jet", "#NoPrelude\nFOLDED :: comptime { fold_record().value }\nfn run() {\n    callback :: plus\n    result :: callback(2)\n    record :: make()\n    amount :: record.amount()\n    boxed :: wrap(result + amount)\n    state :: State.Ready(boxed.value)\n}\n"),
        ("types.jet", "#NoPrelude\nstruct Record { value: Int }\nstruct Box<T> { value: T }\nenum State { Ready(Int) }\nalias RecordView<T> :: Box<T>\nalias Again<T> :: RecordView<T>\n"),
        ("methods.jet", "#NoPrelude\ntrait Measure { fn amount(self) -> Int }\nimpl Record.Measure {\n    fn amount(self) -> Int { self.value }\n}\n"),
        ("values.jet", "#NoPrelude\nTOKEN :: 7\nfn plus(value: Int) -> Int { value + TOKEN }\nfn make() -> Record { Record{value: TOKEN} }\nfn fold_record() -> Record { Record{value: 11} }\nfn wrap(value: Int) -> Again<Int> { Box<Int>{value: value} }\n"),
    ];
    for (name, source) in files {
        fs::write(scratch.join(name), source).unwrap();
    }
    let (diagnostics, bundle, _, _checked_nominals) = jet::Driver::check_file_with_effect_facts_for_run_and_entry(
        scratch.join("main.jet").to_str().unwrap(), "dev", &Default::default(), None,
    );
    assert!(diagnostics.iter().all(|diagnostic|
        diagnostic.severity != jet::Diagnostics::Severity::Error), "{diagnostics:#?}");
    let bundle = bundle.expect("checked package");
    let folded = bundle.modules[bundle.entry].items.iter().find_map(|item| match item {
        jet::AST::Item::Const(constant) if constant.name == "FOLDED" => constant.ct.as_ref(),
        _ => None,
    }).expect("namespace comptime initializer folded");
    match folded {
        jet::AST::CtValue::Int(value) => assert_eq!(value.to_string(), "11"),
        value => panic!("namespace comptime initializer produced {value:?}"),
    }
    let values = bundle.modules.iter().position(|module| module.path.ends_with("values.jet")).unwrap();
    let types = bundle.modules.iter().position(|module| module.path.ends_with("types.jet")).unwrap();
    let methods = bundle.modules.iter().position(|module| module.path.ends_with("methods.jet")).unwrap();
    for (name, owner) in [
        ("plus", values), ("TOKEN", values), ("Record", types), ("Again", types), ("Measure", methods),
    ] {
        assert_eq!(bundle.name_ledger.namespace_owner(bundle.entry, name), Some(owner));
    }
    for module in &bundle.modules {
        if module.path.starts_with(&scratch.path) {
            assert!(module.imports.is_empty(), "namespace members need no generated imports: {:?}", module.path);
        }
    }
}

#[test]
fn comptime_qualified_module_function_call_reaches_its_body() {
    let scratch = common::Scratch::new("comptime_qualified_function_call");
    for (name, source) in [
        ("package.jet", "name: \"comptime_qualified_function_call\"\nversion: \"0.1.0\"\noutputs: { app: .Executable{ entry: run } }\n"),
        ("main.jet", "#NoPrelude\nmodule helper\nFOLDED :: comptime { helper.double(20) + helper.double(1) }\nfn run() {}\n"),
        ("helper.jet", "#NoPrelude\nfn double(value: Int) -> Int { value * 2 }\n"),
    ] {
        fs::write(scratch.join(name), source).unwrap();
    }
    let (diagnostics, bundle, _, _checked_nominals) = jet::Driver::check_file_with_effect_facts_for_run_and_entry(
        scratch.join("main.jet").to_str().unwrap(), "dev", &Default::default(), None,
    );
    assert!(diagnostics.iter().all(|diagnostic|
        diagnostic.severity != jet::Diagnostics::Severity::Error), "{diagnostics:#?}");
    let bundle = bundle.expect("checked package");
    let folded = bundle.modules[bundle.entry].items.iter().find_map(|item| match item {
        jet::AST::Item::Const(constant) if constant.name == "FOLDED" => constant.ct.as_ref(),
        _ => None,
    }).expect("qualified call initializer folded");
    match folded {
        jet::AST::CtValue::Int(value) => assert_eq!(value.to_string(), "42"),
        value => panic!("qualified call initializer produced {value:?}"),
    }
}

#[test]
fn enum_leaf_member_calls_keep_method_nodes_and_groups_build_children() {
    let scratch = common::Scratch::new("enum_leaf_member_boundary");
    let source = r#"
enum Choice {
    Empty
    Branch { Leaf(Int), Nested { Tip(Int), Unit } }
}
impl Choice {
    fn measure(self) -> Int { 7 }
}
fn run() {
    text :: DataTree.Null.to_text()
    equal :: DataTree.Null.equal_unordered(DataTree.Null)
    empty :: Choice.Empty.measure()
    unit :: Choice.Branch.Nested.Unit.measure()
    leaf :: Choice.Branch.Leaf(2)
    tip :: Choice.Branch.Nested.Tip(3)
}
"#;
    let path = scratch.join("main.jet");
    fs::write(&path, source).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(diagnostics.iter().all(|diagnostic|
        diagnostic.severity != jet::Diagnostics::Severity::Error), "{diagnostics:#?}");
    let run = bundle.modules[bundle.entry].items.iter().find_map(|item| match item {
        jet::AST::Item::Func(function) if function.name == "run" => Some(function),
        _ => None,
    }).unwrap();
    for (binding_name, method_name) in [
        ("text", "to_text"), ("equal", "equal_unordered"), ("empty", "measure"), ("unit", "measure"),
    ] {
        let initializer = run.body.iter().find_map(|statement| match statement {
            jet::AST::Stmt::Val(binding) if binding.name == binding_name => Some(&binding.init),
            _ => None,
        }).unwrap();
        assert!(matches!(initializer, jet::AST::Expr::MethodCall { method, .. } if method == method_name),
            "leaf {binding_name} was normalized as a constructor: {initializer:#?}");
    }
    for (binding_name, variant_name) in [("leaf", "Branch.Leaf"), ("tip", "Branch.Nested.Tip")] {
        let initializer = run.body.iter().find_map(|statement| match statement {
            jet::AST::Stmt::Val(binding) if binding.name == binding_name => Some(&binding.init),
            _ => None,
        }).unwrap();
        assert!(matches!(initializer, jet::AST::Expr::EnumLit { variant, .. } if variant == variant_name),
            "group child {binding_name} was not normalized: {initializer:#?}");
    }
}

#[test]
fn imported_enum_groups_use_canonical_facts_and_local_values_shadow_types() {
    let scratch = common::Scratch::new("imported_enum_member_boundary");
    let source = r#"
#NoPrelude
use "tags"
use tags.Choice
struct Holder { Empty: Choice }
fn run() {
    alias_empty :: tags.Choice.Empty.measure()
    imported_empty :: Choice.Empty.measure()
    alias_leaf :: tags.Choice.Branch.Leaf(2)
    imported_tip :: Choice.Branch.Nested.Tip(3)
    holder :: Holder{Empty: Choice.Empty}
    Choice :: holder
    shadowed :: Choice.Empty.measure()
}
"#;
    let tags = r#"
#NoPrelude
enum Choice {
    Empty
    Branch { Leaf(Int), Nested { Tip(Int) } }
}
impl Choice {
    fn measure(self) -> Int { 7 }
}
"#;
    let path = scratch.join("main.jet");
    fs::write(&path, source).unwrap();
    fs::write(scratch.join("tags.jet"), tags).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(diagnostics.iter().all(|diagnostic|
        diagnostic.severity != jet::Diagnostics::Severity::Error), "{diagnostics:#?}");
    let run = bundle.modules[bundle.entry].items.iter().find_map(|item| match item {
        jet::AST::Item::Func(function) if function.name == "run" => Some(function),
        _ => None,
    }).unwrap();
    for (binding_name, variant_name) in [("alias_leaf", "Branch.Leaf"), ("imported_tip", "Branch.Nested.Tip")] {
        let initializer = run.body.iter().find_map(|statement| match statement {
            jet::AST::Stmt::Val(binding) if binding.name == binding_name => Some(&binding.init),
            _ => None,
        }).unwrap();
        assert!(matches!(initializer, jet::AST::Expr::EnumLit { type_name, variant, .. }
            if type_name.contains("::") && variant == variant_name), "{initializer:#?}");
    }
    for binding_name in ["alias_empty", "imported_empty", "shadowed"] {
        let initializer = run.body.iter().find_map(|statement| match statement {
            jet::AST::Stmt::Val(binding) if binding.name == binding_name => Some(&binding.init),
            _ => None,
        }).unwrap();
        assert!(matches!(initializer, jet::AST::Expr::MethodCall { method, .. } if method == "measure"),
            "{initializer:#?}");
    }
}

#[test]
fn enum_group_value_and_bad_child_errors_do_not_turn_leaves_into_constructors() {
    for (label, source, expected_code) in [
        ("enum_group_value", "#NoPrelude\nenum Choice { Branch { Leaf(Int) } }\nfn run() { value :: Choice.Branch }\n", "E0332"),
        ("enum_group_bad_child", "#NoPrelude\nenum Choice { Branch { Leaf(Int) } }\nfn run() { value :: Choice.Branch.Missing(1) }\n", "E0304"),
        ("enum_leaf_bad_method", "#NoPrelude\nenum Choice { Empty }\nfn run() { value :: Choice.Empty.Missing() }\n", "E0102"),
    ] {
        let scratch = common::Scratch::new(label);
        let path = scratch.join("main.jet");
        fs::write(&path, source).unwrap();
        let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
        let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
        let errors = diagnostics.iter().filter(|diagnostic|
            diagnostic.severity == jet::Diagnostics::Severity::Error)
            .map(|diagnostic| diagnostic.code.to_string()).collect::<Vec<_>>();
        assert_eq!(errors, [expected_code], "{label}: {diagnostics:#?}");
    }
}
