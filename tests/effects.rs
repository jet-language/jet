//! Effect system tests (D-EFF1, D-QUAL1): per-function effect inference over the
//! call graph, the `#(…)` boundary check (E0740), and `#Pure` reconciliation.

mod common;
mod tir_support;

use std::fs;
use std::process::Command;

fn codes(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code.clone()).collect(),
    }
}

#[test]
fn source_owned_hostname_effects_are_environment_and_file_read() {
    use jet_foundation::Effects::{core_effect, core_effect_leaf, Effect};
    for (module, method) in [("core.net", "gethostname"), ("core.sys", "hostname")] {
        assert_eq!(core_effect(module, method), Some(Effect::Env));
        assert_eq!(core_effect_leaf(module, method), Some("FS.Read"));
    }
}

#[test]
fn civil_zone_rows_keep_truthful_precise_effects() {
    use jet_foundation::Effects::{core_effect, core_effect_leaf, Effect};
    use jet_foundation::Syntax::{core_call, CoreCapabilityKind};

    for (method, effect, leaf) in [
        ("__zone_env", Some(Effect::Env), None),
        ("__zone_file_bytes", Some(Effect::FS), Some("FS.Read")),
        ("__embedded_zone_bytes", None, None),
        ("zone", Some(Effect::FS), Some("FS.Read")),
        ("parse_zoned", Some(Effect::FS), Some("FS.Read")),
        ("utc", None, None),
        ("parse_rfc3339", None, None),
        ("now", Some(Effect::Time), None),
        ("sleep_until", Some(Effect::Time), Some("Time.Wait")),
    ] {
        let row = core_call("core.time", method)
            .unwrap_or_else(|| panic!("missing core.time.{method} row"));
        assert_eq!(row.effect(), effect, "core.time.{method}");
        assert_eq!(row.effect_leaf(), leaf, "core.time.{method}");
        assert_eq!(core_effect("core.time", method), effect, "core.time.{method}");
        assert_eq!(core_effect_leaf("core.time", method), leaf, "core.time.{method}");
        assert_eq!(
            row.capability().kind,
            if effect.is_some() {
                CoreCapabilityKind::NativeRequired
            } else {
                CoreCapabilityKind::Pure
            },
            "core.time.{method}",
        );
        assert!(row.capability().is_well_formed(), "core.time.{method}");
    }
    let process = core_call("core.process", "__with_directory_raw")
        .expect("held process directory row");
    assert_eq!(process.effect(), Some(Effect::FS));
    assert_eq!(process.effect_leaf(), Some("FS.Read"));
}


