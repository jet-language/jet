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

const PROBE_SOURCE: &str = r####"
fn selfhost_parser_text(facts: &[Int], text: String) {
    bytes := text.bytes()
    &facts.push(bytes.len())
    index := 0
    loop index < bytes.len() {
        &facts.push(Int.from_u8(bytes[index]))
        index += 1
    }
}

fn selfhost_parser_label(facts: &[Int], label: CallLabel?) {
    if label == {
        .Val(value) -> {
            &facts.push(1)
            selfhost_parser_text(&facts, value.name)
            &facts.push(value.span.start)
            &facts.push(value.span.end)
        }
        .None -> &facts.push(0)
    }
}

fn selfhost_parser_expr(facts: &[Int], expression: Expr) {
    if expression == {
        .Ident(name, span) -> {
            &facts.push(1)
            selfhost_parser_text(&facts, name)
            &facts.push(span.start)
            &facts.push(span.end)
        }
        .Spread(value, span) -> {
            &facts.push(2)
            &facts.push(span.start)
            &facts.push(span.end)
            selfhost_parser_expr(&facts, value)
        }
        .Field(base, member, span) -> {
            &facts.push(3)
            selfhost_parser_text(&facts, member)
            &facts.push(span.start)
            &facts.push(span.end)
            selfhost_parser_expr(&facts, base)
        }
        .OptField(base, member, member_span, _, span) -> {
            &facts.push(5)
            selfhost_parser_text(&facts, member)
            &facts.push(member_span.start)
            &facts.push(member_span.end)
            &facts.push(span.start)
            &facts.push(span.end)
            selfhost_parser_expr(&facts, base)
        }
        .OptMethodCall(receiver, method, method_span, _, args, span) -> {
            &facts.push(6)
            selfhost_parser_text(&facts, method)
            &facts.push(method_span.start)
            &facts.push(method_span.end)
            &facts.push(span.start)
            &facts.push(span.end)
            &facts.push(args.len())
            loop argument in args -> selfhost_parser_call_arg(&facts, argument)
            selfhost_parser_expr(&facts, receiver)
        }
        .MethodCall(receiver, method, _, _, _, args, _, _, _, _) -> {
            &facts.push(7)
            selfhost_parser_text(&facts, method)
            &facts.push(args.len())
            loop argument in args -> selfhost_parser_call_arg(&facts, argument)
            selfhost_parser_expr(&facts, receiver)
        }
        else -> &facts.push(0)
    }
}

fn selfhost_parser_call_arg(facts: &[Int], argument: CallArg) {
    convention := 0
    if argument.convention == AccessConvention.Write -> convention = 1
    if argument.convention == AccessConvention.Move -> convention = 2
    &facts.push(convention)
    &facts.push(argument.span.start)
    &facts.push(argument.span.end)
    spread := 0
    if argument.spread -> spread = 1
    &facts.push(spread)
    selfhost_parser_label(&facts, argument.label)
    selfhost_parser_expr(&facts, argument.expr)
}

fn selfhost_parser_marker(facts: &[Int], marker: Marker) {
    &facts.push(marker.args.len())
    index := 0
    loop index < marker.args.len() {
        label := CallLabel?{None}
        if index < marker.arg_labels.len() -> label = marker.arg_labels[index]
        selfhost_parser_label(&facts, label)
        if marker.args[index] == {
            .Expr(value) -> selfhost_parser_expr(&facts, value)
            .EffectRow(_, span) -> {
                &facts.push(4)
                &facts.push(span.start)
                &facts.push(span.end)
            }
        }
        index += 1
    }
}

