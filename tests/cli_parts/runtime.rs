use super::*;
use jet_foundation::JSON::JSONValue;

/// `jet fix --edition=2027` rewrites `json.canonical(x)` per D-JSONCANON1:
/// `json.canonical(x)?` when the enclosing function is fallible, otherwise
/// `json.canonical(x) ?? panic("value is not canonical JSON")`. A re-run
/// must be a no-op (idempotent), including when the existing fallback
/// already has a space before `??` (the bug that made `jet fix` double-
/// append the panic fallback on every re-run).
#[test]
fn jet_fix_edition_2027_rewrites_json_canonical_and_is_idempotent() {
    let dir = isolated_cwd("jsoncanon_fix");
    let file = dir.join("run.jet");
    fs::write(
        &file,
        r#"use core.encoding.json as json

fn show() {
    data := json.parse("{\"a\":1}") ?? panic("json")
    r := json.canonical(data)
    print(r)
}

fn sign() -> String ? {
    data := json.parse("{\"a\":1}")?
    r := json.canonical(data)
    return r
}

fn already_migrated() {
    data := json.parse("{\"a\":1}") ?? panic("json")
    r := json.canonical(data) ?? panic("value is not canonical JSON")
    print(r)
}
"#,
    )
    .unwrap();

    let run_fix = |file: &Path| -> String {
        let out = Command::new(jet())
            .args(["fix", file.to_str().unwrap(), "--edition=2027"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "jet fix failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        fs::read_to_string(file).unwrap()
    };

    let after_first = run_fix(&file);
    assert!(
        after_first.contains("json.canonical(data) ?? panic(\"value is not canonical JSON\")\n    print(r)\n}\n\nfn sign"),
        "infallible fn `show` must get the panic fallback:\n{after_first}"
    );
    assert!(
        after_first.contains("json.canonical(data)?\n    return r"),
        "fallible fn `sign` must get bare `?` propagation, not a panic fallback:\n{after_first}"
    );
    assert!(
        !after_first.contains("value is not canonical JSON\") ?? panic"),
        "an already-migrated call must not be rewritten twice:\n{after_first}"
    );

    // Re-running the migration must be a byte-identical no-op — this is the
    // idempotency the space-before-`??` bug broke.
    let after_second = run_fix(&file);
    assert_eq!(
        after_first, after_second,
        "`jet fix --edition=2027` must be idempotent on a re-run"
    );
}

/// #1659 criterion 4: `jet perf` and `jet diff`/`jet merge` route every exit
/// through the `jet_foundation::ExitCodes` table, never a raw literal. This
/// guards the two files migrated for #1659; it is not a repo-wide sweep.
#[test]
fn perf_and_structural_merge_never_raw_exit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in ["Source/CmdPerf.rs", "Source/CmdStructuralMerge.rs"] {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {relative}: {e}"));
        for (line_no, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            // A raw numeric literal fed to any of the exit shapes this file
            // uses (`exit(2)`, `Outcome::Exit(2)`, `return 2;`) — the exact
            // shapes ExitCodes replaced in this file for #1659. Named
            // constants (`ExitCodes::USAGE`) pass; unrelated numeric literals
            // elsewhere in the file (never matching these exact shapes) pass.
            let all_digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
            let is_raw_exit_call = trimmed.starts_with("exit(")
                && all_digits(trimmed[5..].trim_end_matches(';').trim_end_matches(')'));
            let is_raw_outcome_exit = trimmed.starts_with("Outcome::Exit(")
                && all_digits(trimmed[14..].trim_end_matches(';').trim_end_matches(')'));
            let is_raw_return = trimmed.strip_prefix("return ").is_some_and(|rest| {
                all_digits(rest.trim_end_matches(';'))
            });
            assert!(
                !is_raw_exit_call && !is_raw_outcome_exit && !is_raw_return,
                "{relative}:{}: raw exit-code literal `{trimmed}` — use jet_foundation::ExitCodes",
                line_no + 1
            );
        }
    }
}

#[test]
fn uncoded_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("Source");
    for entry in fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let compact = source.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
        let mut rest = compact.as_str();
        while let Some(at) = rest.find("eprintln!(") {
            let after_macro = &rest[at + "eprintln!(".len()..];
            let after_raw = after_macro.strip_prefix('r').unwrap_or(after_macro);
            let after_hashes = after_raw.trim_start_matches('#');
            assert!(
                !after_hashes.starts_with("\"error:")
                    && !after_hashes.starts_with("\"Error:"),
                "{} contains a bare eprintln error; use the shared Diagnostic renderer with a registered code",
                path.display()
            );
            rest = after_macro;
        }
    }
}

