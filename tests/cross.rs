//! E2-M15 cross-compilation and freestanding profile tests.
//! Tests E3301 (std API in freestanding build).

mod common;

use std::fs;
use std::path::PathBuf;

/// Write `src` to a temp file and compile in freestanding mode.
/// Returns the rendered diagnostic output (or "(no errors)\n").
fn check_freestanding_src(src: &str, label: &str) -> String {
    let dir = std::env::temp_dir().join(format!("jet_cross_test_{}_{}", std::process::id(), label));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.jet");
    fs::write(&path, src).unwrap();
    let file_arg = path.to_string_lossy().into_owned();
    match jet::compile_freestanding(&file_arg) {
        Ok(_) => "(no errors)\n".to_string(),
        Err(diags) => jet::render_diagnostics(&file_arg, src, &diags),
    }
}

// ── E3301: OS-dependent API used in freestanding build ──────────────────────

#[test]
fn e3301_fs_read_in_freestanding() {
    let src = r#"use core.files as fs

fn run() {
    _ :: fs.read("config.txt")
}
"#;
    let out = check_freestanding_src(src, "fs_read");
    assert!(
        out.contains("E3301"),
        "expected E3301 for fs.read in freestanding mode; got:\n{}",
        out
    );
    assert!(
        out.contains("freestanding"),
        "expected 'freestanding' in error; got:\n{}",
        out
    );
}

#[test]
fn e3301_http_in_freestanding() {
    let src = r#"use core.http as http

fn run() {
    _ :: http.get("http://example.com")
}
"#;
    let out = check_freestanding_src(src, "http");
    assert!(
        out.contains("E3301"),
        "expected E3301 for http.get in freestanding mode; got:\n{}",
        out
    );
}

#[test]
fn e3301_tasks_in_freestanding() {
    let src = r#"use core.tasks as tasks

fn run() {
    t :: tasks.spawn(() => 42)
    t.join()
}
"#;
    let out = check_freestanding_src(src, "tasks");
    assert!(
        out.contains("E3301"),
        "expected E3301 for tasks.spawn in freestanding mode; got:\n{}",
        out
    );
}

#[test]
fn freestanding_allows_core_math() {
    // core.math is not OS-dependent; must not trigger E3301.
    let src = r#"use core.math as math

fn run() {
    x :: math.sqrt(4.0)
    print(x)
}
"#;
    let out = check_freestanding_src(src, "core_math");
    assert!(
        !out.contains("E3301"),
        "core.math should be allowed in freestanding mode; got:\n{}",
        out
    );
}

#[test]
fn freestanding_allows_core_json() {
    // core.encoding.json does not need an OS.
    let src = r#"use core.encoding.json as json

fn run() {
    s :: json.to_string("hello")
    print(s)
}
"#;
    let out = check_freestanding_src(src, "core_json");
    assert!(
        !out.contains("E3301"),
        "core.encoding.json should be allowed in freestanding mode; got:\n{}",
        out
    );
}

// ── E3301 UI snapshot ────────────────────────────────────────────────────────

/// Pin the exact rendered output for E3301 so it matches docs/spec/diagnostics.md.
#[test]
fn e3301_snapshot() {
    let src_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/freestanding_e3301.jet");
    let snap_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui/freestanding_e3301.stderr");

    if !src_path.exists() {
        panic!("missing tests/ui/freestanding_e3301.jet (I4 requires a snapshot)");
    }
    let src = fs::read_to_string(&src_path).unwrap();
    let shown = "tests/ui/freestanding_e3301.jet";
    let actual = match jet::compile_freestanding(&src_path.to_string_lossy()) {
        Ok(_) => "(no errors)\n".to_string(),
        Err(diags) => jet::render_diagnostics(shown, &src, &diags),
    };

    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::write(&snap_path, &actual).unwrap();
    } else {
        let expected = fs::read_to_string(&snap_path).unwrap_or_default();
        assert_eq!(
            actual, expected,
            "\nE3301 snapshot mismatch (run UPDATE_EXPECT=1 cargo test to bless)\n"
        );
    }
}

// ── D-OSTARGET1=A: native OS platform gating (c134 Phase 9.3) ───────────────

