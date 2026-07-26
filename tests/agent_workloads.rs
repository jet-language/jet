//! #769: executable AI-agent workload conformance corpus.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const HEADER: &str = "version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tproof\ttower_card";
const EXPECTED_TASKS: &[(&str, &str, &str, &str)] = &[(
    "repository-marker-scan",
    "repository-search-and-edit",
    "success",
    "exit=0;stdout=exact;stderr=empty",
)];
const ADAPTERS: &[(&str, &str)] = &[
    ("jet", "repository_marker_scan.jet"),
    ("bash", "repository_marker_scan.bash"),
    ("python", "repository_marker_scan.py"),
    ("node", "repository_marker_scan.mjs"),
];

#[derive(Debug)]
struct Task {
    id: String,
    domain: String,
    case: String,
    outcome: String,
    input: String,
    expected: String,
    authority: String,
    adapters: String,
    platforms: String,
    proof: String,
    tower_card: String,
}

#[derive(Debug)]
struct Measurement {
    adapter: &'static str,
    source_tokens: usize,
    cold: Duration,
    warm: Duration,
    version: String,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/agent_workloads")
}

fn read_tasks() -> Vec<Task> {
    let manifest = fs::read_to_string(corpus_root().join("manifest.tsv")).unwrap();
    let mut lines = manifest.lines();
    assert_eq!(lines.next(), Some(HEADER), "agent workload manifest schema drifted");
    lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 12, "bad agent workload manifest row: {line}");
            assert_eq!(fields[0], "1", "unsupported corpus version in: {line}");
            Task {
                id: fields[1].into(),
                domain: fields[2].into(),
                case: fields[3].into(),
                outcome: fields[4].into(),
                input: fields[5].into(),
                expected: fields[6].into(),
                authority: fields[7].into(),
                adapters: fields[8].into(),
                platforms: fields[9].into(),
                proof: fields[10].into(),
                tower_card: fields[11].into(),
            }
        })
        .collect()
}

fn command_version(program: &str, arg: &str) -> String {
    let output = Command::new(program)
        .arg(arg)
        .output()
        .unwrap_or_else(|err| panic!("required native adapter `{program}` unavailable: {err}"));
    assert!(output.status.success(), "`{program} {arg}` failed");
    let text = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(text)
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string()
}

fn compile_jet_adapter(source: &Path, scratch: &Path) -> PathBuf {
    assert!(common::have_rustc(), "rustc is required for the Jet adapter");
    let text = fs::read_to_string(source).unwrap();
    let shown = source.to_string_lossy();
    let compiled = jet::compile_with_path(&text, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected Jet adapter:\n{}",
            jet::render_diagnostics(&shown, &text, &diags)
        )
    });
    let rust = scratch.join("repository_marker_scan.rs");
    let binary = scratch.join(format!(
        "repository_marker_scan{}",
        std::env::consts::EXE_SUFFIX
    ));
    fs::write(&rust, compiled.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        rust.to_str().unwrap(),
        "-o",
        binary.to_str().unwrap(),
    ]);
    if let Some(link) = &compiled.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc.arg("-L").arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let output = rustc.output().unwrap();
    assert!(
        output.status.success(),
        "rustc rejected generated Jet adapter (I2):\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn adapter_command(
    adapter: &'static str,
    source: &Path,
    jet_binary: &Path,
    input: &Path,
    scratch: &Path,
) -> Command {
    let mut command = match adapter {
        "jet" => Command::new(jet_binary),
        "bash" => {
            let mut cmd = Command::new("bash");
            cmd.arg(source);
            cmd
        }
        "python" => {
            let mut cmd = Command::new("python3");
            cmd.arg(source);
            cmd
        }
        "node" => {
            let mut cmd = Command::new("node");
            cmd.arg(source);
            cmd
        }
        other => panic!("unknown adapter {other}"),
    };
    command.arg(input).current_dir(scratch);
    command
}

fn timed_output(mut command: Command) -> (Output, Duration) {
    let started = Instant::now();
    let output = command.output().unwrap();
    (output, started.elapsed())
}

