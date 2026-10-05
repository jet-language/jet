mod common;
mod tir_support;

use jet::Codegen::MIREval::{evaluate_mir_function_with_config, MirEvalConfig, MirExecutionStatus, MirValue};
use jet::Diagnostics::{Diagnostic, Severity};
use jet::Lexer::{self, RawTokenFact, StrTokPart, TerminatorDriver, TerminatorEvent, TokKind, Token};
use jet_foundation::MIR::{MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget, MirFunctionId, MirProgram};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The terminator pass unit: the jet_lexer package and its dependency closure.
fn pass_source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    SOURCE.get_or_init(|| common::compiler_package_parity_source("selfhost terminator pass source", "Compiler/JetLexer"))
}

const BOOT_ENTRY: &str = "\nfn bootstrap_probe(source: [U8], facts: [Int]) {\n    print(terminator_events(source, facts).len())\n}\nfn run() { bootstrap_probe([U8]{}, [Int]{}) }\n";

struct Pass {
    mir: MirProgram,
    function: MirFunctionId,
}

fn lower_bundle(bundle: &mut jet::AST::ProgramBundle, context: &str) -> MirProgram {
    let diagnostics = jet::Sema::check_bundle(bundle, jet::Sema::CompileMode::Run);
    let errors = diagnostics.iter().filter(|row| row.severity == Severity::Error).collect::<Vec<_>>();
    assert!(errors.is_empty(), "{context}: checked terminator fixture: {errors:#?}");
    let request = MirArtifactRequest::new(
        MirArtifactTarget::Cranelift, MirArtifactKind::NativeExecutable, MirArtifactBuildMode::Dev,
    );
    jet::Codegen::TIR::lower_checked_mir_program_for(bundle, request)
        .unwrap_or_else(|error| panic!("{context}: canonical optimized terminator MIR: {error}"))
        .0
}

fn bootstrap(context: &str, source: &str) -> Arc<Pass> {
    assert!(Lexer::terminator_driver().is_none(), "bootstrap must precede candidate installation");
    jet::run_compiler_work(|| {
        let scratch = common::Scratch::new("selfhost_terminator_bootstrap");
        tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
        let path = scratch.path.join("terminators.jet");
        fs::write(&path, format!("{source}{BOOT_ENTRY}")).unwrap();
        let mut bundle = jet::Loader::load_entry(path.to_str().unwrap())
            .expect("Rust reference bootstraps the Jet pass");
        let mir = lower_bundle(&mut bundle, context);
        let function = mir.functions.iter().find(|row| row.name == "terminator_events")
            .expect("checked Jet pass entry").id;
        Arc::new(Pass { mir, function })
    })
}

fn pass() -> Arc<Pass> {
    static PASS: OnceLock<Arc<Pass>> = OnceLock::new();
    PASS.get_or_init(|| bootstrap("canonical terminator pass", pass_source())).clone()
}

fn input_values(source: &[u8], facts: &[RawTokenFact]) -> Result<[MirValue; 2], String> {
    let source = source.iter().copied().map(|byte| MirValue::Int(i64::from(byte))).collect();
    let mut rows = Vec::with_capacity(facts.len() * 4);
    for fact in facts {
        rows.push(MirValue::Int(i64::from(fact.kind)));
        rows.push(MirValue::Int(i64::try_from(fact.span.start).map_err(|error| error.to_string())?));
        rows.push(MirValue::Int(i64::try_from(fact.span.end).map_err(|error| error.to_string())?));
        rows.push(MirValue::Int(i64::from(fact.uppercase)));
    }
    Ok([MirValue::List(source), MirValue::List(rows)])
}

fn decode_events(value: MirValue) -> Result<Vec<TerminatorEvent>, String> {
    let value = match value {
        MirValue::Present(value) => *value,
        value => value,
    };
    let MirValue::List(values) = value else {
        return Err(format!("terminator entry returned a non-list carrier: {value:?}"));
    };
    if values.len() % 3 != 0 {
        return Err("terminator event carrier has an incomplete row".to_string());
    }
    let mut events = Vec::with_capacity(values.len() / 3);
    for row in values.chunks_exact(3) {
        let [MirValue::Int(kind), MirValue::Int(before), MirValue::Int(at)] = row else {
            return Err("terminator event fields must be integers".to_string());
        };
        let before = usize::try_from(*before).map_err(|error| error.to_string())?;
        let at = usize::try_from(*at).map_err(|error| error.to_string())?;
        events.push(match kind {
            1 => TerminatorEvent::InsertSemi { before, at },
            2 => TerminatorEvent::SplitHeader { before, at },
            _ => return Err(format!("unknown terminator event tag {kind}")),
        });
    }
    Ok(events)
}

