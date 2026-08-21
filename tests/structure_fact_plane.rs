//! D-STRUCT-PLANE1=A: structure rows, facts, gates, inspection registration,
//! and runtime erasure all use the one existing fact machinery.

use jet::Diagnostics::Span;
use jet_foundation::Names::{NameLedger, StructureFact, StructureFactKind};
use jet_foundation::Registry;
use std::process::Command;

const FACT_SOURCE: &str = r#"
fact StructureLiveness(@name: "Structure.Liveness", @holds: .Build, @safe: .Gain, @gates: ["_name"], @decision: "D-STRUCT-PLANE1")

fn run() {
    print("structure facts erase")
}
"#;

#[test]
fn structure_rows_share_the_one_registry_and_law() {
    let rows: Vec<_> = Registry::structure_rows().collect();
    assert_eq!(
        rows.iter().map(|row| row.name).collect::<Vec<_>>(),
        vec![
            "Structure.Liveness",
            "Structure.Lifecycle",
            "Structure.ImportEdge",
        ]
    );
    assert_eq!(rows[0].gates, &["_name"][..]);
    assert_eq!(rows[1].gates, &["policy allow"][..]);
    assert_eq!(rows[2].gates, &["manifest rule edit"][..]);
    for kind in [
        StructureFactKind::Liveness,
        StructureFactKind::Lifecycle,
        StructureFactKind::ImportEdge,
    ] {
        assert!(Registry::structure_row(kind).is_some());
    }
    assert!(rows.iter().all(|row| {
        row.safe_direction != Registry::SafeDirection::None && !row.gates.is_empty()
    }));
    assert!(
        Registry::law_violations().is_empty(),
        "registry drift: {:?}",
        Registry::law_violations()
    );
}

#[test]
fn structure_facts_and_gates_use_one_ledger() {
    let mut names = NameLedger::default();
    let fact = StructureFact::new(
        StructureFactKind::ImportEdge,
        "app.ui -> app.db",
        "package.jet",
        Span::new(11, 20),
        "denied",
        "boundaries[0]",
        Some("manifest rule edit".to_string()),
    );
    names.record_structure_fact(fact.clone());
    names.record_structure_fact(fact);
    assert_eq!(names.structure_facts().len(), 1);

    // The module and the struct share the name, so the struct needs both segments.
    let mut gates = jet::Sema::GateLedger::GateLedger::default();
    gates.append_structure_facts(&names);
    assert_eq!(gates.entries().len(), 1);
    assert_eq!(
        gates.entries()[0].kind,
        jet::Sema::GateLedger::GateKind::Structure
    );
    assert_eq!(gates.entries()[0].scope, "import-edge");
}

#[test]
fn inspect_structure_is_registered_in_the_cli() {
    let (_, action) = jet::CLI::nested_command("inspect", "structure")
        .expect("ratified structure inspection action");
    assert_eq!(action.usage, "structure [--json] <file.jet>");
    assert_eq!(action.handler.dispatch_word(), "structure");
}

#[test]
fn inspect_structure_text_and_json_expose_the_three_rows() {
    let example = "examples/features/tooling/structure_plane.jet";
    let text = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "structure", example])
        .output()
        .expect("run structure inspection");
    assert!(text.status.success(), "structure text failed: {text:?}");
    let text = String::from_utf8(text.stdout).expect("structure text is utf8");
    for expected in [
        "Structure.Liveness",
        "Structure.Lifecycle",
        "Structure.ImportEdge",
        "safe=gain",
        "safe=shrink",
        "_name",
        "policy allow",
        "manifest rule edit",
    ] {
        assert!(text.contains(expected), "missing {expected} in {text}");
    }

    let json = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "structure", "--json", example])
        .output()
        .expect("run JSON structure inspection");
    assert!(json.status.success(), "structure JSON failed: {json:?}");
    let json = String::from_utf8(json.stdout).expect("structure JSON is utf8");
    assert!(json.starts_with("{\"schema_version\":1,"));
    for expected in [
        "Structure.Liveness",
        "Structure.Lifecycle",
        "Structure.ImportEdge",
        "\"safe_direction\":\"gain\"",
        "\"safe_direction\":\"shrink\"",
        "policy allow",
        "manifest rule edit",
    ] {
        assert!(json.contains(expected), "missing {expected} in {json}");
    }
}

#[test]
fn structure_fact_declaration_erases_before_codegen() {
    let output = jet::compile(FACT_SOURCE).expect("structure fact fixture compiles");
    assert!(output.rust.contains("structure facts erase"));
    assert!(!output.rust.contains("Structure.Liveness"));
    assert!(!output.rust.contains("@holds"));
    assert!(!output.rust.contains("manifest rule edit"));
}

#[test]
fn structure_fact_declaration_erases_for_web_codegen() {
    let output = jet::compile_web_with_path(FACT_SOURCE, "structure_fact_plane_web.jet")
        .expect("structure fact fixture compiles for web");
    let web = output.web.expect("web artifacts are present");
    for generated in [&web.wasm_rust, &web.js_app] {
        assert!(!generated.contains("Structure.Liveness"));
        assert!(!generated.contains("@holds"));
        assert!(!generated.contains("manifest rule edit"));
    }
}
