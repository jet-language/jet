mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::collections::BTreeSet;
use std::path::PathBuf;

const SOURCE_PATH: &str = "examples/features/traits/impl_shape_matrix.jet";
const SOURCE: &str = include_str!("../examples/features/traits/impl_shape_matrix.jet");
const EXPECTED: &str =
    include_str!("../examples/features/expected/traits/impl_shape_matrix.out");

const DISPLAY: &str = "Display";
const DEBUG: &str = "Debug";
const EQUATABLE: &str = "Equatable";
const LABELED_CONTEXTS: &[&str] = &["bare", "debug", "pretty", "nested"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Auto,
    Explicit,
    Bundle,
    Unavailable,
}

#[derive(Debug, Clone, Copy)]
struct MatrixRow {
    owner: &'static str,
    binding: &'static str,
    label: &'static str,
    display: Resolution,
    debug: Resolution,
}

// Keep this list closed. The admission assertion below derives the admitted
// owner set from the fixture, so adding a sema-admitted owner without adding a
// row fails this test instead of silently shrinking the matrix.
const ROWS: &[MatrixRow] = &[
    MatrixRow {
        owner: "AutoOwner",
        binding: "auto",
        label: "auto",
        display: Resolution::Auto,
        debug: Resolution::Auto,
    },
    MatrixRow {
        owner: "DisplayOnlyOwner",
        binding: "display_only",
        label: "display-only",
        display: Resolution::Explicit,
        debug: Resolution::Auto,
    },
    MatrixRow {
        owner: "DebugOnlyOwner",
        binding: "debug_only",
        label: "debug-only",
        display: Resolution::Auto,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "DisplayDebugOwner",
        binding: "display_debug",
        label: "display-debug",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "EquatableOwner",
        binding: "equatable",
        label: "equatable",
        display: Resolution::Explicit,
        debug: Resolution::Auto,
    },
    MatrixRow {
        owner: "GenericOwner",
        binding: "generic",
        label: "generic",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "EnumOwner",
        binding: "enum_value",
        label: "enum",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "UnitOwner",
        binding: "unit",
        label: "unit",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "DistinctOwner",
        binding: "distinct",
        label: "distinct",
        display: Resolution::Bundle,
        // The distinct fixture has a sema-admitted Printable bundle only. Its
        // Debug cells intentionally use the admitted base value (`raw()`),
        // because a distinct's backend Debug impl is not a sema Debug impl.
        debug: Resolution::Unavailable,
    },
];

fn source_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SOURCE_PATH)
}

fn source_items(bundle: &jet::AST::ProgramBundle) -> Vec<jet::AST::Item> {
    bundle.modules[bundle.entry].items.clone()
}

fn local_nominals(items: &[jet::AST::Item]) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|item| match item {
            jet::AST::Item::Struct(def) => Some(def.name.clone()),
            jet::AST::Item::Enum(def) => Some(def.name.clone()),
            jet::AST::Item::Distinct(def) => Some(def.name.clone()),
            _ => None,
        })
        .collect()
}

fn explicit_impls(items: &[jet::AST::Item]) -> BTreeSet<(String, String)> {
    let mut result = BTreeSet::new();
    for item in items {
        match item {
            jet::AST::Item::Struct(def) => {
                for implementation in &def.trait_impls {
                    result.insert((def.name.clone(), implementation.trait_name.clone()));
                }
            }
            jet::AST::Item::Enum(def) => {
                for implementation in &def.trait_impls {
                    result.insert((def.name.clone(), implementation.trait_name.clone()));
                }
            }
            jet::AST::Item::Impl(implementation) => {
                if let Some(trait_name) = &implementation.trait_name {
                    result.insert((implementation.type_name.clone(), trait_name.clone()));
                }
            }
            _ => {}
        }
    }
    result
}

fn row_resolution(row: &MatrixRow, trait_name: &str) -> Resolution {
    match trait_name {
        DISPLAY => row.display,
        DEBUG => row.debug,
        _ => panic!("unsupported matrix trait `{trait_name}`"),
    }
}