impl Pass {
    fn evaluate(&self, source: &[u8], facts: &[RawTokenFact], fuel: u64) -> Result<Vec<TerminatorEvent>, String> {
        let args = input_values(source, facts)?;
        let config = MirEvalConfig { fuel: Some(fuel), ..MirEvalConfig::default() };
        let result = evaluate_mir_function_with_config(&self.mir, self.function, &args, &config)
            .map_err(|error| format!("{:?}", error.diagnostic))?;
        if result.status != MirExecutionStatus::Completed || result.exit_code != 0
            || !result.stdout.is_empty() || !result.stderr.is_empty()
        {
            return Err("terminator evaluator did not complete silently and successfully".to_string());
        }
        decode_events(result.value)
    }

    fn driver(self: &Arc<Self>) -> TerminatorDriver {
        let pass = self.clone();
        Arc::new(move |source, facts| pass.evaluate(source, facts, 50_000_000))
    }
}

struct Witness {
    name: &'static str,
    source: &'static str,
    parses: bool,
}

// Bounded differential manifest. Token-fragment/invalid rows still compare the
// complete downstream parser result; only the named complete programs require Ok.
const WITNESSES: &[Witness] = &[
    Witness { name: "empty", source: "", parses: true },
    Witness { name: "eof-expression", source: "x", parses: false },
    Witness { name: "no-final-newline", source: "fn run() {\n    value :: 1\n}", parses: true },
    Witness { name: "leading-minus", source: "x\n-y", parses: false },
    Witness { name: "line-comment-last-code", source: "x // c\ny", parses: false },
    Witness { name: "nested-block-comment", source: "x /* outer\n/* inner */ end */\ny", parses: false },
    Witness { name: "comment-at-eof", source: "x // final", parses: false },
    Witness { name: "unary-not", source: "x\n!y", parses: false },
    Witness { name: "compact-failure", source: "x\n! problem -> recover", parses: false },
    Witness { name: "scope-member", source: "x\n.member {}", parses: false },
    Witness { name: "scope-member-nested-args", source: "x\n.member(f(1), g(2)) {}", parses: false },
    Witness { name: "scope-member-comment-adjacency", source: "x\n. /* trivia */ member {}", parses: false },
    Witness { name: "fluent-chain", source: "x\n.member(1)\n?.field", parses: false },
    Witness { name: "dispatch-group", source: "x\n.Group.Leaf(payload) -> 1", parses: false },
    Witness { name: "dispatch-null", source: "x\n.null -> 1", parses: false },
    Witness { name: "dispatch-unicode", source: "x\n.Étage -> 1", parses: false },
    Witness { name: "dispatch-record", source: "x\n.{ outer: { inner } } -> 1", parses: false },
    Witness { name: "guard-same-line", source: "x\n.Key && ready -> 1", parses: false },
    Witness { name: "guard-later-arrow", source: "x\n.Key && ready\n-> 1", parses: false },
    Witness { name: "guard-nested-delimiters", source: "x\n.Key(v) && f([v], {x: v}) -> 1", parses: false },
    Witness { name: "unclosed-lookahead", source: "x\n.Member((1\n", parses: false },
    Witness { name: "split-arrow", source: "fn run()\n-> Int { return 1 }", parses: false },
    Witness { name: "split-block-crlf", source: "fn run()\r\n{\r\n}\r\n", parses: false },
    Witness { name: "split-retired-arrow", source: "g()\n=> 1", parses: false },
    Witness { name: "fence-close-continuation", source: "fn run() {\n    <: a,\n        b\n    :> :: 1\n    print(<: a, b :>)\n}\n", parses: true },
    Witness { name: "split-effect-spellings", source: "f()\n= x\ng()\n-- x", parses: false },
    Witness { name: "multi-line-delimiters", source: "fn run() {\n    xs :: [1,\n        2\n    ]\n    print(\n        xs\n    )\n}\n", parses: true },
    Witness { name: "utf8-crlf-payloads", source: "fn run() {\r\n    café :: \"héllo\"\r\n    print(café)\r\n}\r\n", parses: true },
    Witness { name: "all-literal-payloads", source: "x :: 0x0F\ny :: 1_024\nz :: 1.25e-3\nu :: 12.50usd\nc :: 'é'\ns :: \"a{1.25 + 2.0}b\"\nr :: `raw`\n", parses: false },
    Witness { name: "all-continuations", source: "x\n+ y\n- y\n* y\n/ y\n/% y\n% y\n%% y\n== y\n!= y\n< y\n> y\n<= y\n>= y\n<=> y\n& y\n| y\n^ y\n~| y\n<< y\n>> y\n?? y\n&& y\n|| y", parses: false },
    Witness { name: "place-mark-statements", source: "fn run() {\n    buf := [1]\n    total := 3\n    &buf.push(2)\n    ^buf.len()\n    &self.items.push(1)\n    mask := total\n        & 1\n    power := total\n        ^ 2\n    &(total)\n}\n", parses: false },
    Witness { name: "end-token-families", source: "true\nfalse\nself\nnull\nbreak\nreturn\na?\na++\na--\nT>\nT>>", parses: false },
    Witness { name: "invalid-raw-source", source: "fn run() {\n    value :: \"unfinished\n", parses: false },
    Witness { name: "keyword-and-assignment-transport", source: "fn pub priv if else in mutate move copy struct enum impl trait tag effect derive it const comptime loop yield use extern module\n: :: := , ; .. ..< ... @ ~ ~|= ~~ += -= *= /= /%= %= %%= &= |= ^= <<= >>= # $", parses: false },
];

