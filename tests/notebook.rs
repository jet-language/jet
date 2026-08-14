//! #442 notebook kernel — D-NOTEBOOK-SURFACE1/DOC1/TRUST1=D.

mod common;

use jet::REPL::Notebook::{
    decide_render, export_ipynb, export_jet, import_ipynb, merge_by_id, quarantine_outputs,
    run_headless_script, save_jetnb, CellKind, ClientKind, JetNotebook, Kernel, MimeBundle,
    RenderDecision, RerunDecision, TrustStore, POLICY_VERSION,
};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn notebook_declarations_are_file_wide_but_state_cells_stay_ordered() {
    let mut kernel = Kernel::open(None, "entry-order-test");
    let caller = kernel
        .notebook
        .add_cell(
            CellKind::Jet,
            "fn caller() => Int { return helper().value }",
        )
        .id
        .clone();
    let helper = kernel
        .notebook
        .add_cell(
            CellKind::Jet,
            "fn helper() => Answer { return Answer.{ value: 42 } }",
        )
        .id
        .clone();
    let answer_type = kernel
        .notebook
        .add_cell(CellKind::Jet, "struct Answer { value: Int }")
        .id
        .clone();
    let binding = kernel
        .notebook
        .add_cell(CellKind::Jet, "answer :: 7")
        .id
        .clone();
    let read = kernel
        .notebook
        .add_cell(CellKind::Jet, "print(answer)")
        .id
        .clone();

    assert!(
        kernel
            .execute_cell(ClientKind::FirstParty, &caller)
            .unwrap()
            .ok(),
        "caller must see later helper/type declarations"
    );
    assert!(kernel
        .execute_cell(ClientKind::FirstParty, &helper)
        .unwrap()
        .ok());
    assert!(kernel
        .execute_cell(ClientKind::FirstParty, &answer_type)
        .unwrap()
        .ok());

    assert!(
        !kernel
            .execute_cell(ClientKind::FirstParty, &read)
            .unwrap()
            .ok(),
        "state cells must not see a later binding"
    );
    assert!(kernel
        .execute_cell(ClientKind::FirstParty, &binding)
        .unwrap()
        .ok());
    let output = kernel
        .execute_cell(ClientKind::FirstParty, &read)
        .unwrap();
    assert!(output.ok());
    assert!(output.bundle.text_plain.contains('7'));
}

#[test]
fn shared_session_identical_stale_rules_across_clients() {
    let mut kernel = Kernel::open(None, "env-test").unwrap();
    let a = kernel
        .notebook
        .add_cell(CellKind::Jet, "x :: 1")
        .id
        .clone();
    let b = kernel
        .notebook
        .add_cell(CellKind::Jet, "print(\"fx\")")
        .id
        .clone();
    // Wire dependency so closure invalidation is meaningful.
    if let Some(cell) = kernel.notebook.cells.iter_mut().find(|c| c.id == b) {
        cell.depends_on.push(a.clone());
    }

    let observable_success = |result: &jet::REPL::Notebook::CellExecResult| {
        result.ok() && result.bundle.text_plain.contains("fx")
    };
    assert!(kernel
        .execute_cell(ClientKind::FirstParty, &a)
        .unwrap()
        .ok());
    let fx = kernel.execute_cell(ClientKind::JupyterAdapter, &b).unwrap();

    let broken = kernel
        .notebook
        .add_cell(CellKind::Jet, "print(\"fx\") +")
        .id
        .clone();
    let broken_result = kernel
        .execute_cell(ClientKind::JupyterAdapter, &broken)
        .unwrap();
    assert!(!observable_success(&broken_result));
    assert!(observable_success(&fx));

    let plan = kernel.replay_plan(1, Some("x :: 2")).expect("plan");
    assert!(plan.steps.iter().any(|s| s.kind == jet::REPL::RerunPlan::StepKind::ConfirmEffect)
        || plan.steps.len() >= 1);

    // Skip effect → identical stale marking for every client surface.
    let stale_jp = kernel
        .apply_rerun(ClientKind::JupyterAdapter, &plan, &[RerunDecision::SkipStale])
        .unwrap();
    let view_first = kernel.view(ClientKind::FirstParty);
    let view_canvas = kernel.view(ClientKind::CanvasLens);
    let view_jp = kernel.view(ClientKind::JupyterAdapter);
    assert_eq!(view_first.stale_ids, view_canvas.stale_ids);
    assert_eq!(view_first.stale_ids, view_jp.stale_ids);
    assert_eq!(view_first.stale_ids, stale_jp);
}

