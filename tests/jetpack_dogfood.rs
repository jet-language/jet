#![cfg(target_os = "linux")]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use jet_env_model::ModuleEval;
use jetpack::Lock::{self, LockSource};
use jetpack::Store;
use jetpack::JSON::JSONValue;

mod common;
#[path = "support/no_nix_namespace.rs"]
mod no_nix_namespace;

const PROBE_JSON_PREFIX: &str = "JETPACK_DOGFOOD_JSON=";
const DOGFOOD_MODE_ENV: &str = "JETPACK_DOGFOOD_MODE";
const DOGFOOD_ROOT_ENV: &str = "JETPACK_DOGFOOD_ROOT";
const EXPECTED_PACKAGES: &[&str] = &[
    "cargo",
    "sccache",
    "clippy",
    "rustc",
    "gcc",
    "clang",
    "lld",
    "nodejs_22",
    "nixfmt",
    "ripgrep",
    "jq",
    "gh",
    "fd",
    "bashInteractive",
    "zsh",
    "fish",
    "util-linux",
    "wasm-tools",
    "tree-sitter",
    "pkg-config",
];

struct Probe {
    package: &'static str,
    command: &'static str,
    args: &'static [&'static str],
}

const PROBES: &[Probe] = &[
    Probe {
        package: "cargo",
        command: "cargo",
        args: &["--version"],
    },
    Probe {
        package: "sccache",
        command: "sccache",
        args: &["--version"],
    },
    Probe {
        package: "clippy",
        command: "cargo-clippy",
        args: &["--version"],
    },
    Probe {
        package: "rustc",
        command: "rustc",
        args: &["--version"],
    },
    Probe {
        package: "gcc",
        command: "gcc",
        args: &["--version"],
    },
    Probe {
        package: "clang",
        command: "clang",
        args: &["--version"],
    },
    Probe {
        package: "lld",
        command: "ld.lld",
        args: &["--version"],
    },
    Probe {
        package: "nodejs_22",
        command: "node",
        args: &["--version"],
    },
    Probe {
        package: "nixfmt",
        command: "nixfmt",
        args: &["--version"],
    },
    Probe {
        package: "ripgrep",
        command: "rg",
        args: &["--version"],
    },
    Probe {
        package: "jq",
        command: "jq",
        args: &["--version"],
    },
    Probe {
        package: "gh",
        command: "gh",
        args: &["--version"],
    },
    Probe {
        package: "fd",
        command: "fd",
        args: &["--version"],
    },
    Probe {
        package: "bashInteractive",
        command: "bash",
        args: &["--version"],
    },
    Probe {
        package: "zsh",
        command: "zsh",
        args: &["--version"],
    },
    Probe {
        package: "fish",
        command: "fish",
        args: &["--version"],
    },
    Probe {
        package: "util-linux",
        command: "unshare",
        args: &["--version"],
    },
    Probe {
        package: "wasm-tools",
        command: "wasm-tools",
        args: &["--version"],
    },
    Probe {
        package: "tree-sitter",
        command: "tree-sitter",
        args: &["--version"],
    },
    Probe {
        package: "pkg-config",
        command: "pkg-config",
        args: &["--version"],
    },
];

