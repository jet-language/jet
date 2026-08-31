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
fn inspect_guarantees_projects_normalized_team_policy() {
    let dir = isolated_cwd("inspect_guarantees_team_policy");
    fs::write(
        dir.join("package.jet"),
        "name: \"guarantees\"\nversion: \"0.1.0\"\ndeps: .{ zlib: c@system }\npolicy: .{ effects: .{ \"src\": -[IO, IO]> }, unsafe: .Paths([\"./src\", \"src\"]), expert: .Deny, deps: .List([\"zlib\", \"zlib\"]), lints: .{ deny: [float_money] } }\n",
    )
    .unwrap();
    fs::write(dir.join("main.jet"), "fn run() {}\n").unwrap();

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
    for row in [
        "policy.effects:\n  src: -[IO]>",
        "policy.unsafe: .Paths([\"src\"])",
        "policy.expert: .Deny",
        "policy.deps: .List([\"zlib\"])",
        "policy.lints.deny: [\"float_money\"]",
    ] {
        assert!(human.contains(row), "normalized policy row missing `{row}`:\n{human}");
    }

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
    assert!(parse_json(&json).is_ok(), "guarantee policy JSON must parse: {json}");
    for row in [
        "\"path\":\"src\",\"ceiling\":[\"IO\"]",
        "\"unsafe\":{\"mode\":\"paths\",\"paths\":[\"src\"]}",
        "\"expert\":false",
        "\"deps\":[\"zlib\"]",
        "\"lints\":{\"deny\":[\"float_money\"]}",
    ] {
        assert!(json.contains(row), "normalized policy JSON row missing `{row}`:\n{json}");
    }
}

#[test]
fn inspect_guarantees_projects_inherited_package_policy_and_grants() {
    let dir = isolated_cwd("inspect_guarantees_inherited_policy");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"guarantees\"\nversion: \"0.1.0\"\n\
         deps: .{ dep: c@system }\n\
         authority: .{ holds: { allow: [IO] }, grants: .{ \"dep\": [FS.Read] } }\n\
         policy: .{ effects: .{ \"src\": -[IO]> }, unsafe: .Paths([\"src/ffi\"]), \
         expert: .Deny, deps: .List([\"dep\"]), lints: .{ deny: [] })\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.jet"), "fn run() {}\n").unwrap();

    let human = Command::new(jet())
        .args(["inspect", "guarantees", "src/main.jet"])
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
    for row in [
        "policy.effects:\n  src: -[IO]>",
        "policy.unsafe: .Paths([\"src/ffi\"])",
        "policy.expert: .Deny",
        "policy.deps: .List([\"dep\"])",
        "policy.lints.deny: []",
        "authority.holds.allow",
        "authority.grants.dep",
        "granted effects: FS.Read",
    ] {
        assert!(human.contains(row), "inherited policy row missing `{row}`:\n{human}");
    }

    let json = Command::new(jet())
        .args(["inspect", "guarantees", "src/main.jet", "--json"])
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
    assert!(parse_json(&json).is_ok(), "guarantee JSON must parse: {json}");
    for row in [
        "\"path\":\"src\",\"ceiling\":[\"IO\"]",
        "\"unsafe\":{\"mode\":\"paths\",\"paths\":[\"src/ffi\"]}",
        "\"expert\":false",
        "\"deps\":[\"dep\"]",
        "\"lints\":{\"deny\":[]}",
        "\"component\":\"authority.holds.allow\"",
        "\"component\":\"authority.grants.dep\"",
    ] {
        assert!(json.contains(row), "inherited policy JSON row missing `{row}`:\n{json}");
    }
}