fn float_bits(tokens: &[Token], output: &mut Vec<u64>) {
    for token in tokens {
        match &token.kind {
            TokKind::Float(value, _) => output.push(value.to_bits()),
            TokKind::UnitNumber { float: Some(value), .. } => output.push(value.to_bits()),
            TokKind::Str(parts) => {
                for part in parts {
                    if let StrTokPart::Interp(tokens) = part {
                        float_bits(tokens, output);
                    }
                }
            }
            _ => {}
        }
    }
}

fn assert_lex_equal(name: &str, actual: &(Vec<Token>, Vec<Diagnostic>), expected: &(Vec<Token>, Vec<Diagnostic>)) {
    assert_eq!(actual.0, expected.0, "{name}: full token payloads, trees and byte spans");
    let mut actual_bits = Vec::new();
    let mut expected_bits = Vec::new();
    float_bits(&actual.0, &mut actual_bits);
    float_bits(&expected.0, &mut expected_bits);
    assert_eq!(actual_bits, expected_bits, "{name}: floating-point payload bits");
    assert_eq!(format!("{:#?}", actual.1), format!("{:#?}", expected.1), "{name}: ordered diagnostics");
}

#[test]
fn jet_policy_matches_bounded_reference_and_downstream_parser() {
    let pass = pass();
    jet::run_compiler_work(|| {
        for witness in WITNESSES {
            let expected = Lexer::lex(witness.source);
            let actual = Lexer::with_terminator_driver(Some(pass.driver()), || Lexer::lex(witness.source));
            assert_lex_equal(witness.name, &actual, &expected);
            let reference_parse = jet::Parser::parse_with_source(&expected.0, witness.source);
            let candidate_parse = jet::Parser::parse_with_source(&actual.0, witness.source);
            assert_eq!(format!("{candidate_parse:#?}"), format!("{reference_parse:#?}"), "{}: downstream parser", witness.name);
            if matches!(witness.name, "split-arrow" | "split-block-crlf" | "split-retired-arrow" | "split-effect-spellings") {
                assert!(actual.1.iter().any(|diagnostic| diagnostic.code == "E0986"),
                    "{}: invalid split header must retain E0986", witness.name);
            }
            if witness.parses {
                assert!(actual.1.is_empty(), "{}: unexpected lexer diagnostic", witness.name);
                assert!(candidate_parse.is_ok(), "{}: complete program must parse", witness.name);
            }
        }
        for (name, source, lex) in [
            ("own-source", pass_source(), Lexer::lex as fn(&str) -> (Vec<Token>, Vec<Diagnostic>)),
            ("config-url", "url: https://example.test/a\nname: \"probe\"\n", Lexer::lex_config),
            ("generated-name", "fn __generated() {\n    return 1\n}\n", Lexer::lex_generated),
        ] {
            let expected = lex(source);
            let actual = Lexer::with_terminator_driver(Some(pass.driver()), || lex(source));
            assert_lex_equal(name, &actual, &expected);
            assert_eq!(format!("{:#?}", jet::Parser::parse_with_source(&actual.0, source)),
                format!("{:#?}", jet::Parser::parse_with_source(&expected.0, source)), "{name}: parser");
        }
    });
}

