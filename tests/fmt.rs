//! M6 phase 1: `jet fmt` idempotence — fmt(fmt(x)) == fmt(x).

use std::fs;

#[test]
fn repr_c_enum_surface_is_stable() {
    let src = "@Layout(c, tag: U8)\nenum Packet { Ping(Int) = 3; Data(x: I32, y: I32) = 7 }\n";
    let once = jet::format_source(src).expect("C-layout enum should format");
    assert!(once.contains("@Layout(c, tag: U8)"));
    assert!(once.contains("Ping(Int) = 3"));
    assert!(once.contains("Data(x: I32, y: I32) = 7"));
    let twice = jet::format_source(&once).expect("formatted C-layout enum should parse");
    assert_eq!(once, twice, "C-layout enum formatting must be stable");
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
    // D-BINPAT1 (card #506): a `b"…"` binary pattern must survive fmt with
    // every hole intact — the bit widths, endian suffixes, and rest capture.
    // Idempotence alone can miss a dropped token, so assert the exact spelling.
    let src = "fn run() {\n    packet: [U8] :: [0x45]\n    if packet == {\n        b\"{version:U4}{ihl:U4}{tos:U8}{len:U16be}{rest:...}\" -> { print(\"ok\") }\n        else -> { print(\"no\") }\n    }\n}\n";
    let once = jet::format_source(src).expect("binary pattern should format");
    assert!(
        once.contains("b\"{version:U4}{ihl:U4}{tos:U8}{len:U16be}{rest:...}\""),
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
    let src = "fn area(width: Int, height: Int) -> Int {\n    return width * height\n}\n\nfn run() {\n    print(area(width: 3, height: 4))\n}\n";
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
    @Off print("off")
    @DebugOnly print("debug")
    @Off if true {
        print("off block")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept statement switch attributes");
    for needle in [
        "@Off print(\"off\")",
        "@DebugOnly print(\"debug\")",
        "@Off if true {",
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
    let src = r#"@Meta(category: "Movement", tunable)
fn step_speed(speed: Int) -> Int {
    @Meta(category: "Movement", tunable)
    next :: speed + 1
    return next
}
"#;
    let out = jet::format_source(src).expect("fmt should accept @Meta attributes");
    for needle in [
        "@Meta(category: \"Movement\", tunable)",
        "fn step_speed(speed: Int) -> Int {",
        "next :: speed + 1",
    ] {
        assert!(out.contains(needle), "fmt dropped `{needle}`, got:\n{out}");
    }
    let twice = jet::format_source(&out).expect("@Meta output should re-fmt");
    assert_eq!(out, twice, "@Meta formatting must be stable");
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
    @Transact(tx) {
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
fn fmt_canonicalizes_bare_question_return_to_fallible_return() {
    let src = r#"fn parse_count(raw: String) -> Int? {
    return Err("empty");
}
"#;
    let out = jet::format_source(src).expect("fmt should parse default Error return");
    assert!(
        out.contains("fn parse_count(raw: String) -> Int ? {"),
        "expected `Int?` return to format as `Int ?`, got:\n{out}"
    );
}

#[test]
fn fmt_comptime_os_dispatch_round_trips() {
    // D-OSTARGET2=B (ratified 2026-07-03): `comptime if build.os == { … }` — the
    // OS-target dispatch. New token shape: the `comptime if <subject> == { }`
    // dispatch. Must survive fmt (subject + arms + bodies preserved) and be
    // idempotent (the formatter-round-trip-required rule catches dropped tokens).
    let src = r#"fn run() {
    comptime if build.os == {
        .Linux -> {
            b :: LinuxBackend.{ name: "gtk" }
            print(b.label())
        }
        .Macos -> print("mac")
        else -> print("other")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept a comptime OS dispatch");
    assert!(
        out.contains("comptime if build.os == {"),
        "expected the `comptime if build.os == {{` dispatch head, got:\n{out}"
    );
    // Arms and their bodies survive (the formatter canonicalizes braceless arm
    // bodies to block form, like any `if … == { }` dispatch).
    assert!(
        out.contains(".Linux -> {") && out.contains(".Macos -> {"),
        "expected OS arms preserved, got:\n{out}"
    );
    assert!(
        out.contains("print(\"mac\")") && out.contains("print(b.label())"),
        "expected arm bodies preserved, got:\n{out}"
    );
    assert!(
        out.contains("else -> {") && out.contains("print(\"other\")"),
        "expected the else arm preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("comptime OS dispatch output should re-fmt");
    assert_eq!(
        out, twice,
        "comptime OS dispatch formatting must be idempotent"
    );
}

#[test]
fn fmt_if_expression_preserves_condition_parens() {
    // S68 (D-SG2): `if` as a value round-trips.
    // D-FMTPARENS1=A: author-written grouping parens are always preserved.
    let src = r#"fn run() {
    m :: if (a > b) {
        a
    } else {
        b
    }
    if (a > b) {
        print("hi")
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should accept an if-expression");
    assert!(
        out.contains("m :: if (a > b) {"),
        "expected parens preserved in if-expression condition, got:\n{out}"
    );
    assert!(
        out.contains("    if (a > b) {"),
        "expected parens preserved in statement-if condition, got:\n{out}"
    );
    assert!(
        out.contains("} else {"),
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
    let src = r#"fn bounds() -> (min: Int, max: Int) {
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
        out.contains("-> (max: Int, min: Int)"),
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
    let src = r#"fn shell() -> [JSON] {
    return [
        JSON.Null;
    ];
}

fn use_collections(items: [String], counts: [String: Int]) {}
"#;
    let out = jet::format_source(src).expect("fmt should accept collection type sugar");
    assert!(
        out.contains("fn shell() -> [JSON]"),
        "expected list return shorthand, got:\n{out}"
    );
    assert!(
        out.contains("items: [String], counts: [String: Int]"),
        "expected bracket collection type formatting, got:\n{out}"
    );
    assert!(
        out.contains("return [JSON.Null]"),
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
    c: Color = .Red
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
    let src = r#"fn f(flag: Bool) -> Int ? String {
    maybe: Int? :: .Val(1)
    empty: Int? :: .None
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
fn fmt_preserves_named_payload_variant_dot_brace() {
    // D-UITREE1/D-DOTCTOR1: `.Variant.{ field: val }` — named-payload enum
    // construction reuses the struct dot-brace spelling. Must round-trip
    // byte-for-byte (fmt STABILITY, not just accept-without-crash).
    let src = r#"enum View {
    Text(text: String)
    Box(width: Int)
}
fn run() {
    a: View :: .Text.{ text: "hi" }
    b: View :: .Box.{ width: 10 }
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
    // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…{hole:U<w>}…")`
    // — the byte-mode sibling of `fmt_preserves_take_pattern_literal` above.
    // Must round-trip byte-for-byte (fmt STABILITY, not just accept-without-crash).
    let src = r#"fn run() {
    header: [U8] :: [69, 0, 0, 40]
    r :: Reader.over(header)
    parsed :: r.take_pattern(b"{version:U4}{ihl:U4}{tos:U8}{len:U16be}") ?? panic("no match")
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
// A brace body the author wrote on one line stays one line when it holds one
// simple statement, has no inner comment, and fits width 100; a body broken
// across lines stays multiline. fmt only normalizes spacing within the shape
// the author chose. Idempotent (second pass == first), not canonical.

/// Assert `src` formats to itself byte-for-byte and is idempotent.
fn assert_fmt_stable(src: &str, label: &str) {
    let out = jet::format_source(src).unwrap_or_else(|d| {
        panic!(
            "fmt failed for {label}:\n{}",
            jet::render_diagnostics(label, src, &d)
        )
    });
    assert_eq!(
        out, src,
        "{label}: fmt changed the source\n--- got ---\n{out}"
    );
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "{label}: fmt is not idempotent");
}

#[test]
fn fmt_shield_block_stability() {
    let src = "fn run() {\n    @Shield {\n        print(\"committed\")\n    }\n}\n";
    assert_fmt_stable(src, "@Shield block");
}

#[test]
fn fmt_preserves_single_line_if() {
    // A one-line `if` body the author placed inline survives unchanged.
    let src = "fn run() {\n    ready :: true\n    if ready { launch() }\n}\n";
    assert_fmt_stable(src, "single-line if");
}

#[test]
fn fmt_preserves_multiline_if() {
    // The 3-line form the author chose stays 3 lines.
    let src = "fn run() {\n    ready :: true\n    if ready {\n        launch()\n    }\n}\n";
    assert_fmt_stable(src, "multiline if");
}

#[test]
fn fmt_if_else_chain_one_multiline_expands_all() {
    // D-FMT1 chain rule: if any branch is multiline, the whole chain expands.
    // Author wrote `then` inline but `else` multiline → whole chain goes
    // multiline; the expanded form is then stable.
    let src = "fn run() {\n    if a { x() } else {\n        y()\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should accept the mixed chain");
    assert_eq!(
        out, "fn run() {\n    if a {\n        x()\n    } else {\n        y()\n    }\n}\n",
        "mixed if/else chain should expand wholesale, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("expanded chain should re-fmt");
    assert_eq!(out, twice, "expanded chain must be idempotent");
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

    let for_src = "fn run() {\n    loop i in 0..3 { print(\"{i}\") }\n}\n";
    assert_fmt_stable(for_src, "single-line for/loop");

    let fn_src = "fn one() -> Int { return 1 }\n";
    assert_fmt_stable(fn_src, "single-line fn body");
}

#[test]
fn fmt_preserves_single_line_if_expr_branch() {
    // If-expression branches (routed through fmt_value_block) follow the rule.
    let src = "fn run() {\n    a :: true\n    n :: if a { 1 } else { 2 }\n    print(\"{n}\")\n}\n";
    assert_fmt_stable(src, "single-line if-expression");
}

// ── Marker / turbofish round-trip survival (the c-fmt data-loss regression) ──
//
// `fmt_is_idempotent_on_examples` only checks that fmt(fmt(x)) == fmt(x); it does
// NOT catch a formatter that silently drops a token on the FIRST pass (the lost
// `@[Rename]`/`@Codable`/turbofish bug). These tests assert that the load-bearing
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
    d: Damage := Damage.Fire.Burn
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
    // The exact c-fmt regression: `@Codable` (contract plane) / `@[Rename("…")]`
    // (directive/serde plane, D-MARKERMOVE1=B), and a method-call turbofish
    // `decode<Order>` must all survive.
    let src = "\
@Codable
struct Order {
    id: Int
    @[Rename(\"customer\")] who: String
}

fn run() {
    raw :: \"x\"
    back :: json.decode<Order>(raw) ?? panic(\"bad\")
    print(back.who)
}
";
    assert_fmt_keeps(
        src,
        &["@Codable", "@[Rename(\"customer\")]", "decode<Order>"],
        "codable + rename + turbofish",
    );
}

#[test]
fn fmt_keeps_cli_doc_and_default_field_markers() {
    // D-SHAPE2: every field rule shares one `@[…]` group.
    let src = "\
@Cli
struct ServeArgs {
    @[Doc(\"port to listen on\"), Default(3000)] port: Int
    @[Doc(\"print extra detail\")] verbose: Bool
}

fn run(args: ServeArgs) {
    print(args.port)
    print(args.verbose)
}
";
    assert_fmt_keeps(
        src,
        &["@[Doc(\"port to listen on\"), Default(3000)]", "@Cli"],
        "cli @[Doc] + @[Default] field markers",
    );
    assert_fmt_stable(src, "cli doc/default field markers");
}

#[test]
fn fmt_keeps_container_rename_all() {
    // Derive and container serde rules share one applied-rule group.
    let src = "\
@Codable
@RenameAll(camel)
struct Person {
    full_name: String
}
";
    assert_fmt_keeps(
        src,
        &["@[Codable, RenameAll(camel)]"],
        "container RenameAll",
    );
}

#[test]
fn fmt_keeps_layout_columnar_and_codable() {
    // `@[Codable]` then `@Layout(columnar)` on the same struct — both survive,
    // neither is rewritten into body `derive` lines.
    let src = "\
@[Codable]
@Layout(columnar)
struct Particle {
    x: Float
}
";
    assert_fmt_keeps(
        src,
        &["@[Codable]", "@Layout(columnar)"],
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
fn double(n: Int) --[]-> Int {
    return (n * 2)
}
";
    assert_fmt_stable(fn_src, "@Pure fn round-trip");

    let struct_src = "\
@[Codable]
@Layout(columnar)
struct Particle {
    x: Float
}
";
    assert_fmt_stable(struct_src, "@Codable + @Layout struct round-trip");
}

#[test]
fn fmt_stable_effect_arrows() {
    let src = "\
fn load(path: String) --[Fs.Read, ..E]-> String {
    return path
}

fn visit(callback: fn(Int) --[Io]->) --[via callback]-> {
    callback(1)
}

// `--[]->` replaces the retired `@Pure` marker without moving this comment.
fn hash(n: Int) --[]-> Int {
    return n
}
";
    assert_fmt_stable(src, "D-SHAPE8 effect arrows");
}

#[test]
fn fmt_keeps_single_use_marker() {
    let src = "\
@SingleUse struct Lock {
    resource: String
}
";
    assert_fmt_keeps(src, &["@SingleUse struct Lock"], "single-use struct");
}

#[test]
fn fmt_keeps_layout_c_struct() {
    let src = "\
@Layout(c)
struct Header {
    magic: Int
}
";
    assert_fmt_keeps(src, &["@Layout(c)"], "layout c struct");
}

#[test]
fn fmt_keeps_body_derive_line_when_no_brackets() {
    // A struct that uses ONLY a body `derive Comparable` line (no `#[…]` list)
    // must keep emitting it in the body — the new bracket path must not eat it.
    let src = "\
struct Score {
    points: Int

    derive Comparable
}
";
    assert_fmt_keeps(src, &["derive Comparable"], "body derive line");
    // And it must NOT be promoted to a bracket marker (contract plane: `@[Comparable]`).
    let out = jet::format_source(src).unwrap();
    assert!(
        !out.contains("@[Comparable]") && !out.contains("@Comparable"),
        "body derive must not become bracket:\n{out}"
    );
}

#[test]
fn fmt_keeps_enum_variant_rename_and_tag() {
    // Enum derive + container rule share one group; the variant keeps its group.
    let src = "\
@Codable
@Tag(\"type\")
enum Shape {
    @[Rename(\"circle\")] Circle(Float)
    Square(Float)
}
";
    assert_fmt_keeps(
        src,
        &["@[Codable, Tag(\"type\")]", "@[Rename(\"circle\")]"],
        "enum tag + variant rename",
    );
}

#[test]
fn fmt_keeps_replayable_marker() {
    let src = "\
@Replayable fn replay_turn(seed: Int) -> Int {
    return seed + 1
}
";
    assert_fmt_keeps(src, &["@Replayable fn replay_turn"], "replayable fn");
}

#[test]
fn fmt_keeps_typestate_markers() {
    // D-STATE1: the `@State(S)` require-state guard and `@Transition(From -> To)`
    // transition markers (including the entry form `_ -> To`) must survive fmt —
    // dropping a typestate contract would silently change what the checker enforces.
    let src = "\
struct Reservation {
    guest: String

    @Transition(_ -> Pending) fn book(guest: String) -> Reservation {
        return Reservation.{guest: guest}
    }

    @Transition(Pending -> Confirmed) fn pay(self: ^Reservation) -> Reservation {
        return self
    }

    @State(Confirmed) fn check_in(self) {
        print(self.guest)
    }
}
";
    assert_fmt_keeps(
        src,
        &[
            "@Transition(_ -> Pending)",
            "@Transition(Pending -> Confirmed)",
            "@State(Confirmed)",
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
    // D-CTEFFECT1: `@Impure("reason") { … }` must survive a format round-trip
    // with the reason string and body intact.
    let src = "fn run() {\n    @Impure(\"reading build config\") {\n        print(\"inside\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on @Impure block");
    assert!(
        out.contains("@Impure(\"reading build config\")"),
        "@Impure reason dropped by fmt:\n{out}"
    );
    let twice = jet::format_source(&out).expect("@Impure fmt must re-fmt");
    assert_eq!(out, twice, "@Impure fmt must be idempotent");
}

#[test]
fn fmt_impure_block_no_reason_round_trips() {
    // D-CTEFFECT1: `@Impure { … }` without a reason also round-trips (triggers
    // L3102 lint but is parseable).
    let src = "fn run() {\n    @Impure {\n        print(\"inside\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on @Impure block without reason");
    assert!(
        out.contains("@Impure {") || out.contains("@Impure{"),
        "@Impure block without reason dropped by fmt:\n{out}"
    );
    let twice = jet::format_source(&out).expect("@Impure (no reason) fmt must re-fmt");
    assert_eq!(out, twice, "@Impure (no reason) fmt must be idempotent");
}

#[test]
fn fmt_unsafe_reasons_escape_strings() {
    // D-UNSAFE-REASON1=B: unsafe block/function reasons are normal string
    // literals, so fmt must preserve quotes/backslashes as parseable Jet.
    let src = "use core.mem\n\n@Unsafe(\"caller says \\\"ok\\\"\") fn raw() -> Int {\n    return 1\n}\n\nfn run() {\n    @Unsafe(\"path C:\\\\tmp\") {\n        print(\"{raw()}\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on unsafe reasons");
    assert!(
        out.contains("@Unsafe(\"caller says \\\"ok\\\"\")")
            && out.contains("@Unsafe(\"path C:\\\\tmp\")"),
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
    // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` formatting
    // round-trips — the block keyword, brace, and body all survive a second fmt.
    let src = r#"comptime limit = 1000

fn run() {
    comptime {
        comptime ratio = limit / 10
        if ratio < 1 { panic("bad") }
    }
    print("ok")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept comptime block");
    assert!(
        out.contains("comptime {"),
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
    let src = "struct Box<T> {\n    value: T\n}\n\nimpl Box {\n    fn new(value: ^T) -> Box<T> {\n        return Box<T>.{ value: value }\n    }\n}\n\nfn run() {\n    inferred :: Box.new(1)\n    explicit :: Box<Int>.new(2)\n    expected: Box<Int> :: .new(3)\n}\n";
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

fn describe(s: Shape) -> String {
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

fn area(s: Shape) -> Float {
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
    // D-CTMARKER1=C: `$name` comptime splice is a first-class AST node
    // (Expr::ComptimeSplice). The formatter must emit it as `$name` so that
    // the round-trip is stable (previously the `$` would be silently dropped
    // if it reached the formatter without an AST node).
    let src = "derive T.Debug {\n    info :: T.reflect()\n    tname :: info.name\n    emit(\"impl $tname {{ fn tag(self) -> String {{ return \\\"ok\\\" }} }}\")\n}\n\nfn run() {\n    print(\"ok\")\n}\n";
    let once = jet::format_source(src).expect("fmt should accept derive with $name in string");
    let twice = jet::format_source(&once).expect("second fmt should succeed");
    assert_eq!(
        once, twice,
        "derive body with $name in emit string must be fmt-idempotent"
    );

    // Standalone `$name` expression (outside emit string) round-trips as `$name`.
    let splice_src = "derive T.Named {\n    tname :: \"test\"\n    x :: $tname\n    emit(\"impl $tname {{ }}\")\n}\n\nfn run() {}\n";
    let splice_once = jet::format_source(splice_src).expect("fmt should accept $name expression");
    assert!(
        splice_once.contains("$tname"),
        "`$tname` expression must survive fmt, got:\n{splice_once}"
    );
    let splice_twice = jet::format_source(&splice_once).expect("$name fmt must be idempotent");
    assert_eq!(
        splice_once, splice_twice,
        "`$name` expression must be fmt-idempotent"
    );
}

#[test]
fn fmt_impl_dot_trait_stability() {
    // D-IMPLDOT1=A: `impl Type.Trait { … }` round-trips unchanged.
    let src = "\
trait Greet {
    fn hello(self) -> String;
}

struct Foo {}

impl Foo.Greet {
    fn hello(self) -> String {
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
    // D-TESTPAREN1=A: `@Test("name") { … }` must survive fmt unchanged.
    let src = "\
fn run() {}
@Test(\"double returns twice\") {
    require_eq(2 * 2, 4)
}
";
    assert_fmt_keeps(src, &["@Test(\"double returns twice\")"], "test paren name");
    // Idempotence: formatting twice produces the same output.
    let once = jet::format_source(src).expect("fmt should accept @Test(\"name\")");
    let twice = jet::format_source(&once).expect("second fmt of @Test(\"name\") must succeed");
    assert_eq!(once, twice, "@Test(\"name\") must be fmt-idempotent");
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

@Test(\"members\") {
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
fn fmt_explicit_binding_d_bind3_stability() {
    // D-BIND4: `name: Type :: val` (immutable) and `name: Type := val` (mutable)
    // must survive fmt unchanged.
    let src = "\
fn run() {
    x: Int :: 42
    s: String := \"hi\"
    print(\"{x} {s}\")
}
";
    assert_fmt_keeps(
        src,
        &["x: Int :: 42", "s: String := \"hi\""],
        "explicit binding",
    );
    let once = jet::format_source(src).expect("fmt should accept explicit-type bindings");
    let twice = jet::format_source(&once).expect("second fmt of explicit bindings must succeed");
    assert_eq!(once, twice, "explicit-type binding fmt must be idempotent");
}

#[test]
fn fmt_preserves_track_binding_marker() {
    let src = "\
fn run() {
    @Track count :: 1
    @Track total := 2
    @Track label: String :: \"ok\"
    print(\"{count} {total} {label}\")
}
";
    assert_fmt_stable(src, "@Track binding marker");
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
    fn show(self) -> Int { return self.hp }

    fn heal(&self, amount: Int) { self.hp = self.hp + amount }
}

fn damage(p: &Player, amount: Int) {
    p.hp = p.hp - amount
}

fn archive(p: ^Player) -> Int {
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
            "fn archive(p: ^Player) -> Int",
            "damage(&p, 10)",
            "archive(^p)",
        ],
        "D-MEM1 write sigil",
    );
}

#[test]
fn fmt_no_alloc_policy_d_mem1_s7_stability() {
    // D-POLICY-WORD1=A: `@Policy(no_alloc)` is a fixed post-import
    // file marker, same treatment as `@PubFile`/`@Target(…)` — must survive
    // fmt unchanged.
    let src = "\
@Policy(no_alloc)

fn run() {
    print(\"ok\")
}
";
    assert_fmt_keeps(src, &["@Policy(no_alloc)"], "D-POLICY-WORD1 policy marker");
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

fn archive(t: ^Ticket) -> String {
    return t.label
}

fn run() {
    name: String :: \"vault\"
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
fn fmt_loop_label_d_looplabel2_stability() {
    // D-LOOPLABEL2=A: `outer@ loop { break outer@ }` must survive fmt unchanged.
    let src = "\
fn run() {
    outer@ loop i in [1, 2] {
        loop j in [1, 2] {
            if i == j {
                break outer@
            }
        }
    }
    print(\"done\")
}
";
    assert_fmt_keeps(src, &["outer@ loop", "break outer@"], "loop label suffix");
    let once = jet::format_source(src).expect("fmt should accept suffix loop labels");
    let twice = jet::format_source(&once).expect("second fmt of loop labels must succeed");
    assert_eq!(once, twice, "loop label fmt must be idempotent");
}

#[test]
fn fmt_counted_loop_d_loop_semicolon1_stability() {
    // D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` must survive fmt unchanged.
    let src = "\
fn run() {
    sum := 0
    loop i := 0; i < 5; i += 1 {
        sum += i
    }
    print(sum)
}
";
    assert_fmt_keeps(src, &["loop i := 0; i < 5; i += 1"], "counted loop header");
    let once = jet::format_source(src).expect("fmt should accept counted loop");
    let twice = jet::format_source(&once).expect("second fmt of counted loop must succeed");
    assert_eq!(once, twice, "counted loop fmt must be idempotent");
}

#[test]
fn fmt_selective_import_d_selimport1_stability() {
    // D-SELIMPORT1=A: `use mod.{a, b as c}` must survive fmt unchanged.
    let src = "\
module math {
    pub fn clamp(x: Int, lo: Int, hi: Int) -> Int {
        if x < lo { return lo }
        if x > hi { return hi }
        return x
    }
}

use math.{clamp, clamp as c2}

fn run() {
    print(clamp(15, 0, 10))
    print(c2(5, 0, 3))
}
";
    assert_fmt_keeps(
        src,
        &["use math.{clamp, clamp as c2}"],
        "selective import with alias",
    );
    let once = jet::format_source(src).expect("fmt should accept selective imports");
    let twice = jet::format_source(&once).expect("second fmt of selective imports must succeed");
    assert_eq!(once, twice, "selective import fmt must be idempotent");
}

#[test]
fn fmt_value_tag_type_d_qual4_stability() {
    // D-QUAL4=A: `#Marker T` in type position must survive fmt unchanged.
    let src = "\
fn process(input: @Tainted String) -> String {
    return \"{input}-clean\"
}

fn run() {
    result :: process(\"hello\")
    print(result)
}
";
    assert_fmt_keeps(src, &["@Tainted String"], "value-tag type qualifier");
    let once = jet::format_source(src).expect("fmt should accept value-tag types");
    let twice = jet::format_source(&once).expect("second fmt of value-tag types must succeed");
    assert_eq!(once, twice, "value-tag type fmt must be idempotent");
}

#[test]
fn fmt_layout_block_round_trips_byte_for_byte() {
    // D-LAYOUT1: the parser desugars every `box.anchor` read inside
    // `layout NAME { … }` into a `NAME.h(box, anchor)`/`NAME.v(box, anchor)`
    // method call (a purely structural rewrite, `Parser/Statements.rs`); the
    // formatter must re-sugar those calls back to `box.anchor` so `layout`
    // round-trips STABILITY, not just idempotence (memory: a prior formatter
    // change silently dropped tokens while only idempotence was checked).
    let src = "\
fn run() {
    layout form {
        label.width >= 80.0
        label.right + 16.0 == input.left
        label.width + 16.0 + input.width == form.width
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
fn fmt_preserves_trailing_block() {
    // D-TRAILBLOCK1: a bare-name call with only a trailing block (`twice { … }`,
    // no `()` at all) and a call with args plus a trailing block
    // (`ui.button("Save") { save() }`) must both round-trip byte-for-byte — the
    // block must not migrate back inside the parens, and no argument is dropped.
    let src = "\
fn twice(f: fn()) {
    f()
    f()
}

fn run() {
    twice {
        print(\"hi\")
    }
}
";
    assert_fmt_stable(src, "trailing block, no parens");
}

#[test]
fn fmt_preserves_bare_lambda_params() {
    // D-LAMBDAINFER1: fmt must NOT re-insert an inferred parameter type — that
    // would be reading noise the ballot explicitly rejects. `(x) => …` stays
    // exactly as written; an already-annotated `(n: Int) => …` also survives.
    let src = "\
fn run() {
    nums :: [1, 2, 3, 4, 5]
    big :: nums.filter((x) => x > 3)
    print(big)
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
    // D-UNITLIT1: `500ms` must survive with no space inserted between the
    // number and the suffix, and the suffix itself must not be dropped.
    let src = "\
@UnitFamily(Time) { ms, s }

fn run() {
    a :: 500ms
    print(\"{a.raw()}\")
}
";
    assert_fmt_stable(src, "unit literal");
}

#[test]
fn fmt_preserves_range_constraint() {
    // D-RANGETYPE1: `distinct Int(0..10)` — distinct declarations are emitted
    // verbatim, so the `(0..10)` clause survives structurally; this pins it
    // down explicitly rather than relying on that being an accident.
    let src = "\
Severity :: distinct Int(0..10);

fn run() {
    sev :: Severity(3)
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
    // D-TYPEDTEXT1=D: a `Sql`/`Html` literal argument and the `.raw()` escape
    // must survive byte-for-byte.
    let src = "\
fn run_query(q: Sql) {
    print(\"template: {q.template()}\")
}

fn render(h: Html) {
    print(\"html: {h.text()}\")
}

fn run() {
    id :: 42
    q :: sql\"select * from t where id = {id}\"
    run_query(q)
    name :: \"Jet\"
    page :: html\"<p>{name}</p>\"
    render(page)
    trusted :: Html.raw(\"<b>audited</b>\")
    render(trusted)
    arg :: \"two words;*.jet\"
    expected: Sh :: \"printf <%s> {arg}\"
    prefixed :: sh\"printf [%s] {arg}\"
    audited_cmd :: Sh.raw(\"printf raw\")
}
";
    assert_fmt_stable(src, "typed text");
}

#[test]
fn fmt_preserves_yield() {
    // D-STREAMYIELD1: `yield` must keep its own line, and `Stream<T>` in the
    // return-type position must survive byte-for-byte.
    let src = "\
fn count(n: Int) -> Stream<Int> {
    i := 0
    loop i < n {
        yield i
        i = i + 1
    }
}

fn run() {
    loop x in count(3) {
        print(\"{x}\")
    }
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
    if 0 <= sev < 10 {
        print(\"in range\")
    }
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
@Numeric @Comparable Usd :: distinct Int;

@Printable @CodableAsBase CustomerId :: distinct Int;

fn run() {
    a :: Usd(100)
    print(a.raw())
}
";
    assert_fmt_stable(src, "capability bundle markers");
}

#[test]
fn fmt_preserves_contracts() {
    // D-PREPOST1: `@Pre`/`@Post` clauses (condition + message) must survive
    // byte-for-byte, in declared order. Emitted inline before `fn`, space-
    // separated — the same one-marker-placement convention every other `fn`
    // marker uses (`@State(…)`, `@Transition(…)`, `@Pure`, `@MustUse`, …;
    // I8: one way to mean it), not one clause per line.
    let src = "\
@Pre(cents > 0, \"cents must be positive\") @Post(result > cents, \"result must exceed cents\") fn add_fee(cents: Int) -> Int {
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
    // D-PERSIST1: `@Persist` on a module-level `const` must survive
    // byte-for-byte.
    let src = "\
@Persist const counter = 0

fn run() {
    print(\"{counter}\")
}
";
    assert_fmt_stable(src, "persist marker");
}

#[test]
fn fmt_preserves_variadic_trait_bound_bare() {
    // D-ANY-JAI1 (c7jaiany): `parts: ...Renderable` — the bare single-trait
    // bound sugar — must survive byte-for-byte.
    let src = "\
fn log_all(parts: ...Renderable) {
    loop p in parts {
        print(\"{p}\")
    }
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
    loop p in parts {
        print(\"{p}\")
    }
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
    fn shout(self) -> String
}

fn describe<T: [Renderable, Loud]>(item: T) -> String {
    return item.shout()
}
";
    assert_fmt_stable(src, "generic multi-trait bound list");
}

#[test]
fn fmt_preserves_view_call_range_args() {
    // D-SHAPE-PLACE1=A: `.view(a..b)` is retained only so sema can teach E0214,
    // but formatter recovery must still preserve the retired source losslessly.
    let src = "\
fn run() {
    incidents: [Int] := [1, 2, 3]
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
    values: [Int] := [1, 2, 3]
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
    // D-UNINIT-SENTINEL1: `name: Type := uninit` — the binding's `init` AST
    // node is a harmless never-evaluated placeholder, so the formatter must
    // special-case `b.uninit` and print the `uninit` keyword literally
    // instead of formatting the placeholder (own-CLAUDE-memory rule: new
    // syntax needs a formatter round-trip test, not just a parser).
    let src = "\
use core.mem

fn run() {
    n: Int := uninit
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
    s: Stats := Stats.{strength: 10, gear_mod: 3}
    print(\"attack: {s.attack}\")
}
";
    assert_fmt_stable(src, "computed fields");
}

#[test]
fn fmt_preserves_inline_contracts() {
    // D-METHODMACRO1=A: `@Inline`/`@InlineAlways` precede `pub`/`fn` on a free
    // function and on a method — both must round-trip byte-for-byte (own-
    // CLAUDE-memory rule: new syntax needs a formatter round-trip test, not
    // just a parser).
    let src = "\
@Inline fn square(x: Int) -> Int {
    return x * x
}

@InlineAlways fn double(x: Int) -> Int {
    return x * 2
}

struct Meters {
    value: Int

    @InlineAlways fn plus(self, other: Int) -> Int {
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

@Unsafe(\"caller keeps address live\", obligations: .Track)
fn read(address: Int) -> Int {
    pointer :: mem.Ptr<Int>.from_addr(address)
    assert valid_ptr, aligned
    value :: mem.volatile_read(pointer)
    assert valid_ptr, aligned
    return value
}

fn run() {
    @Unsafe(\"calling audited reader\", obligations: .Skip) {
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
// this fix, `@Target(Web)` / `@Html(...)` / `@Target(Os.*)` markers were
// silently DROPPED by `jet fmt` — no error, just gone.

#[test]
fn fmt_target_web_marker_stability() {
    // D-WEBDEFAULT1=A: `@Target(Web)` is a singleton file marker with no
    // captured span (same treatment as `@PubFile`) — it renders at a fixed
    // canonical position right after imports, not wherever the author
    // originally wrote it.
    let src = "use core.io as io\n@Target(Web)\n\nfn run() {\n    io.print(\"hi\")\n}\n";
    assert_fmt_stable(src, "@Target(Web) marker");
}

#[test]
fn fmt_html_marker_stability() {
    // D-HTMLPAIR1=A: `@Html(\"path.html\")` — same marker family as
    // `@Target(Web)`, same fixed-position treatment.
    let src = "use core.ui as ui\n@Target(Web)\n\n@Html(\"dashboard.html\")\n\nfn run() {\n    ui.print(\"hi\")\n}\n";
    assert_fmt_stable(src, "@Html(...) marker");
}

#[test]
fn fmt_os_target_marker_stability() {
    // D-OSTARGET1=A: `@Target(Os.Linux)` precedes the `impl` block it gates,
    // on its own line — item-scoped, not file-scoped, so (unlike the two
    // markers above) it keeps the author's own position.
    let src = "trait Backend {\n    fn label(self) -> String\n}\n\nstruct LinuxBackend {\n    name: String\n}\n\n@Target(Os.Linux)\nimpl LinuxBackend.Backend {\n    fn label(self) -> String {\n        return \"linux: {self.name}\"\n    }\n}\n\nfn run() {\n    print(\"hi\")\n}\n";
    assert_fmt_stable(src, "@Target(Os.Linux) marker");
}

// ── #177 §5 syntax-lock sweep: 5 formatter bugs found reformatting the full
// example tree, all fixed in this pass. Each was silent — `jet fmt` printed
// no error, it just emitted wrong or missing output.

#[test]
fn fmt_pub_file_precedes_imports() {
    // D-VISDEFAULT2: `@PubFile` carries no span (a fixed-position file
    // marker, like `@Target(Web)`), so it used to render *after* the
    // imports loop. But an import's `priv`/`pub` qualifier is chosen
    // relative to `@PubFile` being in effect — emitting the marker after the
    // import it gates produced `priv use …` with no preceding `@PubFile`,
    // which doesn't even reparse (E0413). `@PubFile` must render before
    // imports, not after.
    let src = "@PubFile\n\nuse core.io\n\nfn run() {\n    print(\"hi\")\n}\n";
    assert_fmt_stable(src, "@PubFile before imports");
}

#[test]
fn fmt_verbatim_derive_body_comment_not_duplicated() {
    // D-METADERIVE1: `derive T.Trait { … }` bodies are emitted verbatim
    // (copied straight from source) rather than walked comment-by-comment,
    // so `comment_i` never advanced past a comment living inside the derive
    // body. The next item's `emit_leading` then found that comment
    // "unconsumed" and re-emitted it a second time before `@Label struct
    // Cube` — the comment kept duplicating on every subsequent fmt pass.
    let src = "derive T.Label {\n    info :: T.reflect()\n    tname :: info.name\n    // resolves to the same value as `tname`\n    lbl :: $tname\n    emit(\"impl $lbl {{ fn label(self) -> String {{ return \\\"$lbl\\\" }} }}\")\n}\n\n@Label\nstruct Cube {\n    side: Int\n}\n\nfn run() {\n    c :: Cube.{side: 5}\n    print(c.label())\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on a derive block");
    assert_eq!(
        out.matches("resolves to the same value as `tname`").count(),
        1,
        "derive-body comment must not duplicate, got:\n{out}"
    );
    assert_eq!(
        out, src,
        "derive block with an internal comment must already be canonical"
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
    let src = "fn join(prefix: String, msgs: ...String) -> String {\n    return [prefix, ...msgs].join(\" \")\n}\n\nfn run() {\n    parts: [String] := [\"one\", \"two\"]\n    b :: join(\"tags:\", ...parts)\n    print(b)\n}\n";
    assert_fmt_stable(src, "call-argument spread");
}

#[test]
fn fmt_preserves_trait_associated_type() {
    // D-LIB2: `fmt_trait` only ever walked `t.methods`, never
    // `t.assoc_types` — a trait's `type Elem` associated-type declaration
    // was silently dropped on every fmt pass.
    let src = "trait Indexed {\n    type Elem\n    fn at(self, i: Int) -> Elem\n    fn count(self) -> Int\n}\n\nstruct Nums {\n    vals: [Int]\n}\n\nimpl Nums.Indexed {\n    type Elem = Int\n\n    fn at(self, i: Int) -> Int {\n        return self.vals[i]\n    }\n\n    fn count(self) -> Int {\n        return self.vals.len()\n    }\n}\n\nfn run() {\n    n :: Nums.{vals: [10, 20, 30]}\n    print(n.at(0))\n    print(n.count())\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on trait assoc types");
    assert!(
        out.contains("type Elem\n"),
        "trait's `type Elem` associated-type declaration must not be dropped, got:\n{out}"
    );
    assert_eq!(
        out, src,
        "trait with an associated type must already be canonical"
    );
    let twice = jet::format_source(&out).expect("second fmt should succeed");
    assert_eq!(out, twice, "trait assoc-type fmt must be idempotent");
}

#[test]
fn fmt_preserves_pure_callback_bound_sigil() {
    // D-EFF2 / D-MARKER-FAMILY1: the empty callback effect bound renders as
    // `@Pure ` (a contract marker, `@`-plane) but the code wrote the literal
    // `#` directive-plane prefix instead — every `f: fn(T) --[]-> U`
    // parameter type downgraded to the retired `fn(T) --[]-> U` spelling
    // on the first fmt pass, which then fails to parse (E0062).
    let src = "fn transform(items: [Int], f: fn(Int) --[]-> Int) -> [Int] {\n    return items.map((x) => f(x))\n}\n\nfn run() {\n    doubled :: transform([1, 2, 3], (n: Int) => n * 2)\n    print(\"{doubled}\")\n}\n";
    assert_fmt_stable(src, "@Pure callback bound");
}

#[test]
fn fmt_preserves_dotted_effect_paths() {
    // D-EFFTREE1: an effect-list entry may now be a dotted path (`Fs.Read`,
    // `Net.Http.Get`) — a leaf under one of the ten D-EFF4/5 roots. The name
    // is stored as one opaque string end to end, so fmt needs no new emission
    // logic; this pins that the dot survives every printer that touches an
    // effect list (`#(…)` bounds, prohibitions, `@Caps`/`@Grant` regions).
    let src = "fn load(path: String) --[Fs.Read]-> String {\n    return path\n}\n\nfn archive(path: String) --[Fs.Write]-> {\n    print(path)\n}\n\nfn read_only(path: String) --[Fs.Read, !Fs.Write]-> {\n    load(path)\n}\n\nfn boot() --[Fs]-> {\n    load(\"app.conf\")\n    archive(\"out.tar\")\n    @Caps(Net.Http.Get) {\n        print(\"net\")\n    }\n    @Grant(Fs.Read) { caps ->\n        load(\"app.conf\")\n    }\n}\n";
    assert_fmt_stable(src, "dotted effect paths (D-EFFTREE1)");
}

#[test]
fn fmt_preserves_int_literal_radix() {
    // S34/S67: `0x`/`0o`/`0b` prefixes and `_` digit separators are ratified
    // author-facing spelling. fmt used to re-emit every integer literal from
    // its AST value — rewriting `0x2a` to `42` and `1_000_000` to `1000000`,
    // destroying the radix the author chose (same failure class as a dropped
    // token; caught decimalizing examples/features/parsing/binary-reader.jet
    // and the crypto examples' key material).
    let src = "fn run() {\n    packet: [U8] :: [0x2a, 0x00, 0xFF, 0o17, 0b1010, 116]\n    big :: 1_000_000\n    print(\"{packet.len()} {big}\")\n}\n";
    assert_fmt_stable(src, "int literal radix");
}

#[test]
fn fmt_preserves_web_partition_markers() {
    // D-WASM1, respelled by D-MARK-TARGET1=A (ratified 2026-07-11, card
    // #498): `@Target(Js)` / `@Target(Wasm)` / `@WasmExport` per-function
    // partition overrides, each on its own line before `fn`. fmt dropped the
    // marker entirely (Func.web_marker was never re-emitted) — every
    // browser-side function silently fell back to the Wasm bucket, breaking
    // the cross-partition checks: web_showcase_dashboard_roundtrip and
    // web_compute_wasm_bridge_roundtrip in tests/web_build.rs went red after
    // the #177 §5 tree reformat.
    let src = "@Target(Js)\nfn render_stat(label: String) -> String {\n    return \"<div>{label}</div>\"\n}\n\n@Target(Wasm)\nfn crunch(n: Int) -> Int {\n    return n * n\n}\n\n@WasmExport\nfn bridge_total(n: Int) -> Int {\n    return crunch(n)\n}\n\nfn run() {\n    print(\"{bridge_total(4)}\")\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on web partition markers");
    for tag in ["@Target(Js)\n", "@Target(Wasm)\n", "@WasmExport\n"] {
        assert!(
            out.contains(tag),
            "fmt must keep the `{}` partition marker, got:\n{out}",
            tag.trim_end()
        );
    }
    assert_eq!(
        out, src,
        "web-partition-marked fns must already be canonical"
    );
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
    assert_eq!(out, src, "fmt should canonicalize away the optional `;`");
}

#[test]
fn fmt_preserves_maturity_tags() {
    // D-MARK-META1=B: maturity fields are doc-only but
    // must round-trip (fmt STABILITY — not just accept-without-crash).
    let src = "\
@Meta(maturity: .Experimental)
fn experimental_label() -> String {
    return \"exp\"
}

@Meta(maturity: .Tested)
fn tested_label() -> String {
    return \"tested\"
}

@Meta(maturity: .Hardened)
fn hardened_label() -> String {
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
@Meta(maturity: .Experimental)
fn experimental_label() -> String {
    return \"exp\"
}

fn run() {
    print(experimental_label())
}
";
    let out = jet::format_source(src).expect("next-line maturity metadata should parse");
    assert!(
        out.contains("@Meta(maturity: .Experimental)"),
        "fmt must not drop maturity metadata; got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("re-fmt");
    assert_eq!(out, twice, "maturity next-line canonicalize must be idempotent");
}

#[test]
fn fmt_preserves_schedule_markers() {
    // D-SCHEDULE1 (ratified 2026-07-11, card #505): `@Task`/`@Every(…)` must
    // round-trip byte-for-byte (fmt STABILITY, not just accept-without-crash)
    // — same inline-marker convention as `@Reactive`/`@Sanitizer`/
    // `@Replayable`/`@State(…)` (one space-separated line before `fn`).
    let src = "\
@Task @Every(5min) fn prune_sessions() {
    print(\"pruning\")
}

@Task @Every(\"03:00\") fn nightly_backup() {
    print(\"backing up\")
}

@Task fn manual_only() {
    print(\"manual\")
}

fn run() {
    prune_sessions()
    nightly_backup()
    manual_only()
}
";
    assert_fmt_stable(src, "@Task/@Every schedule markers (D-SCHEDULE1)");
}

#[test]
fn fmt_preserves_per_function_c_abi() {
    let src = "use c.demo as c\n@Extern module c.demo {\n    @Abi(system) fn portable(x: I32) -> I32 = \"portable\"\n    @Abi(sysv64) fn native(x: I32) -> I32 = \"native\"\n}\nfn run() {}\n";
    let once = jet::format_source(src).expect("@Abi C module should format");
    assert!(once.contains("@Abi(system)") && once.contains("@Abi(sysv64)"), "fmt dropped @Abi: {once}");
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
@Meta(category: label)
const size = capacity
pub struct Buffer { slots: [T#capacity] }
module nested<U> { pub fn keep(value: U) -> U { return ~value } }
module inner = nested<T>
@Meta(category: label)
pub fn adjusted() -> Int { return capacity + 1 }
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
        "@Meta(category: label)",
        "[T#capacity]",
        "module nested<U>",
        "capacity + 1",
        "ring<Int, 2 + 2, \"ring\">",
        "module b = a",
    ] {
        assert!(once.contains(preserved), "formatter lost `{preserved}`:\n{once}");
    }
}