#[test]
fn jet_repository_env_cold_and_offline_without_nix_host_store_or_fixtures() {
    let test_name = "jet_repository_env_cold_and_offline_without_nix_host_store_or_fixtures";
    if env::var_os(no_nix_namespace::CHILD_MARKER).is_some() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let contract = assert_env_lock_contract(&repo);
        let jetpack = common::jetpack_bin();
        let test_binary = env::current_exe().expect("current test binary");
        let scratch = DogfoodScratch::existing(PathBuf::from(
            env::var_os(DOGFOOD_ROOT_ENV).expect("dogfood child root"),
        ));
        let mode = env::var(DOGFOOD_MODE_ENV).expect("dogfood child mode");
        let network_mode = match mode.as_str() {
            "online" => no_nix_namespace::NetworkMode::Enabled,
            "offline" => no_nix_namespace::NetworkMode::Disabled,
            other => panic!("unknown dogfood mode {other}"),
        };
        no_nix_namespace::run_in_no_nix_namespace(test_name, network_mode, || {
            run_phase(&repo, &jetpack, &test_binary, &scratch, &contract, &mode);
        });
        return;
    }

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let contract = assert_env_lock_contract(&repo);
    let lock_path = repo.join(".jet/lock");
    let lock_before = fs::read(&lock_path).expect("read checked-in Jet lock");
    let scratch = DogfoodScratch::new(&repo);
    env::set_var(DOGFOOD_ROOT_ENV, &scratch.root);

    assert!(!scratch.root.join("hangar/objects").exists());
    assert!(!scratch.root.join("fixtures").exists());
    assert!(env::var_os("JETPACK_FIXTURES").is_none());

    let modes = [
        ("online", no_nix_namespace::NetworkMode::Enabled),
        ("offline", no_nix_namespace::NetworkMode::Disabled),
    ];
    let mut summaries = BTreeMap::new();
    let mut receipts = BTreeMap::new();
    let mut physical = BTreeMap::new();

    for (mode, network_mode) in modes {
        let mode = mode.to_owned();
        env::set_var(DOGFOOD_MODE_ENV, &mode);
        no_nix_namespace::run_in_no_nix_namespace(test_name, network_mode, || {});
        assert!(!scratch.root.join("fixtures").exists());

        let summary = read_phase_summary(&scratch, &mode);
        let roots = Store::Roots::at(scratch.root.clone());
        let phase_receipts = capture_receipts(&roots, &contract.probe_packages);
        let phase_physical = independent_physical_use(&roots);
        assert_du_matches(&summary.du, phase_physical);
        summaries.insert(mode.clone(), summary);
        receipts.insert(mode.clone(), phase_receipts);
        physical.insert(mode, phase_physical);
    }

    let online = summaries.get("online").expect("online summary");
    let offline = summaries.get("offline").expect("offline summary");
    assert_ne!(
        online.phase_pid, offline.phase_pid,
        "offline run reused online phase process"
    );
    assert_ne!(
        offline.phase_pid,
        u64::from(std::process::id()),
        "offline run executed in the parent test process"
    );
    assert_ne!(
        online.jetpack_pid, offline.jetpack_pid,
        "offline run reused online Jetpack process"
    );
    assert_ne!(offline.jetpack_pid, u64::from(std::process::id()));
    assert_eq!(
        probe_identity(&online.probe),
        probe_identity(&offline.probe)
    );
    assert_eq!(online.du, offline.du);
    assert_eq!(receipts.get("online"), receipts.get("offline"));
    assert_eq!(physical.get("online"), physical.get("offline"));
    assert_eq!(lock_before, fs::read(lock_path).expect("re-read Jet lock"));
}

#[test]
fn jetpack_dogfood_probe_child() {
    if env::var_os("JETPACK_ENV").is_none() {
        return;
    }

    assert!(env::var_os("JETPACK_FIXTURES").is_none());
    assert!(!env::vars_os().any(|(key, _)| key.to_string_lossy().starts_with("NIX_")));
    assert_no_nix_executable();
    assert_no_host_store_lower_layer();

    let mut records = Vec::with_capacity(PROBES.len());
    for probe in PROBES {
        let (path_index, resolved) = find_on_path(probe.command)
            .unwrap_or_else(|| panic!("{} missing from projected PATH", probe.command));
        let output = Command::new(&resolved)
            .args(probe.args)
            .output()
            .unwrap_or_else(|error| panic!("run {}: {error}", probe.command));
        assert!(
            output.status.success(),
            "{} failed: {}",
            probe.command,
            output_text(&output)
        );
        records.push(format!(
            "{{\"package\":{},\"command\":{},\"path_index\":{},\"resolved\":{},\"status\":{},\"version\":{},\"output\":{}}}",
            jetpack::JSON::quote(probe.package),
            jetpack::JSON::quote(probe.command),
            path_index,
            jetpack::JSON::quote(&resolved.to_string_lossy()),
            output.status.code().unwrap_or(-1),
            jetpack::JSON::quote(&version_line(&output)),
            jetpack::JSON::quote(&output_text(&output)),
        ));
    }

    let nix = match Command::new("nix").output() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            "{\"kind\":\"not-found\"}".to_owned()
        }
        Err(error) => format!(
            "{{\"kind\":\"error\",\"message\":{}}}",
            jetpack::JSON::quote(&error.to_string())
        ),
        Ok(output) => format!(
            "{{\"kind\":\"spawned\",\"status\":{},\"version\":{}}}",
            output.status.code().unwrap_or(-1),
            jetpack::JSON::quote(&version_line(&output))
        ),
    };
    assert_eq!(nix, "{\"kind\":\"not-found\"}");

    let path = env::var("PATH").expect("Jetpack must set PATH");
    assert!(!path.is_empty(), "projected PATH is empty");
    println!(
        "{PROBE_JSON_PREFIX}{{\"path\":{},\"nix\":{},\"probes\":[{}]}}",
        jetpack::JSON::quote(&path),
        nix,
        records.join(",")
    );
}

