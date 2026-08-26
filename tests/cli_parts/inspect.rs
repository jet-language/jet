use super::*;

#[test]
fn inspect_env_lists_typed_environment_reads() {
    let dir = isolated_cwd("inspect_env_reads");
    for (file, snapshot) in [
        ("env.jet", "inspect_env_reads.json"),
        ("config.jet", "inspect_config_reads.json"),
    ] {
        fs::write(dir.join(file), "module env.dev {\n    prompt: $HOME\n}\n").unwrap();
        let output = Command::new(jet())
            .args(["inspect", "env", file, "--json"])
            .current_dir(&dir)
            .env("HOME", "/test/home")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json = String::from_utf8(output.stdout).unwrap();
        assert!(
            parse_json(&json).is_ok(),
            "inspect env JSON must parse: {json}"
        );
        assert!(json.contains("\"name\":\"$HOME\""), "{json}");
        assert!(json.contains("\"type\":\"String\""), "{json}");
        check_snapshot(snapshot, &json);
    }
}

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
    assert_eq!(
        human.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(
        human.contains("proven")
            && human.contains("watched")
            && human.contains("fenced")
            && human.contains("TRUSTED"),
        "{human}"
    );
    check_snapshot("inspect_guarantees_mixed.txt", &human);

    let json = Command::new(jet())
        .args(["inspect", "guarantees", "main.jet", "--json"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        json.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let json = String::from_utf8(json.stdout).unwrap();
    assert!(
        parse_json(&json).is_ok(),
        "guarantee report JSON must parse: {json}"
    );
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
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("profile: release") && stdout.contains("libxml") && stdout.contains("libz"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("TRUSTED"),
        "hardened dependencies must not report TRUSTED: {stdout}"
    );
    check_snapshot("inspect_guarantees_hardened.txt", &stdout);
}

#[test]
fn hardened_release_sentry_reaches_a_foreign_dependency() {
    let dir = isolated_cwd("inspect_guarantees_runtime_harden");
    let app = dir.join("app");
    let dependency = dir.join("dep");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&dependency).unwrap();
    fs::write(
        app.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\ndeps: .{ dep: ../dep }\npolicy: .{ harden: true }\n",
    )
    .unwrap();
    fs::write(
        app.join("main.jet"),
        "use dep\nfn run() {\n    #Unsafe(\"calls the audited dependency\") {\n        print(dep.read())\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        dependency.join("package.jet"),
        "name: \"dep\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dependency.join("dep.jet"),
        "use core.mem\n#Unsafe(\"intentionally invalid address\")\npub fn read() Int -[]> {\n    p :: mem.Ptr<Int>.from_addr(1)\n    return mem.volatile_read(p)\n}\n",
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["run", "--release", "main.jet"])
        .current_dir(&app)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "invalid dependency access must stop: {stderr}"
    );
    assert!(
        stderr.contains("R0801"),
        "hardened dependency sentry was not active: {stderr}"
    );
    assert!(
        stderr.contains("intentionally invalid address"),
        "dependency gate was not named: {stderr}"
    );
}

