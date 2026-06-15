//! M6 phase 1: `jet fmt` idempotence — fmt(fmt(x)) == fmt(x).

use std::fs;
use std::path::PathBuf;

fn collect_jet_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some(jet::syntax::FILE_EXT) {
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
fn fmt_canonicalizes_s14_foreign_spellings() {
    let src = r#"fn main() {
    let x = 1;
    print("{x}");
}
"#;
    let out = jet::format_source(src).expect("fmt should parse through S14 recovery");
    assert!(
        out.contains("val x"),
        "expected `let` lowered to `val`, got:\n{out}"
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
    switch fruit {
        fruit == apple -> { print("Apple Juice"); };
        fruit == orange || frozen != true -> { print("Orange Juice"); };
        fruit == tangerine || fruit == yuzu -> { print("Citrus Juice"); };
        else -> { print("Water"); };
    }
}
"#;
    let out = jet::format_source(src).expect("fmt should parse legacy switch syntax");
    assert!(
        out.contains("| apple {"),
        "expected bare equality case, got:\n{out}"
    );
    assert!(
        out.contains("| orange || (frozen != true) {"),
        "expected mixed pipe condition, got:\n{out}"
    );
    assert!(
        out.contains("| tangerine || yuzu {"),
        "expected repeated subject equality to collapse, got:\n{out}"
    );
    assert!(out.contains("| else {"), "expected pipe else, got:\n{out}");
    let twice = jet::format_source(&out).expect("pipe switch output should re-fmt");
    assert_eq!(out, twice, "pipe switch formatting must be idempotent");
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
        out.contains("return [JSON.Null];"),
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
    let src = "fn main() { val x = ; }\n";
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
