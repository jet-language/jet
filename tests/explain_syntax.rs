mod common;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn cli_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli")
}

fn check_snapshot(name: &str, actual: &str) {
    let path = cli_dir().join(name);
    if std::env::var("UPDATE_EXPECT").is_ok() {
        fs::create_dir_all(cli_dir()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run UPDATE_EXPECT=1 cargo test",
            path.display()
        )
    });
    assert_eq!(actual, expected, "snapshot mismatch for {}", name);
}

#[test]
fn explain_syntax_dictionary_golden() {
    for (query, snapshot) in [
        ("@", "explain_syntax_at.txt"),
        ("::", "explain_syntax_bind.txt"),
        ("#Live", "explain_syntax_marker.txt"),
        ("->", "explain_syntax_arrow.txt"),
        ("loop", "explain_syntax_keyword.txt"),
        ("^", "explain_syntax_caret.txt"),
        ("??", "explain_syntax_fallback.txt"),
    ] {
        let out = Command::new(jet())
            .args(["explain", query])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "jet explain {query} failed");
        check_snapshot(snapshot, &String::from_utf8_lossy(&out.stdout));
    }

    let out = Command::new(jet())
        .args(["explain", "@@"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    check_snapshot(
        "explain_syntax_unknown.txt",
        &String::from_utf8_lossy(&out.stderr),
    );
}

/// #3723: `--verbose` adds the registry constant and owning decision that the
/// default plain-words answer leaves out.
#[test]
fn explain_syntax_verbose_names_registry_constant_and_decision() {
    let default = Command::new(jet())
        .args(["explain", "^"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let verbose = Command::new(jet())
        .args(["explain", "^", "--verbose"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(verbose.status.success(), "jet explain ^ --verbose failed");
    let default = String::from_utf8_lossy(&default.stdout);
    let verbose = String::from_utf8_lossy(&verbose.stdout);
    assert!(verbose.starts_with(default.as_ref()), "{verbose}");
    let row = jet::Syntax::lookup("^").expect("`^` row");
    assert!(
        verbose.contains(&format!("Registry name: {}", row.name))
            && verbose.contains(&format!("Decision: {}", row.decision)),
        "{verbose}"
    );
}

/// #3723: every registered sigil and operator explains itself in plain words:
/// each meaning has an example, and the default answer names no internal
/// constant (`SIGIL_`, `OP_`) or decision ID.
#[test]
fn explain_syntax_rows_are_plain_and_exampled() {
    let mut checked = 0;
    for row in jet::Syntax::rows().iter().filter(|row| {
        matches!(
            row.kind,
            jet::Syntax::SyntaxDictionaryKind::Sigil | jet::Syntax::SyntaxDictionaryKind::Operator
        )
    }) {
        checked += 1;
        let shown = jet::Syntax::display(row);
        assert!(
            !row.meanings.is_empty()
                && row
                    .meanings
                    .iter()
                    .all(|meaning| !meaning.position.is_empty() && !meaning.example.trim().is_empty()),
            "`{shown}` needs a positioned meaning with an example for each use"
        );
        let explanation = jet::Explain::lookup(&shown).expect("dictionary row explains");
        let rendered = jet::Explain::render(&explanation, false);
        assert!(rendered.contains("Example:"), "`{shown}` has no example:\n{rendered}");
        for word in rendered.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-') {
            assert!(
                !word.starts_with("SIGIL_") && !word.starts_with("OP_") && !word.starts_with("D-"),
                "`{shown}` default explanation leaks `{word}`:\n{rendered}"
            );
        }
        assert!(
            !rendered.contains(&row.name) || row.name == row.token,
            "`{shown}` default explanation names its constant `{}`",
            row.name
        );
        for code in row.meanings.iter().flat_map(|meaning| &meaning.related_codes) {
            assert!(
                jet::Explain::lookup(code).is_some_and(|related| !related.retired),
                "`{shown}` names related code {code}, which is not a live diagnostic"
            );
        }
    }
    assert!(checked > 40, "expected the full sigil/operator ledger, walked {checked}");
}
