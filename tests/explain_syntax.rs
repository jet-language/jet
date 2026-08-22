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
