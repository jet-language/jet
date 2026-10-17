mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use jet_foundation::MIR::{
    canonical_operation_identity, canonical_operation_payload, mir_program_digest,
    mir_view_copy_element_type, mir_view_copy_kind, MirArtifactBuildMode, MirArtifactKind,
    MirArtifactRequest, MirArtifactTarget, MirCopyFact, MirNominalRef, MirOperation,
    MirOptimizationPolicy, MirProgram, MirTagMarker, MirType, MirTypeId, MirTypeKind,
    MirViewCopyKind, MirValueId,
};

fn apply_type(name: &str, args: Vec<MirType>) -> MirType {
    MirType::from_kind(MirTypeKind::Apply {
        name: MirNominalRef {
            id: MirTypeId(1),
            name: name.to_string(),
        },
        args,
    })
}

fn named_type(name: &str) -> MirType {
    apply_type(name, Vec::new())
}

fn view_of(element: MirType) -> MirType {
    apply_type("View", vec![element])
}
fn lower_checked(source: &str) -> MirProgram {
    let root = common::unique_tmp("jet_view_copy_semantics");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("package.jet"),
        "name: \"view_copy_semantics\"\nversion: \"1.0.0\"\n",
    )
    .unwrap();
    let entry = root.join("main.jet");
    std::fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let (diagnostics, _facts, nominals) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics.into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    jet::Codegen::TIR::lower_checked_mir_program_for(
        &bundle,
        &nominals,
        MirArtifactRequest::new(
            MirArtifactTarget::Interpreter,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        ),
    )
    .expect("checked source lowers through canonical MIR")
    .0
}
fn emitted_kernel_call(rust: &str, symbol: &str) -> bool {
    let needle = format!("{symbol}(");
    let declaration = format!("fn {symbol}");
    rust.lines().any(|line| {
        !line.contains(&declaration) && line.contains(&needle)
    })
}

#[test]
fn canonical_view_copy_kind_preserves_string_and_nested_shapes() {
    let string_view = view_of(named_type("str"));
    let owned_string_view = view_of(MirType::from_kind(MirTypeKind::String));
    let nested_view = view_of(MirType::from_kind(MirTypeKind::List(Box::new(
        MirType::from_kind(MirTypeKind::String),
    ))));
    let tagged_nested_view = MirType::from_kind(MirTypeKind::Tagged {
        marker: MirTagMarker::User("ReadAlias".to_string()),
        inner: Box::new(nested_view.clone()),
    });

    assert_eq!(mir_view_copy_kind(&string_view), Some(MirViewCopyKind::String));
    assert_eq!(mir_view_copy_kind(&owned_string_view), Some(MirViewCopyKind::List));
    assert_eq!(mir_view_copy_kind(&nested_view), Some(MirViewCopyKind::List));
    assert_eq!(mir_view_copy_kind(&tagged_nested_view), Some(MirViewCopyKind::List));
    assert!(mir_view_copy_element_type(&nested_view)
        .expect("nested view element")
        .is_list());

    let string_target = MirType::from_kind(MirTypeKind::String);
    let list_target = MirType::from_kind(MirTypeKind::List(Box::new(
        MirType::from_kind(MirTypeKind::List(Box::new(MirType::from_kind(
            MirTypeKind::String,
        )))),
    )));
    // These nominal spellings are already producer-normalized canonical forms.
    let canonical_string_target = named_type("String");
    let canonical_list_target = apply_type(
        "List",
        vec![MirType::from_kind(MirTypeKind::String)],
    );
    let tagged_canonical_string = MirType::from_kind(MirTypeKind::Tagged {
        marker: MirTagMarker::User("OwnedAlias".to_string()),
        inner: Box::new(canonical_string_target.clone()),
    });
    let fake_string_view = apply_type("foo.View", vec![named_type("str")]);
    let fake_list_view = apply_type("foo.ViewMut", vec![MirType::from_kind(MirTypeKind::Int)]);
    let malformed_view_placeholder = apply_type("View<>", vec![named_type("str")]);
    let fake_string_target = named_type("foo.String");
    let fake_list_target = apply_type(
        "foo.List",
        vec![MirType::from_kind(MirTypeKind::String)],
    );
    let malformed_list_placeholder = apply_type(
        "List<>",
        vec![MirType::from_kind(MirTypeKind::String)],
    );

    assert_eq!(mir_view_copy_kind(&fake_string_view), None);
    assert_eq!(mir_view_copy_kind(&fake_list_view), None);
    assert_eq!(mir_view_copy_kind(&malformed_view_placeholder), None);
    assert!(!MirViewCopyKind::String.target_matches(&fake_string_target));
    assert!(!MirViewCopyKind::List.target_matches(&fake_list_target));
    assert!(!MirViewCopyKind::List.target_matches(&malformed_list_placeholder));
    assert!(MirViewCopyKind::String.target_matches(&canonical_string_target));
    assert!(MirViewCopyKind::String.target_matches(&tagged_canonical_string));
    assert!(MirViewCopyKind::List.target_matches(&canonical_list_target));
    assert!(MirViewCopyKind::String.target_matches(&string_target));
    assert!(!MirViewCopyKind::String.target_matches(&list_target));
    assert!(MirViewCopyKind::List.target_matches(&list_target));
}

