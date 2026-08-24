//! D-TYPE2-MEASURE1=A: symbolic fixed-list lengths stay symbolic in a generic
//! module and become concrete only after value-parameter specialization.

mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

use jet::AST::{Item, Measure, Type};

const GENERIC_JOIN_SOURCE: &str = r#"
fn join<T>(a: [T#N], b: [T#M]) [T#(N + M)] {
    return a.concat(b)
}

fn run() {}
"#;

const FIXED_JOIN_SOURCE: &str = r#"
fn join(left: [Int#2], right: [Int#3]) [Int#5] {
    return left.concat(right)
}

fn run() {
    left :: [Int#2]{1, 2}
    right :: [Int#3]{3, 4, 5}
    joined :: join(left, right)
    print(joined.len())
    print(joined[4])
}
"#;

#[test]
fn parser_preserves_the_generic_additive_measure_rule() {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(GENERIC_JOIN_SOURCE);
    assert!(lexer_diagnostics.is_empty(), "lex: {lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("generic join must parse");
    let Item::Func(function) = &program.items[0] else {
        panic!("expected generic join function");
    };
    let Type::FixedList { len: left_len, .. } = &function.params[0].ty else {
        panic!("expected fixed left list");
    };
    assert!(matches!(
        left_len,
        Measure::Symbol { kind, name } if kind == "length" && name == "N"
    ));
    let Type::FixedList {
        len: return_len, ..
    } = function.return_type.as_ref().unwrap()
    else {
        panic!("expected fixed return list");
    };
    assert!(matches!(
        return_len,
        Measure::Combined { kind, rule: jet::AST::MeasureRule::Add, .. }
            if kind == "length"
    ));
}

#[test]
fn sema_type_checks_generic_join_with_symbolic_additive_length() {
    let scratch = common::Scratch::new("measure-generic-join-sema");
    let entry = scratch.join("app.jet");
    fs::write(&entry, GENERIC_JOIN_SOURCE).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "generic join rejected: {errors:?}");
}

#[test]
fn tir_carries_the_specialized_additive_measure() {
    let scratch = common::Scratch::new("measure-fixed-join-tir");
    let entry = scratch.join("app.jet");
    fs::write(&entry, FIXED_JOIN_SOURCE).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "fixed join rejected: {errors:?}");

    let program =
        jet::Codegen::TIR::lower_jit_program(&bundle).expect("fixed join must lower through TIR");
    let function = program
        .funcs
        .iter()
        .find(|function| function.name == "join")
        .expect("TIR must retain join");
    let Some(Type::FixedList { len, .. }) = function.ret.as_ref() else {
        panic!("TIR join return must retain fixed-list measure");
    };
    assert_eq!(len.literal_value(), Some(5));
}

#[test]
fn aot_jit_and_interpreter_agree_on_additive_fixed_list_length() {
    tir_support::assert_tiers_agree("measure_fixed_join", FIXED_JOIN_SOURCE, "5\n5\n");
}

#[test]
fn comptime_resolves_a_declared_measure_binding() {
    let source = r#"
@capacity :: 3

struct Buffer {
    values: [Int#capacity]
}

fn run() {
    values :: [Int#3]{1, 2, 3}
    print(values.len())
}
"#;
    let compiled = jet::compile(source).expect("comptime measure must compile");
    assert!(compiled.rust.contains("[i64; 3]"));
}

#[test]
fn repl_accepts_fixed_list_measure_use() {
    let transcript = jet::REPL::run_transcript(&["print([Int#3]{1, 2, 3}.len())"], None);
    assert!(
        transcript.contains("3"),
        "REPL rejected fixed-list measure use:\n{transcript}"
    );
}

#[test]
fn web_target_accepts_fixed_list_measure_use() {
    let scratch = common::Scratch::new("measure-web");
    let entry = scratch.join("app.jet");
    fs::write(&entry, FIXED_JOIN_SOURCE).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", entry.to_str().unwrap(), "--target", "web"])
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn jet");
    assert!(
        out.status.success(),
        "web check failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn symbolic_fixed_length_resolves_at_module_specialization() {
    let source = r#"
module buffer<T>(capacity: Int) {
    pub struct Data {
        items: [T#capacity]
    }
}

module three :: buffer<Int>(3)

fn run() {}
"#;

    let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "lex: {lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("generic module must parse");
    let Item::GenericModule(template) = &program.items[0] else {
        panic!("expected generic module template");
    };
    let Item::Struct(data) = &template.body[0] else {
        panic!("expected fixed-list struct");
    };
    let Type::FixedList { len, .. } = &data.fields[0].ty else {
        panic!("expected fixed-list field");
    };
    assert!(matches!(
        len,
        Measure::Symbol { kind, name } if kind == "length" && name == "capacity"
    ));
    assert_eq!(len.symbol_name(), Some("capacity"));
    assert_ne!(len.literal_value(), Some(0));

    let compiled = jet::compile(source).expect("specialized fixed length must compile");
    assert!(compiled.rust.contains("[i64; 3]"));
}

#[test]
fn computed_fixed_length_is_rejected_at_the_measure_boundary() {
    let source = "struct Bad { values: [Int#missing] }\nfn run() {}\n";
    let diagnostics = jet::compile(source).expect_err("unknown measure must be rejected");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "E0963"
            && diagnostic.what.contains("fixed-size list length")
            && diagnostic
                .why
                .contains("declared constant or module value parameter")
    }));
}
