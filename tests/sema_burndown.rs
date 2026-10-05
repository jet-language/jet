//! Focused check/lower/run regressions for semantic boundary burn-down.
mod common;
mod tir_support;

#[test]
fn annotated_tuple_empty_list_tiers() {
    tir_support::assert_tiers_agree(
        "annotated_tuple_empty_list",
        include_str!("../Examples/features/basics/tuple_empty_list.jet"),
        include_str!("../Examples/features/expected/basics/tuple_empty_list.out"),
    );
}

#[test]
fn prelude_assert_eq_shadow_tiers() {
    tir_support::assert_tiers_agree(
        "prelude_assert_eq_shadow",
        include_str!("../Examples/features/traits/prelude_assert_eq_shadow.jet"),
        include_str!("../Examples/features/expected/traits/prelude_assert_eq_shadow.out"),
    );
}

#[test]
fn user_read_dir_tiers() {
    tir_support::assert_tiers_agree(
        "user_read_dir",
        include_str!("../Examples/features/traits/user_read_dir.jet"),
        include_str!("../Examples/features/expected/traits/user_read_dir.out"),
    );
}

#[test]
fn math_copy_tiers() {
    tir_support::assert_tiers_agree(
        "math_copy",
        include_str!("../Examples/features/math/copy.jet"),
        include_str!("../Examples/features/expected/math/copy.out"),
    );
}

#[test]
fn mapped_byte_element_and_copy_inference_tiers() {
    let scratch = common::Scratch::new("mapped_byte_element");
    let path = scratch.path.join("data.txt");
    std::fs::write(&path, "mapped\n").expect("write mapped data");
    let source = format!(r#"
use core.files as files
fn run() {{
    mapped :: files.map(Path.from("{}")) ?? panic("map")
    window :: mapped.window_len(0, 1) ?? panic("window")
    print(Int.from_u8(window[0]))
    loop line in mapped.lines() {{
        print(Int.from_u8(line[0]))
        print(String.from_bytes(~line) ?? panic("utf8"))
    }}
}}
"#, path.display());
    tir_support::assert_tiers_agree("mapped_byte_element", &source, "109\n109\nmapped\n");
}

#[test]
fn source_qualified_file_scope_read_tiers() {
    let scratch = common::Scratch::new("qualified_file_scope");
    std::fs::write(scratch.path.join("inside.txt"), "scoped").expect("write scoped data");
    let source = format!(r#"
use core.files as files
fn read_inside(scope: files.FileScope) -> String {{
    scope.read("inside.txt") ?? panic("scoped read")
}}
fn run() {{
    scope :: files.scope(Authority.from_rights(["FS.Read:{}"]))
    print(read_inside(scope))
}}
"#, scratch.path.display());
    tir_support::assert_tiers_agree("qualified_file_scope", &source, "scoped\n");
}

#[test]
fn build_authority_cli_teaches_current_grant_and_renders_action_lines() {
    let scratch = common::Scratch::new("build_authority_cli");
    let entry = scratch.path.join("build_action_failed.jet");
    std::fs::write(&entry, include_str!("ui/build_action_failed.jet"))
        .expect("write failing build");
    let invoke = |allow: bool| {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("build").arg(&entry)
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .env("JET_STORE_DIR", scratch.path.join("store"));
        if allow {
            command.arg("--allow=Exec");
        }
        command.output().expect("invoke build")
    };
    let denied = invoke(false);
    let denied_text = String::from_utf8_lossy(&denied.stderr);
    assert!(!denied.status.success());
    assert!(denied_text.contains("E3503"), "{denied_text}");
    assert!(denied_text.contains("--allow=Exec"), "{denied_text}");
    assert!(!denied_text.contains("--allow-exec"), "{denied_text}");
    let granted = invoke(true);
    let granted_text = String::from_utf8_lossy(&granted.stderr);
    assert!(!granted.status.success());
    assert!(granted_text.contains("E3505"), "{granted_text}");
    assert!(granted_text.contains("first\nsecond\n"), "{granted_text}");
    assert!(!granted_text.contains("first\\nsecond"), "{granted_text}");
}