fn selfhost_parser_diagnostic(facts: &[Int], diagnostic: Diagnostic) {
    selfhost_parser_text(&facts, diagnostic.code)
    &facts.push(diagnostic.severity)
    selfhost_parser_text(&facts, diagnostic.what)
    selfhost_parser_text(&facts, diagnostic.why)
    selfhost_parser_text(&facts, diagnostic.fix)
    if diagnostic.span == {
        .Val(span) -> {
            &facts.push(1)
            &facts.push(span.start)
            &facts.push(span.end)
        }
        .None -> &facts.push(0)
    }
    if diagnostic.edit == {
        .Val(edit) -> {
            &facts.push(1)
            &facts.push(edit.span.start)
            &facts.push(edit.span.end)
            selfhost_parser_text(&facts, edit.new_text)
        }
        .None -> &facts.push(0)
    }
    applicability := 0
    if diagnostic.applicability == .Val(value) {
        if value == DiagnosticFixApplicability.Safe -> applicability = 1
        if value == DiagnosticFixApplicability.Suggested -> applicability = 2
    }
    &facts.push(applicability)
    safety := 0
    if diagnostic.safety == .Val(value) {
        if value == DiagnosticFixSafety.Formatting -> safety = 1
        if value == DiagnosticFixSafety.BehaviorPreserving -> safety = 2
        if value == DiagnosticFixSafety.APIChanging -> safety = 3
        if value == DiagnosticFixSafety.TargetChanging -> safety = 4
        if value == DiagnosticFixSafety.NeedsReview -> safety = 5
    }
    &facts.push(safety)
    &facts.push(diagnostic.labels.len())
    loop label in diagnostic.labels {
        &facts.push(label.span.start)
        &facts.push(label.span.end)
        selfhost_parser_text(&facts, label.message)
    }
}