#[test]
fn jetnb_cache_merge_ipynb_and_jet_export_never_hide_loss() {
    let mut nb = JetNotebook::new("env-doc");
    let c1 = nb.add_cell(CellKind::Jet, "answer :: 42").id.clone();
    nb.store_output(
        &c1,
        MimeBundle {
            text_plain: "answer : Int :: 42".into(),
            mime: vec![],
            quarantined: false,
            widget_id: None,
            requested_origins: vec![],
            requested_messages: vec![],
        },
        1,
        None,
    )
    .unwrap();
    assert!(nb.visible_output(&c1).is_some());

    // Edit source → closure key changes → stale output hidden.
    nb.cells[0].source = "answer :: 7".into();
    assert!(
        nb.visible_output(&c1).is_none(),
        "stale output must not display"
    );

    let mut theirs = JetNotebook::new("env-doc");
    let shared = c1.clone();
    theirs.cells.push(jet::REPL::Notebook::NotebookCell {
        id: shared.clone(),
        kind: CellKind::Jet,
        source: "answer :: 7".into(),
        output: None,
        depends_on: vec![],
    });
    theirs.add_cell(CellKind::Markdown, "# hi");
    let merged = merge_by_id(&nb, &theirs);
    assert!(merged.cells.iter().any(|c| c.kind == CellKind::Markdown));
    assert_eq!(
        merged.cells.iter().find(|c| c.id == shared).unwrap().source,
        "answer :: 7"
    );

    let dir = std::env::temp_dir().join(format!("jetnb-442-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("demo.jetnb");
    // Re-store live output for export.
    nb.cells[0].source = "answer :: 42".into();
    nb.store_output(
        &c1,
        MimeBundle {
            text_plain: "42".into(),
            mime: vec![],
            quarantined: false,
            widget_id: None,
            requested_origins: vec![],
            requested_messages: vec![],
        },
        2,
        None,
    )
    .unwrap();
    save_jetnb(&nb, &path).unwrap();
    let loaded = jet::REPL::Notebook::load_jetnb(&path).unwrap();
    assert_eq!(loaded.cells.len(), 1);

    let (ipynb, loss) = export_ipynb(&loaded).unwrap();
    assert!(loss.render().contains("export loss") || loss.render().contains("omitted"));
    assert!(ipynb.contains("nbformat"));
    assert!(ipynb.contains(&c1));

    let (jet_src, jet_loss) = export_jet(&loaded);
    assert!(jet_loss.render().contains("stated-loss") || jet_loss.items.len() >= 2);
    assert!(jet_src.contains("answer"));

    // Round-trip ipynb with quarantined import.
    let ipynb_doc = CanonicalJson::object([
        ("nbformat".into(), CanonicalJson::integer("4").unwrap()),
        ("nbformat_minor".into(), CanonicalJson::integer("5").unwrap()),
        ("metadata".into(), CanonicalJson::object([]).unwrap()),
        (
            "cells".into(),
            CanonicalJson::Array(vec![CanonicalJson::object([
                ("id".into(), CanonicalJson::String("keep-me".into())),
                ("cell_type".into(), CanonicalJson::String("code".into())),
                (
                    "source".into(),
                    CanonicalJson::Array(vec![CanonicalJson::String("1".into())]),
                ),
                ("metadata".into(), CanonicalJson::object([]).unwrap()),
                (
                    "outputs".into(),
                    CanonicalJson::Array(vec![CanonicalJson::object([
                        (
                            "output_type".into(),
                            CanonicalJson::String("execute_result".into()),
                        ),
                        (
                            "data".into(),
                            CanonicalJson::object([(
                                "text/plain".into(),
                                CanonicalJson::String("1".into()),
                            )])
                            .unwrap(),
                        ),
                        ("metadata".into(), CanonicalJson::object([]).unwrap()),
                        ("execution_count".into(), CanonicalJson::integer("1").unwrap()),
                    ])
                    .unwrap()]),
                ),
                ("execution_count".into(), CanonicalJson::Null),
            ])
            .unwrap()]),
        ),
    ])
    .unwrap();
    let (imported, import_loss) =
        import_ipynb(&String::from_utf8(ipynb_doc.bytes()).unwrap()).unwrap();
    assert!(import_loss.items.iter().any(|i| i.contains("quarantined")));
    let cell = &imported.cells[0];
    assert_eq!(cell.id, "keep-me");
    assert!(cell.output.as_ref().unwrap().bundle.quarantined);
}

#[test]
fn rich_output_trust_quarantines_imports_and_binds_grants() {
    let store = TrustStore::default();
    let mut bundle = MimeBundle {
        text_plain: "hi".into(),
        mime: vec![("text/html".into(), "<b>hi</b>".into())],
        quarantined: false,
        widget_id: Some("w1".into()),
        requested_origins: vec!["https://evil.example".into()],
        requested_messages: vec!["click".into()],
    };
    quarantine_outputs(&mut bundle);
    assert!(matches!(
        decide_render(&store, "src", "env", "rend", &bundle),
        RenderDecision::FallbackPlain { .. }
    ));

    let mut live = MimeBundle {
        text_plain: "chart".into(),
        mime: vec![("application/javascript".into(), "1".into())],
        quarantined: false,
        widget_id: Some("sales".into()),
        requested_origins: vec!["https://data.example".into()],
        requested_messages: vec!["SelectionChanged".into()],
    };
    assert!(matches!(
        decide_render(&store, "src", "env", "rend", &live),
        RenderDecision::FallbackPlain { .. }
    ));

    let mut store = TrustStore::default();
    let payload_hash =
        SHA256::sha256_hex(format!("{:?}\0{:?}", live.mime, live.widget_id).as_bytes());
    let req = jet::REPL::Notebook::ActiveRequest {
        notebook_source_hash: "src".into(),
        payload_hash,
        renderer_hash: "rend".into(),
        environment_hash: "env".into(),
        policy_version: POLICY_VERSION.into(),
        widget_id: "sales".into(),
        origins: live.requested_origins.clone(),
        messages: live.requested_messages.clone(),
    };
    jet::REPL::Notebook::grant_active(&mut store, &req);
    assert!(matches!(
        decide_render(&store, "src", "env", "rend", &live),
        RenderDecision::AllowActive { .. }
    ));

    // Passive zero-capability path needs no grant.
    live.requested_origins.clear();
    live.requested_messages.clear();
    live.mime = vec![("image/svg+xml".into(), "<svg/>".into())];
    assert!(matches!(
        decide_render(&store, "src", "env", "rend", &live),
        RenderDecision::AllowPassive { .. }
    ));
}

#[test]
fn headless_protocol_interrupt_stdin_debug_perf_and_clients() {
    let mut kernel = Kernel::open(None, "proto-env").unwrap();
    let out = run_headless_script(
        &mut kernel,
        &[
            "add-jet y :: 3",
            "exec first",
            "visible jupyter",
            "visible canvas",
            "visible first",
            "interrupt",
            "stdin hello",
            "debug",
            "perf",
            "complete y",
            "inspect",
            "export-jet",
            "export-ipynb",
        ],
    );
    assert!(out.contains("\"status\":\"ok\""), "{out}");
    assert!(out.contains("interrupt_requested"), "{out}");
    assert!(out.contains("stdin_queued=1"), "{out}");
    assert!(out.contains("debug_attached"), "{out}");
    assert!(out.contains("perf_attached"), "{out}");
    assert!(kernel.debug_attached() && kernel.perf_attached());
}

#[test]
fn notebook_first_hour_uses_shared_prelude_ambients_and_path() {
    let scratch = common::Scratch::new("notebook-first-hour");
    let document = scratch.join("journey.jetnb");
    let source = r#"#Grant(caps: IO, FS) {
    eprint("ambient-eprint")
    name :: input("name: ") ?? "fallback"
    assert(name == "Ada")
    write_file("notes.txt", name) ?? panic("write failed")
    assert(file_exists("notes.txt"))
    assert_eq(file_exists("notes.txt"), true)
    print(read_file(Path.from("notes.txt")) ?? panic("read failed"))
}"#;
    let environment = Kernel::environment_hash(&scratch.path);
    let mut kernel = Kernel::open(Some(&document), environment.clone()).unwrap();
    let cell_id = kernel.notebook.add_cell(CellKind::Jet, source).id.clone();
    kernel.push_stdin("Ada");

    let result = kernel
        .execute_cell(ClientKind::FirstParty, &cell_id)
        .unwrap();
    assert!(result.ok(), "shared Prelude cell failed: {}", result.eval.text);
    assert!(result.bundle.text_plain.contains("ambient-eprint"));
    assert!(result.bundle.text_plain.contains("Ada"));
    assert_eq!(std::fs::read_to_string(scratch.join("notes.txt")).unwrap(), "Ada");

    kernel.save_document(Some(&document)).unwrap();
    let reopened = Kernel::open(Some(&document), environment).unwrap();
    assert_eq!(reopened.document_path.as_deref(), Some(document.as_path()));
    assert_eq!(reopened.notebook.cells[0].source, source);
}

struct RunningNotebook(Child);

impl Drop for RunningNotebook {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command_available(name: &str, environment_name: &str) -> bool {
    let executable = std::env::var_os(environment_name).unwrap_or_else(|| name.into());
    Command::new(executable).arg("--version").output().is_ok()
}

fn free_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for_notebook_server(port: u16, token: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "notebook server did not become ready");
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
            let request = format!(
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(request.as_bytes());
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            if response.starts_with("HTTP/1.1 200") && response.contains("\"ok\":true") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn notebook_browser_matrix_uses_production_server() {
    if !command_available("node", "NODE") {
        eprintln!("skipping notebook browser matrix: node is unavailable");
        return;
    }
    let browsers = [
        ("chromium", command_available("chromium", "CHROMIUM")),
        (
            "firefox",
            command_available("firefox", "FIREFOX")
                && command_available("geckodriver", "GECKODRIVER"),
        ),
    ];
    if !browsers.iter().any(|(_, available)| *available) {
        eprintln!("skipping notebook browser matrix: no supported browser is available");
        return;
    }

    let scratch = common::Scratch::new("notebook-browser");
    let mut ran = 0;
    for (browser, available) in browsers {
        if !available {
            continue;
        }
        let browser_root = scratch.join(browser);
        std::fs::create_dir_all(&browser_root).unwrap();
        let document = browser_root.join("journey.jetnb");
        let merge_document = browser_root.join("merge.jetnb");
        let mut merge = JetNotebook::new(Kernel::environment_hash(&browser_root));
        merge.add_cell(CellKind::Markdown, "# merged from another document");
        save_jetnb(&merge, &merge_document).unwrap();

        let port = free_loopback_port();
        let token = "notebook-browser-test-token";
        let bind = format!("127.0.0.1:{port}");
        let port_text = port.to_string();
        let child = Command::new(env!("CARGO_BIN_EXE_jet"))
            .current_dir(&browser_root)
            .args([
                "notebook",
                document.to_str().unwrap(),
                "--bind",
                bind.as_str(),
                "--token",
                token,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let _server = RunningNotebook(child);
        wait_for_notebook_server(port, token);

        let output = Command::new("node")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "scripts/notebook-test/acceptance.mjs",
                "--browser",
                browser,
                "--port",
                port_text.as_str(),
                "--token",
                token,
                "--save-path",
                document.to_str().unwrap(),
                "--merge-path",
                merge_document.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{browser} notebook browser journey failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        ran += 1;
    }
    assert!(ran > 0);
}
