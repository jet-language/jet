//! #769: executable AI-agent workload conformance corpus.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const HEADER: &str = "version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards";
const DOMAIN_CONTRACT_HEADER: &str =
    "version\ttask_id\tallowed_dependencies\tmachine_spec\tvariant\tscoring";
const PROCESS_DEADLINE: Duration = Duration::from_secs(120);
const DOMAIN_SCORING: &str =
    "#769:v1;exit=0;stdout=exact;cold=recorded;warm=equal;input=unchanged;scratch=closed";
const SANDBOX_HOSTILE_CORPUS: &str = include_str!("fixtures/build_sandbox/hostile-corpus.tsv");
const EXPECTED_TASKS: &[(&str, &str, &str, &str)] = &[
    (
        "repository-marker-scan",
        "repository-search-and-edit",
        "success",
        "exit=0;stdout=exact",
    ),
    (
        "repository-marker-scan-empty",
        "repository-search-and-edit",
        "hostile-empty",
        "exit=0;stdout=exact",
    ),
    (
        "repository-semantic-inspection",
        "repository-search-and-edit",
        "semantic-index",
        "exit=0;stdout=exact",
    ),
    (
        "repository-semantic-edit",
        "repository-search-and-edit",
        "semantic-rename",
        "exit=0;stdout=exact",
    ),
    (
        "git-diff-review",
        "build-test-debug-and-git",
        "mixed-change",
        "exit=0;stdout=exact",
    ),
    (
        "git-diff-empty",
        "build-test-debug-and-git",
        "hostile-empty-diff",
        "exit=0;stdout=exact",
    ),
    (
        "build-test-failure-recovery",
        "build-test-debug-and-git",
        "compile-check-recovery",
        "exit=0;stdout=exact",
    ),
    (
        "incident-report-success",
        "data-cleanup-and-report-generation",
        "success",
        "exit=0;stdout=exact",
    ),
    (
        "incident-report-malformed",
        "data-cleanup-and-report-generation",
        "malformed-input",
        "exit=0;stdout=exact",
    ),
    (
        "incident-report-partial",
        "data-cleanup-and-report-generation",
        "partial-failure",
        "exit=0;stdout=exact",
    ),
    (
        "structured-data-transform",
        "structured-data",
        "json-normalize",
        "exit=0;stdout=exact",
    ),
    (
        "structured-data-hostile",
        "structured-data",
        "malformed-json",
        "exit=0;stdout=exact",
    ),
    (
        "database-access",
        "databases",
        "parameterized-query",
        "exit=0;stdout=exact",
    ),
    (
        "database-hostile",
        "databases",
        "invalid-row",
        "exit=0;stdout=exact",
    ),
    (
        "http-api",
        "http-apis",
        "loopback-json-post",
        "exit=0;stdout=exact",
    ),
    (
        "http-hostile",
        "http-apis",
        "rejected-payload",
        "exit=0;stdout=exact",
    ),
    (
        "process-batch-success",
        "long-running-and-interactive-commands",
        "success",
        "exit=0;stdout=exact",
    ),
    (
        "process-batch-large-stderr",
        "long-running-and-interactive-commands",
        "large-stderr",
        "exit=0;stdout=exact",
    ),
    (
        "process-batch-timeout-recovery",
        "long-running-and-interactive-commands",
        "timeout-cancellation-cleanup",
        "exit=0;stdout=exact",
    ),
    (
        "browser-automation-preflight",
        "browser-and-desktop-work",
        "profile-timeout-hostile",
        "exit=0;stdout=exact",
    ),
    (
        "desktop-interaction-focus",
        "browser-and-desktop-work",
        "keyboard-focus-hostile",
        "exit=0;stdout=exact",
    ),
    (
        "document-markdown-inspection",
        "document-and-media-work",
        "malformed-document",
        "exit=0;stdout=exact",
    ),
    (
        "media-asset-inventory",
        "document-and-media-work",
        "unsupported-media-hostile",
        "exit=0;stdout=exact",
    ),
    (
        "mcp-environment-readonly",
        "mcp-tools-and-hooks",
        "readonly-resource",
        "exit=0;stdout=exact",
    ),
    (
        "mcp-environment-denied",
        "mcp-tools-and-hooks",
        "hostile-denied-resource",
        "exit=0;stdout=exact",
    ),
    (
        "interactive-terminal-dialogue",
        "interactive-terminals",
        "pty-dialogue",
        "exit=0;stdout=exact",
    ),
    (
        "interactive-terminal-closed",
        "interactive-terminals",
        "hostile-closed-session",
        "exit=0;stdout=exact",
    ),
    (
        "service-lifecycle-roundtrip",
        "service-lifecycle",
        "up-health-wait-logs-down",
        "exit=0;stdout=exact",
    ),
    (
        "service-lifecycle-readiness-timeout",
        "service-lifecycle",
        "hostile-readiness-timeout",
        "exit=0;stdout=exact",
    ),
];
const ADAPTERS: &[(&str, &str)] = &[
    ("jet", "jet"),
    ("bash", "bash"),
    ("python", "py"),
    ("node", "mjs"),
];
const EXPECTED_DOMAIN_CONTRACT: &[(&str, &str, &str, &str)] = &[
    (
        "structured-data-transform",
        "jet=Core:encoding.json,files,process;bash=jq;python=stdlib-json;node=stdlib-fs-json",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "structured-data-hostile",
        "jet=Core:encoding.json,files,process;bash=jq;python=stdlib-json;node=stdlib-fs-json",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "database-access",
        "jet=Core:db,files,process+bundled-SQLite;bash=python3-stdlib-sqlite3;python=stdlib-sqlite3;node=node:sqlite",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "database-hostile",
        "jet=Core:db,files,process+bundled-SQLite;bash=python3-stdlib-sqlite3;python=stdlib-sqlite3;node=node:sqlite",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "http-api",
        "jet=Core:http,http.client,http.server,net,files,process;bash=curl+python3-http.server;python=stdlib-urllib+http.server;node=stdlib-http-fs",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "http-hostile",
        "jet=Core:http,http.client,http.server,net,files,process;bash=curl+python3-http.server;python=stdlib-urllib+http.server;node=stdlib-http-fs",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "browser-automation-preflight",
        "jet=Core:web.browser,files,process;bash=bash-stdlib;python=stdlib-pathlib;node=stdlib-fs",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "desktop-interaction-focus",
        "jet=Core:ui,files,process;bash=bash-stdlib;python=stdlib-pathlib;node=stdlib-fs",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "document-markdown-inspection",
        "jet=Core:files,process;bash=find,awk;python=stdlib-pathlib;node=stdlib-fs-path",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "media-asset-inventory",
        "jet=Core:files,net.mime,process;bash=find,wc;python=stdlib-pathlib;node=stdlib-fs-path",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "mcp-environment-readonly",
        "jet=Core:process,sys;bash=jet-cli;python=jet-cli;node=jet-cli",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "mcp-environment-denied",
        "jet=Core:process,sys;bash=jet-cli;python=jet-cli;node=jet-cli",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "interactive-terminal-dialogue",
        "jet=Core:process,terminal,sh;bash=script,timeout,sh;python=script,stdlib-subprocess;node=script,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:util-linux-script-unavailable;windows-native:unavailable",
        "normal",
    ),
    (
        "interactive-terminal-closed",
        "jet=Core:process,terminal,sh;bash=script,timeout,sh;python=script,stdlib-subprocess;node=script,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:util-linux-script-unavailable;windows-native:unavailable",
        "hostile",
    ),
    (
        "service-lifecycle-roundtrip",
        "jet=Core:process,files,sys,jetpack-cli,systemd-shim;bash=jetpack-cli,systemd-shim,coreutils;python=jetpack-cli,systemd-shim,stdlib-subprocess;node=jetpack-cli,systemd-shim,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:E1332-service-authority;windows-native:unavailable",
        "normal",
    ),
    (
        "service-lifecycle-readiness-timeout",
        "jet=Core:process,files,sys,jetpack-cli,systemd-shim;bash=jetpack-cli,systemd-shim,coreutils;python=jetpack-cli,systemd-shim,stdlib-subprocess;node=jetpack-cli,systemd-shim,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:E1332-service-authority;windows-native:unavailable",
        "hostile",
    ),
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
struct DomainContract {
    task_id: String,
    allowed_dependencies: String,
    machine_spec: String,
    variant: String,
    scoring: String,
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

fn adapter_stem(task_id: &str) -> &'static str {
    if task_id == "repository-marker-scan" || task_id == "repository-marker-scan-empty" {
        "repository_marker_scan"
    } else if task_id == "repository-semantic-inspection" {
        "repository_semantic_inspection"
    } else if task_id == "repository-semantic-edit" {
        "repository_semantic_edit"
    } else if task_id == "git-diff-review" || task_id == "git-diff-empty" {
        "git_diff_review"
    } else if task_id == "build-test-failure-recovery" {
        "build_test_recovery"
    } else if task_id.starts_with("incident-report-") {
        "incident_report"
    } else if task_id.starts_with("structured-data-") {
        "structured_data"
    } else if task_id.starts_with("database-") {
        "database_access"
    } else if task_id.starts_with("http-") {
        "http_api"
    } else if task_id.starts_with("process-batch-") {
        "process_batch"
    } else if task_id.starts_with("mcp-environment-") {
        "mcp_resource"
    } else if task_id.starts_with("interactive-terminal-") {
        "interactive_terminal"
    } else if task_id.starts_with("service-lifecycle-") {
        "service_lifecycle"
    } else if task_id == "browser-automation-preflight" {
        "browser_automation_preflight"
    } else if task_id == "desktop-interaction-focus" {
        "desktop_interaction_focus"
    } else if task_id == "document-markdown-inspection" {
        "document_markdown_inspection"
    } else if task_id == "media-asset-inventory" {
        "media_asset_inventory"
    } else {
        panic!("task has no adapter source: {task_id}")
    }
}

fn read_domain_contract() -> Vec<DomainContract> {
    let contract = fs::read_to_string(corpus_root().join("domain_contract.tsv")).unwrap();
    let mut lines = contract.lines();
    assert_eq!(
        lines.next(),
        Some(DOMAIN_CONTRACT_HEADER),
        "agent workload domain contract schema drifted"
    );
    lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "bad domain contract row: {line}");
            assert_eq!(fields[0], "1", "unsupported domain contract version: {line}");
            assert_eq!(fields[5], DOMAIN_SCORING, "domain scoring drifted: {line}");
            DomainContract {
                task_id: fields[1].into(),
                allowed_dependencies: fields[2].into(),
                machine_spec: fields[3].into(),
                variant: fields[4].into(),
                scoring: fields[5].into(),
            }
        })
        .collect()
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
    task_id: &str,
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
    command
        .arg(input)
        .env("JET_CORPUS_JET", jet_cli)
        .env("JET_CORPUS_JETPACK", common::jetpack_bin())
        .env("JET_CORPUS_TASK", task_id)
        .current_dir(scratch);
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

