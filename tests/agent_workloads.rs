//! #769: executable AI-agent workload conformance corpus.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const HEADER: &str = "version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards";
const PROCESS_DEADLINE: Duration = Duration::from_secs(120);
const EXPECTED_TASKS: &[(&str, &str, &str, &str)] = &[(
    "repository-marker-scan",
    "repository-search-and-edit",
    "success",
    "exit=0;stdout=exact",
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
    evidence: String,
    tower_card: String,
    loss_cards: String,
}

#[derive(Debug)]
struct Measurement {
    adapter: &'static str,
    source_tokens: usize,
    cold: Duration,
    warm: Duration,
    version: String,
    cold_stderr_bytes: usize,
    cold_stderr_sha256: String,
    warm_stderr_bytes: usize,
    warm_stderr_sha256: String,
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(prefix: &str) -> Self {
        let path = common::unique_tmp(prefix);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct BoundedOutput {
    output: Output,
    elapsed: Duration,
    timed_out: bool,
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
            assert_eq!(fields.len(), 13, "bad agent workload manifest row: {line}");
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
                evidence: fields[10].into(),
                tower_card: fields[11].into(),
                loss_cards: fields[12].into(),
            }
        })
        .collect()
}

fn run_bounded(mut command: Command, label: &str, deadline: Duration) -> BoundedOutput {
    let capture = Scratch::new("jet_agent_process_output");
    let stdout_path = capture.path.join("stdout");
    let stderr_path = capture.path.join("stderr");
    command
        .stdout(Stdio::from(fs::File::create(&stdout_path).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr_path).unwrap()));
    let started = Instant::now();
    let mut child = command
        .spawn()
        .unwrap_or_else(|err| panic!("cannot start `{label}`: {err}"));

    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break (child.wait().unwrap_or(status), false);
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            break (child.wait().unwrap(), true);
        }
        thread::sleep(Duration::from_millis(5));
    };
    BoundedOutput {
        output: Output {
            status,
            stdout: fs::read(stdout_path).unwrap(),
            stderr: fs::read(stderr_path).unwrap(),
        },
        elapsed: started.elapsed(),
        timed_out,
    }
}

fn command_version(program: &Path, arg: &str, label: &str) -> String {
    let mut command = Command::new(program);
    command.arg(arg);
    let bounded = run_bounded(command, label, PROCESS_DEADLINE);
    assert!(!bounded.timed_out, "`{label}` version timed out");
    assert!(bounded.output.status.success(), "`{label} {arg}` failed");
    let text = if bounded.output.stdout.is_empty() {
        &bounded.output.stderr
    } else {
        &bounded.output.stdout
    };
    String::from_utf8_lossy(text)
        .lines()
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string()
}