#[test]
fn scoped_candidate_reaches_overlay_loader_parser_and_checked_mir() {
    let pass = pass();
    let scratch = common::Scratch::new("selfhost_terminator_overlay");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("run.jet");
    let helper = scratch.path.join("helper.jet");
    let entry_source = "use helper as h\nfn observed() -> Int { return h.answer() }\nfn run() { print(observed()) }\n";
    let helper_source = "pub fn answer() -> Int {\n    return 42\n}\n";
    fs::write(&entry, "fn run() {}\n").unwrap();
    fs::write(&helper, helper_source).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let observed = seen.clone();
    let driver: TerminatorDriver = Arc::new(move |source, facts| {
        let events = pass.evaluate(source, facts, 50_000_000)?;
        observed.lock().unwrap().push(source.to_vec());
        Ok(events)
    });
    Lexer::with_terminator_driver(Some(driver), || jet::run_compiler_work(|| {
        let mut bundle = jet::Loader::load_entry_with_overlays(entry.to_str().unwrap(),
            &[(entry.as_path(), entry_source), (helper.as_path(), helper_source)], false)
            .expect("candidate lexer must feed the real overlay Loader");
        let mir = lower_bundle(&mut bundle, "overlay terminator fixture");
        let function = mir.functions.iter().find(|row| row.name == "observed").unwrap().id;
        let result = evaluate_mir_function_with_config(&mir, function, &[], &MirEvalConfig::default())
            .expect("checked imported value executes");
        assert_eq!(result.status, MirExecutionStatus::Completed);
        assert!(matches!(result.value, MirValue::Present(value) if matches!(*value, MirValue::Int(42))),
            "scoped compiler must preserve the imported function's result");
    }));
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|source| source == entry_source.as_bytes()), "entry overlay bypassed candidate");
    assert!(seen.iter().any(|source| source == helper_source.as_bytes()), "import overlay worker bypassed candidate");
    assert!(Lexer::terminator_driver().is_none());
}

fn compiler_must_fail(driver: TerminatorDriver, expected: &str) {
    let failure = catch_unwind(AssertUnwindSafe(|| {
        Lexer::with_terminator_driver(Some(driver), || {
            let _ = jet::compile("fn run() {}\n");
        });
    })).expect_err("candidate failure must not succeed through the Rust reference");
    let message = failure.downcast_ref::<String>().map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied()).expect("ICE message");
    assert!(message.contains("internal compiler error") && message.contains(expected), "{message}");
    assert!(Lexer::terminator_driver().is_none(), "failed candidate leaked scoped driver");
}

#[test]
fn callback_evaluator_and_event_abi_failures_are_not_reference_fallbacks() {
    let pass = pass();
    compiler_must_fail(Arc::new(|_, _| Err("deliberate callback failure".to_string())), "deliberate callback failure");
    compiler_must_fail(Arc::new(move |source, facts| pass.evaluate(source, facts, 0)), "exhausted its fuel");
    compiler_must_fail(Arc::new(|_, _| decode_events(MirValue::String("wrong ABI".to_string()))), "non-list carrier");
    compiler_must_fail(Arc::new(|_, _| Ok(vec![TerminatorEvent::InsertSemi { before: usize::MAX, at: 0 }])), "invalid raw-token index");
    compiler_must_fail(Arc::new(|_, _| Ok(vec![
        TerminatorEvent::InsertSemi { before: 0, at: 0 },
        TerminatorEvent::InsertSemi { before: 0, at: 0 },
    ])), "event order");
}

#[test]
fn scoped_driver_restores_outer_candidate_after_nested_unwind() {
    let pass = pass();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let driver: TerminatorDriver = Arc::new(move |source, facts| {
        let result = pass.evaluate(source, facts, 50_000_000)?;
        observed.fetch_add(1, Ordering::Relaxed);
        Ok(result)
    });
    jet::run_compiler_work(|| Lexer::with_terminator_driver(Some(driver), || {
        let expected = Lexer::lex("x");
        let failed = catch_unwind(AssertUnwindSafe(|| {
            Lexer::with_terminator_driver(Some(Arc::new(|_, _| Err("nested candidate".to_string()))), || Lexer::lex("x"));
        }));
        assert!(failed.is_err());
        assert_lex_equal("restored outer driver", &Lexer::lex("x"), &expected);
    }));
    assert_eq!(calls.load(Ordering::Relaxed), 2, "nested unwind lost the real evaluator");
    assert!(Lexer::terminator_driver().is_none());
}