#[test]
fn callbacks_entry_effects_are_only_io() {
    let entry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("Examples/features/basics/callbacks.jet");
    let (diagnostics, bundle, facts, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    assert_eq!(
        jet::EffectBudget::summary_line_for_program(
            &bundle.expect("callbacks bundle"),
            &facts.summaries,
            "run",
        ),
        "effects: IO"
    );
}

#[test]
fn selected_output_effects_follow_the_selected_callable() {
    let entry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("Examples/features/tooling/output_callable.jet");
    let (diagnostics, bundle, facts, _checked_nominals) = jet::Driver::check_file_with_effect_facts_for_output(
        entry.to_str().unwrap(),
        "app",
        "dev",
        &std::collections::BTreeMap::new(),
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    let bundle = bundle.expect("selected output bundle");
    let selected = bundle
        .modules
        .iter()
        .flat_map(|module| &module.items)
        .find_map(|item| match item {
            jet::AST::Item::Const(constant) => constant
                .resolved_output
                .as_ref()
                .filter(|output| output.selected),
            _ => None,
        });
    assert_eq!(
        jet::EffectBudget::summary_line_for_program(
            &bundle,
            &facts.summaries,
            "run",
        ),
        "effects: IO",
        "selected={selected:#?}\nsummaries={:#?}",
        facts.summaries
    );
}

#[test]
fn sqlite_style_fail_nth_effect_loop_is_deterministic() {
    let root = common::unique_tmp("jet_test_fault_loop");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    fs::write(
        &entry,
        r#"
use core.files as fs

#Test(faults: [FS.Write]) fn sqlite_style_fail_nth_effect_loop_is_deterministic() {
    fs.write("fault-loop.txt", "x") ?? return
    values := List.try_with_capacity(1) ?? return
    &values.try_push(1) ?? return
    assert(values.len() == 1)
    assert(true)
}

fn run() {}
"#,
    )
    .unwrap();
    let (first, _) = jet::compile_tests_with_path("", entry.to_str().unwrap())
        .expect("fault-configured test should compile");
    let (second, _) = jet::compile_tests_with_path("", entry.to_str().unwrap())
        .expect("fault-configured test should compile twice");
    assert_eq!(first, second, "fault schedules must be deterministic");
    assert!(
        first.contains("jet_fault_test_loop(&[\"FS.Write\"]"),
        "test harness must carry the sema-canonical selector:\n{first}"
    );
    assert!(
        first.contains("while fail_nth <= max_counts[selector_index]"),
        "test harness must enumerate each reachable fail-nth call:\n{first}"
    );
    assert!(
        first.contains("jet_list_try_with_capacity") && first.contains("jet_list_try_push"),
        "the SQLite-style body must use the fallible allocation rail:\n{first}"
    );
}

#[test]
fn imported_effect_boundary_diagnostic_keeps_its_source_origin() {
    let root = common::unique_tmp("jet_imported_effect_origin");
    fs::create_dir_all(&root).unwrap();
    let imported = root.join("worker.jet");
    let imported_source = "\n\n\npub fn work() -[Net]> { print(\"work\") }\n";
    fs::write(&imported, imported_source).unwrap();
    let entry = root.join("main.jet");
    let entry_source = "use \"./worker\" as worker\nfn run() { worker.work() }\n";
    fs::write(&entry, entry_source).unwrap();

    let (diagnostics, _, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0740" && diagnostic.what.contains("work"))
        .expect("the imported function exceeds its declared effect bound");
    let origin = diagnostic
        .origin()
        .expect("imported effect report needs an origin");
    assert_eq!(std::path::Path::new(&origin.path), imported.as_path());
    assert_eq!(origin.source.as_str(), imported_source);
    let span = diagnostic
        .span
        .expect("effect report needs its declared-row span");
    assert_eq!(&origin.source[span.start..span.end], "Net");
    let rendered = jet::render_diagnostics(
        entry.to_str().unwrap(),
        entry_source,
        std::slice::from_ref(diagnostic),
    );
    assert!(rendered.contains("worker.jet:4:"), "{rendered}");
    assert!(!rendered.contains("main.jet:"), "{rendered}");
}

#[test]
fn entry_unbounded_dispatch_preserves_caller_render_context() {
    let root = common::unique_tmp("jet_entry_dispatch_origin");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    let source = include_str!("ui/effect_trait_dispatch_unbounded.jet");
    fs::write(&entry, source).unwrap();
    let (diagnostics, _, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0743")
        .expect("the entry's unbounded dispatch must be rejected");
    assert!(diagnostic.origin().is_none(), "{diagnostic:#?}");
    let rendered = jet::render_diagnostics(
        "tests/ui/effect_trait_dispatch_unbounded.jet",
        source,
        std::slice::from_ref(diagnostic),
    );
    assert_eq!(
        rendered,
        include_str!("ui/effect_trait_dispatch_unbounded.stderr"),
        "the existing entry-owned UI contract must stay byte-identical",
    );
}

#[test]
fn transitive_unbounded_dispatch_keeps_the_foreign_call_origin() {
    let root = common::unique_tmp("jet_foreign_dispatch_origin");
    fs::create_dir_all(&root).unwrap();
    let imported = root.join("worker.jet");
    let imported_source =
        "\n\npub trait Shape { fn area(self) -> Int }\n\npub fn inspect(shape: Shape) -> Int { shape.area() }\n";
    fs::write(&imported, imported_source).unwrap();
    let entry = root.join("main.jet");
    let entry_source =
        "use \"./worker\" as worker\nfn ceiling(shape: worker.Shape) -[IO]> Int { worker.inspect(shape) }\nfn run() {}\n";
    fs::write(&entry, entry_source).unwrap();
    let (diagnostics, _, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0743")
        .expect("the transitive unbounded dispatch must be rejected");
    let origin = diagnostic
        .origin()
        .expect("the actual dispatch needs its origin");
    assert_eq!(std::path::Path::new(&origin.path), imported.as_path());
    assert_eq!(origin.source.as_str(), imported_source);
    let span = diagnostic.span.expect("the actual dispatch needs its span");
    assert!(origin.source[span.start..span.end].contains("area"));
    let rendered = jet::render_diagnostics(
        entry.to_str().unwrap(),
        entry_source,
        std::slice::from_ref(diagnostic),
    );
    assert!(rendered.contains("worker.jet:5:"), "{rendered}");
    assert!(!rendered.contains("main.jet:"), "{rendered}");
}

#[test]
fn cross_module_same_name_effects_keep_qualified_rows() {
    let root = common::unique_tmp("jet_effect_collision");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("left.jet"),
        "fn helper() { print(\"left\") }\nfn same() { helper() }\n",
    )
    .unwrap();
    fs::write(
        root.join("right.jet"),
        "fn helper() {}\nfn same() { helper() }\n",
    )
    .unwrap();
    let entry = root.join("main.jet");
    fs::write(
        &entry,
        "use \"./left\" as left\nuse \"./right\" as right\nfn clean() -[]> { right.same() }\nfn run() {\n    clean()\n    left.same()\n}\n",
    )
    .unwrap();

    let (diagnostics, _, facts, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
        "{diagnostics:#?}"
    );
    assert!(facts.solved["left::same"].contains("IO"));
    assert!(facts.solved["right::same"].is_empty());
    assert!(!facts.solved.contains_key("same"));
}

#[test]
fn dynamic_trait_calls_use_the_declared_dispatch_row() {
    let source = r#"
trait Shape { fn area(self) -[IO]> Int }
fn clean(shape: Shape) -[]> Int { return shape.area() }
fn run() {}
"#;
    let root = common::unique_tmp("jet_trait_dispatch_effect");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    fs::write(&entry, source).unwrap();
    let (diagnostics, _, facts, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(
        facts.summaries["clean"].edges.contains("main::Shape::area"),
        "{:#?}",
        facts.summaries["clean"].edges
    );
    assert!(
        facts.solved["Shape::area"].contains("IO"),
        "{:#?}",
        facts.solved
    );
    assert!(facts.solved["clean"].contains("IO"), "{:#?}", facts.solved);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E3401"));
}

#[test]
fn strided_source_loop_projects_exact_arena_audit_multiplicity() {
    let check = |tag: &str, source: &str| {
        let root = common::unique_tmp(tag);
        fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.jet");
        fs::write(&entry, source).unwrap();
        let (diagnostics, _, facts, _) =
            jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
        (diagnostics, facts)
    };
    let source = |bound: u64, stride: &str| {
        format!(
            r#"use core.mem as mem

fn reserve() {{
    arena :: mem.Arena.new(capacity: 4)
    close(^arena)
}}

fn run() -[!Mem.Alloc(above: {bound})]> {{
    loop item in [1, 2, 3, 4, 5], {stride} {{
        print(item)
        reserve()
    }}
}}
"#
        )
    };

    let (diagnostics, facts) = check("jet_stride_arena_exact", &source(12, "2"));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
        "five items at stride two execute three 4-byte calls:\n{diagnostics:#?}"
    );
    let run_12 = (
        "main::run".to_string(),
        jet::Sema::MemoryFact::MemAllocBounded(12),
    );
    assert_eq!(
        facts.memory_projections.get(&run_12),
        Some(&jet::Sema::MemoryProjection::Proven)
    );
    assert!(facts.solved["run"].contains("IO"));

    let (diagnostics, facts) = check("jet_stride_arena_tight", &source(11, "2"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E0921"));
    assert!(matches!(
        facts.memory_projections.get(&(
            "main::run".to_string(),
            jet::Sema::MemoryFact::MemAllocBounded(11)
        )),
        Some(jet::Sema::MemoryProjection::Violated { .. })
    ));

    let dynamic = source(12, "stride").replace(
        "fn run() -[!Mem.Alloc(above: 12)]> {",
        "fn run() -[!Mem.Alloc(above: 12)]> {\n    stride := 2",
    );
    let (diagnostics, facts) = check("jet_stride_arena_dynamic", &dynamic);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E0921"));
    assert!(matches!(
        facts.memory_projections.get(&run_12),
        Some(jet::Sema::MemoryProjection::OpenWorld { reason, .. })
            if reason.contains("loop iteration count")
    ));
    assert!(facts.solved["run"].contains("IO"));
}

/// I3: a `#FX(…)` region lowers to a plain lexical block — the generated Rust
/// carries no effect machinery (no marker, no `#(`, no effect runtime), and the
/// body runs unchanged.
#[test]
fn authority_region_erases_to_plain_block() {
    let src = r#"
fn run() {
    #FX(IO) {
        print("inside")
    }
    print("outside")
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        !rust.contains("AuthorityScope"),
        "generated Rust must not mention AuthorityScope:\n{rust}"
    );
    assert!(
        !rust.contains("#("),
        "generated Rust must carry no effect annotation:\n{rust}"
    );
    // The region body is still emitted (a plain block).
    assert!(
        rust.contains("inside") && rust.contains("outside"),
        "region body must survive:\n{rust}"
    );
}

#[test]
fn erased_scope_restores_refinement_and_effect_facts() {
    let src = r#"
use core.files as fs

fn need(value: Int) {
    print(value)
}

fn erased_refinement(value: Int?) {
    #Off {
        if value == .None {
            return
        }
        need(value)
    }
    need(value)
}

fn erased_effect() -[IO]> {
    #Off {
        #FX(authority: FS) {
            _text :: fs.read("missing") ?? ""
        }
    }
    print("erased")
}

fn active_effect() -[IO]> {
    #FX(authority: FS) {
        _text :: fs.read("missing") ?? ""
    }
    print("active")
}

fn run() {}
"#;
    let diagnostics = codes(src);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|code| *code == "E0310")
            .count(),
        1,
        "the erased optional refinement must not escape: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics.iter().filter(|code| *code == "E0740").count(),
        1,
        "the erased effect grant must not reach the caller, while the active control remains: {diagnostics:?}"
    );
    assert_eq!(diagnostics.len(), 2, "unexpected diagnostics: {diagnostics:?}");
}