#[test]
fn copy_fact_changes_canonical_payload_and_identity() {
    let ordinary = MirOperation::Copy {
        value: MirValueId(7),
        fact: MirCopyFact::Materialize,
    };
    let materialized = MirOperation::Copy {
        value: MirValueId(7),
        fact: MirCopyFact::ViewMaterialize,
    };
    let ordinary_payload = canonical_operation_payload(&ordinary);
    let materialized_payload = canonical_operation_payload(&materialized);
    assert_ne!(ordinary_payload, materialized_payload);
    assert!(ordinary_payload.contains("\"copy_fact\":\"materialize\""));
    assert!(materialized_payload.contains("\"copy_fact\":\"view_materialize\""));
    assert_ne!(
        canonical_operation_identity(&ordinary),
        canonical_operation_identity(&materialized)
    );
}
#[test]
fn optimization_and_digest_preserve_materialization_marker() {
    let mir = lower_checked(OWNED_WINDOW_SOURCE);
    let optimized =
        jet_foundation::MIR::optimize_mir_program(mir, &MirOptimizationPolicy::conservative())
            .expect("MIR optimization preserves the checked operation schema");
    assert!(optimized.functions.iter().any(|function| {
        function.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.operation,
                    MirOperation::Copy {
                        fact: MirCopyFact::ViewMaterialize,
                        ..
                    }
                )
            })
        })
    }));

    let mut flipped = optimized.clone();
    let mut changed = false;
    for function in &mut flipped.functions {
        for block in &mut function.blocks {
            for instruction in &mut block.instructions {
                if let MirOperation::Copy { fact, .. } = &mut instruction.operation {
                    if *fact == MirCopyFact::ViewMaterialize {
                        *fact = MirCopyFact::Materialize;
                        changed = true;
                    }
                }
            }
        }
    }
    assert!(changed, "fixture must contain a materializing Copy");
    assert_ne!(mir_program_digest(&optimized), mir_program_digest(&flipped));
}


#[test]
fn checked_source_windows_lower_to_marked_mir() {
    let mir = lower_checked(EMPTY_STRING_AND_LIST_SOURCE);
    let marked = mir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .filter(|instruction| {
            matches!(
                &instruction.operation,
                MirOperation::Copy {
                    fact: MirCopyFact::ViewMaterialize,
                    ..
                }
            )
        })
        .count();
    assert!(
        marked >= 4,
        "the checked fixture's four windows must lower to marked Copy operations: {marked}"
    );
}

#[test]
fn user_write_calls_keep_range_arguments_as_owner_places() {
    let source = std::fs::read_to_string("Examples/features/memory/range_write_windows.jet").unwrap();
    let mir = lower_checked(&source);
    let mut range_calls = 0;
    for function in &mir.functions {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let MirOperation::Call { callee: jet_foundation::MIR::MirCallee::User(_), args, .. } =
                &instruction.operation else { continue };
            for arg in args {
                if arg.access != jet_foundation::MIR::MirAccess::Write { continue; }
                if arg.place.is_some_and(|id| function.places.iter().any(|place| {
                    place.id == id && matches!(place.projections.last(),
                        Some(jet_foundation::MIR::MirProjection::Range { .. }))
                })) {
                    range_calls += 1;
                }
            }
        }
    }
    assert_eq!(range_calls, 2,
        "both direct user calls must lend a range of their owner's list, not an owned slice");
}

#[test]
fn evaluator_view_materialization_helpers_use_checked_jet_route() {
    let source = include_str!("../Compiler/JetEval/Source/Evaluator.jet");
    for helper in [
        "jet_eval_materialize_string_window",
        "jet_eval_materialize_list_window",
        "jet_eval_materialize_bytes_window",
    ] {
        assert!(
            source.contains(&format!("fn {helper}(")),
            "Source/Evaluator.jet must define {helper}"
        );
    }
    assert!(
        source.contains("window :: value.after(\"\")\n    window.copy()"),
        "String evaluator materialization must lower through checked Jet `.copy()`"
    );
    assert_eq!(
        source
            .matches("window :: value[0..<value.len()]\n    window.copy()")
            .count(),
        2,
        "List and Bytes evaluator materialization must use checked full windows"
    );
}

