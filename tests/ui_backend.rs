//! D-RENDERTGT2=A (c133 M1): null backend measure→layout→paint conformance.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;
mod tir_support;

// Generic hosted-tier driver; all rights assertions live in Jet. Dies at cutover.
#[test]
fn ui_host_rights_source_preserves_storage_across_hosted_tiers() {
    tir_support::assert_example_cli_tiers_agree_with_package(
        "ui/host_rights",
        Some(tir_support::TIR_TEST_PACKAGE),
        |actual| assert_eq!(actual, include_str!("../Examples/features/expected/ui/host_rights.out")),
    );
}

#[test]
fn raw_ui_transfer_requires_an_audited_gate() {
    let source = "use core.ui as ui\nfn run() { ui.__node_transfer(\"\", 0.0, 0.0, -1, 0, [], false, \"\", 63, false, 0, 0, 0, 0, 0) }\n";
    let (_, user_diags) = jet::Lexer::lex(source);
    assert!(user_diags.iter().any(|diag| diag.code == "E0067"),
        "user source must not enter the compiler-reserved identifier lane");
    let dir = common::Scratch::new("ui_raw_transfer");
    let path = dir.join("main.jet");
    fs::write(&path, "use core.ui as ui\nfn run() { ui.text(\"jet\") }\n").unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    // Embedded Core uses the generated lexer lane. Exercise its semantic gate
    // without relaxing the ordinary user-source reserved-name check.
    let (tokens, lex_diags) = jet::Lexer::lex_generated(source);
    assert!(lex_diags.is_empty(), "{lex_diags:?}");
    let program = jet::Parser::parse(&tokens).unwrap();
    let entry = &mut bundle.modules[bundle.entry];
    entry.source = source.to_string();
    entry.items = program.items;
    entry.imports = program.imports;
    let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let rendered = jet::render_diagnostics(path.to_str().unwrap(), source, &diags);
    assert!(rendered.contains("[E3101]"), "{rendered}");
    assert!(rendered.contains("requires an audited `#Unsafe` region"), "{rendered}");
}

fn build_and_run(dir: &PathBuf, name: &str, src: &str) -> (i32, String, String) {
    tir_support::write_test_package(dir, tir_support::TIR_TEST_PACKAGE);
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[test]
fn null_backend_measure_layout_paint_roundtrip() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping ui backend test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_ui_backend_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ui_null_backend",
        include_str!("../Examples/features/ui/ui_null_backend.jet"),
    );
    assert_eq!(code, 0, "ui backend roundtrip failed: {stderr}");
    let expected = include_str!("../Examples/features/expected/ui/ui_null_backend.out");
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tui_backend_reactive_render_loop() {
    tir_support::assert_example_cli_tiers_agree_with_package(
        "ui/ui_tui_reactive",
        Some(tir_support::TIR_TEST_PACKAGE),
        |actual| assert_eq!(actual, include_str!("../Examples/features/expected/ui/ui_tui_reactive.out")),
    );
}

#[test]
fn tui_portable_policy_tiers_agree() {
    tir_support::assert_example_cli_tiers_agree_with_package(
        "ui/tui_policy",
        Some(tir_support::TIR_TEST_PACKAGE),
        |actual| assert_eq!(actual, include_str!("../Examples/features/expected/ui/tui_policy.out")),
    );
}

#[test]
fn tui_backend_reactive_conditional_capture() {
    tir_support::assert_tiers_agree_with_application_policy(
        "ui_reactive_conditional_capture",
        r#"
use core.ui as ui
use core.reactive as reactive

fn view(title: String) -> UINode {
    ui.box([
        ui.text(title),
        ui.node_role("notes", 30.0, 3.0, ui.aria_role_text_input())
    ])
}

fn run() {
    selected := reactive.signal(0)
    backend := ui.tui_backend()
    render_backend :: backend.copy()
    ui.reactive_render(() -> {
        marker :: if selected.get() == 0 -> "* " else -> "  "
        ui.mount(render_backend, view("{marker}Meeting"))
    })
    print("initial: {backend.render_count()}")
    &selected.set(1)
    print("updated: {backend.render_count()}")
    &selected.set(0)
    print("reset: {backend.render_count()}")
}
"#,
        "initial: 1\nupdated: 2\nreset: 3\n",
        tir_support::TIR_TEST_PACKAGE,
    );
}

/// D-UI-MOUNT1=A: mount twice yields the same paint transcript as one mount.
#[test]
fn mount_twice_is_idempotent() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping ui mount idempotence test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_ui_mount_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"use core.ui as ui
fn run() {
    tree := ui.box([ui.text("Title"), ui.button("Save")])
    null := ui.null_backend()
    ui.mount(null, tree)
    loop command in null.commands() { print(command) }
    print("---")
    ui.mount(null, tree)
    loop command in null.commands() { print(command) }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "ui_mount_twice", src);
    assert_eq!(code, 0, "mount twice failed: {stderr}");
    let expected = "\
text({x:0,y:0,w:8,h:1}, Title)
fill({x:0,y:1,w:8,h:1}, #000000)
text({x:0,y:1,w:8,h:1}, Save)
---
text({x:0,y:0,w:8,h:1}, Title)
fill({x:0,y:1,w:8,h:1}, #000000)
text({x:0,y:1,w:8,h:1}, Save)
";
    assert_eq!(stdout, expected);
    let _ = fs::remove_dir_all(&dir);
}

/// D-UITREE1: typed constructors form one tree consumed unchanged by null and
/// TUI backends. Painting the tree also derives one shared focus order.
#[test]
fn typed_component_tree_has_backend_parity() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping typed UI tree test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_ui_tree_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"use core.ui as ui
fn run() {
    tree := ui.box([ui.text("Title"), ui.button("Save")])
    limit := ui.constraint(0.0, 0.0, 80.0, 24.0)

    null := ui.null_backend()
    size := null.measure(tree, limit)
    null.layout(tree, ui.rect(0.0, 0.0, size.width, size.height))
    null.paint(tree)
    print(null.focused_label())
    null.on_event(ui.key_event("Tab"))
    print(null.focused_label())
    loop command in null.commands() { print(command) }

    tui := ui.tui_backend()
    tui.layout(tree, ui.rect(0.0, 0.0, size.width, size.height))
    tui.paint(tree)
    print(tui.focused_label())
    loop line in tui.frame_lines() { print(line) }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "ui_typed_tree", src);
    assert_eq!(code, 0, "typed tree failed: {stderr}");
    assert_eq!(
        stdout,
        "Save\nSave\ntext({x:0,y:0,w:8,h:1}, Title)\nfill({x:0,y:1,w:8,h:1}, #000000)\ntext({x:0,y:1,w:8,h:1}, Save)\nSave\nTitle\nSave\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn retained_box_children_materialize_borrowed_parameter() {
    if !common::have_rustc() {
        eprintln!("note: skipping retained box children test (need rustc)");
        return;
    }
    let scratch = common::Scratch::new("ui_box_borrowed_children");
    let (code, stdout, stderr) = build_and_run(
        &scratch.path,
        "ui_box_borrowed_children",
        r#"
use core.ui as ui

fn boxed(children: [UINode]) -> UINode {
    return ui.box(children)
}

fn run() {
    children := [ui.text("first"), ui.text("second")]
    first :: boxed(children)
    second :: boxed(children)
    _ :: first
    _ :: second
    print("ok")
}
"#,
    );
    // rustc checks the retained child argument's slot is actually defined.
    // Reusing the source list also exercises its borrowed parameter path.
    assert_eq!(code, 0, "retained UI box children failed: {stderr}");
    assert_eq!(stdout, "ok\n");
}