#[test]
fn erased_authority_delegations_do_not_enter_effect_facts() {
    let source = r#"
use core.process as process

fn delegation_witness() {
    #FX(authority: Exec, IO, Time.Wait) {
        #Off {
            _erased :: process.run(["echo", "erased"], authority)
        }
        _active :: process.run(["echo", "active"], authority)
    }
}

fn run() {
    delegation_witness()
}
"#;
    let root = common::unique_tmp("jet_erased_authority_delegations");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    fs::write(&entry, source).unwrap();
    let (diagnostics, _, facts, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(
        diagnostics.is_empty(),
        "authority delegation witness must compile: {diagnostics:#?}"
    );
    let delegations = &facts.summaries["delegation_witness"].authority_delegations;
    assert_eq!(
        delegations.len(),
        1,
        "only the active process.run may publish an authority delegation: {delegations:#?}"
    );
    assert_eq!(delegations[0].operation, "run");
}

#[test]
fn erased_panic_does_not_cross_into_active_tier_output() {
    let source = r#"
fn run() {
    #Off {
        panic("erased")
    }
    print("active")
}
"#;
    tir_support::assert_tiers_agree("erased_panic_output", source, "active\n");
}

#[test]
fn active_panic_remains_a_tier_stop() {
    let source = r#"
fn run() {
    panic("active")
}
"#;
    let mut tiers = vec![
        ("jit", tir_support::default_run("active_panic", source)),
        (
            "interpreter",
            tir_support::default_run("active_panic", source),
        ),
    ];
    if tir_support::have_rustc() {
        tiers.push((
            "release",
            tir_support::build_and_run_full("active_panic", "active_panic", source),
        ));
    }
    for (tier, (code, stdout, stderr)) in tiers {
        assert_eq!(code, 70, "{tier} active panic exit code");
        assert!(stdout.is_empty(), "{tier} active panic stdout: {stdout:?}");
        assert!(
            stderr.contains("panic: active"),
            "{tier} lost active panic diagnostic: {stderr:?}"
        );
    }
}

