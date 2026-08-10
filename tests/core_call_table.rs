//! D-ONCE-LAW1=A: Core-call rows have one home and engine projections.

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn quoted(text: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = text;
    while let Some((_, after_open)) = rest.split_once('"') {
        let Some((value, after_close)) = after_open.split_once('"') else {
            break;
        };
        values.push(value);
        rest = after_close;
    }
    values
}

fn arm_pairs(source: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut pattern = String::new();
    for line in source.lines() {
        if pattern.is_empty() && !line.trim_start().starts_with("(\"") {
            continue;
        }
        pattern.push_str(line);
        if !line.contains("=>") {
            continue;
        }
        let tuple = pattern
            .split_once("=>")
            .map(|(head, _)| head)
            .unwrap_or(&pattern)
            .trim_start()
            .trim_start_matches('(')
            .split_once(')')
            .map(|(tuple, _)| tuple)
            .unwrap_or_default();
        if let Some((modules, members)) = tuple.split_once(',') {
            for module in quoted(modules) {
                for member in quoted(members) {
                    pairs.push((module.to_string(), member.to_string()));
                }
            }
        }
        pattern.clear();
    }
    pairs
}

fn pure_route_pairs(source: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in source.lines() {
        let Some((head, _)) = line.split_once("=>") else {
            continue;
        };
        let Some((_, route_and_member)) = head.split_once("CoreCallPureRoute::") else {
            continue;
        };
        let route = route_and_member
            .split(|character| matches!(character, ',' | ')' | ' '))
            .next()
            .unwrap_or_default();
        let Some((_, members)) = head.split_once(',') else {
            continue;
        };
        for member in quoted(members) {
            pairs.push((route.to_string(), member.to_string()));
        }
    }
    pairs
}

#[test]
fn core_projection_is_complete_both_directions() {
    assert_eq!(
        arm_pairs("(\"core.a\" | \"core.b\", \"x\" | \"y\") => value"),
        [
            ("core.a".to_string(), "x".to_string()),
            ("core.a".to_string(), "y".to_string()),
            ("core.b".to_string(), "x".to_string()),
            ("core.b".to_string(), "y".to_string()),
        ]
    );
    assert_eq!(
        pure_route_pairs("(CoreCallPureRoute::Mime, \"parse\" | \"extension\") => value"),
        [
            ("Mime".to_string(), "parse".to_string()),
            ("Mime".to_string(), "extension".to_string()),
        ]
    );
    let mut keys = HashSet::new();
    let mut receiver_keys = HashSet::new();
    for row in jet::Syntax::CORE_CALLS {
        assert_eq!(row.signature.arity, row.signature.borrow_mask.len());
        assert!(
            row.signature.arity <= row.signature.max_arity,
            "invalid arity range for {}.{}",
            row.module,
            row.member
        );
        assert_eq!(row.fallibility, jet::Syntax::CoreCallFallibility::Sema);
        if row.is_receiver() {
            assert!(row.module.is_empty());
            assert!(!row.has_direct_symbol());
            assert!(
                row.receiver_types
                    .iter()
                    .all(|receiver| receiver_keys.insert(((*receiver).to_string(), row.member.to_string()))),
                "duplicate receiver Core row: {:?}.{}",
                row.receiver_types,
                row.member
            );
            for receiver in row.receiver_types {
                assert_eq!(
                    jet::Syntax::core_receiver_method(receiver, row.member),
                    Some(row)
                );
            }
            continue;
        }
        assert!(
            keys.insert((row.module.to_string(), row.member.to_string())),
            "duplicate Core call row: {}.{}",
            row.module,
            row.member
        );
        assert_eq!(row.has_direct_symbol(), row.aot_direct);
        assert_eq!(
            row.effect(),
            jet_foundation::Effects::core_effect(row.module, row.member)
        );
        assert_eq!(
            row.sink_class(),
            jet_foundation::Syntax::sink_row(row.module, row.member).map(|sink| sink.class)
        );
        assert_eq!(jet::Syntax::core_call(row.module, row.member), Some(row));
    }
    assert!(keys.len() > 500, "Core call table lost rows: {}", keys.len());
    assert!(
        receiver_keys.len() > 90,
        "receiver Core table lost rows: {}",
        receiver_keys.len()
    );

    let parity = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let evaluate = parity
        .split_once("pub(super) fn evaluate_method")
        .map(|(source, _)| source)
        .expect("CorePureParity evaluator boundary");
    assert!(
        !evaluate.contains("(\"core."),
        "pure evaluator retained a second module/member membership table"
    );
    let table_pure: HashSet<(String, String)> = jet::Syntax::CORE_CALLS
        .iter()
        .filter(|row| !row.is_receiver() && row.pure_route != jet::Syntax::CoreCallPureRoute::None)
        .map(|row| (format!("{:?}", row.pure_route), row.member.to_string()))
        .collect();
    let consumer_pure: HashSet<(String, String)> = pure_route_pairs(evaluate).into_iter().collect();
    assert_eq!(
        consumer_pure, table_pure,
        "pure route consumer/table mismatch"
    );
    let method_start = parity
        .find("pub(super) fn evaluate_method")
        .expect("CorePureParity receiver evaluator");
    let method_end = parity[method_start..]
        .find("pub(super) fn sketch_add")
        .map(|offset| method_start + offset)
        .unwrap_or(parity.len());
    let receiver_source = &parity[method_start..method_end];
    for (receiver, member) in arm_pairs(receiver_source) {
        assert!(
            jet::Syntax::core_receiver_method(&receiver, &member).is_some(),
            "receiver evaluator owns an unregistered method: {receiver}.{member}"
        );
    }
    let table_receivers: HashSet<(String, String)> = jet::Syntax::CORE_CALLS
        .iter()
        .filter(|row| row.is_receiver())
        .flat_map(|row| {
            row.receiver_types
                .iter()
                .map(move |receiver| ((*receiver).to_string(), row.member.to_string()))
        })
        .collect();
    let consumer_receivers: HashSet<(String, String)> = arm_pairs(receiver_source).into_iter().collect();
    assert_eq!(
        consumer_receivers, table_receivers,
        "receiver evaluator/table mismatch"
    );

    let emit = read("crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs");
    assert!(
        emit.contains("crate::Syntax::core_call(module, method)?"),
        "AOT emit no longer projects from the foundation Core-call table"
    );
    assert!(
        !emit.contains("const PLAIN_CORE_CALLS"),
        "AOT emit regained a hand-kept Core-call list"
    );
    let bespoke = emit
        .find("// #1635: every arm below stays bespoke")
        .expect("bespoke Core-call emission is named");
    let direct_keys: HashSet<(String, String)> = jet::Syntax::CORE_CALLS
        .iter()
        .filter(|row| row.aot_direct && !row.is_receiver())
        .map(|row| (row.module.to_string(), row.member.to_string()))
        .collect();
    let shadowed: Vec<String> = arm_pairs(&emit[bespoke..])
        .into_iter()
        .filter(|pair| direct_keys.contains(pair))
        .map(|(module, member)| format!("{module}.{member}"))
        .collect();
    assert!(
        shadowed.is_empty(),
        "bespoke AOT arms repeat foundation rows:\n{}",
        shadowed.join("\n")
    );
}

