mod common;
mod tir_support;

use jet::Codegen::MIREval::{evaluate_mir_function_with_config, MirEvalConfig, MirExecutionStatus, MirValue};
use jet::Diagnostics::{Diagnostic, Severity};
use jet::AST::{AccessConvention, CallArg, Expr, Item, Marker, MarkerCallArg, Program, Stmt};
use jet_foundation::MIR::{MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget, MirFunctionId, MirProgram};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};

const SOURCE_MANIFEST: &str = include_str!("../Compiler/Bootstrap/sources.list");
const PROBE_SOURCE: &str = r####"
fn selfhost_parser_text(facts: &[Int], text: String) {
    bytes := text.bytes()
    facts.push(bytes.len())
    index := 0
    loop index < bytes.len() {
        facts.push(Int.from_u8(bytes[index]))
        index += 1
    }
}

fn selfhost_parser_label(facts: &[Int], label: ?CallLabel) {
    if label == {
        .Val(value) -> {
            facts.push(1)
            selfhost_parser_text(&facts, value.name)
            facts.push(value.span.start)
            facts.push(value.span.end)
        }
        .None -> facts.push(0)
    }
}

fn selfhost_parser_expr(facts: &[Int], expression: Expr) {
    if expression == {
        .Ident(name, span) -> {
            facts.push(1)
            selfhost_parser_text(&facts, name)
            facts.push(span.start)
            facts.push(span.end)
        }
        .Spread(value, span) -> {
            facts.push(2)
            facts.push(span.start)
            facts.push(span.end)
            selfhost_parser_expr(&facts, value)
        }
        .Field(base, member, span) -> {
            facts.push(3)
            selfhost_parser_text(&facts, member)
            facts.push(span.start)
            facts.push(span.end)
            selfhost_parser_expr(&facts, base)
        }
        else -> facts.push(0)
    }
}

fn selfhost_parser_call_arg(facts: &[Int], argument: CallArg) {
    convention := 0
    if argument.convention == AccessConvention.Write -> convention = 1
    if argument.convention == AccessConvention.Move -> convention = 2
    facts.push(convention)
    facts.push(argument.span.start)
    facts.push(argument.span.end)
    spread := 0
    if argument.spread -> spread = 1
    facts.push(spread)
    selfhost_parser_label(&facts, argument.label)
    selfhost_parser_expr(&facts, argument.expr)
}

fn selfhost_parser_marker(facts: &[Int], marker: Marker) {
    facts.push(marker.args.len())
    index := 0
    loop index < marker.args.len() {
        label := None
        if index < marker.arg_labels.len() -> label = marker.arg_labels[index]
        selfhost_parser_label(&facts, label)
        if marker.args[index] == {
            .Expr(value) -> selfhost_parser_expr(&facts, value)
            .EffectRow(_, span) -> {
                facts.push(4)
                facts.push(span.start)
                facts.push(span.end)
            }
        }
        index += 1
    }
}

fn selfhost_parser_diagnostic(facts: &[Int], diagnostic: Diagnostic) {
    selfhost_parser_text(&facts, diagnostic.code)
    facts.push(diagnostic.severity)
    selfhost_parser_text(&facts, diagnostic.what)
    selfhost_parser_text(&facts, diagnostic.why)
    selfhost_parser_text(&facts, diagnostic.fix)
    if diagnostic.span == {
        .Val(span) -> {
            facts.push(1)
            facts.push(span.start)
            facts.push(span.end)
        }
        .None -> facts.push(0)
    }
    if diagnostic.edit == {
        .Val(edit) -> {
            facts.push(1)
            facts.push(edit.span.start)
            facts.push(edit.span.end)
            selfhost_parser_text(&facts, edit.new_text)
        }
        .None -> facts.push(0)
    }
}

pub fn selfhost_parser_probe(source: [U8]) -> [Int] {
    parsed := parse_source(source)
    facts := [Int]{}
    facts.push(parsed.diagnostics.len())
    index := 0
    loop index < parsed.diagnostics.len() {
        selfhost_parser_diagnostic(&facts, parsed.diagnostics[index])
        index += 1
    }

    show_ast := true
    index = 0
    loop index < parsed.diagnostics.len() {
        if parsed.diagnostics[index].code != "E0029" -> show_ast = false
        index += 1
    }
    if !show_ast {
        facts.push(0)
        facts.push(0)
        return facts
    }

    if parsed.program.script_body.len() > 0 {
        statement := parsed.program.script_body[0]
        if statement == {
            .Expr(expression) -> {
                if expression == {
                    .Call(call) -> {
                        facts.push(1)
                        facts.push(call.args.len())
                        index = 0
                        loop index < call.args.len() {
                            selfhost_parser_call_arg(&facts, call.args[index])
                            index += 1
                        }
                        return facts
                    }
                    else -> {}
                }
            }
            .Switched(marker, _, _) -> {
                facts.push(3)
                selfhost_parser_marker(&facts, marker)
                return facts
            }
            else -> {}
        }
    }

    if parsed.program.items.len() > 0 {
        item := parsed.program.items[0]
        if item == {
            .Func(function) -> {
                if function.markers.len() > 0 {
                    facts.push(2)
                    selfhost_parser_marker(&facts, function.markers[0])
                    return facts
                }
            }
            else -> {}
        }
    }
    facts.push(0)
    facts.push(0)
    facts
}
"####;