/// I3 / D-EFF1: effect annotations (`#(…)`, `#Pure`, trait-method bounds) are a
/// compile-time proof only — they must leave NO trace in generated Rust.
#[test]
fn effect_annotations_are_erased() {
    let annotated = r#"
trait Shape { fn area(self) -[]> Int }
struct Square { side: Int }
impl Square.Shape { fn area(self) -> Int { return self.side * self.side } }
fn sq(n: Int) -[]> Int { return n * n }
fn load(p: String) -[IO]> { print(p) }
fn invoke(n: Int) -[IO]> { load("{sq(n)}") }
fn run() {
    s :: Square{ side: 3 }
    print("{s.area()}")
    invoke(2)
}
"#;
    let plain = r#"
trait Shape { fn area(self) -> Int }
struct Square { side: Int }
impl Square.Shape { fn area(self) -> Int { return self.side * self.side } }
fn sq(n: Int) -> Int { return n * n }
fn load(p: String) { print(p) }
fn invoke(n: Int) { load("{sq(n)}") }
fn run() {
    s :: Square{ side: 3 }
    print("{s.area()}")
    invoke(2)
}
"#;
    let a = jet::compile(annotated).expect("annotated compiles").rust;
    let b = jet::compile(plain).expect("plain compiles").rust;
    assert_eq!(
        a, b,
        "effect annotations must leave no trace in generated Rust (I3)"
    );
}


/// D-NOPANIC1=D: the existing call graph projects Panic through every
/// reachable caller and preserves the legacy panic row at the same time.
#[test]
fn panic_effect_propagates_transitively() {
    let src = r#"
fn leaf() { panic("bad input") }
fn middle() { leaf() }
fn top() -[!Panic]> { middle() }
fn run() { top() }
"#;
    let root = common::unique_tmp("jet_panic_effect");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    fs::write(&entry, src).unwrap();
    let (diagnostics, _, facts, _) =
        jet::Driver::check_file_with_effect_facts(entry.to_str().unwrap(), None, false);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E0749"));
    assert!(facts.solved["leaf"].contains("Panic"));
    assert!(facts.solved["middle"].contains("Panic"));
    assert!(facts.solved["top"].contains("Panic"));
    assert!(facts
        .reachability
        .nodes_with("panic", "panic")
        .contains("main::top"));
}









/// D-SHAPE8: retired effect markers are rejected with the canonical arrow fix.
#[test]
fn retired_effect_marker_is_e0066() {
    let src = r#"
fn calc() #(FS) -> Int { return 1 }
fn run() { print(calc()) }
"#;
    assert_eq!(codes(src), vec!["E0066"]);
}











#[test]
fn declared_effect_leaf_is_checked_everywhere() {
    let valid = r#"
effect Log.Audit
fn audit() -[Log.Audit]> {}
fn run() {
    #FX(Log.Audit) { audit() }
    #FX(authority: Log.Audit) { audit() }
}
"#;
    assert!(
        codes(valid).is_empty(),
        "declared leaves should work in rows and authority: {:?}",
        codes(valid)
    );

    for source in [
        "effect Log.Audit\nfn audit() -[Log.Aduut]> {}\nfn run() {}\n",
        "effect Log.Audit\nfn run() { #FX(Log.Aduut) {} }\n",
        "effect Log.Audit\nfn run() { #FX(authority: Log.Aduut) {} }\n",
    ] {
        let diagnostics = jet::compile(source).expect_err("typo under a checked root must fail");
        let error = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0750")
            .unwrap_or_else(|| panic!("expected E0750, got {diagnostics:#?}"));
        assert!(
            error.fix.contains("Log.Audit"),
            "nearest declared leaf should be suggested: {error:#?}"
        );
    }
}