pub fn selfhost_parser_probe(source: [U8]) -> [Int] {
    parsed := parse_source(source)
    facts := [Int]{}
    &facts.push(parsed.diagnostics.len())
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
        &facts.push(0)
        &facts.push(0)
        return facts
    }

    if parsed.program.script_body.len() > 0 {
        statement := parsed.program.script_body[0]
        if statement == {
            .Expr(expression) -> {
                if expression == {
                    .Call(call) -> {
                        &facts.push(1)
                        &facts.push(call.args.len())
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
                &facts.push(3)
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
                    &facts.push(2)
                    selfhost_parser_marker(&facts, function.markers[0])
                    return facts
                }
            }
            else -> {}
        }
    }
    &facts.push(0)
    &facts.push(0)
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
    ("optional-method-call", "f(a?.m(x))"),
    ("optional-method-chain", "f(a?.b?.m())"),
    // L0507 (#3681): exact diagnostic, source span, edit and refusal parity.
    ("arm-table-braced", "fn show(score: Int) {\n    if score >= 90 {\n        print(\"a\")\n    } else if score >= 80 {\n        print(\"b\")\n    } else {\n        print(\"c\")\n    }\n    print(\"done\")\n}\n"),
    ("arm-table-arrow", "fn run() {\n    if false -> print(\"a\") else if true -> print(\"b\") else -> print(\"c\")\n}\n"),
    ("arm-table-value", "fn run() {\n    answer :: if true -> {\n        value :: 40 + 2\n        value\n    } else -> 0\n    print(answer)\n}\n"),
    ("arm-table-nested", "fn run(a: Bool, b: Bool) {\n    if a {\n        if b {\n            print(\"1\")\n        } else if a {\n            print(\"2\")\n        }\n    } else if b {\n        print(\"3\")\n    }\n}\n"),
    ("arm-table-comment-refusal", "fn run(a: Bool) {\n    if a {\n        print(\"x\")\n    } /* keep */ else if !a {\n        print(\"y\")\n    }\n}\n"),
    // E0082 (#3725): `&&` and `||` mixed without parentheses.
    ("logic-or-then-and", "f(a || b && c)"),
    ("logic-and-then-or", "f(a && b || c)"),
    ("logic-grouped-and-chains", "f((a || b) && c, a || (b && c), a && b && c, a || b || c)"),
    // E0083 (#3719): an unclosed delimiter is reported once, at its opener.
    ("unclosed-paren-call-p16", "fn run() {\n    print(\"hello\"\n}\n"),
    ("unclosed-bracket-list", "fn run() {\n    values :: [1, 2, 3\n    print(values.len())\n}\n"),
    ("unclosed-brace-block", "fn run() {\n    print(\"hello\")\n"),
    ("unclosed-brace-interpolation", "fn run() {\n    count :: 1\n    print(\"count {count\")\n}\n"),
    // E0160 (#3727): `++` and `--` are retired.
    ("retired-step-statement", "count++"),
    ("retired-step-double-minus", "f(a--b)"),
    ("retired-step-prefix-increment", "++count"),
    ("retired-step-postfix-decrement", "count--"),
    ("retired-step-prefix-decrement", "--count"),
    ("retired-step-call-argument", "print(n++)"),
    ("retired-step-loop-condition", "loop --lives > 0 { print(lives) }"),
    // D-TYPE-SUFFIX1 (#3687): all prefix positions, with exact safe edits.
    ("type-prefix-list-element", "struct Entry { tags: [?String] }"),
    ("type-prefix-optional-result", "fn find(id: Int) -> ?Entry { return None }"),
    ("type-prefix-optional-literal", "limit :: ?Int{None}"),
    ("type-prefix-unit-contract", "fn save(id: Int) !SaveError {}"),
    ("type-prefix-value-contract", "fn load() -> Int !SaveError { return Ok(1) }"),
    ("type-prefix-union-contract", "fn load() -> Int !(SaveError | LoadError) { return Ok(1) }"),
    ("bracket-range-access-marks", "f(values[0..1], &values[0..1], ~values[0..1])"),
    // E0393 (#3743): the retired `name: Type :: value` binding.
    ("retired-typed-binding", "fn run() {\n    limit: U8 :: 250\n    print(limit)\n}\n"),
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

/// #4555: the parity source is the jet_parser package plus every package its
/// `package.jet` `deps` name, transitively, in `sources.list` order. A
/// hand-picked file slice drifted each time a parser input gained a
/// dependency; the package graph is the closure the parser itself builds with.
fn parser_packages(root: &Path) -> Vec<String> {
    let mut packages = vec![String::from("Compiler/JetParser")];
    let mut index = 0;
    while index < packages.len() {
        let manifest = fs::read_to_string(root.join(&packages[index]).join("package.jet"))
            .expect("parser package manifest");
        for row in manifest.lines() {
            // A dependency row reads `jet_lexer: ../JetLexer,`.
            if let Some((_, directory)) = row.trim().trim_end_matches(',').split_once(": ../") {
                let package = format!("Compiler/{directory}");
                if !packages.contains(&package) {
                    packages.push(package);
                }
            }
        }
        index += 1;
    }
    packages
}

fn parser_source(root: &Path) -> String {
    let packages = parser_packages(root);
    let mut source = common::compiler_parity_source("selfhost parser parity source", |path| {
        packages.iter().any(|package| {
            path.strip_prefix(package.as_str()).is_some_and(|rest| rest.starts_with("/Source/"))
        })
    });
    source.push_str(PROBE_SOURCE);
    source.push_str(&tier_run_source());
    source
}

struct Pass {
    mir: MirProgram,
    function: MirFunctionId,
    source: String,
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
        let source = parser_source(Path::new(env!("CARGO_MANIFEST_DIR")));
        fs::write(&path, &source).unwrap();
        let mut bundle = jet::Loader::load_entry(path.to_str().unwrap())
            .expect("Rust reference bootstraps the Jet parser source");
        let mir = lower_bundle(&mut bundle, "generated Jet parser");
        let function = mir.functions.iter().find(|row| row.name == "selfhost_parser_probe")
            .expect("compiled parser probe entry").id;
        Arc::new(Pass { mir, function, source })
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
        let config = MirEvalConfig { fuel: Some(50_000_000), ..MirEvalConfig::default() };
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
    facts.push(match diagnostic.applicability {
        None => 0,
        Some(jet::Diagnostics::FixApplicability::Safe) => 1,
        Some(jet::Diagnostics::FixApplicability::Suggested) => 2,
    });
    facts.push(match diagnostic.safety {
        None => 0,
        Some(jet::Diagnostics::FixSafety::Formatting) => 1,
        Some(jet::Diagnostics::FixSafety::BehaviorPreserving) => 2,
        Some(jet::Diagnostics::FixSafety::ApiChanging) => 3,
        Some(jet::Diagnostics::FixSafety::TargetChanging) => 4,
        Some(jet::Diagnostics::FixSafety::NeedsReview) => 5,
    });
    facts.push(diagnostic.labels.len() as i64);
    for label in &diagnostic.labels {
        facts.extend([label.span.start as i64, label.span.end as i64]);
        push_text(facts, &label.message);
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
        Expr::OptField {
            base,
            member,
            member_span,
            span,
            ..
        } => {
            facts.push(5);
            push_text(facts, member);
            facts.extend([
                member_span.start as i64,
                member_span.end as i64,
                span.start as i64,
                span.end as i64,
            ]);
            push_expr(facts, base);
        }
        // D-SUGAR6: the native parser keeps `a?.m(args)` as a call through
        // the optional member; the Jet parser names it `OptMethodCall`.
        Expr::CallValue { callee, args, span } if matches!(callee.as_ref(), Expr::OptField { .. }) => {
            let Expr::OptField {
                base,
                member,
                member_span,
                ..
            } = callee.as_ref()
            else {
                unreachable!("matched an optional method call");
            };
            facts.push(6);
            push_text(facts, member);
            facts.extend([
                member_span.start as i64,
                member_span.end as i64,
                base.span().start as i64,
                span.end as i64,
                args.len() as i64,
            ]);
            for argument in args {
                push_call_arg(facts, argument);
            }
            push_expr(facts, base);
        }
        Expr::MethodCall { receiver, method, args, .. } => {
            facts.push(7);
            push_text(facts, method);
            facts.push(args.len() as i64);
            for argument in args {
                push_call_arg(facts, argument);
            }
            push_expr(facts, receiver);
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
    let mut facts = Vec::new();
    // A lexer error stops the native front end before parsing, so the Jet
    // parser must report that one error and nothing after it.
    if !lexer_diagnostics.is_empty() {
        push_diagnostics(&mut facts, &lexer_diagnostics);
        push_program(&mut facts, None, &lexer_diagnostics);
        return facts;
    }
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

/// Decode the diagnostic prefix produced by the Jet parser, retaining the
/// source locations and machine-fix grades for the shared UI renderer.
fn diagnostics_from_facts(facts: &[i64]) -> Vec<Diagnostic> {
    let mut cursor = 0usize;
    let mut next = || {
        cursor += 1;
        facts[cursor - 1]
    };
    fn text(next: &mut impl FnMut() -> i64) -> String {
        let len = next() as usize;
        let bytes = (0..len).map(|_| next() as u8).collect::<Vec<_>>();
        String::from_utf8(bytes).expect("diagnostic text is UTF-8")
    }
    let mut out = Vec::new();
    fn span(next: &mut impl FnMut() -> i64) -> jet::Diagnostics::Span {
        jet::Diagnostics::Span::new(next() as usize, next() as usize)
    }
    for _ in 0..next() {
        let code = text(&mut next);
        let severity = next();
        let what = text(&mut next);
        let why = text(&mut next);
        let fix = text(&mut next);
        let primary = if next() == 1 { Some(span(&mut next)) } else { None };
        let mut diagnostic = Diagnostic::error(code, what, why, fix, primary);
        diagnostic.severity = match severity {
            1 => Severity::Error,
            2 => Severity::Lint,
            value => panic!("unknown Jet diagnostic severity: {value}"),
        };
        diagnostic.edit = if next() == 1 {
            Some(jet::Diagnostics::TextEdit { span: span(&mut next), new_text: text(&mut next) })
        } else {
            None
        };
        diagnostic.applicability = match next() {
            0 => None,
            1 => Some(jet::Diagnostics::FixApplicability::Safe),
            2 => Some(jet::Diagnostics::FixApplicability::Suggested),
            value => panic!("unknown Jet fix applicability: {value}"),
        };
        diagnostic.safety = match next() {
            0 => None,
            1 => Some(jet::Diagnostics::FixSafety::Formatting),
            2 => Some(jet::Diagnostics::FixSafety::BehaviorPreserving),
            3 => Some(jet::Diagnostics::FixSafety::ApiChanging),
            4 => Some(jet::Diagnostics::FixSafety::TargetChanging),
            5 => Some(jet::Diagnostics::FixSafety::NeedsReview),
            value => panic!("unknown Jet fix safety: {value}"),
        };
        for _ in 0..next() {
            diagnostic.labels.push(jet::Diagnostics::DiagnosticLabel {
                span: span(&mut next),
                message: text(&mut next),
            });
        }
        out.push(diagnostic);
    }
    out
}

#[test]
fn generated_jet_call_arguments_match_native_parser_contract() {
    let pass = pass();
    let candidate_facts = jet::run_compiler_work(|| {
        CASES.iter().map(|(_, source)| pass.evaluate(source)).collect::<Vec<_>>()
    });
    for ((name, source), actual) in CASES.iter().zip(candidate_facts) {
        if name.starts_with("unclosed-") || name.starts_with("retired-step-") {
            assert_eq!(actual[0], 1, "{name}: exactly one teaching diagnostic");
        }
        // #3719: the lexer's synthetic line terminator is never shown to a
        // reader as `;`; a source without a `;` must never have one named.
        if !source.contains(';') {
            let rendered = jet::Diagnostics::render_all(name, source, &diagnostics_from_facts(&actual));
            assert!(!rendered.contains("`;`"), "{name}: Jet parser diagnostic names the synthetic terminator: {rendered}");
        }
        assert_eq!(actual, native_facts(source), "{name}: generated Jet parser parity");
    }

    // #3391: the frozen native parser still splits this retired alias into
    // two bounds. Jet must retain one ordinary range argument for sema's
    // E0214 report instead. This expected tree deliberately isn't native parity.
    let actual = jet::run_compiler_work(|| pass.evaluate("f(values.view(0..1))"));
    let mut expected = vec![0, 1, 1, 0, 2, 19, 0, 0, 7];
    push_text(&mut expected, "view");
    expected.extend([1, 0, 14, 18, 0, 0, 0, 1]);
    push_text(&mut expected, "values");
    expected.extend([2, 8]);
    assert_eq!(actual, expected, "retired view retains one ordinary range argument");

    // Render diagnostics actually produced by Jet, not reports from a frozen
    // CLI. These existing UI snapshots remain the one wording contract.
    for stem in [
        "unclosed_paren_call",
        "unclosed_bracket_list",
        "unclosed_brace_block",
        "unclosed_brace_interpolation",
        "logic_mix_or_then_and",
        "logic_mix_and_then_or",
        "incr_retired_statement",
        "incr_retired_expression",
        "incr_retired_double_minus",
        "type_mark_prefix_optional_retired",
    ] {
        let file = format!("tests/ui/{stem}.jet");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = fs::read_to_string(root.join(&file)).expect("teaching UI fixture");
        let actual = jet::run_compiler_work(|| pass.evaluate(&source));
        let diagnostics = diagnostics_from_facts(&actual);
        if stem.starts_with("unclosed_") {
            assert_eq!(diagnostics.len(), 1, "{file}: exactly one report");
        }
        let rendered = jet::Diagnostics::render_all(&file, &source, &diagnostics);
        assert!(!rendered.contains("found `;`"), "{file}: synthetic terminator wording: {rendered}");
        let expected = fs::read_to_string(root.join(format!("tests/ui/{stem}.stderr")))
            .expect("teaching UI snapshot");
        assert_eq!(rendered.trim_end(), expected.trim_end(), "{file}: Jet-produced UI snapshot");
    }

    assert!(tir_support::have_rustc(), "AOT and awaited-Wasm proof is required, never skipped");
    let node = Command::new("node").arg("--version").output()
        .expect("awaited-Wasm proof requires Node; absence is not a pass");
    assert!(node.status.success(), "awaited-Wasm proof requires working Node");
    let expected = expected_tier_stdout();
    tir_support::assert_tiers_agree("selfhost_parser_call_args", &pass.source, &expected);
    tir_support::assert_awaited_web_tier("selfhost_parser_call_args_web", &pass.source, &expected);
}

