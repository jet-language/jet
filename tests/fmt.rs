//! M6 phase 1: `jet fmt` idempotence — fmt(fmt(x)) == fmt(x).

mod common;

use std::fs;

#[test]
fn package_transition_surface_formats_canonically_and_idempotently() {
    let sources = [
        "name: \"demo\"\noutputs: .{ app: .Executable.{ entry: run } }\n",
        "pub development :: Config.{ environments: .{ development: .Environment.{ tools: [\"git\"] } } }\n",
        "pub app :: Config.{ version: \"1\" }\n",
    ];

    for source in sources {
        let once = jet::Package::format_source(source, "package.jet")
            .expect("canonical package source should format");
        let twice = jet::Package::format_source(&once, "package.jet")
            .expect("formatted package source should reformat");
        assert_eq!(once, twice, "canonical package formatting must be stable");
        assert!(
            once.contains("Executable") || once.contains("Config"),
            "formatter dropped package contribution:\n{once}"
        );
    }

    let formatted = jet::Package::format_source("name : \"demo\"\n", "package.jet")
        .expect("package field spacing should format");
    assert!(formatted.contains("name: \"demo\""), "{formatted}");
    assert!(!formatted.contains("name :"), "{formatted}");
}

#[test]
fn type_alias_binding_sigils_are_canonical_and_idempotent() {
    let old = "alias Result<T> = T ? Int\nfn run() {}\n";
    let once = jet::format_source(old).expect("retired alias spelling should format");
    assert!(once.contains("alias Result<T> :: T ? Int"), "{once}");
    assert!(!once.contains("alias Result<T> ="), "{once}");
    let twice = jet::format_source(&once).expect("canonical alias spelling should reformat");
    assert_eq!(once, twice, "alias formatting must be idempotent");
}

#[test]
fn fmt_preserves_script_and_declaration_source_order() {
    let source = "message :: \"script entry\"\nprint(message)\n\nfn helper() => Int {\n    return 42\n}\n";
    let once = jet::format_source(source).expect("mixed script source should format");
    let message = once.find("message ::").expect("script binding should remain");
    let helper = once.find("fn helper").expect("declaration should remain");
    assert!(message < helper, "formatter reordered source items:\n{once}");
    let twice = jet::format_source(&once).expect("formatted source should reformat");
    assert_eq!(once, twice, "mixed script formatting must be idempotent");
}

#[test]
fn package_formatter_fails_closed_on_comments() {
    let error = jet::Package::format_source(
        "name: \"demo\" // comment ownership is not typed yet\n",
        "package.jet",
    )
    .expect_err("typed formatter must not report commented source as clean");
    assert!(error.contains("cannot safely rewrite comments"), "{error}");
}

#[test]
fn fixed_interpolation_selector_is_stable() {
    let src = "fn run(){price::1234.5\nprint(\"{price:Fixed(2)}\")}\n";
    let once = jet::format_source(src).expect("fixed interpolation should format");
    assert_eq!(
        once,
        "fn run() {\n    price :: 1234.5\n    print(\"{price:Fixed(2)}\")\n}\n"
    );
    let twice = jet::format_source(&once).expect("fixed interpolation should re-format");
    assert_eq!(twice, once);
}

#[test]
fn fmt_preserves_root_receiver_declarations() {
    let src = "fn show(#Root value: Int) { print(value) }\n";
    let once = jet::format_source(src).expect("#Root declaration should format");
    assert!(
        once.contains("fn show(#Root value: Int)"),
        "formatter dropped the #Root marker:\n{once}"
    );
    let twice = jet::format_source(&once).expect("formatted #Root declaration should re-format");
    assert_eq!(once, twice, "#Root formatting must be stable");
}

#[test]
fn fmt_canonicalizes_unit_return_types() {
    let src = "fn run() ? { return Err(\"boom\") }\n";
    let once = jet::format_source(src).expect("unit return type should format");
    assert!(once.contains("fn run() ?"), "formatter lost the unit-fallible return:\n{once}");
    let twice = jet::format_source(&once).expect("formatted unit return should re-format");
    assert_eq!(once, twice, "unit return formatting must be idempotent");
    assert!(
        jet::format_source("fn run() => Void ? { return Err(\"boom\") }\n").is_err(),
        "retired Void must not be accepted by the formatter"
    );
}

#[test]
fn fmt_parallel_collection_adapters_are_stable() {
    let src = r#"fn run() {
    values := [1, 2, 3, 4]
    doubled :: values.para_map((n: Int) => n * 2)
    even :: values.para_filter((n: Int) => n % 2 == 0)
    split :: values.para_partition((n: Int) => n % 2 == 0)
    total :: values.para_fold(() => 0, (acc: Int, n: Int) => acc + n, (left: Int, right: Int) => left + right)
}
"#;
    let once = jet::format_source(src).expect("parallel collection adapters should format");
    for spelling in ["para_map", "para_filter", "para_partition", "para_fold"] {
        assert!(once.contains(spelling), "formatter dropped `{spelling}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("formatted parallel adapters should parse");
    assert_eq!(once, twice, "parallel adapter formatting must be stable");
}

#[test]
fn repr_c_enum_surface_is_stable() {
    let src = "#Layout(c, tag: U8)\nenum Packet { Ping(Int) = 3; Data(x: I32, y: I32) = 7 }\n";
    let once = jet::format_source(src).expect("C-layout enum should format");
    assert!(once.contains("#Layout(c, tag: U8)"));
    assert!(once.contains("Ping(Int) = 3"));
    assert!(once.contains("Data(x: I32, y: I32) = 7"));
    let twice = jet::format_source(&once).expect("formatted C-layout enum should parse");
    assert_eq!(once, twice, "C-layout enum formatting must be stable");
}

#[test]
fn computed_declaration_values_format_stably() {
    let src = "@lanes :: 2\n@base :: 40\nstruct Frame { values: [Int#(@lanes * 2)] }\n#Layout(c, tag: U8) enum Code { First = @base + 1 Second }\n";
    let once = jet::format_source(src).expect("computed declaration values should format");
    assert!(once.contains("[Int#(@lanes * 2)]"), "fixed-list expression was lost:\n{once}");
    assert!(once.contains("First = @base + 1"), "enum expression was lost:\n{once}");
    let twice = jet::format_source(&once).expect("formatted computed values should re-format");
    assert_eq!(once, twice, "computed declaration formatting must be idempotent");
}

#[test]
fn multi_head_function_surface_round_trips() {
    let src = "enum Shape { Circle(Float) Rect(w: Float, h: Float) }\n\nfn area(Circle(r: Float)) => Float = r * r\nfn area(Rect(w: Float, h: Float)) => Float = w * h\n";
    let once = jet::format_source(src).expect("multi-head functions should format");
    assert!(once.contains("fn area(Circle(r: Float)) => Float = r * r"));
    assert!(once.contains("fn area(Rect(w: Float, h: Float)) => Float = w * h"));
    let twice = jet::format_source(&once).expect("formatted multi-head functions should parse");
    assert_eq!(once, twice, "multi-head formatting must be stable");
}

#[test]
fn alternatives_only_bar_formatting_is_stable() {
    let src = "enum State { Ready Waiting }\n\nfn run() {\n    state :: State.Ready\n    if state == {\n        .Ready | .Waiting -> print(\"known\")\n    }\n}\n";
    let once = jet::format_source(src).expect("choice alternatives should format");
    assert!(
        once.contains(".Ready | .Waiting ->"),
        "formatter lost the alternatives-only bar:\n{once}"
    );
    let twice = jet::format_source(&once).expect("formatted alternatives should parse");
    assert_eq!(once, twice, "alternative formatting must be stable");

    assert!(
        jet::format_source("fn run() { value :: 1 | 2 }").is_err(),
        "formatter must not preserve a general single-bar expression"
    );
    assert!(
        jet::format_source("fn run() { value :: 1 |> print }").is_err(),
        "formatter must not invent a flow path"
    );
}
use std::path::PathBuf;

#[path = "support/fmt_lossless.rs"]
mod fmt_lossless;

#[test]
fn fmt_rejects_retired_annotated_uninit_binding() {
    let src = include_str!("ui/uninit_annotated_retired.jet");
    let parsed = jet::Compiler::parse_source(src);
    assert!(
        parsed.diagnostics.iter().any(|diagnostic| diagnostic.code == "E0003"),
        "retired annotated binding must remain an ordinary parse error: {:?}",
        parsed.diagnostics
    );
    assert!(
        jet::format_source(src).is_err(),
        "formatter must reject the parser-invalid retired annotated binding"
    );
}

fn collect_jet_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some(jet::Syntax::FILE_EXT) {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn fmt_is_idempotent_on_examples() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for path in collect_jet_files(&root.join("examples")) {
        let src = fs::read_to_string(&path).unwrap();
        let once = jet::format_source(&src).unwrap_or_else(|d| {
            panic!(
                "fmt failed on {}:\n{}",
                path.display(),
                jet::render_diagnostics(&path.display().to_string(), &src, &d)
            )
        });
        let twice = jet::format_source(&once).expect("second fmt should succeed");
        assert_eq!(once, twice, "fmt is not idempotent on {}", path.display());
    }
}

#[test]
fn fmt_preserves_binary_pattern_holes() {
    // D-BINPAT1 / D-UNIFYLIT1=A: `[U8].{"…"}` must survive fmt with every hole intact.
    let src = "fn run() {\n    packet :: [0x45]\n    if packet == {\n        [U8].{\"{version:U4}{ihl:U4}{tos:U8}{len:U16be}{rest:...}\"} -> { print(\"ok\") }\n        else -> { print(\"no\") }\n    }\n}\n";
    let once = jet::format_source(src).expect("binary pattern should format");
    assert!(
        once.contains("[U8].{\"{version:U4}{ihl:U4}{tos:U8}{len:U16be}{rest:...}\"}"),
        "formatter dropped or garbled the binary pattern:\n{once}"
    );
    let twice = jet::format_source(&once).expect("formatted binary pattern should parse");
    assert_eq!(once, twice, "binary pattern formatting must be stable");
}

#[test]
fn fmt_preserves_typed_performance_budget_role() {
    let src = r#"module perf.release{budgets:[Budget.{name:"binary",scope:.Target("cli"),metric:.BinarySize,limit:.AtMost(2MiB)}]}"#;
    let once = jet::format_source(src).expect("perf budget role should format");
    for token in [
        "module perf.release",
        "budgets:",
        "Budget.{",
        ".Target(\"cli\")",
        ".BinarySize",
        ".AtMost(2MiB)",
    ] {
        assert!(once.contains(token), "formatter dropped `{token}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("formatted perf role should parse");
    assert_eq!(once, twice, "perf budget formatting must be stable");
}

#[test]
fn fmt_preserves_rate_count_literal_spelling() {
    let src = "fn run() {\n    rate :: Rate.{ count: 000_100, per: 2s }\n}\n";
    let once = jet::format_source(src).expect("Rate count spelling should format");
    assert!(once.contains("count: 000_100"), "formatter rewrote Rate count:\n{once}");
    let twice = jet::format_source(&once).expect("formatted Rate count should parse");
    assert_eq!(once, twice, "Rate count formatting must be stable");
}

#[test]
fn fmt_preserves_s61_call_labels() {
    // S61: call-site argument labels (`name:`) must survive fmt — previously
    // `fmt_call_args` dropped them, so `area(width: 3, height: 4)` round-tripped
    // to `area(3, 4)`, silently losing the labels.
    let src = "fn area(width: Int, height: Int) => Int {\n    return width * height\n}\n\nfn run() {\n    print(area(width: 3, height: 4))\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on labeled calls");
    assert!(
        out.contains("width: 3") && out.contains("height: 4"),
        "fmt must preserve S61 call labels, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("labeled-call fmt should re-fmt");
    assert_eq!(out, twice, "labeled-call fmt must be idempotent");
}

#[test]
fn fmt_off_debug_only_statement_attributes_stability() {
    let src = r#"fn run() {
    #Off print("off")
    #DebugOnly print("debug")
    #Off if true {
        print("off block")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept statement switch attributes");
    for needle in [
        "#Off print(\"off\")",
        "#DebugOnly print(\"debug\")",
        "#Off if true {",
        "print(\"off block\")",
    ] {
        assert!(out.contains(needle), "fmt dropped `{needle}`, got:\n{out}");
    }
    let twice = jet::format_source(&out).expect("statement switch attributes should re-fmt");
    assert_eq!(
        out, twice,
        "statement switch attribute formatting must be stable"
    );
}

#[test]
fn fmt_meta_attribute_stability() {
    let src = r#"#Meta(category: "Movement", tunable)
fn step_speed(speed: Int) => Int {
    #Meta(category: "Movement", tunable)
    next :: speed + 1
    return next
}
"#;
    let out = jet::format_source(src).expect("fmt should accept #Meta attributes");
    for needle in [
        "#Meta(category: \"Movement\", tunable)",
        "fn step_speed(speed: Int) => Int {",
        "next :: speed + 1",
    ] {
        assert!(out.contains(needle), "fmt dropped `{needle}`, got:\n{out}");
    }
    let twice = jet::format_source(&out).expect("#Meta output should re-fmt");
    assert_eq!(out, twice, "#Meta formatting must be stable");
}

#[test]
fn fmt_preserves_block_comments() {
    // S5: `/* … */` block comments, nesting allowed.
    let src = r#"/* a leading block comment */
fn run() {
    /* explains the next line, /* with a nested comment */ inside */
    x :: 5
    print("{x}")
}
"#;
    let out = jet::format_source(src).expect("fmt should keep block comments");
    assert!(
        out.contains("/* a leading block comment */"),
        "leading block comment dropped:\n{out}"
    );
    assert!(
        out.contains("/* with a nested comment */ inside */"),
        "nested block comment dropped:\n{out}"
    );
    let twice = jet::format_source(&out).expect("formatted output should re-fmt");
    assert_eq!(out, twice, "block-comment fmt must be idempotent");
}

#[test]
fn fmt_keeps_call_comment_outside_lambda_block() {
    let src = r#"fn transfer(from: Shared<Account>, amount: Int) {
    #Transact(tx) {
        from.edit(a => { a.balance -= amount })  // both land, or neither
    }
}
"#;
    let out = jet::format_source(src).expect("lambda call with trailing comment should format");
    let lambda_close = out
        .find("})")
        .expect("formatted call should close the lambda and call before its comment");
    let comment = out
        .find("// both land, or neither")
        .expect("formatter should preserve the trailing comment");
    assert!(
        lambda_close < comment,
        "trailing call comment moved inside its lambda block:\n{out}"
    );
    let twice = jet::format_source(&out).expect("formatted lambda call should re-format");
    assert_eq!(out, twice, "lambda-call comment formatting must be stable");
}

#[test]
fn fmt_keeps_optional_return_sugar() {
    // D-RESULT-OPTION-CANON1: bare `T?` is Optional; fallible is spaced `T ?`.
    let src = r#"fn parse_count(raw: String) => Int? {
    return Err("empty");
}
"#;
    let out = jet::format_source(src).expect("fmt should parse optional return");
    assert!(
        out.contains("fn parse_count(raw: String) => Int? {"),
        "expected `Int?` optional return to stay `Int?`, got:\n{out}"
    );
    let fallible = r#"fn parse_count(raw: String) => Int ? {
    return Err("empty");
}
"#;
    let fallible_out =
        jet::format_source(fallible).expect("fmt should parse fallible return");
    assert!(
        fallible_out.contains("fn parse_count(raw: String) => Int ? {"),
        "expected spaced `Int ?` fallible return to stay spaced, got:\n{fallible_out}"
    );
}