#[test]
fn dependency_effect_declaration_joins_the_package_view() {
    let root = common::unique_tmp("jet_effect_leaf_dependency");
    let dependency = root.join("audit");
    let app = root.join("app");
    fs::create_dir_all(&dependency).unwrap();
    fs::create_dir_all(&app).unwrap();
    fs::write(
        dependency.join("package.jet"),
        "name: \"audit\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dependency.join("audit.jet"),
        "effect Log.Audit\nfn record() -[Log.Audit]> {}\n",
    )
    .unwrap();
    fs::write(
        app.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\ndeps: { audit: ../audit }\n",
    )
    .unwrap();
    let entry = app.join("main.jet");
    fs::write(&entry, "use audit\nfn run() { audit.record() }\n").unwrap();

    let checked = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", "main.jet"])
        .current_dir(&app)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    fs::write(
        &entry,
        "use audit\nfn typo() -[Log.Aduut]> {}\nfn run() { audit.record() }\n",
    )
    .unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", "main.jet"])
        .current_dir(&app)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&checked.stderr);
    assert!(!checked.status.success(), "{stderr}");
    assert!(stderr.contains("E0750"), "{stderr}");
    assert!(stderr.contains("Log.Audit"), "{stderr}");
}

// ── Scoped authority (D-SCAP1) ─────────────────────────────────────────────────








/// I3: a named `#FX(…)` region lowers to a plain lexical block — the generated
/// Rust carries no authority machinery (no handle value or revoke), no effect
/// annotation, and NO `unsafe`. The body runs unchanged.
#[test]
fn grant_region_erases_to_plain_block() {
    let src = r#"
fn run() {
    #FX(authority: IO) {
        print("inside")
    }
    print("outside")
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        !rust.contains("grant"),
        "generated Rust must not mention grant:\n{rust}"
    );
    assert!(
        !rust.contains("Authority"),
        "generated Rust must not mention the handle type:\n{rust}"
    );
    assert!(
        !rust.contains("#("),
        "generated Rust must carry no effect annotation:\n{rust}"
    );
    assert!(
        !common::strip_vetted_prelude_modules(&rust).contains("unsafe"),
        "grant codegen must contain no `unsafe`:\n{rust}"
    );
    assert!(
        rust.contains("inside") && rust.contains("outside"),
        "region body must survive:\n{rust}"
    );
}

/// I3 (D-AUTHORITY-SCOPE1): a named `#FX(…)` region lowers to the SAME plain
/// Rust block as the already-erased bare `#FX(…)` region — the handle is
/// sema-only and erased.
#[test]
fn grant_lowers_like_authority_region() {
    let granted = r#"
fn run() {
    #FX(authority: IO) {
        print("a")
        print("b")
    }
}
"#;
    let bare = r#"
fn run() {
    #FX(IO) {
        print("a")
        print("b")
    }
}
"#;
    let a = jet::compile(granted).expect("granted compiles").rust;
    let b = jet::compile(bare).expect("bare effect scope compiles").rust;
    assert_eq!(
        a, b,
        "named #FX region must lower identically to the erased #FX region (I3)"
    );
}

// ── Transactions (D-TXN1–D-TXN4) ──────────────────────────────────────────────


/// D-TXN2: a Net effect directly in the block is also rejected (E0746).
#[test]
fn transact_irreversible_net_is_e0746() {
    let src = r#"
use core.http as http
use core.http.server as server
fn run() {
    #Transact(tx) {
        r :: http.get("http://x") ?? server.response(200, "")
        print(r)
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0746"),
        "Net in #Transact should be E0746: {:?}",
        codes(src)
    );
}





/// D-BOUND-UNDO1=A: the native inline-FFI bridge executes the compensating
/// function when the transaction fails, so the declaration cannot bypass the
/// existing rollback guard at runtime.
#[test]
fn transact_inline_ffi_undo_runs_and_unwinds_lifo() {
    if !common::have_rustc() {
        return;
    }
    let src = r#"
enum Failed { Bad }

#[Unsafe("the scalar ABI contract matches the C definition"), FFI(c), Undo(undo_first)]
fn mutate_first(value: Int) -> Int {
    """int64_t mutate_first(int64_t value) { return value; }"""
}

#[Unsafe("the scalar ABI contract matches the C definition"), FFI(c), Undo(undo_second)]
fn mutate_second(value: Int) -> Int {
    """int64_t mutate_second(int64_t value) { return value; }"""
}

fn undo_first(value: Int) { print("first") }
fn undo_second(value: Int) { print("second") }

fn change() -> Int !Failed {
    #Transact(tx) {
        #Unsafe("call the audited inline C contract") {
            mutate_first(1)
        }
        tx.on_rollback(() -> { print("manual") })
        #Unsafe("call the audited inline C contract") {
            mutate_second(2)
        }
        return Err(Failed.Bad)
    }
    return Ok(0)
}

