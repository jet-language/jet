//! Card #2900: prove the first-hour path on one real, isolated project.
//!
//! This is intentionally an end-to-end CLI scenario rather than a unit test.
//! It keeps every generated project and every Jet execution/package cache
//! below the disk-backed Scratch root, and it never turns a missing tier into a
//! skip.

mod common;

use common::Scratch;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CARD: &str = "#2900";
const OWNER_NEW: &str = "#2433";
const OWNER_DEPS: &str = "#2883";
const OWNER_WEB: &str = "#2882";
const OWNER_ADVANCED: &str = "#2878/#2879";

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn prepare_environment(root: &Path) {
    for name in [
        "store",
        "package-store",
        "run-cache",
        "test-scratch",
        "oracle-cache",
        "tmp",
    ] {
        fs::create_dir_all(root.join(name)).unwrap_or_else(|error| {
            panic!(
                "{CARD} environment setup (owner {CARD}) could not create {}: {error}",
                root.join(name).display()
            )
        });
    }
}

fn command_with_environment(
    label: &str,
    owner: &str,
    cwd: &Path,
    environment: &Path,
    args: &[&str],
) -> Output {
    let mut command = Command::new(jet_bin());
    command.args(args).current_dir(cwd);
    command
        .env("JET_STORE_DIR", environment.join("store"))
        .env("JET_PACKAGE_STORE_DIR", environment.join("package-store"))
        .env("JET_RUN_CACHE_DIR", environment.join("run-cache"))
        .env("JET_TEST_SCRATCH_DIR", environment.join("test-scratch"))
        .env("JET_DEV_ORACLE_CACHE_DIR", environment.join("oracle-cache"))
        .env("TMPDIR", environment.join("tmp"))
        .env("TMP", environment.join("tmp"))
        .env("TEMP", environment.join("tmp"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "{CARD} step `{label}` (owner {owner}) could not start in {}: {error}",
                cwd.display()
            )
        })
}

fn assert_success(label: &str, owner: &str, cwd: &Path, output: &Output) {
    assert!(
        output.status.success(),
        "{CARD} step `{label}` (owner {owner}) failed in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_exact(
    label: &str,
    owner: &str,
    cwd: &Path,
    environment: &Path,
    args: &[&str],
    expected_stdout: &str,
) {
    let output = command_with_environment(label, owner, cwd, environment, args);
    assert_success(label, owner, cwd, &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout,
        "{CARD} step `{label}` (owner {owner}) produced unexpected stdout"
    );
}

fn run_contains(
    label: &str,
    owner: &str,
    cwd: &Path,
    environment: &Path,
    args: &[&str],
    expected_stdout: &str,
) {
    let output = command_with_environment(label, owner, cwd, environment, args);
    assert_success(label, owner, cwd, &output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(expected_stdout),
        "{CARD} step `{label}` (owner {owner}) stdout did not contain `{expected_stdout}`:\n{stdout}"
    );
}

fn write_source(project: &Path, relative: &str, source: &str, owner: &str) {
    let path = project.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| {
            panic!(
                "{CARD} source step `{relative}` (owner {owner}) could not create {}: {error}",
                parent.display()
            )
        });
    }
    fs::write(&path, source).unwrap_or_else(|error| {
        panic!(
            "{CARD} source step `{relative}` (owner {owner}) could not write {}: {error}",
            path.display()
        )
    });
}

fn assert_file(project: &Path, relative: &str, owner: &str) {
    let path = project.join(relative);
    assert!(
        path.is_file(),
        "{CARD} expected `{}` (owner {owner}) after the preceding step",
        path.display()
    );
}

const SECOND_MODULE: &str = r#"pub fn message() String -> "second"
"#;

const SECOND_MODULE_RUN: &str = r#"use second

#CLI
struct GreetingArgs {
    #Doc("name to greet") name: String{"world"}
}

fn greeting(name: String) String -> "hello, {name}"

fn run(args: GreetingArgs) {
    print(greeting(args.name))
    print(second.message())
}

#Test("the greeting stays stable") {
    assert_eq(greeting("world"), "hello, world")
}
"#;

const PATH_DEPENDENCY_RUN: &str = r#"use second
use lib1

#CLI
struct GreetingArgs {
    #Doc("name to greet") name: String{"world"}
}

fn greeting(name: String) String -> "hello, {name}"

fn run(args: GreetingArgs) {
    print(greeting(args.name))
    print(second.message())
    print(lib1.answer())
}

#Test("the greeting stays stable") {
    assert_eq(greeting("world"), "hello, world")
}
"#;

const WEB_RUN: &str = r#"#Target(Web)

fn run() {}
"#;