#[test]
fn one_fake_record_projects_without_a_consumer_arm() {
    const FAKE: &[jet_foundation::Syntax::CoreCallRecord] =
        &[jet_foundation::Syntax::CoreCallRecord::new(
            "core.fake",
            "only",
            "jet_fake_only",
            true,
            &[],
        )];

    let row = jet_foundation::Syntax::core_call_in(FAKE, "core.fake", "only")
        .expect("a data-only Core row must be visible through the generic lookup");
    assert_eq!(row.signature.arity, 0);
    assert_eq!(row.symbol.name(), "jet_fake_only");
    let candidates = row.jit_symbol_candidates();
    assert!(candidates.contains(&"jet_fake_only".to_string()));
    assert!(candidates.contains(&"jet_jit_fake_only".to_string()));
    assert_eq!(
        jet_foundation::Syntax::core_call_in(FAKE, "core.fake", "missing"),
        None
    );
}

#[test]
fn sema_tir_and_comptime_route_plain_calls_through_the_record() {
    let sema = read("crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs");
    assert!(
        sema.contains("Syntax::core_call(module, name)"),
        "sema effect routing no longer reads the Core-call record"
    );
    assert!(
        sema.contains("core_fixed_sig_for_row(row)"),
        "sema typed signatures no longer project through the Core-call record"
    );
    let fixed_sigs = read("crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs");
    assert!(
        fixed_sigs.contains("pub fn core_fixed_sig_for_row"),
        "sema has no typed-signature adapter for Core-call rows"
    );

    let tir = read("crates/jet-codegen/src/Codegen/TIR/subset/core_calls.rs");
    assert!(
        tir.contains("crate::Syntax::core_call(module, method)"),
        "TIR coverage no longer reads the Core-call record"
    );

    let comptime = read("crates/jet-comptime/src/Comptime/Methods/core_calls.rs");
    assert!(
        comptime.contains("jet_foundation::Syntax::core_call(module, method)"),
        "comptime dispatch no longer reads the Core-call record"
    );
    assert!(
        comptime.contains("core_call_allows_pure_parity(row)"),
        "comptime pure parity is not gated by the Core-call record"
    );
    assert!(
        comptime.contains("core_pure_parity::evaluate(row"),
        "comptime does not pass the canonical row into the pure evaluator"
    );
    assert!(
        !comptime.contains("const CORE_CALLS") && !comptime.contains("const PLAIN_CORE_CALLS"),
        "comptime regained a second Core-call table"
    );

    let interpreter = read("crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs");
    assert!(
        interpreter.contains("jet_foundation::Syntax::core_call(module, method)"),
        "TIR evaluation no longer reads the Core-call record"
    );

    let jit = read("crates/jet-jit/src/jit/lower_ctx.rs");
    assert!(
        jit.contains("jet_foundation::Syntax::core_call(module, method)"),
        "JIT lowering no longer reads the Core-call record"
    );
    assert!(
        jit.contains("lower_recorded_core_call") && jit.contains("self.host.lookup(&symbol)"),
        "JIT lowering has no table-driven direct host projection"
    );
    assert!(
        read("crates/jet-jit/src/lib.rs").contains("pub(crate) fn lookup(&self, symbol: &str)"),
        "JIT host declarations do not expose their generated symbol lookup"
    );

    let ambient = read("crates/jet-jit/src/ambient_interp.rs");
    assert!(
        ambient.contains("jet_foundation::Syntax::core_call(module, method)"),
        "the interpreter ambient adapter no longer reads the Core-call record"
    );
    let parity = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    assert!(
        parity.contains("core_receiver_method(type_name, method)"),
        "receiver parity does not resolve through the shared Core-call table"
    );
}