#[test]
fn generic_cli_diagnostics_match_registered_snapshots() {
    let cases = [
        (
            "E2104",
            "`jet inspect example` needs an entry file",
            include_str!("../fixtures/cli-diagnostics/E2104.stderr"),
        ),
        (
            "E2105",
            "couldn't read `package.jet`: permission denied",
            include_str!("../fixtures/cli-diagnostics/E2105.stderr"),
        ),
    ];
    for (code, what, expected) in cases {
        let (why, fix) = if code == "E2104" {
            (
                "Jet needs valid command input before it can run this command",
                "correct the named argument or input, then run the command again",
            )
        } else {
            (
                "Jet could not complete the named file, tool, or operating-system operation",
                "correct the named problem, then run the command again",
            )
        };
        let diagnostic = jet::Diagnostics::Diagnostic::error(
            code,
            what.to_string(),
            why.to_string(),
            fix.to_string(),
            None,
        );
        assert_eq!(diagnostic.render_colored("", "", false), expected);
    }
}

/// #1659 criterion 5: `jet inspect reserved` lists Jet's real keywords, the
/// five teaching-reserved words, and the sigil surface — both as human text
/// and as `--json`.
#[test]
fn inspect_reserved_lists_keywords_teaching_words_and_sigils() {
    let text = Command::new(jet())
        .args(["inspect", "reserved"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(text.status.success(), "{:?}", text);
    let text = String::from_utf8_lossy(&text.stdout);
    for word in ["copy", "mut", "take", "const", "unsafe"] {
        assert!(text.contains(word), "reserved report missing teaching-reserved word `{word}`: {text}");
    }
    for sigil in ["::", ":=", "&", "^", "~", "@"] {
        assert!(text.contains(sigil), "reserved report missing sigil `{sigil}`: {text}");
    }
    assert!(text.contains("fn"), "reserved report missing a real keyword: {text}");

    let json = Command::new(jet())
        .args(["inspect", "reserved", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(json.status.success(), "{:?}", json);
    let json = String::from_utf8_lossy(&json.stdout);
    assert!(json.contains("\"keywords\":["), "{json}");
    assert!(json.contains("\"teaching_reserved\":["), "{json}");
    assert!(json.contains("\"sigils\":["), "{json}");
    parse_json(&json).unwrap_or_else(|_| panic!("reserved --json must parse: {json}"));
}

/// #1659 criterion 3: `--quiet` is one real spelling wired through the shared
/// `OutputMode`, not a declared-but-unread facade. `jet init` prints a
/// confirmation status line on success; `--quiet` must suppress it while
/// still succeeding (exit 0) and still creating the same files.
#[test]
fn quiet_suppresses_status_output_without_changing_behavior() {
    let loud = std::env::temp_dir().join("jet_cli_quiet_init_loud");
    let quiet = std::env::temp_dir().join("jet_cli_quiet_init_quiet");
    let _ = fs::remove_dir_all(&loud);
    let _ = fs::remove_dir_all(&quiet);
    fs::create_dir_all(&loud).unwrap();
    fs::create_dir_all(&quiet).unwrap();

    let loud_out = Command::new(jet()).arg("init").current_dir(&loud).env("NO_COLOR", "1").output().unwrap();
    assert!(loud_out.status.success(), "{:?}", loud_out);
    assert!(
        !String::from_utf8_lossy(&loud_out.stdout).trim().is_empty(),
        "jet init without --quiet should print a confirmation"
    );

    let quiet_out = Command::new(jet()).args(["init", "--quiet"]).current_dir(&quiet).env("NO_COLOR", "1").output().unwrap();
    assert!(quiet_out.status.success(), "{:?}", quiet_out);
    assert!(
        String::from_utf8_lossy(&quiet_out.stdout).trim().is_empty(),
        "jet init --quiet must suppress its non-error status line, got: {}",
        String::from_utf8_lossy(&quiet_out.stdout)
    );
    assert!(quiet.join(jet::Syntax::PACKAGE_FILE).is_file(), "--quiet must not change what init creates");

    let _ = fs::remove_dir_all(&loud);
    let _ = fs::remove_dir_all(&quiet);
}

/// `--quiet` is declared exactly once in the shared flag table (I7-style
/// one-spelling law for the CLI surface).
#[test]
fn quiet_flag_declared_once_in_the_shared_table() {
    let count = jet::CLI::FLAGS.iter().filter(|f| f.long == "--quiet").count();
    assert_eq!(count, 1, "--quiet must have exactly one spelling in jet::CLI::FLAGS");
}

#[test]
fn dry_run_flag_declared_once_for_rewriters() {
    let count = jet::CLI::FLAGS
        .iter()
        .filter(|flag| flag.long == jet::CLI::DRY_RUN_FLAG)
        .count();
    assert_eq!(count, 1, "--dry-run must have exactly one FlagSpec row");
    let usage = jet::CLI::command_group_usage("inspect");
    assert!(usage.contains("codemod <plan.json> --dry-run"), "{usage}");
    assert!(!usage.contains("codemod dry-run"), "{usage}");
}

/// #1659 criterion 1 (round 2): `--ar`/`--clang`/`--facts`/`--from`/
/// `--no-sign`/`--pkg`/`--registry`/`--to` used to exist only in
/// `NestedCommandSpec::usage` prose, invisible to the flag registry. Real
/// `FlagSpec` rows make `is_known_flag` recognize them, `closest_flag`
/// suggest them on a typo (E2102), and the man page and every shell's
/// completions mention them.
#[test]
fn formerly_prose_only_flags_are_real_flag_rows() {
    let flags = ["--ar", "--clang", "--facts", "--from", "--no-sign", "--pkg", "--registry", "--to"];
    let man = jet::CLI::man_page("0.0.0");
    let bash = jet::CLI::completions_bash();
    let zsh = jet::CLI::completions_zsh();
    let fish = jet::CLI::completions_fish();
    let powershell = jet::CLI::completions_powershell();
    for flag in flags {
        assert!(jet::CLI::is_known_flag(flag), "`{flag}` must be a known flag");
        assert!(man.contains(flag), "man page missing `{flag}`");
        assert!(bash.contains(flag), "bash completions missing `{flag}`");
        assert!(zsh.contains(flag), "zsh completions missing `{flag}`");
        assert!(powershell.contains(flag), "powershell completions missing `{flag}`");
        // fish completions drop the leading `--` (`complete -l <name>`, not `--<name>`).
        assert!(fish.contains(flag.trim_start_matches("--")), "fish completions missing `{flag}`");
    }
    // E2102 "did you mean" now finds the real flag on a one-character typo.
    assert_eq!(jet::CLI::closest_flag("--pk"), Some("--pkg"));
    assert_eq!(jet::CLI::closest_flag("--regsitry"), Some("--registry"));
}

/// #1659 criterion 2: bare `jet --help` and `jet -h` print the full command
/// table (same as `jet help`), not the short orientation greeting and not an
/// E2101/E2102 teaching error.
#[test]
fn top_level_help_flag_prints_full_usage() {
    for flag in ["--help", "-h"] {
        let out = Command::new(jet()).arg(flag).env("NO_COLOR", "1").output().unwrap();
        assert!(out.status.success(), "{flag}: {:?}", out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("usage:"), "{flag} did not print full usage: {stdout}");
        assert!(stdout.contains("build"), "{flag} usage missing a real command: {stdout}");
        assert!(stdout.contains("inspect commands:"), "{flag} usage missing the inspect group: {stdout}");
        assert!(stdout.contains("shared-store"), "{flag} usage missing the live shared-store command: {stdout}");
    }
}

#[test]
fn env_help_lists_shipped_test_and_hook_actions() {
    let out = Command::new(jet())
        .args(["env", "--help"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "env help failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("env test [-- command]"), "env help omitted `env test`: {stdout}");
    assert!(stdout.contains("env hook <bash|zsh|fish>"), "env help omitted `env hook`: {stdout}");
}

#[test]
fn top_level_help_lists_job_vocabulary_only() {
    let out = Command::new(jet())
        .arg("help")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "help failed: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("jobs"), "help must list `jet jobs`: {stdout}");
    assert!(stdout.contains("#Job"), "help must list `#Job`: {stdout}");
    assert!(stdout.contains("<file.jet> --"), "help must show job subcommands: {stdout}");
    assert!(!stdout.contains("tasks"), "retired task CLI vocabulary leaked: {stdout}");
    let retired_flag = format!("--{}", "task");
    assert!(!stdout.contains(&retired_flag), "retired job flag leaked: {stdout}");
}

#[test]
fn command_overrides_report_themselves_and_default_bypasses_them() {
    if !common::have_rustc() {
        return;
    }
    let dir = isolated_cwd("command_override_precedence");
    let file = dir.join("override.jet");
    fs::write(
        &file,
        r#"use core.testing as testing

fn test(suite: TestSuite) {
    print("override")
    suite.run()
}

#Test("stock") {
    print("stock")
}

fn run() {}
"#,
    )
    .unwrap();

    let override_run = Command::new(jet())
        .args(["test", "override.jet", "--serial"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(override_run.status.success(), "override failed: {:?}", override_run);
    let override_stdout = String::from_utf8_lossy(&override_run.stdout);
    assert!(override_stdout.contains("jet test: using fn test override"), "{override_stdout}");
    assert!(override_stdout.contains("override"), "{override_stdout}");
    assert!(override_stdout.contains("stock: pass"), "{override_stdout}");

    let stock_run = Command::new(jet())
        .args(["test", "override.jet", "--show-default", "--serial"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(stock_run.status.success(), "stock failed: {:?}", stock_run);
    let stock_stdout = String::from_utf8_lossy(&stock_run.stdout);
    assert!(!stock_stdout.contains("using fn test override"), "{stock_stdout}");
    assert!(!stock_stdout.contains("override\n"), "{stock_stdout}");
    assert!(stock_stdout.contains("stock"), "{stock_stdout}");
    assert!(stock_stdout.contains("stock: pass"), "{stock_stdout}");

    let _ = fs::remove_dir_all(&dir);
}

/// #1659 criterion 2: `jet <cmd> --help`/`-h` works for a command that used
/// to hit the generic E2102 "unknown flag" — it neither runs the command nor
/// errors, and names the command in its own help text.
#[test]
fn per_command_help_flag_works_without_running_the_command() {
    let dir = isolated_cwd("help_flag_build");
    for (cmd, flag) in [("build", "--help"), ("check", "-h"), ("new", "--help")] {
        let out = Command::new(jet())
            .args([cmd, flag])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "`{cmd} {flag}`: {:?}", out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(cmd), "`{cmd} {flag}` help text missing the command name: {stdout}");
        // A real `jet build`/`jet check` with no file argument would fail
        // (E2xxx missing target); `--help` must short-circuit before that.
        assert!(!stdout.contains("Error ["), "`{cmd} {flag}` ran the command instead of printing help: {stdout}");
    }
    assert!(!dir.join("new").exists(), "`jet new --help` must not create a project");
}

/// #1659 criterion 2 (round 2): `--help`/`-h` also works for the
/// `owns_flag_vocabulary` commands — `jet prove`/`jet budget`/`jet report`
/// used to error-teach E2102 or E2101, and `jet clean`/`jet update`/
/// `jet image`/`jet trust` used to silently EXECUTE the real command instead
/// of printing help. All seven must print help and exit 0 without doing real
/// work.
#[test]
fn owns_flag_vocabulary_help_flag_prints_help_not_execute() {
    let dir = isolated_cwd("help_flag_owns_vocab");
    for (cmd, flag) in [
        ("prove", "--help"),
        ("budget", "-h"),
        ("report", "--help"),
        ("clean", "--help"),
        ("update", "-h"),
        ("image", "--help"),
        ("trust", "--help"),
        ("hangar", "--help"),
    ] {
        assert!(jet::CLI::owns_flag_vocabulary(cmd), "test assumption: `{cmd}` should own a flag vocabulary");
        let out = Command::new(jet())
            .args([cmd, flag])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "`{cmd} {flag}` must print help and exit 0: {:?}", out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(cmd), "`{cmd} {flag}` help text missing the command name: {stdout}");
        assert!(out.stderr.is_empty(), "`{cmd} {flag}` must not print to stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
    // `clean`/`update`/`image`/`trust` would otherwise mutate the package
    // store, dependency pins, or trust grants — none of that happened.
    assert!(!dir.join(".jet").exists(), "`--help` on an owns_flag_vocabulary command must not do real work");
}

/// #1659 criterion 2 (round 2): `jet self devtools --help` (bare, and with a
/// sub-verb like `grammars`) prints help instead of erroring on the unknown
/// `--help` "subcommand" or, worse, actually regenerating the grammar files.
#[test]
fn devtools_help_flag_prints_help_not_execute() {
    let dir = isolated_cwd("help_flag_devtools");
    for args in [vec!["self", "devtools", "--help"], vec!["self", "devtools", "grammars", "--help"]] {
        let out = Command::new(jet()).args(&args).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
        assert!(out.status.success(), "`jet {}`: {:?}", args.join(" "), out);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("unknown"), "`jet {}` hit the unknown-subcommand path: {stderr}", args.join(" "));
    }
    assert!(!dir.join("editors").exists(), "`devtools grammars --help` must not write any files");
}

/// #1659 criterion 2: an exhaustive group's bare `--help`/`-h` (`jet hangar
/// --help`, `jet registry -h`) prints the group's action table instead of
/// the E2101 "isn't a jet hangar command" that `--help` used to trigger.
#[test]
fn exhaustive_group_help_flag_lists_actions_instead_of_e2101() {
    for (group, flag) in [("hangar", "--help"), ("registry", "-h"), ("inspect", "--help")] {
        let out = Command::new(jet()).args([group, flag]).env("NO_COLOR", "1").output().unwrap();
        assert!(out.status.success(), "`{group} {flag}`: {:?}", out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("E2101"), "`{group} {flag}` still hit E2101: {stderr}");
        let expected_action = jet::CLI::command_group(group).unwrap().actions[0].name;
        assert!(stdout.contains(expected_action), "`{group} {flag}` missing action `{expected_action}`: {stdout}");
    }
}

/// #1659 criterion 2: `jet perf` (bare) and `jet perf --help`/`-h` print the
/// group's action table instead of the E2101/E2102 they used to raise.
#[test]
fn perf_bare_and_help_flag_list_actions_instead_of_e2101_e2102() {
    for args in [vec!["perf"], vec!["perf", "--help"], vec!["perf", "-h"], vec!["perf", "help"]] {
        let out = Command::new(jet()).args(&args).env("NO_COLOR", "1").output().unwrap();
        assert!(out.status.success(), "`jet {}`: {:?}", args.join(" "), out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("E2101") && !stderr.contains("E2102"), "`jet {}`: {stderr}", args.join(" "));
        assert!(stdout.contains("view") && stdout.contains("compare"), "`jet {}` missing perf actions: {stdout}", args.join(" "));
    }
}

/// #1659 criterion 3: `jet new --quiet` still creates the project but
/// suppresses its confirmation lines.
#[test]
fn quiet_suppresses_new_confirmation() {
    let dir = isolated_cwd("quiet_new");
    let out = Command::new(jet())
        .args(["new", "quiet_project", "--quiet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "jet new --quiet must suppress its confirmation, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(dir.join("quiet_project").join(jet::Syntax::PACKAGE_FILE).is_file(), "--quiet must not change what new creates");
}

/// #1659 criterion 3: `jet build --sbom --quiet` still writes the SBOM file
/// but suppresses the `sbom: <path>` confirmation line.
#[test]
fn quiet_suppresses_sbom_write_confirmation() {
    let dir = isolated_cwd("quiet_sbom");
    let file = dir.join("main.jet");
    fs::write(&file, "fn run() {\n    print(\"hi\")\n}\n").unwrap();
    let out = Command::new(jet())
        .args(["build", "main.jet", "--sbom", "--quiet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("sbom:"),
        "jet build --sbom --quiet must suppress the sbom confirmation, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(dir.join("build/main.spdx").is_file(), "--quiet must not change what --sbom writes");
}

/// #1659 criterion 3 (round 2): `jet self devtools grammars --quiet` still
/// writes every generated grammar section but suppresses the per-file `wrote
/// <path>` lines and the `regenerated editor grammar sections` summary. Runs
/// in an isolated cwd carrying stub grammar files (with just the real
/// BEGIN/END markers) instead of the tracked `editors/` tree, so a test run
/// never rewrites real repo files.
#[test]
fn quiet_suppresses_devtools_grammars_confirmation() {
    let dir = isolated_cwd("quiet_devtools_grammars");
    let stub = "before\n// BEGIN GENERATED JET SYNTAX HIGHLIGHTS\nstale\n// END GENERATED JET SYNTAX HIGHLIGHTS\nafter\n";
    let files = [
        "editors/vscode/syntaxes/jet.tmLanguage.json",
        "editors/jet.tmGrammar",
        "editors/tree-sitter/grammar.js",
        "editors/zed/languages/jet/highlights.scm",
    ];
    for rel in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, stub).unwrap();
    }

    let out = Command::new(jet())
        .args(["self", "devtools", "grammars", "--quiet"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "jet self devtools grammars --quiet must suppress its status lines, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    for rel in files {
        let content = fs::read_to_string(dir.join(rel)).unwrap();
        assert!(!content.contains("stale"), "--quiet must not change what devtools grammars writes ({rel}): {content}");
    }
}

/// #1659 criterion 3 (round 2): `jet registry publish --quiet` suppresses its
/// `status!`-gated progress narration (`publishing ...`, `[N/3] checking
/// ...`) while every real error/warning (`build: failed`, `tests: failed`,
/// the `could not snapshot` warning) and the exit code stay identical —
/// `--quiet` never hides an error. Uses a locally pre-seeded fake signing key
/// (valid 64-hex format, never a real Ed25519 key) via `JET_KEYS_DIR` so the
/// pre-publish gate reaches its `status!` lines without building the real
/// crypto helper or touching `~/.jet`; the project has no entry file, so the
/// gate fails deterministically before any registry network/git action.
#[test]
fn quiet_suppresses_registry_publish_status_lines() {
    fn publish_project(tag: &str) -> PathBuf {
        let dir = isolated_cwd(tag);
        let init = Command::new(jet()).arg("init").current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
        assert!(init.status.success(), "{:?}", init);
        let keys = dir.join("keys");
        fs::create_dir_all(&keys).unwrap();
        fs::write(keys.join("jet.ed25519"), "test-fixture-seed-not-a-real-key").unwrap();
        fs::write(keys.join("jet.ed25519.pub"), "ab".repeat(32)).unwrap();
        dir
    }

    let loud_dir = publish_project("quiet_publish_loud");
    let loud = Command::new(jet())
        .args(["registry", "publish"])
        .current_dir(&loud_dir)
        .env("NO_COLOR", "1")
        .env("JET_KEYS_DIR", loud_dir.join("keys"))
        .output()
        .unwrap();
    assert_eq!(loud.status.code(), Some(1), "{:?}", loud);
    let loud_stdout = String::from_utf8_lossy(&loud.stdout);
    assert!(loud_stdout.contains("publishing"), "expected loud status narration, got: {loud_stdout}");
    assert!(loud_stdout.contains("[1/3]"), "expected loud gate narration, got: {loud_stdout}");

    let quiet_dir = publish_project("quiet_publish_quiet");
    let quiet = Command::new(jet())
        .args(["registry", "publish", "--quiet"])
        .current_dir(&quiet_dir)
        .env("NO_COLOR", "1")
        .env("JET_KEYS_DIR", quiet_dir.join("keys"))
        .output()
        .unwrap();
    assert_eq!(quiet.status.code(), Some(1), "--quiet must not change the exit code: {:?}", quiet);
    assert!(
        String::from_utf8_lossy(&quiet.stdout).trim().is_empty(),
        "jet registry publish --quiet must suppress its status narration, got: {}",
        String::from_utf8_lossy(&quiet.stdout)
    );
    let quiet_stderr = String::from_utf8_lossy(&quiet.stderr);
    assert!(quiet_stderr.contains("build: failed"), "--quiet must not suppress the real gate error: {quiet_stderr}");
}

/// #1659 criterion 3 (round 2): `jet budget check --quiet` is accepted (it
/// used to be a hard E2102 "unknown flag or argument") and suppresses the
/// trailing `budgets: N passed · report ...` recap while the exit code stays
/// identical. Uses a bare `jet init` project (no `#Budget` declared, so the
/// real behavior is "0 budgets passed") rather than the `budget_project`
/// helper above, which is tailored to a specific budget declaration.
#[test]
fn quiet_accepted_and_suppresses_budget_check_recap() {
    fn budget_dir(tag: &str) -> PathBuf {
        let dir = isolated_cwd(tag);
        let init = Command::new(jet()).arg("init").current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
        assert!(init.status.success(), "{:?}", init);
        fs::write(dir.join("run.jet"), "fn run() {\n    print(\"hi\")\n}\n").unwrap();
        dir
    }

    let loud_dir = budget_dir("quiet_budget_loud");
    let loud = Command::new(jet()).args(["budget", "check"]).current_dir(&loud_dir).env("NO_COLOR", "1").output().unwrap();
    assert!(loud.status.success(), "{:?}", loud);
    assert!(
        !String::from_utf8_lossy(&loud.stderr).trim().is_empty(),
        "expected the loud recap line on stderr"
    );

    let quiet_dir = budget_dir("quiet_budget_quiet");
    let quiet = Command::new(jet()).args(["budget", "check", "--quiet"]).current_dir(&quiet_dir).env("NO_COLOR", "1").output().unwrap();
    assert!(quiet.status.success(), "`jet budget check --quiet` must be accepted, not E2102: {:?}", quiet);
    assert_eq!(quiet.status.code(), loud.status.code(), "--quiet must not change the exit code");
    assert!(
        String::from_utf8_lossy(&quiet.stderr).trim().is_empty(),
        "jet budget check --quiet must suppress its recap line, got: {}",
        String::from_utf8_lossy(&quiet.stderr)
    );
}

#[test]
fn bench_targets_filter_and_json_match_test_runner_contract() {
    if !common::have_rustc() {
        return;
    }
    let dir = isolated_cwd("bench_runner_contract");
    fs::create_dir_all(dir.join("nested")).unwrap();
    let source = r#"fn run() {}

#Test("needle") {
    require_eq(1, 1)
}

#Test("other") {
    require_eq(2, 2)
}

#Bench("needle") {
    require_eq(1, 1)
}

#Bench("other") {
    require_eq(2, 2)
}
"#;
    fs::write(dir.join("root.jet"), source).unwrap();
    fs::write(dir.join("nested/child.jet"), source).unwrap();

    let tests = Command::new(jet())
        .args(["test", "--show-default", ".", "--filter=needle"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(tests.status.success(), "test target failed: {}", String::from_utf8_lossy(&tests.stderr));
    let test_stdout = String::from_utf8_lossy(&tests.stdout);
    assert!(test_stdout.contains("needle"), "filtered test missing needle: {test_stdout}");
    assert!(!test_stdout.contains("other: pass"), "filtered test ran other: {test_stdout}");

    let benches = Command::new(jet())
        .args(["bench", "--show-default", ".", "--filter=needle"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(benches.status.success(), "bench target failed:\nstdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&benches.stdout), String::from_utf8_lossy(&benches.stderr));
    let bench_stdout = String::from_utf8_lossy(&benches.stdout);
    assert_eq!(bench_stdout.matches("ns/iter").count(), 2, "directory bench must run one selected region per file: {bench_stdout}");
    assert!(bench_stdout.contains("root.jet::needle"), "human bench output must qualify region by path: {bench_stdout}");
    assert!(bench_stdout.contains("nested/child.jet::needle"), "human bench output must include nested file path: {bench_stdout}");
    assert!(!bench_stdout.contains("::other"), "filtered bench ran other: {bench_stdout}");

    let json = Command::new(jet())
        .args(["bench", "--show-default", ".", "--filter=needle", "--json"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(json.status.success(), "bench JSON target failed: {}", String::from_utf8_lossy(&json.stderr));
    let json_stdout = String::from_utf8_lossy(&json.stdout);
    let records: Vec<_> = json_stdout
        .lines()
        .map(|line| parse_json(line).unwrap_or_else(|_| panic!("bench JSON line does not parse: {line}")))
        .collect();
    assert_eq!(records.len(), 2, "JSON must contain one record per selected region: {json_stdout}");
    for record in records {
        let JSONValue::Object(record) = record else { panic!("bench JSON record is not an object") };
        assert!(matches!(record.get("profile"), Some(JSONValue::String(profile)) if profile == "release"), "JSON record has wrong profile: {record:?}");
        assert!(matches!(record.get("name"), Some(JSONValue::String(name)) if name.ends_with("::needle")), "JSON region name is not path-qualified: {record:?}");
    }
}