#[test]
fn fmt_comptime_os_dispatch_round_trips() {
    // D-OSTARGET2=B (ratified 2026-07-03): `@if build.os == { … }` — the
    // OS-target dispatch. New token shape: the `@if <subject> == { }`
    // dispatch. Must survive fmt (subject + arms + bodies preserved) and be
    // idempotent (the formatter-round-trip-required rule catches dropped tokens).
    let src = r#"fn run() {
    @if build.os == {
        .Linux -> {
            b :: LinuxBackend.{ name: "gtk" }
            print(b.label())
        }
        .MacOS -> print("mac")
        else -> print("other")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept a comptime OS dispatch");
    assert!(
        out.contains("@if build.os == {"),
        "expected the `@if build.os == {{` dispatch head, got:\n{out}"
    );
    // Arms and their bodies survive. Braceless simple arms stay concise; the
    // author-written scoped arm stays braced.
    assert!(
        out.contains(".Linux -> {") && out.contains(".MacOS -> print(\"mac\")"),
        "expected OS arms preserved, got:\n{out}"
    );
    assert!(
        out.contains("print(\"mac\")") && out.contains("print(b.label())"),
        "expected arm bodies preserved, got:\n{out}"
    );
    assert!(
        out.contains("else -> print(\"other\")"),
        "expected the else arm preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("comptime OS dispatch output should re-fmt");
    assert_eq!(
        out, twice,
        "comptime OS dispatch formatting must be idempotent"
    );
}

#[test]
fn fmt_preserves_concise_dispatch_arms() {
    // D-IF1/D-FMT1: simple braceless arms remain one line. Explicit braces
    // still define visible scope and remain exactly as written.
    let src = r#"fn run() {
    value :: 2
    if value == {
        1 -> print("one")
        2 -> { print("two") }
        else -> print("other")
    }
}
"#;
    assert_fmt_stable(src, "concise dispatch arms");
}

#[test]
fn fmt_keeps_empty_dispatch_arms_compact_and_stable() {
    let src = r#"fn run() {
    if "ready" == {
        "ready" -> print("ready")
        else -> {}
    }
    print("done")
}
"#;
    let out = jet::format_source(src).expect("empty dispatch arm should format");
    assert!(
        out.contains("else -> {}"),
        "empty dispatch arm should stay compact, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("empty dispatch arm should reformat");
    assert_eq!(out, twice, "empty dispatch arm must be stable");
}

#[test]
fn fmt_preserves_predicate_arms_under_eq_table() {
    // D-IFDIST1: only the table marker is stripped. A predicate like
    // `code >= 500` under `if code == { … }` must round-trip verbatim.
    let src = r#"fn handle(code: Int) {
    if code == {
        200 -> print("ok")
        code >= 500 -> print("retry")
        else -> print("other")
    }
}

fn run() {
    handle(503)
}
"#;
    let out = jet::format_source(src).expect("predicate arm dispatch should format");
    assert!(
        out.contains("if code =="),
        "table marker lost or rewritten:\n{out}"
    );
    assert!(
        out.contains("code >= 500"),
        "predicate arm stripped to a bare atom:\n{out}"
    );
    assert_fmt_stable(&out, "predicate arms under == table");
}

#[test]
fn fmt_keeps_trailing_dispatch_arm_comments_attached() {
    let src = r#"fn run() {
    value :: 1
    if value == {
        1 -> print("one") // The common case.
        else -> print("other")
    }
}
"#;
    let once = jet::format_source(src).expect("commented dispatch arm should format");
    assert!(
        once.contains("print(\"one\")  // The common case."),
        "formatter moved or dropped the arm comment:\n{once}"
    );
    let twice = jet::format_source(&once).expect("commented dispatch arm should reformat");
    assert_eq!(once, twice, "commented dispatch arm must be idempotent");
}

#[test]
fn fmt_expands_multiline_lambda_dispatch_arms_without_losing_comments() {
    let src = r#"fn run() {
    value :: 1
    if value == {
        1 -> apply(x => {
            // Keep this explanation with the lambda.
            return x + 1
        })
        else -> print("other")
    }
}
"#;
    let once = jet::format_source(src).expect("multiline lambda arm should format");
    assert!(
        once.contains("// Keep this explanation with the lambda."),
        "formatter dropped the lambda comment:\n{once}"
    );
    let twice = jet::format_source(&once).expect("multiline lambda arm should reformat");
    assert_eq!(once, twice, "multiline lambda arm must be idempotent");
}

#[test]
fn fmt_preserves_multiline_collection_literals() {
    // S44 author intent applies to collection layout too: readable vertical
    // data must not collapse into one over-width line.
    let src = r#"fn run() {
    values :: [
        1,
        2,
        3
    ]
    typed :: [Int].{
        4,
        5,
        6
    }
    lookup :: [
        "a": 1,
        "b": 2
    ]
    point :: (
        x: 7,
        y: 8
    )
    record :: Point.{
        x: 9,
        y: 10
    }
    print(values.len() + typed.len() + lookup.len() + point.x + point.y + record.x)
}
"#;
    assert_fmt_stable(src, "multiline collection literals");
}

#[test]
fn fmt_preserves_comments_inside_multiline_collection_literals() {
    let src = r#"fn run() {
    values :: [
        // Primary value.
        1,
        2  // Secondary value.
    ]
    point :: (
        // Horizontal coordinate.
        x: /* coordinate value */ 7,
        y: 8
    )
    lookup :: [
        // Stable key.
        "a": /* mapped value */ 1,
        "b": 2
    ]
    typed :: [Int].{
        // First typed value.
        3,
        4
    }
    record :: Point.{
        // Named field.
        x: /* field value */ 9,
        y: 10
    }
    typed_lookup :: [String: Int].{
        "c": /* typed mapped value */ 5,
        "d": 6
    }
    empty :: [
        // Intentionally empty.
    ]
    typed_empty :: [Int].{
        /* Typed sentinel. */
    }
    print(values.len() + point.x + lookup.len() + typed.len())
}
"#;
    let once = jet::format_source(src).expect("commented collection literals should format");
    for comment in [
        "// Primary value.",
        "// Secondary value.",
        "// Horizontal coordinate.",
        "/* coordinate value */",
        "// Stable key.",
        "/* mapped value */",
        "// First typed value.",
        "// Named field.",
        "/* field value */",
        "/* typed mapped value */",
        "// Intentionally empty.",
        "/* Typed sentinel. */",
    ] {
        assert!(once.contains(comment), "formatter dropped `{comment}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("commented collection literals should reformat");
    assert_eq!(once, twice, "commented collection literals must be idempotent");
}

#[test]
fn fmt_preserves_blank_lines_between_statement_sections() {
    let src = r#"fn run() {
    first()

    // Second phase.
    second()

    third()
}
"#;
    assert_fmt_stable(src, "statement section breaks");
}

#[test]
fn fmt_keeps_section_break_after_leading_comment() {
    let src = r#"fn run() {
    first()
    // First phase is complete.

    second()
}
"#;
    assert_fmt_stable(src, "section break after a leading comment");
}

#[test]
fn fmt_keeps_section_break_between_leading_comment_groups() {
    let src = r#"fn run() {
    first()
    // First phase is complete.

    // Second phase starts here.
    second()
}
"#;
    assert_fmt_stable(src, "section break between leading comment groups");
}

