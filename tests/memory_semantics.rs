mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MEMORY_DENIAL_SOURCE: &str = r#"
fn run() :[!Mem.Alloc]> {
    print("memory denial")
}
"#;

const MEMORY_COMPTIME_SOURCE: &str = r#"
@answer :: "memory denial"

fn run() :[!Mem.Alloc]> {
    print(@answer)
}
"#;

#[test]
fn memory_denial_parser_keeps_the_canonical_effect_row() {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(MEMORY_DENIAL_SOURCE);
    assert!(lexer_diagnostics.is_empty(), "lex: {lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("memory denial source parses");
    let function = program
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(function) if function.name == "run" => Some(function),
            _ => None,
        })
        .expect("run function");
    let effects = function
        .declared_effects
        .as_ref()
        .expect("declared denial row")
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(effects, vec!["!Mem.Alloc"]);

    let (tokens, lexer_diagnostics) = jet::Lexer::lex("fn run() :[Mem.Alloc(above: 1)]> {}");
    assert!(lexer_diagnostics.is_empty(), "lex positive row: {lexer_diagnostics:?}");
    let diagnostics = jet::Parser::parse(&tokens).expect_err("bounded rights are denial-only");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "E0119" && diagnostic.fix.contains("!Mem.Alloc(above: 1)")
    }));
}

#[test]
fn memory_denial_sema_and_tir_share_one_erased_contract() {
    let compiled = jet::compile(MEMORY_DENIAL_SOURCE).expect("memory denial source compiles");
    assert!(!compiled.rust.contains("!Mem.Alloc"));
}

#[test]
fn parameterized_memory_rights_are_denials_not_positive_effects() {
    let error = jet::Package::PackageFacts::parse(
        "name: \"memory\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [Mem.Alloc(above: 65536)] } }\n",
        "test",
    )
    .expect_err("a bounded memory right must be a denial");
    assert!(matches!(
        error,
        jet::Package::PackageParseError::BadEffectsBlock(detail)
            if detail.contains("authority.holds.deny")
    ));
}

#[test]
fn manifest_memory_denial_reaches_the_same_sema_fact_pass() {
    let root = common::unique_tmp("jet_manifest_memory_denial");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("package.jet"),
        "name: \"memory\"\nversion: \"0.1.0\"\nauthority: .{ holds: { deny: [Mem.Alloc] } }\n",
    )
    .unwrap();
    std::fs::write(root.join("main.jet"), "fn run() { print(\"frame {1}\") }\n").unwrap();
    let mut bundle = jet::Loader::load_entry(root.join("main.jet").to_str().unwrap()).unwrap();
    assert_eq!(bundle.package_guarantees.memory_denials, vec!["Mem.Alloc"]);
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0921"),
        "manifest denial did not reach the memory fact pass: {diagnostics:#?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn memory_denial_matches_aot_jit_and_interpreter() {
    tir_support::assert_tiers_agree("memory-denial-parity", MEMORY_DENIAL_SOURCE, "memory denial\n");
}

#[test]
fn memory_denial_example_matches_all_hosted_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "memory/effect_denials",
        include_str!("../examples/features/expected/memory/effect_denials.out"),
    );
}

#[test]
fn memory_denial_matches_the_dev_interpreter_path() {
    let root = common::unique_tmp("jet_memory_denial_dev");
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.jet");
    std::fs::write(&entry, MEMORY_DENIAL_SOURCE).unwrap();
    let output = Command::new(jet())
        .args(["dev", entry.to_str().unwrap(), "--interpret", "--watch=off"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dev rejected the memory denial source: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "memory denial\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn memory_denial_matches_comptime() {
    let compiled = jet::compile(MEMORY_COMPTIME_SOURCE).expect("comptime source compiles");
    assert!(compiled.rust.contains("memory denial"));
}

#[test]
fn memory_denial_matches_repl() {
    let transcript = jet::REPL::run_transcript(
        &[
            "fn denied() :[!Mem.Alloc]> { print(\"memory denial\") }",
            "denied()",
        ],
        None,
    );
    assert!(transcript.contains("memory denial"), "REPL lost the denial program: {transcript}");
    assert!(!transcript.contains("E0921"), "REPL changed a valid denial into a diagnostic: {transcript}");
}

#[test]
fn memory_denial_matches_web_lowering() {
    let compiled = jet::compile_web_with_path(
        MEMORY_DENIAL_SOURCE,
        "tests/fixtures/memory_denial_web.jet",
    )
    .expect("web accepts the memory denial source");
    assert!(compiled.web.is_some(), "web lowering produced no artifact");
}

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(jet())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env(
            "JET_MEMORY_LEDGER",
            root.join(".jet/memory/ledger-v1.jsonl"),
        )
        .output()
        .unwrap()
}
fn assert_cli_snapshot(name: &str, actual: &[u8]) {
    let expected = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/cli")
            .join(name),
    )
    .unwrap();
    assert_eq!(actual, expected, "CLI snapshot mismatch: {name}");
}