const OWNED_WINDOW_SOURCE: &str = r#"
fn own_first(values: [Int]) -> [Int] {
    return values[0..0].copy()
}

fn run() {
    values := [1, 2]
    copied := own_first(values)
    values[0] = 8
    copied[0] = 9
    print(values[0])
    print(copied[0])

    nested := [[1, 2], [3, 4]]
    nested_copy := nested[0..0].copy()
    nested[0][0] = 6
    nested_copy[0][0] = 7
    print(nested[0][0])
    print(nested_copy[0][0])
}
"#;

const EMPTY_STRING_AND_LIST_SOURCE: &str = r#"
fn run() {
    empty_string := ""
    empty_text_view :: empty_string.after("")
    owned_empty_text :: empty_text_view.copy()
    print(owned_empty_text.len())

    unicode_text := "π🙂"
    full_text_view :: unicode_text.after("")
    owned_unicode_text :: full_text_view.copy()
    print(owned_unicode_text == "π🙂")

    empty_values := [Int]{}
    empty_values_view :: empty_values[0..<empty_values.len()]
    owned_empty_values :: empty_values_view.copy()
    print(owned_empty_values.len())

    bytes := unicode_text.bytes()
    bytes_view :: bytes[0..<bytes.len()]
    owned_bytes :: bytes_view.copy()
    print(owned_bytes.len())
}
"#;
const LIST_STRING_SOURCE: &str = r#"
fn copy_values(values: [String]) -> [String] {
    return values[0..<values.len()].copy()
}

fn run() {
    values := ["a", "b"]
    copied := copy_values(values)
    values[0] = "source"
    copied[0] = "copy"
    print(values[0])
    print(copied[0])
}
"#;

#[test]
fn empty_unicode_and_bytes_windows_agree_across_native_tiers() {
    assert!(
        tir_support::have_rustc(),
        "view-copy native-tier acceptance requires AOT rustc proof"
    );
    tir_support::assert_tiers_agree(
        "view_copy_empty_unicode_bytes",
        EMPTY_STRING_AND_LIST_SOURCE,
        "0\ntrue\n0\n6\n",
    );
}

#[test]
fn empty_unicode_and_bytes_windows_agree_on_awaited_web() {
    tir_support::assert_awaited_web_tier(
        "view_copy_empty_unicode_bytes_web",
        EMPTY_STRING_AND_LIST_SOURCE,
        "0\ntrue\n0\n6\n",
    );
}

#[test]
fn owned_window_escape_and_nested_mutation_agree_across_native_tiers() {
    assert!(
        tir_support::have_rustc(),
        "view-copy native-tier acceptance requires AOT rustc proof"
    );
    tir_support::assert_tiers_agree("view_copy_owned_window", OWNED_WINDOW_SOURCE, "8\n9\n6\n7\n");
}

#[test]
fn owned_window_escape_and_nested_mutation_agree_on_awaited_web() {
    tir_support::assert_awaited_web_tier(
        "view_copy_owned_window_web",
        OWNED_WINDOW_SOURCE,
        "8\n9\n6\n7\n",
    );
}

#[test]
fn string_view_materialization_uses_string_kernel_and_list_view_uses_list_kernel() {
    let string_source = r#"
fn run() {
    text := "  x  "
    view :: text.trim()
    print(view.copy())
}
"#;
    let string_compiled = jet::compile(string_source).expect("String View materializes");
    assert!(
        emitted_kernel_call(&string_compiled.rust, "jet_string_view_copy"),
        "String view lowering must emit a call-site use of jet_string_view_copy"
    );
    assert!(
        !emitted_kernel_call(&string_compiled.rust, "jet_view_copy"),
        "String view lowering must not call the List kernel"
    );

    let list_compiled = jet::compile(LIST_STRING_SOURCE).expect("View<String> materializes");
    assert!(
        emitted_kernel_call(&list_compiled.rust, "jet_view_copy"),
        "List<String> view lowering must emit a call-site use of jet_view_copy"
    );
    assert!(
        !emitted_kernel_call(&list_compiled.rust, "jet_string_view_copy"),
        "List<String> view lowering must not call the String kernel"
    );
}
#[test]
fn list_string_view_materialization_runs_across_native_tiers() {
    assert!(
        tir_support::have_rustc(),
        "List<String> view acceptance requires AOT rustc proof"
    );
    tir_support::assert_tiers_agree(
        "view_copy_list_string_runtime",
        LIST_STRING_SOURCE,
        "source\ncopy\n",
    );
}