#[test]
fn six_policy_mutations_are_killed_through_candidate_event_application() {
    let mutations = [
        ("omit-eof", "if ends_statement(previous) && previous != K_RBRACE {", "if false {", "x"),
        ("minus-boundary", "|| kind == K_MINUS || kind == K_STAR", "|| kind == K_STAR", "x\n-y"),
        ("comment-last-code", "if kind == K_LINE_COMMENT || kind == K_BLOCK_COMMENT -> index += 1 else {", "if kind == K_LINE_COMMENT || kind == K_BLOCK_COMMENT {\n            last_code = index\n            index += 1\n        } else {", "x // c\ny"),
        ("guard-newline", "return !newline_between(source, guard_start, facts[cursor * 4 + 1])", "return true", "x\n.Key && ready\n-> 1"),
        ("unary-not-suppression", "return kind_at(facts, index) == K_BANG && kind_at(facts, index + 1) == K_IDENT && kind_at(facts, index + 2) == K_UNIFIED_ARROW", "return kind_at(facts, index) == K_BANG", "x\n!y"),
        ("shift-synthetic-span", "events.push(end)", "events.push(end + 1)", "x\ny"),
    ];
    for (name, before, after, source) in mutations {
        assert_eq!(pass_source().matches(before).count(), 1, "mutation {name} must target one decision");
        let mutant = bootstrap(name, &pass_source().replacen(before, after, 1));
        jet::run_compiler_work(|| {
            let reference = Lexer::lex(source);
            let changed = Lexer::with_terminator_driver(Some(mutant.driver()), || Lexer::lex(source));
            assert_ne!(changed.0, reference.0, "{name} survived: candidate driver/application was bypassed");
        });
    }
}

fn golden_program() -> (String, String) {
    let cases: &[(&str, &[i64])] = &[
        ("x", &[1, 1, 1]),
        ("x\n-y", &[1, 3, 4]),
        ("x // c\ny", &[1, 2, 1, 1, 3, 8]),
        ("x\n!y", &[1, 1, 1, 1, 3, 4]),
        ("f()\n{}", &[2, 3, 4]),
        ("x\n.Key && ready\n-> 1", &[1, 5, 15, 1, 7, 20]),
        ("x\n.Key && ready -> 1", &[1, 1, 1, 1, 7, 20]),
        ("x\n.member(1) {}", &[1, 1, 1]),
    ];
    // The printing wrapper's parameters have no compile-time values. Keep the
    // pure pass call inside it rather than folding literal facts in `run`.
    let pass_source = pass_source();
    let mut source = format!("{pass_source}\nfn emit_events(source: [U8], facts: [Int]) {{\n    events :: terminator_events_scanned(source, false)\n    loop value in events {{ print(value) }}\n}}\nfn run() {{\n");
    let mut expected = String::new();
    for (input, events) in cases {
        let (tokens, diagnostics) = Lexer::lex_raw(input);
        assert!(diagnostics.is_empty(), "golden raw input: {diagnostics:?}");
        let bytes = input.bytes().map(|byte| byte.to_string()).collect::<Vec<_>>().join(", ");
        let rows = tokens.iter().map(Lexer::raw_token_fact).flat_map(|fact| [
            i64::from(fact.kind), fact.span.start as i64, fact.span.end as i64, i64::from(fact.uppercase),
        ]).map(|value| value.to_string()).collect::<Vec<_>>().join(", ");
        source.push_str(&format!("    emit_events([U8]{{{bytes}}}, [Int]{{{rows}}})\n"));
        for event in *events {
            expected.push_str(&format!("{event}\n"));
        }
    }
    source.push_str("}\n");
    (source, expected)
}

#[test]
fn same_jet_pass_golden_executes_aot_default_interpreter_and_awaited_web() {
    assert!(tir_support::have_rustc(), "AOT proof is required, never skipped");
    let (source, expected) = golden_program();
    tir_support::assert_tiers_agree("selfhost_terminators_golden", &source, &expected);
    let (native_code, native_stdout, trace) =
        tir_support::jit_run_traced("selfhost_terminators_golden", &source);
    assert_eq!(native_code, 0, "traced default run failed:\n{trace}");
    assert_eq!(native_stdout, expected, "traced native pass changed the golden:\n{trace}");
    assert!(trace.lines().any(|line| {
        let mut columns = line.split_whitespace();
        columns.next().is_some_and(|name| {
            name == "terminator_events" || name.ends_with("::terminator_events")
        }) && columns.next() == Some("tier1") && columns.next() == Some("native")
    }), "the executed golden must include the Jet pass in tier1 native:\n{trace}");
    assert!(!trace.contains("tier0 interp") && !trace.contains("deopt:"),
        "the default-native witness must not use interpreter/deopt execution:\n{trace}");
    tir_support::assert_awaited_web_tier("selfhost_terminators_web", &source, &expected);
}