const CASES: &[(&str, &str)] = &[
    ("unmarked-read", "f(x)"),
    ("write-marker", "f(&x)"),
    ("move-marker", "f(^x)"),
    ("keyword-tag-label", "f(tag: &x)"),
    ("ordinary-name-label", "f(value: &x)"),
    ("spread-before-label", "f(...tag: x)"),
    ("spread-after-label-stays-in-expression", "f(tag: ...x)"),
    ("duplicate-marker-recovers-after-label", "f(&tag: ^x)"),
    ("projected-write-remains-an-expression", "f(&x.field)"),
    ("missing-value-diagnostic", "f(&)"),
    ("use-label-is-not-regular-call-syntax", "f(use: x)"),
    ("declaration-marker-use-label", "#M(use: x) fn run() {}"),
    ("marker-group-use-label", "#[M(use: x)] fn run() {}"),
    ("statement-marker-use-label", "#Off(use: x) {}"),
];

fn jet_byte_array_literal(source: &str) -> String {
    let bytes = source.bytes().map(|byte| byte.to_string()).collect::<Vec<_>>().join(", ");
    format!("[U8]{{{bytes}}}")
}

fn tier_run_source() -> String {
    let mut source = String::from("\nfn run() {\n");
    for (index, &(_, input)) in CASES.iter().enumerate() {
        let input_bytes = format!("parser_input_{index}");
        let facts = format!("parser_facts_{index}");
        let cursor = format!("parser_cursor_{index}");
        source.push_str(&format!("    {input_bytes} :: {}\n", jet_byte_array_literal(input)));
        source.push_str(&format!("    print({index})\n    print({input_bytes}.len())\n    {cursor} := 0\n"));
        source.push_str(&format!("    loop {cursor} < {input_bytes}.len() {{\n"));
        source.push_str(&format!("        print(Int.from_u8({input_bytes}[{cursor}]))\n"));
        source.push_str(&format!("        {cursor} += 1\n    }}\n"));
        source.push_str(&format!("    {facts} :: selfhost_parser_probe({input_bytes})\n    print({facts}.len())\n    {cursor} = 0\n"));
        source.push_str(&format!("    loop {cursor} < {facts}.len() {{\n"));
        source.push_str(&format!("        print({facts}[{cursor}])\n"));
        source.push_str(&format!("        {cursor} += 1\n    }}\n"));
    }
    source.push_str("}\n");
    source
}

fn expected_tier_stdout() -> String {
    let mut output = String::new();
    for (index, &(_, input)) in CASES.iter().enumerate() {
        let bytes = input.as_bytes();
        output.push_str(&format!("{index}\n{}\n", bytes.len()));
        for byte in bytes {
            output.push_str(&format!("{byte}\n"));
        }
        let facts = native_facts(input);
        output.push_str(&format!("{}\n", facts.len()));
        for fact in facts {
            output.push_str(&format!("{fact}\n"));
        }
    }
    output
}

fn parser_source(root: &Path) -> (String, usize) {
    let mut source = String::new();
    let mut source_file_count = 0;
    for row in SOURCE_MANIFEST.lines() {
        let path = row.trim();
        if path.is_empty() || path.starts_with('#') {
            continue;
        }
        let parser_input = path.starts_with("Compiler/JetLexer/Source/Lexer/")
            || path == "Compiler/JetFoundation/Source/Types/Types.jet"
            || path.starts_with("Compiler/JetAst/Source/AST/")
            || path.starts_with("Compiler/JetParser/Source/Parser/");
        if parser_input {
            source_file_count += 1;
            source.push_str("// [selfhost parser parity source: ");
            source.push_str(path);
            source.push_str("]\n");
            source.push_str(&fs::read_to_string(root.join(path)).expect("canonical parser source"));
            source.push('\n');
        }
    }
    source.push_str(PROBE_SOURCE);
    source.push_str(&tier_run_source());
    (source, source_file_count)
}