fn run() {
    _ :: change() ?? 0
    print("done")
}
"#;
    let (code, stdout, stderr) = common::build_and_run("jet_effects_ffi", "undo", src);
    assert_eq!(code, 0, "inline FFI rollback run failed: {stderr}");
    assert_eq!(stdout, "second\nmanual\nfirst\ndone\n");
    let (jit_code, jit_stdout, jit_stderr) =
        tir_support::default_run("jet_effects_ffi_jit", src);
    assert_eq!(
        jit_code, 0,
        "inline FFI JIT rollback run failed: {jit_stderr}"
    );
    assert_eq!(jit_stdout, "second\nmanual\nfirst\ndone\n");
}

/// Two modules may use the same foreign binding and inverse spelling without
/// one bundle row stealing the other's rollback identity.
#[test]
fn cross_module_foreign_undo_bindings_keep_distinct_identities() {
    if !common::have_rustc() {
        return;
    }
    let root = common::unique_tmp("jet_effects_cross_module_undo");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("left.jet"),
        r#"
extern rust "std" {
    #Undo(undo_mutate) fn mutate(value: Int) -> Int = "std::convert::identity"
}
fn undo_mutate(value: Int) { print("left") }
fn exercise() {
    #Transact(tx) {
        mutate(1)
        return
    }
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("right.jet"),
        r#"
extern rust "std" {
    #Undo(undo_mutate) fn mutate(value: Int) -> Int = "std::convert::identity"
}
fn undo_mutate(value: Int) { print("right") }
fn exercise() {
    #Transact(tx) {
        mutate(2)
        return
    }
}
"#,
    )
    .unwrap();
    let entry = root.join("main.jet");
    fs::write(
        &entry,
        r#"
use "./left" as left
use "./right" as right
fn run() {
    left.exercise()
    right.exercise()
    print("done")
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("run")
        .arg("--release")
        .arg(&entry)
        .current_dir(&root)
        .env("JET_STORE_DIR", root.join("cache"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "left\nright\ndone\n"
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&root);
}

/// A re-homed C binding cannot choose between same-named inverses in two
/// importing modules. Sema must reject the ambiguity before codegen.
#[test]
fn shared_foreign_undo_rejects_ambiguous_owners() {
    let root = common::unique_tmp("jet_effects_ambiguous_undo");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("left.jet"),
        r#"
use c.shared as ffi
#Import module c.shared {
    #Undo(undo_mutate) fn mutate(value: Int) -> Int = "mutate"
}
fn undo_mutate(value: Int) { print(value) }
"#,
    )
    .unwrap();
    fs::write(
        root.join("right.jet"),
        r#"
use c.shared as ffi
fn undo_mutate(value: Int) { print(value) }
"#,
    )
    .unwrap();
    let entry = root.join("main.jet");
    fs::write(
        &entry,
        r#"
use "./left" as left
use "./right" as right
fn run() {}
"#,
    )
    .unwrap();

    let diagnostics = jet::check_with_path(entry.to_str().unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0105" && diagnostic.what.contains("ambiguous")),
        "shared foreign undo ambiguity was not rejected: {diagnostics:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// The canonical evaluator does not execute native foreign code. It reports
/// the unsupported operation instead of silently running a foreign call without its undo hook.
#[test]
fn transact_ffi_interpreter_reports_unsupported_diagnostic() {
    let dir = common::unique_tmp("jet_effects_ffi_interpreter");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    let src = r#"
extern rust "std" {
    #Undo(undo_mutate) fn mutate(left: Int, right: Int) -> Int = "std::cmp::max"
}
fn undo_mutate(left: Int, right: Int) { print(left + right) }
fn run() {
    #Transact(tx) {
        mutate(1, 2)
    }
}
"#;
    std::fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy().into_owned();
    let outcome = crate::common::dev_run(&shown, false, true);
    match outcome {
        jet::Interpreter::RunOutcome::Problems(diagnostics) => assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0956"),
            "native FFI must use the canonical unsupported diagnostic: {diagnostics:?}"
        ),
        other => panic!("interpreter ran native FFI: {other:?}"),
    }
}




