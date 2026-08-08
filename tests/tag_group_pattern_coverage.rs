//! Card #1672: `note_pattern_coverage` (crates/jet-sema/src/Sema/CheckerCore/switches.rs)
//! calls the shared `jet_foundation::Facts::fact_covers` instead of hand-inlining the
//! dotted-prefix subsumption test. This proves D-TAG1 group subsumption still fires for
//! a true ancestor arm, and — the case the inline `starts_with` copy could get wrong —
//! does not fire for a sibling group whose name merely shares a text prefix.

fn codes(source: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!(
        "jet_tag_group_pattern_coverage_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, source).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check)
        .into_iter()
        .map(|diagnostic| diagnostic.code.to_string())
        .collect()
}

/// D-TAG1: an earlier group arm `.Fire ->` already covers every leaf under it,
/// so a later `.Fire.Burn ->` arm is unreachable.
#[test]
fn ancestor_group_arm_still_flags_later_leaf_as_unreachable() {
    let diagnostics = codes(
        r#"
enum Damage {
    Fire { Burn, Scald }
    Cold
}
fn run() {
    d := Damage.Fire.Burn
    if d == {
        .Fire -> { print("fire") }
        .Fire.Burn -> { print("burn") }
        .Cold -> { print("cold") }
    }
}
"#,
    );
    assert!(
        diagnostics.iter().any(|code| code == "L0301"),
        "{diagnostics:?}"
    );
}

/// `fact_covers` requires the `.` separator right after the exact prefix, so a
/// sibling top-level variant whose name merely starts with the same text
/// (`FireAlarm` starts with `Fire`) must NOT be treated as already covered.
#[test]
fn sibling_variant_with_shared_text_prefix_is_not_falsely_covered() {
    let diagnostics = codes(
        r#"
enum Alarm {
    Fire
    FireAlarm
}
fn run() {
    a := Alarm.FireAlarm
    if a == {
        .Fire -> { print("fire") }
        .FireAlarm -> { print("fire alarm") }
    }
}
"#,
    );
    assert!(
        !diagnostics.iter().any(|code| code == "L0301" || code == "E0365"),
        "{diagnostics:?}"
    );
}