fn lexer_payload_golden_program() -> (String, String) {
    let input = "\t\x0b\x0c\r\n  1_2.5e-1 1.25e+1 3.5E-1 3.25E+1 \"prefix {1.25e-2} suffix\"";
    let (tokens, diagnostics) = Lexer::lex_raw(input);
    assert!(diagnostics.is_empty(), "payload golden raw input: {diagnostics:?}");
    assert_eq!(tokens.len(), 6, "payload golden token count changed");
    assert!(matches!(&tokens[0].kind, TokKind::Float(..)));
    assert!(matches!(&tokens[1].kind, TokKind::Float(..)));
    assert!(matches!(&tokens[2].kind, TokKind::Float(..)));
    assert!(matches!(&tokens[3].kind, TokKind::Float(..)));
    let nested = match &tokens[4].kind {
        TokKind::Str(parts) => match parts.get(1) {
            Some(StrTokPart::Interp(tokens)) => tokens.first().expect("interpolation token"),
            _ => panic!("payload golden string parts lost its interpolation"),
        },
        other => panic!("payload golden string token changed: {other:?}"),
    };
    assert!(matches!(&nested.kind, TokKind::Float(..)));

    let bytes = input.bytes().map(|byte| byte.to_string()).collect::<Vec<_>>().join(", ");
    let pass_source = pass_source();
    let source = format!(
        "{pass_source}\nfn run() {{\n    result :: lex_payload([U8]{{{bytes}}}, false)\n    index := 0\n    loop index < result.tokens.len() {{\n        print(result.tokens[index].span.start)\n        print(result.tokens[index].span.end)\n        print(result.tokens[index].kind)\n        print(result.tokens[index].payload.text)\n        print(result.tokens[index].payload.integer ?? -1)\n        print(result.tokens[index].payload.decimal ?? Float{{0}})\n        print(result.tokens[index].payload.suffix)\n        index += 1\n    }}\n    print(result.diagnostics.len())\n    nested :: result.tokens[4].payload.string_parts[1].tokens[0]\n    print(nested.span.start)\n    print(nested.span.end)\n    print(nested.kind)\n    print(nested.payload.text)\n    print(nested.payload.decimal ?? Float{{0}})\n}}\n"
    );
    let mut expected = String::new();
    for token in &tokens {
        let raw = &input[token.span.start..token.span.end];
        expected.push_str(&format!("{}\n{}\n{}\n", token.span.start, token.span.end, Lexer::raw_token_fact(token).kind));
        expected.push_str(&format!("{}\n", raw));
        match &token.kind {
            TokKind::Float(value, _) => expected.push_str(&format!("-1\n{value}\n\n")),
            TokKind::Str(_) | TokKind::Eof => expected.push_str("-1\n0\n\n"),
            other => panic!("payload golden token kind changed: {other:?}"),
        }
    }
    expected.push_str("0\n");
    let TokKind::Float(value, _) = &nested.kind else {
        panic!("payload golden interpolation is not a float");
    };
    let raw = &input[nested.span.start..nested.span.end];
    let fact = Lexer::raw_token_fact(nested);
    expected.push_str(&format!(
        "{}\n{}\n{}\n{}\n{}\n",
        nested.span.start, nested.span.end, fact.kind, raw, value
    ));
    (source, expected)
}

#[test]
fn jet_lexer_payload_golden_covers_whitespace_exponents_and_shifted_spans() {
    assert!(tir_support::have_rustc(), "AOT proof is required, never skipped");
    let (source, expected) = lexer_payload_golden_program();
    tir_support::assert_tiers_agree("selfhost_lexer_payload_golden", &source, &expected);
    tir_support::assert_awaited_web_tier("selfhost_lexer_payload_web", &source, &expected);
}

