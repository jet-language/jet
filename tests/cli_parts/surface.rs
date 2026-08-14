use super::*;

#[test]
fn target_equals_and_space_forms_match_for_build_run_and_dev() {
    let host = String::from_utf8(
        Command::new("rustc")
            .args(["-vV"])
            .output()
            .expect("rustc must report its host target")
            .stdout,
    )
    .unwrap()
    .lines()
    .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
    .expect("rustc -vV must include a host target");

    let target = [
        "wasm32-unknown-unknown",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-gnu",
    ]
    .into_iter()
    .find(|candidate| {
        if *candidate == host {
            return false;
        }
        let Ok(output) = Command::new("rustc")
            .args(["--print", "target-libdir", "--target", candidate])
            .output()
        else {
            return false;
        };
        output.status.success()
            && Path::new(String::from_utf8_lossy(&output.stdout).trim()).is_dir()
    })
    .expect("rustc must provide an installed non-host target");

    let invoke = |tag: &str, command: &str, source: &str, target: Option<&str>, spaced: bool| {
        let dir = isolated_cwd(tag);
        fs::write(dir.join("main.jet"), source).unwrap();
        let mut args = vec![command.to_string(), "main.jet".to_string()];
        if spaced {
            args.push("--target".to_string());
            if let Some(target) = target {
                args.push(target.to_string());
            }
        } else if let Some(target) = target {
            args.push(format!("--target={target}"));
        }
        Command::new(jet())
            .args(args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };

    for (command, source) in [
        ("build", "fn run() { print(\"target\") }\n"),
        ("run", "fn run() { print(\"target\") }\n"),
        ("dev", "fn dev() { print(\"target\") }\n"),
    ] {
        let equals = invoke(&format!("target_{command}_equals"), command, source, Some(target), false);
        let space = invoke(&format!("target_{command}_space"), command, source, Some(target), true);
        assert_eq!(equals.status.code(), Some(0), "{command} equals failed: {}", String::from_utf8_lossy(&equals.stderr));
        assert_eq!(space.status.code(), Some(0), "{command} space failed: {}", String::from_utf8_lossy(&space.stderr));
        assert_eq!(equals.stdout, space.stdout, "{command} target forms changed stdout");
        assert_eq!(equals.stderr, space.stderr, "{command} target forms changed stderr");
    }

    let unknown = "definitely-not-a-rust-target";
    for (command, source) in [
        ("build", "fn run() {}\n"),
        ("run", "fn run() {}\n"),
        ("dev", "fn run() {}\n"),
    ] {
        let equals = invoke(&format!("target_{command}_unknown_equals"), command, source, Some(unknown), false);
        let space = invoke(&format!("target_{command}_unknown_space"), command, source, Some(unknown), true);
        assert_eq!(equals.status.code(), Some(1), "{command} equals did not reject target: {}", String::from_utf8_lossy(&equals.stderr));
        assert_eq!(space.status.code(), Some(1), "{command} space did not reject target: {}", String::from_utf8_lossy(&space.stderr));
        assert_eq!(equals.stderr, space.stderr, "{command} target forms changed normalized E3302 output");
        let stderr = String::from_utf8_lossy(&equals.stderr);
        assert!(stderr.contains("Error [E3302]:"), "{command} missing E3302: {stderr}");
        check_snapshot("unknown_target_e3302.txt", &stderr);
    }

    for (command, source) in [
        ("build", "fn run() {}\n"),
        ("run", "fn run() {}\n"),
        ("dev", "fn run() {}\n"),
    ] {
        for (suffix, spaced) in [("space", true), ("equals", false)] {
            let value = if spaced { None } else { Some("") };
            let out = invoke(&format!("target_{command}_missing_{suffix}"), command, source, value, spaced);
            assert_eq!(out.status.code(), Some(2), "{command} missing target value was not rejected: {}", String::from_utf8_lossy(&out.stderr));
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("Error [E2104]:"), "{command} missing target value lost E2104: {stderr}");
            assert!(stderr.contains("`--target` needs a value"), "{command} missing target value was not truthful: {stderr}");
        }
    }
}

