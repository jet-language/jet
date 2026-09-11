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
fn package_team_policy_rejects_effect_unsafe_expert_and_dependency_violations() {
    for (tag, policy, source, expected_rule) in [
        (
            "team_policy_effect",
            "effects: { \"main.jet\": -[]> }",
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
                "name: \"guarantees\"\nversion: \"0.1.0\"\ndeps: {{ zlib: c@system }}\npolicy: {{ {policy} }}\n"
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
fn hardened_release_sentry_reaches_a_foreign_dependency() {
    let dir = isolated_cwd("inspect_guarantees_runtime_harden");
    let app = dir.join("app");
    let dependency = dir.join("dep");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&dependency).unwrap();
    fs::write(
        app.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\ndeps: { dep: ../dep }\npolicy: { harden: true }\n",
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
            "name: \"sentry-profile\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
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
            "name: \"test-sentry-profile\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n",
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
            assert_eq!(
                output.status.code(),
                Some(1),
                "hardened test must report a failed suite: {stderr}"
            );
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
            assert!(
                !report.contains("panicked at") && !report.contains("RUST_BACKTRACE"),
                "hardened test leaked Rust panic voice: {report}"
            );
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
        ("safe_release_hardened", "policy: { harden: true }\n", "hardened"),
    ] {
        let dir = isolated_cwd(tag);
        fs::write(
            dir.join("package.jet"),
            format!("name: \"safe-profile\"\nversion: \"0.1.0\"\n{policy}"),
        )
        .unwrap();
        fs::write(dir.join("main.jet"), "fn run() { print(7) }\n").unwrap();
        let path = dir.join("main.jet");
        let src = fs::read_to_string(&path).unwrap();
        let shown = path.to_string_lossy();
        let output = jet::compile_with_target_and_gates_and_profile(
            &src,
            &shown,
            jet::Policy::GateSet::default(),
            None,
            profile,
        )
        .unwrap_or_else(|diags| panic!("safe profile rejected: {diags:?}"));
        assert!(
            !output.rust.contains("jet_mem::jet_sentry_"),
            "safe {tag} output carries sentry runtime machinery:\n{}",
            output.rust
        );
    }
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

