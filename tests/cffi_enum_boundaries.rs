//! Bounded front-end guards for the existing C-ABI enum boundary.
//!
//! These checks deliberately stay outside the shared native matrix owned by
//! cards #524/#525. They pin the existing rejection paths only; they are not
//! native target evidence and do not replace a Darwin or Windows run.

use std::fs;

mod common;

fn codes(source: &str, stem: &str) -> Vec<String> {
    let root = common::unique_tmp(stem);
    let path = root.join("main.jet");
    fs::write(&path, source).unwrap();
    let diagnostics = jet::check_with_path(path.to_str().unwrap());
    let _ = fs::remove_dir_all(root);
    diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn cffi_enum_unknown_tag_width_stays_at_the_checked_boundary() {
    let diagnostics = codes(
        r#"
use c.bad as c
#Layout(c, tag: U24)
enum BadTag {
    Ok = 0
}
#Import module c.bad {
    fn take(value: BadTag) -> I32 = "take_bad_tag"
}
fn run() {}
"#,
        "jet_cffi_enum_bad_tag",
    );
    assert!(
        diagnostics.iter().any(|code| code == "E0003"),
        "unsupported C enum tag width must be rejected by the marker boundary: {diagnostics:?}"
    );
}

#[test]
fn cffi_enum_out_of_range_discriminant_stays_at_the_checked_boundary() {
    let diagnostics = codes(
        r#"
enum BadDiscriminant {
    TooWide = 9223372036854775808
}
fn run() {}
"#,
        "jet_cffi_enum_bad_discriminant",
    );
    assert!(
        diagnostics.iter().any(|code| code == "E0035"),
        "an out-of-range enum discriminant must stop before C ABI lowering: {diagnostics:?}"
    );
}