#[test]
fn jet_and_rust_triple_delimiter_lines_have_identical_text_and_diagnostics() {
    let cases = [
        "\"\"\"Hello\n    kept\n    \"\"\"",
        "\"\"\"\n    kept\n    end\"\"\"",
        "\"\"\"Hello\n    kept\n    end\"\"\"",
        "\"\"\"abc\"\"\"",
        "\"\"\"{name}\nkept\n\"\"\"",
        "\"\"\" \té😀 rest\nkept\n\"\"\"",
        "\"\"\"   \nhello\n   \"\"\" }",
        "\"\"\"\t\t\nhello\n\t\t\"\"\"",
        "\"\"\" \t\r\nhello\r\n \t\"\"\"",
        "\"\"\"\rstray\nkept\n\"\"\"",
        "SQL{\"\"\"query\nkept\n\"\"\"}",
        "SQL{\"\"\"\nhello\n\"\"\"}",
        "\"\"\"\nhello\n\"\"\"",
        "#FFI(c) fn f() { \"\"\"text {value} \\raw\"\"\" }",
        "#FFI(cpp) fn f() { \"\"\"text {value} \\raw\"\"\" }",
        "#FFI(asm) fn f() { \"\"\"text {value} \\raw\"\"\" }",
    ];
    let pass_source = pass_source();
    let mut source = format!("{pass_source}\nfn run() {{\n");
    let mut expected = String::new();
    for (index, input) in cases.iter().enumerate() {
        let bytes = input.bytes().map(|byte| byte.to_string()).collect::<Vec<_>>().join(", ");
        source.push_str(&format!(
            "    result{index} :: lex_payload([U8]{{{bytes}}}, false)\n    loop token in result{index}.tokens {{\n        print(token.span.start)\n        print(token.span.end)\n        print(token.kind)\n        print(token.payload.text)\n        print(token.payload.decoded)\n    }}\n    print(result{index}.diagnostics.len())\n    loop diagnostic in result{index}.diagnostics {{\n        print(diagnostic.code)\n        span :: diagnostic.span ?? return\n        print(span.start)\n        print(span.end)\n        print(diagnostic.what)\n        print(diagnostic.why)\n        print(diagnostic.fix)\n    }}\n"
        ));
        let (tokens, diagnostics) = Lexer::lex_raw(input);
        for token in tokens {
            let decoded = match &token.kind {
                TokKind::Str(parts) => parts.iter().map(|part| match part {
                    StrTokPart::Lit(text) => text.as_str(),
                    _ => panic!("parity input should have no retained interpolation"),
                }).collect::<String>(),
                _ => String::new(),
            };
            expected.push_str(&format!(
                "{}\n{}\n{}\n{}\n{}\n",
                token.span.start, token.span.end, Lexer::raw_token_fact(&token).kind,
                &input[token.span.start..token.span.end], decoded,
            ));
        }
        expected.push_str(&format!("{}\n", diagnostics.len()));
        for diagnostic in diagnostics {
            let span = diagnostic.span.expect("delimiter diagnostic span");
            expected.push_str(&format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n",
                diagnostic.code, span.start, span.end, diagnostic.what, diagnostic.why, diagnostic.fix,
            ));
        }
    }
    source.push_str("}\n");
    tir_support::assert_tiers_agree("triple_delimiter_lexer_parity", &source, &expected);
    tir_support::assert_awaited_web_tier("triple_delimiter_lexer_parity_web", &source, &expected);
}

#[test]
fn jet_lexer_exports_parser_ready_tokens_spans_payloads_and_diagnostics() {
    let pass_source = pass_source();
    let source = format!(
        "{pass_source}\nfn run() {{\n    result :: lex_payload(\"Name 500ms 1.25 true \\\"ok\\\" 'x'\", false)\n    print(result.tokens[0].kind)\n    print(result.tokens[0].payload.text)\n    print(result.tokens[1].kind)\n    print(result.tokens[1].payload.integer ?? -1)\n    print(result.tokens[1].payload.suffix)\n    print(result.tokens[2].kind)\n    print(result.tokens[2].payload.decimal ?? Float{{0}})\n    print(result.tokens[3].kind)\n    print(result.tokens[4].kind)\n    print(result.tokens[4].payload.decoded)\n    print(result.tokens[5].kind)\n    print(result.tokens[5].payload.decoded)\n    print(result.tokens[6].kind)\n    print(result.diagnostics.len())\n    interpolated :: lex_payload(\"\\\"a{{value + 1}}b\\\"\".bytes(), false)\n    print(interpolated.tokens[0].payload.string_parts.len())\n    print(interpolated.tokens[0].payload.string_parts[0].kind)\n    print(interpolated.tokens[0].payload.string_parts[1].kind)\n    print(interpolated.tokens[0].payload.string_parts[1].tokens.len())\n    print(interpolated.tokens[0].payload.string_parts[1].tokens[0].span.start)\n    print(interpolated.tokens[0].payload.string_parts[1].tokens[3].kind)\n    print(interpolated.tokens[0].payload.string_parts[1].tokens[3].span.start)\n    terminated :: lex(\"fn f() {{{{\\n    value\\n}}}}\".bytes())\n    print(terminated.tokens[6].kind)\n    print(terminated.tokens[6].span.start)\n    print(terminated.tokens[6].span.end)\n    bad :: lex_payload(\"§§\".bytes(), false)\n    print(bad.diagnostics.len())\n    print(bad.diagnostics[0].span.start)\n    print(bad.diagnostics[1].span.start)\n    prefix :: lex_payload(\"r\\\"bad\\\"\".bytes(), false)\n    print(prefix.diagnostics[0].code)\n    print(prefix.diagnostics[0].span.end)\n}}\n",
    );
    let (code, stdout, stderr) = tir_support::jit_run("selfhost_parser_ready_lexer", &source);
    assert_eq!(code, 0, "lexer interface run failed:\n{stderr}");
    assert_eq!(
        stdout,
        "1\nName\n6\n500\nms\n5\n1.25\n8\n3\nok\n7\nx\n49\n0\n3\n1\n2\n4\n3\n49\n12\n86\n18\n18\n2\n0\n2\nE0003\n1\n"
    );
}

