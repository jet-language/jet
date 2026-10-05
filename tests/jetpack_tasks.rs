//! D-JET-VERBS1=A: `#Job fn`s run as `jet <name>`.

use std::fs;
use std::process::Command;

mod common;
use common::Scratch;

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}

fn write_main(dir: &std::path::Path, src: &str) {
    fs::write(dir.join("main.jet"), src).unwrap();
    // The jobs print, so the fixture package decides IO authority up front.
    fs::write(
        dir.join("package.jet"),
        "name: \"test-fixture\"\nversion: \"0.1.0\"\nauthority: {\n    holds: {\n        allow: [IO, Mem.Alloc]\n    }\n}\n",
    )
    .unwrap();
}

#[test]
fn jet_run_job_invokes_marked_fn() {
    let scratch = Scratch::new("jet-job");
    write_main(
        &scratch.path,
        r#"
#Job
fn greet() {
        print("hello-job")
}
fn run() { print("run-entry") }
"#,
    );
    let entry = scratch.path.join("main.jet");
    let out = jet()
        .args(["run", entry.to_str().unwrap(), "--", "greet"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello-job"), "stdout: {stdout}");
    assert!(
        !stdout.contains("run-entry"),
        "must not run fn run when a job subcommand is set: {stdout}"
    );
}

#[test]
fn jet_top_job_runs_by_name_and_unknown_suggests_jobs() {
    // Commands then parsed project jobs; an unknown word suggests both.
    let scratch = Scratch::new("jet-jobs");
    write_main(
        &scratch.path,
        r#"
#Job
fn greet() {
    print("from-jobs")
}
#Job
fn seed_data() {
    print("seeded")
}
fn run() {}
"#,
    );
    fs::rename(scratch.path.join("main.jet"), scratch.path.join("run.jet")).unwrap();

    let ok = jet()
        .args(["--no-color", "greet"])
        .current_dir(&scratch.path)
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ok.stdout).contains("from-jobs"),
        "stdout: {}",
        String::from_utf8_lossy(&ok.stdout)
    );

    let bad = jet()
        .args(["--no-color", "seed_dat"])
        .current_dir(&scratch.path)
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(stderr.contains("E2101"), "stderr: {stderr}");
    assert!(stderr.contains("Did you mean `jet seed_data`"), "stderr: {stderr}");
}

#[test]
fn jet_run_job_leaf_stays_callable_from_sibling() {
    // D-JPK-TASKRUN1: dependency = plain call. Selecting leaf `greet` as
    // entry must not rename it away — sibling `seed`'s `greet()` must not
    // die with E0102, and both jobs must run.
    let scratch = Scratch::new("job-dep");
    write_main(
        &scratch.path,
        r#"
#Job
fn greet() {
    print("leaf-ok")
}
#Job
fn seed_data() {
    greet()
    print("dep-ok")
}
fn run() { print("run-entry") }
"#,
    );
    let entry = scratch.path.join("main.jet");
    let path = entry.to_str().unwrap();

    let greet = jet().args(["run", path, "--", "greet"]).output().unwrap();
    assert!(
        greet.status.success(),
        "leaf job subcommand must not E0102; stderr: {}",
        String::from_utf8_lossy(&greet.stderr)
    );
    let greet_out = String::from_utf8_lossy(&greet.stdout);
    assert!(greet_out.contains("leaf-ok"), "stdout: {greet_out}");
    assert!(
        !String::from_utf8_lossy(&greet.stderr).contains("E0102"),
        "stderr: {}",
        String::from_utf8_lossy(&greet.stderr)
    );

    let seed = jet()
        .args(["run", path, "--", "seed_data"])
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&seed.stderr)
    );
    let seed_out = String::from_utf8_lossy(&seed.stdout);
    assert!(
        seed_out.contains("leaf-ok") && seed_out.contains("dep-ok"),
        "stdout: {seed_out}"
    );
}

