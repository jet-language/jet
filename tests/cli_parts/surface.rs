use super::*;
use jet_foundation::JSON::{json_get, json_int, json_str, JSONValue};
use std::collections::{BTreeMap, BTreeSet};

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
fn top_level_help_is_registry_inventory_and_env_help_lists_live_actions() {
    let has_route = |text: &str, route: &str| {
        text.lines().any(|line| {
            let line = line.trim_start();
            line == format!("jet {route}") || line.starts_with(&format!("jet {route} "))
        })
    };

    for flag in ["--help", "help"] {
        let out = Command::new(jet()).arg(flag).env("NO_COLOR", "1").output().unwrap();
        assert!(out.status.success(), "{flag}: {:?}", out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        for command in jet::CLI::COMMANDS.iter().filter(|command| jet::CLI::is_canonical_top_level(command.name)) {
            assert!(has_route(&stdout, command.name), "help omitted {}: {stdout}", command.name);
        }
        for retired in jet::CLI::RETIRED_COMMANDS {
            assert!(!has_route(&stdout, retired.spelling), "help advertised retired {}: {stdout}", retired.spelling);
        }
    }

    let env_help = Command::new(jet()).args(["env", "--help"]).output().unwrap();
    assert!(env_help.status.success(), "env help failed: {:?}", env_help);
    let env_help = String::from_utf8_lossy(&env_help.stdout);
    for action in ["hook", "test", "sync", "info"] {
        assert!(env_help.contains(&format!("jet env {action}")), "env help omitted {action}: {env_help}");
    }

    let completions = [
        Command::new(jet()).args(["self", "completions", "bash"]).output().unwrap(),
        Command::new(jet()).args(["self", "completions", "zsh"]).output().unwrap(),
        Command::new(jet()).args(["self", "completions", "fish"]).output().unwrap(),
        Command::new(jet()).args(["self", "completions", "powershell"]).output().unwrap(),
    ];
    for output in completions {
        assert!(output.status.success(), "completion failed: {:?}", output);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("env"), "completion omitted env: {text}");
    }

    let man = jet::CLI::man_page("0.0.0");
    for retired in jet::CLI::RETIRED_COMMANDS {
        assert!(!man.lines().any(|line| line.trim() == format!(".B {}", retired.spelling)), "man advertised retired {}: {man}", retired.spelling);
    }
}