fn sema_admits(registry: &jet::Traits::TraitRegistry, owner: &str, trait_name: &str) -> bool {
    match trait_name {
        DISPLAY => {
            registry.implements_trait(owner, DISPLAY) || registry.auto_printable.contains(owner)
        }
        DEBUG => registry.implements_trait(owner, DEBUG),
        _ => panic!("unsupported matrix trait `{trait_name}`"),
    }
}

fn matrix_admitted_pairs() -> BTreeSet<(String, String)> {
    ROWS.iter()
        .flat_map(|row| [DISPLAY, DEBUG].into_iter().filter_map(move |trait_name| {
            (row_resolution(row, trait_name) != Resolution::Unavailable)
                .then(|| (row.owner.to_string(), trait_name.to_string()))
        }))
        .collect()
}

fn sema_admitted_pairs(
    registry: &jet::Traits::TraitRegistry,
    local: &BTreeSet<String>,
) -> BTreeSet<(String, String)> {
    local
        .iter()
        .flat_map(|owner| {
            [DISPLAY, DEBUG].into_iter().filter_map(move |trait_name| {
                sema_admits(registry, owner, trait_name)
                    .then(|| (owner.clone(), trait_name.to_string()))
            })
        })
        .collect()
}

fn assert_matrix_cells() {
    assert_eq!(ROWS.len(), 9, "the closed impl-shape space changed");
    let owners: BTreeSet<_> = ROWS.iter().map(|row| row.owner).collect();
    assert_eq!(owners.len(), ROWS.len(), "matrix owners must be unique");
    let bindings: BTreeSet<_> = ROWS.iter().map(|row| row.binding).collect();
    assert_eq!(bindings.len(), ROWS.len(), "matrix bindings must be unique");
    let labels: BTreeSet<_> = ROWS.iter().map(|row| row.label).collect();
    assert_eq!(labels.len(), ROWS.len(), "matrix labels must be unique");

    for row in ROWS {
        for context in LABELED_CONTEXTS {
            let prefix = format!("{}/{context}=", row.label);
            assert_eq!(
                SOURCE.matches(prefix.as_str()).count(),
                1,
                "source needs one {context} cell for {}",
                row.owner
            );
            assert_eq!(
                EXPECTED.matches(prefix.as_str()).count(),
                1,
                "golden needs one {context} cell for {}",
                row.owner
            );
        }
        let print_call = format!("print(~{})", row.binding);
        assert_eq!(
            SOURCE.matches(print_call.as_str()).count(),
            1,
            "source needs one print(v) cell for {}",
            row.owner
        );
    }
    assert_eq!(
        SOURCE.matches("print(~").count(),
        ROWS.len(),
        "source must have one direct print cell per row"
    );
}

fn assert_admission(
    registry: &jet::Traits::TraitRegistry,
    explicit: &BTreeSet<(String, String)>,
    local: &BTreeSet<String>,
) {
    let admitted = sema_admitted_pairs(registry, local);
    let rows = matrix_admitted_pairs();
    assert_eq!(
        admitted, rows,
        "every sema-admitted local Display/Debug shape needs a matrix row"
    );

    for matrix_row in ROWS {
        assert_resolution(
            registry,
            explicit,
            matrix_row.owner,
            DISPLAY,
            matrix_row.display,
        );
        assert_resolution(
            registry,
            explicit,
            matrix_row.owner,
            DEBUG,
            matrix_row.debug,
        );
    }
}