#[test]
fn fmt_if_expression_preserves_condition_parens() {
    // S68 (D-SG2): `if` as a value round-trips.
    // D-FMTPARENS1=A: author-written grouping parens are always preserved.
    let src = r#"fn run() {
    m :: if (a > b) -> {
        a
    } else -> {
        b
    }
    if (a > b) {
        print("hi")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept an if-expression");
    assert!(
        out.contains("m :: if (a > b) -> {"),
        "expected parens preserved in if-expression condition, got:\n{out}"
    );
    assert!(
        out.contains("    if (a > b) {"),
        "expected parens preserved in statement-if condition, got:\n{out}"
    );
    assert!(
        out.contains("} else -> {"),
        "expected else branch, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("if-expression output should re-fmt");
    assert_eq!(out, twice, "if-expression formatting must be idempotent");
}

#[test]
fn fmt_preserves_author_placed_chain_breaks() {
    // S69 (D-SG3): a broken dot-chain keeps one step per line, each step may
    // carry its own trailing comment, and the final step's comment stays after
    // the statement terminator.
    let src = r#"fn run() {
    raw :: "  hi  "
    out :: raw
        .trim()  // strip padding
        .to_upper()  // shout it
    print("{out}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept a broken dot-chain");
    assert!(
        out.contains(".trim()  // strip padding"),
        "expected the break and per-step comment to survive, got:\n{out}"
    );
    assert!(
        out.contains(".to_upper()  // shout it"),
        "expected the final step's comment to stay after the chain, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("broken-chain output should re-fmt");
    assert_eq!(out, twice, "chain-break formatting must be idempotent");
}

#[test]
fn fmt_preserves_triple_quoted_strings() {
    // S70 (D-SG5): a `"""…"""` string keeps its multi-line shape, relative
    // indentation, and interpolation across fmt.
    let src = r#"fn run() {
    who :: "Jet"
    banner :: """
    hello, {who}
        indented
    """
    print("{banner}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept a triple-quoted string");
    assert!(
        out.contains("banner :: \"\"\"\n"),
        "expected the opening `\"\"\"` to stay on its own, got:\n{out}"
    );
    assert!(
        out.contains("    hello, {who}\n"),
        "expected interpolation and indentation preserved, got:\n{out}"
    );
    assert!(
        out.contains("        indented\n"),
        "expected relative indentation preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("triple-quoted output should re-fmt");
    assert_eq!(out, twice, "triple-quoted formatting must be idempotent");
}

#[test]
fn fmt_preserves_destructuring_targets() {
    // S74: destructuring of a struct and a list round-trips.
    // D-DOTCTOR1: the canonical destructuring form is `Point.{x, y}`; old dotless
    // form (E0320 recovery) is auto-fixed to the new form by fmt.
    let src = r#"struct Point { x: Int, y: Int }

fn run() {
    Point.{ x, y } :: make()
    [a, b, c] := nums()
    print("{x}{y}{a}{b}{c}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept destructuring targets");
    assert!(
        out.contains("Point.{x, y} :: make()"),
        "expected dot struct destructuring (D-DOTCTOR1), got:\n{out}"
    );
    assert!(
        out.contains("[a, b, c] := nums()"),
        "expected list destructuring preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("destructuring output should re-fmt");
    assert_eq!(out, twice, "destructuring formatting must be idempotent");
}

#[test]
fn fmt_flush_construction() {
    // D-DOTCTOR1: the canonical form is `Point.{x: 1, y: 2}` (dot before brace).
    // Old dotless input (E0320 recovery) is auto-fixed by fmt.
    let src = r#"struct Point { x: Int, y: Int }

fn run() {
    p :: Point.{ x: 1, y: 2 }
    print("{p.x}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept struct construction");
    assert!(
        out.contains("Point.{x: 1, y: 2}"),
        "expected dot construction (D-DOTCTOR1), got:\n{out}"
    );
    assert!(
        !out.contains("Point {x: 1"),
        "construction must not keep the space before the brace, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("construction output should re-fmt");
    assert_eq!(out, twice, "construction formatting must be idempotent");
}

#[test]
fn fmt_preserves_named_tuples() {
    // S73 (D-SG7): named tuple literals, types, access, and destructuring round-trip.
    let src = r#"fn bounds() => (min: Int, max: Int) {
    return (min: 0, max: 10)
}

fn run() {
    p :: (x: 1, y: 2)
    (a, b) :: p
    print("{p.x}{a}{b}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept named tuples");
    assert!(
        out.contains("p :: (x: 1, y: 2)"),
        "expected named tuple literal preserved, got:\n{out}"
    );
    assert!(
        out.contains("=> (max: Int, min: Int)"),
        "expected canonical named tuple return type preserved, got:\n{out}"
    );
    assert!(
        out.contains("(a, b) :: p"),
        "expected tuple destructuring preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("named tuple output should re-fmt");
    assert_eq!(out, twice, "named tuple formatting must be idempotent");
}

#[test]
fn fmt_preserves_optional_chaining() {
    // S71 (D-SG6): `?.` chains round-trip unchanged.
    let src = r#"fn run() {
    n :: o.mid?.inner?.name
    print("{n}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept optional chaining");
    assert!(
        out.contains("o.mid?.inner?.name"),
        "expected `?.` chain preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("optional-chain output should re-fmt");
    assert_eq!(out, twice, "optional-chain formatting must be idempotent");
}

#[test]
fn fmt_canonicalizes_collection_type_sugar() {
    let src = r#"fn shell() => [JSON] {
    return [
        JSON.Null;
    ];
}

fn use_collections(items: [String], counts: [String: Int]) {}
"#;
    let out = jet::format_source(src).expect("fmt should accept collection type sugar");
    assert!(
        out.contains("fn shell() => [JSON]"),
        "expected list return shorthand, got:\n{out}"
    );
    assert!(
        out.contains("items: [String], counts: [String: Int]"),
        "expected bracket collection type formatting, got:\n{out}"
    );
    assert!(
        out.contains("JSON.Null") && out.contains("return ["),
        "expected semicolon-separated list input to format cleanly, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("collection shorthand output should re-fmt");
    assert_eq!(
        out, twice,
        "collection shorthand formatting must be idempotent"
    );
}

#[test]
fn fmt_still_errors_on_real_parse_problems() {
    let src = "fn run() { x :: ; }\n";
    assert!(
        jet::format_source(src).is_err(),
        "fmt must not run when the AST is not recoverable"
    );
}

#[test]
fn fmt_is_idempotent_on_ui_fixes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui = root.join("tests/ui");
    for entry in fs::read_dir(&ui).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".fixed.jet"))
        {
            let src = fs::read_to_string(&path).unwrap();
            let once = jet::format_source(&src).unwrap_or_else(|d| {
                panic!(
                    "fmt failed on {}:\n{}",
                    path.display(),
                    jet::render_diagnostics(&path.display().to_string(), &src, &d)
                )
            });
            let twice = jet::format_source(&once).expect("second fmt should succeed");
            assert_eq!(once, twice, "fmt is not idempotent on {}", path.display());
        }
    }
}

// --- D-ENUMDOT2=A: leading-dot enum literal stability ---

#[test]
fn fmt_preserves_leading_dot_enum_lit() {
    // D-ENUMDOT2=A: `.Variant` (type_name="") round-trips unchanged through fmt.
    // The formatter emits `"" + "." + variant` = `.Variant`.
    let src = r#"enum Color {
    Red
    Blue
}

fn paint(c: Color) {
    print(c)
}
fn run() {
    c := .Red
    paint(.Blue)
    paint(c)
}
"#;
    let out = jet::format_source(src).expect("fmt should accept leading-dot enum literals");
    assert!(
        out.contains(".Red"),
        "expected `.Red` preserved, got:\n{out}"
    );
    assert!(
        out.contains(".Blue"),
        "expected `.Blue` preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("leading-dot output should re-fmt");
    assert_eq!(
        out, twice,
        "leading-dot enum literal formatting must be idempotent"
    );
}

#[test]
fn fmt_preserves_optional_result_variants() {
    let src = r#"fn f(flag: Bool) => Int ? String {
    maybe :: .Val(1)
    empty :: .None
    if maybe == {
        .Val(n) -> { print(n) }
        .None -> { print(0) }
    }
    if flag { return .Ok(1) }
    return .Err("no")
}
"#;
    let once = jet::format_source(src).expect("variant forms should format");
    for spelling in [".Val(1)", ".None", ".Val(n)", ".Ok(1)", ".Err(\"no\")"] {
        assert!(once.contains(spelling), "formatter dropped `{spelling}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("variant output should re-format");
    assert_eq!(once, twice, "variant formatting must be idempotent");
}

#[test]
fn fmt_canonicalizes_anonymous_union_types() {
    // D-UNIONTYPE1=A: order-insensitive identity; formatter prints canonical member order.
    let src = r#"fn hold(v: String | Int) => Int | String {
    return v
}
fn parse(raw: String) => Int ? String | Bool {
    return .Err(false)
}
"#;
    let once = jet::format_source(src).expect("anonymous unions should format");
    assert!(
        once.contains("Int | String"),
        "expected canonical `Int | String` spelling, got:\n{once}"
    );
    assert!(
        once.contains("Int ? Bool | String") || once.contains("Int ? String | Bool"),
        "expected fallible error-side union, got:\n{once}"
    );
    let twice = jet::format_source(&once).expect("union fmt must be idempotent");
    assert_eq!(once, twice, "anonymous union formatting must be idempotent");
}

#[test]
fn fmt_preserves_ambiguous_codable_union_source_order() {
    for src in [
        include_str!("ui/union_codable_ambiguous.jet"),
        include_str!("ui/union_codable_enum_ambiguous.jet"),
    ] {
        let once = jet::format_source(src).expect("ambiguous Codable union should format");
        let twice =
            jet::format_source(&once).expect("ambiguous Codable union output should re-format");
        assert_eq!(
            once, twice,
            "ambiguous Codable union formatting must be idempotent"
        );
    }
}

#[test]
fn fmt_preserves_comments_in_ambiguous_decode_unions() {
    for (src, label) in [
        (
            "#Decode\nstruct Row {\n    value: String /* Text */ | Char\n}\n",
            "block comment inside ambiguous Decode union",
        ),
        (
            "#Decode\nstruct Row {\n    value: String | Char // note\n}\n",
            "line comment after ambiguous Decode union",
        ),
    ] {
        assert_fmt_stable(src, label);
    }
}

#[test]
fn fmt_still_canonicalizes_valid_codable_union() {
    let src = "#Codable\nstruct Row {\n    value: String | Int\n}\n";
    let once = jet::format_source(src).expect("valid Codable union should format");
    assert!(
        once.contains("value: Int | String"),
        "valid Codable union should retain canonical member order:\n{once}"
    );
    let twice = jet::format_source(&once).expect("valid Codable union output should re-format");
    assert_eq!(
        once, twice,
        "valid Codable union formatting must be idempotent"
    );
}

#[test]
fn fmt_canonicalizes_skipped_decode_union() {
    let src = "#Decode\nstruct Row {\n    #Skip value: String | Char\n}\n";
    let once = jet::format_source(src).expect("skipped Decode union should format");
    assert!(
        once.contains("#Skip value: Char | String"),
        "skipped Decode field should retain canonical union order:\n{once}"
    );
    let twice = jet::format_source(&once).expect("skipped Decode union output should re-format");
    assert_eq!(
        once, twice,
        "skipped Decode union formatting must be idempotent"
    );
}

#[test]
fn fmt_preserves_named_payload_variant_dot_brace() {
    // D-UITREE1/D-DOTCTOR1: `.Variant.{ field: val }` — named-payload enum
    // construction reuses the struct dot-brace spelling. Must round-trip
    // byte-for-byte (fmt STABILITY, not just accept-without-crash).
    let src = r#"enum View {
    Text(text: String)
    Box(width: Int)
}
fn run() {
    a :: .Text.{ text: "hi" }
    b :: .Box.{ width: 10 }
}
"#;
    let out =
        jet::format_source(src).expect("fmt should accept named-payload dot-brace enum literals");
    assert!(
        out.contains(".Text.{"),
        "expected `.Text.{{` preserved, got:\n{out}"
    );
    assert!(
        out.contains(".Box.{"),
        "expected `.Box.{{` preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("named-payload dot-brace output should re-fmt");
    assert_eq!(
        out, twice,
        "named-payload dot-brace enum literal formatting must be idempotent"
    );
}

#[test]
fn fmt_preserves_take_pattern_literal() {
    // D-SHIFT1 (c7shift): `cursor.take_pattern("…{hole:Type}…")` — the
    // pattern-literal argument (typed holes, D-PARSESTR1 grammar) must
    // round-trip byte-for-byte (fmt STABILITY, not just accept-without-crash).
    let src = r#"fn run() {
    c :: Cursor.over("inc-4411 sev 3: disk full")
    c.skip_ws()
    m :: c.take_pattern("inc-{id:Int} sev {sev:Int}: ") ?? panic("no match")
    rest :: c.take_pattern("disk ") ?? panic("no match")
    print("{m.id} {m.sev}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept take_pattern literals");
    assert_eq!(out, src, "take_pattern literal formatting must be stable");
    let twice = jet::format_source(&out).expect("take_pattern output should re-fmt");
    assert_eq!(out, twice, "take_pattern formatting must be idempotent");
}

#[test]
fn fmt_preserves_bin_take_pattern_literal() {
    // D-BINPAT1 / D-UNIFYLIT1=A: `reader.take_pattern([U8].{"…"})`.
    let src = r#"fn run() {
    header :: [69, 0, 0, 40]
    r :: Reader.over(header)
    parsed :: r.take_pattern([U8].{"{version:U4}{ihl:U4}{tos:U8}{len:U16be}"}) ?? panic("no match")
    print("{parsed.version} {parsed.ihl} {parsed.tos} {parsed.len}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept binary take_pattern literals");
    assert_eq!(out, src, "binary take_pattern literal formatting must be stable");
    let twice = jet::format_source(&out).expect("binary take_pattern output should re-fmt");
    assert_eq!(out, twice, "binary take_pattern formatting must be idempotent");
}

// --- D-FMT1 (revises S44): author-intent single-line brace bodies ---
//
// A braced control body with one simple statement and no inner comment
// collapses when it fits width 100. Wider, nested, or commented bodies expand.

/// Assert formatting is idempotent (D-FMTCOLLAPSE1 may rewrite fitting braces).
fn assert_fmt_stable(src: &str, label: &str) {
    let out = jet::format_source(src).unwrap_or_else(|d| {
        panic!(
            "fmt failed for {label}:\n{}",
            jet::render_diagnostics(label, src, &d)
        )
    });
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "{label}: fmt is not idempotent\n--- once ---\n{out}");
}

#[test]
fn fmt_concurrency_spellings_that_parse_today() {
    // D-CONC-JOIN1=A / D-CONC-CHAN1=A / D-CONC-FAIL1=A: these spellings
    // already use parser shapes owned by existing generic/type/call/loop
    // grammar. Future task/shared/select forms stay on their implementation
    // cards and do not get parser stubs here.
    let src = r#"fn inspect(handle: Task<Int>, group: TaskGroup, rx: Receiver<Int>, tx: Sender<Int>) => TaskFailure {
    joined :: handle.join()
    cancelled :: .Cancelled
    deadline :: .DeadlineBlown
    loop value, rx {
        tx.send(value)
    }
    return .Panicked("boom")
}

fn open() {
    pair :: channel<Int>(capacity: 8)
}
"#;
    let once = jet::format_source(src).expect("parseable concurrency spellings should format");
    for spelling in [
        "Task<Int>",
        "TaskGroup",
        "Receiver<Int>",
        "Sender<Int>",
        "TaskFailure",
        "handle.join()",
        ".Cancelled",
        ".DeadlineBlown",
        "loop value, rx",
        ".Panicked(\"boom\")",
        "channel<Int>(capacity: 8)",
    ] {
        assert!(once.contains(spelling), "fmt dropped {spelling:?}:\n{once}");
    }
    assert_eq!(
        once,
        jet::format_source(&once).expect("concurrency spellings should re-format")
    );
}

#[test]
fn fmt_shield_block_stability() {
    let src = "fn run() {\n    #Shield {\n        print(\"committed\")\n    }\n}\n";
    assert_fmt_stable(src, "#Shield block");
}

#[test]
fn fmt_preserves_single_line_if() {
    // A one-line `if` body the author placed inline survives unchanged.
    let src = "fn run() {\n    ready :: true\n    if ready { launch() }\n}\n";
    assert_fmt_stable(src, "single-line if");
}

#[test]
fn fmt_preserves_multiline_if() {
    let src = "fn run() {\n    ready :: true\n    if ready {\n        launch()\n    }\n}\n";
    let expected = "fn run() {\n    ready :: true\n    if ready { launch() }\n}\n";
    let out = jet::format_source(src).expect("multiline if should format");
    assert_eq!(out, expected);
    assert_eq!(out, jet::format_source(&out).expect("collapsed if should reformat"));
}

#[test]
fn fmt_classic_if_ignores_braces_inside_leading_trivia() {
    let src =
        "fn run() {\n    ready :: true\n    if /* { trivia */ ready { launch() }\n}\n";
    let out = jet::format_source(src).expect("commented classic if should format");
    assert_eq!(out, src);
    assert_eq!(
        out,
        jet::format_source(&out).expect("commented classic if should reformat")
    );
}

#[test]
fn fmt_if_else_chain_collapses_when_every_branch_fits() {
    let src = "fn run() {\n    if a { x() } else {\n        y()\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should accept the mixed chain");
    assert_eq!(
        out, "fn run() {\n    if a { x() } else { y() }\n}\n",
        "fitting if/else branches should collapse, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("collapsed chain should re-fmt");
    assert_eq!(out, twice, "collapsed chain must be idempotent");
}

#[test]
fn fmt_single_line_comment_forces_expand() {
    // Gate (c): a comment inside the braces forces the body multiline.
    let src = "fn run() {\n    if ready { launch() /* go */ }\n}\n";
    let out = jet::format_source(src).expect("fmt should accept the commented body");
    assert!(
        out.contains("if ready {\n"),
        "inner comment should force expansion, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("expanded body should re-fmt");
    assert_eq!(out, twice, "comment-forced expansion must be idempotent");
}

#[test]
fn fmt_single_line_over_width_forces_expand() {
    // Gate (d): a one-line body whose rendered width exceeds 100 expands.
    let long = "x".repeat(120);
    let src = format!("fn run() {{\n    if ready {{ print(\"{long}\") }}\n}}\n");
    let out = jet::format_source(&src).expect("fmt should accept the wide body");
    assert!(
        out.contains("if ready {\n"),
        "over-width body should expand, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("expanded wide body should re-fmt");
    assert_eq!(out, twice, "over-width expansion must be idempotent");
}

#[test]
fn fmt_single_line_nested_forces_expand() {
    // Gate (b): the lone statement is itself a block (`if`), so it cannot stay
    // inline — the outer body expands.
    let src = "fn run() {\n    if a { if b { x() } }\n}\n";
    let out = jet::format_source(src).expect("fmt should accept the nested if");
    assert!(
        out.contains("if a {\n"),
        "outer body should expand around a nested block, got:\n{out}"
    );
    // The inner one-line `if b { x() }` is itself simple and stays inline.
    assert!(
        out.contains("if b { x() }"),
        "inner single-line if should survive, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("nested output should re-fmt");
    assert_eq!(out, twice, "nested expansion must be idempotent");
}

#[test]
fn fmt_preserves_single_line_loops_and_fn() {
    // The rule applies uniformly to `while`/`for`/`fn` bodies.
    let while_src = "fn run() {\n    n :: 0\n    loop n < 3 { n += 1 }\n}\n";
    assert_fmt_stable(while_src, "single-line while/loop");

    let for_src = "fn run() {\n    loop i, 0..3 { print(\"{i}\") }\n}\n";
    assert_fmt_stable(for_src, "single-line for/loop");
    let excl_src = "fn run() {\n    loop i, 0..<3 { print(\"{i}\") }\n}\n";
    assert_fmt_stable(excl_src, "exclusive range loop");
    let two_bind = "fn run() {\n    loop (i, x), xs { print(\"{i}\") }\n}\n";
    assert_fmt_stable(two_bind, "list two-binding loop");


    let fn_src = "fn one() => Int { return 1 }\n";
    assert_fmt_stable(fn_src, "single-line fn body");

    let empty_fn_src = "fn noop() {}\n";
    assert_fmt_stable(empty_fn_src, "empty single-line fn body");
}

#[test]
fn fmt_preserves_single_line_if_expr_branch() {
    // If-expression branches (routed through fmt_value_block) follow the rule.
    let src = "fn run() {\n    a :: true\n    n :: if a -> 1 else -> 2\n    print(\"{n}\")\n}\n";
    assert_fmt_stable(src, "single-line if-expression");
}

// ── Marker / turbofish round-trip survival (the c-fmt data-loss regression) ──
//
// `fmt_is_idempotent_on_examples` only checks that fmt(fmt(x)) == fmt(x); it does
// NOT catch a formatter that silently drops a token on the FIRST pass (the lost
// `#Rename`/`#Codable`/turbofish bug). These tests assert that the load-bearing
// surface tokens SURVIVE one fmt pass, then that the result is stable.

/// Assert every needle appears in `format_source(src)`, then that fmt is stable.
fn assert_fmt_keeps(src: &str, needles: &[&str], label: &str) {
    let out = jet::format_source(src).unwrap_or_else(|d| {
        panic!(
            "fmt failed for {label}:\n{}",
            jet::render_diagnostics(label, src, &d)
        )
    });
    for needle in needles {
        assert!(
            out.contains(needle),
            "{label}: fmt dropped `{needle}`\n--- got ---\n{out}"
        );
    }
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(
        out, twice,
        "{label}: fmt is not idempotent\n--- got ---\n{out}"
    );
}

#[test]
fn fmt_nested_enum_groups_stability() {
    let src = "\
enum Damage {
    Physical { Blunt, Pierce }
    Fire { Burn, Scald }
    Cold
}

fn run() {
    d := Damage.Fire.Burn
    if d == {
        .Physical -> { print(\"physical\") }
        .Fire -> { print(\"fire\") }
        .Cold -> { print(\"cold\") }
    }
}
";
    assert_fmt_keeps(
        src,
        &["Physical {", "Blunt", "Pierce", ".Fire ->", "Fire {"],
        "nested enum variant groups (D-TAG1)",
    );
}

#[test]
fn fmt_keeps_codable_and_field_rename_and_turbofish() {
    // The exact c-fmt regression: `#Codable` / `#Rename("…")`
    // (directive/serde plane, D-MARKERMOVE1=B), and a method-call turbofish
    // `decode<Order>` must all survive.
    let src = "\
#Codable
struct Order {
    id: Int
    #Rename(\"customer\") who: String
}

fn run() {
    raw :: \"x\"
    back :: json.decode<Order>(raw) ?? panic(\"bad\")
    print(back.who)
}
";
    assert_fmt_keeps(
        src,
        &["#Codable", "#Rename(\"customer\")", "decode<Order>"],
        "codable + rename + turbofish",
    );
}

#[test]
fn fmt_rewrites_marker_stacking_to_one_shape_and_is_stable() {
    let single = jet::format_source("#[Codable]\nstruct Invoice { id: Int }\n")
        .expect("fmt should recover a single-item marker list");
    assert!(
        single.contains("#Codable\nstruct Invoice"),
        "fmt must remove brackets around one marker:\n{single}"
    );
    let single_twice = jet::format_source(&single).expect("canonical single marker should parse");
    assert_eq!(single, single_twice, "single-marker rewrite must be stable");

    let stacked = jet::format_source("#Job #Every(5min) fn prune() {\n}\n")
        .expect("fmt should recover a bare marker stack");
    assert!(
        stacked.contains("#[Job, Every(5min)] fn prune"),
        "fmt must combine a bare stack into one marker list:\n{stacked}"
    );
    let stacked_twice =
        jet::format_source(&stacked).expect("canonical marker list should parse");
    assert_eq!(stacked, stacked_twice, "marker-list rewrite must be stable");
}

#[test]
fn fmt_keeps_cli_doc_and_default_field_markers() {
    // D-SHAPE2: two field rules share one `#[…]` group; one stays bare.
    let src = "\
#CLI
struct ServeArgs {
    #[Doc(\"port to listen on\")] port: Int = 3000
    #Doc(\"print extra detail\") verbose: Bool
}

fn run(args: ServeArgs) {
    print(args.port)
    print(args.verbose)
}
";
    assert_fmt_keeps(
        src,
        &["#Doc(\"port to listen on\")", "port: Int = 3000", "#CLI"],
        "cli #Doc + field = default",
    );
    assert_fmt_stable(src, "cli doc/default field markers");
}

#[test]
fn fmt_keeps_container_rename_all() {
    // Derive and container serde rules share one applied-rule group.
    let src = "\
#[Codable, RenameAll(camel)]
struct Person {
    full_name: String
}
";
    assert_fmt_keeps(
        src,
        &["#[Codable, RenameAll(camel)]"],
        "container RenameAll",
    );
}

#[test]
fn fmt_keeps_layout_columnar_and_codable() {
    // Two rules on the same struct share one ordered group; neither is
    // rewritten into body `derive` lines.
    let src = "\
#[Codable, Layout(columnar)]
struct Particle {
    x: Float
}
";
    assert_fmt_keeps(
        src,
        &["#[Codable, Layout(columnar)]"],
        "layout columnar + codable",
    );
    // And no `derive Encode`/`derive Decode` body lines leak in.
    let out = jet::format_source(src).unwrap();
    assert!(
        !out.contains("derive Encode"),
        "no leaked body derive:\n{out}"
    );
    assert!(
        !out.contains("derive Decode"),
        "no leaked body derive:\n{out}"
    );
}

#[test]
fn fmt_stable_at_marked_fn_and_struct() {
    // D-MARKER-FAMILY1/D-MARKERMOVE1: a byte-identical round-trip (not just
    // idempotence — idempotence alone misses a formatter that drops a token
    // on the FIRST pass) for an `@`-marked fn and an `@`-marked struct.
    let fn_src = "\
fn double(n: Int) =[]=> Int {
    return (n * 2)
}
";
    assert_fmt_stable(fn_src, "#Pure fn round-trip");

    let struct_src = "\
#[Codable, Layout(columnar)]
struct Particle {
    x: Float
}
";
    assert_fmt_stable(struct_src, "#Codable + #Layout struct round-trip");
}

#[test]
fn fmt_stable_effect_arrows() {
    let src = "\
fn load(path: String) =[FS.Read, ..E]=> String {
    return path
}

fn visit(callback: fn(Int) =[IO]=>) =[via callback]=> {
    callback(1)
}

// `=[]=>` replaces the retired `#Pure` marker without moving this comment.
fn hash(n: Int) =[]=> Int {
    return n
}
";
    assert_fmt_stable(src, "D-SHAPE8 effect arrows");
}

#[test]
fn fmt_keeps_single_use_marker() {
    let src = "\
#SingleUse struct Lock {
    resource: String
}
";
    assert_fmt_keeps(src, &["#SingleUse struct Lock"], "single-use struct");
}

#[test]
fn fmt_keeps_layout_c_struct() {
    let src = "\
#Layout(c)
struct Header {
    magic: Int
}
";
    assert_fmt_keeps(src, &["#Layout(c)"], "layout c struct");
}

#[test]
fn fmt_rejects_retired_body_derive_line() {
    // Capability requests have one spelling: a marker before the type.
    let src = "\
struct Score {
    points: Int

    derive Comparable
}
";
    assert!(jet::format_source(src).is_err());
}

#[test]
fn fmt_keeps_enum_variant_rename_and_tag() {
    // Enum derive + container rule share one group; the variant keeps its group.
    let src = "\
#[Codable, Discriminant(\"type\")]
enum Shape {
    #Rename(\"circle\") Circle(Float)
    Square(Float)
}
";
    assert_fmt_keeps(
        src,
        &["#[Codable, Discriminant(\"type\")]", "#Rename(\"circle\")"],
        "enum tag + variant rename",
    );
}

#[test]
fn fmt_keeps_replayable_marker() {
    let src = "\
#Replayable fn replay_turn(seed: Int) => Int {
    return seed + 1
}
";
    assert_fmt_keeps(src, &["#Replayable fn replay_turn"], "replayable fn");
}

#[test]
fn fmt_keeps_typestate_markers() {
    // D-STATE1: the `#State(S)` require-state guard and `#Transition(From, To)`
    // transition markers (including the entry form `_, To`) must survive fmt —
    // dropping a typestate contract would silently change what the checker enforces.
    let src = "\
struct Reservation {
    guest: String

    #Transition(_, Pending) fn book(guest: String) => Reservation {
        return Reservation.{guest: ~guest}
    }

    #Transition(Pending, Confirmed) fn pay(self: ^Reservation) => Reservation {
        return self
    }

    #State(Confirmed) fn check_in(self) {
        print(self.guest)
    }
}
";
    assert_fmt_keeps(
        src,
        &[
            "#Transition(_, Pending)",
            "#Transition(Pending, Confirmed)",
            "#State(Confirmed)",
        ],
        "typestate markers",
    );
}

#[test]
fn fmt_box_drawing_comment_does_not_panic() {
    // c143: a comment containing multibyte box-drawing glyphs, placed on its own
    // line between two steps of a broken method chain, used to panic. The
    // chain-break path measures the output's last-newline byte offset against
    // the *source* (`is_trailing_comment_at` → `line_of`); with a multibyte
    // glyph in the source that offset could land mid-codepoint and the raw slice
    // panicked. fmt must never panic on valid input (I2). The box-drawing glyphs
    // round-trip verbatim.
    let src = "fn run() {\n    x :: foo()\n        // \u{2502}\u{250c}\u{2514}\u{2500}\n        .bar()\n    print(x)\n}\n";
    let out = jet::format_source(src).expect("fmt should not panic on box-drawing comments");
    assert!(
        out.contains('\u{250c}') && out.contains('\u{2502}'),
        "box glyphs dropped:\n{out}"
    );
    let twice = jet::format_source(&out).expect("box-drawing fmt must re-fmt");
    assert_eq!(out, twice, "box-drawing fmt must be idempotent");
}

#[test]
fn fmt_impure_block_round_trips() {
    // D-CTEFFECT1: `#Impure("reason") { … }` must survive a format round-trip
    // with the reason string and body intact.
    let src = "fn run() {\n    #Impure(\"reading build config\") {\n        print(\"inside\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on #Impure block");
    assert!(
        out.contains("#Impure(\"reading build config\")"),
        "#Impure reason dropped by fmt:\n{out}"
    );
    let twice = jet::format_source(&out).expect("#Impure fmt must re-fmt");
    assert_eq!(out, twice, "#Impure fmt must be idempotent");
}

#[test]
fn fmt_impure_block_no_reason_round_trips() {
    // D-CTEFFECT1: `#Impure { … }` without a reason also round-trips (triggers
    // L3102 lint but is parseable).
    let src = "fn run() {\n    #Impure {\n        print(\"inside\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on #Impure block without reason");
    assert!(
        out.contains("#Impure {") || out.contains("#Impure{"),
        "#Impure block without reason dropped by fmt:\n{out}"
    );
    let twice = jet::format_source(&out).expect("#Impure (no reason) fmt must re-fmt");
    assert_eq!(out, twice, "#Impure (no reason) fmt must be idempotent");
}

#[test]
fn fmt_unsafe_reasons_escape_strings() {
    // D-UNSAFE-REASON1=A: unsafe block/function reasons are normal string
    // literals, so fmt must preserve quotes/backslashes as parseable Jet.
    let src = "use core.mem\n\n#Unsafe(\"caller says \\\"ok\\\"\") fn raw() => Int {\n    return 1\n}\n\nfn run() {\n    #Unsafe(\"path C:\\\\tmp\") {\n        print(\"{raw()}\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on unsafe reasons");
    assert!(
        out.contains("#Unsafe(\"caller says \\\"ok\\\"\")")
            && out.contains("#Unsafe(\"path C:\\\\tmp\")"),
        "unsafe reason escaping broke:\n{out}"
    );
    let twice = jet::format_source(&out).expect("unsafe reason fmt must re-fmt");
    assert_eq!(out, twice, "unsafe reason fmt must be idempotent");
}

#[test]
fn fmt_keeps_parens_around_binary_receiver() {
    // c143(b): `(a + b).method()` must keep its parens — dropping them rebinds
    // the `.method()` to `b` alone and changes the meaning. Likewise `(a + b) * c`
    // must not become `a + b * c`.
    let src = "fn run() {\n    c :: (1 + 2).to_string()\n    d :: (1 + 2) * 3\n    print(c)\n    print(d)\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed");
    assert!(
        out.contains("(1 + 2).to_string()"),
        "parens around binary method receiver dropped:\n{out}"
    );
    assert!(
        out.contains("(1 + 2) * 3"),
        "parens around lower-prec binary operand dropped:\n{out}"
    );
    // The common case must NOT gain spurious parens.
    let plain = jet::format_source("fn run() {\n    a :: 1 + 2 * 3\n    print(a)\n}\n").unwrap();
    assert!(
        plain.contains("a :: 1 + 2 * 3"),
        "added spurious parens:\n{plain}"
    );
    let twice = jet::format_source(&out).expect("paren fmt must re-fmt");
    assert_eq!(out, twice, "paren fmt must be idempotent");
}

