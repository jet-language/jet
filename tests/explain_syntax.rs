mod common;
include!("cli_parts/support.rs");

#[test]
fn explain_syntax_dictionary_golden() {
    for (query, snapshot) in [
        ("@", "explain_syntax_at.txt"),
        ("::", "explain_syntax_bind.txt"),
        ("#Live", "explain_syntax_marker.txt"),
        (":>", "explain_syntax_arrow.txt"),
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