#[test]
fn jet_run_job_typed_cli_args() {
    let scratch = Scratch::new("job-cli");
    write_main(
        &scratch.path,
        r#"
#CLI
struct MigrateArgs {
    #Doc("target") to: String{"latest"}
}
#Job
fn migrate(args: MigrateArgs) {
    print(args.to)
}
fn run() {}
"#,
    );
    let entry = scratch.path.join("main.jet");
    let out = jet()
        .args([
            "run",
            entry.to_str().unwrap(),
            "--",
            "migrate",
            "--to",
            "004",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "004");
}

#[test]
fn jet_jobs_after_runs_predecessors_and_scalar_args() {
    // D-DX-JOBGRAPH1=A / I9: the default tier runs `after:` predecessors
    // through the Prelude job graph, and scalar job parameters are flags.
    let scratch = Scratch::new("job-graph");
    write_main(
        &scratch.path,
        r#"
#Job
fn migrate(to: String{"latest"}) {
    print("migrate {to}")
}
#Job(after: ["migrate"])
fn populate(label: String{"dev"}) {
    print("populate {label}")
}
fn run() {}
"#,
    );
    let entry = scratch.path.join("main.jet");
    let path = entry.to_str().unwrap();

    let populate = jet()
        .args(["run", path, "--", "populate", "--label", "ci"])
        .output()
        .unwrap();
    assert!(
        populate.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&populate.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&populate.stdout).trim(),
        "migrate latest\npopulate ci"
    );

    let migrate = jet()
        .args(["run", path, "--", "migrate", "--to", "004"])
        .output()
        .unwrap();
    assert!(
        migrate.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&migrate.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&migrate.stdout).trim(), "migrate 004");

    // Root arguments are checked before any predecessor runs.
    let bad = jet()
        .args(["run", path, "--", "populate", "--bogus"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert!(
        !String::from_utf8_lossy(&bad.stdout).contains("migrate"),
        "stdout: {}",
        String::from_utf8_lossy(&bad.stdout)
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("E1331"),
        "stderr: {}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

/// The `after:` + scalar-parameter fixture shared by the job-argv and AOT
/// parity tests, written as the package's `run.jet` role file.
fn write_job_graph_package(dir: &std::path::Path) {
    write_main(
        dir,
        r#"
#Job
fn migrate(to: String{"latest"}) {
    print("migrate {to}")
}
#Job(after: ["migrate"])
fn populate(label: String{"dev"}) {
    print("populate {label}")
}
#Job
fn web(port: Int{8080}, host: String{"127.0.0.1"}) {
    print("web host={host} port={port}")
}
#Job
fn broken() {
    panic("broken job")
}
#Job(after: ["broken"])
fn needs_broken() {
    print("needs_broken ran")
}
fn run() {}
"#,
    );
    fs::write(
        dir.join("package.jet"),
        "name: \"test-fixture\"\nversion: \"0.1.0\"\nauthority: {\n    holds: {\n        allow: [IO, Mem.Alloc, Panic]\n    }\n}\n",
    )
    .unwrap();
    fs::rename(dir.join("main.jet"), dir.join("run.jet")).unwrap();
}

#[test]
fn jet_top_jobs_forward_flags_after_name() {
    // D-JOB-ARGV1=A: `jet <name> …` gives the job every word after its
    // name; Jet's own flags go before the name; `--` after the name is the
    // same separator and still works.
    let scratch = Scratch::new("job-argv");
    write_job_graph_package(&scratch.path);
    let jobs = |args: &[&str]| {
        jet()
            .args(args)
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };

    for (args, expected) in [
        (&["web", "--port", "3000"][..], "web host=127.0.0.1 port=3000"),
        (&["web", "--", "--port", "3000"][..], "web host=127.0.0.1 port=3000"),
        (
            &["--interpret", "web", "--port", "3000", "--host", "0.0.0.0"][..],
            "web host=0.0.0.0 port=3000",
        ),
        (&["populate", "--label", "ci"][..], "migrate latest\npopulate ci"),
    ] {
        let out = jobs(args);
        assert!(
            out.status.success(),
            "jet {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            expected,
            "jet {args:?}"
        );
    }

    // A Jet flag after the name belongs to the job, whose argument check
    // rejects it before any predecessor runs.
    let forwarded = jobs(&["populate", "--json"]);
    assert_eq!(forwarded.status.code(), Some(2));
    let forwarded_stderr = String::from_utf8_lossy(&forwarded.stderr);
    assert!(forwarded_stderr.contains("E1331"), "{forwarded_stderr}");
    assert!(
        !String::from_utf8_lossy(&forwarded.stdout).contains("migrate"),
        "{forwarded_stderr}"
    );

    // A job flag before the name is still Jet's, and the fix says where the
    // job's flags go.
    let misplaced = jobs(&["--port", "3000", "web"]);
    assert_eq!(misplaced.status.code(), Some(2));
    let misplaced_stderr = String::from_utf8_lossy(&misplaced.stderr);
    assert!(
        misplaced_stderr.contains("E2102") && misplaced_stderr.contains("after the job name"),
        "{misplaced_stderr}"
    );
}

#[test]
fn jet_build_job_dispatch_matches_default_and_interpreter() {
    // I9: the built binary dispatches `<binary> <job> <args…>` through the
    // same Prelude job graph as `jet <name>` and `jet run --interpret --`.
    if !common::have_rustc() {
        return;
    }
    let scratch = Scratch::new("job-aot");
    write_job_graph_package(&scratch.path);
    let build = jet()
        .args(["build", "run.jet"])
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "jet build: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let binary = scratch.path.join(".jet/build/run");

    for args in [
        &["populate", "--label", "ci"][..],
        &["migrate", "--to", "004"][..],
        &["web", "--port", "3000"][..],
        &["needs_broken"][..],
        &["populate", "--bogus"][..],
    ] {
        let aot = Command::new(&binary)
            .args(args)
            .current_dir(&scratch.path)
            .output()
            .unwrap();
        let jobs = jet()
            .args(args)
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let interpreted = jet()
            .args(["run", "--interpret", "run.jet", "--"])
            .args(args)
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let aot_stdout = String::from_utf8_lossy(&aot.stdout);
        for (tier, out) in [("jet <name>", &jobs), ("jet run --interpret", &interpreted)] {
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                aot_stdout,
                "{tier} {args:?} stdout differs from the built binary; stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                out.status.success(),
                aot.status.success(),
                "{tier} {args:?}: aot stderr: {}\n{tier} stderr: {}",
                String::from_utf8_lossy(&aot.stderr),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        match args {
            ["populate", "--label", "ci"] => assert_eq!(aot_stdout.trim(), "migrate latest\npopulate ci"),
            ["migrate", "--to", "004"] => assert_eq!(aot_stdout.trim(), "migrate 004"),
            ["web", "--port", "3000"] => assert_eq!(aot_stdout.trim(), "web host=127.0.0.1 port=3000"),
            // A failing predecessor stops its dependent.
            ["needs_broken"] => {
                assert!(!aot.status.success());
                assert!(!aot_stdout.contains("needs_broken ran"), "{aot_stdout}");
            }
            // A bad root argument fails before any predecessor runs.
            _ => {
                for out in [&aot, &jobs, &interpreted] {
                    assert_eq!(out.status.code(), Some(2));
                    assert!(
                        String::from_utf8_lossy(&out.stderr).contains("E1331"),
                        "{}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                assert!(!aot_stdout.contains("migrate"), "{aot_stdout}");
            }
        }
    }
}

#[test]
fn top_level_job_example_uses_one_argv_meaning() {
    let scratch = Scratch::new("top-level-job");
    // The example carries its own inline package block and authority, so the
    // scratch root's default package.jet would be a second manifest (E1363).
    fs::remove_file(scratch.path.join("package.jet")).unwrap();
    fs::write(
        scratch.path.join("run.jet"),
        include_str!("../Examples/features/script_job/top_level/run.jet"),
    )
    .unwrap();
    let expected = include_str!("../Examples/features/expected/script_job/top_level.out");
    for args in [
        &["deploy", "--region", "eu"][..],
        &["--interpret", "deploy", "--region", "eu"][..],
        &["run", "run.jet", "--", "deploy", "--region", "eu"][..],
    ] {
        let output = jet().args(args).current_dir(&scratch.path).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
    let listed = jet().arg("jobs").current_dir(&scratch.path).output().unwrap();
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("deploy"));
    let help = jet().args(["help", "jobs"]).current_dir(&scratch.path).output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("jet [jet flags] <name> [job args...]"));
    let retired = jet().args(["jobs", "deploy"]).current_dir(&scratch.path).output().unwrap();
    assert_eq!(retired.status.code(), Some(2));
    let report = String::from_utf8_lossy(&retired.stderr);
    assert!(report.contains("E2101") && report.contains("Run `jet deploy`"), "{report}");
}

#[cfg(unix)]
#[test]
fn top_level_jobs_precede_retired_words_paths_and_path_plugins() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("job-lookup-order");
    write_main(&scratch.path, r#"
#Job fn deploy() { print("job") }
#Job fn serve() { print("retired-word-job") }
#Job fn output() { print("moved-word-job") }
#Job(.Internal) fn secret() { print("internal") }
fn run() { print("entry") }
"#);
    fs::rename(scratch.path.join("main.jet"), scratch.path.join("run.jet")).unwrap();
    // Every root `.jet` file belongs to the package (one `fn run`, D-CMDOVERRIDE1),
    // so `deploy.jet` is a plain member and the `deploy/` folder is its own
    // package; a job still beats both names.
    fs::create_dir(scratch.path.join("deploy")).unwrap();
    fs::write(
        scratch.path.join("deploy/package.jet"),
        "name: \"deploy-path\"\nversion: \"0.1.0\"\nauthority: {\n    holds: {\n        allow: [IO, Mem.Alloc]\n    }\n}\n",
    )
    .unwrap();
    fs::write(scratch.path.join("deploy/run.jet"), "fn run() { print(\"path\") }\n").unwrap();
    fs::write(scratch.path.join("deploy.jet"), "fn deploy_note() -> String { \"file\" }\n").unwrap();
    let plugins = Scratch::new("job-plugins");
    let plugin = plugins.path.join("jet-deploy");
    fs::write(&plugin, "#!/bin/sh\nprintf 'plugin\\n'\n").unwrap();
    fs::set_permissions(&plugin, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(plugins.path.clone())
        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    for (args, expected) in [
        (&["deploy"][..], "job"),
        (&["serve"][..], "retired-word-job"),
        (&["output"][..], "moved-word-job"),
        (&["run", "deploy"][..], "path"),
    ] {
        let output = jet().args(args).current_dir(&scratch.path).env("PATH", &path).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    }
    // A word ending in `.jet` always names the file: it runs `deploy.jet`,
    // which has no `fn run`, and never the job.
    let file = jet().arg("deploy.jet").current_dir(&scratch.path).env("PATH", &path).output().unwrap();
    assert!(!file.status.success());
    assert!(String::from_utf8_lossy(&file.stderr).contains("E0101"), "{}", String::from_utf8_lossy(&file.stderr));
    assert!(!String::from_utf8_lossy(&file.stdout).contains("job"));
    let internal = jet().arg("secret").current_dir(&scratch.path).env("PATH", &path).output().unwrap();
    assert_eq!(internal.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&internal.stderr).contains("E2101"));
    let outside = Scratch::new("job-outside-project");
    let external = jet().arg("deploy").current_dir(&outside.path).env("PATH", &path).output().unwrap();
    assert!(external.status.success(), "{}", String::from_utf8_lossy(&external.stderr));
    assert_eq!(String::from_utf8_lossy(&external.stdout).trim(), "plugin");
    let help = jet().arg("help").current_dir(&scratch.path).output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
}