/// `#Target(OS.Linux)` / `#Target(OS.MacOS)` impls, both present in source —
/// only the `impl` matching `--target=<triple>`'s OS reaches the generated
/// Rust (mirrors how `Codegen/Web.rs` filters function membership by
/// `WebBucket`, E2-M15's cross-compile flag reused, no new flag). The
/// compiler-synthesized `JetShow`/`JetDebug`/`JetDisplay` impls for both
/// structs are NOT filtered (only the user's own OS-gated `impl` is) — this
/// asserts on the specific `impl __jet_Backend for __jet_<Type>` line, not a
/// bare substring count of the type name.
fn os_target_gating_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/lowlevel/os_target_gating.jet")
        .to_string_lossy()
        .into_owned()
}

fn os_target_gating_src() -> String {
    fs::read_to_string(os_target_gating_path()).unwrap()
}

#[test]
fn os_target_gating_emits_only_linux_impl_for_linux_triple() {
    let src = os_target_gating_src();
    let file = os_target_gating_path();
    let out = jet::compile_with_target(&src, &file, Some("x86_64-unknown-linux-gnu"))
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected os_target_gating.jet:\n{}",
                jet::render_diagnostics(&file, &src, &diags)
            )
        });
    assert!(
        out.rust.contains("impl __jet_Backend for __jet_LinuxBackend"),
        "Linux triple should keep the OS.Linux impl:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("impl __jet_Backend for __jet_MacosBackend"),
        "Linux triple should strip the OS.MacOS impl:\n{}",
        out.rust
    );
    // D-OSTARGET2=B: `$if build.os == { … }` in `main` must fold to the
    // Linux arm for a Linux triple — the arm constructs `LinuxBackend.{ name:
    // "gtk" }`, so "gtk" appears and the discarded arms' payloads do not.
    assert!(
        out.rust.contains("\"gtk\""),
        "Linux triple should keep only the .Linux dispatch arm (\"gtk\"):\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("\"appkit\"") && !out.rust.contains("\"win32\""),
        "Linux triple should discard the .MacOS/.Windows dispatch arms:\n{}",
        out.rust
    );
}

#[test]
fn os_target_gating_emits_only_macos_impl_for_macos_triple() {
    let src = os_target_gating_src();
    let file = os_target_gating_path();
    let out = jet::compile_with_target(&src, &file, Some("aarch64-apple-darwin")).unwrap_or_else(
        |diags| {
            panic!(
                "front end rejected os_target_gating.jet:\n{}",
                jet::render_diagnostics(&file, &src, &diags)
            )
        },
    );
    assert!(
        out.rust.contains("impl __jet_Backend for __jet_MacosBackend"),
        "macOS triple should keep the OS.MacOS impl:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("impl __jet_Backend for __jet_LinuxBackend"),
        "macOS triple should strip the OS.Linux impl:\n{}",
        out.rust
    );
    // D-OSTARGET2=B: the same `main` switch folds to the .MacOS arm for a macOS
    // triple — constructing `MacOSBackend.{ name: "appkit" }`.
    assert!(
        out.rust.contains("\"appkit\""),
        "macOS triple should keep only the .MacOS dispatch arm (\"appkit\"):\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("\"gtk\"") && !out.rust.contains("\"win32\""),
        "macOS triple should discard the .Linux/.Windows dispatch arms:\n{}",
        out.rust
    );
}

#[test]
fn os_target_gating_defaults_to_host_os_with_no_target_flag() {
    let src = os_target_gating_src();
    let file = os_target_gating_path();
    let out = jet::compile_with_target(&src, &file, None).unwrap_or_else(|diags| {
        panic!(
            "front end rejected os_target_gating.jet:\n{}",
            jet::render_diagnostics(&file, &src, &diags)
        )
    });
    // This repo's dev shell / CI host is Linux (see env: `jet::OSTarget::host()`).
    let host_is_linux = jet::Syntax::OSTarget::host() == jet::Syntax::OSTarget::Linux;
    assert_eq!(
        out.rust.contains("impl __jet_Backend for __jet_LinuxBackend"),
        host_is_linux,
        "no --target= should default to the host OS:\n{}",
        out.rust
    );
}

// ── D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend ────────────────
//
// Phase 8 is the real native backend behind the `#Target(OS.Linux)` / `comptime
// if build.os` frame. These structural tests are the headless proof (this
// environment has no reliable display): they inspect the generated Rust to show
// (a) a Linux build emits the vetted `jet_gtk` FFI module and wires real
// libgtk-4 / GLib C-ABI calls, and (b) a non-Linux build folds the gtk arm away
// so nothing links gtk. A real `jet build` of the example (golden, gated on
// `pkg-config gtk4`) is the compile-and-link proof.