#[test]
fn fmt_comptime_block_is_idempotent() {
    // D-META-STAGE1=B: `@ { … }` formatting
    // round-trips — the block keyword, brace, and body all survive a second fmt.
    let src = r#"@limit :: 1000

fn run() {
    @ {
        @ratio :: limit / 10
        if ratio < 1 { panic("bad") }
    }
    print("ok")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept comptime block");
    assert!(
        out.contains("@ {"),
        "comptime block keyword + open brace must survive fmt, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "comptime block formatting must be idempotent");
}

#[test]
fn fmt_dot_construction_d_dotctor1() {
    // D-DOTCTOR1 (ratified 2026-06-25): named struct construction `Type.{ … }` and
    // inferred `.{ … }` survive fmt unchanged (stability), and the old dotless
    // `Type { … }` form (E0320 recovery) is auto-fixed to `Type.{ … }`.

    // Named form round-trips unchanged.
    let named = "struct Pt {\n    x: Int\n    y: Int\n}\n\nfn run() {\n    p :: Pt.{ x: 1, y: 2 }\n    print(\"{p.x}\")\n}\n";
    let out_named = jet::format_source(named).expect("fmt should accept Pt.{ … }");
    assert!(
        out_named.contains("Pt.{"),
        "named form `Pt.{{` must survive fmt, got:\n{out_named}"
    );
    assert!(
        !out_named.contains("Pt {x") && !out_named.contains("p :: Pt {"),
        "fmt must not regress to dotless construction form, got:\n{out_named}"
    );
    let twice_named = jet::format_source(&out_named).expect("named form must re-fmt");
    assert_eq!(out_named, twice_named, "named form must be fmt-idempotent");

    // Old dotless form (E0320 recovery) is auto-fixed to `Type.{`.
    let old = "struct Pt {\n    x: Int\n    y: Int\n}\n\nfn run() {\n    p :: Pt { x: 1, y: 2 }\n    print(\"{p.x}\")\n}\n";
    let out_old = jet::format_source(old).expect("fmt should recover dotless E0320 form");
    assert!(
        out_old.contains("Pt.{"),
        "E0320 recovery must auto-fix to `Pt.{{`, got:\n{out_old}"
    );
    assert!(
        !out_old.contains("Pt {x") && !out_old.contains("p :: Pt {"),
        "dotless construction form must not survive fmt, got:\n{out_old}"
    );
    let twice_old = jet::format_source(&out_old).expect("fixed form must re-fmt");
    assert_eq!(out_old, twice_old, "fixed form must be fmt-idempotent");
}

#[test]
fn fmt_inferred_new_stability() {
    let src = "struct Box<T> {\n    value: T\n}\n\nimpl Box {\n    fn new(value: ^T) => Box<T> {\n        return Box<T>.{ value: value }\n    }\n}\n\nfn run() {\n    inferred :: Box.new(1)\n    explicit :: Box<Int>.new(2)\n    expected :: Box<Int>.new(3)\n}\n";
    let out = jet::format_source(src).expect("fmt should accept `.new(...)`");
    for spelling in ["Box.new(1)", "Box<Int>.new(2)", ".new(3)"] {
        assert!(out.contains(spelling), "formatter lost `{spelling}`:\n{out}");
    }
    assert_eq!(out, jet::format_source(&out).expect("second fmt"));
}

#[test]
fn fmt_enum_dot_pattern_stability() {
    // D-ENUMDOT1 (ratified 2026-06-26): a leading `.` before a variant name in pattern position
    // is accepted and canonical. The formatter always emits `.` before a Pattern::Variant name,
    // so `.Circle(r)` round-trips unchanged and bare `Circle(r)` is canonicalized to `.Circle(r)`.

    // Dot form round-trips unchanged (stability).
    let dot_src = "\
enum Shape {
    Circle(Float)
    Square(Float)
    Empty
}

fn describe(s: Shape) => String {
    if s == {
        .Circle(r) -> { return \"circle:{r}\" }
        .Square(side) -> { return \"square:{side}\" }
        .Empty -> { return \"empty\" }
    }
    return \"?\"
}

fn run() {
    print(describe(Shape.Circle(2.0)))
    print(describe(Shape.Empty))
}
";
    assert_fmt_keeps(
        dot_src,
        &[".Circle(r)", ".Square(side)", ".Empty"],
        "dot-variant patterns in if-dispatch arms",
    );

    // Bare payload form is canonicalized to dot form by fmt.
    let bare_src = "\
enum Shape {
    Circle(Float)
    Square(Float)
}

fn area(s: Shape) => Float {
    if s == {
        Circle(r) -> { return r * r }
        Square(side) -> { return side * side }
    }
    return 0.0
}

fn run() {
    print(area(Shape.Circle(3.0)))
}
";
    let out = jet::format_source(bare_src).expect("fmt should accept bare variant patterns");
    assert!(
        out.contains(".Circle(r)"),
        "bare payload variant in if-dispatch arm must gain leading dot, got:\n{out}"
    );
    assert!(
        out.contains(".Square(side)"),
        "bare payload variant `Square(side)` must gain leading dot, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("second fmt of dot-form must succeed");
    assert_eq!(
        out, twice,
        "dot-form variant patterns must be fmt-idempotent"
    );
}

#[test]
fn fmt_comptime_splice_stability() {
    // D-ONCE-AT1=D: `@name` compile-time value is a first-class AST node
    // (Expr::ComptimeName). The formatter must emit it as `@name` so that
    // the round-trip is stable (previously the mark could be silently dropped
    // if it reached the formatter without an AST node).
    let src = "derive T.Debug {\n    info :: T.reflect()\n    tname :: info.name\n    emit(\"impl @tname {{ fn tag(self) => String {{ return \\\"ok\\\" }} }}\")\n}\n\nfn run() {\n    print(\"ok\")\n}\n";
    let once = jet::format_source(src).expect("fmt should accept derive with @name in string");
    let twice = jet::format_source(&once).expect("second fmt should succeed");
    assert_eq!(
        once, twice,
        "derive body with @name in emit string must be fmt-idempotent"
    );

    // Standalone `@name` expression (outside emit string) round-trips as `@name`.
    let splice_src = "derive T.Named {\n    tname :: \"test\"\n    x :: @tname\n    emit(\"impl @tname {{ }}\")\n}\n\nfn run() {}\n";
    let splice_once = jet::format_source(splice_src).expect("fmt should accept @name expression");
    assert!(
        splice_once.contains("@tname"),
        "`@tname` expression must survive fmt, got:\n{splice_once}"
    );
    let splice_twice = jet::format_source(&splice_once).expect("@name fmt must be idempotent");
    assert_eq!(
        splice_once, splice_twice,
        "`@name` expression must be fmt-idempotent"
    );
}

#[test]
fn fmt_layout_compiler_fact_and_field_selector_stability() {
    let src = "derive T.LayoutFacts {\n    info :: T.@layout\n    selected :: info[.count]\n    full :: T.reflect().layout\n}\n\nfn run() {}\n";
    let once = jet::format_source(src).expect("layout compiler fact should parse");
    assert!(once.contains("T.@layout"), "fact spelling was lost:\n{once}");
    assert!(once.contains("info[.count]"), "typed selector spelling was lost:\n{once}");
    assert!(once.contains("T.reflect().layout"), "reflection projection was lost:\n{once}");
    let twice = jet::format_source(&once).expect("formatted layout fact should parse");
    assert_eq!(once, twice, "layout fact formatting must be idempotent");
}

#[test]
fn layout_compiler_fact_rejects_unknown_and_user_owned_at_members() {
    let unknown = jet::Compiler::parse_source(
        "derive T.LayoutFacts { info :: T.@unknown }\nfn run() {}\n",
    );
    let unknown = unknown
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0302")
        .expect("unknown compiler fact should have a registered diagnostic");
    assert!(unknown.message.contains("@unknown"), "{unknown:?}");
    assert!(unknown.fix.contains("@layout"), "{unknown:?}");

    let user_member = jet::Compiler::parse_source(
        "struct Bad { @layout: Int }\nfn run() {}\n",
    );
    assert!(
        user_member.diagnostics.iter().any(|diagnostic| diagnostic.code == "E0003"),
        "user declarations must not claim the compiler-owned @ member: {:?}",
        user_member.diagnostics
    );
}

#[test]
fn fmt_impl_dot_trait_stability() {
    // D-IMPLDOT1=A: `impl Type.Trait { … }` round-trips unchanged.
    let src = "\
trait Greet {
    fn hello(self) => String;
}

struct Foo {}

impl Foo.Greet {
    fn hello(self) => String {
        return \"hi\"
    }
}
";
    assert_fmt_keeps(src, &["impl Foo.Greet"], "impl dot trait");
    let once = jet::format_source(src).expect("fmt should accept impl Type.Trait");
    let twice = jet::format_source(&once).expect("second fmt of impl Type.Trait must succeed");
    assert_eq!(once, twice, "impl Type.Trait must be fmt-idempotent");
}

#[test]
fn fmt_test_paren_name_stability() {
    // D-TESTPAREN1=A: `#Test("name") { … }` must survive fmt unchanged.
    let src = "\
fn run() {}
#Test(\"double returns twice\") {
    require_eq(2 * 2, 4)
}
";
    assert_fmt_keeps(src, &["#Test(\"double returns twice\")"], "test paren name");
    // Idempotence: formatting twice produces the same output.
    let once = jet::format_source(src).expect("fmt should accept #Test(\"name\")");
    let twice = jet::format_source(&once).expect("second fmt of #Test(\"name\") must succeed");
    assert_eq!(once, twice, "#Test(\"name\") must be fmt-idempotent");
}

#[test]
fn fmt_scope_members_stability() {
    // D-DOTSCOPE1: `.setup` / `.expect_fail` / `.timeout(dur)` / `.skip("why")`
    // scope-member statements must survive fmt byte-for-byte, including the
    // duration literal and the reason string — and the terminator inserted
    // between two members (`}` then `.name {`) must round-trip.
    let src = "\
fn run() {

}

#Test(\"members\") {
    .setup {
        base :: 1
    }
    .expect_fail {
        require(false)
    }
    .timeout(500ms) {
        require((base == 1))
    }
    .skip(\"later\") {
        require(false)
    }
}
";
    assert_fmt_stable(src, "scope members");
}

