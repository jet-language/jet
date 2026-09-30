//! #2517 S1: package identity and fingerprints over parsed sources.
//!
//! A whitespace or comment edit changes no fingerprint. A body edit changes
//! the item's source fingerprint but not its interface fingerprint or the
//! package interface digest. A signature edit changes both. Identities and
//! digests do not depend on where the checkout lives.

use jet_foundation::AST::{CFfi, Item, LoadedModule, NameLedger, ProgramBundle};
use jet_foundation::PackageIdentity::{
    item_interface_fingerprint, item_source_fingerprint, PackageGraph, PackageKind, CORE_PACKAGE,
};
use jet_parser::{Lexer, Parser};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

fn module(path: &Path, source: &str) -> LoadedModule {
    let (tokens, diagnostics) = Lexer::lex(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let mut program = Parser::parse(&tokens).expect("parses");
    LoadedModule {
        path: path.to_path_buf(),
        display: path.display().to_string(),
        source: source.to_string(),
        alias: path.file_stem().unwrap().to_string_lossy().into_owned(),
        imports: std::mem::take(&mut program.imports),
        items: std::mem::take(&mut program.items),
        script_body: std::mem::take(&mut program.script_body),
        block_spans: std::mem::take(&mut program.block_spans),
        web_target_ceiling: program.web_target_ceiling,
        pub_file: program.pub_file,
        no_prelude: program.no_prelude,
        default_target: program.default_target.clone(),
        html_path: program.html_path.clone(),
        policy_declarations: program.policy_declarations.clone(),
        user_policy_declarations: program.user_policy_declarations.clone(),
        rule_facts: std::mem::take(&mut program.rule_facts),
    }
}

fn items(source: &str) -> Vec<Item> {
    module(Path::new("x.jet"), source).items
}

fn only_item(source: &str) -> Item {
    let mut items = items(source);
    assert_eq!(items.len(), 1);
    items.remove(0)
}

/// A bundle with one directory package (`lib/`, two files), the entry as a
/// loose file importing it, and one Core module.
fn bundle(root: &Path, lib_a: &str) -> ProgramBundle {
    let lib = root.join("lib");
    let modules = vec![
        module(&root.join("main.jet"), "fn run() {\n    print(\"hi\")\n}\n"),
        module(&lib.join("a.jet"), lib_a),
        module(&lib.join("b.jet"), "pub fn b() -> Int {\n    2\n}\n"),
        module(Path::new("<corelib>/text.jet"), "pub fn len() -> Int {\n    0\n}\n"),
    ];
    let mut name_ledger = NameLedger::default();
    name_ledger.set_module_namespace(1, lib.display().to_string());
    name_ledger.set_module_namespace(2, lib.display().to_string());
    name_ledger.record_import_target(0, jet_foundation::Diagnostics::Span::new(0, 1), 1);
    ProgramBundle {
        entry: 0,
        project_root: root.to_path_buf(),
        modules,
        devtools_registry: Default::default(),
        parse_teaching: Vec::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        cffi: CFfi::default(),
        comptime_inputs: Vec::new(),
        name_ledger,
        layer_ceiling: None,
        inferred_layer: jet_foundation::RingLayer::RuntimeLayer::Core,
        web_partitions: HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: HashMap::new(),
        package_guarantees: Default::default(),
        program_allocator: Default::default(),
        active_os: jet_foundation::OSTarget::OSTarget::host(),
        build_facts: Default::default(),
        edition: "2027".to_string(),
    }
}

const LIB_A: &str = "pub fn add(a: Int, b: Int) -> Int {\n    a + b\n}\n\nfn helper() -> Int {\n    1\n}\n";

#[test]
fn item_fingerprints_ignore_whitespace_and_comments() {
    let plain = only_item("pub fn add(a: Int, b: Int) -> Int {\n    a + b\n}\n");
    let spaced = only_item(
        "// adds two numbers\n\n\npub fn add(a: Int,   b: Int) -> Int {\n\n    // the sum\n    a  +  b\n}\n",
    );
    assert_eq!(item_source_fingerprint(&plain), item_source_fingerprint(&spaced));
    assert_eq!(
        item_interface_fingerprint(&plain, false),
        item_interface_fingerprint(&spaced, false)
    );
}

#[test]
fn body_edit_keeps_interface_fingerprint_and_signature_edit_changes_it() {
    let plain = only_item("pub fn add(a: Int, b: Int) -> Int {\n    a + b\n}\n");
    let body = only_item("pub fn add(a: Int, b: Int) -> Int {\n    b + a\n}\n");
    let signature = only_item("pub fn add(a: Int, b: Int, c: Int) -> Int {\n    a + b\n}\n");
    assert_ne!(item_source_fingerprint(&plain), item_source_fingerprint(&body));
    assert_eq!(
        item_interface_fingerprint(&plain, false),
        item_interface_fingerprint(&body, false)
    );
    assert_ne!(
        item_interface_fingerprint(&plain, false),
        item_interface_fingerprint(&signature, false)
    );
    // Generic bodies are templates importers instantiate, so they stay in the
    // interface.
    let generic = only_item("pub fn same<T>(x: T) -> T {\n    x\n}\n");
    let generic_body = only_item("pub fn same<T>(x: T) -> T {\n    y :: x\n    y\n}\n");
    assert_ne!(
        item_interface_fingerprint(&generic, false),
        item_interface_fingerprint(&generic_body, false)
    );
    // A private function is not part of the interface.
    assert_eq!(item_interface_fingerprint(&only_item("fn hidden() -> Int {\n    1\n}\n"), false), None);
}

#[test]
fn package_graph_partitions_by_namespace_and_orders_dependencies_first() {
    let bundle = bundle(Path::new("/work/a/project"), LIB_A);
    let graph = PackageGraph::from_bundle(&bundle);
    let identities = graph
        .packages
        .iter()
        .map(|package| (package.identity.as_str(), package.kind, package.modules.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        [
            (CORE_PACKAGE, PackageKind::Core, vec![3]),
            ("file:main.jet", PackageKind::File, vec![0]),
            ("pkg:lib", PackageKind::Manifest, vec![1, 2]),
        ]
    );
    // main depends on lib and Core; lib depends on Core.
    assert_eq!(graph.packages[1].dependencies, [0, 2]);
    assert_eq!(graph.packages[2].dependencies, [0]);
    assert!(graph.is_acyclic());
    assert_eq!(graph.components(), [vec![0], vec![2], vec![1]]);
    assert_eq!(graph.member_label(&bundle, 2), "b.jet");
}

#[test]
fn digests_are_equal_across_checkout_paths_and_edits_cut_off_correctly() {
    let digests = |root: &str, lib_a: &str| {
        let bundle = bundle(Path::new(root), lib_a);
        let graph = PackageGraph::from_bundle(&bundle);
        let lib = graph
            .packages
            .iter()
            .position(|package| package.identity == "pkg:lib")
            .unwrap();
        (
            graph.source_digest(&bundle, lib, Some(b"package { name: \"lib\" }\n")),
            graph.interface_digest(&bundle, lib, &BTreeMap::new()),
        )
    };
    let (source, interface) = digests("/work/a/project", LIB_A);
    assert_eq!(digests("/elsewhere/b/project", LIB_A), (source.clone(), interface.clone()));

    // A comment edit changes the source digest only.
    let commented = LIB_A.replace("fn helper", "// the helper\nfn helper");
    let (comment_source, comment_interface) = digests("/work/a/project", &commented);
    assert_ne!(comment_source, source);
    assert_eq!(comment_interface, interface);

    // A private body edit and a new private function keep the interface.
    let private = LIB_A.replace("    1\n", "    2\n") + "\nfn other() -> Int {\n    3\n}\n";
    assert_eq!(digests("/work/a/project", &private).1, interface);

    // A public signature edit, or a new public function, changes it.
    let signature = LIB_A.replace("b: Int) -> Int", "b: Int) -> Float");
    assert_ne!(digests("/work/a/project", &signature).1, interface);
    let added = format!("{LIB_A}\npub fn more() -> Int {{\n    4\n}}\n");
    assert_ne!(digests("/work/a/project", &added).1, interface);

    // An inferred fact importers depend on changes the digest through the
    // checker's summaries.
    let bundle = bundle(Path::new("/work/a/project"), LIB_A);
    let graph = PackageGraph::from_bundle(&bundle);
    let lib = graph.packages.iter().position(|p| p.identity == "pkg:lib").unwrap();
    let summaries = BTreeMap::from([("pkg:lib::a.jet::add".to_string(), "effects:FS".to_string())]);
    assert_ne!(graph.interface_digest(&bundle, lib, &summaries), interface);
}
