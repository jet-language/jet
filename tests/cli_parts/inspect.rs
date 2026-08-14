use super::*;

#[test]
fn inspect_guarantees_reports_mixed_components_and_json() {
    let dir = isolated_cwd("inspect_guarantees_mixed");
    fs::write(
        dir.join("package.jet"),
        "name: \"guarantees\"\nversion: \"0.1.0\"\ndeps: .{ libxml: c@system, libz: c@system }\npolicy: .{ contain: [\"libxml\"] }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "fn run() {\n    #Unsafe(\"audit\") {}\n}\n",
    )
    .unwrap();

    let human = Command::new(jet())
        .args(["inspect", "guarantees", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(0), "{}", String::from_utf8_lossy(&human.stderr));
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("proven") && human.contains("watched") && human.contains("fenced") && human.contains("TRUSTED"), "{human}");
    check_snapshot("inspect_guarantees_mixed.txt", &human);

    let json = Command::new(jet())
        .args(["inspect", "guarantees", "main.jet", "--json"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0), "{}", String::from_utf8_lossy(&json.stderr));
    let json = String::from_utf8(json.stdout).unwrap();
    assert!(parse_json(&json).is_ok(), "guarantee report JSON must parse: {json}");
    assert!(json.contains("\"guarantee\":\"TRUSTED\""), "{json}");
    check_snapshot("inspect_guarantees_mixed.json", &json);
}

#[test]
fn inspect_guarantees_harden_contains_every_dependency() {
    let dir = isolated_cwd("inspect_guarantees_harden");
    fs::write(
        dir.join("package.jet"),
        "name: \"guarantees\"\nversion: \"0.1.0\"\ndeps: .{ libxml: c@system, libz: c@system }\npolicy: .{ harden: true }\n",
    )
    .unwrap();
    fs::write(dir.join("main.jet"), "fn run() {}\n").unwrap();
    let output = Command::new(jet())
        .args(["inspect", "guarantees", "--release", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("profile: release") && stdout.contains("libxml") && stdout.contains("libz"), "{stdout}");
    assert!(!stdout.contains("TRUSTED"), "hardened dependencies must not report TRUSTED: {stdout}");
    check_snapshot("inspect_guarantees_hardened.txt", &stdout);
}

#[test]
fn inspect_guarantees_is_honest_for_single_file_and_freestanding() {
    let dir = isolated_cwd("inspect_guarantees_single_file");
    fs::write(dir.join("main.jet"), "fn run() {}\n").unwrap();

    let single = Command::new(jet())
        .args(["inspect", "guarantees", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(single.status.code(), Some(0), "{}", String::from_utf8_lossy(&single.stderr));
    let single = String::from_utf8(single.stdout).unwrap();
    assert!(single.contains("scope: single-file") && single.contains("externs") && single.contains("TRUSTED"), "{single}");
    check_snapshot("inspect_guarantees_single_file.txt", &single);

    let freestanding = Command::new(jet())
        .args(["inspect", "guarantees", "--freestanding", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(freestanding.status.code(), Some(0), "{}", String::from_utf8_lossy(&freestanding.stderr));
    let freestanding = String::from_utf8(freestanding.stdout).unwrap();
    assert!(freestanding.contains("target: freestanding") && freestanding.contains("prover + audit only"), "{freestanding}");
    check_snapshot("inspect_guarantees_freestanding.txt", &freestanding);
}

#[test]
fn package_guarantees_are_tighten_only() {
    let mut weak = jet::Package::PackagePolicy::default();
    weak.contain.insert("libxml".to_string());
    let mut strong = weak.clone();
    strong.contain.insert("libz".to_string());
    strong.harden = true;
    assert!(weak.guarantees_tighten(&strong).is_ok());
    assert!(strong.guarantees_tighten(&weak).is_err());
}

#[test]
fn source_policy_spelling_is_rejected_with_teaching_diagnostic() {
    let dir = isolated_cwd("inspect_guarantees_source_policy");
    let file = dir.join("main.jet");
    fs::write(&file, "#Policy(contain)\nfn run() {}\n").unwrap();
    let output = Command::new(jet())
        .args(["check", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Error [E0355]") && stderr.contains("contain") && stderr.contains("not a scoped policy"), "{stderr}");
    let teaching = stderr
        .lines()
        .filter(|line| line.starts_with("Error [") || line.starts_with(" Why:") || line.starts_with(" Fix:"))
        .collect::<Vec<_>>()
        .join("\n");
    check_snapshot("inspect_guarantees_source_policy.txt", &format!("{teaching}\n"));
}