/// I3: a `#Transact` block + `on_commit` carry no effect/transaction machinery
/// that leaks user-visible effect annotations; and generated Rust has NO `unsafe`.
#[test]
fn transact_generates_no_unsafe() {
    let src = r#"
fn run() {
    #Transact(tx) {
        print("work")
        tx.on_commit(() -> { print("hook") })
    }
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    // The transaction lowers to the safe `JetTransaction` prelude + boxed hooks.
    assert!(
        rust.contains("jet_transaction()"),
        "expected the transaction guard: {}",
        rust
    );
    assert!(
        rust.contains(".on_commit(Box::new("),
        "expected a boxed commit hook: {}",
        rust
    );
    // No `unsafe` word anywhere in generated code (golden grep parity).
    assert!(
        !common::strip_vetted_prelude_modules(&rust).contains("unsafe"),
        "transaction codegen must contain no `unsafe`"
    );
}



/// D-TXN-ROLLBACK (layer 1): a value mutated inside a `#Transact` block is
/// auto-snapshotted on entry — the generated Rust calls the `jet_txn::snapshot`
/// prelude helper against the transaction guard for the mutated root.
#[test]
fn transact_auto_snapshot_mutated_value() {
    let src = r#"
enum Fail { Bad }
fn transfer(from: &Int, to: &Int, amount: Int) -> Int Fail! {
    #Transact(tx) {
        from -= amount
        to += amount
        if amount > 100 {
            return Err(Fail.Bad)
        }
    }
    return Ok(amount)
}
fn run() {
    a := 100
    b := 0
    n :: transfer(&a, &b, 40) ?? (-1)
    print("{n}")
}
"#;
    assert!(
        codes(src).is_empty(),
        "auto-snapshot transfer should compile: {:?}",
        codes(src)
    );
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        rust.contains("jet_txn::snapshot(&mut __jet_tx, &mut (*__jet_from))"),
        "expected an auto-snapshot of the mutated `from`: {}",
        rust
    );
    assert!(
        rust.contains("jet_txn::snapshot(&mut __jet_tx, &mut (*__jet_to))"),
        "expected an auto-snapshot of the mutated `to`: {}",
        rust
    );
}

/// D-TXN-ROLLBACK (layer 1): the auto-snapshot mechanism's one vetted raw-pointer
/// writeback lives ONLY inside the `mod jet_txn` prelude module — never in user code.
#[test]
fn transact_auto_snapshot_unsafe_only_in_prelude() {
    let src = r#"
enum Fail { Bad }
fn bump(x: &Int) -> Int Fail! {
    #Transact(tx) {
        x += 1
        return Err(Fail.Bad)
    }
    return Ok(0)
}
fn run() {
    a := 0
    n :: bump(&a) ?? (-1)
    print("{n}")
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        rust.contains("mod jet_txn"),
        "jet_txn module present (snapshot used)"
    );
    // The only `unsafe` is inside vetted prelude regions (mod jet_txn { … } and
    // the rest of the canonical list). Strip them all and assert none remain.
    let stripped = common::strip_vetted_prelude_modules(&rust);
    assert!(
        !stripped.contains("unsafe"),
        "auto-snapshot `unsafe` must be confined to `mod jet_txn`: {}",
        stripped
    );
}


// ---------------------------------------------------------------------------
// D-EFF2 expert levers: callback param effect bounds + `#(via f)` pass-through.
// ---------------------------------------------------------------------------

/// Lever 1: a `fn(…) -[]>` parameter handed a pure callback compiles clean.
#[test]
fn callback_pure_bound_pure_arg_ok() {
    let src = r#"
fn transform(items: [Int], f: fn(Int) -[]> Int) -> [Int] {
    return items.map((x) -> f(x)).to_list()
}
fn inc(n: Int) -[]> Int { return n + 1 }
fn run() { print("{transform([1, 2], inc)}") }
"#;
    assert!(
        codes(src).is_empty(),
        "pure callback to a #Pure bound is clean: {:?}",
        codes(src)
    );
}

/// Lever 1: a `fn(…) -[]>` parameter handed an impure callback is E0747.
#[test]
fn callback_pure_bound_impure_arg_is_e0747() {
    let src = r#"
fn transform(items: [Int], f: fn(Int) -[]> Int) -> [Int] {
    return items.map((x) -> f(x)).to_list()
}
fn noisy(n: Int) -> Int {
    print("{n}")
    return n
}
fn run() { print("{transform([1, 2], noisy)}") }
"#;
    assert_eq!(
        codes(src),
        vec!["E0747"],
        "impure callback to a #Pure bound is E0747"
    );
}

/// Lever 1: a `fn(…) -[E]>` parameter handed a callback within `E` compiles clean.
#[test]
fn callback_set_bound_within_ok() {
    let src = r#"
fn invoke(n: Int, act: fn(Int) -[IO]>) {
    act(n)
}
fn show(n: Int) { print("{n}") }
fn run() { invoke(5, show) }
"#;
    assert!(
        codes(src).is_empty(),
        "IO callback within an #(IO) bound is clean: {:?}",
        codes(src)
    );
}

/// Lever 1: a `fn(…) -[E]>` parameter handed a callback that reaches an effect
/// outside `E` is E0747.
#[test]
fn callback_set_bound_exceeded_is_e0747() {
    let src = r#"
use core.files as fs
fn invoke(p: String, act: fn(String) -[IO]>) {
    act(p)
}
fn read_it(p: String) {
    x :: fs.read(p.copy()) ?? ""
    print("{x}")
}
fn run() { invoke("f.txt", read_it) }
"#;
    assert_eq!(
        codes(src),
        vec!["E0747"],
        "FS callback to an #(IO) bound is E0747"
    );
}

