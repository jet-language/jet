//! M6 phase 1: `jet fmt` idempotence — fmt(fmt(x)) == fmt(x).

use std::fs;
use std::path::PathBuf;

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
fn fmt_preserves_s61_call_labels() {
    // S61: call-site argument labels (`name:`) must survive fmt — previously
    // `fmt_call_args` dropped them, so `area(width: 3, height: 4)` round-tripped
    // to `area(3, 4)`, silently losing the labels.
    let src = "fn area(width: Int, height: Int) -> Int {\n    return width * height\n}\n\nfn main() {\n    print(area(width: 3, height: 4))\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on labeled calls");
    assert!(
        out.contains("width: 3") && out.contains("height: 4"),
        "fmt must preserve S61 call labels, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("labeled-call fmt should re-fmt");
    assert_eq!(out, twice, "labeled-call fmt must be idempotent");
}

#[test]
fn fmt_preserves_block_comments() {
    // S5: `/* … */` block comments, nesting allowed.
    let src = r#"/* a leading block comment */
fn main() {
    /* explains the next line, /* with a nested comment */ inside */
    x @= 5
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
fn fmt_canonicalizes_bare_question_return_to_fallible_return() {
    let src = r#"fn parse_count(raw: String) -> Int? {
    return err("empty");
}
"#;
    let out = jet::format_source(src).expect("fmt should parse default Error return");
    assert!(
        out.contains("fn parse_count(raw: String) -> Int ? {"),
        "expected `Int?` return to format as `Int ?`, got:\n{out}"
    );
}

#[test]
fn fmt_if_expression_preserves_condition_parens() {
    // S68 (D-SG2): `if` as a value round-trips.
    // D-FMTPARENS1=A: author-written grouping parens are always preserved.
    let src = r#"fn main() {
    m @= if (a > b) {
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
        out.contains("m @= if (a > b) {"),
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
    let src = r#"fn main() {
    raw @= "  hi  "
    out @= raw
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
    let src = r#"fn main() {
    who @= "Jet"
    banner @= """
    hello, {who}
        indented
    """
    print("{banner}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept a triple-quoted string");
    assert!(
        out.contains("banner @= \"\"\"\n"),
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

fn main() {
    Point.{ x, y } @= make()
    [a, b, c] := nums()
    print("{x}{y}{a}{b}{c}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept destructuring targets");
    assert!(
        out.contains("Point.{x, y} @= make()"),
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

fn main() {
    p @= Point.{ x: 1, y: 2 }
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

fn main() {
    p @= (x: 1, y: 2)
    (a, b) @= p
    print("{p.x}{a}{b}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept named tuples");
    assert!(
        out.contains("p @= (x: 1, y: 2)"),
        "expected named tuple literal preserved, got:\n{out}"
    );
    assert!(
        out.contains("-> (max: Int, min: Int)"),
        "expected canonical named tuple return type preserved, got:\n{out}"
    );
    assert!(
        out.contains("(a, b) @= p"),
        "expected tuple destructuring preserved, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("named tuple output should re-fmt");
    assert_eq!(out, twice, "named tuple formatting must be idempotent");
}

#[test]
fn fmt_preserves_optional_chaining() {
    // S71 (D-SG6): `?.` chains round-trip unchanged.
    let src = r#"fn main() {
    n @= o.mid?.inner?.name
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

fn use_collections(items: List<String>, counts: Map<String, Int>) {}
"#;
    let out = jet::format_source(src).expect("fmt should accept collection type sugar");
    assert!(
        out.contains("fn shell() -> [JSON]"),
        "expected list return shorthand, got:\n{out}"
    );
    assert!(
        out.contains("items: [String], counts: [String, Int]"),
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
    let src = "fn main() { x @= ; }\n";
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
fn fmt_preserves_single_line_if() {
    // A one-line `if` body the author placed inline survives unchanged.
    let src = "fn main() {\n    ready @= true\n    if ready { launch() }\n}\n";
    assert_fmt_stable(src, "single-line if");
}

#[test]
fn fmt_preserves_multiline_if() {
    // The 3-line form the author chose stays 3 lines.
    let src = "fn main() {\n    ready @= true\n    if ready {\n        launch()\n    }\n}\n";
    assert_fmt_stable(src, "multiline if");
}

#[test]
fn fmt_if_else_chain_one_multiline_expands_all() {
    // D-FMT1 chain rule: if any branch is multiline, the whole chain expands.
    // Author wrote `then` inline but `else` multiline → whole chain goes
    // multiline; the expanded form is then stable.
    let src = "fn main() {\n    if a { x() } else {\n        y()\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should accept the mixed chain");
    assert_eq!(
        out, "fn main() {\n    if a {\n        x()\n    } else {\n        y()\n    }\n}\n",
        "mixed if/else chain should expand wholesale, got:\n{out}"
    );
    let twice = jet::format_source(&out).expect("expanded chain should re-fmt");
    assert_eq!(out, twice, "expanded chain must be idempotent");
}

#[test]
fn fmt_single_line_comment_forces_expand() {
    // Gate (c): a comment inside the braces forces the body multiline.
    let src = "fn main() {\n    if ready { launch() /* go */ }\n}\n";
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
    let src = format!("fn main() {{\n    if ready {{ print(\"{long}\") }}\n}}\n");
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
    let src = "fn main() {\n    if a { if b { x() } }\n}\n";
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
    let while_src = "fn main() {\n    n @= 0\n    loop n < 3 { n += 1 }\n}\n";
    assert_fmt_stable(while_src, "single-line while/loop");

    let for_src = "fn main() {\n    loop i in 0..3 { print(\"{i}\") }\n}\n";
    assert_fmt_stable(for_src, "single-line for/loop");

    let fn_src = "fn one() -> Int { return 1 }\n";
    assert_fmt_stable(fn_src, "single-line fn body");
}

#[test]
fn fmt_preserves_single_line_if_expr_branch() {
    // If-expression branches (routed through fmt_value_block) follow the rule.
    let src = "fn main() {\n    a @= true\n    n @= if a { 1 } else { 2 }\n    print(\"{n}\")\n}\n";
    assert_fmt_stable(src, "single-line if-expression");
}

// ── Marker / turbofish round-trip survival (the c-fmt data-loss regression) ──
//
// `fmt_is_idempotent_on_examples` only checks that fmt(fmt(x)) == fmt(x); it does
// NOT catch a formatter that silently drops a token on the FIRST pass (the lost
// `#[Rename]`/`#[Codable]`/turbofish bug). These tests assert that the load-bearing
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
fn fmt_keeps_codable_and_field_rename_and_turbofish() {
    // The exact c-fmt regression: `#[Codable]`, a field `#[Rename("…")]`, and a
    // method-call turbofish `decode<Order>` must all survive.
    let src = "\
#[Codable]
struct Order {
    id: Int
    #[Rename(\"customer\")] who: String
}

fn main() {
    raw @= \"x\"
    back @= json.decode<Order>(raw) ?? panic(\"bad\")
    print(back.who)
}
";
    assert_fmt_keeps(
        src,
        &["#[Codable]", "#[Rename(\"customer\")]", "decode<Order>"],
        "codable + rename + turbofish",
    );
}

#[test]
fn fmt_keeps_container_rename_all() {
    // A combined derive + container serde marker `#[Codable, RenameAll(camel)]`
    // round-trips as the single bracket list the user wrote.
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
    // `#[Codable]` then `#Layout(columnar)` on the same struct — both survive,
    // neither is rewritten into body `derive` lines.
    let src = "\
#[Codable]
#Layout(columnar)
struct Particle {
    x: Float
}
";
    assert_fmt_keeps(
        src,
        &["#[Codable]", "#Layout(columnar)"],
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
    // And it must NOT be promoted to a bracket marker.
    let out = jet::format_source(src).unwrap();
    assert!(
        !out.contains("#[Comparable]"),
        "body derive must not become bracket:\n{out}"
    );
}

#[test]
fn fmt_keeps_enum_variant_rename_and_tag() {
    // Enum container `#[Tag("type")]` + a per-variant `#[Rename("…")]` both survive.
    let src = "\
#[Codable, Tag(\"type\")]
enum Shape {
    #[Rename(\"circle\")] Circle(Float)
    Square(Float)
}
";
    assert_fmt_keeps(
        src,
        &["#[Codable, Tag(\"type\")]", "#[Rename(\"circle\")]"],
        "enum tag + variant rename",
    );
}

#[test]
fn fmt_keeps_typestate_markers() {
    // D-STATE1: the `#State(S)` require-state guard and `#Transition(From -> To)`
    // transition markers (including the entry form `_ -> To`) must survive fmt —
    // dropping a typestate contract would silently change what the checker enforces.
    let src = "\
struct Reservation {
    guest: String

    #Transition(_ -> Pending) fn book(guest: String) -> Reservation {
        return Reservation.{guest: guest}
    }

    #Transition(Pending -> Confirmed) fn pay(self: ^Reservation) -> Reservation {
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
            "#Transition(_ -> Pending)",
            "#Transition(Pending -> Confirmed)",
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
    let src = "fn main() {\n    x @= foo()\n        // \u{2502}\u{250c}\u{2514}\u{2500}\n        .bar()\n    print(x)\n}\n";
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
    let src = "fn main() {\n    #Impure(\"reading build config\") {\n        print(\"inside\")\n    }\n}\n";
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
    let src = "fn main() {\n    #Impure {\n        print(\"inside\")\n    }\n}\n";
    let out = jet::format_source(src).expect("fmt should succeed on #Impure block without reason");
    assert!(
        out.contains("#Impure {") || out.contains("#Impure{"),
        "#Impure block without reason dropped by fmt:\n{out}"
    );
    let twice = jet::format_source(&out).expect("#Impure (no reason) fmt must re-fmt");
    assert_eq!(out, twice, "#Impure (no reason) fmt must be idempotent");
}

#[test]
fn fmt_keeps_parens_around_binary_receiver() {
    // c143(b): `(a + b).method()` must keep its parens — dropping them rebinds
    // the `.method()` to `b` alone and changes the meaning. Likewise `(a + b) * c`
    // must not become `a + b * c`.
    let src = "fn main() {\n    c @= (1 + 2).to_string()\n    d @= (1 + 2) * 3\n    print(c)\n    print(d)\n}\n";
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
    let plain = jet::format_source("fn main() {\n    a @= 1 + 2 * 3\n    print(a)\n}\n").unwrap();
    assert!(
        plain.contains("a @= 1 + 2 * 3"),
        "added spurious parens:\n{plain}"
    );
    let twice = jet::format_source(&out).expect("paren fmt must re-fmt");
    assert_eq!(out, twice, "paren fmt must be idempotent");
}

#[test]
fn fmt_comptime_block_is_idempotent() {
    // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` formatting
    // round-trips — the block keyword, brace, and body all survive a second fmt.
    let src = r#"comptime LIMIT = 1000

fn main() {
    comptime {
        comptime ratio = LIMIT / 10
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
    let named = "struct Pt {\n    x: Int\n    y: Int\n}\n\nfn main() {\n    p @= Pt.{ x: 1, y: 2 }\n    print(\"{p.x}\")\n}\n";
    let out_named = jet::format_source(named).expect("fmt should accept Pt.{ … }");
    assert!(
        out_named.contains("Pt.{"),
        "named form `Pt.{{` must survive fmt, got:\n{out_named}"
    );
    assert!(
        !out_named.contains("Pt {x") && !out_named.contains("p @= Pt {"),
        "fmt must not regress to dotless construction form, got:\n{out_named}"
    );
    let twice_named = jet::format_source(&out_named).expect("named form must re-fmt");
    assert_eq!(out_named, twice_named, "named form must be fmt-idempotent");

    // Old dotless form (E0320 recovery) is auto-fixed to `Type.{`.
    let old = "struct Pt {\n    x: Int\n    y: Int\n}\n\nfn main() {\n    p @= Pt { x: 1, y: 2 }\n    print(\"{p.x}\")\n}\n";
    let out_old = jet::format_source(old).expect("fmt should recover dotless E0320 form");
    assert!(
        out_old.contains("Pt.{"),
        "E0320 recovery must auto-fix to `Pt.{{`, got:\n{out_old}"
    );
    assert!(
        !out_old.contains("Pt {x") && !out_old.contains("p @= Pt {"),
        "dotless construction form must not survive fmt, got:\n{out_old}"
    );
    let twice_old = jet::format_source(&out_old).expect("fixed form must re-fmt");
    assert_eq!(out_old, twice_old, "fixed form must be fmt-idempotent");
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

fn main() {
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

fn main() {
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
    let src = "derive Debug for T {\n    info @= T.reflect()\n    tname @= info.name\n    emit(\"impl $tname {{ fn tag(self) -> String {{ return \\\"ok\\\" }} }}\")\n}\n\nfn main() {\n    print(\"ok\")\n}\n";
    let once = jet::format_source(src).expect("fmt should accept derive with $name in string");
    let twice = jet::format_source(&once).expect("second fmt should succeed");
    assert_eq!(
        once, twice,
        "derive body with $name in emit string must be fmt-idempotent"
    );

    // Standalone `$name` expression (outside emit string) round-trips as `$name`.
    let splice_src = "derive Named for T {\n    tname @= \"test\"\n    x @= $tname\n    emit(\"impl $tname {{ }}\")\n}\n\nfn main() {}\n";
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
    // D-TESTPAREN1=A: `#Test("name") { … }` must survive fmt unchanged.
    let src = "\
fn main() {}
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
fn fmt_explicit_binding_d_bindexplicit1_stability() {
    // D-BINDEXPLICIT1=A: `name@ Type = val` (immutable) and `name: Type = val`
    // (mutable) must survive fmt unchanged.
    let src = "\
fn main() {
    x@ Int = 42
    s: String = \"hi\"
    print(\"{x} {s}\")
}
";
    assert_fmt_keeps(
        src,
        &["x@ Int = 42", "s: String = \"hi\""],
        "explicit binding",
    );
    let once = jet::format_source(src).expect("fmt should accept explicit-type bindings");
    let twice = jet::format_source(&once).expect("second fmt of explicit bindings must succeed");
    assert_eq!(once, twice, "explicit-type binding fmt must be idempotent");
}

#[test]
fn fmt_loop_label_d_looplabel2_stability() {
    // D-LOOPLABEL2=A: `outer@ loop { break outer@ }` must survive fmt unchanged.
    let src = "\
fn main() {
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
fn main() {
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

fn main() {
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
fn process(input: #Tainted String) -> String {
    return \"{input}-clean\"
}

fn main() {
    result @= process(\"hello\")
    print(result)
}
";
    assert_fmt_keeps(src, &["#Tainted String"], "value-tag type qualifier");
    let once = jet::format_source(src).expect("fmt should accept value-tag types");
    let twice = jet::format_source(&once).expect("second fmt of value-tag types must succeed");
    assert_eq!(once, twice, "value-tag type fmt must be idempotent");
}