fn ledger(root: &Path, rows: &[&str]) {
    let path = root.join(".jet/memory/ledger-v1.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, rows.join("\n") + "\n").unwrap();
}

fn row(kind: &str, code: &str, repairs: &[&str]) -> String {
    let repairs = repairs
        .iter()
        .map(|repair| format!("\"{repair}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"jet.memory.ledger\",\"version\":1,\"kind\":\"{kind}\",\"code\":\"{code}\",\"source\":\"main.jet\",\"span_start\":0,\"span_end\":6,\"byte_spans\":true,\"scope\":\"run\",\"provenance\":\"fixture\",\"detail\":\"observed\",\"expected\":\"borrow\",\"repairs\":[{repairs}]}}"
    )
}

#[test]
fn memo_computed_and_view_copy_examples_agree_across_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "memory/memoize",
        include_str!("../examples/features/expected/memory/memoize.out"),
    );
    tir_support::assert_example_cli_tiers_agree(
        "memory/computed_field",
        include_str!("../examples/features/expected/memory/computed_field.out"),
    );
    tir_support::assert_example_cli_tiers_agree(
        "memory/copy_verb",
        include_str!("../examples/features/expected/memory/copy_verb.out"),
    );
    // D-MEM-COPYSEM1=A criterion 2: the IMPLICIT half of the same rule. Every
    // line of this golden is a read window entering an owning slot — an
    // interpolation, an owned `String` parameter, and a returned `String` — so
    // AOT, default `jet run`, and the forced interpreter must agree byte for
    // byte, not only on the explicit `~` spelling above.
    tir_support::assert_example_cli_tiers_agree(
        "memory/string_view",
        include_str!("../examples/features/expected/memory/string_view.out"),
    );
}

/// D-MEM-COPYSEM1=A + I8/I9: the read-view materialization symbol has ONE home,
/// `jet::Codegen::TIR::view_copy_symbol`. Before this guard the same four-arm
/// type ladder was written out three times — the AOT emitter, the wasm emitter,
/// and the JS emitter — so a rename or a new window shape could give one tier a
/// different kernel than the others while every golden still passed.
#[test]
fn one_table_names_the_shared_read_view_copy_kernel_for_every_tier() {
    use jet::AST::Type;
    use jet::Codegen::TIR::{view_copy_owned_type, view_copy_symbol};

    let view_of = |element: Type| Type::Apply {
        name: "View".to_string(),
        args: vec![element],
    };
    let str_view = view_of(Type::Named("str".to_string()));
    let int_view = view_of(Type::Int);

    // A string window and a list window are the only two shapes, and the
    // symbol travels with the owned destination type sema chose.
    assert_eq!(view_copy_symbol(&str_view), "jet_string_view_copy");
    assert_eq!(view_copy_owned_type(&str_view), Some(Type::String));
    assert_eq!(view_copy_symbol(&int_view), "jet_view_copy");
    assert_eq!(
        view_copy_owned_type(&int_view),
        Some(Type::List(Box::new(Type::Int)))
    );
    // A range place keeps `[T]` at the Jet surface; a `string_view` local keeps
    // `String`. Both still reach the same two kernels.
    assert_eq!(
        view_copy_symbol(&Type::List(Box::new(Type::Int))),
        "jet_view_copy"
    );
    assert_eq!(view_copy_symbol(&Type::String), "jet_string_view_copy");
    // Not a declared window: the caller keeps its own type.
    assert_eq!(view_copy_owned_type(&Type::String), None);

    // Both Prelude kernels declare exactly the symbols the table names, so the
    // native/wasm and web tiers cannot drift apart from each other either.
    let crate_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/jet-codegen/src/Prelude/Core");
    let rust_kernel = std::fs::read_to_string(crate_root.join("ViewCopy.rs")).unwrap();
    let js_kernel = std::fs::read_to_string(crate_root.join("ViewCopy.js")).unwrap();
    for symbol in ["jet_view_copy", "jet_string_view_copy"] {
        assert!(
            rust_kernel.contains(&format!("fn {symbol}")),
            "Prelude/Core/ViewCopy.rs must declare {symbol}"
        );
        assert!(
            js_kernel.contains(&format!("function {symbol}")),
            "Prelude/Core/ViewCopy.js must declare {symbol}"
        );
    }

    // No engine may re-derive the choice. Every tier reads the table above, so
    // a copy-symbol literal outside the table and the Prelude is I8 drift.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/jet-codegen/src");
    for engine in [
        src.join("Codegen/TIR/emit/expressions.rs"),
        src.join("Codegen/Web.rs"),
        src.join("Codegen/TIR/lower/lambdas.rs"),
    ] {
        let text = std::fs::read_to_string(&engine).unwrap();
        assert!(
            !text.contains("jet_string_view_copy(") && !text.contains("jet_view_copy("),
            "{} must call view_copy_symbol instead of naming a copy kernel itself",
            engine.display()
        );
    }
}

