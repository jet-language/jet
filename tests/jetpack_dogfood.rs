#![cfg(target_os = "linux")]

//! Live compiler dogfood, not a package fixture. Invoke deliberately with the
//! coordinated shared target and a fresh, bounded scratch root. The ordinary
//! native fixture regression lives in jetpack_env_use_offline.rs.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jet_env_model::ModuleEval;
use jetpack::Store::{self, ProducerRecord};

mod common;
#[path = "support/no_nix_namespace.rs"]
mod no_nix_namespace;

const TEST: &str = "jet_repository_env_cold_and_offline_without_nix_host_store_or_fixtures";
const ROOT: &str = "JETPACK_DOGFOOD_ROOT";
const TARGET: &str = "JETPACK_DOGFOOD_TARGET_DIR";
const MODE: &str = "JETPACK_DOGFOOD_MODE";

#[test]
#[ignore = "live signed-cache downloads and real compiler build; requires a bounded coordinated dogfood root and target"]
fn jet_repository_env_cold_and_offline_without_nix_host_store_or_fixtures() {
    let base = required_directory(ROOT);
    let target = required_directory(TARGET);
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = base.join("project");
    if env::var_os(no_nix_namespace::CHILD_MARKER).is_some() {
        let offline = env::var(MODE).unwrap() == "offline";
        no_nix_namespace::run_in_no_nix_namespace(
            TEST,
            if offline { no_nix_namespace::NetworkMode::Disabled } else { no_nix_namespace::NetworkMode::Enabled },
            || run_phase(&base, &project, &target, offline),
        );
        return;
    }

    assert!(env::var_os("JETPACK_FIXTURES").is_none(), "live proof must not use fixtures");
    assert!(!project.exists(), "use a fresh dogfood root; do not overwrite a previous receipt");
    assert!(!base.join("store").exists(), "cold proof requires an empty Hangar");
    for directory in [&project, &base.join("home"), &base.join("tmp")] {
        fs::create_dir_all(directory).unwrap();
    }
    for entry in fs::read_dir(&repo).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if matches!(name.to_str(), Some(".git" | ".jet" | "target"))
            || name.to_string_lossy().starts_with("target-")
        {
            continue;
        }
        if name == "env.jet" {
            fs::copy(entry.path(), project.join(name)).unwrap();
        } else {
            std::os::unix::fs::symlink(entry.path(), project.join(name)).unwrap();
        }
    }
    // Share only the coordinated build target; package state and lock are fresh.
    std::os::unix::fs::symlink(&target, project.join("target")).unwrap();
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap()).join(".cargo"));
    env::set_var("JETPACK_DOGFOOD_CARGO_HOME", cargo_home);
    env::set_var("TMPDIR", base.join("tmp"));
    // Re-exec in two separate processes: the second namespace has no network.
    env::set_var(MODE, "online");
    no_nix_namespace::run_in_no_nix_namespace(TEST, no_nix_namespace::NetworkMode::Enabled, || {});
    let lock = fs::read(project.join(".jet/lock")).expect("native admission must produce a lock");
    // Replacing discovery authority must not move an already realized lock.
    fs::create_dir(base.join("replaced-catalog")).unwrap();
    env::set_var(MODE, "offline");
    no_nix_namespace::run_in_no_nix_namespace(TEST, no_nix_namespace::NetworkMode::Disabled, || {});
    assert_eq!(fs::read(project.join(".jet/lock")).unwrap(), lock, "offline replay changed locked authority");
    assert_eq!(
        fs::read_to_string(base.join("offline-compiler-smoke.stdout")).unwrap().trim(),
        "hello, world",
        "the namespace child must execute the ignored test body, not silently skip it"
    );
    assert!(!base.join("home/.jet/trust").exists(), "one-run trust must not persist a grant");
}

fn required_directory(variable: &str) -> PathBuf {
    let path = PathBuf::from(env::var_os(variable).unwrap_or_else(|| panic!("set {variable} explicitly")));
    assert!(path.is_absolute() && path.is_dir(), "{variable} must be an existing absolute directory");
    let canonical = path.canonicalize().unwrap();
    assert!(!canonical.starts_with("/tmp"), "dogfood scratch and target must be disk-backed");
    canonical
}