#[test]
fn external_completion_preserves_checked_program_commands() {
    let dir = isolated_cwd("shape_cli_program_commands");
    fs::write(dir.join("commands.jet"), r#"#CLI(Standard)
struct Commands {
    #[Doc("shared config"), Short("c"), Env("JET_CONFIG")] config: String{"default"}
    #Doc("start the service") fn serve(self, port: Int{3000}) {}
    #Doc("import one file") fn import(self, file: String) {}
}
fn run(args: Commands) {}
"#).unwrap();
    let build = Command::new(jet()).args(["build", "commands.jet"]).current_dir(&dir).output().unwrap();
    assert!(build.status.success(), "program build failed: {}", String::from_utf8_lossy(&build.stderr));
    let help = Command::new(dir.join("build/commands"))
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(help.status.success(), "program root help failed: {}", String::from_utf8_lossy(&help.stderr));
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--config CONFIG"), "root help omitted shared config: {help}");
    assert_eq!(help.matches("--config").count(), 1, "root help repeated shared config: {help}");
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
            "bash" => ["serve", "import", "--port", "file --file", "--config"],
            "zsh" => ["serve", "import", "--port", ":file:value for --file", "--config"],
            "fish" => ["serve", "import", "-l port", "-l file", "-l config"],
            "powershell" => ["serve", "import", "'--port'", "'file','--file'", "'--config'"],
            _ => unreachable!(),
        };
        for fragment in expected {
            assert!(script.contains(fragment), "{shell} external completion omitted {fragment}: {script}");
        }
        assert!(script.contains("--verbose") || script.contains("-l verbose") || script.contains("'--verbose'"), "{shell} omitted the Standard root flags: {script}");
        assert!(script.contains("--quiet") || script.contains("-l quiet") || script.contains("'--quiet'"), "{shell} omitted the Standard root flags: {script}");
        assert!(script.contains("--color") || script.contains("-l color") || script.contains("'--color'"), "{shell} omitted the Standard root flags: {script}");
        check_snapshot(&format!("shape_cli_program_{shell}.txt"), &script);
    }
    let dossier = Command::new(jet())
        .args(["inspect", "dossier", "commands.jet", "run", "--json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(dossier.status.success());
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(dossier.contains("\"completion_words\":[\"--help\",\"--verbose\",\"-v\",\"--quiet\",\"-q\",\"--color\",\"--version\",\"--config\",\"-c\",\"serve\",\"import\"]"), "dossier flattened program flags: {dossier}");
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
        "--config",
        "JET_CONFIG",
        "shared config",
        "command import",
        "--file: String (required) — value for --file",
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
struct Commands {
    #Doc("Start the service") fn serve(self) {}
    #Doc("Import one data file") fn import(self) {}
}
fn run(args: Commands) {}
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
fn derived_program_help_uses_program_basename_for_compiled_and_jet_run_paths() {
    let dir = isolated_cwd("shape_cli_program_help_program_name");
    fs::write(
        dir.join("commands.jet"),
        "#CLI\nstruct Commands { fn serve(self) {} }\nfn run(args: Commands) {}\n",
    )
    .unwrap();

    let build = Command::new(jet())
        .args(["build", "commands.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "program build failed: {}",
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
    check_snapshot("shape_cli_program_help_program_names.txt", &program_names);
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
        vec![
            "inspect audit",
            "hangar import",
            "env test",
            "gc report",
            "perf run",
            "perf test",
        ]
    );
}

#[test]
fn test_help_exposes_measurement_and_retired_command_teaches_it() {
    let test_help = Command::new(jet()).args(["test", "--help"]).env("NO_COLOR", "1").output().unwrap();
    assert!(test_help.status.success(), "jet test --help failed: {}", String::from_utf8_lossy(&test_help.stderr));
    let test_help = String::from_utf8_lossy(&test_help.stdout);
    assert!(test_help.contains("--filter"), "test help lost shared filter flag: {test_help}");
    assert!(test_help.contains("--measure"), "test help omitted measurement mode: {test_help}");
    assert!(test_help.contains("--show-default"), "test help missing command override escape hatch: {test_help}");
    let retired = Command::new(jet()).args(["bench", "--help"]).env("NO_COLOR", "1").output().unwrap();
    assert!(!retired.status.success(), "retired measurement command unexpectedly succeeded");
    let retired = String::from_utf8_lossy(&retired.stderr);
    assert!(retired.contains("jet test --measure"), "retired command did not teach measurement: {retired}");
}

/// I8: this test owns exactly ONE fact — that every user-visible surface names
/// the coverage levels the instrumentation actually emits (function and branch
/// rows, per `jet_test_coverage_reports_branch_taken_and_not_taken_in_text_and_json`).
/// Whole-artifact drift belongs to its canonical owners,
/// `completions_generate_for_every_shell` and `man_page_golden` in
/// `cli_parts/commands.rs`; re-asserting those goldens here made an unrelated
/// command or flag addition look like a coverage-help regression.
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
        let coverage_flag = if shell == "fish" {
            "-l coverage"
        } else {
            "--coverage"
        };
        assert!(script.contains(coverage_flag), "{shell} omitted {coverage_flag}");
        if matches!(shell, "fish" | "zsh") {
            assert!(script.contains(COVERAGE_HELP), "{shell}: {script}");
        }
        assert!(!script.contains("function and line coverage"), "{shell}: {script}");
    }

    let man = Command::new(jet()).args(["self", "man"]).output().unwrap();
    assert!(man.status.success(), "man page failed: {}", String::from_utf8_lossy(&man.stderr));
    let man = String::from_utf8_lossy(&man.stdout);
    assert!(man.contains(COVERAGE_HELP), "{man}");
    assert!(!man.contains("function and line coverage"), "{man}");
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
            let Some(owner) = jet::CLI::moved_command_group(action.name) else {
                continue;
            };
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
fn name_suggestion_json_golden() {
    let dir = isolated_cwd("name_suggestion_json");
    let p = dir.join("name.jet");
    fs::write(
        &p,
        "fn run() {\n    score :: 90\n    print(scor)\n}\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .args(["check", p.to_str().unwrap(), "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let stderr = scrub(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("name_suggestion_json.txt", &stderr);
}

#[test]
fn machine_report_paths_stay_resolvable_across_repository_layouts() {
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
    assert_eq!(
        report_file(&report(Path::new("../outside/bad.jet"))),
        outside.display().to_string()
    );

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
    let fix_report = report(Path::new("../fix/bad.jet"));
    let fix_file = report_file(&fix_report);
    assert_eq!(
        jet_foundation::JSON::json_str(
            jet_foundation::JSON::json_get(&fix_report, "applicability").unwrap(),
        )
        .unwrap(),
        "safe"
    );
    let edits = match jet_foundation::JSON::json_get(&fix_report, "fix_edits").unwrap() {
        jet_foundation::JSON::JSONValue::Array(edits) => edits,
        _ => panic!("fix_edits is not an array"),
    };
    assert_eq!(edits.len(), 1);
    let edit = match &edits[0] {
        jet_foundation::JSON::JSONValue::Object(_) => &edits[0],
        _ => panic!("fix edit is not an object"),
    };
    let edit_file = jet_foundation::JSON::json_str(
        jet_foundation::JSON::json_get(&edits[0], "file").unwrap(),
    )
    .unwrap();
    assert_eq!(fix_file, edit_file);
    let span = match jet_foundation::JSON::json_get(edit, "span").unwrap() {
        span @ jet_foundation::JSON::JSONValue::Object(_) => span,
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
    assert_eq!(
        jet_foundation::JSON::json_str(
            jet_foundation::JSON::json_get(edit, "safety").unwrap(),
        )
        .unwrap(),
        "behavior-preserving"
    );
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
        .args(["fmt", "../left/bad.jet", "../right/bad.jet", "--json"])
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
fn name_suggestion_json_edit_applies_and_rechecks_clean() {
    let dir = isolated_cwd("name_suggestion_fix_edit");
    let file = dir.join("name.jet");
    fs::write(
        &file,
        "fn run() {\n    score :: 90\n    print(scor)\n}\n",
    )
    .unwrap();

    let check = || {
        Command::new(jet())
            .args(["check", file.to_str().unwrap(), "--json"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let output = check();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let report = parse_json(String::from_utf8(output.stderr).unwrap().trim()).unwrap();
    assert_eq!(
        jet_foundation::JSON::json_str(jet_foundation::JSON::json_get(&report, "code").unwrap())
            .unwrap(),
        "E0107"
    );
    let edits = match jet_foundation::JSON::json_get(&report, "fix_edits").unwrap() {
        jet_foundation::JSON::JSONValue::Array(edits) => edits,
        _ => panic!("fix_edits is not an array"),
    };
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    let edit_file = jet_foundation::JSON::json_str(
        jet_foundation::JSON::json_get(edit, "file").unwrap(),
    )
    .unwrap();
    assert_eq!(edit_file, file.to_str().unwrap());
    let span = jet_foundation::JSON::json_get(edit, "span").unwrap();
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
    let source = fs::read_to_string(&file).unwrap();
    assert_eq!(&source[start..end], "scor");
    assert_eq!(new_text, "score");

    let applied = Command::new(jet())
        .args(["fix", file.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(applied.status.code(), Some(0), "{}", String::from_utf8_lossy(&applied.stderr));
    let mut expected = source;
    expected.replace_range(start..end, new_text);
    assert_eq!(fs::read_to_string(&file).unwrap(), expected);

    let fixed = check();
    assert_eq!(fixed.status.code(), Some(0), "{}", String::from_utf8_lossy(&fixed.stderr));
    assert!(fixed.stdout.is_empty());
    assert!(fixed.stderr.is_empty());
}

const AGENT_FIX_LOOP_MAX_ROUNDS: usize = 4;
const AGENT_FIX_LOOP_FIXTURES: &[(&str, &str)] = &[
    (
        "run.jet",
        r#"fn run() {
    pirnt("main typo")
    score :: 90
    print(scor)
    println("main old")
}
"#,
    ),
    (
        "helper.jet",
        r#"fn run() {
    pirnt("helper typo")
    score :: 90
    print(scor)
    println("helper old")
}
"#,
    ),
];

#[derive(Debug, Clone)]
struct RepairEdit {
    code: String,
    file: PathBuf,
    start: usize,
    end: usize,
    new_text: String,
}

#[derive(Debug, Clone)]
struct RepairReport {
    code: String,
    edits: Vec<RepairEdit>,
}

type RepairState = BTreeMap<PathBuf, String>;

fn repair_json_field<'a>(value: &'a JSONValue, key: &str) -> &'a JSONValue {
    json_get(value, key).unwrap_or_else(|| panic!("machine report missing {key}"))
}

fn repair_json_string(value: &JSONValue, key: &str) -> String {
    json_str(repair_json_field(value, key))
        .unwrap_or_else(|| panic!("machine report {key} is not a string"))
        .to_string()
}

fn repair_json_usize(value: &JSONValue, key: &str) -> usize {
    usize::try_from(
        json_int(repair_json_field(value, key))
            .unwrap_or_else(|| panic!("machine report {key} is not an integer")),
    )
    .unwrap_or_else(|_| panic!("machine report {key} is negative"))
}

fn parse_repair_report(value: &JSONValue) -> RepairReport {
    assert_eq!(repair_json_string(value, "schema"), "jet.report/v1");
    let code = repair_json_string(value, "code");
    let edits = match repair_json_field(value, "fix_edits") {
        JSONValue::Array(edits) => edits
            .iter()
            .map(|edit| {
                let span = repair_json_field(edit, "span");
                RepairEdit {
                    code: code.clone(),
                    file: PathBuf::from(repair_json_string(edit, "file")),
                    start: repair_json_usize(span, "start"),
                    end: repair_json_usize(span, "end"),
                    new_text: repair_json_string(edit, "new_text"),
                }
            })
            .collect(),
        _ => panic!("machine report fix_edits is not an array"),
    };
    RepairReport { code, edits }
}

// Read only published jet.report/v1 fields. Human What/Why/Fix text never
// participates in repair loop.
fn check_repair_state(dir: &Path, state: &RepairState) -> Vec<RepairReport> {
    for (file, source) in state {
        fs::write(file, source).unwrap();
    }
    let mut reports = Vec::new();
    for file in state.keys() {
        let output = Command::new(jet())
            .args(["check", file.to_str().unwrap(), "--json"])
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();

        if output.status.success() {
            assert!(output.stderr.is_empty(), "clean check wrote stderr: {:?}", output.stderr);
            let clean = parse_json(String::from_utf8(output.stdout).unwrap().trim())
                .expect("clean check must emit one JSON object");
            assert_eq!(repair_json_string(&clean, "schema"), "jet.report/v1");
            assert_eq!(repair_json_string(&clean, "status"), "ok");
            assert!(matches!(
                json_get(&clean, "ok"),
                Some(JSONValue::Bool(true))
            ));
            match repair_json_field(&clean, "diagnostics") {
                JSONValue::Array(diagnostics) => assert!(diagnostics.is_empty()),
                _ => panic!("clean report diagnostics is not an array"),
            }
            continue;
        }

        assert_eq!(output.status.code(), Some(1), "check failed: {:?}", output);
        assert!(output.stdout.is_empty(), "error check wrote stdout: {:?}", output.stdout);
        reports.extend(
            String::from_utf8(output.stderr)
                .unwrap()
                .lines()
                .map(|line| parse_json(line).expect("error check must emit JSON lines"))
                .map(|report| parse_repair_report(&report)),
        );
    }
    reports
}

fn apply_repair_edits(state: &RepairState, reports: &[RepairReport]) -> Result<RepairState, String> {
    let mut by_file: BTreeMap<PathBuf, Vec<RepairEdit>> = BTreeMap::new();
    for report in reports {
        if report.edits.is_empty() {
            return Err(format!("report {} has no fix_edits", report.code));
        }
        for edit in &report.edits {
            by_file
                .entry(edit.file.clone())
                .or_default()
                .push(edit.clone());
        }
    }

    let mut next = state.clone();
    for (file, mut edits) in by_file {
        let Some(source) = state.get(&file) else {
            let code = edits.first().map_or("unknown", |edit| edit.code.as_str());
            return Err(format!("fix_edit for {code} names untracked file {}", file.display()));
        };
        edits.sort_by(|left, right| {
            right
                .start
                .cmp(&left.start)
                .then(right.end.cmp(&left.end))
        });
        for pair in edits.windows(2) {
            if pair[0].start < pair[1].end {
                return Err(format!(
                    "overlapping fix_edits for {} and {} in {}",
                    pair[0].code,
                    pair[1].code,
                    file.display()
                ));
            }
        }

        let mut fixed = source.clone();
        for edit in edits {
            if edit.start > edit.end || fixed.get(edit.start..edit.end).is_none() {
                return Err(format!(
                    "invalid span {}..{} for fix_edit {} in {}",
                    edit.start,
                    edit.end,
                    edit.code,
                    file.display()
                ));
            }
            fixed.replace_range(edit.start..edit.end, &edit.new_text);
        }
        next.insert(file, fixed);
    }
    Ok(next)
}

fn repair_codes(reports: &[RepairReport]) -> BTreeSet<String> {
    reports.iter().map(|report| report.code.clone()).collect()
}

fn run_repair_loop<F>(
    initial: RepairState,
    max_rounds: usize,
    mut check: F,
) -> Result<(RepairState, usize), String>
where
    F: FnMut(&RepairState) -> Vec<RepairReport>,
{
    let mut state = initial;
    let mut seen = Vec::new();
    let mut previous_codes = None;

    for round in 0..=max_rounds {
        let reports = check(&state);
        if reports.is_empty() {
            return Ok((state, round));
        }
        let codes = repair_codes(&reports);
        if seen.iter().any(|old| old == &state) {
            return Err(format!(
                "repeated file state; offending code(s): {}",
                codes.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if let Some(previous_codes) = &previous_codes {
            let introduced: Vec<_> = codes.difference(previous_codes).cloned().collect();
            if !introduced.is_empty() {
                return Err(format!(
                    "fix_edit regression introduced report code(s): {}",
                    introduced.join(", ")
                ));
            }
        }
        if round == max_rounds {
            return Err(format!(
                "round bound {max_rounds} exceeded; offending code(s): {}",
                codes.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        seen.push(state.clone());
        state = apply_repair_edits(&state, &reports)?;
        previous_codes = Some(codes);
    }
    unreachable!()
}

#[test]
fn agent_fix_loop_corpus_converges_without_report_regression() {
    let dir = isolated_cwd("agent_fix_loop");
    let mut initial = RepairState::new();
    for (name, source) in AGENT_FIX_LOOP_FIXTURES {
        let file = dir.join(name);
        fs::write(&file, source).unwrap();
        initial.insert(file, (*source).to_string());
    }
    let run_file = dir.join("run.jet");
    let helper_file = dir.join("helper.jet");
    let mut check = |state: &RepairState| check_repair_state(&dir, state);

    let initial_reports = check(&initial);
    assert!(
        initial_reports.iter().all(|report| !report.edits.is_empty()),
        "every fixture report must carry fix_edits: {initial_reports:?}"
    );
    let mut edits_per_file = BTreeMap::new();
    for edit in initial_reports.iter().flat_map(|report| &report.edits) {
        *edits_per_file.entry(edit.file.clone()).or_insert(0usize) += 1;
    }
    assert_eq!(
        edits_per_file.keys().cloned().collect::<BTreeSet<_>>(),
        initial.keys().cloned().collect::<BTreeSet<_>>(),
        "one repair-loop round must report edits for every fixture file"
    );
    assert!(edits_per_file[&run_file] >= 3, "run.jet needs several errors");
    assert!(
        edits_per_file[&helper_file] >= 3,
        "helper.jet needs several errors"
    );

    let bound_error = run_repair_loop(initial.clone(), 0, &mut check).unwrap_err();
    assert!(
        bound_error.contains("round bound 0") && bound_error.contains("E0102"),
        "bound failure must name offending code: {bound_error}"
    );

    let (final_state, rounds) =
        run_repair_loop(initial, AGENT_FIX_LOOP_MAX_ROUNDS, &mut check).unwrap();
    assert!(rounds <= AGENT_FIX_LOOP_MAX_ROUNDS);
    assert!(check(&final_state).is_empty(), "final recheck was not clean");
    assert_eq!(
        final_state[&run_file],
        r#"fn run() {
    print("main typo")
    score :: 90
    print(score)
    print("main old")
}
"#
    );
    assert_eq!(
        final_state[&helper_file],
        r#"fn run() {
    print("helper typo")
    score :: 90
    print(score)
    print("helper old")
}
"#
    );
}

#[test]
fn agent_fix_loop_repeat_guard_names_offending_code() {
    let file = PathBuf::from("repeat.jet");
    let state = BTreeMap::from([(file.clone(), "x".to_string())]);
    let forward = RepairReport {
        code: "E0102".to_string(),
        edits: vec![RepairEdit {
            code: "E0102".to_string(),
            file: file.clone(),
            start: 0,
            end: 1,
            new_text: "y".to_string(),
        }],
    };
    let backward = RepairReport {
        code: "E0102".to_string(),
        edits: vec![RepairEdit {
            code: "E0102".to_string(),
            file: file.clone(),
            start: 0,
            end: 1,
            new_text: "x".to_string(),
        }],
    };
    let error = run_repair_loop(state, 3, |state| {
        if state[&file] == "x" {
            vec![forward.clone()]
        } else {
            vec![backward.clone()]
        }
    })
    .unwrap_err();
    assert!(
        error.contains("repeated file state") && error.contains("E0102"),
        "repeat failure must name offending code: {error}"
    );
}

#[test]
fn agent_fix_loop_regression_guard_names_offending_code() {
    let file = PathBuf::from("regression.jet");
    let state = BTreeMap::from([(file.clone(), "x".to_string())]);
    let first_report = RepairReport {
        code: "E0102".to_string(),
        edits: vec![RepairEdit {
            code: "E0102".to_string(),
            file: file.clone(),
            start: 0,
            end: 1,
            new_text: "y".to_string(),
        }],
    };
    let introduced_report = RepairReport {
        code: "E0037".to_string(),
        edits: vec![RepairEdit {
            code: "E0037".to_string(),
            file,
            start: 0,
            end: 1,
            new_text: "z".to_string(),
        }],
    };
    let mut checks = 0;
    let error = run_repair_loop(state, 2, |_| {
        checks += 1;
        if checks == 1 {
            vec![first_report.clone()]
        } else {
            vec![introduced_report.clone()]
        }
    })
    .unwrap_err();
    assert!(
        error.contains("fix_edit regression") && error.contains("E0037"),
        "regression failure must name offending code: {error}"
    );
}

#[test]
fn fix_safety_tiers_are_reported_and_applied() {
    let dir = isolated_cwd("typed_fix_edits");
    let typo = dir.join("typo.jet");
    fs::write(&typo, "fn run() {\n    pirnt(\"hi\");\n}\n").unwrap();
    let formatting = dir.join("formatting.jet");
    fs::write(
        &formatting,
        "fn run() {\n    loop item; [1, 2, 3] { print(item) }\n}\n",
    )
    .unwrap();
    let immutable = dir.join("immutable.jet");
    let immutable_source = "fn run() {\n    x :: 1\n    x = 2;\n    print(x);\n}\n";
    fs::write(&immutable, immutable_source).unwrap();

    let report_grades = |file: &Path| {
        let output = Command::new(jet())
            .args(["check", file.to_str().unwrap(), "--json"])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
        let report = parse_json(String::from_utf8(output.stderr).unwrap().trim()).unwrap();
        let applicability = jet_foundation::JSON::json_str(
            jet_foundation::JSON::json_get(&report, "applicability").unwrap(),
        )
        .unwrap()
        .to_string();
        let edits = match jet_foundation::JSON::json_get(&report, "fix_edits").unwrap() {
            jet_foundation::JSON::JSONValue::Array(edits) => edits,
            _ => panic!("fix_edits is not an array"),
        };
        assert_eq!(edits.len(), 1);
        let safety = jet_foundation::JSON::json_str(
            jet_foundation::JSON::json_get(&edits[0], "safety").unwrap(),
        )
        .unwrap()
        .to_string();
        (applicability, safety)
    };

    assert_eq!(
        report_grades(&formatting),
        ("safe".to_string(), "formatting".to_string())
    );
    assert_eq!(
        report_grades(&immutable),
        ("suggested".to_string(), "api-changing".to_string())
    );

    let output = Command::new(jet())
        .args(["fix", typo.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "jet fix failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&typo).unwrap(),
        "fn run() {\n    print(\"hi\");\n}\n"
    );

    let output = Command::new(jet())
        .args(["fix", formatting.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(fs::read_to_string(&formatting).unwrap().contains("loop item in"));

    let output = Command::new(jet())
        .args(["fix", immutable.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&immutable).unwrap(), immutable_source);
    assert!(String::from_utf8_lossy(&output.stdout).contains("skipped 1 suggestion"));

    let output = Command::new(jet())
        .args(["fix", immutable.to_str().unwrap(), "--all"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(fs::read_to_string(&immutable).unwrap().contains("x := 1"));
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
        .arg("--show-default")
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
fn explain_build_fact_golden() {
    let dir = isolated_cwd("explain_build_fact");
    let file = dir.join("fact.jet");
    fs::write(&file, "fn run() {}\n").unwrap();
    let out = Command::new(jet())
        .args(["explain", "Build.Package.Name", file.to_str().unwrap()])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "build fact explain failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("explain_build_fact.txt", &stdout);
}

#[test]
fn explain_typed_build_setting_golden() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/packages/typed_settings");
    let out = Command::new(jet())
        .args([
            "explain",
            "build.settings.tls",
            "run.jet",
            "--profile=release",
            "--set",
            "tls=true",
        ])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "typed setting explain failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    check_snapshot("explain_build_setting.txt", &stdout);
}

#[test]
fn explain_runtime_stop_golden() {
    for code in [
        "E3001", "E3002", "E3003", "E3004", "E3005", "E3010", "E3011", "E3012",
    ] {
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
        "use core.sys as env\nfn run() {\n    print(env.current_dir())\n}\n",
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
fn audit_missing_advisory_database_fails_closed() {
    let dir = isolated_cwd("audit_missing_e2611");
    fs::write(dir.join("package.jet"), "name: \"app\"\nversion: \"0.1.0\"\n").unwrap();
    fs::create_dir(dir.join(".jet")).unwrap();
    fs::write(dir.join(".jet/lock"), "version = 1\n").unwrap();

    let output = Command::new(jet())
        .args(["inspect", "audit"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    check_snapshot(
        "audit_missing_e2611.txt",
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
fn jetpack_missing_package_explain_golden() {
    let cwd = isolated_cwd(&line!().to_string());
    let root = cwd.join("jetpack-root");
    let out = Command::new(jet())
        .args(["explain", "definitely_missing", "--no-color"])
        .current_dir(&cwd)
        .env("JETPACK_ROOT", &root)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "missing package is an explain error");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("e1274_missing_package_explain.txt", &stderr);
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
        .expect("run jet ! List.filter");
    assert!(output.status.success(), "status: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("List.filter(f: fn(T) Bool) -> List<T>"), "signature missing: {stdout}");
    assert!(stdout.contains("Keeps items where f(item) is true."), "summary missing: {stdout}");
    assert!(stdout.contains("Example:"), "example missing: {stdout}");
    assert!(stdout.contains("core.collections"), "provenance missing: {stdout}");
}

// ── #2072: the jet binary's own per-command help ─────────────────────────

/// `jet help <cmd>` prints the per-command screen, not the ~200-line global
/// inventory. The golden pins summary + usage + flag rows.
#[test]
fn help_run_is_per_command_not_the_global_screen() {
    let out = Command::new(jet())
        .args(["help", "run"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "`jet help run` should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !stdout.contains("Welcome to"),
        "`jet help run` must not dump the global usage screen:\n{stdout}"
    );
    check_snapshot("help_run.txt", &stdout);
}

/// One renderer, two spellings: `jet help <cmd>` and `jet <cmd> --help` can
/// never drift because both call `command_help`.
#[test]
fn help_command_and_command_help_flag_render_identically() {
    for command in ["run", "build", "test", "check", "repl", "eval", "fuzz"] {
        let via_help = Command::new(jet())
            .args(["help", command])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let via_flag = Command::new(jet())
            .args([command, "--help"])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&via_help.stdout),
            String::from_utf8_lossy(&via_flag.stdout),
            "`jet help {command}` and `jet {command} --help` disagree"
        );
    }
}

/// A target-requiring verb invoked bare prints its OWN usage on stderr with
/// exit 2 — not the whole command inventory.
#[test]
fn bare_target_requiring_verbs_print_targeted_usage() {
    let out = Command::new(jet()).arg("fix").env("NO_COLOR", "1").output().unwrap();
    assert_eq!(out.status.code(), Some(2), "bare `jet fix` should exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    check_snapshot("fix_bare_usage.txt", &stderr);

    for verb in ["fix", "lint", "fuzz"] {
        let out = Command::new(jet()).arg(verb).env("NO_COLOR", "1").output().unwrap();
        assert_eq!(out.status.code(), Some(2), "bare `jet {verb}` should exit 2");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("Welcome to"),
            "bare `jet {verb}` must not dump the global screen:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("jet {verb}")),
            "bare `jet {verb}` should show its own usage line:\n{stderr}"
        );
    }
}

/// The registry carries a real positional shape for the argument-taking
/// verbs, so help stops saying the meaningless `jet <cmd> [args]`.
#[test]
fn per_command_help_shows_real_argument_shapes() {
    for (command, shape) in [
        ("repl", "jet repl [<file.jet>]"),
        ("eval", "jet eval <file.jet|expression>"),
        ("lint", "jet lint --a11y <file.jet>"),
        ("fuzz", "jet fuzz <file.jet> [<test>]"),
    ] {
        let out = Command::new(jet())
            .args([command, "--help"])
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(shape),
            "`jet {command} --help` should show `{shape}`, got:\n{stdout}"
        );
        assert!(
            !stdout.contains(&format!("jet {command} [args]")),
            "`jet {command} --help` still shows the placeholder shape:\n{stdout}"
        );
    }
}

// ── #2068: `jet eval` expression form and print output ───────────────────

/// The help contract ("Evaluate pure Jet and print JSON") is honored: an
/// expression evaluates instead of dying as `E2105 couldn't read '1 + 2'`.
#[test]
fn eval_accepts_an_expression_argument() {
    let json = Command::new(jet())
        .args(["eval", "1 + 2", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&json.stderr);
    assert_eq!(json.status.code(), Some(0), "stderr: {stderr}");
    assert!(!stderr.contains("E2105"), "expression was read as a path: {stderr}");
    assert_eq!(String::from_utf8_lossy(&json.stdout).trim(), "3");

    let pretty = Command::new(jet())
        .args(["eval", "1 + 2"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(pretty.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&pretty.stdout).trim(), "3");
}

/// A `.jet` name that isn't there stays a file-not-found error. A typo'd
/// filename must never be silently re-read as an expression.
#[test]
fn eval_missing_jet_file_still_reports_not_found() {
    let out = Command::new(jet())
        .args(["eval", "no_such_program.jet"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2105"), "expected a read failure, got: {stderr}");
}

/// `jet eval <file>` no longer swallows what the program printed. Human mode
/// shows the output then the value; `--json` keeps stdout to exactly one JSON
/// document and moves the program's own output to stderr.
#[test]
fn eval_file_forwards_print_output_alongside_the_value() {
    let dir = isolated_cwd("eval_print_output");
    let file = dir.join("printer.jet");
    fs::write(&file, "fn run() {\n    print(\"hi\");\n}\n").unwrap();

    let human = Command::new(jet())
        .args(["eval", &file.to_string_lossy()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert_eq!(human.status.code(), Some(0), "{}", String::from_utf8_lossy(&human.stderr));
    assert!(stdout.contains("hi"), "print output was swallowed: {stdout:?}");
    assert!(stdout.contains("()"), "the run value should still render: {stdout:?}");

    let json = Command::new(jet())
        .args(["eval", &file.to_string_lossy(), "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0));
    assert!(
        !String::from_utf8_lossy(&json.stdout).contains("hi"),
        "--json stdout must stay one JSON document: {:?}",
        String::from_utf8_lossy(&json.stdout)
    );
    assert!(
        String::from_utf8_lossy(&json.stderr).contains("hi"),
        "--json should forward program output to stderr: {:?}",
        String::from_utf8_lossy(&json.stderr)
    );
}
