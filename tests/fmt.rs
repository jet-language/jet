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
fn fmt_canonicalizes_s14_foreign_spellings() {
    let src = r#"fn main() {
    let x = 1;
    print("{x}");
}
"#;
    let out = jet::format_source(src).expect("fmt should parse through S14 recovery");
    assert!(
        out.contains("x :: 1"),
        "expected `let` lowered to the `::` binding sigil, got:\n{out}"
    );
    assert!(
        !out.contains("let x"),
        "foreign `let` should be gone:\n{out}"
    );
    let twice = jet::format_source(&out).expect("canonical output should re-fmt");
    assert_eq!(out, twice, "canonicalized output must be idempotent");
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
fn fmt_canonicalizes_switch_arms_to_pipe_syntax() {
    let src = r#"fn main() {
    val fruit = "orange";
    val frozen = false;
    when fruit {
        fruit == apple -> { print("Apple Juice"); };
        fruit == orange || frozen != true -> { print("Orange Juice"); };
        fruit == tangerine || fruit == yuzu -> { print("Citrus Juice"); };
        else -> { print("Water"); };
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should parse legacy switch syntax");
    assert!(
        out.contains("if fruit {"),
        "expected `when` lowered to `if SUBJECT {{`, got:\n{out}"
    );
    assert!(
        out.contains("apple -> {"),
        "expected bare equality case, got:\n{out}"
    );
    assert!(
        out.contains("orange || (frozen != true) -> {"),
        "expected mixed condition arm, got:\n{out}"
    );
    assert!(
        out.contains("tangerine || yuzu -> {"),
        "expected repeated subject equality to collapse, got:\n{out}"
    );
    assert!(out.contains("else -> {"), "expected else arm, got:\n{out}");
    let twice = jet::format_source(&out).expect("pipe switch output should re-fmt");
    assert_eq!(out, twice, "pipe switch formatting must be idempotent");
}

#[test]
fn fmt_if_expression_and_strips_redundant_condition_parens() {
    // S68 (D-SG2): `if` as a value round-trips; redundant condition parens go.
    let src = r#"fn main() {
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
        out.contains("m :: if a > b {"),
        "expected paren-free condition in if-expression, got:\n{out}"
    );
    assert!(
        out.contains("    if a > b {"),
        "expected paren-free statement-if condition, got:\n{out}"
    );
    assert!(out.contains("} else {"), "expected else branch, got:\n{out}");
    let twice = jet::format_source(&out).expect("if-expression output should re-fmt");
    assert_eq!(out, twice, "if-expression formatting must be idempotent");
}

#[test]
fn fmt_preserves_author_placed_chain_breaks() {
    // S69 (D-SG3): a broken dot-chain keeps one step per line, each step may
    // carry its own trailing comment, and the final step's comment stays after
    // the statement terminator.
    let src = r#"fn main() {
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
    let src = r#"fn main() {
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
fn fmt_rewrites_retired_or_fallback_to_question_question() {
    // S71 (D-SG6): the retired word `or` formats to `??`; `??` round-trips.
    let src = r#"fn pick(xs: [Int]) -> Int {
    return xs.first() or 0
}
"#;
    let out = jet::format_source(src).expect("fmt should recover the retired `or`");
    assert!(
        out.contains("xs.first() ?? 0"),
        "expected `or` rewritten to `??`, got:\n{out}"
    );
    assert!(!out.contains(" or "), "stray `or` left:\n{out}");
    let twice = jet::format_source(&out).expect("`??` output should re-fmt");
    assert_eq!(out, twice, "`??` formatting must be idempotent");
}

#[test]
fn fmt_preserves_destructuring_targets() {
    // S74: `val`/`var` destructuring of a struct and a list round-trips.
    // S29-FLUSH: the canonical destructuring form is flush — `Point{x, y}` — so a
    // spaced input normalizes to the flush form on format.
    let src = r#"struct Point { x: Int, y: Int }

fn main() {
    Point { x, y } :: make()
    [a, b, c] := nums()
    print("{x}{y}{a}{b}{c}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept destructuring targets");
    assert!(
        out.contains("Point{x, y} :: make()"),
        "expected flush struct destructuring (S29-FLUSH), got:\n{out}"
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
    // S29-FLUSH: a struct literal hugs its field block — `Point{x: 1, y: 2}`, no
    // space before the brace. A spaced input normalizes to the flush form.
    let src = r#"struct Point { x: Int, y: Int }

fn main() {
    p :: Point { x: 1, y: 2 }
    print("{p.x}")
}
"#;
    let out = jet::format_source(src).expect("fmt should accept struct construction");
    assert!(
        out.contains("Point{x: 1, y: 2}"),
        "expected flush construction (S29-FLUSH), got:\n{out}"
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
    let src = r#"fn main() {
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
    let src = "fn main() { x :: ; }\n";
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