struct Pass {
    mir: MirProgram,
    function: MirFunctionId,
    source: String,
    source_file_count: usize,
}

fn lower_bundle(bundle: &mut jet::AST::ProgramBundle, context: &str) -> MirProgram {
    let diagnostics = jet::Sema::check_bundle(bundle, jet::Sema::CompileMode::Run);
    let errors = diagnostics.iter().filter(|row| row.severity == Severity::Error).collect::<Vec<_>>();
    assert!(errors.is_empty(), "{context}: checked parser fixture: {errors:#?}");
    let request = MirArtifactRequest::new(
        MirArtifactTarget::Cranelift,
        MirArtifactKind::NativeExecutable,
        MirArtifactBuildMode::Dev,
    );
    jet::Codegen::TIR::lower_checked_mir_program_for(bundle, request)
        .unwrap_or_else(|error| panic!("{context}: canonical parser MIR: {error}"))
        .0
}

fn bootstrap() -> Arc<Pass> {
    jet::run_compiler_work(|| {
        let scratch = common::Scratch::new("selfhost_parser_bootstrap");
        tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
        let path = scratch.path.join("parser.jet");
        let (source, source_file_count) = parser_source(Path::new(env!("CARGO_MANIFEST_DIR")));
        fs::write(&path, &source).unwrap();
        let mut bundle = jet::Loader::load_entry(path.to_str().unwrap())
            .expect("Rust reference bootstraps the Jet parser source");
        let mir = lower_bundle(&mut bundle, "generated Jet parser");
        let function = mir.functions.iter().find(|row| row.name == "selfhost_parser_probe")
            .expect("compiled parser probe entry").id;
        Arc::new(Pass { mir, function, source, source_file_count })
    })
}

fn pass() -> Arc<Pass> {
    static PASS: OnceLock<Arc<Pass>> = OnceLock::new();
    PASS.get_or_init(bootstrap).clone()
}


impl Pass {
    fn evaluate(&self, source: &str) -> Vec<i64> {
        let bytes = source.bytes().map(|byte| MirValue::Int(i64::from(byte))).collect();
        let args = [MirValue::List(bytes)];
        let config = MirEvalConfig { fuel: 50_000_000, ..MirEvalConfig::default() };
        let result = evaluate_mir_function_with_config(&self.mir, self.function, &args, &config)
            .unwrap_or_else(|error| panic!("generated Jet parser evaluation: {:?}", error.diagnostic));
        assert_eq!(result.status, MirExecutionStatus::Completed, "parser evaluator status");
        assert_eq!(result.exit_code, 0, "parser evaluator exit code");
        assert!(result.stdout.is_empty() && result.stderr.is_empty(), "parser probe must be silent");
        let value = match result.value {
            MirValue::Present(value) => *value,
            value => value,
        };
        let MirValue::List(values) = value else {
            panic!("parser probe returned a non-list carrier: {value:?}");
        };
        values.into_iter().map(|value| match value {
            MirValue::Int(value) => value,
            value => panic!("parser probe returned a non-integer fact: {value:?}"),
        }).collect()
    }
}

fn push_text(facts: &mut Vec<i64>, text: &str) {
    let bytes = text.as_bytes();
    facts.push(bytes.len() as i64);
    facts.extend(bytes.iter().map(|byte| i64::from(*byte)));
}

fn push_diagnostic(facts: &mut Vec<i64>, diagnostic: &Diagnostic) {
    push_text(facts, &diagnostic.code);
    facts.push(match diagnostic.severity {
        Severity::Error => 1,
        Severity::Lint => 2,
    });
    push_text(facts, &diagnostic.what);
    push_text(facts, &diagnostic.why);
    push_text(facts, &diagnostic.fix);
    if let Some(span) = diagnostic.span.as_ref() {
        facts.extend([1, span.start as i64, span.end as i64]);
    } else {
        facts.push(0);
    }
    if let Some(edit) = diagnostic.edit.as_ref() {
        facts.extend([1, edit.span.start as i64, edit.span.end as i64]);
        push_text(facts, &edit.new_text);
    } else {
        facts.push(0);
    }
}

fn push_expr(facts: &mut Vec<i64>, expression: &Expr) {
    match expression {
        Expr::Ident(name, span) => {
            facts.push(1);
            push_text(facts, name);
            facts.extend([span.start as i64, span.end as i64]);
        }
        Expr::Spread(value, span) => {
            facts.extend([2, span.start as i64, span.end as i64]);
            push_expr(facts, value);
        }
        Expr::Field(base, member, span) => {
            facts.push(3);
            push_text(facts, member);
            facts.extend([span.start as i64, span.end as i64]);
            push_expr(facts, base);
        }
        _ => facts.push(0),
    }
}