fn assert_env_lock_contract(repo: &Path) -> EnvContract {
    let source = fs::read_to_string(repo.join("env.jet")).expect("read repository env.jet");
    let plan = ModuleEval::evaluate_env_with_environment(&source, repo, Some("dev"))
        .expect("evaluate repository env.jet");
    let all_declared: Vec<String> = plan
        .package_refs
        .into_iter()
        .map(|reference| {
            reference
                .strip_suffix("@default")
                .unwrap_or_else(|| {
                    panic!("env.jet package is not from default source: {reference}")
                })
                .to_owned()
        })
        .collect();
    let mut probe_positions = Vec::with_capacity(EXPECTED_PACKAGES.len());
    for expected in EXPECTED_PACKAGES {
        let position = all_declared
            .iter()
            .position(|name| name == expected)
            .unwrap_or_else(|| panic!("env.jet is missing probe package {expected}"));
        probe_positions.push(position);
    }
    assert!(
        probe_positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the fixed 20-tool probe order is not preserved in env.jet"
    );
    let probe_packages = EXPECTED_PACKAGES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();

    let lock = Lock::parse(&fs::read_to_string(repo.join(".jet/lock")).expect("read Jet lock"))
        .expect("parse Jet lock");
    let locked: Vec<String> = lock
        .packages
        .iter()
        .filter_map(|package| match &package.source {
            LockSource::Nix { .. } => Some(package.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        locked, all_declared,
        "checked-in .jet/lock must contain every env.jet attr in declared order"
    );
    assert_eq!(lock.packages.len(), all_declared.len());
    assert_eq!(
        lock.source_channels.len(),
        1,
        "lock must pin the source channel"
    );
    assert!(!lock.source_channels[0].exact.is_empty());
    EnvContract {
        all_packages: all_declared,
        probe_packages,
    }
}

fn run_phase(
    repo: &Path,
    jetpack: &Path,
    test_binary: &Path,
    scratch: &DogfoodScratch,
    contract: &EnvContract,
    mode: &str,
) {
    let offline = mode == "offline";
    assert!(
        env::var_os(no_nix_namespace::CHILD_MARKER).is_some(),
        "dogfood phase must run in the namespace child process"
    );
    fs::write(scratch.phase_pid_path(mode), std::process::id().to_string())
        .expect("save phase process identity");
    let mut probe_command = enter_command(repo, jetpack, scratch, offline);
    probe_command
        .arg("--")
        .arg(test_binary)
        .args(["--exact", "jetpack_dogfood_probe_child", "--nocapture"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let probe_process = probe_command.spawn().expect("run projected command probe");
    let jetpack_pid = probe_process.id();
    let probe = probe_process
        .wait_with_output()
        .expect("collect projected command probe");
    assert_success("projected command probe", &probe);
    let probe_json = extract_probe_json(&probe);
    assert_projected_paths(&probe_json, scratch, contract);

    fs::write(scratch.jetpack_pid_path(mode), jetpack_pid.to_string())
        .expect("save Jetpack process identity");

    let build = enter_command(repo, jetpack, scratch, offline)
        .args(["--", "cargo", "build", "--locked", "--bin", "jet"])
        .output()
        .expect("run Jet compiler build in projected environment");
    assert_success("cargo build --locked --bin jet", &build);
    assert!(
        scratch.target.join("debug/jet").is_file(),
        "projected cargo build did not produce target/debug/jet"
    );

    let tests = enter_command(repo, jetpack, scratch, offline)
        .args([
            "--",
            "cargo",
            "test",
            "--locked",
            "--test",
            "exact_number_tiers",
        ])
        .output()
        .expect("run targeted Jet test in projected environment");
    assert_success("cargo test --locked --test exact_number_tiers", &tests);

    let mut du_command = clean_command(jetpack, repo, scratch, offline);
    du_command.args(["hangar", "du", "--json"]);
    if offline {
        du_command.arg("--offline");
    }
    let du = du_command.output().expect("run hangar du --json");
    assert_success("hangar du --json", &du);
    let du_text = String::from_utf8(du.stdout).expect("hangar du JSON is UTF-8");
    let du_json = du_text.trim();
    jetpack::JSON::parse(du_json).expect("hangar du --json output is valid JSON");

    fs::write(scratch.probe_path(mode), probe_json_text(&probe)).expect("save probe evidence");
    fs::write(scratch.du_path(mode), du_json).expect("save du evidence");
}

fn enter_command<'a>(
    repo: &'a Path,
    jetpack: &'a Path,
    scratch: &'a DogfoodScratch,
    offline: bool,
) -> Command {
    let mut command = clean_command(jetpack, repo, scratch, offline);
    command.args(["env", "--trust"]);
    if offline {
        command.arg("--offline");
    }
    command
}

fn clean_command(program: &Path, repo: &Path, scratch: &DogfoodScratch, _offline: bool) -> Command {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").expect("HOME")).join(".cargo"));
    let loader_path = env::var_os("LD_LIBRARY_PATH");
    let mut command = Command::new(program);
    command
        .env_clear()
        .current_dir(repo)
        .env("PATH", "")
        .env("HOME", &scratch.home)
        .env("TMPDIR", &scratch.tmp)
        .env("TMP", &scratch.tmp)
        .env("TEMP", &scratch.tmp)
        .env("CARGO_HOME", cargo_home)
        .env("CARGO_TARGET_DIR", &scratch.target)
        .env("JETPACK_ROOT", &scratch.root)
        .env("NO_COLOR", "1");
    if let Some(loader_path) = loader_path {
        command.env("LD_LIBRARY_PATH", loader_path);
    }
    command
}

fn assert_projected_paths(value: &JSONValue, scratch: &DogfoodScratch, contract: &EnvContract) {
    let path = value_field(value, "path")
        .as_str()
        .expect("probe PATH string");
    let path_dirs: Vec<PathBuf> = env::split_paths(&OsString::from(path)).collect();
    assert!(!path_dirs.is_empty());
    let probes = value_field(value, "probes")
        .as_array()
        .expect("probe records");
    assert_eq!(probes.len(), PROBES.len());
    let roots = Store::Roots::at(scratch.root.clone());
    let entries = Store::list_checked(&roots).expect("list admitted Hangar packages");
    let package_entries: Vec<_> = entries
        .iter()
        .filter(|entry| contract.all_packages.iter().any(|name| entry.name == *name))
        .collect();
    assert_eq!(
        package_entries.len(),
        contract.all_packages.len(),
        "every declared package must have one projected Hangar record"
    );
    let ordered: Vec<_> = contract
        .all_packages
        .iter()
        .map(|name| {
            package_entries
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap_or_else(|| panic!("Hangar package {name} missing"))
        })
        .collect();
    let wrappers: Vec<_> = ordered
        .iter()
        .filter(|entry| !entry.bin.is_empty())
        .collect();

    let first_probe = json_i64(object_field(&probes[0], "path_index")).expect("cargo PATH index");
    let first_exporter = wrappers
        .iter()
        .position(|entry| Path::new(&entry.bin).join(PROBES[0].command).is_file())
        .expect("cargo has a Hangar exporter");
    assert!(first_probe >= first_exporter as i64);
    assert!((first_probe as usize) < path_dirs.len());
    let offset = first_probe - first_exporter as i64;

    for (probe, record) in PROBES.iter().zip(probes) {
        assert_eq!(
            value_field(record, "package").as_str().ok(),
            Some(probe.package)
        );
        assert_eq!(
            value_field(record, "command").as_str().ok(),
            Some(probe.command)
        );
        assert_eq!(json_i64(object_field(record, "status")).ok(), Some(0));
        assert!(!value_field(record, "version")
            .as_str()
            .unwrap_or("")
            .is_empty());
        let path_index = json_i64(object_field(record, "path_index")).expect("PATH index");
        assert!(path_index >= 0 && (path_index as usize) < path_dirs.len());
        let exporter = wrappers
            .iter()
            .position(|entry| Path::new(&entry.bin).join(probe.command).is_file())
            .unwrap_or_else(|| panic!("{} has no Hangar exporter", probe.command));
        assert_eq!(
            path_index,
            offset + exporter as i64,
            "PATH collision order changed for {}",
            probe.command
        );
        let resolved = PathBuf::from(
            value_field(record, "resolved")
                .as_str()
                .expect("resolved path"),
        );
        assert!(resolved.is_absolute());
        assert!(resolved.starts_with(&path_dirs[path_index as usize]));
        assert!(!resolved.starts_with("/nix/store"));
    }

    let nix = value_field(value, "nix");
    assert_eq!(value_field(nix, "kind").as_str().ok(), Some("not-found"));
    assert_snapshot(value);
}

fn assert_snapshot(value: &JSONValue) {
    let snapshot = jetpack::JSON::parse(include_str!("cli/jetpack_dogfood_versions.json"))
        .expect("dogfood version snapshot JSON");
    let expected = value_field(&snapshot, "packages")
        .as_array()
        .expect("snapshot packages");
    let actual = value_field(value, "probes")
        .as_array()
        .expect("probe packages");
    assert_eq!(expected.len(), actual.len());
    for (expected, actual) in expected.iter().zip(actual) {
        assert_eq!(
            value_field(expected, "package"),
            value_field(actual, "package")
        );
        assert_eq!(
            value_field(expected, "command"),
            value_field(actual, "command")
        );
        let version = value_field(expected, "version")
            .as_str()
            .expect("snapshot version");
        assert!(
            !version.is_empty(),
            "populate the reviewed version snapshot before closing the gate"
        );
        assert_eq!(
            version,
            value_field(actual, "version")
                .as_str()
                .expect("probe version")
        );
    }
}

fn assert_du_matches(value: &JSONValue, expected: PhysicalUse) {
    assert_eq!(json_u64(value, "unique_bytes"), expected.unique_bytes);
    assert_eq!(json_u64(value, "shared_bytes"), expected.shared_bytes);
    assert_eq!(
        json_u64(value, "closure_physical_bytes"),
        expected.total_bytes
    );
    assert_eq!(
        expected.unique_bytes.checked_add(expected.shared_bytes),
        Some(expected.total_bytes)
    );
}

fn capture_receipts(roots: &Store::Roots, expected_packages: &[String]) -> Vec<ReceiptIdentity> {
    let entries = Store::list_checked(roots).expect("list Hangar receipts");
    expected_packages
        .iter()
        .map(|name| {
            let entry = entries
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap_or_else(|| panic!("receipt for {name} missing"));
            ReceiptIdentity {
                id: entry.id.clone(),
                name: entry.name.clone(),
                version: entry.version.clone(),
                reference: entry.reference.clone(),
                out: entry.out.clone(),
                output_hash: entry.envelope.output_hash.clone(),
                references: entry.references.clone(),
                named_outputs: entry.named_outputs.clone(),
                producer_record: entry.producer_record.clone(),
                receipt: entry.receipt.clone(),
            }
        })
        .collect()
}

fn independent_physical_use(roots: &Store::Roots) -> PhysicalUse {
    let entries = Store::list_checked(roots).expect("list Hangar objects");
    let graph = Store::closure_graph(roots).expect("read Hangar closure graph");
    let mut owners = BTreeMap::<String, BTreeSet<String>>::new();
    let mut paths = BTreeMap::<String, PathBuf>::new();
    let hangar = roots.hangar_dir().join("objects");
    for entry in &entries {
        let record = graph
            .records
            .get(&entry.id)
            .unwrap_or_else(|| panic!("missing graph record for {}", entry.name));
        for output in record.outputs.values() {
            for digest in graph.closure(output) {
                let object = graph.objects.get(&digest).expect("closure object record");
                let object_path = PathBuf::from(&object.path);
                assert!(object_path.starts_with(&hangar));
                owners
                    .entry(digest.clone())
                    .or_default()
                    .insert(entry.id.clone());
                if let Some(previous) = paths.insert(digest, object_path.clone()) {
                    assert_eq!(
                        previous, object_path,
                        "closure digest has conflicting paths"
                    );
                }
            }
        }
    }

    let mut nodes = BTreeMap::<(u64, u64), (u64, BTreeSet<String>)>::new();
    let mut active = BTreeSet::new();
    for (digest, path) in paths {
        let object_owners = owners.get(&digest).expect("object owners");
        walk_physical_nodes(&path, object_owners, &mut nodes, &mut active);
    }
    let mut unique_bytes: u64 = 0;
    let mut shared_bytes: u64 = 0;
    for (bytes, object_owners) in nodes.values() {
        if object_owners.len() > 1 {
            shared_bytes = shared_bytes
                .checked_add(*bytes)
                .expect("shared byte overflow");
        } else {
            unique_bytes = unique_bytes
                .checked_add(*bytes)
                .expect("unique byte overflow");
        }
    }
    PhysicalUse {
        unique_bytes,
        shared_bytes,
        total_bytes: unique_bytes
            .checked_add(shared_bytes)
            .expect("physical byte overflow"),
    }
}

fn walk_physical_nodes(
    path: &Path,
    owners: &BTreeSet<String>,
    nodes: &mut BTreeMap<(u64, u64), (u64, BTreeSet<String>)>,
    active: &mut BTreeSet<(u64, u64)>,
) {
    let metadata = fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("stat {}: {error}", path.display()));
    let key = (metadata.dev(), metadata.ino());
    let bytes = metadata
        .blocks()
        .checked_mul(512)
        .expect("physical byte overflow");
    nodes
        .entry(key)
        .and_modify(|(_, existing)| existing.extend(owners.iter().cloned()))
        .or_insert_with(|| (bytes, owners.clone()));
    if metadata.is_dir() && active.insert(key) {
        for child in
            fs::read_dir(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        {
            walk_physical_nodes(
                &child.expect("Hangar directory entry").path(),
                owners,
                nodes,
                active,
            );
        }
        active.remove(&key);
    }
}

fn read_phase_summary(scratch: &DogfoodScratch, mode: &str) -> PhaseSummary {
    let probe_text = fs::read_to_string(scratch.probe_path(mode)).expect("read probe evidence");
    let du_text = fs::read_to_string(scratch.du_path(mode)).expect("read du evidence");
    let phase_pid = fs::read_to_string(scratch.phase_pid_path(mode))
        .expect("read phase process identity")
        .trim()
        .parse()
        .expect("phase process identity is a pid");
    let jetpack_pid = fs::read_to_string(scratch.jetpack_pid_path(mode))
        .expect("read Jetpack process identity")
        .trim()
        .parse()
        .expect("Jetpack process identity is a pid");
    PhaseSummary {
        phase_pid,
        jetpack_pid,
        probe: jetpack::JSON::parse(probe_text.trim()).expect("parse probe evidence"),
        du: jetpack::JSON::parse(du_text.trim()).expect("parse du evidence"),
    }
}

fn extract_probe_json(output: &Output) -> JSONValue {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix(PROBE_JSON_PREFIX))
        .expect("probe child JSON marker");
    jetpack::JSON::parse(line).expect("probe child JSON")
}

fn probe_json_text(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(PROBE_JSON_PREFIX))
        .expect("probe child JSON marker")
        .to_owned()
}

fn probe_identity(value: &JSONValue) -> Vec<(String, String, i64, String, String, i64)> {
    value_field(value, "probes")
        .as_array()
        .expect("probe records")
        .iter()
        .map(|record| {
            (
                value_field(record, "package")
                    .as_str()
                    .expect("package")
                    .to_owned(),
                value_field(record, "command")
                    .as_str()
                    .expect("command")
                    .to_owned(),
                json_i64(object_field(record, "path_index")).expect("PATH index"),
                value_field(record, "version")
                    .as_str()
                    .expect("version")
                    .to_owned(),
                value_field(record, "output")
                    .as_str()
                    .expect("output")
                    .to_owned(),
                json_i64(object_field(record, "status")).expect("status"),
            )
        })
        .collect()
}

fn find_on_path(command: &str) -> Option<(usize, PathBuf)> {
    env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .find_map(|(index, directory)| {
            let candidate = directory.join(command);
            candidate.is_file().then_some((index, candidate))
        })
}

fn assert_no_nix_executable() {
    let error = Command::new("nix")
        .output()
        .expect_err("ambient nix executable is forbidden");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

fn assert_no_host_store_lower_layer() {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").expect("read mountinfo");
    assert!(!mountinfo
        .lines()
        .any(|line| { line.contains("lowerdir=") && line.contains("/nix/store") }));
    assert!(!mountinfo
        .lines()
        .any(|line| { line.contains(" - overlay ") && line.contains("/nix/store") }));
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn version_line(output: &Output) -> String {
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned()
}

fn value_field<'a>(value: &'a JSONValue, key: &str) -> &'a JSONValue {
    value
        .get(key)
        .unwrap_or_else(|_| panic!("JSON field {key} missing"))
}

fn object_field<'a>(value: &'a JSONValue, key: &str) -> &'a JSONValue {
    value_field(value, key)
}

