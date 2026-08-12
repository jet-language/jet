use super::*;

#[test]
fn inspect_unsafe_reports_policy_provenance_and_operations() {
    let dir = isolated_cwd("inspect_unsafe");
    let file = dir.join("main.jet");
    fs::write(&file, "use core.mem\nfn run() {\n value :: 7\n #Unsafe(\"local\", obligations: .Track) {\n  pointer :: *Int.{ *value }\n  assert no_alias\n  band :: pointer.*..8\n  assert valid_ptr, aligned\n  print(band.start)\n }\n}\n").unwrap();
    let human = Command::new(jet()).args(["inspect", "unsafe", "main.jet"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(human.status.code(), Some(0), "{}", String::from_utf8_lossy(&human.stderr));
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("main.jet:4:2"), "{human}");
    assert!(human.contains("main.jet:5:14"), "{human}");
    assert!(!human.contains('\u{1b}'), "NO_COLOR leaked ANSI: {human:?}");
    let output = Command::new(jet()).args(["inspect", "unsafe", "main.jet", "--json"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"schema_version\":1") && stdout.contains("\"mode\":\"Obligations\"") && stdout.contains("\"kind\":\"raw_pointer\"") && stdout.contains("\"kind\":\"dereference\"") && stdout.contains("\"discharged\":true"), "{stdout}");
    assert!(parse_json(&stdout).is_ok(), "inspect unsafe JSON must parse: {stdout}");
    assert!(stdout.contains("\"location\":{\"start\":{\"line\":4,\"column\":2}"), "{stdout}");
    let repeat = Command::new(jet()).args(["inspect", "unsafe", "main.jet", "--json"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(repeat.status.code(), Some(0));
    assert_eq!(stdout, String::from_utf8(repeat.stdout).unwrap());
}

#[test]
fn inspect_unsafe_loader_failures_use_standard_diagnostics() {
    let dir = isolated_cwd("inspect_unsafe_loader");
    let file = dir.join("malformed.jet");
    fs::write(&file, "fn run( {\n").unwrap();
    let malformed = Command::new(jet()).args(["inspect", "unsafe", "malformed.jet"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(malformed.status.code(), Some(1));
    let stderr = String::from_utf8(malformed.stderr).unwrap();
    assert!(stderr.contains("Error [E0003]") && stderr.contains("Why:") && stderr.contains("Fix:") && stderr.contains("malformed.jet:1:9"), "{stderr}");

    let colored = Command::new(jet()).args(["inspect", "unsafe", "malformed.jet", "--color=always"]).current_dir(&dir).output().unwrap();
    assert_eq!(colored.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&colored.stderr).contains("\x1b"), "--color=always must preserve diagnostic styling");

    let missing = Command::new(jet()).args(["inspect", "unsafe", "missing.jet"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let stderr = String::from_utf8(missing.stderr).unwrap();
    assert!(stderr.contains("Error [E0603]") && stderr.contains("Why:") && stderr.contains("Fix:"), "{stderr}");

    let missing_json = Command::new(jet()).args(["inspect", "unsafe", "missing.jet", "--json"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(missing_json.status.code(), Some(1));
    let stdout = String::from_utf8(missing_json.stdout).unwrap();
    let stderr = String::from_utf8(missing_json.stderr).unwrap();
    assert!(stdout.starts_with("{\"schema\":\"jet.report/v1\""), "{stdout}");
    assert!(stdout.contains("\"code\":\"E0603\""), "{stdout}");
    assert!(parse_json(&stdout).is_ok(), "loader diagnostic JSON must parse: {stdout}");
    assert!(stderr.is_empty(), "JSON loader diagnostics should keep stderr quiet: {stderr}");

    fs::write(dir.join("missing_reason.jet"), "fn run() {\n #Unsafe { print(\"unchecked\") }\n}\n").unwrap();
    let missing_reason = Command::new(jet()).args(["inspect", "unsafe", "missing_reason.jet"]).current_dir(&dir).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(missing_reason.status.code(), Some(1));
    let stderr = String::from_utf8(missing_reason.stderr).unwrap();
    assert!(stderr.contains("Error [E3112]") && stderr.contains("Why:") && stderr.contains("Fix:") && stderr.contains("missing_reason.jet:2:2"), "{stderr}");
}

#[test]
fn inspect_unsafe_reports_sema_diagnostics_with_loaded_module_sources() {
    let dir = isolated_cwd("inspect_unsafe_diagnostics");
    let main = dir.join("main.jet");
    let helper = dir.join("helper.jet");
    let runner = dir.join("runner");
    fs::create_dir(&runner).unwrap();
    fs::write(&main, "use \"helper\"\nfn run() {}\n").unwrap();
    fs::write(&helper, "use core.mem\n#Unsafe(\"helper\", obligations: .Track) fn unsafe_run() {\n    value :: 7\n    pointer :: *Int.{*value}\n}\n").unwrap();

    let human = Command::new(jet()).args(["inspect", "unsafe", main.to_str().unwrap()]).current_dir(&runner).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(human.status.code(), Some(1), "{}", String::from_utf8_lossy(&human.stderr));
    let stderr = String::from_utf8(human.stderr).unwrap();
    assert!(stderr.contains("Error [E3107]"), "{stderr}");
    assert!(stderr.contains("helper.jet:4:16"), "{stderr}");

    let json = Command::new(jet()).args(["inspect", "unsafe", main.to_str().unwrap(), "--json"]).current_dir(&runner).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(json.status.code(), Some(1), "{}", String::from_utf8_lossy(&json.stderr));
    let stdout = String::from_utf8(json.stdout).unwrap();
    let stderr = String::from_utf8(json.stderr).unwrap();
    assert!(stdout.starts_with("{\"schema\":\"jet.report/v1\""), "{stdout}");
    assert!(stdout.contains("\"code\":\"E3107\"") && stdout.contains("helper.jet") && stdout.contains("\"line\":4,\"col\":16"), "{stdout}");
    assert!(parse_json(&stdout).is_ok(), "inspection diagnostic JSON must parse: {stdout}");
    assert!(stderr.is_empty(), "JSON inspection diagnostics should keep stderr quiet: {stderr}");

    let bad_main = dir.join("bad_main.jet");
    let bad_helper = dir.join("bad_helper.jet");
    fs::write(&bad_main, "use \"bad_helper\"\nfn run() {}\n").unwrap();
    fs::write(&bad_helper, "fn run( {\n").unwrap();
    let loader_human = Command::new(jet()).args(["inspect", "unsafe", bad_main.to_str().unwrap()]).current_dir(&runner).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(loader_human.status.code(), Some(1), "{}", String::from_utf8_lossy(&loader_human.stderr));
    let loader_stderr = String::from_utf8(loader_human.stderr).unwrap();
    assert!(loader_stderr.contains("bad_helper.jet:1:9") && !loader_stderr.contains("bad_main.jet:1:9"), "{loader_stderr}");

    let loader_json = Command::new(jet()).args(["inspect", "unsafe", bad_main.to_str().unwrap(), "--json"]).current_dir(&runner).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(loader_json.status.code(), Some(1), "{}", String::from_utf8_lossy(&loader_json.stderr));
    let loader_stdout = String::from_utf8(loader_json.stdout).unwrap();
    assert!(loader_stdout.contains("\"file\":\"bad_helper.jet\"") && loader_stdout.contains("\"line\":1,\"col\":9"), "{loader_stdout}");
    assert!(parse_json(&loader_stdout).is_ok(), "imported loader diagnostic JSON must parse: {loader_stdout}");
    assert!(loader_json.stderr.is_empty(), "JSON imported loader diagnostics should keep stderr quiet");
}

#[test]
fn epoch3_string_and_set_surface_runs_on_default_tier() {
    let dir = isolated_cwd("epoch3_string_set_default");
    fs::write(
        dir.join("main.jet"),
        r#"fn run() {
    print("  jet".trim_start())
    print("jet  ".trim_end())
    print("jet".pad_start(5, "."))
    print("jet".pad_end(5, "."))
    print("hello jet".index_of("jet"))
    print("banana".count("an"))
    print("hELLO jet".to_title())
    print("Hello".is_alphabetic())
    print("123".is_numeric())
    print(" \t".is_whitespace())
    print("Jet lang".is_ascii())
    pair :: "left:right".split_once(":") ?? panic("split")
    print(pair.before)
    print(pair.after)
    print(Set.from([1, 2, 3]).intersection(Set.from([2, 3, 4])).len())
    print(Set.from([1, 2, 3]).symmetric_difference(Set.from([2, 3, 4])).len())
    print(Set.from([1, 2, 3]).is_subset(Set.from([1, 2, 3, 4])))
    print(Set.from([1, 2, 3]).is_superset(Set.from([1, 2])))
    print(Set.from([1, 2, 3]).is_disjoint(Set.from([8])))
    print(SortedSet.from([1, 2, 3]).union(SortedSet.from([3, 4])).len())
    print(SortedSet.from([1, 2, 3]).difference(SortedSet.from([2, 3, 4])).len())
    print(Set.from(["left", "right"]).intersection(Set.from(["right", "other"])).len())
    print(SortedSet.from(["left", "right"]).is_subset(SortedSet.from(["left", "right", "other"])))
    print(Set.from(["a", "a"]).len())
    print(SortedSet.from(["z", "z", "a"]).len())
    print(SortedSet.from(["z", "a"]).first() ?? "none")
    print(SortedSet.from(["z", "a"]).last() ?? "none")
    typed_words := SortedSet<String>.{SortedSet.new()}
    print(typed_words.add("z"))
    print(typed_words.add("z"))
    print(typed_words.first() ?? "none")
}
"#,
    )
    .unwrap();
    let output = Command::new(jet())
        .args(["run", "main.jet"])
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
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "jet\njet\n..jet\njet..\n6\n2\nHello Jet\ntrue\ntrue\ntrue\ntrue\nleft\nright\n2\n2\ntrue\ntrue\ntrue\n4\n1\n1\ntrue\n1\n2\na\nz\ntrue\nfalse\nz\n"
    );
}

#[test]
fn configured_organization_unsafe_policy_fails_closed_and_keeps_path() {
    let dir = isolated_cwd("organization_unsafe");
    fs::write(dir.join("main.jet"), "fn run() {}\n").unwrap();
    let configured = dir.join("org-policy.jet");
    let output = Command::new(jet()).args(["check", "main.jet"]).current_dir(&dir).env(jet::Syntax::ENV_ORG_UNSAFE_POLICY, &configured).env("NO_COLOR", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("E3109") && stderr.contains(configured.to_str().unwrap()), "{stderr}");
    check_snapshot("unsafe_org_policy.txt", &stderr.replace(configured.to_str().unwrap(), "ORG_POLICY"));
}

#[test]
fn lua_bind_runs_embedded_vm_and_recovers_after_hostile_calls() {
    if Command::new("lua").arg("-v").output().is_err() { return }
    let dir=isolated_cwd("lua_bind_e2e");let root=PathBuf::from(env!("CARGO_MANIFEST_DIR"));let example=root.join("examples/interop/lua");
    fs::copy(example.join("ops.lua"),dir.join("ops.lua")).unwrap();fs::copy(example.join("main.jet"),dir.join("main.jet")).unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","lua","ops.lua","--pkg","ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"Lua bind failed:\n{}",String::from_utf8_lossy(&bind.stderr));
    let cache=dir.join(".jet/bindings/lua");assert!(cache.join("libjet_lua_ops.a").is_file());let provenance=fs::read_to_string(cache.join("ops.provenance")).unwrap();assert!(provenance.contains("state=per-session\ntransport=datatree+table-view\ntable-view=zero-copy\nhook=instructions\n"));
    let generated=fs::read_to_string(cache.join("ops.jet")).unwrap();assert!(generated.contains("pub struct TableView")&&generated.contains("pub fn counters_view(session: Session, deadline_ms: Int) => TableView ? LuaError")&&generated.contains("pub fn view_get_int(view: TableView, key: String) => Int ? LuaError")&&generated.contains("pub fn view_set_int(view: TableView, key: String, value: Int) => Bool ? LuaError"));
    let run=Command::new(jet()).args(["run","--release","main.jet"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(run.status.success(),"embedded Lua binding did not run:\n{}",String::from_utf8_lossy(&run.stderr));assert_eq!(String::from_utf8_lossy(&run.stdout),fs::read_to_string(example.join("expected.out")).unwrap());
    fs::copy(root.join("tests/fixtures/lua_lifecycle.c"),dir.join("lifecycle.c")).unwrap();let lua_dir=fs::read_to_string(cache.join("ops.lua-path")).unwrap();let lua_dir=lua_dir.trim();let link_dir=format!("-L{lua_dir}");let rpath=format!("-Wl,-rpath,{lua_dir}");
    let cc=Command::new("cc").arg("lifecycle.c").args(["-L.jet/bindings/lua","-l:libjet_lua_ops.a"]).arg(link_dir).arg(rpath).args(["-llua","-lpthread","-ldl","-lm","-o","lifecycle"]).current_dir(&dir).output().unwrap();assert!(cc.status.success(),"Lua lifecycle probe link failed:\n{}",String::from_utf8_lossy(&cc.stderr));let lifecycle=Command::new(dir.join("lifecycle")).current_dir(&dir).output().unwrap();assert!(lifecycle.status.success(),"Lua lifecycle probe failed: {:?}",lifecycle.status.code());
}

#[test]
fn lua_bind_discovers_without_executing_and_launders_parse_errors() {
    if Command::new("luac").arg("-v").output().is_err() { return }
    let dir=isolated_cwd("lua_bind_static");let script=dir.join("static.lua");fs::write(&script,"error('discovery executed source')\n-- function fake(input) end\nlocal function hidden(input) return input end\nfunction visible(input) return input end\n").unwrap();
    let bind=Command::new(jet()).args(["inspect","bind","lua"]).arg(&script).args(["--pkg","static_ops"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert!(bind.status.success(),"static Lua discovery failed:\n{}",String::from_utf8_lossy(&bind.stderr));let generated=fs::read_to_string(dir.join(".jet/bindings/lua/static_ops.jet")).unwrap();assert!(generated.contains("pub fn visible("));assert!(!generated.contains("pub fn fake(")&&!generated.contains("pub fn hidden("));
    let invalid=dir.join("invalid.lua");fs::write(&invalid,"function broken(input)\n  return {\nend\n").unwrap();let failed=Command::new(jet()).args(["inspect","bind","lua"]).arg(&invalid).args(["--pkg","bad"]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert_eq!(failed.status.code(),Some(1));let stderr=String::from_utf8_lossy(&failed.stderr);assert!(stderr.contains("Error [E3208]"));assert!(!stderr.contains("invalid.lua:")&&!stderr.contains("near 'end'"),"raw Lua parser detail escaped: {stderr}");
}

#[test]
fn lua_bind_rejects_generated_fixed_abi_names() {
    if Command::new("luac").arg("-v").output().is_err() { return }
    let dir=isolated_cwd("lua_bind_reserved_helpers");for name in ["take_error","view_release"]{let script=dir.join(format!("{name}.lua"));fs::write(&script,format!("function {name}(input) return input end\n")).unwrap();let package=format!("reserved_{name}");let failed=Command::new(jet()).args(["inspect","bind","lua"]).arg(&script).args(["--pkg",&package]).current_dir(&dir).env("NO_COLOR","1").output().unwrap();assert_eq!(failed.status.code(),Some(1));let stderr=String::from_utf8_lossy(&failed.stderr);assert!(stderr.contains("Error [E3208]")&&stderr.contains(&format!("`{name}` cannot be exported")),"{stderr}");assert!(!stderr.contains("E0105"),"duplicate generated extern escaped binder validation: {stderr}");assert!(!dir.join(format!(".jet/bindings/lua/{package}.jet")).exists());}
}

#[test]
fn tasks_lists_documented_scheduled_project_tasks_and_matches_run_outside_projects() {
    let project = isolated_cwd("tasks_project");
    fs::write(
        project.join("package.jet"),
        "name: \"task_runner\"\nversion: \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("run.jet"),
        include_str!("../../examples/features/devloop/task_runner.jet"),
    )
    .unwrap();

    let listed = Command::new(jet())
        .arg("tasks")
        .current_dir(&project)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        listed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert_eq!(
        String::from_utf8(listed.stdout).unwrap(),
        "greet  Say hello from a project task\nseed   Seed local data (every 5min)\n"
    );

    let unknown = Command::new(jet())
        .args(["run", "--task", "missing", "run.jet"])
        .current_dir(&project)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let unknown_stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(!unknown.status.success(), "{unknown_stderr}");
    assert!(unknown_stderr.contains("E1294"), "{unknown_stderr}");
    assert!(
        unknown_stderr.contains("declared tasks: greet, seed"),
        "{unknown_stderr}"
    );

    let help = Command::new(jet()).arg("help").output().unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("jet tasks"),
        "jet help must list task discovery"
    );

    let outside = isolated_cwd("tasks_outside_project");
    let tasks_error = Command::new(jet())
        .arg("tasks")
        .current_dir(&outside)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let run_error = Command::new(jet())
        .arg("run")
        .current_dir(&outside)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(tasks_error.status.code(), run_error.status.code());
    assert_eq!(tasks_error.stderr, run_error.stderr);

    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/packages/monorepo");
    let workspace = isolated_cwd("tasks_workspace");
    fs::remove_dir_all(&workspace).unwrap();
    copy_dir_all(&fixture, &workspace);
    let hello = workspace.join("packages/hello/hello.jet");
    let mut hello_source = fs::read_to_string(&hello).unwrap();
    hello_source.push_str(
        "\n#[Job, Doc(\"Say hello from this workspace member\")] fn greet() {}\n",
    );
    fs::write(&hello, hello_source).unwrap();

    let ambiguous = Command::new(jet())
        .arg("tasks")
        .current_dir(&workspace)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let ambiguous_stderr = String::from_utf8_lossy(&ambiguous.stderr);
    assert_eq!(ambiguous.status.code(), Some(2), "{ambiguous_stderr}");
    assert!(
        ambiguous_stderr.contains("`jet tasks` is ambiguous"),
        "{ambiguous_stderr}"
    );
    assert!(
        ambiguous_stderr.contains("hello") && ambiguous_stderr.contains("ranker"),
        "{ambiguous_stderr}"
    );
    assert!(
        ambiguous_stderr.contains("jet tasks -p <member>")
            && !ambiguous_stderr.contains("jet run"),
        "{ambiguous_stderr}"
    );

    let selected = Command::new(jet())
        .args(["tasks", "-p", "hello"])
        .current_dir(&workspace)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        selected.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    assert_eq!(
        String::from_utf8(selected.stdout).unwrap(),
        "greet  Say hello from this workspace member\n"
    );
}

#[test]
fn project_parts_lists_skipped_explicit_and_conflicting_modules() {
    let dir = isolated_cwd("project_parts");
    fs::write(dir.join("main.jet"), "module app { }\nfn run() {}\n").unwrap();
    fs::write(dir.join("bench.jet"), "module _bench { }\n").unwrap();

    let skipped = Command::new(jet())
        .args(["project", "parts", "--skipped"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(skipped.status.code(), Some(0));
    let stdout = String::from_utf8(skipped.stdout).unwrap();
    assert!(stdout.contains("skipped") && stdout.contains("_bench"), "{stdout}");
    assert!(!stdout.contains("app"), "{stdout}");

    fs::write(
        dir.join("main.jet"),
        "use project._bench;\nmodule app { }\nfn run() {}\n",
    )
    .unwrap();
    let explicit = Command::new(jet())
        .args(["project", "parts", "--json"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(explicit.status.code(), Some(0));
    let stdout = String::from_utf8(explicit.stdout).unwrap();
    assert!(
        stdout.contains("\"name\":\"project._bench\"")
            && stdout.contains("\"state\":\"explicit\""),
        "{stdout}"
    );

    fs::write(dir.join("other.jet"), "module _bench { }\n").unwrap();
    let conflict = Command::new(jet())
        .args(["project", "parts"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&conflict.stderr);
    assert!(stderr.contains("Error [E0606]:"), "{stderr}");
    assert!(stderr.contains(" Why:"), "{stderr}");
    assert!(stderr.contains(" Fix:"), "{stderr}");
}

#[test]
#[cfg(target_os = "linux")]
fn isolated_cwd_child_holds_executable() {
    let Some(ready) = std::env::var_os("JET_CLI_EXECUTABLE_HOLDER_READY") else {
        return;
    };
    fs::write(ready, "ready").unwrap();
    let mut release = [0];
    std::io::stdin().read_exact(&mut release).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn isolated_cwd_never_reuses_executing_fixture_path() {
    let first = isolated_cwd("executing_fixture_collision");
    let executable = first.join("cli-test-holder");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let ready = first.join("ready");
    let mut child = Command::new(&executable)
        .args(["--exact", "isolated_cwd_child_holds_executable"])
        .env("JET_CLI_EXECUTABLE_HOLDER_READY", &ready)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let ready_seen = loop {
        if ready.is_file() {
            break true;
        }
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };

    let second = ready_seen.then(|| isolated_cwd("executing_fixture_collision"));
    let copy_result = second
        .as_ref()
        .map(|dir| fs::copy(std::env::current_exe().unwrap(), dir.join("cli-test-holder")));
    let release_result = child.stdin.as_mut().unwrap().write_all(&[1]);
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();

    let _ = fs::remove_dir_all(&first);
    if let Some(second) = &second {
        let _ = fs::remove_dir_all(second);
    }

    assert!(ready_seen, "holder exited or timed out: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "holder failed: {}", String::from_utf8_lossy(&output.stderr));
    release_result.unwrap();
    let second = second.unwrap();
    assert_ne!(first, second, "fixture path reused while stale executable was running");
    copy_result.unwrap().unwrap();
}

#[test]
fn budget_usage_and_preflight_fail_without_artifacts() {
    let dir = budget_project("budget_no_artifact", 10);
    for argv in [
        vec!["budget", "check", "--unknown"],
        vec!["budget", "update", "--baseline", "ci/linux", "--reason", "no gate"],
        vec!["budget", "report"],
        vec!["budget", "check", "--json", "--unknown"],
        vec!["budget", "check", "--unknown", "--json"],
        vec!["budget", "check", "--json", "--json"],
        vec!["budget", "check", "--annotations", "gitlab"],
        vec!["budget", "update", "--baseline", "CI/Linux"],
        vec!["budget", "update", "--baseline", "ci/linux", "--bootstrap", "--accept-regression", "--reason", "invalid"],
        vec!["budget", "update", "--baseline", "ci/linux", "--yes", "-y"],
    ] {
        let out = Command::new(jet()).args(argv).current_dir(&dir).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(!dir.join(".jet").exists(), "usage failure created an artifact");
    }
    fs::write(dir.join("src/main.jet"), "fn run( {\n").unwrap();
    let out = Command::new(jet()).args(["budget", "check"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.join(".jet").exists(), "compiler preflight created an artifact");
}

#[test]
fn budget_check_uses_real_compiler_fact_and_writes_verified_report() {
    let dir = budget_project("budget_check", 10);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let value = jet_foundation::PerformanceBudget::CanonicalJson::parse_canonical(&out.stdout).unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"schema\":\"jet.budget-command\""));
    assert!(text.contains("\"budget_id\":\"package:public-api\""));
    assert!(text.contains("\"num\":1"), "public API count must be measured: {text}");
    let reports = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(reports.len(), 1);
    let bytes = fs::read(reports[0].path()).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let command = match &value { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("command JSON is not an object") };
    let report = match &command["report"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("report is not an object") };
    let content = match &report["content"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("content is not an object") };
    let tool = match &content["toolchain"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("toolchain is not an object") };
    for key in ["compiler_build_id", "stdlib_id", "runner_id"] {
        let jet_foundation::PerformanceBudget::CanonicalJson::String(id) = &tool[key] else { panic!("{key} is not text") };
        assert_eq!(id.len(), 64, "{key} must identify real executable bytes");
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!matches!(id.as_str(), "jet" | "stdlib" | "compiler"));
    }
    let subject = match &content["subject"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("subject is not an object") };
    let jet_foundation::PerformanceBudget::CanonicalJson::String(triple) = &subject["target_triple"] else { panic!("target triple is not text") };
    assert!(triple.split('-').count() >= 3, "target triple must be canonical: {triple}");
    let jet_foundation::PerformanceBudget::CanonicalJson::String(measured_start) = &subject["measured_start"] else { panic!("measurement start is not text") };
    let jet_foundation::PerformanceBudget::CanonicalJson::String(measured_end) = &subject["measured_end"] else { panic!("measurement end is not text") };
    assert!(measured_start < measured_end, "measurement must cover preflight and evidence: {measured_start}..{measured_end}");
    let measurements = match &content["measurements"] { jet_foundation::PerformanceBudget::CanonicalJson::Array(value) => value, _ => panic!("measurements is not an array") };
    let measurement = match &measurements[0] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("measurement is not an object") };
    let provider = match &measurement["provider"] { jet_foundation::PerformanceBudget::CanonicalJson::Object(value) => value, _ => panic!("provider is not an object") };
    for key in ["cpu_model", "kernel", "power_governor"] {
        let jet_foundation::PerformanceBudget::CanonicalJson::String(value) = &provider[key] else { panic!("{key} is not text") };
        assert!(!value.is_empty() && !matches!(value.as_str(), "compiler" | "unknown"));
    }
}

#[test]
fn budget_build_artifact_measures_real_selected_binary() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = artifact_budget_project("budget_build_artifact", 100_000_000);
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject object") };
    let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("artifact identity") };
    let CanonicalJson::Integer(bytes) = &artifact["bytes"] else { panic!("artifact byte count") };
    let CanonicalJson::String(digest) = &artifact["sha256"] else { panic!("artifact digest") };
    let artifact_path = dir.join("build/main");
    let metadata = fs::metadata(&artifact_path).unwrap();
    assert_eq!(bytes, &metadata.len().to_string());
    assert_eq!(digest, &jet::SHA256::sha256_file_hex(&artifact_path).unwrap());
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    let CanonicalJson::Object(measurement) = &measurements[0] else { panic!("measurement") };
    let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
    let CanonicalJson::Object(sample) = &samples[0] else { panic!("sample") };
    assert_eq!(sample["num"], CanonicalJson::Integer(metadata.len().to_string()));
    let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
    assert_eq!(provider["kind"], CanonicalJson::String("BuildArtifact".into()));
    assert_eq!(measurement["unit"], CanonicalJson::String("Bytes".into()));
}

#[test]
fn budget_report_collects_mixed_providers_measurement_locally() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = mixed_budget_project("budget_mixed_providers");
    let out = Command::new(jet()).args(["budget", "check", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let CanonicalJson::Object(command) = CanonicalJson::parse_canonical(&out.stdout).unwrap() else { panic!("command object") };
    let CanonicalJson::Object(report) = &command["report"] else { panic!("report object") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content object") };
    let CanonicalJson::Array(measurements) = &content["measurements"] else { panic!("measurements") };
    assert_eq!(measurements.len(), 2);
    let mut providers = std::collections::BTreeMap::new();
    for measurement in measurements {
        let CanonicalJson::Object(measurement) = measurement else { panic!("measurement object") };
        let CanonicalJson::String(id) = &measurement["budget_id"] else { panic!("budget id") };
        let CanonicalJson::Object(provider) = &measurement["provider"] else { panic!("provider") };
        let CanonicalJson::String(kind) = &provider["kind"] else { panic!("provider kind") };
        let CanonicalJson::Array(samples) = &measurement["samples"] else { panic!("samples") };
        assert_eq!(samples.len(), 1, "{id} must own its provider sample");
        providers.insert(id.clone(), kind.clone());
    }
    assert_eq!(providers.get("package:binary").map(String::as_str), Some("BuildArtifact"));
    assert_eq!(providers.get("package:public-api").map(String::as_str), Some("CompilerFacts"));
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
    let CanonicalJson::Object(artifact) = &subject["artifact"] else { panic!("shared artifact provenance") };
    let CanonicalJson::Integer(bytes) = &artifact["bytes"] else { panic!("artifact bytes") };
    assert_eq!(bytes, &fs::metadata(dir.join("build/main")).unwrap().len().to_string());
    let report_path = fs::read_dir(dir.join(".jet/perf/reports")).unwrap().next().unwrap().unwrap().path();
    jet_foundation::PerformanceBudget::verify_budget_report(&fs::read(report_path).unwrap()).unwrap();
}

#[test]
fn build_enforces_deterministic_fail_budgets_and_reuses_relevant_identity() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = mixed_budget_project("build_budget_gates");
    let source_path = dir.join("src/main.jet");
    let passing = fs::read_to_string(&source_path).unwrap();
    let failing = passing.replace(".AtMost(10)", ".AtMost(0)");
    fs::write(&source_path, &failing).unwrap();

    let failed = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(failed.status.code(), Some(1), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&failed.stdout), String::from_utf8_lossy(&failed.stderr));
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("built:"), "failed budget claimed build success");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Error [E2907]: performance budget public-api regressed"), "{}", String::from_utf8_lossy(&failed.stderr));
    let report_dir = dir.join(".jet/perf/reports");
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 1);

    fs::write(&source_path, &passing).unwrap();
    let passed = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(passed.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&passed.stdout), String::from_utf8_lossy(&passed.stderr));
    assert!(String::from_utf8_lossy(&passed.stderr).contains("budgets: 2 budgets passed · report "));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 2, "source/spec change must refresh evidence");

    let reused = Command::new(jet()).args(["build", "src/main.jet"]).current_dir(&dir).output().unwrap();
    assert_eq!(reused.status.code(), Some(0), "{}", String::from_utf8_lossy(&reused.stderr));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 2, "unchanged relevant identity must reuse canonical report");

    let ci = Command::new(jet()).args(["build", "src/main.jet", "--profile=ci"]).current_dir(&dir).output().unwrap();
    assert_eq!(ci.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&ci.stdout), String::from_utf8_lossy(&ci.stderr));
    assert_eq!(fs::read_dir(&report_dir).unwrap().count(), 3, "CI profile identity must refresh evidence");
    let mut profiles = Vec::new();
    for entry in fs::read_dir(&report_dir).unwrap() {
        let value = CanonicalJson::parse_canonical(&fs::read(entry.unwrap().path()).unwrap()).unwrap();
        let CanonicalJson::Object(report) = value else { panic!("report") };
        let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
        let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
        let CanonicalJson::String(profile) = &subject["profile"] else { panic!("profile") };
        profiles.push(profile.clone());
    }
    assert!(profiles.iter().any(|profile| profile == "dev"));
    assert!(profiles.iter().any(|profile| profile == "ci"));
}

#[test]
#[cfg(target_os = "linux")]
fn perf_report_reuse_ignores_nonsemantic_compiler_bytes_under_parallel_load() {
    let bin_dir = isolated_cwd("perf_report_compiler_identity");
    let compiler = bin_dir.join("jet-semantic-a");
    let padded_compiler = bin_dir.join("jet-semantic-b");
    fs::copy(jet(), &compiler).unwrap();
    fs::copy(jet(), &padded_compiler).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&padded_compiler)
        .unwrap()
        .write_all(b"nonsemantic linker padding")
        .unwrap();
    assert_ne!(
        jet::SHA256::sha256_file_hex(&compiler).unwrap(),
        jet::SHA256::sha256_file_hex(&padded_compiler).unwrap(),
        "controlled compiler copies must have different file bytes",
    );

    let workspace = benchmark_budget_project("perf_report_compiler_identity");
    let seeded = Command::new(&compiler)
        .args(["bench", "src/main.jet"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert_eq!(
        seeded.status.code(), Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&seeded.stdout),
        String::from_utf8_lossy(&seeded.stderr),
    );
    assert_eq!(fs::read_dir(workspace.join(".jet/perf/reports")).unwrap().count(), 1);

    let start = std::sync::Barrier::new(3);
    let outputs = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            Command::new(&compiler)
                .args(["bench", "src/main.jet"])
                .current_dir(&workspace)
                .output()
                .unwrap()
        });
        let second = scope.spawn(|| {
            start.wait();
            Command::new(&padded_compiler)
                .args(["bench", "src/main.jet"])
                .current_dir(&workspace)
                .output()
                .unwrap()
        });
        start.wait();
        [first.join().unwrap(), second.join().unwrap()]
    });

    for output in outputs {
        assert_eq!(
            output.status.code(), Some(0),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("ns/iter"), "compatible report reran benchmark workload");
    }
    assert_eq!(
        fs::read_dir(workspace.join(".jet/perf/reports")).unwrap().count(),
        1,
        "nonsemantic compiler bytes must not invalidate compatible report identity",
    );
}

#[test]
fn budget_bench_measurement_bootstraps_then_consumes_compatible_history() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("budget_bench_measurement");
    let bootstrap = Command::new(jet()).args(["budget","update","--baseline","ci/linux","--bootstrap","--reason","initial benchmark","--yes","--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(bootstrap.status.code(),Some(0),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&bootstrap.stdout),String::from_utf8_lossy(&bootstrap.stderr));
    let CanonicalJson::Object(first)=CanonicalJson::parse_canonical(&bootstrap.stdout).unwrap() else{panic!("command")};
    let CanonicalJson::Object(report)=&first["report"] else{panic!("report")};let CanonicalJson::Object(content)=&report["content"] else{panic!("content")};let CanonicalJson::Array(measurements)=&content["measurements"] else{panic!("measurements")};let CanonicalJson::Object(measurement)=&measurements[0] else{panic!("measurement")};
    let CanonicalJson::Array(samples)=&measurement["samples"] else{panic!("samples")};assert_eq!(samples.len(),20);assert!(matches!(measurement["statistics"],CanonicalJson::Object(_)));assert!(matches!(measurement["policy"],CanonicalJson::Object(_)));assert_eq!(measurement["history"],CanonicalJson::Null);assert_eq!(measurement["baseline"],CanonicalJson::Null);
    let CanonicalJson::Object(provider)=&measurement["provider"] else{panic!("provider")};assert_eq!(provider["kind"],CanonicalJson::String("BenchMeasurement".into()));assert_eq!(provider["identity"],CanonicalJson::String("parse".into()));
    let first_id=match &report["report_id"]{CanonicalJson::String(value)=>value.clone(),_=>panic!("report id")};

    let check=Command::new(jet()).args(["budget","check","--json"]).current_dir(&dir).output().unwrap();
    assert!(matches!(check.status.code(),Some(0)|Some(1)),"stdout: {}\nstderr: {}",String::from_utf8_lossy(&check.stdout),String::from_utf8_lossy(&check.stderr));
    let CanonicalJson::Object(second)=CanonicalJson::parse_canonical(&check.stdout).unwrap() else{panic!("command")};
    let CanonicalJson::Object(report)=&second["report"] else{panic!("report")};let CanonicalJson::Object(content)=&report["content"] else{panic!("content")};let CanonicalJson::Array(measurements)=&content["measurements"] else{panic!("measurements")};let CanonicalJson::Object(measurement)=&measurements[0] else{panic!("measurement")};let CanonicalJson::Object(history)=&measurement["history"] else{panic!("history")};let CanonicalJson::Array(ids)=&history["report_ids"] else{panic!("ids")};assert_eq!(ids, &vec![CanonicalJson::String(first_id.clone())]);let CanonicalJson::Object(baseline)=&measurement["baseline"] else{panic!("baseline")};let CanonicalJson::Array(pooled)=&baseline["pooled_samples"] else{panic!("pooled")};assert_eq!(pooled.len(),20);let CanonicalJson::Object(decision)=&measurement["decision"] else{panic!("decision")};assert_ne!(decision["evidence"],CanonicalJson::String("unavailable".into()));
    let CanonicalJson::Array(results)=&second["results"] else{panic!("results")};let CanonicalJson::Object(result)=&results[0] else{panic!("result")};
    assert_eq!(result["baseline_report_ids"],CanonicalJson::Array(vec![CanonicalJson::String(first_id)]));
    assert_eq!(result["metric"],measurement["metric"]);
    assert_eq!(result["lower95"],decision["lower95"]);assert_eq!(result["upper95"],decision["upper95"]);assert_eq!(result["trend"],decision["trend"]);assert_eq!(result["reason"],decision["reason"]);
}

#[test]
fn bench_owns_canonical_refresh_and_dossier_only_projects_it() {
    use jet_foundation::PerformanceBudget::CanonicalJson;
    let dir = benchmark_budget_project("bench_owned_budget_refresh");
    let run = || Command::new(jet()).args(["bench", "src/main.jet"]).current_dir(&dir).output().unwrap();
    let first = run();
    assert_eq!(first.status.code(), Some(0), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&first.stdout), String::from_utf8_lossy(&first.stderr));
    assert!(String::from_utf8_lossy(&first.stderr).contains("report "));
    let reports = dir.join(".jet/perf/reports");
    let report_paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = report_paths();
    assert_eq!(initial.len(), 1);
    let bytes = fs::read(&initial[0]).unwrap();
    jet_foundation::PerformanceBudget::verify_budget_report(&bytes).unwrap();
    let CanonicalJson::Object(report) = CanonicalJson::parse_canonical(&bytes).unwrap() else { panic!("report") };
    let CanonicalJson::Object(content) = &report["content"] else { panic!("content") };
    let CanonicalJson::Object(subject) = &content["subject"] else { panic!("subject") };
    assert_eq!(subject["profile"], CanonicalJson::String("bench".into()));

    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&second.stdout).contains("ns/iter"), "unchanged relevant identity reran measurement harness");
    assert_eq!(report_paths(), initial, "unchanged relevant identity must reuse report");

    let before = fs::read_dir(&reports).unwrap().map(|entry| { let path=entry.unwrap().path();(path.clone(),fs::metadata(&path).unwrap().modified().unwrap()) }).collect::<Vec<_>>();
    let dossier = Command::new(jet()).args(["inspect", "dossier", "src/main.jet", "run", "--json"]).current_dir(&dir).output().unwrap();
    assert_eq!(dossier.status.code(), Some(0), "{}", String::from_utf8_lossy(&dossier.stderr));
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(dossier.contains("\"performance_budgets\":{\"mode\":\"read_only\""), "{dossier}");
    assert!(dossier.contains("\"budget_id\":\"package:parse\""), "{dossier}");
    let after = fs::read_dir(&reports).unwrap().map(|entry| { let path=entry.unwrap().path();(path.clone(),fs::metadata(&path).unwrap().modified().unwrap()) }).collect::<Vec<_>>();
    assert_eq!(before, after, "dossier projection must not rewrite reports");

    fs::OpenOptions::new().append(true).open(dir.join("src/main.jet")).unwrap().write_all(b"\n// relevant source digest change\n").unwrap();
    let third = run();
    assert_eq!(third.status.code(), Some(0), "{}", String::from_utf8_lossy(&third.stderr));
    assert_eq!(report_paths().len(), 2, "source digest change must refresh canonical report");
}
