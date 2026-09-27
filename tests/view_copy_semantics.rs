mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use jet_foundation::MIR::{
    canonical_operation_identity, canonical_operation_payload, mir_program_digest,
    mir_view_copy_element_type, mir_view_copy_kind, MirArtifactBuildMode, MirArtifactKind,
    MirArtifactRequest, MirArtifactTarget, MirNominalRef, MirOperation, MirOptimizationPolicy,
    MirProgram, MirTagMarker, MirType, MirTypeId, MirTypeKind, MirViewCopyKind, MirValueId,
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
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    jet::Codegen::TIR::lower_checked_mir_program_for(
        &bundle,
        MirArtifactRequest::new(
            MirArtifactTarget::Cranelift,
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
fn copy_marker_changes_canonical_payload_and_identity() {
    let ordinary = MirOperation::Copy {
        value: MirValueId(7),
        materialize_view: false,
    };
    let materialized = MirOperation::Copy {
        value: MirValueId(7),
        materialize_view: true,
    };
    let ordinary_payload = canonical_operation_payload(&ordinary);
    let materialized_payload = canonical_operation_payload(&materialized);
    assert_ne!(ordinary_payload, materialized_payload);
    assert!(ordinary_payload.contains("\"materialize_view\":false"));
    assert!(materialized_payload.contains("\"materialize_view\":true"));
    assert_ne!(
        canonical_operation_identity(&ordinary),
        canonical_operation_identity(&materialized)
    );
}
#[test]
fn optimization_and_digest_preserve_materialization_marker() {
    let mir = lower_checked(OWNED_WINDOW_SOURCE);
    let optimized =
        jet_foundation::MIR::optimize_mir_program(&mir, &MirOptimizationPolicy::conservative())
            .expect("MIR optimization preserves the checked operation schema");
    assert!(optimized.functions.iter().any(|function| {
        function.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.operation,
                    MirOperation::Copy {
                        materialize_view: true,
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
                if let MirOperation::Copy {
                    materialize_view, ..
                } = &mut instruction.operation
                {
                    if *materialize_view {
                        *materialize_view = false;
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
                    materialize_view: true,
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
        source.contains("window :: value.after(\"\")\n    ~window"),
        "String evaluator materialization must lower through checked Jet `~`"
    );
    assert_eq!(
        source
            .matches("window :: value[0..<value.len()]\n    ~window")
            .count(),
        2,
        "List and Bytes evaluator materialization must use checked full windows"
    );
}

const OWNED_WINDOW_SOURCE: &str = r#"
fn own_first(values: [Int]) -> [Int] {
    return ~values[0..0]
}

fn run() {
    values := [1, 2]
    copied := own_first(values)
    values[0] = 8
    copied[0] = 9
    print(values[0])
    print(copied[0])

    nested := [[1, 2], [3, 4]]
    nested_copy := ~nested[0..0]
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
    owned_empty_text :: ~empty_text_view
    print(owned_empty_text.len())

    unicode_text := "π🙂"
    full_text_view :: unicode_text.after("")
    owned_unicode_text :: ~full_text_view
    print(owned_unicode_text == "π🙂")

    empty_values := [Int]{}
    empty_values_view :: empty_values[0..<empty_values.len()]
    owned_empty_values :: ~empty_values_view
    print(owned_empty_values.len())

    bytes := unicode_text.bytes()
    bytes_view :: bytes[0..<bytes.len()]
    owned_bytes :: ~bytes_view
    print(owned_bytes.len())
}
"#;
const LIST_STRING_SOURCE: &str = r#"
fn copy_values(values: [String]) -> [String] {
    return ~values[0..<values.len()]
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
    print(~view)
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