fn ui_native_linux_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/ui/ui_native_linux.jet")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn gtk_backend_emits_gtk_ffi_for_linux_triple() {
    let path = ui_native_linux_path();
    let src = fs::read_to_string(&path).unwrap();
    let out = jet::compile_with_target(&src, &path, Some("x86_64-unknown-linux-gnu"))
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected ui_native_linux.jet:\n{}",
                jet::render_diagnostics(&path, &src, &diags)
            )
        });
    // The native backend is selected, so its vetted FFI module is emitted.
    assert!(
        out.rust.contains("mod jet_gtk"),
        "Linux build should emit the native GTK4 backend prelude:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("jet_ui_gtk()"),
        "Linux build should construct the GTK backend via core.ui:\n{}",
        out.rust
    );
    // Real libgtk-4 / GLib symbols are declared and called — rustc verifies the
    // `extern "C"` shims (I2), and the window/mount/click/style floor is present.
    for sym in [
        "gtk_init_check",
        "gtk_window_new",
        "gtk_box_append",
        "gtk_label_new",
        "gtk_label_set_text",
        "gtk_button_new_with_label",
        "gtk_entry_new",
        "gtk_editable_set_text",
        "gtk_box_remove",
        "gtk_widget_grab_focus",
        "gtk_widget_set_size_request",
        "gtk_css_provider_load_from_string",
        "g_signal_connect_data",
        "g_main_loop_run",
    ] {
        assert!(
            out.rust.contains(sym),
            "Linux GTK4 backend should call `{sym}`:\n{}",
            out.rust
        );
    }
}

#[test]
fn gtk_backend_folds_away_for_macos_triple() {
    let path = ui_native_linux_path();
    let src = fs::read_to_string(&path).unwrap();
    let out = jet::compile_with_target(&src, &path, Some("aarch64-apple-darwin")).unwrap_or_else(
        |diags| {
            panic!(
                "front end rejected ui_native_linux.jet:\n{}",
                jet::render_diagnostics(&path, &src, &diags)
            )
        },
    );
    // A macOS build folds the `.Linux` dispatch arm out (D-OSTARGET2=B), so the
    // gtk backend is never constructed, the FFI module is never emitted, and the
    // build never links libgtk-4 — an honest non-Linux degrade.
    assert!(
        !out.rust.contains("mod jet_gtk"),
        "macOS build must not emit the GTK4 backend prelude:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("gtk_window_new"),
        "macOS build must not wire any gtk calls:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("not yet on macOS"),
        "macOS build should keep only the .MacOS dispatch arm:\n{}",
        out.rust
    );
}