fn json_u64(value: &JSONValue, key: &str) -> u64 {
    let number = json_i64(value_field(value, key)).expect("JSON integer");
    u64::try_from(number).expect("JSON non-negative integer")
}

fn json_i64(value: &JSONValue) -> Result<i64, String> {
    match value {
        JSONValue::Number(number) => Ok(*number),
        _ => Err("expected JSON integer".to_owned()),
    }
}

struct DogfoodScratch {
    root: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
    target: PathBuf,
    owned: bool,
}

impl DogfoodScratch {
    fn new(repo: &Path) -> Self {
        let base = common::test_scratch_root("jetpack-dogfood");
        let path = base.join(format!("run-{}", std::process::id()));
        if path.exists() {
            common::make_tree_writable(&path);
            fs::remove_dir_all(&path).expect("remove stale dogfood scratch");
        }
        let root = path.join("root");
        let home = path.join("home");
        let tmp = path.join("tmp");
        let target = path.join("target");
        for directory in [&root, &home, &tmp, &target] {
            fs::create_dir_all(directory).expect("create dogfood scratch directory");
        }
        install_signed_index_config(repo, &root);
        Self {
            root,
            home,
            tmp,
            target,
            owned: true,
        }
    }

    fn existing(root: PathBuf) -> Self {
        let base = root
            .parent()
            .expect("dogfood root parent")
            .to_path_buf();
        Self {
            root,
            home: base.join("home"),
            tmp: base.join("tmp"),
            target: base.join("target"),
            owned: false,
        }
    }