#[test]
fn external_completion_preserves_checked_subcommands() {
    let dir = isolated_cwd("shape_cli_subcommands");
    fs::write(dir.join("commands.jet"), r#"#CLI
struct ServeArgs {
    #[Doc("port to listen on"), Short("p"), Env("JET_SERVE_PORT")] port: Int = 3000
}
#CLI
struct ImportArgs {
    #Doc("file to import") file: String
}
enum Cmd { Serve(ServeArgs) Import(ImportArgs) }
fn run(cmd: Cmd) {}
"#).unwrap();
    let build = Command::new(jet()).args(["build", "commands.jet"]).current_dir(&dir).output().unwrap();
    assert!(build.status.success(), "subcommand build failed: {}", String::from_utf8_lossy(&build.stderr));
    let help = Command::new(dir.join("build/commands"))
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(help.status.success(), "subcommand root help failed: {}", String::from_utf8_lossy(&help.stderr));
    let help = String::from_utf8(help.stdout).unwrap();
    assert_eq!(help, "Usage: commands <command> [options]\n\nCommands:\n  serve\n  import\n");
    assert!(!help.contains("Serve") && !help.contains("Import"));

    for shell in ["bash", "zsh", "fish", "powershell"] {
        let completion = Command::new(jet())
            .args(["self", "completions", shell, "--for", "build/commands"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(completion.status.success(), "{shell} subcommand completion failed: {}", String::from_utf8_lossy(&completion.stderr));
        let script = String::from_utf8(completion.stdout).unwrap();
        let expected = match shell {
            "bash" => ["serve", "import", "--port -p", "file --file"],
            "zsh" => ["serve", "import", "{-p,--port}", ":file:file to import"],
            "fish" => ["serve", "import", "-l port -s p", "-l file"],
            "powershell" => ["serve", "import", "'--port','-p'", "'file','--file'"],
            _ => unreachable!(),
        };
        for fragment in expected {
            assert!(script.contains(fragment), "{shell} external completion omitted {fragment}: {script}");
        }
        check_snapshot(&format!("shape_cli_enum_{shell}.txt"), &script);
    }
    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "commands.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(dossier.contains("\"completion_words\":[\"--help\",\"serve\",\"import\"]"), "dossier flattened enum flags: {dossier}");
    for fact in ["\"commands\":[", "\"name\":\"serve\"", "\"name\":\"import\"", "\"flag\":\"--port\"", "\"flag\":\"--file\""] {
        assert!(dossier.contains(fact), "dossier omitted {fact}: {dossier}");
    }
    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "commands.jet", "run"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "command serve",
        "-p / --port: Int (optional, default 3000, env JET_SERVE_PORT) — port to listen on",
        "command import",
        "--file: String (required) — file to import",
    ] {
        assert!(dossier.contains(fact), "text dossier omitted {fact}: {dossier}");
    }
}

#[test]
fn documented_subcommands_are_projected_into_the_dossier_schema() {
    let dir = isolated_cwd("shape_cli_documented_dossier");
    fs::write(
        dir.join("commands.jet"),
        r#"#CLI
struct ServeArgs {}
#CLI
struct ImportArgs {}
#Doc("Manage the service")
enum Cmd {
    #Doc("Start the service") Serve(ServeArgs)
    #Doc("Import one data file") Import(ImportArgs)
}
fn run(cmd: Cmd) {}
"#,
    )
    .unwrap();
    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "commands.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "documented dossier failed: {}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    let json = String::from_utf8(dossier.stdout).unwrap();
    for fact in [
        "\"description\":\"Manage the service\"",
        "\"name\":\"serve\",\"description\":\"Start the service\"",
        "\"name\":\"import\",\"description\":\"Import one data file\"",
        "\"completion_words\":[\"--help\",\"serve\",\"import\"]",
    ] {
        assert!(json.contains(fact), "documented dossier omitted {fact}: {json}");
    }
}

