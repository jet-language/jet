mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

/// Card #2250 / I9: one invocation argv and its user-argument projection must
/// survive every native execution adapter. The first item is the tier's
/// program identity, so the observable comparison covers the forwarded
/// program arguments while the same source proves flags, filenames, Unicode,
/// an empty value and separator, a value containing --, a bare --, and
/// --release survive the CLI split unchanged.
#[test]
fn argv_agrees_on_every_native_tier() {
    let dir = std::env::temp_dir().join(format!("jet_run_tiers_argv_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_tiers_argv\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [Exec, IO, Mem.Alloc] } }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.process as process\n\nfn run() {\n    args :: process.argv()\n    user_args :: process.args()\n    print(args.len())\n    print(user_args.len())\n    loop arg in user_args {\n        print(\"<{arg}>\")\n    }\n}\n",
    )
    .unwrap();

    let program_args = [
        "--flag",
        "--port=50000",
        "report file.jet",
        "Δ",
        "",
        "pre--post",
        "--",
        "--release",
    ];
    let cache = dir.join("cache");
    let run = |label: &str, args: &[&str]| {
        let output = Command::new(jet())
            .args(args)
            .current_dir(&dir)
            .env("JET_STORE_DIR", cache.join(label).join("run"))
            .env("JET_STORE_DIR", cache.join(label).join("build"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|error| panic!("{label} should start: {error}"));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{label} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{label} wrote stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };

    let invocation = |label: &str, command: &str, extra: &[&str]| {
        let mut args = vec![command];
        args.extend_from_slice(extra);
        args.push("main.jet");
        args.push("--");
        args.extend_from_slice(&program_args);
        run(label, &args)
    };
    let empty_invocation = |label: &str, command: &str, extra: &[&str]| {
        let mut args = vec![command];
        args.extend_from_slice(extra);
        args.push("main.jet");
        args.push("--");
        run(label, &args)
    };
    let default = invocation("default", "run", &[]);
    let release = invocation("release", "run", &["--release"]);
    let dev = {
        let mut args = vec!["dev", "--watch=off", "main.jet", "--"];
        args.extend_from_slice(&program_args);
        run("dev", &args)
    };
    let empty_default = empty_invocation("empty-default", "run", &[]);
    let empty_release = empty_invocation("empty-release", "run", &["--release"]);
    let empty_dev = empty_invocation("empty-dev", "dev", &["--watch=off"]);

    let build = Command::new(jet())
        .args(["build", "main.jet", "--quiet"])
        .current_dir(&dir)
        .env("JET_STORE_DIR", cache.join("aot-build"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        build.status.code(),
        Some(0),
        "AOT build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let aot_run = |label: &str, args: &[&str]| {
        let output = Command::new(dir.join(".jet/build/main"))
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{label} binary failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{label} binary wrote stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    let aot = aot_run("AOT", &program_args);
    let empty_aot = aot_run("AOT empty", &[]);

    for (label, output) in [
        ("release", release),
        ("dev", dev),
        ("AOT", aot),
    ] {
        assert_eq!(output, default, "{label} argv output diverged");
    }
    let expected =
        "9\n8\n<--flag>\n<--port=50000>\n<report file.jet>\n<Δ>\n<>\n<pre--post>\n<-->\n<--release>\n";
    assert_eq!(
        default.as_slice(),
        expected.as_bytes(),
        "forwarded argv was not preserved"
    );

    for (label, output) in [
        ("empty default", empty_default),
        ("empty release", empty_release),
        ("empty dev", empty_dev),
        ("empty AOT", empty_aot),
    ] {
        assert_eq!(output, b"1\n0\n", "{label} argv output diverged");
    }

    let _ = fs::remove_dir_all(&dir);
}

/// An undrained `core.db` pool is released when the run ends, while the
/// native connection table still exists. Before, in-process tiers printed an
/// internal error from thread-local teardown after the program had finished
/// (exit code 0); AOT never did.
#[test]
fn undrained_db_pool_exits_cleanly() {
    let dir = std::env::temp_dir().join(format!("jet_run_tiers_db_pool_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_tiers_db_pool\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [DB, IO, Mem.Alloc, Time.Wait] } }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.db as db\n\nfn run() {\n    pool :: db.pool(\":memory:\", max: 1) ?? return\n    ready :: pool.ready() ?? false\n    print(\"ready {ready}\")\n}\n",
    )
    .unwrap();

    for label in ["default", "release"] {
        let output = Command::new(jet())
            .arg("run")
            .args((label == "release").then_some("--release"))
            .arg("main.jet")
            .current_dir(&dir)
            .env("JET_STORE_DIR", dir.join("cache").join(label))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{label} failed: {stderr}");
        assert_eq!(output.stdout, b"ready true\n", "{label} stdout");
        assert!(stderr.is_empty(), "{label} wrote stderr at exit: {stderr}");
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unused_c_member_lists_stay_runnable() {
    let dir = std::env::temp_dir().join(format!("jet_run_tiers_imports_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_tiers_imports\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [IO] } }\ndeps: { c: c@system, m: c@system }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "use core.math.[abs, min]\nuse core.encoding.[json, csv]\nuse c.[c as libc, m]\nfn run() {\n    print(abs(-8))\n    print(min(9, 4))\n}\n",
    )
    .unwrap();

    let output = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("JET_STORE_DIR", dir.join("cache-default"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"8\n4\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn c_extern_calls_match_aot() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = root.join("Examples/features/lowlevel/cbind");
    let cache = std::env::temp_dir().join(format!("jet_c_extern_parity_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);

    let run = |mode: &str| {
        let mut command = Command::new(jet());
        command.arg("run");
        if mode == "release" {
            command.arg("--release");
        }
        command
            .arg("run.jet")
            .current_dir(&project)
            .env("JET_STORE_DIR", cache.join(mode).join("run"))
            .env("JET_STORE_DIR", cache.join(mode).join("build"))
            .env("NO_COLOR", "1")
            .env("JETPACK_ENV", "1")
            .output()
            .unwrap()
    };

    let release = run("release");
    let default = run("default");
    for (mode, output) in [
        ("release", &release),
        ("default", &default),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{mode} C extern run failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"10\n7\n", "{mode} C extern output");
        assert!(
            output.stderr.is_empty(),
            "{mode} C extern stderr: {:?}",
            output.stderr
        );
    }

    let _ = fs::remove_dir_all(&cache);
}

/// Cards #2014/#2015 (I9): an example AOT completes must complete identically
/// on default `jet run` — same stdout, same stderr, same exit code.
fn assert_example_tier_parity(tag: &str, example: &str, golden: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected = fs::read_to_string(root.join(golden)).expect("example golden output");
    let cache = std::env::temp_dir().join(format!("jet_tier_parity_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);

    let run = |mode: &str| {
        let mut command = Command::new(jet());
        command.arg("run");
        if mode == "release" {
            command.arg("--release");
        }
        command
            .arg(example)
            .current_dir(&root)
            .env("JET_STORE_DIR", cache.join(mode).join("build"))
            .env("NO_COLOR", "1")
            .output()
            .expect("jet run")
    };

    // Byte-exact differential: stderr carries only what the program itself
    // wrote and both tiers must agree on all of it.
    for mode in ["release", "default"] {
        let output = run(mode);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{tag}: `{mode}` exited nonzero:\n{stderr}"
        );
        assert_eq!(stderr, "", "{tag}: `{mode}` wrote to stderr");
        assert_eq!(
            stdout, expected,
            "{tag}: `{mode}` stdout diverged from the golden"
        );
    }

    let _ = fs::remove_dir_all(&cache);
}

/// Card #2014: `task.all` over children that borrow disjoint places of an owner
/// list. The evaluator once failed the join outright because the child
/// thread could not name the owner local behind the write window.
#[test]
fn scoped_borrow_bands_agrees_on_every_tier() {
    assert_example_tier_parity(
        "scoped_borrow_bands",
        "Examples/features/concurrency/scoped_borrow_bands.jet",
        "Examples/features/expected/concurrency/scoped_borrow_bands.out",
    );
}

/// Card #2015: `data.json<T>` into a user struct. The evaluator once
/// answered from a stub instead of the typed decode AOT calls.
#[test]
fn data_json_agrees_on_every_tier() {
    assert_example_tier_parity(
        "data_json",
        "Examples/features/tooling/data_json.jet",
        "Examples/features/expected/tooling/data_json.out",
    );
}

/// Card #2816 (I9): a spawned task blocked in a Core UDP receive must make
/// progress while its parent waits in a Core DNS call. An in-process tier
/// once held the shared program-output sink lock across the parent's DNS
/// wait, so the responder could not even enter `udp_receive` until the
/// parent's deadline expired — and the parent then panicked on the timeout.
/// The DNS deadline inside the program is the pass criterion; the process
/// bound only turns a wedged run into a failure instead of a hang.
#[test]
fn udp_responder_progresses_under_parent_wait() {
    let dir = std::env::temp_dir().join(format!(
        "jet_run_tiers_udp_responder_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.jet"),
        "name: \"run_tiers_udp_responder\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [Exec, IO, Mem.Alloc, Net, Panic] } }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        r#"use core.net as net

fn run() {
    server :: net.udp_bind("127.0.0.1:0") ?? panic("DNS server")
    address :: net.socket_to_string(net.udp_local_addr(server) ?? panic("server address"))
    responder :: task {
        request :: net.udp_receive(server, 512) ?? panic("DNS query")
        query :: net.udp_packet_bytes(request)
        response :: [U8]{
            query[0], query[1], 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0,
            7, 101, 120, 97, 109, 112, 108, 101, 4, 116, 101, 115, 116, 0,
            0, 16, 0, 1,
            0xc0, 0x0c, 0, 16, 0, 1, 0, 0, 0, 60, 0, 4,
            3, 106, 101, 116
        }
        net.udp_send_bytes_to(server, response, net.udp_packet_addr(request)) ?? panic("DNS response")
        print("responder answered")
    }
    result :: net.dns_txt_at(address, "example.test", 1000) ?? panic("DNS TXT")
    responder.join() ?? panic("DNS responder")
    print(result.len() == 1 && result[0] == "jet")
}
"#,
    )
    .unwrap();

    let mut child = Command::new(jet())
        .args(["run", "main.jet"])
        .current_dir(&dir)
        .env("JET_STORE_DIR", dir.join("run-cache"))
        .env("JET_STORE_DIR", dir.join("store"))
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let bound = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= bound {
            let _ = child.kill();
            let _ = child.wait();
            panic!("default `jet run` wedged: the UDP responder task never progressed");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stdout, "responder answered\ntrue\n", "stderr:\n{stderr}");

    let _ = fs::remove_dir_all(&dir);
}