fn command(base: &Path, project: &Path, target: &Path) -> Command {
    let mut command = Command::new(common::jetpack_bin());
    command.env_clear().current_dir(project)
        .env("PATH", "")
        .env("HOME", base.join("home"))
        .env("TMPDIR", base.join("tmp"))
        .env("JETPACK_ROOT", base.join("store"))
        .env("JETPACK_NIX_FALLBACK_POLICY", "deny")
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_BUILD_JOBS", "2")
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_HOME", env::var_os("JETPACK_DOGFOOD_CARGO_HOME").unwrap())
        .env("RUST_MIN_STACK", "8388608")
        .env("NO_COLOR", "1");
    // Only the namespace harness's staged bootstrap libraries, never its PATH.
    if let Some(loader) = env::var_os("LD_LIBRARY_PATH") {
        command.env("LD_LIBRARY_PATH", loader);
    }
    command
}

fn enter(base: &Path, project: &Path, target: &Path, offline: bool) -> Command {
    let mut command = command(base, project, target);
    command.args(["env", "--env", "compiler", "--trust", "--yes"]);
    if offline {
        // A complete locked closure must replay without any catalog discovery.
        command.arg("--offline").arg("--local-nix-catalog").arg(base.join("replaced-catalog"));
    } else {
        command.arg("--local-nix-catalog").arg(project.join("crates/jetpack/catalogs/jet-dev").canonicalize().unwrap());
    }
    command.arg("--");
    command
}

fn checked(base: &Path, label: &str, command: &mut Command) -> Output {
    let output = command.output().unwrap_or_else(|error| panic!("{label}: {error}"));
    fs::write(base.join(format!("{label}.stdout")), &output.stdout).unwrap();
    fs::write(base.join(format!("{label}.stderr")), &output.stderr).unwrap();
    assert!(output.status.success(), "{label}: {}\n{}", output.status, String::from_utf8_lossy(&output.stderr));
    output
}

fn run_phase(base: &Path, project: &Path, target: &Path, offline: bool) {
    let mode = if offline { "offline" } else { "online" };
    if !offline {
        checked(base, "update", command(base, project, target)
            .args(["update", "default", "--env", "compiler", "--yes"])
            .arg("--local-nix-catalog").arg(project.join("crates/jetpack/catalogs/jet-dev").canonicalize().unwrap()));
    }
    let probe = checked(base, &format!("{mode}-tools"), enter(base, project, target, offline)
        .args(["bash", "-eu", "-c", r#"
! command -v nix
cargo --version
rustc --version
cc --version
node --version
python3 --version
bwrap --version
printf '#include <stdio.h>\nint main(void) { puts("native-c-ok"); }\n' > "$TMPDIR/probe.c"
cc "$TMPDIR/probe.c" -o "$TMPDIR/probe"
test "$("$TMPDIR/probe")" = native-c-ok
printf 'JETPACK_NATIVE_TOOLS_OK\n'
"#]));
    assert!(String::from_utf8_lossy(&probe.stdout).ends_with("JETPACK_NATIVE_TOOLS_OK\n"));
    if !offline {
        checked(base, "compiler-build", enter(base, project, target, false)
            .args(["cargo", "build", "--locked", "--offline", "--bin", "jet", "-j", "2"]));
    }
    let executable = target.join("debug/jet");
    let mut magic = [0; 4];
    use std::io::Read;
    fs::File::open(&executable).unwrap().read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"\x7fELF", "compiler proof must execute a real compiled binary, not a shell fixture");
    let smoke = checked(base, &format!("{mode}-compiler-smoke"), enter(base, project, target, offline)
        .arg(&executable).args(["run", "Examples/features/basics/hello.jet"]));
    assert_eq!(String::from_utf8(smoke.stdout).unwrap().trim(), "hello, world");

    let source = fs::read_to_string(project.join("env.jet")).unwrap();
    let plan = ModuleEval::evaluate_env_with_environment(&source, project, Some("compiler")).unwrap();
    let roots = Store::Roots::at(base.join("store"));
    let entries = Store::list_checked(&roots).unwrap();
    for reference in &plan.package_refs {
        let entry = entries.iter().find(|entry| &entry.reference == reference)
            .unwrap_or_else(|| panic!("missing declared package {reference}"));
        let producer = ProducerRecord::decode(&entry.producer_record).unwrap();
        assert_eq!(producer.facts.get("nix.index.revision").map(String::as_str),
            Some("3ed67ec0a4d3c7ab4ae1f04f8ee8df07bfa506a2"));
        assert_eq!(producer.facts.get("nix.index.tier").map(String::as_str), Some("local-unofficial"));
        assert!(producer.facts.contains_key("nix.cache.closure.receipt.sha256"));
        assert!(!producer.facts.contains_key("nix.fallback.provenance"));
    }
    checked(base, &format!("{mode}-du"), command(base, project, target).args(["hangar", "du", "--json"]));
}