#[test]
fn list_string_view_materialization_runs_on_awaited_web() {
    tir_support::assert_awaited_web_tier(
        "view_copy_list_string_runtime_web",
        LIST_STRING_SOURCE,
        "source\ncopy\n",
    );
}

const STRING_VIEW_RUNTIME_SOURCE: &str = r##"
fn run() {
    text := "  key  "
    key :: text.trim()
    counts := [String:Int]{}
    counts[key] = 7
    counts[key] = 9
    print(counts["key"])
    print(key.is_empty())
    print(key.starts_with("k"))
    print(key.ends_with("y"))
    print(key.contains("e"))
    print(key.count_bytes())
    print(key.repeat(2))
    print(key.replace("e", "a"))
    print(key.slice(0, 2))
    print(key.split("e").len())
    print(key.lines().len())
    print(key.chars().len())
    print(["a", "b"].join(key))
    whitespace := " \t "
    empty :: whitespace.trim()
    print(empty.is_empty())
    print(empty.starts_with(""))
    loop line in ["  # comment  ", "  value  "] {
        trimmed :: line.trim()
        if !trimmed.is_empty() && !trimmed.starts_with("#") -> print(~trimmed)
    }
}
"##;

#[test]
fn string_view_runtime_read_leaves_accept_text_without_materialization() {
    let compiled = jet::compile(STRING_VIEW_RUNTIME_SOURCE)
        .expect("checked string views can lend text to runtime read parameters");
    // Builtin receivers and indexed map writes borrow the same view carrier.
    // Only the explicit ~trimmed above may materialize it.
    for signature in [
        "fn jet_string_is_empty(text: &str)",
        "fn jet_string_repeat(text: &str,",
        "fn jet_string_replace(text: &str, from: &str, to: &str)",
        "fn jet_string_count_bytes(text: &str)",
        "fn jet_string_split(s: &str,",
        "fn jet_string_lines(s: &str)",
        "fn jet_string_chars(s: &str)",
        "fn jet_string_slice_builtin(s: &str,",
    ] {
        assert!(compiled.rust.contains(signature), "missing read ABI: {signature}");
    }
    assert!(
        compiled.rust.contains("key: &str,\n    f: F,"),
        "map update must borrow text and own its key only on insertion"
    );
    for line in compiled.rust.lines().filter(|line| {
        !line.trim_start().starts_with("fn ")
            && (line.contains("jet_map_update_string(")
                || line.contains("jet_string_is_empty(")
                || line.contains("jet_string_starts_with("))
    }) {
        assert!(!line.contains("jet_string_view_copy("), "{line}");
    }
}

#[test]
fn string_view_runtime_text_borrow_contract_covers_sibling_fragments() {
    for (source, signature) in [
        (
            include_str!("../crates/jet-codegen/src/Prelude/Core/Fmt.rs"),
            "fn jet_fmt_pad(text: &str, width: i64, fill: &str)",
        ),
        (
            include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs"),
            "fn jet_text_caseless_eq(a: &str, b: &str)",
        ),
        (
            include_str!("../crates/jet-codegen/src/Prelude/Core/Time.rs"),
            "fn jet_time_parse_rfc3339(s: &str)",
        ),
        (
            include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs"),
            "fn jet_std_io_stdout_write(_s: &JetStdout, text: &str)",
        ),
        (
            include_str!("../crates/jet-codegen/src/Prelude/Core/CsvQueryRead.rs"),
            "fn jet_data_query_read_bounded(path: &str, max: i64)",
        ),
        (
            include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPClient.rs"),
            "fn jet_http_client_request_new(method: &str, url: &str)",
        ),
        (
            include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs"),
            "fn jet_net_dns_a_at(server: &str, name: &str, ms: i64)",
        ),
    ] {
        assert!(source.contains(signature), "missing read ABI: {signature}");
    }
}

#[test]
fn string_view_runtime_reads_agree_across_native_tiers() {
    tir_support::assert_tiers_agree(
        "string_view_runtime_reads",
        STRING_VIEW_RUNTIME_SOURCE,
        "9\nfalse\ntrue\ntrue\ntrue\n3\nkeykey\nkay\nke\n2\n1\n3\nakeyb\ntrue\ntrue\nvalue\n",
    );
}