#[test]
fn fmt_bare_binding_d_bind_bare1_stability() {
    // D-BIND-BARE1: bare bindings must survive fmt unchanged.
    let src = "\
fn run() {
    x :: 42
    s := \"hi\"
    print(\"{x} {s}\")
}
";
    assert_fmt_keeps(
        src,
        &["x :: 42", "s := \"hi\""],
        "bare binding",
    );
    let once = jet::format_source(src).expect("fmt should accept bare bindings");
    let twice = jet::format_source(&once).expect("second fmt of bare bindings must succeed");
    assert_eq!(once, twice, "bare binding fmt must be idempotent");
}

#[test]
fn fmt_preserves_track_binding_marker() {
    let src = "\
fn run() {
    #Track count :: 1
    #Track total := 2
    #Track label :: \"ok\"
    print(\"{count} {total} {label}\")
}
";
    assert_fmt_stable(src, "#Track binding marker");
}

#[test]
fn fmt_preserves_local_and_shared_reactive_pins() {
    let src = "\
use core.reactive as reactive

fn run() {
    #Local pending := reactive.signal(0)
    #Shared shared := reactive.signal(1)
    print(\"{pending.get()} {shared.get()}\")
}
";
    assert_fmt_stable(src, "#Local/#Shared reactive pins");
}

#[test]
fn fmt_write_sigil_d_mem1_stability() {
    // D-MEM1 (S1): `&T` is the write sigil (param + call-site mirror), `&self`
    // is the write receiver, `^T`/`^self` (take/move) are unchanged, and plain
    // `T`/`self` (read) carries no sigil. Must all survive fmt unchanged.
    let src = "\
struct Player {
    hp: Int
}

impl Player {
    fn show(self) => Int { return self.hp }

    fn heal(&self, amount: Int) { self.hp = self.hp + amount }
}

fn damage(p: &Player, amount: Int) {
    p.hp = p.hp - amount
}

fn archive(p: ^Player) => Int {
    return p.hp
}

fn run() {
    p := Player.{hp: 100}
    damage(&p, 10)
    p.heal(5)
    print(archive(^p))
}
";
    assert_fmt_keeps(
        src,
        &[
            "fn heal(&self, amount: Int)",
            "fn damage(p: &Player, amount: Int)",
            "fn archive(p: ^Player) => Int",
            "damage(&p, 10)",
            "archive(^p)",
        ],
        "D-MEM1 write sigil",
    );
}

#[test]
fn fmt_no_alloc_policy_d_mem1_s7_stability() {
    // D-POLICY-WORD1=A: `#Policy(no_alloc)` is a fixed post-import
    // file marker, same treatment as `#PubFile`/`#Target(…)` — must survive
    // fmt unchanged.
    let src = "\
#Policy(no_alloc)

fn run() {
    print(\"ok\")
}
";
    assert_fmt_keeps(src, &["#Policy(no_alloc)"], "D-POLICY-WORD1 policy marker");
}

#[test]
fn fmt_copy_sigil_d_shape_copy1_stability() {
    // D-SHAPE-COPY1=A (supersedes D-CAP2/D-MEM1/S4): `~x` is a prefix-verb
    // expression — must survive fmt unchanged in binding position, call-arg
    // position, and on a field.
    let src = "\
struct Ticket {
    id: Int
    label: String
}

fn archive(t: ^Ticket) => String {
    return t.label
}

fn run() {
    name :: \"vault\"
    saved :: ~name
    t :: Ticket.{id: 1, label: \"root\"}
    print(archive(~t))
    print(~t.label)
}
";
    assert_fmt_keeps(
        src,
        &[
            "saved :: ~name",
            "print(archive(~t))",
            "print(~t.label)",
        ],
        "D-SHAPE-COPY1 copy sigil",
    );
}

#[test]
fn fmt_loop_label_d_looplabel3_stability() {
    // D-LOOPLABEL3=A: named loops and target-argument exits survive unchanged.
    let src = "\
fn run() {
    outer :: loop i, [1, 2] {
        loop j, [1, 2] {
            if i == j {
                value() ?? next(outer)
                break(outer)
            }
        }
    }
    print(\"done\")
}
";
    assert_fmt_keeps(
        src,
        &["outer :: loop", "next(outer)", "break(outer)"],
        "named loop target-argument exits",
    );
    let once = jet::format_source(src).expect("fmt should accept named loop dot exits");
    let twice = jet::format_source(&once).expect("second fmt of loop labels must succeed");
    assert_eq!(once, twice, "loop label fmt must be idempotent");
}

#[test]
fn fmt_loop_values_and_yielding_loops_are_idempotent() {
    let src = r#"fn find(xs: [Int]) => Int {
    found :: loop {
        loop x, xs {
            if x > 2 { break(found, x) }
        }
        break -1
    }
    found
}

fn run() {
    xs :: [Int].{ 1, 2, 3, 4 }
    values :: loop x, xs -> {
        if x > 3 { break }
        x * 2
    }
    print(values)
}
"#;
    assert_fmt_keeps(
        src,
        &["found :: loop", "break(found, x)", "values :: loop x, xs ->"],
        "loop values",
    );
    let once = jet::format_source(src).expect("fmt should accept loop values");
    let twice = jet::format_source(&once).expect("second loop-value fmt should succeed");
    assert_eq!(once, twice, "loop-value fmt must be idempotent");
    assert!(
        !once.contains("__jet_"),
        "formatter must hide compiler-generated machine labels: {once}"
    );
}

#[test]
fn fmt_counted_loop_d_loop_semicolon1_stability() {
    // D-LOOP-HEADER3=D: range form is the live three-slot / counted surface.
    let src = "\
fn run() {
    sum := 0
    loop i, 0..<5 {
        sum += i
    }
    print(sum)
}
";
    assert_fmt_keeps(src, &["loop i, 0..<5"], "range counted loop header");
    let once = jet::format_source(src).expect("fmt should accept range counted loop");
    let twice = jet::format_source(&once).expect("second fmt of counted loop must succeed");
    assert_eq!(once, twice, "counted loop fmt must be idempotent");
}

#[test]
fn fmt_unified_loop_headers_and_next_stability() {
    let src = "fn next() => Int { return 7 }\n\nfn run() {\n    next()\n    cursor.next()\n    saved :: Int.parse(\"1\") ?? (next)\n    loop item, [1, 2, 3], 2 {\n        value :: Int.parse(\"1\") ?? next\n        if value == 1 { next }\n    }\n}\n";
    assert_fmt_keeps(
        src,
        &[
            "fn next()",
            "next()",
            ".next()",
            "?? (next)",
            "loop item, [1, 2, 3], 2",
            "?? next",
            "{ next }",
        ],
        "unified loop header",
    );
    let once = jet::format_source(src).expect("fmt should accept unified loop syntax");
    let twice = jet::format_source(&once).expect("formatted unified loop must parse");
    assert_eq!(once, twice, "unified loop formatting must be stable");

    for retired in [
        "fn run() { loop x in [1] {} }\n",
        "fn run() { loop i, 0..2 step 1 {} }\n",
        "fn run() { loop { continue } }\n",
        "fn run() { loop i :: 0, true {} }\n",
        "fn run() { loop [i] := [0], true {} }\n",
        "fn run() { loop i := 0 true {} }\n",
        "fn run() { loop i := 0, true, i += 1, i += 2 {} }\n",
        "fn run() { loop i := 0, i < 3, i += 1 {} }\n",
    ] {
        assert!(
            jet::format_source(retired).is_err(),
            "retired loop spelling must take the ordinary parse-error path: {retired}"
        );
    }
}

#[test]
fn control_bodies_gain_braces_and_collapse_when_they_fit() {
    let src = "fn run() {\n    if true {\n        print(\"yes\")\n    }\n    loop false print(\"no\")\n}\n";
    let once = jet::format_source(src).expect("fmt should recover the retired adjacent loop");
    assert!(once.contains("if true { print(\"yes\") }"), "{once}");
    assert!(once.contains("loop false { print(\"no\") }"), "{once}");
    let twice = jet::format_source(&once).expect("formatted controls must parse");
    assert_eq!(once, twice);
}

