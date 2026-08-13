use super::*;

#[test]
fn no_separator_positional_regression() {
    // Plain positional words with no `--` still reach the program (regression
    // guard). `jet run file.jet hello` → len == 2.
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", "--release", p.to_str().unwrap(), "hello"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "2",
        "expected 2 args (argv[0] + hello), got: {stdout}"
    );
}

#[test]
fn unknown_flag_before_separator_is_e2102_with_passthrough_hint() {
    // `jet run file.jet --port` (no `--`) — unknown flag before `--` is E2102
    // and the Fix line teaches the `--` form (D-CLI1=A).
    let p = args_fixture(&line!().to_string());
    let out = Command::new(jet())
        .args(["run", p.to_str().unwrap(), "--port"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown flag before -- should exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E2102"), "should cite E2102:\n{stderr}");
    assert!(
        stderr.contains("--"),
        "Fix should mention `--` separator:\n{stderr}"
    );
}

#[test]
fn profile_unknown_name_emits_e1219() {
    // D-BUILDPROFILE1: `--profile=<unknown>` with no package.jet defining that name
    // must emit E1219 and exit 1 (user error).
    let p = std::env::temp_dir().join("jet_cli_profile_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=staging"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown --profile should exit 1 (user error)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1219"),
        "unknown profile should cite E1219:\n{stderr}"
    );
    assert!(
        stderr.contains("staging"),
        "E1219 should name the unknown profile:\n{stderr}"
    );
}

#[test]
fn profile_release_flag_is_accepted() {
    // `--release` is valid (blessed profile) and must not emit E1219.
    let p = std::env::temp_dir().join("jet_cli_release_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    // We can't guarantee rustc is in PATH for the binary build, but `jet check`
    // doesn't accept --release yet, so test that `jet build --release` at least
    // doesn't emit E1219. We check that the exit code is NOT 1-with-E1219.
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--release"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--release must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_ci_flag_is_accepted() {
    let p = std::env::temp_dir().join("jet_cli_ci_test.jet");
    fs::write(&p, "fn run() { print(\"ok\") }\n").unwrap();
    let out = Command::new(jet())
        .args(["build", p.to_str().unwrap(), "--profile=ci"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "--profile=ci must not emit E1219 (it's a blessed profile):\n{stderr}"
    );
}

#[test]
fn profile_custom_name_from_package_jet() {
    let dir = std::env::temp_dir().join(format!(
        "jet_cli_custom_profile_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        r#"name: "p"
version: "0.1.0"
build: { staging: Build.{ optimize: basic } }
"#,
    )
    .unwrap();
    let main = dir.join("main.jet");
    fs::write(&main, "fn run() { print(\"ok\") }\n").unwrap();
    // Isolated cwd: this fixture's stem is `main` — see `isolated_cwd`. Also
    // the semantically correct place for `build/` to land, since it's this
    // fixture's own project directory.
    let out = Command::new(jet())
        .args(["build", main.to_str().unwrap(), "--profile=staging"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1219"),
        "package.jet-defined profile must resolve:\n{stderr}"
    );
}

#[test]
fn expand_inline_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "inline"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "expand --facts inline should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_inline.txt", &s);
}

#[test]
fn expand_callable_signature_uses_one_checked_fact_document() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/callable/callable_policies.jet");
    let human = Command::new(jet())
        .args(["inspect", "expand", "--facts", "callable-signature"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(0), "{}", String::from_utf8_lossy(&human.stderr));
    let human_text = scrub_fixture(&String::from_utf8_lossy(&human.stdout), &fixture);
    assert!(human_text.contains("load_user [fn:"), "{human_text}");
    assert!(human_text.contains("label: label = \"user\": String"), "{human_text}");
    assert!(human_text.contains("policies=[trace(\"users.load\")]") , "{human_text}");

    let json = Command::new(jet())
        .args(["inspect", "expand", "--facts", "callable-signature", "--json"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0), "{}", String::from_utf8_lossy(&json.stderr));
    let json_text = scrub_fixture(&String::from_utf8_lossy(&json.stdout), &fixture);
    assert!(parse_json(&json_text).is_ok(), "{json_text}");
    assert!(json_text.contains("\"selection\":\"callable-signature\""));
    assert!(json_text.contains("\"policies\":[\"trace(\\\"users.load\\\")\"]"));
    assert!(json_text.contains("\"default\":\"\\\"user\\\"\""));
}

#[test]
fn expand_derive_lens_projects_derived_capabilities() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples/features/types/auto_derive_policy/main.jet");
    let human = Command::new(jet())
        .args(["inspect", "expand", "--facts", "derive"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(0), "{}", String::from_utf8_lossy(&human.stderr));
    let human_text = scrub_fixture(&String::from_utf8_lossy(&human.stdout), &fixture);
    assert!(human_text.contains("derive —"), "{human_text}");
    assert!(human_text.contains("Visible: Printable"), "{human_text}");

    let json = Command::new(jet())
        .args(["inspect", "expand", "--facts", "derive", "--json"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0), "{}", String::from_utf8_lossy(&json.stderr));
    let json_text = scrub_fixture(&String::from_utf8_lossy(&json.stdout), &fixture);
    assert!(parse_json(&json_text).is_ok(), "{json_text}");
    assert!(json_text.contains("\"selection\":\"derive\""));
    assert!(json_text.contains("\"derives\":[\"Printable\"]"));
}

#[test]
fn expand_json_is_canonical_and_lens_scoped() {
    let p = expand_fixture();
    let run = || {
        Command::new(jet())
            .args(["inspect", "expand", "--facts", "inline", "--json"])
            .arg(&p)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout, "expand JSON must be byte-stable");
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.starts_with('{'), "JSON mode must not print human headers: {stdout}");
    assert!(stdout.contains("\"schema_version\":13"), "must reuse semindex schema: {stdout}");
    assert!(stdout.contains("\"expand\":{\"selection\":\"inline\""), "missing expand projection: {stdout}");
    assert!(stdout.contains("\"contract\":\"#Inline"), "inline facts missing: {stdout}");
    assert!(!stdout.contains("inline —"), "human lens header leaked into JSON: {stdout}");
}

#[test]
fn expand_layout_human_and_json_are_deterministic() {
    let fixture = expand_layout_fixture();
    let run = || {
        Command::new(jet())
            .args(["inspect", "expand", "--facts", "layout"])
            .arg(&fixture)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout, "layout text must be byte-stable");
    let human = scrub_fixture(&String::from_utf8_lossy(&first.stdout), &fixture);
    assert!(!human.contains('\u{1b}'), "NO_COLOR leaked ANSI: {human:?}");
    for type_name in ["PlainPacket", "CPacket", "ColumnPacket", "PacketState"] {
        assert!(human.contains(&format!("{type_name}.$layout")), "{type_name}: {human}");
    }
    assert!(human.contains("size=unknown") && human.contains("offset=unknown"));
    assert!(human.contains("byte_facts=unavailable") && human.contains("E0959"));
    check_snapshot("expand_layout.txt", &human);

    let json_run = || {
        Command::new(jet())
            .args(["inspect", "expand", "--facts", "layout", "--json"])
            .arg(&fixture)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let json_first = json_run();
    let json_second = json_run();
    assert_eq!(json_first.status.code(), Some(0));
    assert_eq!(json_first.stdout, json_second.stdout, "layout JSON must be byte-stable");
    let json = scrub_fixture(&String::from_utf8_lossy(&json_first.stdout), &fixture);
    assert!(parse_json(&json).is_ok(), "layout JSON must parse: {json}");
    assert!(json.contains("\"selection\":\"layout\""));
    assert!(json.contains("\"kind\":\"c\"") && json.contains("\"kind\":\"columnar\""));
    assert!(json.contains("\"type\":\"PacketState\"") && json.contains("\"size\":null"));
    assert!(json.contains("\"offset\":null"));
    assert!(json.contains("\"byte_facts\":{\"diagnostic\""));
    assert!(json.contains("\"status\":\"unavailable\""));
    assert!(json.contains("\"code\":\"E0959\""));
}

#[test]
fn expand_effects_and_layout_report_checked_facts() {
    let fixture = expand_effects_layout_fixture();
    let effects = Command::new(jet())
        .args(["inspect", "expand", "--facts", "effects"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(effects.status.code(), Some(0));
    let effects_human = scrub_fixture(&String::from_utf8_lossy(&effects.stdout), &fixture);
    assert!(effects_human.contains("audit"), "{effects_human}");
    // `=[Log.Audit]=>` is an upper bound. The pure fixture body resolves to
    // an empty inferred row; a declaration is not an executed effect.
    assert!(effects_human.contains("resolved=[]"), "{effects_human}");

    let layout = Command::new(jet())
        .args(["inspect", "expand", "--facts", "layout"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(layout.status.code(), Some(0));
    let layout_human = scrub_fixture(&String::from_utf8_lossy(&layout.stdout), &fixture);
    assert!(layout_human.contains("AuditPacket.$layout"), "{layout_human}");
    assert!(layout_human.contains("byte_facts=unavailable"), "{layout_human}");
    assert!(layout_human.contains("[E0959]"), "{layout_human}");
    check_snapshot(
        "expand_effects_layout.txt",
        &format!("effects\n{effects_human}layout\n{layout_human}"),
    );

    let effects_json = Command::new(jet())
        .args(["inspect", "expand", "--facts", "effects", "--json"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(effects_json.status.code(), Some(0));
    let effects_json = String::from_utf8_lossy(&effects_json.stdout);
    assert!(parse_json(&effects_json).is_ok(), "{effects_json}");
    assert!(effects_json.contains("\"selection\":\"effects\""));
    assert!(effects_json.contains("\"inferred\":[]"));

    let layout_json = Command::new(jet())
        .args(["inspect", "expand", "--facts", "layout", "--json"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(layout_json.status.code(), Some(0));
    let layout_json = String::from_utf8_lossy(&layout_json.stdout);
    assert!(parse_json(&layout_json).is_ok(), "{layout_json}");
    assert!(layout_json.contains("\"selection\":\"layout\""));
    assert!(layout_json.contains("\"byte_facts\":{\"diagnostic\""));
    assert!(layout_json.contains("\"status\":\"unavailable\""));
    assert!(layout_json.contains("\"code\":\"E0959\""));
}

#[test]
fn expand_json_bare_projects_every_lens() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--json"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(parse_json(&stdout).is_ok(), "bare expand JSON must parse: {stdout}");
    assert!(stdout.contains("\"selection\":\"all\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"inline\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"memory\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"web\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"effects\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"layout\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"derive\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"callable-signature\""), "{stdout}");
    assert!(!stdout.contains("inline —"), "human output leaked into JSON: {stdout}");
}

#[test]
fn expand_json_selected_empty_and_positions_are_proved() {
    let fixture = expand_fixture();
    let empty = Command::new(jet())
        .args(["inspect", "expand", "--facts", "memory", "--json"])
        .arg(&fixture)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(empty.status.code(), Some(0));
    let empty_json = String::from_utf8_lossy(&empty.stdout);
    assert!(parse_json(&empty_json).is_ok(), "selected empty JSON must parse: {empty_json}");
    assert!(empty_json.contains("\"selection\":\"memory\""));
    assert!(empty_json.contains("\"facts\":[]"), "selected empty lens must be explicit: {empty_json}");

    let memory = Command::new(jet())
        .args(["inspect", "expand", "--facts", "memory", "--json"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/memory/no_alloc_policy.jet"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(memory.status.code(), Some(0));
    let memory_json = String::from_utf8_lossy(&memory.stdout);
    assert!(parse_json(&memory_json).is_ok());
    assert!(memory_json.contains("\"fact\":\"no_alloc\""));
    assert!(memory_json.contains("\"line\":5"), "memory fact location missing: {memory_json}");

    let web = Command::new(jet())
        .args(["inspect", "expand", "--facts", "web", "--json"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/web/web_app.jet"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(web.status.code(), Some(0));
    let web_json = String::from_utf8_lossy(&web.stdout);
    assert!(parse_json(&web_json).is_ok());
    assert!(web_json.contains("\"kind\":\"web_graph\""));
    assert!(web_json.contains("\"span\":{"), "web fact positions missing: {web_json}");
}

#[test]
fn expand_json_compile_error_uses_machine_diagnostics() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .args(["inspect", "expand", "--json"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.starts_with("{\"schema\":\"jet.report/v1\""), "JSON diagnostic must use jet.report/v1: {stdout}");
    assert!(stdout.contains("\"code\":\"E0102\""), "missing registered diagnostic: {stdout}");
    assert!(parse_json(&stdout).is_ok(), "diagnostic JSON line must parse: {stdout}");
    assert!(!stdout.contains("Error ["), "human diagnostic leaked into JSON: {stdout}");
    assert!(stderr.is_empty(), "JSON mode should keep stderr quiet: {stderr}");
}

#[test]
fn expand_json_unknown_lens_uses_machine_diagnostic() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "bogus", "--json"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("{\"schema\":\"jet.report/v1\""), "{stdout}");
    assert!(stdout.contains("\"code\":\"E2941\""), "{stdout}");
    assert!(parse_json(&stdout).is_ok(), "diagnostic JSON line must parse: {stdout}");
    assert!(out.stderr.is_empty());
}

#[test]
fn expand_all_golden() {
    let p = expand_fixture();
    // Bare `jet inspect expand <file>`: every lens, grouped, magic default.
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare expand should exit 0:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stdout), &p);
    check_snapshot("expand_all.txt", &s);
}

#[test]
fn expand_unknown_lens_golden() {
    let p = expand_fixture();
    let out = Command::new(jet())
        .args(["inspect", "expand", "--facts", "bogus"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unknown lens should exit 1 (USER_ERROR), listing available lenses"
    );
    let s = scrub_fixture(&String::from_utf8_lossy(&out.stderr), &p);
    check_snapshot("expand_unknown_lens.txt", &s);
}

#[test]
fn expand_missing_file_is_user_error() {
    let out = Command::new(jet()).args(["inspect", "expand"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "missing entry file is USER_ERROR"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs an entry file"),
        "should explain the missing file:\n{}",
        stderr
    );
}

#[test]
fn expand_compile_error_reports_ordinary_diagnostics() {
    let p = bad_file(&line!().to_string());
    let out = Command::new(jet())
        .args(["inspect", "expand"])
        .arg(&p)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "a program that fails to compile can't print facts (USER_ERROR)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E0102"),
        "should render the ordinary front-end diagnostic:\n{}",
        stderr
    );
}

#[test]
fn stale_manifest_name_pack_jet_is_e1226() {
    let dir = isolated_cwd("stale_pack_jet");
    fs::write(
        dir.join("pack.jet"),
        "name: \"x\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("add")
        .arg("dep")
        .arg("--path")
        .arg("../dep")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("pack.jet"),
        "names the found file:\n{stderr}"
    );
    assert!(
        stderr.contains("package.jet"),
        "names the fix target:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_jet_toml_is_e1226() {
    let dir = isolated_cwd("stale_jet_toml");
    fs::write(dir.join("jet.toml"), "").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("jet.toml"),
        "names the found file:\n{stderr}"
    );
}

#[test]
fn stale_manifest_name_payload_jet_is_e1226() {
    let dir = isolated_cwd("stale_payload_jet");
    fs::write(dir.join("payload.jet"), "").unwrap();
    let out = Command::new(jet())
        .args(["inspect", "schema"])
        .arg("status")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1226"),
        "expected E1226 in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("payload.jet"),
        "names the found file:\n{stderr}"
    );
}

/// `jetpack.toml` is a different, still-live file (D-JPK-FILES repo
/// metadata) — it must NOT be mistaken for a retired manifest name.
#[test]
fn jetpack_toml_alone_is_not_e1226() {
    let dir = isolated_cwd("jetpacktoml_not_stale");
    fs::write(dir.join("jetpack.toml"), "[repo]\nname = \"x\"\n").unwrap();
    let out = Command::new(jet())
        .arg("build")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1226"),
        "jetpack.toml is a different live file, not a retired manifest name:\n{stderr}"
    );
    assert!(
        stderr.contains("no file given and no `package.jet` found") || stderr.contains("E1225"),
        "should fall back to the generic no-manifest message:\n{stderr}"
    );
}

/// D-PLUGIN1=B (c81): a `target: plugin` package is deny-by-default — its own
/// code using any effect (here `core.env`) must fail cleanly at build time
/// (E1258), not defer to a runtime instantiation failure. This check lives in
/// the CLI's post-compile effect-budget pass (`Source/CmdCompile.rs`), so it
/// needs the real subprocess (not the `jet::compile_plugin` library call the
/// `tests/ui` `@plugin_target` harness drives).
#[test]
fn plugin_using_an_effect_is_e1258() {
    let dir = isolated_cwd("plugin_effect_denied");
    fs::write(
        dir.join("main.jet"),
        "use core.env as env\n\npub fn get_secret() => Int {\n    _ :: env.get(\"SECRET\")\n    return 1\n}\n",
    )
    .unwrap();
    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1258"),
        "expected E1258 (plugin capability-denied) in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Env"),
        "should name the offending effect:\n{stderr}"
    );
}

/// D-DEP-WASM1=A (c81): `jet build --target=plugin` shells out to
/// `wasm-tools` to lift the rustc-built core wasm module into a Component. A
/// PATH without `wasm-tools` on it (but with `rustc` still reachable, so the
/// core-module half of the build succeeds) must fail as a clean E1259, never
/// a raw "No such file or directory" panic (I2).
#[test]
fn plugin_missing_wasm_tools_is_e1259() {
    let which = |tool: &str| -> Option<String> {
        Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let (Some(rustc_path), Some(lld_path)) = (which("rustc"), which("lld")) else {
        eprintln!("note: skipping plugin_missing_wasm_tools_is_e1259 (no `rustc`/`lld` on PATH to re-expose)");
        return;
    };

    let dir = isolated_cwd("plugin_no_wasmtools");
    fs::write(
        dir.join("main.jet"),
        "pub fn scale(a: Float, b: Float) => Float {\n    return a * b\n}\n",
    )
    .unwrap();

    // A minimal PATH exposing only `rustc` + `lld` (via symlinks), so the
    // core-wasm-module half of the build still works but `wasm-tools`
    // resolves to nothing.
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink(&rustc_path, bin_dir.join("rustc"));
        let _ = symlink(&lld_path, bin_dir.join("lld"));
    }

    let out = Command::new(jet())
        .arg("build")
        .arg("main.jet")
        .arg("--target=plugin")
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("PATH", &bin_dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1259"),
        "expected E1259 (missing wasm-tools) in stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "must never panic, only report a clean diagnostic (I2):\n{stderr}"
    );
}

#[test]
fn monorepo_bare_entry_honors_d_ile1_search_order() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/monorepo");
    let root = isolated_cwd("monorepo_d_ile1");
    fs::remove_dir_all(&root).ok();
    copy_dir_all(&fixture, &root);

    let run = |dir: &Path, extra_args: &[&str]| -> std::process::Output {
        Command::new(jet())
            .arg("run")
            .args(extra_args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let mut golden = String::new();

    // 1. Bare `jet run` at the workspace root: both members resolve via
    //    D-ILE1 (`<package>.jet`, since neither has `run.jet`/`src/run.jet`), so the
    //    result is the D-CLI-BARE1 ambiguity error naming both.
    let out = run(&root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "ambiguous bare run is USAGE:\n{stderr}"
    );
    assert!(
        stderr.contains("ambiguous"),
        "expected the D-CLI-BARE1 ambiguity error:\n{stderr}"
    );
    assert!(
        stderr.contains("hello") && stderr.contains("ranker"),
        "ambiguity error should list both runnable members by their real package.jet name:\n{stderr}"
    );
    assert!(
        !stderr.contains("hello\"") && !stderr.contains("ranker\""),
        "member names must not carry a stray trailing quote:\n{stderr}"
    );

    // 2. `-p hello` picks the member unambiguously and actually runs it.
    let out = run(&root, &["-p", "hello"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "-p hello should run: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    golden.push_str(&String::from_utf8(out.stdout).unwrap());

    // 3. `-p ranker` likewise.
    let out = run(&root, &["-p", "ranker"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "-p ranker should run: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    golden.push_str(&String::from_utf8(out.stdout).unwrap());
    // Package workspaces use this dedicated golden path because they are not
    // single-entry files in `tests/golden.rs`'s example scan.
    assert_eq!(
        golden,
        include_str!("../../examples/features/expected/packages/monorepo.out"),
        "monorepo output differs from its golden artifact"
    );

    // 4. `cd packages/hello && jet run` (bare, single-package convention):
    //    the member directory's own `package.jet` names it `hello`, so D-ILE1
    //    resolves `hello.jet` directly — no workspace ambiguity from inside.
    let member_dir = root.join("packages/hello");
    let out = run(&member_dir, &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare run inside a member should run its own entry: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello from the monorepo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // 5. Outside any package or workspace, the bare-form usage error is
    //    unchanged (D-CLI-BARE1: "outside a package the bare form stays the
    //    current usage error").
    let outside = isolated_cwd("monorepo_d_ile1_outside");
    let out = run(&outside, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr.contains("no file given and no `package.jet` found"),
        "outside-package bare error text must stay the current usage error:\n{stderr}"
    );
}

#[test]
fn malformed_workspace_never_falls_back_to_an_ordinary_entry() {
    let dir = isolated_cwd("workspace_no_fallback");
    fs::write(dir.join("workspace.jet"), "module workspace { members: [\n").unwrap();
    fs::write(
        dir.join("run.jet"),
        "fn run() { print(\"SHOULD-NOT-RUN\") }\n",
    )
    .unwrap();
    let output = Command::new(jet())
        .args(["run"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SHOULD-NOT-RUN"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0995") || stderr.contains("E0003"), "{stderr}");
}

#[test]
fn stale_workspace_lock_never_becomes_an_empty_member_index() {
    let dir = isolated_cwd("workspace_lock_no_fallback");
    fs::create_dir_all(dir.join(".jet")).unwrap();
    fs::write(
        dir.join(".jet/lock"),
        "version = 1\nworkspace_source_digest = \"sha256-stale\"\n",
    )
    .unwrap();
    fs::write(dir.join("run.jet"), "fn run() { print(\"SHOULD-NOT-RUN\") }\n").unwrap();
    let output = Command::new(jet())
        .args(["run"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SHOULD-NOT-RUN"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("malformed or stale"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn service_probe_unavailable_without_dev_reports_diagnostic() {
    let dir = isolated_cwd("service_probe_no_env");
    fs::write(dir.join("package.jet"), "name: \"app\"\nversion: \"0.1.0\"\n").unwrap();
    fs::write(dir.join("run.jet"), r#"module env.dev {
    services: { mydb: { run: ["echo", "mydb"], ready: "true" } }
}

module perf.package {
    budgets: [Budget.{
        name: "readiness",
        scope: .Service("mydb"),
        metric: .ServiceReadiness,
        provider: .ServiceProbe("mydb"),
        comparison: .AbsoluteFrom("local/mydb"),
        limit: .AtMost(500ms),
        enforcement: .Warn,
    }],
}
fn run() {}
"#).unwrap();
    // jet budget check: ServiceProbe stub should report "unavailable", not 101.
    let out = Command::new(jet())
        .args(["budget", "check"])
        .current_dir(&dir)
        .output()
        .unwrap();
    // Exit code must not be 101 (I2 — rustc never speaks to user).
    assert_ne!(out.status.code(), Some(101), "rustc must never speak to user (I2)");
    assert_ne!(out.status.code(), Some(2), "usage error unexpected");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("ServiceProbe") || combined.contains("unavailable") || combined.contains("jet dev"),
        "expected unavailability message; got:\n{combined}"
    );
}

#[test]
fn service_probe_uses_jetpack_lifecycle_and_produces_twenty_samples() {
    use jet_foundation::PerformanceBudget::CanonicalJson;

    let dir = isolated_cwd("service_probe_runtime");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"app\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("env.jet"),
        r#"module env.dev {
    services: { mydb: { run: ["sleep", "30"], ready: "true" } }
}
"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/main.jet"),
        r#"use project.env.dev

module perf.package {
    budgets: [Budget.{
        name: "readiness",
        scope: .Service("mydb"),
        metric: .ServiceReadiness,
        provider: .ServiceProbe("mydb"),
        comparison: .AbsoluteFrom("local/mydb"),
        limit: .AtMost(500ms),
        enforcement: .Warn,
    }],
}
fn run() {}
"#,
    )
    .unwrap();

    let out = Command::new(jet())
        .args(["self", "devtools", "probe", "src/main.jet"])
        .current_dir(&dir)
        .env("JETPACK_ROOT", dir.join("jetpack-root"))
        .env("HOME", dir.join("home"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let reports = fs::read_dir(dir.join(".jet/perf/reports"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(reports.len(), 1, "expected one report; got {reports:?}");
    let bytes = fs::read(&reports[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else {
        panic!("report")
    };
    let CanonicalJson::Object(content) = &report["content"] else {
        panic!("content")
    };
    let CanonicalJson::Array(measurements) = &content["measurements"] else {
        panic!("measurements")
    };
    assert_eq!(measurements.len(), 1);
    let CanonicalJson::Object(measurement) = &measurements[0] else {
        panic!("measurement")
    };
    let CanonicalJson::Object(provider) = &measurement["provider"] else {
        panic!("provider")
    };
    assert_eq!(provider["kind"], CanonicalJson::String("ServiceProbe".into()));
    assert_eq!(provider["identity"], CanonicalJson::String("mydb".into()));
    let CanonicalJson::Array(samples) = &measurement["samples"] else {
        panic!("samples")
    };
    assert_eq!(samples.len(), 20, "ServiceProbe must produce exactly 20 samples");
}