#[test]
fn computed_field_nested_value_blocks_use_and_invalidate_sibling_dependencies() {
    let src = r#"
struct Score {
    enabled: Bool
    base: Int
    bonus: Int
    #Memo
    total: Int => if enabled -> {
        subtotal :: base + bonus
        subtotal
    } else -> base
}

fn run() {
    score := Score.{enabled: true, base: 10, bonus: 2}
    print(score.total)
    score.bonus = 7
    print(score.total)
}
"#;
    tir_support::assert_tiers_agree("computed_field_nested_value", src, "12\n17\n");
}

#[test]
fn exercised_memory_ledger_reports_empty_gc_sentry_and_combined_runs() {
    let root = common::unique_tmp("jet_memory_audit");
    std::fs::create_dir_all(&root).unwrap();

    ledger(&root, &[]);
    let empty = run(&root, &["audit", "memory", "--json"]);
    assert!(empty.status.success(), "{}", String::from_utf8_lossy(&empty.stderr));
    assert_cli_snapshot("memory_audit_empty.txt", &empty.stdout);
    let empty_json = String::from_utf8_lossy(&empty.stdout);
    assert!(empty_json.contains("\"coverage\":\"exercised runs only\""));
    assert!(empty_json.contains("\"witnesses\":0"));

    let gc = row("gc", "gc", &["own the value directly"]);
    ledger(&root, &[&gc]);
    let gc_only = run(&root, &["audit", "memory", "--json"]);
    let gc_json = String::from_utf8_lossy(&gc_only.stdout);
    assert!(gc_only.status.success(), "{}", String::from_utf8_lossy(&gc_only.stderr));
    assert_cli_snapshot("memory_audit_gc.txt", &gc_only.stdout);
    assert!(gc_json.contains("\"kind\":\"gc\""));
    assert!(gc_json.contains("\"witnesses\":1"));

    let sentry = row("sentry", "R0802", &["move the raw access"]);
    ledger(&root, &[&sentry]);
    let sentry_only = run(&root, &["audit", "memory", "--json"]);
    let sentry_json = String::from_utf8_lossy(&sentry_only.stdout);
    assert!(sentry_only.status.success(), "{}", String::from_utf8_lossy(&sentry_only.stderr));
    assert_cli_snapshot("memory_audit_sentry.txt", &sentry_only.stdout);
    assert!(sentry_json.contains("\"kind\":\"sentry\""));
    assert!(sentry_json.contains("\"witnesses\":1"));

    ledger(&root, &[&gc, &sentry]);
    let combined = run(&root, &["audit", "memory", "--json"]);
    let combined_json = String::from_utf8_lossy(&combined.stdout);
    assert!(combined.status.success(), "{}", String::from_utf8_lossy(&combined.stderr));
    assert_cli_snapshot("memory_audit_combined.txt", &combined.stdout);
    assert!(combined_json.contains("\"kind\":\"gc\""));
    assert!(combined_json.contains("\"kind\":\"sentry\""));
    assert!(combined_json.contains("\"witnesses\":2"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn memory_fix_applies_one_exact_repair_and_names_ambiguous_options() {
    let root = common::unique_tmp("jet_memory_fix");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.jet"), "borrow").unwrap();

    let exact = row("gc", "gc", &["owned"]);
    ledger(&root, &[&exact]);
    let fixed = run(&root, &["fix", "memory"]);
    assert!(fixed.status.success(), "{}", String::from_utf8_lossy(&fixed.stderr));
    assert_cli_snapshot("memory_fix_exact.txt", &fixed.stdout);
    assert_eq!(std::fs::read_to_string(root.join("main.jet")).unwrap(), "owned");

    std::fs::write(root.join("main.jet"), "borrow").unwrap();
    let ambiguous = row("gc", "gc", &["owned", "#Policy(gc) borrow"]);
    ledger(&root, &[&ambiguous]);
    let unchanged = run(&root, &["fix", "memory"]);
    assert!(unchanged.status.success(), "{}", String::from_utf8_lossy(&unchanged.stderr));
    assert_cli_snapshot("memory_fix_ambiguous.txt", &unchanged.stdout);
    let stdout = String::from_utf8_lossy(&unchanged.stdout);
    assert!(stdout.contains("options: main.jet:0..6 observed"));
    assert!(stdout.contains("  - owned"));
    assert!(stdout.contains("  - #Policy(gc) borrow"));
    assert_eq!(std::fs::read_to_string(root.join("main.jet")).unwrap(), "borrow");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_memory_ledger_is_stable_e2112() {
    let root = common::unique_tmp("jet_memory_missing");
    std::fs::create_dir_all(&root).unwrap();
    let output = run(&root, &["audit", "memory"]);
    assert_eq!(output.status.code(), Some(1));
    let actual = String::from_utf8_lossy(&output.stderr)
        .replace(root.to_str().unwrap(), "WORKSPACE");
    assert_eq!(actual, include_str!("cli/memory_ledger_missing_e2112.txt"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gc_promotions_append_to_the_configured_cross_run_memory_ledger() {
    let root = common::unique_tmp("jet_memory_gc_feed");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("gc.jet");
    std::fs::write(
        &source,
        r#"#Policy(gc)
#[!Equatable, !Comparable]
enum Link {
    End(Int)
    Next(Link)
}

fn promoted_cycle() => Link {
    first := Link.Next(Link.End(1))
    second := Link.Next(first)
    first = Link.Next(second)
    return first
}

fn run() {
    cycle :: promoted_cycle()
    print(cycle)
}
"#,
    )
    .unwrap();
    let source = source.to_str().unwrap();

    let first = run(&root, &["run", source]);
    assert!(
        first.status.success(),
        "first GC run failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        "Next(Next(Next(End(1))))\n"
    );
    let ledger_path = root.join(".jet/memory/ledger-v1.jsonl");
    let first_ledger = std::fs::read_to_string(&ledger_path).unwrap();
    let first_rows = first_ledger.lines().count();
    assert!(first_rows > 0, "the exercised GC run wrote no witnesses");
    assert!(first_ledger.lines().all(|row| {
        row.contains("\"kind\":\"gc\"")
            && row.contains("\"provenance\":\"")
    }));

    let second = run(&root, &["run", source]);
    assert!(
        second.status.success(),
        "second GC run failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&second.stdout),
        "Next(Next(Next(End(1))))\n"
    );
    let persisted = std::fs::read_to_string(&ledger_path).unwrap();
    assert_eq!(persisted.lines().count(), first_rows * 2);

    let audit = run(&root, &["audit", "memory", "--json"]);
    assert!(
        audit.status.success(),
        "memory audit failed: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit = String::from_utf8_lossy(&audit.stdout);
    assert!(audit.contains("\"coverage\":\"exercised runs only\""));
    assert!(audit.contains(&format!("\"witnesses\":{}", first_rows * 2)));
    let _ = std::fs::remove_dir_all(root);
}