fn input_hashes(input: &Path) -> BTreeMap<String, String> {
    if input.is_dir() {
        return tree_hashes(input);
    }
    BTreeMap::from([(
        input.file_name().unwrap().to_string_lossy().into_owned(),
        jet::SHA256::sha256_hex(&fs::read(input).unwrap()),
    )])
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

fn scratch_output_violations(
    adapter: &str,
    path: &Path,
    jet_artifact_name: &str,
) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, entries: &mut BTreeMap<String, &'static str>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let file_type = fs::symlink_metadata(&path).unwrap().file_type();
            let kind = if file_type.is_dir() {
                "directory"
            } else if file_type.is_file() {
                "file"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "other"
            };
            entries.insert(relative, kind);
            if file_type.is_dir() {
                walk(root, &path, entries);
            }
        }
    }

    let mut actual = BTreeMap::new();
    walk(path, path, &mut actual);
    let (required, optional) = if adapter == "jet" {
        (
            BTreeMap::from([
                ("build".to_string(), "directory"),
                (format!("build/{jet_artifact_name}"), "file"),
            ]),
            BTreeMap::from([(format!("build/{jet_artifact_name}.rs"), "file")]),
        )
    } else {
        (BTreeMap::new(), BTreeMap::new())
    };
    let mut violations = actual
        .iter()
        .filter(|(entry, kind)| {
            required.get(*entry) != Some(*kind) && optional.get(*entry) != Some(*kind)
        })
        .map(|(entry, kind)| format!("unexpected {kind}: {entry}"))
        .collect::<Vec<_>>();
    violations.extend(
        required
            .iter()
            .filter(|(entry, kind)| actual.get(*entry) != Some(*kind))
            .map(|(entry, kind)| format!("missing {kind}: {entry}")),
    );
    violations.sort();
    violations
}