#[test]
fn gtk_backend_resolves_gtk4_link_flags() {
    // `use c.gtk4` names the native link via the S59 pkg-config path. Gated on a
    // present gtk4 (the nix dev shell); elsewhere link resolution is untested
    // here and the missing-lib path (E3201) covers absence.
    let have_gtk = std::process::Command::new("pkg-config")
        .args(["--exists", "gtk4"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !have_gtk {
        eprintln!("note: skipping gtk4 link-flag check (no pkg-config gtk4)");
        return;
    }
    let path = ui_native_linux_path();
    let clinks = jet::resolve_c_links(&path).expect("gtk4 link flags should resolve");
    assert!(
        clinks.iter().any(|a| a == "gtk-4"),
        "expected `-l gtk-4` in the link line; got {:?}",
        clinks
    );
}

#[test]
fn gtk_canonical_tree_reconciles_real_widgets_under_xvfb() {
    let have_gtk = std::process::Command::new("pkg-config")
        .args(["--exists", "gtk4"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let have_xvfb = std::process::Command::new("xvfb-run")
        .arg("--help")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    let have_display = std::env::var_os("DISPLAY").is_some();
    if !have_gtk || (!have_xvfb && !have_display) {
        eprintln!("note: skipping live GTK reconcile proof (need gtk4 + display)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_gtk_tree_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gtk_tree.jet");
    let src = r#"use core.ui as ui
use core.reactive as reactive
use c.gtk4 as gtk4
fn run() {
    app := ui.gtk_backend()
    clicks := reactive.signal(0)
    first := ui.box([
        ui.text("Title"),
        ui.button("Save"),
        ui.node_role("Name", 120.0, 24.0, ui.aria_role_text_input())
    ])
    size := app.measure(first, ui.constraint(0.0, 0.0, 320.0, 240.0))
    app.layout(first, ui.rect(0.0, 0.0, size.width, size.height))
    app.paint(first)
    save := app.button("Save")
    app.on_click(save, () => {
        clicks.set(clicks.get() + 1)
    })
    app.set_text(save, "__JET_TEST_CLICK__")
    app.on_event(ui.key_event("Tab"))
    second := ui.box([ui.text("Updated")])
    app.paint(second)
    app.set_text(save, "stale")
    third := ui.box([ui.text("Updated"), ui.button("Save")])
    app.paint(third)
    app.set_text(save, "Rebound")
    app.set_text(save, "__JET_TEST_CLICK__")
    print("{app.focused_label()}:{clicks.get()}")
}
"#;
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_target(src, &shown, Some("x86_64-unknown-linux-gnu"))
        .unwrap_or_else(|diags| panic!("{}", jet::render_diagnostics(&shown, src, &diags)));
    // Test-only generated-Rust hook: emit `clicked` on the resolved GTK button
    // when the fixture writes the sentinel text. This uses GTK's real signal
    // without adding a Jet-visible testing API to core.ui.
    let extern_marker = "    extern \"C\" {\n        fn gtk_init_check() -> gboolean;";
    let extern_replacement = "    extern \"C\" {\n        fn g_signal_emit_by_name(instance: gpointer, detailed_signal: *const c_char, ...);\n        fn gtk_init_check() -> gboolean;";
    let set_text_marker = "        pub fn set_text(&self, id: i64, text: &str) {\n            let state = self.state.borrow();";
    let set_text_replacement = r#"        pub fn set_text(&self, id: i64, text: &str) {
            if text == "__JET_TEST_CLICK__" {
                let state = self.state.borrow();
                if let Some((widget, GtkWidgetKind::Button)) = state.resolve_widget_handle(id) {
                    let signal = CString::new("clicked").unwrap();
                    unsafe { g_signal_emit_by_name(widget as gpointer, signal.as_ptr()); }
                }
                return;
            }
            let state = self.state.borrow();"#;
    assert!(out.rust.contains(extern_marker), "missing GTK extern injection marker");
    assert!(out.rust.contains(set_text_marker), "missing GTK signal injection marker");
    let rust = out
        .rust
        .replacen(extern_marker, extern_replacement, 1)
        .replacen(set_text_marker, set_text_replacement, 1);
    let rs = dir.join("gtk_tree.rs");
    let bin = dir.join("gtk_tree");
    fs::write(&rs, rust).unwrap();
    let mut rustc = std::process::Command::new("rustc");
    rustc.args(["--edition", "2021"]).arg(&rs).arg("-o").arg(&bin);
    for arg in jet::resolve_c_links(&shown).expect("gtk4 link flags") {
        rustc.arg(arg);
    }
    let compiled = rustc.output().unwrap();
    assert!(compiled.status.success(), "generated GTK Rust failed:\n{}", String::from_utf8_lossy(&compiled.stderr));

    let mut run_command = if have_xvfb {
        let mut command = std::process::Command::new("xvfb-run");
        command.arg("-a").arg(&bin);
        command
    } else {
        std::process::Command::new(&bin)
    };
    let run = run_command.env("JET_UI_GTK_TRACE", "1").output().unwrap();
    assert!(run.status.success(), "live GTK fixture failed:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Save:2\n");
    let trace = String::from_utf8_lossy(&run.stderr);
    for event in [
        "GTK_UI create root Box",
        "GTK_UI create root/1 Button",
        "GTK_UI create root/2 Entry",
        "GTK_UI click-connect root/1 1",
        "GTK_UI focus root/2",
        "GTK_UI update root/0 Updated",
        "GTK_UI remove root/2",
        "GTK_UI remove root/1",
        "GTK_UI handle-miss 0",
        "GTK_UI create root/1 Button",
        "GTK_UI click-connect root/1 1",
        "GTK_UI handle-set-text 0 Rebound",
        "GTK_UI cleanup",
    ] {
        assert!(trace.contains(event), "missing `{event}` in GTK trace:\n{trace}");
    }
    assert_eq!(
        trace.matches("GTK_UI click-connect root/1 1").count(),
        2,
        "click handler must connect exactly once per widget lifetime:\n{trace}"
    );
    let _ = fs::remove_dir_all(&dir);
}