#[test]
fn derived_help_uses_program_basename_for_compiled_and_jet_run_paths() {
    let dir = isolated_cwd("shape_cli_help_program_name");
    fs::write(
        dir.join("typed.jet"),
        "#CLI\nstruct RunArgs { verbose: Bool }\nfn run(args: RunArgs) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "typed.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "typed build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built = dir.join("build/typed").canonicalize().unwrap();
    let compiled_help = Command::new(&built)
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        compiled_help.status.success(),
        "compiled typed help failed: {}",
        String::from_utf8_lossy(&compiled_help.stderr)
    );

    let run_help = Command::new(jet())
        .args(["run", "typed.jet", "--", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run_help.status.success(),
        "jet run typed help failed: {}",
        String::from_utf8_lossy(&run_help.stderr)
    );

    let program_names = format!(
        "compiled: {}\njet run: {}\n",
        String::from_utf8(compiled_help.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
        String::from_utf8(run_help.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
    );
    check_snapshot("shape_cli_help_program_names.txt", &program_names);
}

#[test]
fn derived_enum_help_uses_program_basename_for_compiled_and_jet_run_paths() {
    let dir = isolated_cwd("shape_cli_enum_help_program_name");
    fs::write(
        dir.join("commands.jet"),
        "#CLI\nstruct ServeArgs { verbose: Bool }\nenum Cmd { Serve(ServeArgs) }\nfn run(cmd: Cmd) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "commands.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "enum build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let built = dir.join("build/commands").canonicalize().unwrap();
    let compiled_root = Command::new(&built)
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    let compiled_sub = Command::new(&built)
        .args(["serve", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let run_root = Command::new(jet())
        .args(["run", "commands.jet", "--", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let run_sub = Command::new(jet())
        .args(["run", "commands.jet", "--", "serve", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    for (label, output) in [
        ("compiled root", &compiled_root),
        ("compiled subcommand", &compiled_sub),
        ("jet run root", &run_root),
        ("jet run subcommand", &run_sub),
    ] {
        assert!(
            output.status.success(),
            "{label} help failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let first_line = |output: Output| {
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    let program_names = format!(
        "compiled root: {}\ncompiled subcommand: {}\njet run root: {}\njet run subcommand: {}\n",
        first_line(compiled_root),
        first_line(compiled_sub),
        first_line(run_root),
        first_line(run_sub)
    );
    check_snapshot("shape_cli_enum_help_program_names.txt", &program_names);
}

#[test]
fn moved_bare_commands_are_teaching_errors_not_aliases() {
    for (verb, replacement) in [
        ("publish", "jet registry publish"),
        ("semindex", "jet inspect semindex"),
        ("doctor", "jet self doctor"),
        ("lsp", "jet self lsp"),
        ("push", "jet os push"),
    ] {
        let out = Command::new(jet()).arg(verb).arg("sentinel").output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{verb} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{verb}: {stderr}");
        assert!(stderr.contains(replacement), "{verb}: {stderr}");
    }

    let out = Command::new(jet()).args(["lsp", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"code\":\"E2101\""));
    assert!(stdout.contains("jet self lsp --json"));
}

/// D-ONCE-RETIRE1=C / D-CLI-STORE2=A / D-CLI-DEVSERVE1=A: semantic retirements
/// have zero rows in every generated CLI surface but still reach their one
/// registry-owned teaching refusal.
#[test]
fn retired_cli_routes_are_absent_but_teach_real_spelling() {
    let surfaces = [
        jet::CLI::usage_page("0.0.0"),
        jet::CLI::man_page("0.0.0"),
        jet::CLI::completions_bash(),
        jet::CLI::completions_zsh(),
        jet::CLI::completions_fish(),
        jet::CLI::completions_powershell(),
    ];
    let top_help = Command::new(jet()).arg("--help").output().unwrap();
    assert!(top_help.status.success(), "jet --help failed: {:?}", top_help);
    let top_help = String::from_utf8_lossy(&top_help.stdout);
    for retired in jet::CLI::RETIRED_COMMANDS {
        let needle = format!("jet {}", retired.spelling);
        assert!(
            surfaces.iter().all(|surface| !surface.contains(&needle)),
            "retired route leaked into generated surface: {needle}"
        );
        assert!(!top_help.contains(&needle), "retired route leaked into `jet --help`: {needle}");
    }
    for (argv, replacement) in [
        (vec!["gc"], "jet clean"),
        (vec!["serve"], "jet dev <file.jet> --swap"),
        (vec!["store"], "jet hangar / jet clean / jet fetch"),
        (vec!["store", "fetch"], "jet fetch"),
        (vec!["store", "verify"], "jet hangar verify"),
        (vec!["store", "generations"], "jet hangar generations"),
        (vec!["store", "rollback", "2"], "jet hangar rollback 2"),
        (vec!["store", "gc"], "jet clean"),
        (vec!["store", "lock", "stats.jet"], "jet fetch --lock stats.jet"),
        (vec!["serve", "main.jet"], "jet dev main.jet --swap"),
        (vec!["lock", "stats.jet"], "jet fetch --lock stats.jet"),
    ] {
        let out = Command::new(jet()).args(&argv).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{argv:?} must be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101"), "{argv:?}: {stderr}");
        assert!(stderr.contains(replacement), "{argv:?}: {stderr}");
    }
}

#[test]
fn moved_command_registry_agrees_with_dispatch_exceptions() {
    let mut declared = Vec::new();
    for group in jet::CLI::command_groups() {
        for action in group.actions {
            let dispatch_exempts = jet::CLI::moved_command(action.name).is_none()
                && action.name != "install";
            assert_eq!(
                action.also_canonical_top_level,
                dispatch_exempts,
                "registry and moved_command disagree for {} {}",
                group.name,
                action.name
            );
            if action.also_canonical_top_level {
                declared.push(format!("{} {}", group.name, action.name));
            }
        }
    }
    assert_eq!(
        declared,
        vec!["hangar import", "gc report", "env test", "perf run", "perf test", "perf bench"]
    );
}

#[test]
fn test_and_bench_help_keep_shared_runner_flags_paired() {
    let test_help = Command::new(jet()).args(["test", "--help"]).env("NO_COLOR", "1").output().unwrap();
    let bench_help = Command::new(jet()).args(["bench", "--help"]).env("NO_COLOR", "1").output().unwrap();
    assert!(test_help.status.success(), "jet test --help failed: {}", String::from_utf8_lossy(&test_help.stderr));
    assert!(bench_help.status.success(), "jet bench --help failed: {}", String::from_utf8_lossy(&bench_help.stderr));
    let test_help = String::from_utf8_lossy(&test_help.stdout);
    let bench_help = String::from_utf8_lossy(&bench_help.stdout);
    assert!(test_help.contains("--filter"), "test help lost shared filter flag: {test_help}");
    assert!(bench_help.contains("--filter"), "bench help missing shared filter flag: {bench_help}");
    for flag in ["--shuffle", "--coverage", "--update-snapshots", "--serial"] {
        assert!(!bench_help.contains(flag), "bench help exposed test-only flag {flag}: {bench_help}");
    }
}

#[test]
fn coverage_help_matches_instrumentation() {
    const COVERAGE_HELP: &str = "function and branch coverage";
    let help = Command::new(jet())
        .args(["test", "--help"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        help.status.success(),
        "jet test --help failed: {}",
        String::from_utf8_lossy(&help.stderr)
    );
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("--coverage") && help.contains(COVERAGE_HELP), "{help}");
    assert!(!help.contains("function and line coverage"), "{help}");

    for shell in ["bash", "fish", "zsh", "powershell"] {
        let completion = Command::new(jet())
            .args(["self", "completions", shell])
            .output()
            .unwrap();
        assert!(
            completion.status.success(),
            "{shell} completion failed: {}",
            String::from_utf8_lossy(&completion.stderr)
        );
        let script = String::from_utf8_lossy(&completion.stdout);
        assert!(script.contains("--coverage"), "{shell} omitted --coverage");
        if matches!(shell, "fish" | "zsh") {
            assert!(script.contains(COVERAGE_HELP), "{shell}: {script}");
        }
        assert!(!script.contains("function and line coverage"), "{shell}: {script}");
        check_snapshot(&format!("completions_{shell}.txt"), &script);
    }

    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert!(man.status.success(), "man page failed: {}", String::from_utf8_lossy(&man.stderr));
    let man = String::from_utf8_lossy(&man.stdout);
    assert!(man.contains(COVERAGE_HELP), "{man}");
    assert!(!man.contains("function and line coverage"), "{man}");
    check_snapshot("man.txt", &man);
}

#[test]
fn bare_dev_uses_file_scoped_run() {
    let out = Command::new(jet()).args(["dev", "--no-color"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("`jet dev` needs a file to watch"), "{stderr}");
    assert!(stderr.contains("jet dev <file.jet>"), "{stderr}");
}

#[test]
fn every_moved_bare_action_is_e2101_in_human_and_json_modes() {
    for group in jet::CLI::command_groups() {
        for action in group.actions {
            // `import` names both the canonical source translator and a
            // physical hangar action. Only actions without a canonical
            // top-level meaning are moved bare commands.
            if jet::CLI::is_canonical_top_level(action.name) {
                continue;
            }
            let owner = jet::CLI::moved_command_group(action.name).unwrap_or(group.name);
            let replacement = format!("jet {} {}", owner, action.name);
            let out = Command::new(jet()).arg(action.name).arg("sentinel").output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {}", action.name);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("E2101") && stderr.contains(&replacement), "{}: {stderr}", action.name);

            let out = Command::new(jet()).args([action.name, "sentinel\\\"quoted", "--json"]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "bare {} --json", action.name);
            assert!(out.stderr.is_empty(), "JSON diagnostic leaked stderr for {}", action.name);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("\"code\":\"E2101\"") && stdout.contains(&replacement), "{}: {stdout}", action.name);
            assert!(stdout.contains("sentinel\\\\\\\"quoted"), "replacement was not JSON escaped: {stdout}");
        }
    }
}

#[test]
fn invalid_nested_action_is_e2101_and_json_escaped() {
    let bad = "bad\\\"action";
    // D-CLI-SURFACE3=B: `os` is not exhaustive (see `CommandSpec::exhaustive`)
    // — an unmodeled subword falls through to the real `jet os` dispatcher,
    // which teaches its own (non-E2101) "not a jetos verb" error, not this
    // registry's generic invalid-action path.
    for group in jet::CLI::command_groups().filter(|g| g.exhaustive) {
        let out = Command::new(jet()).args([group.name, bad]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("E2101") && stderr.contains(bad), "{stderr}");

        let out = Command::new(jet()).args([group.name, bad, "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("bad\\\\\\\"action"), "invalid JSON escaping: {stdout}");
        assert!(!stdout.contains("`bad\\\"action`"), "raw quote leaked into JSON: {stdout}");
    }
}

#[test]
fn grouped_e2101_human_and_json_goldens() {
    let moved = Command::new(jet()).args(["publish", "sentinel"]).output().unwrap();
    assert_eq!(moved.status.code(), Some(2));
    assert!(moved.stdout.is_empty());
    check_snapshot("moved_bare_e2101_human.txt", &String::from_utf8_lossy(&moved.stderr));

    let moved_json = Command::new(jet()).args(["publish", "sentinel\\\"quoted", "--json"]).output().unwrap();
    assert_eq!(moved_json.status.code(), Some(2));
    assert!(moved_json.stderr.is_empty());
    check_snapshot("moved_bare_e2101_json.txt", &String::from_utf8_lossy(&moved_json.stdout));

    let invalid = Command::new(jet()).args(["inspect", "bad\\\"action"]).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    check_snapshot("invalid_nested_e2101_human.txt", &String::from_utf8_lossy(&invalid.stderr));

    let invalid_json = Command::new(jet()).args(["inspect", "bad\\\"action", "--json"]).output().unwrap();
    assert_eq!(invalid_json.status.code(), Some(2));
    assert!(invalid_json.stderr.is_empty());
    check_snapshot("invalid_nested_e2101_json.txt", &String::from_utf8_lossy(&invalid_json.stdout));
}

#[test]
fn group_help_and_man_inventory_every_nested_description() {
    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert_eq!(man.status.code(), Some(0));
    let man = String::from_utf8_lossy(&man.stdout);
    for group in jet::CLI::command_groups() {
        // D-CLI-SURFACE3=B: a non-exhaustive group (`os`) doesn't own its bare
        // `help` output — that stays the real `jet os` dispatcher's, which
        // this registry can't predict — so only the *static* man-page
        // inventory is checked for it. An exhaustive group's `help` is
        // CLI-owned and must list every action.
        if group.exhaustive {
            let out = Command::new(jet()).args([group.name, "help"]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let help = String::from_utf8_lossy(&out.stdout);
            assert!(help.contains(group.summary), "{} help missing summary", group.name);
            for action in group.actions {
                assert!(help.contains(action.name) && help.contains(action.summary), "{} help missing {}", group.name, action.name);
            }
        }
        for action in group.actions {
            assert!(man.contains(&format!(".B {} {}", group.name, action.name)), "man missing {} {}", group.name, action.name);
            assert!(man.contains(action.summary), "man missing summary for {} {}", group.name, action.name);
        }
    }
}

#[test]
fn palette_uses_canonical_nested_routes() {
    for group in jet::CLI::command_groups() {
        for action in group.actions {
            let route = format!("{} {}", group.name, action.name);
            let out = Command::new(jet()).args(["?", &route]).output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains(&route), "palette missing {route}: {stdout}");
            assert!(!stdout.contains(&format!("jet {}   ", action.name)), "palette advertised bare moved action {}", action.name);
        }
    }
}

#[test]
fn jet_install_teaches_jet_fetch() {
    // `jet install` is not a Jet command; the compiler emits E0043 pointing to `jet fetch`.
    let out = Command::new(jet()).arg("install").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0043"),
        "`jet install` should emit E0043 teaching error:\n{stderr}"
    );
    assert!(
        stderr.contains("jet fetch"),
        "`jet install` error should mention `jet fetch`:\n{stderr}"
    );
}

#[test]
fn exit_code_explain_unknown() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E9999")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "unknown code should exit 1");
}

#[test]
fn check_human_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_human.txt", &stderr);
}

#[test]
fn check_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("check_json.txt", &stderr);
}

#[test]
fn machine_report_paths stay_resolvable_across_repository_layouts() {
    let dir = isolated_cwd("machine_report_paths");
    let runner = dir.join("runner");
    fs::create_dir_all(&runner).unwrap();

    let report = |path: &Path| {
        let output = Command::new(jet())
            .args(["check", path.to_str().unwrap(), "--json"])
            .current_dir(&runner)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
        parse_json(stderr.trim()).unwrap_or_else(|_| panic!("invalid diagnostic JSON: {stderr}"))
    };
    let report_file = |value: &jet_foundation::JSON::JSONValue| {
        jet_foundation::JSON::json_str(
            jet_foundation::JSON::json_get(value, "file").unwrap(),
        )
        .unwrap()
        .to_string()
    };

    let outside = dir.join("outside/bad.jet");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, "fn run() {\n    pirnt(\"hi\")\n}\n").unwrap();
    assert_eq!(report_file(&report(&outside)), outside.display().to_string());

    let inside = dir.join("repo/src/bad.jet");
    fs::create_dir_all(inside.parent().unwrap()).unwrap();
    fs::create_dir_all(dir.join("repo/.git")).unwrap();
    fs::write(&inside, "fn run() {\n    pirnt(\"hi\")\n}\n").unwrap();
    assert_eq!(report_file(&report(&inside)), inside.display().to_string());

    let stray_root = dir.join("stray");
    let stray = stray_root.join("nested/bad.jet");
    fs::create_dir_all(stray.parent().unwrap()).unwrap();
    fs::create_dir_all(stray_root.join(".git")).unwrap();
    fs::write(&stray, "fn run() {\n    pirnt(\"hi\")\n}\n").unwrap();
    let with_ancestor_git = report_file(&report(&stray));
    fs::remove_dir(stray_root.join(".git")).unwrap();
    let without_ancestor_git = report_file(&report(&stray));
    assert_eq!(with_ancestor_git, without_ancestor_git);
    assert_eq!(without_ancestor_git, stray.display().to_string());

    let fix = dir.join("fix/bad.jet");
    fs::create_dir_all(fix.parent().unwrap()).unwrap();
    fs::write(&fix, "fn run() {\n    println(\"hi\")\n}\n").unwrap();
    let fix_report = report(&fix);
    let fix_file = report_file(&fix_report);
    let edits = match jet_foundation::JSON::json_get(&fix_report, "fix_edits").unwrap() {
        jet_foundation::JSON::JSONValue::Array(edits) => edits,
        _ => panic!("fix_edits is not an array"),
    };
    assert_eq!(edits.len(), 1);
    let edit = match &edits[0] {
        jet_foundation::JSON::JSONValue::Object(edit) => edit,
        _ => panic!("fix edit is not an object"),
    };
    let edit_file = jet_foundation::JSON::json_str(
        jet_foundation::JSON::json_get(&edits[0], "file").unwrap(),
    )
    .unwrap();
    assert_eq!(fix_file, edit_file);
    let span = match jet_foundation::JSON::json_get(edit, "span").unwrap() {
        jet_foundation::JSON::JSONValue::Object(span) => span,
        _ => panic!("fix span is not an object"),
    };
    let start = jet_foundation::JSON::json_int(
        jet_foundation::JSON::json_get(span, "start").unwrap(),
    )
    .unwrap() as usize;
    let end = jet_foundation::JSON::json_int(
        jet_foundation::JSON::json_get(span, "end").unwrap(),
    )
    .unwrap() as usize;
    let new_text = jet_foundation::JSON::json_str(
        jet_foundation::JSON::json_get(edit, "new_text").unwrap(),
    )
    .unwrap();
    let mut fixed = fs::read_to_string(edit_file).unwrap();
    fixed.replace_range(start..end, new_text);
    fs::write(edit_file, &fixed).unwrap();
    assert_eq!(fs::read_to_string(&fix).unwrap(), fixed);
    assert!(!fixed.contains("println"));

    let left = dir.join("left/bad.jet");
    let right = dir.join("right/bad.jet");
    fs::create_dir_all(left.parent().unwrap()).unwrap();
    fs::create_dir_all(right.parent().unwrap()).unwrap();
    fs::write(&left, "fn run( {\n").unwrap();
    fs::write(&right, "fn run( {\n").unwrap();
    let output = Command::new(jet())
        .args(["fmt", left.to_str().unwrap(), right.to_str().unwrap(), "--json"])
        .current_dir(&runner)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stderr));
    let reports = String::from_utf8(output.stderr).unwrap();
    let files = reports
        .lines()
        .map(|line| report_file(&parse_json(line).unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(files, vec![left.display().to_string(), right.display().to_string()]);
}

#[test]
fn e0102_and_e0111_typed_fix_edits_apply() {
    let dir = isolated_cwd("typed_fix_edits");
    let typo = dir.join("typo.jet");
    fs::write(&typo, "fn run() {\n    pirnt(\"hi\");\n}\n").unwrap();
    let immutable = dir.join("immutable.jet");
    fs::write(
        &immutable,
        "fn run() {\n    x :: 1\n    x = 2;\n    print(x);\n}\n",
    )
    .unwrap();

    for (file, expected) in [
        (&typo, "fn run() {\n    print(\"hi\");\n}\n"),
        (
            &immutable,
            "fn run() {\n    x := 1\n    x = 2;\n    print(x);\n}\n",
        ),
    ] {
        let output = Command::new(jet())
            .args(["fix", file.to_str().unwrap()])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "jet fix failed for {}: {}",
            file.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(file).unwrap(), expected);
    }
}

#[test]
fn clean_check_json_golden() {
    let dir = isolated_cwd("check_json_clean");
    fs::write(dir.join("clean.jet"), "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .args(["check", "clean.jet", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty(), "clean JSON check wrote stderr: {:?}", out.stderr);
    check_snapshot(
        "check_json_clean.txt",
        &scrub(&String::from_utf8_lossy(&out.stdout), &dir.join("clean.jet")),
    );
}

#[test]
fn build_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("build")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("build_json.txt", &stderr);
}

#[test]
fn test_json_golden() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("test")
        .arg(&p)
        .arg("--json")
        .output()
        .unwrap();
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("test_json.txt", &stderr);
}

#[test]
fn ci_output_is_ansi_free_when_piped() {
    let p = bad_file(&line!().to_string());
    // Default (piped, not a TTY): must be plain.
    let piped = Command::new(jet()).arg("check").arg(&p).output().unwrap();
    let s = String::from_utf8_lossy(&piped.stderr);
    assert!(
        !s.contains('\x1b'),
        "piped output must be ANSI-free:\n{}",
        s
    );

    // NO_COLOR explicitly set: also plain.
    let no_color = Command::new(jet())
        .arg("check")
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let nc = String::from_utf8_lossy(&no_color.stderr);
    assert!(!nc.contains('\x1b'), "NO_COLOR output must be ANSI-free");

    // And the two must be byte-identical (determinism).
    assert_eq!(s, nc, "piped and NO_COLOR output must match exactly");
}

#[test]
fn color_always_adds_ansi_but_flag_wins_over_no_color() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .arg("check")
        .arg(&p)
        .arg("--color=always")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(
        s.contains('\x1b'),
        "--color=always must win over NO_COLOR and emit ANSI"
    );
}

#[test]
fn every_registered_code_has_an_explain_entry() {
    let codes: Vec<String> = jet_foundation::Registry::diagnostic_rows()
        .iter()
        .map(|row| row.code.to_string())
        .collect();
    assert!(
        codes.len() > 150,
        "expected the full code registry, found {}",
        codes.len()
    );

    let index = jet::Explain::index();
    for code in &codes {
        assert!(
            index.contains_key(code),
            "code {} is registered in typed diagnostic rows but has no explain entry",
            code
        );
        // And `jet explain <code>` must succeed at the CLI for every code.
        let out = Command::new(jet())
            .arg("explain")
            .arg(code)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "`jet explain {}` should succeed",
            code
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(code.as_str()),
            "`jet explain {}` output should name the code",
            code
        );
    }
}

#[test]
fn explain_golden() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E2001")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("explain_E2001.txt", &stdout);
}

#[test]
fn explain_runtime_stop_golden() {
    for code in ["E3010", "E3011", "E3012"] {
        let out = Command::new(jet()).arg("explain").arg(code).output().unwrap();
        assert!(out.status.success(), "jet explain {code} should succeed");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        check_snapshot(&format!("explain_{code}.txt"), &stdout);
    }
}

#[test]
fn explain_golden_e0003() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E0003")
        .output()
        .unwrap();
    assert!(out.status.success(), "jet explain E0003 should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("What this means:"), "{stdout}");
    assert!(stdout.contains("Why Jet enforces it:"), "{stdout}");
    assert!(stdout.contains("How to fix it:"), "{stdout}");
    assert!(stdout.contains("Example:"), "{stdout}");
    assert!(!stdout.contains("longer explanation will land"), "{stdout}");
    check_snapshot("explain_E0003.txt", &stdout);
}

#[test]
fn explain_e2211_golden() {
    let out = Command::new(jet())
        .arg("explain")
        .arg("E2211")
        .output()
        .unwrap();
    assert!(out.status.success(), "jet explain E2211 should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("This code is retired"), "{stdout}");
    check_snapshot("explain_E2211.txt", &stdout);
}

#[test]
fn default_jet_run_deopts_jit_gap_silently() {
    let dir = isolated_cwd("jit_gap_run");
    let file = dir.join("env.jet");
    fs::write(
        &file,
        "use core.env as env\nfn run() {\n    print(env.current_dir())\n}\n",
    )
    .unwrap();
    let output = Command::new(jet())
        .args(["run", file.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}\nstdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("E2211"), "E2211 retired: {stderr}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).is_empty(),
        "deopted env.current_dir() should print a path"
    );
}

#[test]
fn malformed_advisory_database_is_e2607_snapshot() {
    let dir = isolated_cwd("audit_e2607");
    fs::write(dir.join("package.jet"), "name: \"app\"\nversion: \"0.1.0\"\n").unwrap();
    fs::create_dir(dir.join(".jet")).unwrap();
    fs::write(dir.join(".jet/lock"), "version = 1\n").unwrap();
    let advisory_db = dir.join("advisories.txt");
    fs::write(&advisory_db, "missing|fields|only\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "audit", "--advisory-db"])
        .arg(&advisory_db)
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    check_snapshot(
        "audit_malformed_e2607.txt",
        &String::from_utf8(output.stderr).unwrap(),
    );
}

#[test]
fn jetpack_missing_build_log_golden() {
    let cwd = isolated_cwd(&line!().to_string());
    let root = cwd.join("jetpack-root");
    let out = Command::new(jet())
        .args(["inspect", "logs", "definitely_missing", "--no-color"])
        .current_dir(&cwd)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing log is usage-class error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("e1274_missing_build_log.txt", &stderr);
}

#[test]
fn no_args_repl_banner_golden() {
    let out = Command::new(jet()).env("NO_COLOR", "1").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("no_args_repl_banner.txt", &stdout);
}

#[test]
fn question_mark_is_help_golden() {
    let out = Command::new(jet())
        .arg("?")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ?` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("question_mark_help.txt", &stdout);
}

/// D-FE-HELP1=D: `jet ? <query>` (piped, i.e. non-TTY) is the non-interactive
/// floor — best matches for the query, printed once, no raw mode.
#[test]
fn question_mark_query_prints_matches_non_interactively() {
    let out = Command::new(jet())
        .args(["?", "run"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet ? run` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("jet run"), "expected a `run` match, got:\n{}", stdout);
}

#[test]
fn question_mark_language_symbol_uses_shared_semantic_index() {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["?", "List.filter"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run jet ? List.filter");
    assert!(output.status.success(), "status: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("List.filter(f: fn(T) => Bool) => List<T>"), "signature missing: {stdout}");
    assert!(stdout.contains("Keeps items where f(item) is true."), "summary missing: {stdout}");
    assert!(stdout.contains("Example:"), "example missing: {stdout}");
    assert!(stdout.contains("core.collections"), "provenance missing: {stdout}");
}