#[test]
fn jet_lexer_scans_inline_foreign_bodies_and_interpolations() {
    let pass_source = pass_source();
    let source = format!(
        r####"{pass_source}
fn run() {{
    foreign :: lex_payload(`#FFI(C) fn f() {{ """int f() {{ return "x"; }}""" }}`.bytes(), false)
    print(foreign.tokens[10].payload.decoded)
    print(foreign.tokens[10].payload.string_parts[0].kind)
    print(foreign.diagnostics.len())
    missing :: lex_payload(`#FFI(C) fn bad() {{ """int f() {{`.bytes(), false)
    print(missing.diagnostics.len())
    print(missing.diagnostics[0].code)
    diagnostic_span :: missing.diagnostics[0].span ?? return
    print(diagnostic_span.start)
    commented :: lex_payload(`"a{{ /* }} */ value + 1}}b"`.bytes(), false)
    print(commented.tokens[0].payload.string_parts[1].tokens[1].payload.text)
    print(commented.tokens[0].payload.string_parts[1].tokens[1].span.start)
}}
"####,
    );
    let (code, stdout, stderr) = tir_support::jit_run("selfhost_inline_foreign_lexer", &source);
    assert_eq!(code, 0, "inline foreign lexer run failed:\n{stderr}");
    assert_eq!(stdout, "int f() { return \"x\"; }\n1\n0\n1\nE0002\n19\nvalue\n12\n");
}

#[test]
fn jet_lexer_keeps_unicode_alphanumeric_unit_suffixes() {
    let pass_source = pass_source();
    let source = format!(
        r####"{pass_source}
fn run() {{
    result :: lex_payload(`12msμ 3usd٢`.bytes(), false)
    print(result.tokens[0].kind)
    print(result.tokens[0].payload.suffix)
    print(result.tokens[0].span.end)
    print(result.tokens[1].kind)
    print(result.tokens[1].payload.suffix)
    print(result.tokens[1].span.end)
    print(result.tokens[2].kind)
}}
"####,
    );
    let (code, stdout, stderr) = tir_support::jit_run("selfhost_unicode_unit_suffixes", &source);
    assert_eq!(code, 0, "unicode unit suffix lexer run failed:\n{stderr}");
    assert_eq!(stdout, "6\nmsμ\n6\n6\nusd٢\n13\n49\n");
}

#[test]
fn jet_lexer_uses_zero_for_integer_payload_overflow() {
    let pass_source = pass_source();
    let source = format!(
        r####"{pass_source}
fn run() {{
    values :: lex_payload(`9223372036854775807 9223372036854775808 0x7fffffffffffffff 0x8000000000000000`.bytes(), false)
    print(values.tokens[0].payload.integer ?? -1)
    print(values.tokens[1].payload.integer ?? -1)
    print(values.tokens[2].payload.integer ?? -1)
    print(values.tokens[3].payload.integer ?? -1)
    print(values.diagnostics.len())
    print(values.tokens[4].kind)
}}
"####,
    );
    let (code, stdout, stderr) = tir_support::jit_run("selfhost_integer_payload_overflow", &source);
    assert_eq!(code, 0, "integer overflow lexer run failed:\n{stderr}");
    assert_eq!(stdout, "9223372036854775807\n0\n9223372036854775807\n0\n0\n49\n");
}