const FINAL_RUN: &str = r#"#CLI
struct GreetingArgs {
    #Doc("name to greet") name: String{"world"}
}

#Comparable
struct Score {
    value: Int
}

struct Node {
    value: Int
    next: ?Node
}

fn comparable_max<T: Comparable>(left: T, right: T) T -> {
    if right > left -> return right
    return left
}

fn greeting(name: String) String -> "hello, {name}"

fn run(args: GreetingArgs) {
    print(greeting(args.name))
    best :: comparable_max(Score{value: 3}, Score{value: 5})
    print(best.value)
    node :: Node{value: 7, next: None}
    print(node.value)
}

#Test("the greeting stays stable") {
    assert_eq(greeting("world"), "hello, world")
}
"#;

#[test]
fn first_hour_cli_journey() {
    let scratch = Scratch::new("first-hour");
    let root = scratch.path.clone();
    let environment = root.join("environment");
    prepare_environment(&environment);

    run_exact(
        "jet new demo",
        OWNER_NEW,
        &root,
        &environment,
        &["new", "demo"],
        "",
    );
    let demo = root.join("demo");
    for file in [
        "package.jet",
        "run.jet",
        "@run.jet",
        "@build.jet",
        "@dev.jet",
        "@test.jet",
        ".gitignore",
    ] {
        assert_file(&demo, file, OWNER_NEW);
    }

    run_exact(
        "jet run",
        OWNER_NEW,
        &demo,
        &environment,
        &["run"],
        "hello, world\n",
    );
    run_contains(
        "jet test",
        CARD,
        &demo,
        &environment,
        &["test", "--capture=all"],
        "greeting stays stable: pass",
    );

    write_source(&demo, "second.jet", SECOND_MODULE, CARD);
    write_source(&demo, "run.jet", SECOND_MODULE_RUN, CARD);
    run_exact(
        "second module call",
        CARD,
        &demo,
        &environment,
        &["run"],
        "hello, world\nsecond\n",
    );

    run_exact(
        "jet new lib1",
        OWNER_NEW,
        &root,
        &environment,
        &["new", "lib1"],
        "",
    );
    let lib1 = root.join("lib1");
    write_source(
        &lib1,
        "lib1.jet",
        "pub fn answer() Int -> {\n    return 42\n}\n",
        OWNER_DEPS,
    );
    run_contains(
        "jet add lib1 --path ../lib1",
        OWNER_DEPS,
        &demo,
        &environment,
        &["add", "lib1", "--path", "../lib1"],
        "added `lib1`",
    );
    write_source(&demo, "run.jet", PATH_DEPENDENCY_RUN, OWNER_DEPS);
    run_exact(
        "path dependency call",
        OWNER_DEPS,
        &demo,
        &environment,
        &["run"],
        "hello, world\nsecond\n42\n",
    );
    run_exact(
        "jet fetch",
        OWNER_DEPS,
        &demo,
        &environment,
        &["fetch"],
        "fetched all dependencies\n",
    );
    run_exact(
        "jet build --locked",
        OWNER_DEPS,
        &demo,
        &environment,
        &["build", "--locked"],
        "built: build/run\n",
    );
    assert_file(&demo, "build/run", OWNER_DEPS);
    run_exact(
        "jet build --release",
        OWNER_DEPS,
        &demo,
        &environment,
        &["build", "--release"],
        "built: build/run\n",
    );

    write_source(&demo, "run.jet", WEB_RUN, OWNER_WEB);
    run_contains(
        "jet build --target web",
        OWNER_WEB,
        &demo,
        &environment,
        &["build", "--target", "web"],
        "built: build/app.wasm + build/app.js",
    );
    assert_file(&demo, "build/app.wasm", OWNER_WEB);
    assert_file(&demo, "build/app.js", OWNER_WEB);

    write_source(&demo, "run.jet", FINAL_RUN, OWNER_ADVANCED);
    run_exact(
        "AOT final program",
        OWNER_ADVANCED,
        &demo,
        &environment,
        &["run", "--release"],
        "hello, world\n5\n7\n",
    );
    run_exact(
        "JIT final program",
        OWNER_ADVANCED,
        &demo,
        &environment,
        &["run"],
        "hello, world\n5\n7\n",
    );
    run_exact(
        "interpreter final program",
        OWNER_ADVANCED,
        &demo,
        &environment,
        &["run", "--interpret"],
        "hello, world\n5\n7\n",
    );
    run_exact(
        "jet fmt result",
        CARD,
        &demo,
        &environment,
        &["fmt", "run.jet"],
        "",
    );
    run_contains(
        "jet check result",
        CARD,
        &demo,
        &environment,
        &["check"],
        "check: passed",
    );
}