/// Lever 1: an unknown effect name in a callback bound is E0119.
#[test]
fn callback_bound_unknown_effect_is_e0119() {
    let src = r#"
fn invoke(n: Int, act: fn(Int) -[Nope]>) { act(n) }
fn show(n: Int) { print("{n}") }
fn run() { invoke(5, show) }
"#;
    assert_eq!(
        codes(src),
        vec!["E0119"],
        "unknown effect in a callback bound is E0119"
    );
}

/// D-SHAPE8: open rows stay inside the arrow and remain generic proof terms;
/// they are not effect roots and therefore must not produce E0119/E0740/E0747.
#[test]
fn open_effect_row_parses_checks_and_erases() {
    let src = r#"
fn invoke<E>(marker: E, act: fn() -[..E]> Int) -[Log, ..E]> Int {
    return act()
}
fn value() -[]> Int { return 1 }
fn pure_caller() -[]> Int { return invoke(0, value) }
fn run() { print("{invoke(0, value)}") }
"#;
    let rust = jet::compile(src)
        .expect("generic open effect row compiles")
        .rust;
    assert!(!rust.contains("..E"), "effect rows erase before codegen");
}

/// Lever 2: `#(via f)` publishes the callback's effects — a `#Pure fn` calling a
/// via-fn whose callback carries `IO` is rejected (E3401), proving the
/// pass-through surfaced the effect even though the body only calls a fn-value.
#[test]
fn effect_via_publishes_callback_effect() {
    let src = r#"
fn invoke(n: Int, act: fn(Int) -[IO]>) -[via act]> {
    act(n)
}
fn show(n: Int) { print("{n}") }
fn caller() -[]> Int {
    invoke(5, show)
    return 0
}
fn run() { print("{caller()}") }
"#;
    assert_eq!(
        codes(src),
        vec!["E3401"],
        "#(via act) must publish the IO effect to callers"
    );
}

/// Lever 2: `#(via f)` naming a non-existent parameter is E0748.
#[test]
fn effect_via_unknown_param_is_e0748() {
    let src = r#"
fn invoke(n: Int, act: fn(Int) -[IO]>) -[via missing]> { act(n) }
fn show(n: Int) { print("{n}") }
fn run() { invoke(5, show) }
"#;
    assert_eq!(codes(src), vec!["E0748"], "#(via missing) is E0748");
}

/// Lever 2: `#(via f)` naming a non-callback parameter is E0748.
#[test]
fn effect_via_non_callback_param_is_e0748() {
    let src = r#"
fn invoke(n: Int) -[via n]> { print("{n}") }
fn run() { invoke(5) }
"#;
    assert_eq!(
        codes(src),
        vec!["E0748"],
        "#(via n) on a non-fn param is E0748"
    );
}

/// I3: the D-EFF2 levers are erased — a program using `fn(…) -[]>` callback
/// bounds and `#(via f)` generates the same Rust as its annotation-stripped twin.
#[test]
fn eff2_levers_are_erased() {
    let annotated = r#"
fn transform(items: [Int], f: fn(Int) -[]> Int) -> [Int] {
    return items.map((x) -> f(x)).to_list()
}
fn invoke(n: Int, act: fn(Int) -[IO]>) -[via act]> { act(n) }
fn inc(n: Int) -[]> Int { return n + 1 }
fn show(n: Int) { print("{n}") }
fn run() {
    print("{transform([1], inc)}")
    invoke(5, show)
}
"#;
    let plain = r#"
fn transform(items: [Int], f: fn(Int) -> Int) -> [Int] {
    return items.map((x) -> f(x)).to_list()
}
fn invoke(n: Int, act: fn(Int)) { act(n) }
fn inc(n: Int) -> Int { return n + 1 }
fn show(n: Int) { print("{n}") }
fn run() {
    print("{transform([1], inc)}")
    invoke(5, show)
}
"#;
    let a = jet::compile(annotated).expect("annotated compiles").rust;
    let b = jet::compile(plain).expect("plain compiles").rust;
    assert_eq!(
        a, b,
        "D-EFF2 levers must leave no trace in generated Rust (I3)"
    );
}

#[test]
fn rights_denials_keep_the_semantic_frame() {
    let src = r#"
use core.files as fs

fn load(path: String) -[]> String {
    return fs.read(path) ?? ""
}

fn run() {}
"#;
    let diagnostics = jet::compile(src).expect_err("pure filesystem access is rejected");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E3401")
        .expect("pure access should produce E3401");
    assert_eq!(diagnostic.denial_kind.as_deref(), Some("pure"));
    assert_eq!(
        diagnostic.call_chain,
        vec!["load".to_string(), "fs.read".to_string()]
    );
}