fn push_label(facts: &mut Vec<i64>, label: Option<&(String, jet::Diagnostics::Span)>) {
    if let Some((name, span)) = label {
        facts.push(1);
        push_text(facts, name);
        facts.extend([span.start as i64, span.end as i64]);
    } else {
        facts.push(0);
    }
}

fn push_call_arg(facts: &mut Vec<i64>, argument: &CallArg) {
    let convention = match argument.convention {
        AccessConvention::Read => 0,
        AccessConvention::Write => 1,
        AccessConvention::Move => 2,
    };
    facts.extend([
        convention,
        argument.span.start as i64,
        argument.span.end as i64,
        i64::from(argument.spread),
    ]);
    push_label(facts, argument.label.as_ref());
    push_expr(facts, &argument.expr);
}

fn push_marker(facts: &mut Vec<i64>, marker: &Marker) {
    facts.push(marker.args.len() as i64);
    for (index, argument) in marker.args.iter().enumerate() {
        push_label(facts, marker.arg_labels.get(index).and_then(Option::as_ref));
        match argument {
            MarkerCallArg::Expr(expression) => push_expr(facts, expression),
            MarkerCallArg::EffectRow { span, .. } => {
                facts.extend([4, span.start as i64, span.end as i64]);
            }
        }
    }
}

fn push_program(facts: &mut Vec<i64>, program: Option<&Program>, diagnostics: &[Diagnostic]) {
    let show_ast = diagnostics.iter().all(|diagnostic| diagnostic.code == "E0029");
    if !show_ast {
        facts.extend([0, 0]);
        return;
    }
    let Some(program) = program else {
        facts.extend([0, 0]);
        return;
    };
    if let Some(statement) = program.script_body.first() {
        match statement {
            Stmt::Expr(Expr::Call(call)) => {
                facts.extend([1, call.args.len() as i64]);
                for argument in &call.args {
                    push_call_arg(facts, argument);
                }
                return;
            }
            Stmt::Switched { marker, .. } => {
                facts.push(3);
                push_marker(facts, marker);
                return;
            }
            _ => {}
        }
    }
    if let Some(Item::Func(function)) = program.items.first() {
        if let Some(marker) = function.markers.first() {
            facts.push(2);
            push_marker(facts, marker);
            return;
        }
    }
    facts.extend([0, 0]);
}

fn push_diagnostics(facts: &mut Vec<i64>, diagnostics: &[Diagnostic]) {
    facts.push(diagnostics.len() as i64);
    for diagnostic in diagnostics {
        push_diagnostic(facts, diagnostic);
    }
}

fn native_facts(source: &str) -> Vec<i64> {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(source);
    assert!(lexer_diagnostics.is_empty(), "fixture lex diagnostics: {lexer_diagnostics:?}");
    let mut facts = Vec::new();
    match jet::Parser::parse_for_check_with_source(&tokens, source) {
        Ok((program, diagnostics)) => {
            push_diagnostics(&mut facts, &diagnostics);
            push_program(&mut facts, Some(&program), &diagnostics);
        }
        Err(diagnostics) => {
            push_diagnostics(&mut facts, &diagnostics);
            push_program(&mut facts, None, &diagnostics);
        }
    }
    facts
}

#[test]
fn generated_jet_call_arguments_match_native_parser_contract() {
    let pass = pass();
    assert_eq!(
        pass.source_file_count, 29,
        "source slice is 4 lexer + 1 foundation-types + 5 AST + 19 parser files"
    );
    let candidate_facts = jet::run_compiler_work(|| {
        CASES.iter().map(|(_, source)| pass.evaluate(source)).collect::<Vec<_>>()
    });
    for ((name, source), actual) in CASES.iter().zip(candidate_facts) {
        assert_eq!(actual, native_facts(source), "{name}: generated Jet parser parity");
    }

    assert!(tir_support::have_rustc(), "AOT and awaited-Wasm proof is required, never skipped");
    let node = Command::new("node").arg("--version").output()
        .expect("awaited-Wasm proof requires Node; absence is not a pass");
    assert!(node.status.success(), "awaited-Wasm proof requires working Node");
    let expected = expected_tier_stdout();
    tir_support::assert_tiers_agree("selfhost_parser_call_args", &pass.source, &expected);
    tir_support::assert_awaited_web_tier("selfhost_parser_call_args_web", &pass.source, &expected);
}