#[test]
fn plain_row_guards_reject_second_tables_and_missing_projection_hooks() {
    let table = read("crates/jet-foundation/src/Syntax/core_calls.rs");
    assert!(table.contains("pub const CORE_CALLS"));
    assert!(table.contains("pub struct CoreCallSignature"));
    assert!(table.contains("pub fallibility: CoreCallFallibility"));
    assert!(table.contains("jit_direct: bool"));
    assert!(table.contains("receiver_types: &'static [&'static str]"));

    for path in [
        "crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs",
        "crates/jet-codegen/src/Codegen/TIR/subset/core_calls.rs",
        "crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs",
        "crates/jet-comptime/src/Comptime/Methods/core_calls.rs",
        "crates/jet-jit/src/jit/lower_ctx.rs",
        "crates/jet-jit/src/ambient_interp.rs",
    ] {
        let source = read(path);
        assert!(
            !source.contains("AOT_CORE_CALLS") && !source.contains("PLAIN_CORE_CALLS"),
            "{path} retained a retired second Core-call table"
        );
    }
}

#[test]
fn receiver_and_plain_consumer_sets_have_one_table_home() {
    let table = read("crates/jet-foundation/src/Syntax/core_calls.rs");
    let parity = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let jit = read("crates/jet-jit/src/jit/lower_ctx.rs");

    assert!(
        table.contains("pub fn core_receiver_method"),
        "receiver lookup is not exported from the canonical table"
    );
    assert!(
        parity.contains("core_receiver_method(type_name, method)?"),
        "receiver evaluator has a hand-kept membership gate"
    );
    assert!(
        jit.contains("fn lower_recorded_core_call")
            && jit.contains("jit_symbol_candidates()"),
        "JIT has no row-driven receiver/plain projection seam"
    );

    // Exercise the mismatch detector itself with both directions. This keeps
    // the guard honest: a consumer-only row and a table-only row are distinct
    // failures, not one collapsed length check.
    fn mismatch(table: &[(&str, &str)], consumer: &[(&str, &str)]) -> Vec<String> {
        let table = table.iter().copied().collect::<HashSet<_>>();
        let consumer = consumer.iter().copied().collect::<HashSet<_>>();
        table
            .difference(&consumer)
            .chain(consumer.difference(&table))
            .map(|(module, member)| format!("{module}.{member}"))
            .collect()
    }
    assert_eq!(
        mismatch(&[("core.fake", "only")], &[]),
        vec!["core.fake.only".to_string()]
    );
    assert_eq!(
        mismatch(&[], &[("core.consumer", "only")]),
        vec!["core.consumer.only".to_string()]
    );
}

#[test]
fn core_call_truth_names_the_foundation_home() {
    let row = jet_foundation::Registry::row("CoreCalls")
        .expect("CoreCalls truth is registered");
    assert_eq!(
        row.home,
        Some("crates/jet-foundation/src/Syntax/core_calls.rs")
    );
    assert_eq!(
        row.guard.map(|guard| guard.test),
        Some("core_projection_is_complete_both_directions")
    );
}