#[test]
fn loop_headers_use_comma_clauses_and_group_two_names() {
    let src = "fn run() {\n    loop item, [1, 2, 3], 2 { print(item) }\n    loop (key, value), counts { print(key) }\n    loop i, 0..<3 { print(i) }\n}\n";
    let once = jet::format_source(src).expect("fmt should accept comma loop headers");
    for expected in [
        "loop item, [1, 2, 3], 2 { print(item) }",
        "loop (key, value), counts { print(key) }",
        "loop i, 0..<3 { print(i) }",
    ] {
        assert!(once.contains(expected), "missing `{expected}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("formatted loop headers must parse");
    assert_eq!(once, twice);
}

#[test]
fn fmt_long_loop_headers_wrap_at_clause_boundaries_stability() {
    let src = r#"fn run() {
    loop item, // source clause
        [1000000000000000000, 2000000000000000000, 3000000000000000000, 4000000000000000000], // stride clause
        2 {
        next
    }
    loop cursor, // start
        0..<9000000000000000000, // end
        1000000000000000000 {
        print(cursor)
    }
    loop ready := true, ready {
        break
    }
}
"#;
    let once = jet::format_source(src).expect("long unified loop headers should format");
    for token in [
        "loop item,",
        "// source clause",
        "],",
        "// stride clause",
        "loop cursor,",
        "0..<9000000000000000000,",
        "1000000000000000000",
        "loop ready := true, ready {",
    ] {
        assert!(once.contains(token), "fmt dropped loop token `{token}`:\n{once}");
    }
    assert!(
        once.contains("loop cursor,"),
        "long range header must keep comma clauses:\n{once}"
    );
    let twice = jet::format_source(&once).expect("formatted long loop headers must reparse");
    assert_eq!(once, twice, "long loop header formatting must be byte-stable");
}

#[test]
fn fmt_selective_import_d_selimport1_stability() {
    // D-SELIMPORT1=A: `use mod.[a, b as c]` must survive fmt unchanged.
    let src = "\
module math {
    pub fn clamp(x: Int, lo: Int, hi: Int) => Int {
        if x < lo { return lo }
        if x > hi { return hi }
        return x
    }
}

use math.[clamp, clamp as c2]
use core.math.[abs as absolute, min]
use core.encoding.[json, csv]
use c.[raylib as rl, sqlite3]

fn run() {
    print(clamp(15, 0, 10))
    print(c2(5, 0, 3))
}
";
    assert_fmt_keeps(
        src,
        &[
            "use math.[clamp, clamp as c2]",
            "use core.math.[abs as absolute, min]",
            "use core.encoding.[json, csv]",
            "use c.[raylib as rl, sqlite3]",
        ],
        "selective import with alias",
    );
    let once = jet::format_source(src).expect("fmt should accept selective imports");
    let twice = jet::format_source(&once).expect("second fmt of selective imports must succeed");
    assert_eq!(once, twice, "selective import fmt must be idempotent");
}

#[test]
fn fmt_value_tag_type_d_qual4_stability() {
    // D-QUAL4=A: `#TagName T` in type position must survive fmt unchanged.
    let src = "\
fn process(input: #Input String) => String {
    return \"{input}-clean\"
}

fn run() {
    result :: process(\"hello\")
    print(result)
}
";
    assert_fmt_keeps(src, &["#Input String"], "value-tag type qualifier");
    let once = jet::format_source(src).expect("fmt should accept value-tag types");
    let twice = jet::format_source(&once).expect("second fmt of value-tag types must succeed");
    assert_eq!(once, twice, "value-tag type fmt must be idempotent");
}