#[test]
fn release_hardened_profile_catches_a_local_wrong_unsafe_region() {
    let source = include_str!("../../examples/features/memory/unsafe_sentries.jet");
    for (tag, hardened) in [("release_sentry_normal", false), ("release_sentry_hardened", true)] {
        let dir = isolated_cwd(tag);
        let policy = if hardened {
            "policy: .{ harden: true }\n"
        } else {
            ""
        };
        fs::write(
            dir.join("package.jet"),
            format!("name: \"sentry-profile\"\nversion: \"0.1.0\"\n{policy}"),
        )
        .unwrap();
        fs::write(dir.join("main.jet"), source).unwrap();

        let output = Command::new(jet())
            .args(["run", "--release", "main.jet"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if hardened {
            assert!(!output.status.success(), "hardened profile missed the stale pointer: {stderr}");
            for marker in [
                "Runtime fault [R0802]",
                "pointer is used only after arena reset to prove quarantine",
                "Why:",
                "Fix:",
                "obligation `valid_ptr` was not met on this run",
            ] {
                assert!(stderr.contains(marker), "hardened report missing `{marker}`: {stderr}");
            }
        } else {
            assert!(
                output.status.success(),
                "normal release must run the deliberately wrong region: {stderr}"
            );
            assert_eq!(stdout, "7\n", "normal release did not execute the fixture: {stderr}");
            assert!(!stderr.contains("Runtime fault [R0802]"), "normal release was hardened: {stderr}");
        }
    }
}

#[test]
fn safe_release_profiles_emit_no_sentry_runtime_overhead() {
    for (tag, policy) in [
        ("safe_release_normal", ""),
        ("safe_release_hardened", "policy: .{ harden: true }\n"),
    ] {
        let dir = isolated_cwd(tag);
        fs::write(
            dir.join("package.jet"),
            format!("name: \"safe-profile\"\nversion: \"0.1.0\"\n{policy}"),
        )
        .unwrap();
        fs::write(dir.join("main.jet"), "fn run() { print(7) }\n").unwrap();
        let path = dir.join("main.jet");
        let shown = path.to_string_lossy();
        let output = jet::compile_with_target_and_gates_and_profile(
            "",
            &shown,
            jet::Policy::GateSet::default(),
            None,
            "release",
        )
        .unwrap_or_else(|diags| panic!("safe profile rejected: {diags:?}"));
        assert!(
            !output.rust.contains("jet_mem::jet_sentry_set_hardened("),
            "safe {tag} output carries sentry machinery:\n{}",
            output.rust
        );
    }
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
    assert_eq!(
        single.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&single.stderr)
    );
    let single = String::from_utf8(single.stdout).unwrap();
    assert!(
        single.contains("scope: single-file")
            && single.contains("externs")
            && single.contains("TRUSTED"),
        "{single}"
    );
    check_snapshot("inspect_guarantees_single_file.txt", &single);

    let freestanding = Command::new(jet())
        .args(["inspect", "guarantees", "--freestanding", "main.jet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        freestanding.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&freestanding.stderr)
    );
    let freestanding = String::from_utf8(freestanding.stdout).unwrap();
    assert!(
        freestanding.contains("target: freestanding")
            && freestanding.contains("prover + audit only"),
        "{freestanding}"
    );
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
    assert!(
        stderr.contains("Error [E0355]")
            && stderr.contains("contain")
            && stderr.contains("not a scoped policy"),
        "{stderr}"
    );
    let teaching = stderr
        .lines()
        .filter(|line| {
            line.starts_with("Error [") || line.starts_with(" Why:") || line.starts_with(" Fix:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    check_snapshot(
        "inspect_guarantees_source_policy.txt",
        &format!("{teaching}\n"),
    );
}

#[test]
fn inspect_dossier_snapshots_program_allocator_fact() {
    let dir = isolated_cwd("inspect_dossier_program_allocator");
    fs::write(
        dir.join("package.jet"),
        "name: \"allocator_dossier\"\nversion: \"0.1.0\"\nallocator: mem.Counting.over(mem.Heap, cap: 2.gb)\n",
    )
    .unwrap();
    fs::write(dir.join("main.jet"), "fn run() { print(\"ok\") }\n").unwrap();
    let output = Command::new(jet())
        .args(["inspect", "dossier", "main.jet", "run", "--json"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).unwrap();
    let allocator = json
        .split_once("\"program_allocator\":")
        .and_then(|(_, tail)| tail.split_once(",\"performance_budgets\""))
        .map(|(allocator, _)| allocator)
        .expect("dossier must project the program allocator before performance budgets");
    check_snapshot(
        "inspect_dossier_program_allocator.json",
        &format!("{{\"program_allocator\":{allocator}}}\n"),
    );
}