fn tree_hashes(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(relative, jet::SHA256::sha256_hex(&fs::read(path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn source_tokens(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap()
        .split_whitespace()
        .count()
}

#[test]
fn manifest_is_complete_frozen_and_non_vacuous() {
    let tasks = read_tasks();
    let actual = tasks
        .iter()
        .map(|task| {
            (
                task.id.as_str(),
                task.domain.as_str(),
                task.case.as_str(),
                task.outcome.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, EXPECTED_TASKS, "task removed, added, or reclassified");

    let mut ids = BTreeSet::new();
    for task in tasks {
        assert!(ids.insert(task.id.clone()), "duplicate task ID {}", task.id);
        assert_eq!(
            task.authority,
            "argv=input-root;cwd=sandbox;env=inherited;network=unused;write=none"
        );
        assert_eq!(task.adapters, "jet,bash,python,node");
        assert_eq!(
            task.platforms,
            "linux=native;macos=native;windows=unavailable:bash-not-native"
        );
        assert_eq!(
            task.proof,
            "tests/agent_workloads.rs::equivalent_adapters_complete_repository_marker_scan"
        );
        assert_eq!(task.tower_card, "#769");
        assert!(corpus_root().join(&task.input).is_dir());
        assert!(corpus_root().join(&task.expected).is_file());
        for (_, source) in ADAPTERS {
            assert!(
                corpus_root().join("adapters").join(source).is_file(),
                "missing adapter {source}"
            );
        }
    }

    let sums = fs::read_to_string(corpus_root().join("SHA256SUMS")).unwrap();
    let mut verified = 0;
    for line in sums.lines().filter(|line| !line.is_empty()) {
        let (hash, relative) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("bad SHA256SUMS row: {line}"));
        let bytes = fs::read(corpus_root().join(relative)).unwrap();
        assert_eq!(jet::SHA256::sha256_hex(&bytes), hash, "fixture drift: {relative}");
        verified += 1;
    }
    assert_eq!(verified, 4, "all inputs and declared outputs must be frozen");
}

#[test]
fn equivalent_adapters_complete_repository_marker_scan() {
    if std::env::consts::OS == "windows" {
        panic!("Bash is explicitly unavailable as a native Windows adapter; this task cannot pass");
    }

    let task = read_tasks().into_iter().next().unwrap();
    let input = corpus_root().join(&task.input);
    let expected = fs::read(corpus_root().join(&task.expected)).unwrap();
    let before = tree_hashes(&input);
    let scratch = common::unique_tmp("jet_agent_workload");
    fs::create_dir_all(&scratch).unwrap();
    let jet_source = corpus_root()
        .join("adapters")
        .join("repository_marker_scan.jet");
    let jet_binary = compile_jet_adapter(&jet_source, &scratch);
    let versions = BTreeMap::from([
        ("jet", format!("jet-test-{}", env!("CARGO_PKG_VERSION"))),
        ("bash", command_version("bash", "--version")),
        ("python", command_version("python3", "--version")),
        ("node", command_version("node", "--version")),
    ]);

    let mut measurements = Vec::new();
    let mut declared_outputs = Vec::new();
    for &(adapter, source_name) in ADAPTERS {
        let source = corpus_root().join("adapters").join(source_name);
        let (cold, cold_time) = timed_output(adapter_command(
            adapter,
            &source,
            &jet_binary,
            &input,
            &scratch,
        ));
        let (warm, warm_time) = timed_output(adapter_command(
            adapter,
            &source,
            &jet_binary,
            &input,
            &scratch,
        ));
        assert_eq!(
            cold.status.code(),
            Some(0),
            "{adapter} cold exit drifted:\n{}",
            String::from_utf8_lossy(&cold.stderr)
        );
        assert_eq!(cold.stdout, expected, "{adapter} cold stdout drifted");
        assert!(cold.stderr.is_empty(), "{adapter} cold stderr was not empty");
        assert_eq!(warm.status.code(), Some(0), "{adapter} warm exit drifted");
        assert_eq!(warm.stdout, cold.stdout, "{adapter} output was unstable");
        assert!(warm.stderr.is_empty(), "{adapter} warm stderr was not empty");
        assert_eq!(
            tree_hashes(&input),
            before,
            "{adapter} changed its read-only input authority"
        );
        declared_outputs.push(cold.stdout);
        measurements.push(Measurement {
            adapter,
            source_tokens: source_tokens(&source),
            cold: cold_time,
            warm: warm_time,
            version: versions[adapter].clone(),
        });
    }
    assert!(
        declared_outputs.windows(2).all(|pair| pair[0] == pair[1]),
        "adapters disagreed on the declared outcome"
    );

    println!(
        "machine\tos={}\tarch={}\tcorpus=1\ttask={}\tproof={}\tcard={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        task.id,
        task.proof,
        task.tower_card
    );
    for result in &measurements {
        println!(
            "result\tadapter={}\tsuccess=true\tsource_tokens={}\tcold_ns={}\twarm_ns={}\toutput_stable=true\tversion={}\tagent_tool_calls=not-recorded:#769\trepair_turns=not-recorded:#769\tpeak_memory=not-recorded:#769\tdiagnostic_quality=not-recorded:#769\torphan_processes=not-recorded:#769\tsandbox_escapes=not-recorded:#769\tcross_platform=not-run:#769",
            result.adapter,
            result.source_tokens,
            result.cold.as_nanos(),
            result.warm.as_nanos(),
            result.version.replace('\t', " ")
        );
    }

    fs::remove_dir_all(&scratch).unwrap();
}