    fn probe_path(&self, mode: &str) -> PathBuf {
        self.root.join(format!("dogfood-{mode}.probe.json"))
    }

    fn du_path(&self, mode: &str) -> PathBuf {
        self.root.join(format!("dogfood-{mode}.du.json"))
    }

    fn jetpack_pid_path(&self, mode: &str) -> PathBuf {
        self.root.join(format!("dogfood-{mode}.pid"))
    }

    fn phase_pid_path(&self, mode: &str) -> PathBuf {
        self.root.join(format!("dogfood-{mode}.phase.pid"))
    }
}

fn install_signed_index_config(repo: &Path, root: &Path) {
    let feed = repo.join("target-nixfeed/feed");
    let endpoint = feed.join("config/nix-index-v1.endpoint");
    let trust = feed.join("trust/nix-index-v1.ed25519.pub");
    match (endpoint.is_file(), trust.is_file()) {
        (false, false) => return,
        (true, true) => {}
        _ => panic!("generated nix index feed has an incomplete config/trust pair"),
    }
    for relative in [
        "config/nix-index-v1.endpoint",
        "trust/nix-index-v1.ed25519.pub",
    ] {
        let destination = root.join(relative);
        fs::create_dir_all(destination.parent().expect("index config parent"))
            .expect("create index config directory");
        fs::copy(feed.join(relative), destination).expect("copy signed index configuration");
    }
}

impl Drop for DogfoodScratch {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        let base = self.root.parent().expect("scratch parent");
        common::make_tree_writable(base);
        let _ = fs::remove_dir_all(base);
    }
}

#[derive(Debug, PartialEq)]
struct PhaseSummary {
    phase_pid: u64,
    jetpack_pid: u64,
    probe: JSONValue,
    du: JSONValue,
}

struct EnvContract {
    all_packages: Vec<String>,
    probe_packages: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct ReceiptIdentity {
    id: String,
    name: String,
    version: String,
    reference: String,
    out: String,
    output_hash: String,
    references: Vec<String>,
    named_outputs: BTreeMap<String, String>,
    producer_record: String,
    receipt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalUse {
    unique_bytes: u64,
    shared_bytes: u64,
    total_bytes: u64,
}
