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
    display: Resolution,
    debug: Resolution,
}

// Keep this list closed. The admission assertion below derives the admitted
// owner set from the fixture, so adding a sema-admitted owner without adding a
// row fails this test instead of silently shrinking the matrix.
const ROWS: &[MatrixRow] = &[
    MatrixRow {
        owner: "AutoOwner",
        display: Resolution::Auto,
        debug: Resolution::Auto,
    },
    MatrixRow {
        owner: "DisplayOnlyOwner",
        display: Resolution::Explicit,
        debug: Resolution::Auto,
    },
    MatrixRow {
        owner: "DebugOnlyOwner",
        display: Resolution::Auto,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "DisplayDebugOwner",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "EquatableOwner",
        display: Resolution::Explicit,
        debug: Resolution::Auto,
    },
    MatrixRow {
        owner: "GenericOwner",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "EnumOwner",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "UnitOwner",
        display: Resolution::Explicit,
        debug: Resolution::Explicit,
    },
    MatrixRow {
        owner: "DistinctOwner",
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

fn assert_admission(
    registry: &jet::Traits::TraitRegistry,
    explicit: &BTreeSet<(String, String)>,
    local: &BTreeSet<String>,
) {
    let admitted: BTreeSet<String> = local
        .iter()
        .filter_map(|owner| {
            let owner = owner.as_str();
            let display_admitted = registry.implements_trait(owner, DISPLAY)
                || registry.auto_printable.contains(owner);
            let debug_admitted = registry.implements_trait(owner, DEBUG);
            (display_admitted || debug_admitted).then(|| owner.to_string())
        })
        .collect();
    let rows: BTreeSet<String> = ROWS.iter().map(|row| row.owner.to_string()).collect();
    assert_eq!(
        admitted, rows,
        "every sema-admitted local Display/Debug owner needs a matrix row"
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

fn protocol_parts(trait_name: &str) -> (&'static str, &'static str, &'static str) {
    match trait_name {
        DISPLAY => ("JetDisplay", "__jet_Display", "display"),
        DEBUG => ("JetDebug", "__jet_Debug", "debug"),
        _ => panic!("unsupported matrix protocol `{trait_name}`"),
    }
}

fn assert_aot_resolution(rust: &str, owner: &str, trait_name: &str, resolution: Resolution) {
    let rust_owner = format!("__jet_{owner}");
    let (jet_trait, internal_trait, method) = protocol_parts(trait_name);
    let representation = format!("{jet_trait} for {rust_owner}");
    let custom_representation = format!("{internal_trait} for {rust_owner}");
    match resolution {
        Resolution::Explicit => {
            assert!(
                rust.contains(&representation),
                "AOT must emit {trait_name} bridge for {owner}"
            );
            assert!(
                rust.contains(&custom_representation),
                "AOT must emit the custom {trait_name} impl for {owner}"
            );
            assert!(
                rust.contains(&format!("as {internal_trait}>::{method}(self)")),
                "AOT bridge must call {owner}.{method}"
            );
        }
        Resolution::Auto => {
            assert!(
                rust.contains(&representation),
                "AOT must emit automatic {trait_name} rendering for {owner}"
            );
            assert!(
                !rust.contains(&custom_representation),
                "AOT must not invent a custom {trait_name} impl for {owner}"
            );
        }
        Resolution::Bundle => {
            assert!(
                rust.contains(&representation),
                "AOT must emit the {trait_name} bundle for {owner}"
            );
            assert!(
                !rust.contains(&custom_representation),
                "AOT must not invent a custom {trait_name} impl for {owner}"
            );
        }
        Resolution::Unavailable => {
            assert!(
                !rust.contains(&custom_representation),
                "AOT must not emit a custom {trait_name} impl for unavailable {owner}"
            );
        }
    }
}

fn tir_owners(
    program: &jet::Codegen::TIR::JitProgram,
    trait_name: &str,
    method: &str,
) -> BTreeSet<String> {
    program
        .trait_method_owners
        .get(&(trait_name.to_string(), method.to_string()))
        .into_iter()
        .flatten()
        .cloned()
        .collect()
}

fn expected_explicit_owners(trait_name: &str) -> BTreeSet<String> {
    ROWS.iter()
        .filter(|row| {
            let resolution = match trait_name {
                DISPLAY => row.display,
                DEBUG => row.debug,
                _ => panic!("unsupported matrix trait `{trait_name}`"),
            };
            resolution == Resolution::Explicit
        })
        .map(|row| row.owner.to_string())
        .collect::<BTreeSet<_>>()
}

#[test]
fn impl_shape_matrix_covers_all_contexts_and_resolution_seams() {
    assert_eq!(ROWS.len(), 9, "the closed impl-shape space changed");
    for context in ["/bare=", "/debug=", "/pretty=", "/nested="] {
        assert_eq!(
            EXPECTED.matches(context).count(),
            ROWS.len(),
            "golden lost a {context} context"
        );
    }

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

    // Sema admission + the ratchet. `local` is the exact source nominal set,
    // before sema adds derived items.
    assert_admission(&registry, &explicit, &local);
    assert!(
        items_before_sema.iter().any(|item| matches!(
            item,
            jet::AST::Item::Struct(def)
                if def.name == "EquatableOwner"
                    && def.derives.iter().any(|(name, _)| name == EQUATABLE)
        )),
        "EquatableOwner must retain the derived-Equatable matrix shape"
    );

    // AOT bridge emission and TIR's shared method-owner ledger.
    let compiled = jet::compile_with_path(SOURCE, path.to_str().unwrap())
        .expect("matrix AOT codegen");
    for matrix_row in ROWS {
        assert_aot_resolution(
            &compiled.rust,
            matrix_row.owner,
            DISPLAY,
            matrix_row.display,
        );
        assert_aot_resolution(
            &compiled.rust,
            matrix_row.owner,
            DEBUG,
            matrix_row.debug,
        );
    }

    let program = jet::Codegen::TIR::lower_jit_program(&bundle).expect("matrix lowers to TIR");
    for (trait_name, method) in [(DISPLAY, "display"), (DEBUG, "debug")] {
        assert_eq!(
            tir_owners(&program, trait_name, method),
            expected_explicit_owners(trait_name),
            "TIR method-owner lookup drifted for {trait_name}.{method}"
        );
        for matrix_row in ROWS {
            let resolution = match trait_name {
                DISPLAY => matrix_row.display,
                DEBUG => matrix_row.debug,
                _ => unreachable!(),
            };
            if resolution == Resolution::Explicit {
                let function_name = format!("{}::{method}", matrix_row.owner);
                assert!(
                    program.funcs.iter().any(|function| function.name == function_name),
                    "TIR must lower {function_name} for the JIT func_ids lookup"
                );
            }
        }
    }

    // This runs the same fixture through release/AOT, default Cranelift (whose
    // private func_ids table resolves the custom methods), and forced TIR eval.
    // It is the executable half of the four-seam contract and the golden gate.
    tir_support::assert_example_cli_tiers_agree("traits/impl_shape_matrix", EXPECTED);
}
