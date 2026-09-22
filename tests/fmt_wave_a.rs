//! Card #3172: a bounded formatter preservation matrix and CLI outcome proof.
//!
//! Keep this slice source-local: it exercises the public formatter facade and
//! the existing `jet fmt` front door without introducing another formatting
//! policy or a second fixture convention.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const WHITESPACE_AND_LITERALS: &str = concat!(
    "fn run(){\n",
    "\t// tabbed comment: \"quotes\" {braces}\n",
    "\tvalue::\"quoted \\\"text\\\" and {{braces}}; escapes: \\t \\n\"\n",
    "\t/* outer {brace}, \"quote\", /* nested */ still here */\n",
    "\tprint(\"{value}\")\n",
    "}\n",
);

const TRIPLE_STRING: &str = r#"fn run() {
    who :: "Jet"
    banner :: """
    hello, {who}
        indented
    """
    print("{banner}")
}
"#;

const INTERPOLATION_SELECTORS: &str = concat!(
    "fn run(){\n",
    "price::1234.5\n",
    "print(\"{price:Fixed(2)}\")\n",
    "print(\"{price:Grouped(2)}\")\n",
    "}\n",
);

fn semantic_identity(source: &str) -> Vec<u8> {
    let (tokens, diagnostics) = jet::Lexer::lex(source);
    assert!(
        diagnostics.is_empty(),
        "fixture should lex before identity comparison: {diagnostics:?}"
    );
    let program = jet::Parser::parse_for_fmt(&tokens)
        .unwrap_or_else(|diagnostics| panic!("fixture should parse: {diagnostics:?}"));
    jet::Formatter::canonical_program(&program)
}

#[test]
fn preservation_matrix_survives_two_canonical_formatter_passes() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "comments, tabs, newlines, quotes, and braces",
            WHITESPACE_AND_LITERALS,
            &[
                "// tabbed comment: \"quotes\" {braces}",
                r#"quoted \"text\" and {{braces}}; escapes: \t \n"#,
                "/* nested */ still here */",
                "print(\"{value}\")",
            ],
        ),
        (
            "triple strings",
            TRIPLE_STRING,
            &[
                "banner :: \"\"\"\n",
                "hello, {who}\n",
                "        indented\n",
                "print(\"{banner}\")",
            ],
        ),
        (
            "interpolation selectors",
            INTERPOLATION_SELECTORS,
            &[
                "print(\"{price:Fixed(2)}\")",
                "print(\"{price:Grouped(2)}\")",
            ],
        ),
    ];

    for &(label, source, needles) in cases {
        let before = semantic_identity(source);
        let once = jet::format_source(source)
            .unwrap_or_else(|diagnostics| panic!("fmt failed for {label}: {diagnostics:?}"));
        for needle in needles {
            assert!(
                once.contains(needle),
                "{label}: formatter dropped {needle:?}\n--- got ---\n{once}"
            );
        }
        assert!(
            once.ends_with('\n'),
            "{label}: canonical output must end with a newline"
        );
        assert_eq!(
            before,
            semantic_identity(&once),
            "{label}: first formatter pass changed semantic identity"
        );

        let twice = jet::format_source(&once)
            .unwrap_or_else(|diagnostics| panic!("second fmt failed for {label}: {diagnostics:?}"));
        assert_eq!(once, twice, "{label}: formatter is not idempotent");
        assert_eq!(
            semantic_identity(&once),
            semantic_identity(&twice),
            "{label}: semantic identity changed across formatter passes"
        );
    }
}

const CLI_FIXTURE: &str = WHITESPACE_AND_LITERALS;

fn run_fmt(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("run jet fmt {args:?}: {error}"))
}

#[test]
fn cli_check_diff_and_write_keep_distinct_file_outcomes() {
    let scratch = common::Scratch::new("fmt-wave-a-cli");
    let path = scratch.join("main.jet");
    fs::write(&path, CLI_FIXTURE).expect("write deterministic fmt fixture");
    let before = fs::read_to_string(&path).expect("read fmt fixture");
    let canonical = jet::format_source(CLI_FIXTURE).expect("fixture should have canonical output");

    let check = run_fmt(&scratch.path, &["fmt", "--check", "main.jet"]);
    assert_eq!(
        check.status.code(),
        Some(1),
        "--check should reject dirty source: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        before,
        "--check must not mutate the fixture"
    );

    let diff = run_fmt(&scratch.path, &["fmt", "--check", "--diff", "main.jet"]);
    assert_eq!(
        diff.status.code(),
        Some(1),
        "--check --diff should reject dirty source: {}",
        String::from_utf8_lossy(&diff.stderr)
    );
    let stdout = String::from_utf8_lossy(&diff.stdout);
    assert!(
        stdout.contains("--- main.jet") && stdout.contains("+++ main.jet"),
        "--check --diff should print a unified diff, got:\n{stdout}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        before,
        "--check --diff must not mutate the fixture"
    );

    let write = run_fmt(&scratch.path, &["fmt", "main.jet"]);
    assert_eq!(
        write.status.code(),
        Some(0),
        "write mode should succeed: {}",
        String::from_utf8_lossy(&write.stderr)
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        canonical,
        "write mode must persist the canonical formatter output"
    );

    let clean = run_fmt(&scratch.path, &["fmt", "--check", "main.jet"]);
    assert_eq!(clean.status.code(), Some(0), "canonical fixture should pass --check");
}