#[test]
fn inspect_guarantees_reports_source_less_dependencies_once_and_sorted() {
    let dir = isolated_cwd("inspect_guarantees_dependency_facts");
    fs::write(
        dir.join("package.jet"),
        "name: \"guarantees\"\nversion: \"0.1.0\"\n\
         deps: .{ a_dep: c@system, m_dep: c@system, z_dep: js@\"widget#version=1@npm\" }\n\
         policy: .{ deps: .List([\"a_dep\", \"m_dep\", \"z_dep\"]) }\n",
    )
    .unwrap();
    fs::write(dir.join("main.jet"), "fn run() {}\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "guarantees", "main.jet", "--json"])
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
    assert!(parse_json(&json).is_ok(), "guarantee JSON must parse: {json}");
    for dependency in ["a_dep", "m_dep", "z_dep"] {
        let component = format!("\"component\":\"{dependency}\"");
        assert_eq!(
            json.matches(component.as_str()).count(),
            1,
            "dependency component must appear once: {dependency}: {json}"
        );
    }
    let a = json.find("\"component\":\"a_dep\"").unwrap();
    let m = json.find("\"component\":\"m_dep\"").unwrap();
    let z = json.find("\"component\":\"z_dep\"").unwrap();
    assert!(a < m && m < z, "dependency components must be sorted: {json}");
}

#[test]
fn package_team_policy_rejects_effect_unsafe_expert_and_dependency_violations() {
    for (tag, policy, source, expected_rule) in [
        (
            "team_policy_effect",
            "effects: .{ \"main.jet\": -[]> }",
            "fn run() { print(\"outside the ceiling\") }\n",
            "policy.effects",
        ),
        (
            "team_policy_unsafe",
            "unsafe: .Paths([\"src\"])",
            "fn run() {\n    #Unsafe(\"audit\") {}\n}\n",
            "policy.unsafe",
        ),
        (
            "team_policy_expert",
            "expert: .Deny",
            "fn run() {\n    #Shield {}\n}\n",
            "policy.expert",
        ),
        (
            "team_policy_dependency",
            "deps: .List([\"allowed\"])",
            "fn run() {}\n",
            "policy.deps",
        ),
    ] {
        let dir = isolated_cwd(tag);
        fs::write(
            dir.join("package.jet"),
            format!(
                "name: \"guarantees\"\nversion: \"0.1.0\"\ndeps: .{{ zlib: c@system }}\npolicy: .{{ {policy} }}\n"
            ),
        )
        .unwrap();
        fs::write(dir.join("main.jet"), source).unwrap();
        let output = Command::new(jet())
            .args(["check", "main.jet"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{tag} unexpectedly passed:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Error [E2960]") && stderr.contains(expected_rule),
            "{tag} did not report {expected_rule}:\n{stderr}"
        );
        assert!(
            stderr.contains("Package policy") && stderr.contains("package.jet"),
            "{tag} lacks actionable policy text:\n{stderr}"
        );
    }
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
    for (tag, profile, hardened) in [
        ("release_sentry_normal", "release", false),
        ("release_sentry_hardened", "hardened", true),
    ] {
        let dir = isolated_cwd(tag);
        fs::write(
            dir.join("package.jet"),
            "name: \"sentry-profile\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [IO, Mem.Alloc] } }\n",
        )
        .unwrap();
        fs::write(dir.join("main.jet"), source).unwrap();

        let profile_arg = format!("--profile={profile}");
        let output = Command::new(jet())
            .args(["run", profile_arg.as_str(), "main.jet"])
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
                "in #Unsafe gate",
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
fn test_hardened_profile_catches_a_local_wrong_unsafe_region() {
    let source = "\
use core.mem
#Test(\"wrong unsafe region\") {
    arena :: mem.Arena.new()
    #Unsafe(\"test deliberately reads after arena reset\") {
        stale :: *Int{*arena.alloc(7)}
        arena.reset()
        print(stale.*)
    }
}
fn run() {}
";
    for (tag, profile, hardened) in [
        ("test_release_sentry_normal", "release", false),
        ("test_hardened_sentry", "hardened", true),
    ] {
        let dir = isolated_cwd(tag);
        fs::write(
            dir.join("package.jet"),
            "name: \"test-sentry-profile\"\nversion: \"0.1.0\"\nauthority: .{ holds: { allow: [IO, Mem.Alloc] } }\n",
        )
        .unwrap();
        fs::write(dir.join("main.jet"), source).unwrap();
        let profile_arg = format!("--profile={profile}");
        let output = Command::new(jet())
            .args(["test", "main.jet", profile_arg.as_str()])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if hardened {
            assert!(!output.status.success(), "hardened test missed stale pointer: {stderr}");
            let report = format!("{stdout}{stderr}");
            for marker in [
                "Runtime fault [R0802]",
                "test deliberately reads after arena reset",
                "Why:",
                "Fix:",
                "obligation `valid_ptr` was not met on this run",
            ] {
                assert!(report.contains(marker), "hardened test report missing `{marker}`: {report}");
            }
        } else {
            assert!(
                output.status.success(),
                "normal test profile must run the deliberately wrong region: {stderr}"
            );
            assert!(stdout.contains("1 passed"), "normal test did not run: {stdout}");
            assert!(!stdout.contains("Runtime fault [R0802]") && !stderr.contains("Runtime fault [R0802]"), "normal test was hardened: {stdout}{stderr}");
        }
    }
}

#[test]
fn safe_release_profiles_emit_no_sentry_runtime_overhead() {
    for (tag, policy, profile) in [
        ("safe_release_normal", "", "release"),
        ("safe_release_hardened", "policy: .{ harden: true }\n", "hardened"),
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
            profile,
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