fn assert_resolution(
    registry: &jet::Traits::TraitRegistry,
    explicit: &BTreeSet<(String, String)>,
    owner: &str,
    trait_name: &str,
    resolution: Resolution,
) {
    let has_explicit = explicit.contains(&(owner.to_string(), trait_name.to_string()));
    match resolution {
        Resolution::Explicit => {
            assert!(has_explicit, "{owner} needs an explicit {trait_name} impl");
            assert!(
                registry.implements_trait(owner, trait_name),
                "sema must admit {owner}.{trait_name}"
            );
        }
        Resolution::Auto => {
            assert!(!has_explicit, "{owner}.{trait_name} must not have a custom impl");
            if trait_name == DISPLAY {
                assert!(
                    registry.auto_printable.contains(owner),
                    "sema must admit auto {owner}.{trait_name}"
                );
            }
            if trait_name == DEBUG {
                assert!(
                    registry.auto_debug.contains(owner),
                    "sema must admit auto {owner}.{trait_name}"
                );
            }
        }
        Resolution::Bundle => {
            assert!(!has_explicit, "{owner}.{trait_name} must use its bundle");
            assert!(
                registry.implements_trait(owner, trait_name),
                "sema must admit bundled {owner}.{trait_name}"
            );
        }
        Resolution::Unavailable => {
            assert!(!has_explicit, "{owner}.{trait_name} is not sema-admitted");
            assert!(!registry.implements_trait(owner, trait_name));
            assert!(!registry.auto_printable.contains(owner));
            assert!(!registry.auto_debug.contains(owner));
        }
    }
}


fn assert_aot_and_admission_contract(
    registry: &jet::Traits::TraitRegistry,
    explicit: &BTreeSet<(String, String)>,
    local: &BTreeSet<String>,
) {
    assert_admission(registry, explicit, local);
}

#[test]
fn impl_shape_matrix_covers_all_contexts_and_resolution_seams() {
    assert_matrix_cells();

    let path = source_path();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).expect("matrix loads");
    let items_before_sema = source_items(&bundle);
    let explicit = explicit_impls(&items_before_sema);
    let local = local_nominals(&items_before_sema);
    let registry = jet::Traits::TraitRegistry::auto_derives_for_items(&items_before_sema);
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "matrix sema errors: {errors:?}");

    assert!(
        items_before_sema.iter().any(|item| matches!(
            item,
            jet::AST::Item::Struct(def)
                if def.name == "EquatableOwner"
                    && def.derives.iter().any(|(name, _)| name == EQUATABLE)
        )),
        "EquatableOwner must retain the derived-Equatable matrix shape"
    );

    let compiled = jet::compile_with_path(SOURCE, path.to_str().unwrap())
        .expect("matrix AOT codegen");

    // The closed matrix is the admission oracle; executable tier parity below
    // is the consumer-facing proof of its rendering behavior.
    assert_aot_and_admission_contract(&registry, &explicit, &local);

    // This runs the same fixture through release/AOT and the default Cranelift
    // tier. It is the executable half of the observable contract and golden gate.
    tir_support::assert_example_cli_tiers_agree("traits/impl_shape_matrix", EXPECTED);

    // The ordinary three-mode helper proves output parity, but it does not
    // prove that the default run stayed on the resident seam. Trace the same
    // fixture once so the matrix cannot pass through tier 0 only.
    let (code, stdout, stderr) =
        tir_support::jit_run_traced("impl_shape_matrix_native", SOURCE);
    assert_eq!(code, 0, "default `jet run` failed for impl_shape_matrix: {stderr}");
    assert_eq!(stdout, EXPECTED);
    assert!(
        stderr
            .lines()
            .any(|line| line.starts_with("run") && line.contains("tier1 native")),
        "impl_shape_matrix did not execute on the resident JIT: {stderr}"
    );
}

#[test]
fn impl_shape_matrix_retains_typed_operator_rhs() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/operators/mixed_types.jet");
    let bundle = jet::Loader::load_entry(path.to_str().unwrap()).expect("operator example loads");
    let rhs = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| {
            let jet::AST::Item::Impl(implementation) = item else {
                return None;
            };
            if implementation.type_name == "Money"
                && implementation.trait_name.as_deref() == Some("Add")
            {
                implementation.operator_rhs.clone()
            } else {
                None
            }
        });
    assert_eq!(rhs, Some(jet::AST::Type::Int));
}
