//! #442 notebook kernel — D-NOTEBOOK-SURFACE1/DOC1/TRUST1=D.

mod common;

use jet::REPL::Notebook::{
    decide_render, export_ipynb, export_jet, import_ipynb, merge_by_id, quarantine_outputs,
    run_headless_script, save_jetnb, CellKind, ClientKind, JetNotebook, Kernel, MimeBundle,
    RenderDecision, RerunDecision, TrustStore, POLICY_VERSION,
};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;

#[test]
fn shared_session_identical_stale_rules_across_clients() {
    let mut kernel = Kernel::open(None, "env-test");
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

    assert!(kernel
        .execute_cell(ClientKind::FirstParty, &a)
        .unwrap()
        .ok());
    let fx = kernel.execute_cell(ClientKind::JupyterAdapter, &b).unwrap();
    assert!(fx.ok() || !fx.bundle.text_plain.is_empty() || fx.eval.text.contains("fx"));

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
    let mut kernel = Kernel::open(None, "proto-env");
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