fn adapter_command(
    adapter: &'static str,
    source: &Path,
    jet_cli: &Path,
    input: &Path,
    scratch: &Path,
) -> Command {
    let mut command = match adapter {
        "jet" => {
            let mut cmd = Command::new(jet_cli);
            cmd.args(["run", "--release"]).arg(source).arg("--");
            cmd
        }
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

fn fixture_files(root: &Path) -> BTreeSet<String> {
    fn walk(corpus: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(corpus, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(corpus)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut files = BTreeSet::new();
    for name in ["inputs", "expected"] {
        let dir = root.join(name);
        if dir.is_dir() {
            walk(root, &dir, &mut files);
        }
    }
    files
}

fn verify_checksum_closure(root: &Path, sums: &str) -> Result<usize, String> {
    let mut declared = BTreeMap::new();
    for line in sums.lines().filter(|line| !line.is_empty()) {
        let (hash, relative) = line
            .split_once("  ")
            .ok_or_else(|| format!("bad SHA256SUMS row: {line}"))?;
        if Path::new(relative).is_absolute()
            || relative.split('/').any(|part| part == ".." || part.is_empty())
        {
            return Err(format!("invalid checksum path: {relative}"));
        }
        if declared.insert(relative.to_string(), hash.to_string()).is_some() {
            return Err(format!("duplicate checksum path: {relative}"));
        }
    }
    let actual = fixture_files(root);
    let listed = declared.keys().cloned().collect::<BTreeSet<_>>();
    if actual != listed {
        let unhashed = actual.difference(&listed).cloned().collect::<Vec<_>>();
        let missing = listed.difference(&actual).cloned().collect::<Vec<_>>();
        return Err(format!(
            "checksum closure mismatch; unhashed={unhashed:?}; missing={missing:?}"
        ));
    }
    for (relative, hash) in &declared {
        let bytes = fs::read(root.join(relative)).map_err(|err| err.to_string())?;
        let actual_hash = jet::SHA256::sha256_hex(&bytes);
        if actual_hash != *hash {
            return Err(format!("fixture drift: {relative}"));
        }
    }
    Ok(declared.len())
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
            "argv=input-root;cwd=scratch;host=ambient;network=unmeasured;external-write=unmeasured"
        );
        assert_eq!(task.adapters, "jet,bash,python,node");
        assert_eq!(
            task.platforms,
            "linux=native;macos=native;windows=unavailable:bash-not-native"
        );
        assert_eq!(
            task.evidence,
            "tests/agent_workloads.rs::equivalent_adapters_complete_repository_marker_scan"
        );
        assert_eq!(task.tower_card, "#769");
        assert_eq!(task.loss_cards, "default-run=#688;wall-time=#666");
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
    let verified = verify_checksum_closure(&corpus_root(), &sums).unwrap();
    assert_eq!(verified, 4, "all inputs and declared outputs must be frozen");
}

#[test]
fn checksum_closure_rejects_an_extra_fixture() {
    let scratch = Scratch::new("jet_agent_checksum_closure");
    fs::create_dir_all(scratch.path.join("inputs")).unwrap();
    fs::create_dir_all(scratch.path.join("expected")).unwrap();
    fs::write(scratch.path.join("inputs/task.txt"), "input").unwrap();
    fs::write(scratch.path.join("expected/task.out"), "output").unwrap();
    let sums = format!(
        "{}  inputs/task.txt\n{}  expected/task.out\n",
        jet::SHA256::sha256_hex(b"input"),
        jet::SHA256::sha256_hex(b"output")
    );
    assert_eq!(verify_checksum_closure(&scratch.path, &sums), Ok(2));
    fs::write(scratch.path.join("inputs/unhashed.txt"), "extra").unwrap();
    let error = verify_checksum_closure(&scratch.path, &sums).unwrap_err();
    assert!(error.contains("unhashed") && error.contains("inputs/unhashed.txt"), "{error}");
}

#[test]
fn process_deadline_reaps_and_scratch_drop_cleans() {
    let scratch_path;
    {
        let scratch = Scratch::new("jet_agent_process_deadline");
        scratch_path = scratch.path.clone();
        fs::write(scratch.path.join("sentinel"), "cleanup").unwrap();
        let mut command = Command::new("python3");
        command.args(["-c", "import time; time.sleep(10)"]);
        let bounded = run_bounded(command, "timeout regression", Duration::from_millis(25));
        assert!(bounded.timed_out, "deadline did not stop the process");
        assert!(
            bounded.elapsed < Duration::from_secs(2),
            "deadline cleanup took too long: {:?}",
            bounded.elapsed
        );
    }
    assert!(!scratch_path.exists(), "scratch directory survived Drop");
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
    let jet_cli = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let jet_artifact = jet::SHA256::sha256_hex(&fs::read(&jet_cli).unwrap());
    let versions = BTreeMap::from([
        ("jet", command_version(&jet_cli, "--version", "jet")),
        ("bash", command_version(Path::new("bash"), "--version", "bash")),
        (
            "python",
            command_version(Path::new("python3"), "--version", "python3"),
        ),
        ("node", command_version(Path::new("node"), "--version", "node")),
    ]);

    let mut measurements = Vec::new();
    let mut declared_outputs = Vec::new();
    for &(adapter, source_name) in ADAPTERS {
        let scratch = Scratch::new("jet_agent_workload");
        let source = corpus_root().join("adapters").join(source_name);
        let cold = run_bounded(
            adapter_command(
                adapter,
                &source,
                &jet_cli,
                &input,
                &scratch.path,
            ),
            adapter,
            PROCESS_DEADLINE,
        );
        let warm = run_bounded(
            adapter_command(
                adapter,
                &source,
                &jet_cli,
                &input,
                &scratch.path,
            ),
            adapter,
            PROCESS_DEADLINE,
        );
        assert!(!cold.timed_out, "{adapter} cold run timed out");
        assert!(!warm.timed_out, "{adapter} warm run timed out");
        assert_eq!(
            cold.output.status.code(),
            Some(0),
            "{adapter} cold exit drifted:\n{}",
            String::from_utf8_lossy(&cold.output.stderr)
        );
        assert_eq!(cold.output.stdout, expected, "{adapter} cold stdout drifted");
        assert_eq!(
            warm.output.status.code(),
            Some(0),
            "{adapter} warm exit drifted"
        );
        assert_eq!(
            warm.output.stdout, cold.output.stdout,
            "{adapter} output was unstable"
        );
        assert_eq!(
            tree_hashes(&input),
            before,
            "{adapter} changed its read-only input authority"
        );
        declared_outputs.push(cold.output.stdout);
        measurements.push(Measurement {
            adapter,
            source_tokens: source_tokens(&source),
            cold: cold.elapsed,
            warm: warm.elapsed,
            version: versions[adapter].clone(),
            cold_stderr_bytes: cold.output.stderr.len(),
            cold_stderr_sha256: jet::SHA256::sha256_hex(&cold.output.stderr),
            warm_stderr_bytes: warm.output.stderr.len(),
            warm_stderr_sha256: jet::SHA256::sha256_hex(&warm.output.stderr),
        });
    }
    assert!(
        declared_outputs.windows(2).all(|pair| pair[0] == pair[1]),
        "adapters disagreed on the declared outcome"
    );

    println!(
        "machine\tos={}\tarch={}\tcorpus=1\ttask={}\tevidence={}\tcard={}\tlosses=red:{}\tjet_artifact={}\tjet_sha256={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        task.id,
        task.evidence,
        task.tower_card,
        task.loss_cards,
        jet_cli.display(),
        jet_artifact
    );
    for result in &measurements {
        println!(
            "result\tadapter={}\tsuccess=true\tsource_tokens={}\tcold_ns={}\twarm_ns={}\toutput_stable=true\tversion={}\tcold_stderr_bytes={}\tcold_stderr_sha256={}\twarm_stderr_bytes={}\twarm_stderr_sha256={}\tagent_tool_calls=not-recorded:#769\trepair_turns=not-recorded:#769\tpeak_memory=not-recorded:#769\tdiagnostic_quality=not-recorded:#769\torphan_processes=not-recorded:#769\tsandbox_escapes=not-recorded:#769\tnetwork=unmeasured:#769\texternal_writes=unmeasured:#769\tcross_platform=not-run:#769",
            result.adapter,
            result.source_tokens,
            result.cold.as_nanos(),
            result.warm.as_nanos(),
            result.version.replace('\t', " "),
            result.cold_stderr_bytes,
            result.cold_stderr_sha256,
            result.warm_stderr_bytes,
            result.warm_stderr_sha256
        );
    }
}