#[test]
fn manifest_is_complete_frozen_and_non_vacuous() {
    let hostile_cases = SANDBOX_HOSTILE_CORPUS
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(hostile_cases.len(), 7);
    for row in &hostile_cases {
        assert_eq!(row.len(), 4, "malformed shared sandbox hostile corpus row");
        assert_eq!(row[3], "blocked-or-unsupported");
    }
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
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    for task in &tasks {
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
            "tests/agent_workloads.rs::equivalent_adapters_complete_declared_tasks"
        );
        assert_eq!(task.tower_card, "#769");
        assert_eq!(task.loss_cards, "default-run=#688;wall-time=#666");
        assert!(corpus_root().join(&task.input).exists());
        assert!(corpus_root().join(&task.expected).is_file());
        if matches!(
            task.id.as_str(),
            "browser-automation-preflight"
                | "desktop-interaction-focus"
                | "document-markdown-inspection"
                | "media-asset-inventory"
                | "mcp-environment-denied"
                | "interactive-terminal-closed"
                | "service-lifecycle-readiness-timeout"
        ) {
            let expected = fs::read_to_string(corpus_root().join(&task.expected)).unwrap();
            let hostile = if task.id == "desktop-interaction-focus" {
                expected.lines().any(|line| line == "event|Empty|observed")
            } else {
                expected.lines().any(|line| line.starts_with("reject|"))
            };
            assert!(hostile, "target task {} lost its hostile variant", task.id);
        }
        let stem = adapter_stem(&task.id);
        for (_, extension) in ADAPTERS {
            let source = format!("{stem}.{extension}");
            assert!(
                corpus_root().join("adapters").join(&source).is_file(),
                "missing adapter {source}"
            );
        }
    }

    let contracts = read_domain_contract();
    let actual_contract = contracts
        .iter()
        .map(|contract| {
            (
                contract.task_id.as_str(),
                contract.allowed_dependencies.as_str(),
                contract.machine_spec.as_str(),
                contract.variant.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_contract, EXPECTED_DOMAIN_CONTRACT,
        "domain contract changed without a frozen task review"
    );
    for contract in contracts {
        assert!(
            task_ids.contains(contract.task_id.as_str()),
            "domain contract names unknown task {}",
            contract.task_id
        );
        assert_eq!(contract.scoring, DOMAIN_SCORING);
    }

    let sums = fs::read_to_string(corpus_root().join("SHA256SUMS")).unwrap();
    let verified = verify_checksum_closure(&corpus_root(), &sums).unwrap();
    assert_eq!(verified, 63, "all inputs and declared outputs must be frozen");
}

#[test]
fn checksum_closure_rejects_drift_and_hostile_sums() {
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
    fs::remove_file(scratch.path.join("inputs/unhashed.txt")).unwrap();

    let input_hash = jet::SHA256::sha256_hex(b"input");
    let output_hash = jet::SHA256::sha256_hex(b"output");

    // A drifted byte in a declared fixture names the fixture.
    fs::write(scratch.path.join("inputs/task.txt"), "drifted").unwrap();
    let error = verify_checksum_closure(&scratch.path, &sums).unwrap_err();
    assert_eq!(error, "fixture drift: inputs/task.txt");
    fs::write(scratch.path.join("inputs/task.txt"), "input").unwrap();

    // A removed declared fixture is reported as missing, never as a pass.
    fs::remove_file(scratch.path.join("expected/task.out")).unwrap();
    let error = verify_checksum_closure(&scratch.path, &sums).unwrap_err();
    assert!(error.contains("missing") && error.contains("expected/task.out"), "{error}");
    fs::write(scratch.path.join("expected/task.out"), "output").unwrap();
    assert_eq!(verify_checksum_closure(&scratch.path, &sums), Ok(2));

    // A hostile SHA256SUMS cannot smuggle a path out of the corpus root, hide a
    // fixture behind a duplicate row, or pass a row the reader cannot split.
    for (hostile, reason) in [
        (
            format!("{input_hash}  /etc/passwd\n{output_hash}  expected/task.out\n"),
            "invalid checksum path: /etc/passwd".to_string(),
        ),
        (
            format!("{input_hash}  ../outside.txt\n{output_hash}  expected/task.out\n"),
            "invalid checksum path: ../outside.txt".to_string(),
        ),
        (
            format!("{input_hash}  inputs//task.txt\n{output_hash}  expected/task.out\n"),
            "invalid checksum path: inputs//task.txt".to_string(),
        ),
        (
            format!(
                "{input_hash}  inputs/task.txt\n{output_hash}  inputs/task.txt\n{output_hash}  expected/task.out\n"
            ),
            "duplicate checksum path: inputs/task.txt".to_string(),
        ),
        (
            format!("{input_hash} inputs/task.txt\n{output_hash}  expected/task.out\n"),
            format!("bad SHA256SUMS row: {input_hash} inputs/task.txt"),
        ),
    ] {
        assert_eq!(verify_checksum_closure(&scratch.path, &hostile), Err(reason));
    }
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
fn scratch_output_shape_rejects_arbitrary_build_residue() {
    fn valid_jet_scratch(prefix: &str) -> Scratch {
        let scratch = Scratch::new(prefix);
        fs::create_dir(scratch.path.join("build")).unwrap();
        fs::write(scratch.path.join("build/process_batch"), "declared artifact").unwrap();
        scratch
    }

    let cache_hit = valid_jet_scratch("jet_agent_scratch_cache_hit");
    assert_eq!(
        scratch_output_violations("jet", &cache_hit.path, "process_batch"),
        Vec::<String>::new()
    );

    let cache_miss = valid_jet_scratch("jet_agent_scratch_cache_miss");
    fs::write(
        cache_miss.path.join("build/process_batch.rs"),
        "declared generated Rust",
    )
    .unwrap();
    assert_eq!(
        scratch_output_violations("jet", &cache_miss.path, "process_batch"),
        Vec::<String>::new()
    );

    let build_leak = valid_jet_scratch("jet_agent_scratch_build_leak");
    fs::write(build_leak.path.join("build/leak"), "undeclared residue").unwrap();
    assert_eq!(
        scratch_output_violations("jet", &build_leak.path, "process_batch"),
        vec!["unexpected file: build/leak".to_string()]
    );

    let root_leak = valid_jet_scratch("jet_agent_scratch_root_leak");
    fs::write(root_leak.path.join("leak"), "undeclared residue").unwrap();
    assert_eq!(
        scratch_output_violations("jet", &root_leak.path, "process_batch"),
        vec!["unexpected file: leak".to_string()]
    );

    let nested_leak = valid_jet_scratch("jet_agent_scratch_nested_leak");
    fs::create_dir(nested_leak.path.join("build/nested")).unwrap();
    fs::write(
        nested_leak.path.join("build/nested/leak"),
        "undeclared residue",
    )
    .unwrap();
    assert_eq!(
        scratch_output_violations("jet", &nested_leak.path, "process_batch"),
        vec![
            "unexpected directory: build/nested".to_string(),
            "unexpected file: build/nested/leak".to_string(),
        ]
    );

    let wrong_type = valid_jet_scratch("jet_agent_scratch_wrong_type");
    fs::create_dir(wrong_type.path.join("build/process_batch.rs")).unwrap();
    assert_eq!(
        scratch_output_violations("jet", &wrong_type.path, "process_batch"),
        vec!["unexpected directory: build/process_batch.rs".to_string()]
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let symlink_leak = valid_jet_scratch("jet_agent_scratch_symlink");
        symlink("missing-target", symlink_leak.path.join("build/process_batch.rs")).unwrap();
        assert_eq!(
            scratch_output_violations("jet", &symlink_leak.path, "process_batch"),
            vec!["unexpected symlink: build/process_batch.rs".to_string()]
        );
    }
}

#[test]
fn equivalent_adapters_complete_declared_tasks() {
    if std::env::consts::OS == "windows" {
        panic!("Bash is explicitly unavailable as a native Windows adapter; this task cannot pass");
    }

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
    let git_version = command_version(Path::new("git"), "--version", "git");

    for task in read_tasks() {
        let input = corpus_root().join(&task.input);
        let expected = fs::read(corpus_root().join(&task.expected)).unwrap();
        let before = input_hashes(&input);
        let stem = adapter_stem(&task.id);
        let mut measurements = Vec::new();
        let mut declared_outputs = Vec::new();
        for &(adapter, extension) in ADAPTERS {
            let scratch = Scratch::new("jet_agent_workload");
            let source = corpus_root()
                .join("adapters")
                .join(format!("{stem}.{extension}"));
            let cold = run_bounded(
                adapter_command(
                    adapter,
                    &source,
                    &jet_cli,
                    &input,
                    &scratch.path,
                    &task.id,
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
                    &task.id,
                ),
                adapter,
                PROCESS_DEADLINE,
            );
            assert!(!cold.timed_out, "{} {adapter} cold run timed out", task.id);
            assert!(!warm.timed_out, "{} {adapter} warm run timed out", task.id);
            assert_eq!(
                cold.output.status.code(),
                Some(0),
                "{} {adapter} cold exit drifted:\n{}",
                task.id,
                String::from_utf8_lossy(&cold.output.stderr)
            );
            assert_eq!(
                cold.output.stdout, expected,
                "{} {adapter} cold stdout drifted",
                task.id
            );
            assert_eq!(
                warm.output.status.code(),
                Some(0),
                "{} {adapter} warm exit drifted",
                task.id
            );
            assert_eq!(
                warm.output.stdout, cold.output.stdout,
                "{} {adapter} output was unstable",
                task.id
            );
            assert_eq!(
                input_hashes(&input),
                before,
                "{} {adapter} changed its read-only input authority",
                task.id
            );
            let jet_artifact_name = source.file_stem().unwrap().to_string_lossy();
            // This must run before Scratch::drop so adapter residue cannot be
            // mistaken for successful cleanup. Jet may leave only its exact
            // public AOT cache-hit or cache-miss shape; no adapter gets a broad
            // path exception.
            assert_eq!(
                scratch_output_violations(adapter, &scratch.path, &jet_artifact_name),
                Vec::<String>::new(),
                "{} {adapter} left undeclared scratch residue",
                task.id
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
            "{} adapters disagreed on the declared outcome",
            task.id
        );

        println!(
            "machine\tos={}\tarch={}\tcorpus=1\ttask={}\tevidence={}\tcard={}\tlosses=red:{}\tjet_artifact={}\tjet_sha256={}\tgit_version={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            task.id,
            task.evidence,
            task.tower_card,
            task.loss_cards,
            jet_cli.display(),
            jet_artifact,
            git_version.replace('\t', " ")
        );
        for result in &measurements {
            println!(
                "result\ttask={}\tadapter={}\tsuccess=true\tsource_tokens={}\tcold_ns={}\twarm_ns={}\toutput_stable=true\tversion={}\tcold_stderr_bytes={}\tcold_stderr_sha256={}\twarm_stderr_bytes={}\twarm_stderr_sha256={}\tagent_tool_calls=not-recorded:#769\trepair_turns=not-recorded:#769\tpeak_memory=not-recorded:#769\tdiagnostic_quality=not-recorded:#769\torphan_processes=not-recorded:#769\tsandbox_escapes=not-recorded:#769\tnetwork=unmeasured:#769\texternal_writes=unmeasured:#769\tcross_platform=not-run:#769",
                task.id,
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
}

#[test]
fn llm_digest_first_program() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let digest = fs::read_to_string(root.join("llms.text")).unwrap();
    let fixture = corpus_root().join("llm_digest");
    let source = fixture.join("first_program.jet");
    let expected = fs::read(fixture.join("transcript.txt")).unwrap();
    let sums = fs::read_to_string(fixture.join("SHA256SUMS")).unwrap();

    let rows = sums
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split_once("  ")
                .map(|(hash, path)| (hash, path))
                .unwrap_or_else(|| panic!("bad llm digest fixture checksum row: {line}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "llm digest fixture checksum row count drifted");
    assert_eq!(
        rows.iter().map(|(_, path)| *path).collect::<BTreeSet<_>>(),
        BTreeSet::from(["first_program.jet", "transcript.txt"]),
        "llm digest fixture checksum closure drifted"
    );
    for (hash, relative) in rows {
        let actual = jet::SHA256::sha256_hex(&fs::read(fixture.join(relative)).unwrap());
        assert_eq!(actual, hash, "llm digest fixture drifted: {relative}");
    }

    for required in [
        "fn run() {",
        "greeting :: \"Hello, Jet\"",
        "print(greeting)",
        "No semicolons.",
    ] {
        assert!(digest.contains(required), "digest lacks first-program context: {required}");
    }

    let scratch = Scratch::new("jet_llm_digest_first_program");
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command
        .args(["run", "--release"])
        .arg(&source)
        .current_dir(&scratch.path);
    let bounded = run_bounded(command, "llm digest first program", PROCESS_DEADLINE);
    assert!(!bounded.timed_out, "llm digest first program timed out");
    assert!(
        bounded.output.status.success(),
        "llm digest first program failed:\n{}",
        String::from_utf8_lossy(&bounded.output.stderr)
    );
    assert_eq!(bounded.output.stdout, expected, "first-program transcript drifted");
}