#[test]
fn fmt_layout_block_round_trips_byte_for_byte() {
    // D-LAYOUT1: the parser desugars every `box.anchor` read inside
    // `NAME :: Layout.{ … }` into a `NAME.h(box, anchor)`/`NAME.v(box, anchor)`
    // method call (a purely structural rewrite, `Parser/Statements.rs`); the
    // formatter must re-sugar those calls back to `box.anchor` so `layout`
    // round-trips STABILITY, not just idempotence (memory: a prior formatter
    // change silently dropped tokens while only idempotence was checked).
    let src = "\
fn run() {
    form :: Layout.{
        label.width >= 80.0,
        label.right + 16.0 == input.left,
        label.width + 16.0 + input.width == self.width
    }
    w :: form.value(form.h(\"label\", \"width\"))
    print(w)
}
";
    let out = jet::format_source(src).expect("fmt should accept a `layout` block");
    assert_eq!(
        out, src,
        "`layout {{ … }}` must round-trip byte-for-byte, not just idempotently"
    );
    let twice = jet::format_source(&out).expect("second fmt of a layout block must succeed");
    assert_eq!(out, twice, "layout block fmt must be idempotent");
}

#[test]
fn fmt_preserves_multiline_lambda_call_arg() {
    // D-TRAILBLOCK2=A: multiline `() => { … }` inside call parentheses is the
    // spelling for code-as-argument; fmt must round-trip it byte-for-byte and
    // must not rewrite it into retired trailing-block sugar.
    let src = "\
fn twice(f: fn()) {
    f()
    f()
}

fn run() {
    twice(() => {
        print(\"HI\")
        print(\"Hello\")
    })
}
";
    assert_fmt_stable(src, "multiline () => call arg");
}

#[test]
fn fmt_preserves_bare_lambda_params() {
    // D-LAMBDAINFER1: fmt preserves the bare spelling and does not insert a
    // parameter type. Explicit parentheses and annotations also survive.
    let src = "\
fn run() {
    nums :: [1, 2, 3, 4, 5]
    big :: nums.filter(x => x > 3)
    explicit :: nums.filter((x) => x > 3)
    typed :: nums.filter((n: Int) => n > 3)
    print(big)
    print(explicit)
    print(typed)
}
";
    assert_fmt_stable(src, "bare lambda params");
}

#[test]
fn fmt_preserves_destructure_rest() {
    // D-DESTRUCT1: a rename (`severity: sev`) and a trailing mandatory `..`
    // must both survive byte-for-byte — this is exactly the class of drop
    // idempotence-only checks miss.
    let src = "\
struct Incident {
    id: Int
    severity: Int
    title: String
}

fn run() {
    Incident.{id, severity: sev, ..} :: Incident.{id: 1, severity: 5, title: \"boom\"}
    print(\"{id} {sev}\")
}
";
    assert_fmt_stable(src, "destructure rename + rest");
}

#[test]
fn fmt_preserves_struct_pattern_arm_head() {
    // D-DESTRUCT1: dispatch-arm struct patterns carry both value checks and bindings.
    // The formatter must keep the leading `.{`, field value, binding, and `..`.
    let src = "\
struct Incident {
    title: String
    kind: String
}

fn run() {
    routed :: Incident.{title: \"database down\", kind: \"page\"}
    if routed == {
        .{ kind: \"page\", title, .. } -> print(\"page {title}\")
        else -> print(\"other\")
    }
}
";
    assert_fmt_keeps(
        src,
        &[".{kind: \"page\", title, ..}", "print(\"page {title}\")"],
        "struct-pattern dispatch arm",
    );
}

#[test]
fn fmt_preserves_unit_literal() {
    // D-UNITLIT1 / D-TYPE2-TIME1: `500ms` must survive with no space inserted
    // between the number and the suffix, and the suffix itself must not be dropped.
    let src = "\
#UnitFamily(Time) { ms, s }

fn run() {
    a :: 500ms
    print(\"{a.raw()}\")
}
";
    assert_fmt_stable(src, "unit literal");
}

#[test]
fn fmt_preserves_scaled_affine_unit_declaration() {
    // D-QUANTITY-DECL1=A: STABILITY must retain every base/scale/offset token;
    // idempotence alone could bless a first pass that discarded conversion law.
    let src = "\
#UnitFamily(Temperature, base: kelvin) {
    kelvin
    celsius(scale: 1, offset: 27315/100)
}
";
    assert_fmt_stable(src, "scaled affine #UnitFamily declaration");
}

#[test]
fn fmt_preserves_open_dimension_and_scale_provenance() {
    let src = r#"#UnitFamily(Force, dimension: Mass * Length / Time / Time, base: newton) {
    newton
    standard_gravity(scale: conventional(9.80665, source: "BIPM-2026"))
}
"#;
    assert_fmt_stable(src, "open dimension and scale provenance");
}

#[test]
fn fmt_preserves_rounded_unit_conversion_contract() {
    let src = r#"#UnitFamily(Length, base: meter) {
    meter
    half(scale: 1/2)
}

fn run() {
    source :: 5half
    Meter.from_half_rounded(source, .TowardZero, digits: 0).drop("mode")
    Meter.from_half_rounded(source, .Floor, digits: 1).drop("mode")
    Meter.from_half_rounded(source, .Ceiling, digits: 2).drop("mode")
    Meter.from_half_rounded(source, .NearestEven, digits: 3).drop("mode")
}
"#;
    assert_fmt_stable(src, "D-QUANTITY-CONVERT1 rounded conversion");
}

#[test]
fn fmt_preserves_range_constraint() {
    // D-RANGETYPE1 / D-TYPE2-REFINE1: `distinct Int(0..10)` — distinct
    // declarations are emitted verbatim, so the `(0..10)` clause survives
    // structurally; this pins it down explicitly rather than relying on that
    // being an accident.
    let src = "\
Severity :: distinct Int(0..10)

fn run() {
    sev :: Severity.from_int(3)
    print(\"{sev.raw()}\")
}
";
    assert_fmt_stable(src, "range constraint");
}

#[test]
fn fmt_preserves_parse_pattern() {
    // D-PARSESTR1: a str-match arm head must keep BOTH an untyped hole and a
    // typed hole's `:Type` suffix byte-for-byte — the `:Type` suffix never
    // appears on an ordinary format literal, so idempotence alone (the
    // c-fmt data-loss regression class) wouldn't catch the formatter
    // silently dropping it.
    let src = "\
fn run() {
    ticket :: \"inc-42-open\"
    if ticket == {
        \"inc-{id:Int}-{status}\" -> { print(\"incident #{id} {status}\") }
        else -> {
            print(\"not an incident id\")
        }
    }
}
";
    assert_fmt_keeps(
        src,
        &[
            "\"inc-{id:Int}-{status}\"",
            "print(\"incident #{id} {status}\")",
        ],
        "str-match pattern arm (untyped + typed hole)",
    );
}

#[test]
fn fmt_preserves_typed_text() {
    // D-UNIFYLIT1=A: typed-literal heads and `.raw()` must survive byte-for-byte.
    let src = "\
fn run_query(q: SQL) {
    print(\"template: {q.template()}\")
}

fn render(h: HTML) {
    print(\"html: {h.text()}\")
}

fn run() {
    id :: 42
    q :: SQL.{\"select * from t where id = {id}\"}
    run_query(q)
    name :: \"Jet\"
    page :: HTML.{\"<p>{name}</p>\"}
    render(page)
    trusted :: HTML.raw(\"<b>audited</b>\")
    render(trusted)
    arg :: \"two words;*.jet\"
    expected :: Sh.{\"printf <%s> {arg}\"}
    audited_cmd :: Sh.raw(\"printf raw\")
    endpoint :: URL.{\"https://api.example.com/v2/{name}\"}
    log_path :: Path.{\"/var/log/{name}.log\"}
    stamp :: DateTime.{\"2026-08-07T12:00:00Z\"}
}
";
    assert_fmt_stable(src, "typed text");
}

#[test]
fn fmt_preserves_yield() {
    // D-STREAMYIELD1: `yield` must keep its own line, and `Stream<T>` in the
    // return-type position must survive byte-for-byte.
    let src = "\
fn count(n: Int) => Stream<Int> {
    i := 0
    loop i < n {
        yield i
        i = i + 1
    }
}

fn run() {
    loop x, count(3) { print(\"{x}\") }
}
";
    assert_fmt_stable(src, "yield");
}

#[test]
fn fmt_preserves_chained_comparison() {
    // D-CHAINCMP1: a same-direction relational chain (`Expr::CompareChain`)
    // must survive byte-for-byte — single spaces around each operator, no
    // operand dropped, no extra parens inserted between pairs.
    let src = "\
fn run() {
    sev :: 5
    if 0 <= sev < 10 { print(\"in range\") }
}
";
    assert_fmt_stable(src, "chained comparison");
}

#[test]
fn fmt_preserves_capbundle_markers() {
    // D-CAPBUNDLE1: a stack of capability-bundle markers before a `distinct`
    // type declaration must survive byte-for-byte — no marker dropped, no
    // reordering, the whole stack round-trips (distinct decls format
    // verbatim from source span, but the span must actually start at the
    // first marker, not the type name).
    let src = "\
#[Numeric, Comparable] Usd :: distinct Int

#[Printable, CodableAsBase] CustomerId :: distinct Int

fn run() {
    a :: Usd.from_int(100)
    print(a.raw())
}
";
    assert_fmt_stable(src, "capability bundle markers");
}

#[test]
fn fmt_preserves_contracts() {
    // D-PREPOST1: `#Pre`/`#Post` clauses (condition + message) must survive
    // byte-for-byte, in declared order. Emitted inline before `fn`, space-
    // separated — the same one-marker-placement convention every other `fn`
    // marker uses (`#State(…)`, `#Transition(…)`, `#Pure`, `#MustUse`, …;
    // I8: one way to mean it), not one clause per line.
    let src = "\
#[Pre(cents > 0, \"cents must be positive\"), Post(result > cents, \"result must exceed cents\")] fn add_fee(cents: Int) => Int {
    return cents + 5
}

fn run() {
    print(\"{(add_fee(100))}\")
}
";
    assert_fmt_stable(src, "pre/post contracts");
}

#[test]
fn fmt_preserves_persist() {
    // D-PERSIST1 / D-BIND-BARE1: `#Persist` and the bare bind sigil (`:=` / `::`)
    // must survive byte-for-byte.
    let mut_src = "\
#Persist counter := 0

fn run() {
    print(\"{counter}\")
}
";
    assert_fmt_stable(mut_src, "persist marker :=");
    let immut_src = "\
#Persist counter :: 0

fn run() {
    print(\"{counter}\")
}
";
    assert_fmt_stable(immut_src, "persist marker ::");
}

#[test]
fn fmt_preserves_variadic_trait_bound_bare() {
    // D-ANY-JAI1 (c7jaiany): `parts: ...Renderable` — the bare single-trait
    // bound sugar — must survive byte-for-byte.
    let src = "\
fn log_all(parts: ...Renderable) {
    loop p, parts { print(\"{p}\") }
}

fn run() {
    log_all(1, \"two\", true)
}
";
    assert_fmt_stable(src, "bare variadic trait bound");
}

#[test]
fn fmt_preserves_variadic_trait_bound_list() {
    // D-VARARGBOUND1 (c7jaiany, owner-amended): multi-trait bounds are a list
    // everywhere, never `+` — `parts: ...[Renderable]` (and `...[A, B]` for a
    // real multi-trait bound) must survive byte-for-byte.
    let src = "\
fn log_all(prefix: String, parts: ...[Renderable]) {
    print(prefix)
    loop p, parts { print(\"{p}\") }
}

fn run() {
    log_all(\"first:\", 1, \"two\")
    log_all(\"second:\", 3.5, false, \"x\")
}
";
    assert_fmt_stable(src, "list-form variadic trait bound");
}

#[test]
fn fmt_preserves_generic_type_param_bound_list() {
    // D-VARARGBOUND1: the same list-bound spelling applies to an ordinary
    // S45 generic type parameter, not just variadics — `<T: [A, B]>`, never
    // `<T: A + B>`. Single-trait bounds stay bare.
    let src = "\
trait Loud {
    fn shout(self) => String
}

fn describe<T: [Renderable, Loud]>(item: T) => String {
    return item.shout()
}
";
    assert_fmt_stable(src, "generic multi-trait bound list");
}

#[test]
fn fmt_preserves_quantity_dimension_kind_bound() {
    let src = "\
fn keep<Q: Quantity<Length, .Linear>>(value: ^Q) => Q {
    return value
}
";
    assert_fmt_stable(src, "quantity dimension/kind generic bound");
}

#[test]
fn fmt_preserves_view_call_range_args() {
    // D-SHAPE-PLACE1=A: `.view(a..b)` is retained only so sema can teach E0214,
    // but formatter recovery must still preserve the retired source losslessly.
    let src = "\
fn run() {
    incidents := [1, 2, 3]
    window :: incidents.view(0..2)
    print(window.len())
}
";
    assert_fmt_stable(src, "view call range args");
}

#[test]
fn retired_view_keyword_is_an_ordinary_identifier() {
    // D-SHAPE-PLACE1=A: retired `.view(a..b)` is an ordinary method parse used
    // for E0214 teaching and must not reserve `view` everywhere else.
    let src = "\
fn run() {
    view :: 7
    values := [1, 2, 3]
    window :: values.view(0..2)
    print(view + window.len())
}
";
    assert_fmt_stable(src, "retired view keyword as ordinary identifier");
}

#[test]
fn fmt_place_access_stability() {
    // D-SHAPE-PLACE1=A: formatting once must retain every bare/read, `&` write,
    // and `~` owned-copy token; formatting twice must be byte-identical.
    let src = "\
fn run() {
    values := [1, 2, 3, 4]
    read :: values[0..1]
    edit :: &values[2..3].sort()
    owned :: ~values[0..1]
    print(\"{read.len()},{edit.len()},{owned.len()}\")
}
";
    assert_fmt_stable(src, "place access");
}

#[test]
fn fmt_preserves_uninit_sentinel() {
    // D-UNINIT-SENTINEL2: formatter special-cases `b.uninit` and prints
    // `Type.{ uninit }` from the binding type (init AST is a never-evaluated
    // placeholder). Round-trip must stay byte-identical.
    let src = "\
use core.mem

fn run() {
    n := Int.{ uninit }
    n = 99
    print(n)
}
";
    assert_fmt_stable(src, "uninit sentinel binding");
}

#[test]
fn fmt_preserves_computed_fields() {
    // D-FIELDPOL1 (card #181): `name: T => expr` — a computed field — must
    // survive the round-trip byte-for-byte, including a formula that
    // references another computed field (own-CLAUDE-memory rule: new syntax
    // needs a formatter round-trip test, not just a parser).
    let src = "\
struct Stats {
    strength: Int
    gear_mod: Int
    attack: Int => strength * 2 + gear_mod
    threat: Int => attack + gear_mod
}

fn run() {
    s := Stats.{strength: 10, gear_mod: 3}
    print(\"attack: {s.attack}\")
}
";
    assert_fmt_stable(src, "computed fields");
}

#[test]
fn fmt_preserves_inline_contracts() {
    // D-METHODMACRO1=A: `#Inline`/`#Inline(Always)` precede `pub`/`fn` on a free
    // function and on a method — both must round-trip byte-for-byte (own-
    // CLAUDE-memory rule: new syntax needs a formatter round-trip test, not
    // just a parser).
    let src = "\
#Inline fn square(x: Int) => Int {
    return x * x
}

#Inline(Always) fn double(x: Int) => Int {
    return x * 2
}

struct Meters {
    value: Int

    #Inline(Always) fn plus(self, other: Int) => Int {
        return self.value + other
    }
}

fn run() {
    m :: Meters.{value: 7}
    print(\"{square(4)} {double(5)} {m.plus(3)}\")
}
";
    assert_fmt_stable(src, "inline contracts");
}

#[test]
fn fmt_preserves_unsafe_site_modes_and_postfix_obligations() {
    let src = "\
use core.mem

#Unsafe(\"caller keeps address live\", obligations: .Track)
fn read(address: Int) => Int {
    pointer :: mem.Ptr<Int>.from_addr(address)
    assert valid_ptr, aligned
    value :: mem.volatile_read(pointer)
    assert valid_ptr, aligned
    return value
}

fn run() {
    #Unsafe(\"calling audited reader\", obligations: .Skip) {
        print(read(1))
    }
}
";
    assert_fmt_stable(src, "unsafe obligations");
}

// ── D-WEBDEFAULT1 / D-HTMLPAIR1 / D-OSTARGET1 formatter round-trip (c134 Phase 9) ──
//
// Formatter round-trip is required for new syntax, not optional (house
// lesson: a past miss here silently corrupted syntax for months). Before
// this fix, `#Target(Web)` / `#HTML(...)` / `#Target(OS.*)` markers were
// silently DROPPED by `jet fmt` — no error, just gone.

#[test]
fn fmt_target_web_marker_stability() {
    // D-WEBDEFAULT1=A: `#Target(Web)` is a singleton file marker with no
    // captured span (same treatment as `#PubFile`) — it renders at a fixed
    // canonical position right after imports, not wherever the author
    // originally wrote it.
    let src = "use core.io as io\n#Target(Web)\n\nfn run() {\n    io.print(\"hi\")\n}\n";
    assert_fmt_stable(src, "#Target(Web) marker");
}

#[test]
fn fmt_html_marker_stability() {
    // D-HTMLPAIR1=A: `#HTML(\"path.html\")` — same marker family as
    // `#Target(Web)`, same fixed-position treatment.
    let src = "use core.ui as ui\n#[Target(Web), HTML(\"dashboard.html\")]\n\nfn run() {\n    ui.print(\"hi\")\n}\n";
    assert_fmt_stable(src, "#HTML(...) marker");
}

#[test]
fn fmt_os_target_marker_stability() {
    // D-OSTARGET1=A: `#Target(OS.Linux)` precedes the `impl` block it gates,
    // on its own line — item-scoped, not file-scoped, so (unlike the two
    // markers above) it keeps the author's own position.
    let src = "trait Backend {\n    fn label(self) => String\n}\n\nstruct LinuxBackend {\n    name: String\n}\n\n#Target(OS.Linux)\nimpl LinuxBackend.Backend {\n    fn label(self) => String {\n        return \"linux: {self.name}\"\n    }\n}\n\nfn run() {\n    print(\"hi\")\n}\n";
    assert_fmt_stable(src, "#Target(OS.Linux) marker");
}

// ── #177 §5 syntax-lock sweep: 5 formatter bugs found reformatting the full
// example tree, all fixed in this pass. Each was silent — `jet fmt` printed
// no error, it just emitted wrong or missing output.

#[test]
fn fmt_pub_file_precedes_imports() {
    // D-VISDEFAULT2: `#PubFile` carries no span (a fixed-position file
    // marker, like `#Target(Web)`), so it used to render *after* the
    // imports loop. But an import's `priv`/`pub` qualifier is chosen
    // relative to `#PubFile` being in effect — emitting the marker after the
    // import it gates produced `priv use …` with no preceding `#PubFile`,
    // which doesn't even reparse (E0413). `#PubFile` must render before
    // imports, not after.
    let src = "#PubFile\n\nuse core.io\n\nfn run() {\n    print(\"hi\")\n}\n";
    assert_fmt_stable(src, "#PubFile before imports");
}

#[test]
fn fmt_verbatim_derive_body_comment_not_duplicated() {
    // D-METADERIVE1: `derive T.Trait { … }` bodies are emitted verbatim
    // (copied straight from source) rather than walked comment-by-comment,
    // so `comment_i` never advanced past a comment living inside the derive
    // body. The next item's `emit_leading` then found that comment
    // "unconsumed" and re-emitted it a second time before `#Label struct
    // Cube` — the comment kept duplicating on every subsequent fmt pass.
    let src = "derive T.Label {\n    info :: T.reflect()\n    tname :: info.name\n    // resolves to the same value as `tname`\n    lbl :: @tname\n    emit(\"impl @lbl {{ fn label(self) => String {{ return \\\"@lbl\\\" }} }}\")\n}\n\n#Label\nstruct Cube {\n    side: Int\n}\n\nfn run() {\n    c :: Cube.{side: 5}\n    print(c.label())\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on a derive block");
    assert_eq!(
        out.matches("resolves to the same value as `tname`").count(),
        1,
        "derive-body comment must not duplicate, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "derive-body comment fmt must be idempotent");
}

#[test]
fn fmt_preserves_call_spread() {
    // D-VARIADIC1: `f(...xs)` call spread is tracked as a `CallArg.spread`
    // flag, not an `Expr::Spread` wrapper — `fmt_call_args` never checked
    // that flag, so every spread call argument silently lost its `...` on
    // the first fmt pass (a real behavior change: `join("tags:", ...parts)`
    // reformatted to `join("tags:", parts)`, which then fails to type-check).
    let src = "fn join(prefix: String, msgs: ...String) => String {\n    return [prefix, ...msgs].join(\" \")\n}\n\nfn run() {\n    parts := [\"one\", \"two\"]\n    b :: join(\"tags:\", ...parts)\n    print(b)\n}\n";
    assert_fmt_stable(src, "call-argument spread");
}

#[test]
fn fmt_preserves_trait_associated_type() {
    // D-LIB2: `fmt_trait` only ever walked `t.methods`, never
    // `t.assoc_types` — a trait's `type Elem` associated-type declaration
    // was silently dropped on every fmt pass.
    let src = "trait Indexed {\n    type Elem\n    fn at(self, i: Int) => Elem\n    fn count(self) => Int\n}\n\nstruct Nums {\n    vals: [Int]\n}\n\nimpl Nums.Indexed {\n    type Elem = Int\n\n    fn at(self, i: Int) => Int {\n        return self.vals[i]\n    }\n\n    fn count(self) => Int {\n        return self.vals.len()\n    }\n}\n\nfn run() {\n    n :: Nums.{vals: [10, 20, 30]}\n    print(n.at(0))\n    print(n.count())\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on trait assoc types");
    assert!(
        out.contains("type Elem\n"),
        "trait's `type Elem` associated-type declaration must not be dropped, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "trait assoc-type fmt must be idempotent");
}

#[test]
fn fmt_preserves_pure_callback_bound_sigil() {
    // D-EFF2 / D-MARKER-FAMILY1: the empty callback effect bound renders as
    // `#Pure ` (a contract marker, `@`-plane) but the code wrote the literal
    // The effect row belongs inside the callable arrow and must survive the
    // first formatting pass unchanged.
    let src = "fn transform(items: [Int], f: fn(Int) =[]=> Int) => [Int] {\n    return items.map((x) => f(x))\n}\n\nfn run() {\n    doubled :: transform([1, 2, 3], (n: Int) => n * 2)\n    print(\"{doubled}\")\n}\n";
    assert_fmt_stable(src, "#Pure callback bound");
}

#[test]
fn fmt_preserves_dotted_effect_paths() {
    // D-EFFTREE1: an effect-list entry may now be a dotted path (`FS.Read`,
    // `Net.HTTP.Get`) — a leaf under one of the ten D-EFF4/5 roots. The name
    // is stored as one opaque string end to end, so fmt needs no new emission
    // logic; this pins that the dot survives every printer that touches an
    // effect list (`#(…)` bounds, prohibitions, `#Caps`/`#Grant` regions).
    let src = "fn load(path: String) =[FS.Read]=> String {\n    return path\n}\n\nfn archive(path: String) =[FS.Write]=> {\n    print(path)\n}\n\nfn read_only(path: String) =[FS.Read, !FS.Write]=> {\n    load(path)\n}\n\nfn boot() =[FS]=> {\n    load(\"app.conf\")\n    archive(\"out.tar\")\n    #Caps(Net.HTTP.Get) {\n        print(\"net\")\n    }\n    #Grant(caps: FS.Read) {\n        load(\"app.conf\")\n    }\n}\n";
    assert_fmt_stable(src, "dotted effect paths (D-EFFTREE1)");
}

#[test]
fn fmt_preserves_effect_leaf_declarations() {
    let src =
        "effect Log.Audit\n\neffect Metrics.Emit\n\nfn run() =[Log.Audit]=> {}\n";
    let once = jet::format_source(src).expect("effect declarations should format");
    assert!(once.contains("effect Log.Audit"));
    assert!(once.contains("effect Metrics.Emit"));
    let twice = jet::format_source(&once).expect("formatted effect declarations should parse");
    assert_eq!(once, twice, "effect declaration formatting must be stable");
}

#[test]
fn fmt_preserves_int_literal_radix() {
    // S34/S67: `0x`/`0o`/`0b` prefixes and `_` digit separators are ratified
    // author-facing spelling. fmt used to re-emit every integer literal from
    // its AST value — rewriting `0x2a` to `42` and `1_000_000` to `1000000`,
    // destroying the radix the author chose (same failure class as a dropped
    // token; caught decimalizing examples/features/parsing/binary-reader.jet
    // and the crypto examples' key material).
    let src = "fn run() {\n    packet :: [0x2a, 0x00, 0xFF, 0o17, 0b1010, 116]\n    big :: 1_000_000\n    print(\"{packet.len()} {big}\")\n}\n";
    assert_fmt_stable(src, "int literal radix");
}

#[test]
fn fmt_preserves_web_partition_markers() {
    // D-WASM1, respelled by D-MARK-TARGET1=A (ratified 2026-07-11, card
    // #498): `#Target(JS)` / `#Target(Wasm)` / `#WasmExport` per-function
    // partition overrides, each on its own line before `fn`. fmt dropped the
    // marker entirely (Func.web_marker was never re-emitted) — every
    // browser-side function silently fell back to the Wasm bucket, breaking
    // the cross-partition checks: web_showcase_dashboard_roundtrip and
    // web_compute_wasm_bridge_roundtrip in tests/web_build.rs went red after
    // the #177 §5 tree reformat.
    let src = "#Target(JS)\nfn render_stat(label: String) => String {\n    return \"<div>{label}</div>\"\n}\n\n#Target(Wasm)\nfn crunch(n: Int) => Int {\n    return n * n\n}\n\n#WasmExport\nfn bridge_total(n: Int) => Int {\n    return crunch(n)\n}\n\nfn run() {\n    print(\"{bridge_total(4)}\")\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on web partition markers");
    for tag in ["#Target(JS)\n", "#Target(Wasm)\n", "#WasmExport\n"] {
        assert!(
            out.contains(tag),
            "fmt must keep the `{}` partition marker, got:\n{out}",
            tag.trim_end()
        );
    }
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "web partition marker fmt must be idempotent");
}

#[test]
fn fmt_preserves_inline_dep_version() {
    // U11 (D-JPK-SCRIPTDEP1=A): `use pkg#version;` — the version selector
    // must round-trip (fmt_import only re-emits it if
    // `ImportDecl::inline_version` survives the parse → fmt path). Imports
    // don't render a trailing `;` (same as an ordinary `use core.mem`
    // import — the canonical form drops the optional semicolon), so the
    // no-semicolon spelling is what's already stable.
    let src = "use textkit#1.4.2\n\nfn run() {\n    print(\"hi\")\n}\n";
    assert_fmt_stable(src, "inline dep version (3-part)");

    // A loose two-part selector (the common rung-0 spelling) round-trips too.
    let loose = "use textkit#1.4\n\nfn run() {\n    print(\"hi\")\n}\n";
    assert_fmt_stable(loose, "inline dep version (loose)");

    // The written-with-semicolon spelling still parses and fmt drops the
    // redundant `;`, same as it does for every other `use` import.
    let with_semi = "use textkit#1.4.2;\n\nfn run() {\n    print(\"hi\")\n}\n";
    let out = jet::format_source(with_semi).expect("fmt should accept a trailing `;`");
    assert!(
        out.contains("use textkit#1.4.2\n") && !out.contains("use textkit#1.4.2;"),
        "fmt should canonicalize away the optional `;`, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "inline dep version with dropped `;` must be idempotent");
}

#[test]
fn fmt_preserves_maturity_tags() {
    // D-MARK-META1=B: maturity fields are doc-only but
    // must round-trip (fmt STABILITY — not just accept-without-crash).
    let src = "\
#Meta(maturity: .Experimental)
fn experimental_label() => String {
    return \"exp\"
}

#Meta(maturity: .Tested)
fn tested_label() => String {
    return \"tested\"
}

#Meta(maturity: .Hardened)
fn hardened_label() => String {
    return \"hard\"
}

fn run() {
    print(experimental_label())
    print(tested_label())
    print(hardened_label())
}
";
    assert_fmt_stable(src, "maturity metadata (D-MARK-META1=B)");
}

#[test]
fn fmt_preserves_maturity_tags_next_line() {
    // Metadata on the preceding line parses and round-trips canonically.
    let src = "\
#Meta(maturity: .Experimental)
fn experimental_label() => String {
    return \"exp\"
}

fn run() {
    print(experimental_label())
}
";
    let out = jet::format_source(src).expect("next-line maturity metadata should parse");
    assert!(
        out.contains("#Meta(maturity: .Experimental)"),
        "fmt must not drop maturity metadata; got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("re-fmt");
    assert_eq!(out, twice, "maturity next-line canonicalize must be idempotent");
}

#[test]
fn fmt_preserves_schedule_markers() {
    // D-SCHEDULE1 (ratified 2026-07-11, card #505): `#Job`/`#Every(…)` must
    // round-trip byte-for-byte (fmt STABILITY, not just accept-without-crash)
    // — same inline-marker convention as `#Reactive`/`#Scrub(Input)`/
    // `#Replayable`/`#State(…)` (one space-separated line before `fn`).
    let src = "\
#[Job, Doc(\"Prune old sessions\"), Every(5min)] fn prune_sessions() {
    print(\"pruning\")
}

#[Job, Every(\"03:00\")] fn nightly_backup() {
    print(\"backing up\")
}

#Job fn manual_only() {
    print(\"manual\")
}

fn run() {
    prune_sessions()
    nightly_backup()
    manual_only()
}
";
    assert_fmt_stable(src, "#Job/#Doc/#Every task markers (D-TASKS-LIST1)");
}

#[test]
fn fmt_preserves_per_function_c_abi() {
    let src = "use c.demo as c\n#Extern module c.demo {\n    #ABI(system) fn portable(x: I32) => I32 = \"portable\"\n    #ABI(sysv64) fn native(x: I32) => I32 = \"native\"\n}\nfn run() {}\n";
    let once = jet::format_source(src).expect("#ABI C module should format");
    assert!(once.contains("#ABI(system)") && once.contains("#ABI(sysv64)"), "fmt dropped #ABI: {once}");
    assert_eq!(once, jet::format_source(&once).expect("re-fmt"));
}


#[test]
fn fmt_preserves_casing_errors_for_sema() {
    let src = "struct bad_type { BadField: Int }\nfn BadFunction() {}\n";
    let once = jet::format_source(src).expect("casing is a sema diagnostic, not a formatter rewrite");
    assert!(once.contains("bad_type") && once.contains("BadField") && once.contains("BadFunction"));
    assert_eq!(once, jet::format_source(&once).expect("re-fmt"));
}
#[test]
fn generic_modules_roundtrip_templates_symbolic_lengths_nested_items_and_alias_chains() {
    let src = r#"module ring<T, capacity: Int, label: String> {
#Meta(category: label)
@size :: capacity
pub struct Buffer { slots: [T#capacity] }
module nested<U> { pub fn keep(value: U) => U { return ~value } }
module inner = nested<T>
#Meta(category: label)
pub fn adjusted() => Int { return capacity + 1 }
}
module a = ring<Int, 2 + 2, "ring">
module b = a
fn run() {}
"#;
    let once = jet::format_source(src).expect("generic module format");
    let twice = jet::format_source(&once).expect("formatted generic module reparses");
    assert_eq!(once, twice);
    for preserved in [
        "module ring<T, capacity: Int, label: String>",
        "#Meta(category: label)",
        "[T#capacity]",
        "module nested<U>",
        "capacity + 1",
        "ring<Int, 2 + 2, \"ring\">",
        "module b = a",
    ] {
        assert!(once.contains(preserved), "formatter lost `{preserved}`:\n{once}");
    }
}

#[test]
fn subjectless_guards_preserve_tokens_and_are_byte_stable() {
    // D-IFGUARD1=A + D-ARROW-CONTROL1=A: effect-only `if` has no arrow and
    // keeps its mandatory body braces. Guard-table and value arms keep arrows.
    let src = r#"fn run() {
    ready :: true
    if ready { print("inline") }
    if {
        ready -> print("ready")
        false -> print("never")
    }
    label :: if {
        ready -> "ready"
        else -> "waiting"
    }
    print(label)
}
"#;
    let once = jet::format_source(src).expect("subjectless guards should format");
    assert_eq!(once, src, "concise subjectless guards should stay byte-stable");
    for preserved in [
        "if ready { print(\"inline\") }",
        "if {",
        "ready ->",
        "false ->",
        "else -> \"waiting\"",
        "print(label)",
    ] {
        assert!(once.contains(preserved), "formatter lost `{preserved}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("formatted guards should reparse");
    assert_eq!(once, twice, "subjectless guard formatting must be byte-stable");
}

#[test]
fn overwide_inline_guard_widens_once() {
    let src = "fn run() {\n    if this_condition_name_is_deliberately_longer_than_the_formatter_width_limit && another_deliberately_long_condition_name print(\"wide\")\n}\n";
    let out = jet::format_source(src).expect("format guard");
    assert_eq!(
        out,
        "fn run() {\n    if this_condition_name_is_deliberately_longer_than_the_formatter_width_limit && another_deliberately_long_condition_name {\n        print(\"wide\")\n    }\n}\n"
    );
    assert_eq!(jet::format_source(&out).expect("format guard twice"), out);
}

#[test]
fn arrow_in_block_if_comment_does_not_create_inline_guard() {
    let src = "fn run() {\n    ready :: true\n    if ready { // -> explains the branch\n        print(\"ready\")\n    }\n}\n";
    let once = jet::format_source(src).expect("format ordinary block if");
    assert!(once.contains("if ready {"), "{once}");
    assert!(once.contains("// -> explains the branch"), "{once}");
    assert!(!once.contains("if ready ->"), "{once}");
    assert_eq!(jet::format_source(&once).expect("format twice"), once);
}
#[test]
fn fmt_output_callable_stability() {
    let source = "app: Output :: .Executable.{ name: \"demo\", entry: start };\n\nfn start() {}\n";
    let once = jet::format_source(source).expect("typed Output should format");
    for token in [
        "app: Output ::",
        ".Executable.{",
        "name: \"demo\"",
        "entry: start",
        ";",
    ] {
        assert!(once.contains(token), "fmt dropped Output token `{token}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("formatted Output should reparse");
    assert_eq!(once, twice, "Output callable formatting must be byte-stable");
}

#[test]
fn fmt_preserves_parameter_zones_and_public_labels() {
    // D-APILABEL1=A: `/` closes the positional-only zone, `*` opens the
    // label-only zone, and `timeout seconds: Int` splits the public call label
    // from the local name. All three must round-trip byte-for-byte (fmt
    // STABILITY — idempotence alone would not notice a dropped separator,
    // because a dropped one stays dropped on the second pass).
    let src = "fn connect(host: String, /, *, timeout seconds: Int = 30, tls: Bool = true) => String = host\n";
    let once = jet::format_source(src).expect("fmt should accept parameter zones");
    for token in ["host: String", ", /,", ", *,", "timeout seconds: Int = 30", "tls: Bool = true"] {
        assert!(once.contains(token), "fmt dropped `{token}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("zoned parameters should re-fmt");
    assert_eq!(once, twice, "parameter-zone formatting must be byte-stable");
}

#[test]
fn fmt_keeps_reordered_and_skipped_argument_labels() {
    // D-APILABEL1=A: a label binds by name, so fmt must never reorder a call
    // back into declaration order nor add a label the author omitted.
    let src = "fn run() {\n    a :: connect(\"db\", tls: false, timeout: 5)\n    b :: connect(\"db\", tls: false)\n}\n";
    let once = jet::format_source(src).expect("fmt should accept reordered labels");
    assert!(
        once.contains("connect(\"db\", tls: false, timeout: 5)"),
        "fmt reordered or relabelled the call:\n{once}"
    );
    assert!(
        once.contains("connect(\"db\", tls: false)"),
        "fmt filled in a skipped default:\n{once}"
    );
    let twice = jet::format_source(&once).expect("labelled call should re-fmt");
    assert_eq!(once, twice, "argument-label formatting must be byte-stable");
}

#[test]
fn fmt_keeps_power_and_exclusive_or_spellings() {
    // D-EXPOP1=A / D-XORSPELL1=A: `^` is the power, `~|` is exclusive-or, and
    // both compounds must survive a round trip byte-for-byte (fmt STABILITY —
    // idempotence alone would not notice a dropped sigil, because a dropped
    // one stays dropped on the second pass).
    let src = "fn run() {\n    a :: 2 ^ 3 ^ 2\n    b :: -3 ^ 2\n    c :: (2 ^ 3) ^ 2\n    d :: (-3) ^ 2\n    e :: 2 ^ -1\n    f :: 12 ~| 10\n    g := 2\n    g ^= 10\n    h := 12\n    h ~|= 10\n    print(\"{a}{b}{c}{d}{e}{f}{g}{h}\")\n}\n";
    let once = jet::format_source(src).expect("fmt should accept power and exclusive-or");
    for token in [
        "2 ^ 3 ^ 2",
        "-3 ^ 2",
        "(2 ^ 3) ^ 2",
        "(-3) ^ 2",
        "2 ^ -1",
        "12 ~| 10",
        "g ^= 10",
        "h ~|= 10",
    ] {
        assert!(once.contains(token), "fmt dropped `{token}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("power and exclusive-or should re-fmt");
    assert_eq!(once, twice, "power formatting must be byte-stable");
}

#[test]
fn fmt_keeps_the_division_family_spellings() {
    // D-FLOORDIV1=A / D-MODSEM1=A: `/%` rounds down, `%` is the floored modulo,
    // `%%` is the truncated remainder, and each has a compound. All six must
    // survive a round trip byte-for-byte (fmt STABILITY — idempotence alone
    // would not notice a dropped sigil, because a dropped one stays dropped on
    // the second pass, and `/%` in particular could silently degrade to `/`).
    let src = "fn run() {\n    a :: 7 /% 2\n    b :: -7 /% 2\n    c :: 7.5 /% 2.0\n    d :: -7 % 2\n    e :: -7 %% 2\n    f :: 20 /% 3 /% 2\n    g :: 2 * 7 /% 4\n    h := 9\n    h /%= 2\n    i := -3\n    i %= 5\n    j := -7\n    j %%= 5\n    print(\"{a}{b}{c}{d}{e}{f}{g}{h}{i}{j}\")\n}\n";
    let once = jet::format_source(src).expect("fmt should accept the division family");
    for token in [
        "7 /% 2",
        "-7 /% 2",
        "7.5 /% 2.0",
        "-7 % 2",
        "-7 %% 2",
        "20 /% 3 /% 2",
        "2 * 7 /% 4",
        "h /%= 2",
        "i %= 5",
        "j %%= 5",
    ] {
        assert!(once.contains(token), "fmt dropped `{token}`:\n{once}");
    }
    let twice = jet::format_source(&once).expect("the division family should re-fmt");
    assert_eq!(once, twice, "division formatting must be byte-stable");
}

/// D-MARK-FORM1=A: parentheses appear exactly when arguments are written, so
/// `jet fmt` deletes an empty pair and the canonical spelling round-trips
/// byte-for-byte (fmt STABILITY, not just accept-without-crash).
#[test]
fn fmt_empty_marker_parentheses_canonicalize_and_are_stable() {
    let src = r#"#Inline()
fn double(x: Int) => Int {
    return x * 2
}

#Job()
fn refresh() {
    print("refresh")
}
"#;
    let out = jet::format_source(src).expect("fmt should recover from empty marker parentheses");
    assert!(
        out.contains("#Inline fn double") || out.contains("#Inline\nfn double"),
        "fmt kept empty parentheses on `#Inline`, got:\n{out}"
    );
    assert!(
        !out.contains("#Inline()") && !out.contains("#Job()"),
        "fmt kept an empty marker parameter list, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("canonical marker output should re-fmt");
    assert_eq!(out, twice, "marker placement formatting must be stable");
}

/// D-MARK-FORM1=A: the canonical spellings of every placement the retired
/// five forms used to distinguish are one shape, and each survives fmt
/// unchanged.
#[test]
fn fmt_one_placement_law_round_trips_every_target_kind() {
    let src = r#"#Codable
struct Widget {
    #Doc("display name") label: String
}

@limit :: 32

#Inline
fn hot(a: Int) => Int {
    return a * limit
}

fn run() {
    #Off print("off")
    #Impure("reads the wall clock") {
        print("now")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept every marker placement");
    for needle in [
        "#Codable",
        "#Doc(\"display name\") label: String",
        "@limit :: 32",
        "#Inline",
        "#Off print(\"off\")",
        "#Impure(\"reads the wall clock\") {",
    ] {
        assert!(out.contains(needle), "fmt dropped `{needle}`, got:\n{out}");
    }
    let twice = jet::format_source(&out).expect("marker placements should re-fmt");
    assert_eq!(out, twice, "marker placement formatting must be stable");
}
