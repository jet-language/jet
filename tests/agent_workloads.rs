//! #769: executable AI-agent workload conformance corpus.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const HEADER: &str = "version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards";
const DOMAIN_CONTRACT_HEADER: &str =
    "version\ttask_id\tallowed_dependencies\tmachine_spec\tvariant\tscoring";
const BASELINE_HEADER: &str =
    "version\trun_id\tmachine\tadapter\ttask_id\texpressibility\tfinding\tinput_sha256\texpected_sha256\texit_code\tsource_tokens\tstdout_file\tstdout_sha256\tstderr_file\tstderr_sha256\tcold_ns\twarm_ns\twarm_stdout_sha256\twarm_stderr_sha256\toutput_stable\tscoring\ttool_version\tpolicy_digest";
const PROCESS_DEADLINE: Duration = Duration::from_secs(120);
const PROCESS_OUTPUT_LIMIT_BYTES: u64 = 1_048_576;
const DOMAIN_SCORING: &str =
    "#769:v1;exit=0;stdout=exact;cold=recorded;warm=equal;input=unchanged;scratch=closed";
const TASK_AUTHORITY: &str =
    "argv=input-root;cwd=scratch;host=ambient;network=unmeasured;external-write=unmeasured";
const POLICY_PLAN: &str = "manifest-v1+domain-contract-v1";
const POLICY_LAUNCH_TRANSACTION: &str =
    "argv=input-root;cwd=scratch;env=JET_CORPUS_JET,JET_CORPUS_JETPACK,JET_CORPUS_TASK";
const POLICY_DESCENDANTS: &str = "process-group=owned;timeout=kill-reap";
const POLICY_LIMITS: &str = "wall=120s;output=1048576";
const POLICY_OUTPUTS: &str = "stdout=exact;stderr=hashed;input=unchanged;scratch=closed";
const POLICY_RECEIPT: &str = "result-v2;baseline-v2;report-v2";
const NATIVE_OS_MATRIX_HEADER: &str = "version\tos\tarch_policy\trequirement\tadapters\treason";
const EXPECTED_NATIVE_OS_MATRIX: &[(&str, &str, &str, &str, &str)] = &[
    (
        "linux",
        "x86_64",
        "required",
        "jet,bash,python,node",
        "native",
    ),
    ("macos", "any", "required", "jet,bash,python,node", "native"),
    (
        "windows",
        "any",
        "excluded",
        "jet,python,node",
        "bash-not-native",
    ),
];
const JET_BASELINE_HEADER: &str = "version\ttask_id\tjet_status\tloss_owner";
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
        "semantic-rename-with-decoys",
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
const BASELINE_ADAPTERS: &[&str] = &["bash", "python", "node"];
const EXPECTED_DOMAIN_CONTRACT: &[(&str, &str, &str, &str)] = &[
    (
        "build-test-failure-recovery",
        "jet=Core:files,process,sys,time;bash=bash,coreutils;python=stdlib-pathlib,stdlib-subprocess;node=stdlib-fs,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "process-batch-success",
        "jet=Core:files,process,time;bash=bash,coreutils;python=stdlib-pathlib,stdlib-subprocess;node=stdlib-fs,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "process-batch-large-stderr",
        "jet=Core:files,process,time;bash=bash,coreutils;python=stdlib-pathlib,stdlib-subprocess;node=stdlib-fs,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "process-batch-timeout-recovery",
        "jet=Core:files,process,time;bash=bash,coreutils;python=stdlib-pathlib,stdlib-subprocess;node=stdlib-fs,stdlib-child-process",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
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
    (
        "repository-marker-scan",
        "jet=Core:files,process;bash=bash-stdlib;python=stdlib-pathlib;node=stdlib-fs-path",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "repository-marker-scan-empty",
        "jet=Core:files,process;bash=bash-stdlib;python=stdlib-pathlib;node=stdlib-fs-path",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "repository-semantic-inspection",
        "jet=Core:process,sys,inspect-semindex;bash=awk;python=stdlib-pathlib;node=stdlib-fs",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "repository-semantic-edit",
        "jet=Core:files,process,inspect-codemod;bash=cp,awk,mv;python=stdlib-shutil,pathlib;node=stdlib-fs",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "hostile",
    ),
    (
        "git-diff-review",
        "jet=Core:process,git;bash=git;python=stdlib-subprocess+git;node=stdlib-child-process+git",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
        "normal",
    ),
    (
        "git-diff-empty",
        "jet=Core:process,git;bash=git;python=stdlib-subprocess+git;node=stdlib-child-process+git",
        "linux-x86_64:nix-core;macos-native:host;windows-native:unavailable",
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
    limit_exceeded: bool,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/agent_workloads")
}

fn policy_digest() -> String {
    let canonical = [
        "version=1",
        &format!("plan={POLICY_PLAN}"),
        &format!("launch_transaction={POLICY_LAUNCH_TRANSACTION}"),
        &format!("descendants={POLICY_DESCENDANTS}"),
        &format!("limits={POLICY_LIMITS}"),
        &format!("outputs={POLICY_OUTPUTS}"),
        &format!("receipt={POLICY_RECEIPT}"),
        &format!("authority={TASK_AUTHORITY}"),
    ]
    .join("\n");
    jet::SHA256::sha256_hex(canonical.as_bytes())
}

fn policy_receipt(digest: &str) -> String {
    format!(
        "policy\tversion=1\tdigest={digest}\tplan={POLICY_PLAN}\tlaunch_transaction={POLICY_LAUNCH_TRANSACTION}\tdescendants={POLICY_DESCENDANTS}\tlimits={POLICY_LIMITS}\toutputs={POLICY_OUTPUTS}\treceipt={POLICY_RECEIPT}\tauthority={TASK_AUTHORITY}"
    )
}

fn validate_authority(authority: &str) -> Result<(), String> {
    if authority == TASK_AUTHORITY {
        Ok(())
    } else {
        Err(format!("unsupported authority enforcement: {authority}"))
    }
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

fn read_native_os_matrix() -> Vec<(String, String, String, String, String)> {
    let matrix = fs::read_to_string(corpus_root().join("native_os_matrix.tsv")).unwrap();
    let mut lines = matrix.lines();
    assert_eq!(
        lines.next(),
        Some(NATIVE_OS_MATRIX_HEADER),
        "native OS matrix schema drifted"
    );
    lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "bad native OS matrix row: {line}");
            assert_eq!(
                fields[0], "1",
                "unsupported native OS matrix version: {line}"
            );
            (
                fields[1].into(),
                fields[2].into(),
                fields[3].into(),
                fields[4].into(),
                fields[5].into(),
            )
        })
        .collect()
}

fn read_jet_baseline() -> Vec<(String, String, String)> {
    let baseline = fs::read_to_string(corpus_root().join("jet_baseline.tsv")).unwrap();
    let mut lines = baseline.lines();
    assert_eq!(
        lines.next(),
        Some(JET_BASELINE_HEADER),
        "Jet baseline schema drifted"
    );
    lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4, "bad Jet baseline row: {line}");
            assert_eq!(
                fields[0], "1",
                "unsupported Jet baseline version: {line}"
            );
            (fields[1].into(), fields[2].into(), fields[3].into())
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
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let mut child = command
        .spawn()
        .unwrap_or_else(|err| panic!("cannot start `{label}`: {err}"));
    let child_pid = child.id();

    let (status, timed_out, limit_exceeded) = loop {
        if let Some(status) = child.try_wait().unwrap() {
            // The bounded child owns its process group, including detached
            // helpers. Reap that group on normal completion as well as on
            // timeout/limit failure, or a descendant can race the next run.
            #[cfg(unix)]
            kill_process_group(child_pid);
            break (
                child.wait().unwrap_or(status),
                false,
                output_size(&stdout_path, &stderr_path) > PROCESS_OUTPUT_LIMIT_BYTES,
            );
        }
        if output_size(&stdout_path, &stderr_path) > PROCESS_OUTPUT_LIMIT_BYTES {
            #[cfg(unix)]
            kill_process_group(child.id());
            let _ = child.kill();
            break (child.wait().unwrap(), false, true);
        }
        if started.elapsed() >= deadline {
            #[cfg(unix)]
            kill_process_group(child.id());
            let _ = child.kill();
            break (child.wait().unwrap(), true, false);
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
        limit_exceeded,
    }
}

fn output_size(stdout: &Path, stderr: &Path) -> u64 {
    fs::metadata(stdout).map(|file| file.len()).unwrap_or(0)
        + fs::metadata(stderr).map(|file| file.len()).unwrap_or(0)
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // SAFETY: the negative PID targets only the process group created for the
    // bounded child; SIGKILL makes timeout cleanup fail closed.
    unsafe {
        let _ = kill(-pid, 9);
    }
}

#[cfg(target_os = "linux")]
fn process_is_gone_or_zombie(pid: u32) -> bool {
    let path = format!("/proc/{pid}/stat");
    let Ok(stat) = fs::read_to_string(path) else {
        return true;
    };
    stat.rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().next())
        == Some("Z")
}

fn command_version(program: &Path, arg: &str, label: &str) -> String {
    let mut command = Command::new(program);
    command.arg(arg);
    let bounded = run_bounded(command, label, PROCESS_DEADLINE);
    assert!(!bounded.timed_out, "`{label}` version timed out");
    assert!(!bounded.limit_exceeded, "`{label}` output limit exceeded");
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

fn input_digest(input: &Path) -> String {
    let mut canonical = String::new();
    for (path, hash) in input_hashes(input) {
        canonical.push_str(&path);
        canonical.push('\t');
        canonical.push_str(&hash);
        canonical.push('\n');
    }
    jet::SHA256::sha256_hex(canonical.as_bytes())
}

struct BaselineCapture {
    root: PathBuf,
    run_id: String,
    machine: String,
    receipt: fs::File,
}

impl BaselineCapture {
    fn from_env() -> Option<Self> {
        let root = PathBuf::from(std::env::var_os("JET_CORPUS_BASELINE_DIR")?);
        let run_id = std::env::var("JET_CORPUS_BASELINE_RUN_ID")
            .expect("JET_CORPUS_BASELINE_RUN_ID is required when capturing baselines");
        let machine = std::env::var("JET_CORPUS_BASELINE_MACHINE")
            .expect("JET_CORPUS_BASELINE_MACHINE is required when capturing baselines");
        fs::create_dir_all(root.join("outputs")).unwrap();
        let mut receipt = fs::File::create(root.join("receipt.tsv")).unwrap();
        writeln!(receipt, "{BASELINE_HEADER}").unwrap();
        Some(Self {
            root,
            run_id,
            machine,
            receipt,
        })
    }

    fn record(
        &mut self,
        task: &Task,
        adapter: &str,
        source: &Path,
        input: &Path,
        expected: &[u8],
        cold: &BoundedOutput,
        warm: &BoundedOutput,
        version: &str,
    ) {
        let output_dir = self.root.join("outputs").join(adapter);
        fs::create_dir_all(&output_dir).unwrap();
        let stdout_file = format!("outputs/{adapter}/{}.stdout", task.id);
        let stderr_file = format!("outputs/{adapter}/{}.stderr", task.id);
        fs::write(self.root.join(&stdout_file), &cold.output.stdout).unwrap();
        fs::write(self.root.join(&stderr_file), &cold.output.stderr).unwrap();
        let output_stable = cold.output.stdout == warm.output.stdout;
        let supported = cold.output.status.code() == Some(0)
            && warm.output.status.code() == Some(0)
            && cold.output.stdout == expected
            && warm.output.stdout == expected
            && output_stable;
        let expressibility = if supported { "supported" } else { "unsupported" };
        let finding = if supported {
            "none".to_string()
        } else {
            format!(
                "cold_exit={:?};warm_exit={:?};stdout_expected={};warm_stable={}",
                cold.output.status.code(),
                warm.output.status.code(),
                cold.output.stdout == expected && warm.output.stdout == expected,
                output_stable
            )
        };
        let row = [
            "1".to_string(),
            self.run_id.clone(),
            self.machine.clone(),
            adapter.to_string(),
            task.id.clone(),
            expressibility.to_string(),
            finding,
            input_digest(input),
            jet::SHA256::sha256_hex(expected),
            cold.output.status.code().unwrap_or(-1).to_string(),
            source_tokens(source).to_string(),
            stdout_file,
            jet::SHA256::sha256_hex(&cold.output.stdout),
            stderr_file,
            jet::SHA256::sha256_hex(&cold.output.stderr),
            cold.elapsed.as_nanos().to_string(),
            warm.elapsed.as_nanos().to_string(),
            jet::SHA256::sha256_hex(&warm.output.stdout),
            jet::SHA256::sha256_hex(&warm.output.stderr),
            output_stable.to_string(),
            DOMAIN_SCORING.to_string(),
            version.replace('\t', " "),
            policy_digest(),
        ]
        .join("\t");
        writeln!(self.receipt, "{row}").unwrap();
    }
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

fn baseline_artifact_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute() {
        return Err(format!("baseline artifact escaped root: {relative}"));
    }
    if relative
        .split('/')
        .any(|part| part.is_empty() || part == "..")
    {
        return Err(format!("invalid baseline artifact path: {relative}"));
    }
    Ok(root.join(path))
}

#[test]
fn recorded_baselines_cover_frozen_tasks() {
    let root = corpus_root().join("baselines");
    let receipt = fs::read_to_string(root.join("receipt.tsv")).unwrap();
    let mut lines = receipt.lines();
    assert_eq!(lines.next(), Some(BASELINE_HEADER), "baseline schema drifted");

    let tasks = read_tasks();
    let mut seen = BTreeSet::new();
    let mut run_id = None;
    let mut machine = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 23, "bad baseline receipt row: {line}");
        assert_eq!(fields[0], "1", "unsupported baseline receipt version: {line}");
        assert!(BASELINE_ADAPTERS.contains(&fields[3]), "unknown baseline adapter: {line}");
        let task = tasks
            .iter()
            .find(|task| task.id == fields[4])
            .unwrap_or_else(|| panic!("baseline names unknown task: {line}"));
        assert!(
            seen.insert((fields[3].to_string(), fields[4].to_string())),
            "duplicate baseline row: {line}"
        );
        if let Some(previous) = run_id {
            assert_eq!(previous, fields[1], "baseline mixes run IDs");
        } else {
            run_id = Some(fields[1]);
        }
        if let Some(previous) = machine {
            assert_eq!(previous, fields[2], "baseline mixes machines");
        } else {
            machine = Some(fields[2]);
        }
        assert!(!fields[1].is_empty() && !fields[2].is_empty());
        assert!(fields[21] != "" && fields[21] != "unknown");
        assert_eq!(fields[22], policy_digest(), "baseline policy digest drifted: {line}");
        assert_eq!(fields[7], input_digest(&corpus_root().join(&task.input)));
        let expected = fs::read(corpus_root().join(&task.expected)).unwrap();
        assert_eq!(fields[8], jet::SHA256::sha256_hex(&expected));
        assert_eq!(fields[10].parse::<usize>().unwrap().to_string(), fields[10]);
        assert!(fields[15].parse::<u128>().is_ok());
        assert!(fields[16].parse::<u128>().is_ok());
        assert_eq!(fields[20], DOMAIN_SCORING);

        match fields[5] {
            "supported" => {
                assert_eq!(fields[6], "none");
                assert_eq!(fields[9], "0");
                assert_eq!(fields[19], "true");
                let stdout = fs::read(
                    baseline_artifact_path(&root, fields[11])
                        .unwrap_or_else(|error| panic!("{error}")),
                )
                .unwrap();
                let stderr = fs::read(
                    baseline_artifact_path(&root, fields[13])
                        .unwrap_or_else(|error| panic!("{error}")),
                )
                .unwrap();
                assert_eq!(stdout, expected, "baseline stdout drifted: {line}");
                assert_eq!(fields[12], jet::SHA256::sha256_hex(&stdout));
                assert_eq!(fields[14], jet::SHA256::sha256_hex(&stderr));
                assert_eq!(fields[17], fields[12]);
            }
            "unsupported" => {
                assert_ne!(fields[6], "none", "unsupported baseline lacks finding: {line}");
                assert_eq!(fields[11], "-");
                assert_eq!(fields[13], "-");
            }
            other => panic!("unknown baseline expressibility {other}: {line}"),
        }
    }
    assert_eq!(
        seen.len(),
        tasks.len() * BASELINE_ADAPTERS.len(),
        "baseline omitted a frozen task or adapter"
    );
}

#[test]
fn policy_digest_covers_authority_and_receipt_contract() {
    let digest = policy_digest();
    let receipt = policy_receipt(&digest);
    for field in [
        format!("digest={digest}"),
        format!("plan={POLICY_PLAN}"),
        format!("launch_transaction={POLICY_LAUNCH_TRANSACTION}"),
        format!("descendants={POLICY_DESCENDANTS}"),
        format!("limits={POLICY_LIMITS}"),
        format!("outputs={POLICY_OUTPUTS}"),
        format!("receipt={POLICY_RECEIPT}"),
        format!("authority={TASK_AUTHORITY}"),
    ] {
        assert!(receipt.contains(&field), "policy receipt lost {field}");
    }
    assert!(validate_authority(TASK_AUTHORITY).is_ok());
    assert!(validate_authority(
        "argv=input-root;cwd=scratch;host=ambient;network=denied;external-write=unmeasured"
    )
    .is_err());
    assert!(validate_authority(
        "argv=input-root;cwd=scratch;host=ambient;network=unmeasured;external-write=denied"
    )
    .is_err());
}

#[test]
fn receipt_artifact_paths_reject_escape_attempts() {
    let root = Path::new("/corpus/baselines");
    assert!(baseline_artifact_path(root, "outputs/bash/task.stdout").is_ok());
    for path in ["/etc/passwd", "../outside", "outputs//task", "outputs/../task"] {
        assert!(baseline_artifact_path(root, path).is_err(), "accepted {path}");
    }
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
        assert_eq!(task.authority, TASK_AUTHORITY);
        validate_authority(&task.authority).unwrap_or_else(|error| panic!("{error}"));
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
            "build-test-failure-recovery"
                | "process-batch-large-stderr"
                | "process-batch-timeout-recovery"
                | "browser-automation-preflight"
                | "desktop-interaction-focus"
                | "document-markdown-inspection"
                | "media-asset-inventory"
                | "mcp-environment-denied"
                | "interactive-terminal-closed"
                | "service-lifecycle-readiness-timeout"
        ) {
            let input = corpus_root().join(&task.input);
            match task.id.as_str() {
                "build-test-failure-recovery" => {
                    for invalid in ["invalid.jet", "invalid.sh", "invalid.py", "invalid.mjs"] {
                        let source = fs::read_to_string(input.join(invalid)).unwrap();
                        assert!(
                            source.contains("fn run( {")
                                || source.contains("if then")
                                || source.contains("def run(:")
                                || source.contains("function run( {"),
                            "build recovery lost hostile source {invalid}"
                        );
                    }
                }
                "process-batch-large-stderr" => {
                    let source = fs::read_to_string(input).unwrap();
                    assert!(
                        source.contains("100000") && source.contains(">&2"),
                        "large-stderr task lost hostile output pressure"
                    );
                }
                "process-batch-timeout-recovery" => {
                    let source = fs::read_to_string(input).unwrap();
                    assert!(
                        source.contains("trap '' TERM") && source.contains("50"),
                        "timeout task lost TERM-resistant cancellation case"
                    );
                }
                _ => {
                    let expected = fs::read_to_string(corpus_root().join(&task.expected)).unwrap();
                    let hostile = match task.id.as_str() {
                        "desktop-interaction-focus" => expected.lines().any(|line| line == "event|Empty|observed"),
                        "browser-automation-preflight" => expected.lines().any(|line| line.ends_with("|rejected")),
                        "mcp-environment-denied" => expected.lines().any(|line| line == "error=-32002"),
                        "interactive-terminal-closed" => expected.lines().any(|line| line == "closed=ok"),
                        "service-lifecycle-readiness-timeout" => expected.lines().any(|line| line == "error=E1261"),
                        _ => expected.lines().any(|line| line.starts_with("reject|")),
                    };
                    assert!(hostile, "target task {} lost its hostile variant", task.id);
                }
            }
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
    assert_eq!(verified, 82, "all inputs and declared outputs must be frozen");
}

#[test]
fn repository_and_git_jet_adapters_use_production_paths() {
    let checks = [
        (
            "repository_marker_scan.jet",
            ["core.files", "fs.walk"].as_slice(),
        ),
        (
            "repository_semantic_inspection.jet",
            ["inspect", "semindex"].as_slice(),
        ),
        (
            "repository_semantic_edit.jet",
            ["inspect", "codemod", "--yes"].as_slice(),
        ),
        (
            "git_diff_review.jet",
            ["git", "diff", "--no-index", "--name-status"].as_slice(),
        ),
    ];
    for (adapter, needles) in checks {
        let source = fs::read_to_string(corpus_root().join("adapters").join(adapter)).unwrap();
        for needle in needles {
            assert!(
                source.contains(needle),
                "{adapter} no longer reaches the production path containing {needle}"
            );
        }
    }
}

#[test]
fn structured_data_database_http_jet_adapters_use_production_paths() {
    let checks = [
        (
            "structured_data.jet",
            [
                "use core.encoding.json",
                "fs.read(input)",
                "json.decode<Batch>",
                "json.to_string",
            ]
            .as_slice(),
        ),
        (
            "database_access.jet",
            [
                "use core.db",
                "db.open_memory()",
                "db.migrate",
                "scoped.execute(",
                "scoped.query_one(",
                "scoped.close()",
            ]
            .as_slice(),
        ),
        (
            "http_api.jet",
            [
                "use core.http.client",
                "use core.http.server",
                "net.tcp_listen(",
                "http_server.serve_once_listener",
                "http_client.request(",
                "request.send()",
            ]
            .as_slice(),
        ),
    ];
    for (adapter, needles) in checks {
        let source = fs::read_to_string(corpus_root().join("adapters").join(adapter)).unwrap();
        for needle in needles {
            assert!(
                source.contains(needle),
                "{adapter} no longer reaches the production path containing {needle}"
            );
        }
    }
}

#[test]
fn native_os_matrix_is_frozen_and_names_current_host() {
    let matrix = read_native_os_matrix();
    let actual = matrix
        .iter()
        .map(|row| {
            (
                row.0.as_str(),
                row.1.as_str(),
                row.2.as_str(),
                row.3.as_str(),
                row.4.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        EXPECTED_NATIVE_OS_MATRIX,
        "native OS matrix changed without a frozen review"
    );
    assert!(
        matrix.iter().any(|row| row.0 == std::env::consts::OS),
        "current host `{}` is absent from the frozen native OS matrix",
        std::env::consts::OS
    );
}

#[test]
fn jet_baseline_is_frozen_and_each_loss_has_an_owner() {
    let tasks = read_tasks();
    let baseline = read_jet_baseline();
    assert_eq!(
        baseline.len(),
        tasks.len(),
        "Jet baseline must contain one row per corpus task"
    );
    for (task, row) in tasks.iter().zip(baseline) {
        assert_eq!(row.0, task.id, "Jet baseline task order drifted");
        assert_eq!(row.1, "pass", "baseline must freeze a passing Jet task");
        assert_eq!(row.2, task.loss_cards, "baseline loss owner drifted");
        assert!(
            row.2.split(';').all(|owner| {
                owner
                    .split_once('=')
                    .map(|(_, target)| target.starts_with('#') || target.starts_with("non-goal:"))
                    .unwrap_or(false)
            }),
            "Jet loss owner must name a card or ratified non-goal: {}",
            row.2
        );
    }
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
        assert!(!bounded.limit_exceeded, "deadline fixture hit output limit");
        assert!(
            bounded.elapsed < Duration::from_secs(2),
            "deadline cleanup took too long: {:?}",
            bounded.elapsed
        );
    }
    assert!(!scratch_path.exists(), "scratch directory survived Drop");
}

#[cfg(unix)]
#[test]
fn process_output_limit_fails_closed() {
    let scratch = Scratch::new("jet_agent_process_output_limit");
    let mut command = Command::new("python3");
    command.args(["-c", "import sys; sys.stdout.write('x' * 2000000)"]);
    command.current_dir(&scratch.path);
    let bounded = run_bounded(command, "output limit regression", PROCESS_DEADLINE);
    assert!(bounded.limit_exceeded, "output limit did not stop the process");
    assert!(!bounded.timed_out, "output limit was reported as a timeout");
    assert!(
        bounded.elapsed < Duration::from_secs(2),
        "output limit cleanup took too long: {:?}",
        bounded.elapsed
    );
}

#[cfg(target_os = "linux")]
#[test]
fn production_process_limits_authority_and_descendant_cleanup() {
    fn run_lens(source: &Path, release: bool, scratch: &Scratch) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        if release {
            command.arg("--release");
        }
        command.arg(source).current_dir(&scratch.path);
        let bounded = run_bounded(command, "process policy corpus", PROCESS_DEADLINE);
        assert!(!bounded.timed_out, "Jet process policy fixture timed out");
        assert!(!bounded.limit_exceeded, "Jet process policy fixture hit output limit");
        assert_eq!(bounded.output.status.code(), Some(0), "{}", String::from_utf8_lossy(&bounded.output.stderr));
        bounded.output
    }

    let source = r#"use core.process as process

fn run() {
    limited :: process.cmd(["printf", "12345"])
        .stdout(.Capture)
        .stderr(.Capture)
        .output_limit(3)
        .run()
    if limited == {
        .Ok(_) -> print("limit=leaked")
        .Err(_) -> print("limit=blocked")
        else -> {}
    }

    policy :: Abilities.from_rights(["Net:example.com"])
    planned :: process.cmd(["printf", "authority"]).under(policy).plan()
    if planned == {
        .Ok(_) -> print("authority=planned")
        .Err(_) -> print("authority=refused")
        else -> {}
    }

    child :: process.cmd(["sh", "-c", "sleep 30 & child=$!; echo $child > child.pid; wait"])
        .stdout(.Capture)
        .stderr(.Capture)
        .timeout(Duration.milliseconds(50) ?? panic("bad timeout"))
        .run() ?? panic("timeout launch failed")
    if !child.timed_out -> panic("timeout did not fire")
    print("timeout=cancelled")
}
"#;

    for release in [false, true] {
        let scratch = Scratch::new(if release {
            "jet_agent_process_policy_release"
        } else {
            "jet_agent_process_policy_default"
        });
        let source_path = scratch.path.join("process_policy.jet");
        fs::write(&source_path, source).unwrap();
        let output = run_lens(&source_path, release, &scratch);
        assert_eq!(
            output.stdout,
            b"limit=blocked\nauthority=refused\ntimeout=cancelled\n"
        );
        let child_pid = fs::read_to_string(scratch.path.join("child.pid"))
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !process_is_gone_or_zombie(child_pid) {
            assert!(Instant::now() < deadline, "timed-out process left descendant {child_pid}");
            thread::sleep(Duration::from_millis(10));
        }
    }
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
    let mut baseline = BaselineCapture::from_env();
    let capture_baselines = baseline.is_some();
    let run_policy_digest = policy_digest();
    println!("{}", policy_receipt(&run_policy_digest));

    for task in read_tasks() {
        validate_authority(&task.authority).unwrap_or_else(|error| panic!("{error}"));
        let input = corpus_root().join(&task.input);
        let expected = fs::read(corpus_root().join(&task.expected)).unwrap();
        let before = input_hashes(&input);
        let stem = adapter_stem(&task.id);
        let mut measurements = Vec::new();
        let mut declared_outputs = Vec::new();
        for &(adapter, extension) in ADAPTERS {
            if capture_baselines && adapter == "jet" {
                continue;
            }
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
            assert!(!cold.limit_exceeded, "{} {adapter} cold output limit exceeded", task.id);
            assert!(!warm.limit_exceeded, "{} {adapter} warm output limit exceeded", task.id);
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
            if adapter != "jet" {
                if let Some(capture) = baseline.as_mut() {
                    capture.record(
                        &task,
                        adapter,
                        &source,
                        &input,
                        &expected,
                        &cold,
                        &warm,
                        &versions[adapter],
                    );
                }
            }
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
            "machine\tos={}\tarch={}\tcorpus=1\ttask={}\tevidence={}\tcard={}\tlosses=red:{}\tjet_artifact={}\tjet_sha256={}\tgit_version={}\tpolicy_digest={}\tauthority={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            task.id,
            task.evidence,
            task.tower_card,
            task.loss_cards,
            jet_cli.display(),
            jet_artifact,
            git_version.replace('\t', " "),
            run_policy_digest,
            task.authority
        );
        for result in &measurements {
            println!(
                "result\ttask={}\tadapter={}\tsuccess=true\tsource_tokens={}\tcold_ns={}\twarm_ns={}\toutput_stable=true\tversion={}\tcold_stderr_bytes={}\tcold_stderr_sha256={}\twarm_stderr_bytes={}\twarm_stderr_sha256={}\tpolicy_digest={}\tlimits={}\tdescendants={}\toutputs={}\treceipt={}\tagent_tool_calls=not-recorded:#769\trepair_turns=not-recorded:#769\tpeak_memory=not-recorded:#769\tdiagnostic_quality=not-recorded:#769\torphan_processes=not-recorded:#769\tsandbox_escapes=not-recorded:#769\tnetwork=unmeasured:#769\texternal_writes=unmeasured:#769\tcross_platform=not-run:#769",
                task.id,
                result.adapter,
                result.source_tokens,
                result.cold.as_nanos(),
                result.warm.as_nanos(),
                result.version.replace('\t', " "),
                result.cold_stderr_bytes,
                result.cold_stderr_sha256,
                result.warm_stderr_bytes,
                result.warm_stderr_sha256,
                run_policy_digest,
                POLICY_LIMITS,
                POLICY_DESCENDANTS,
                POLICY_OUTPUTS,
                POLICY_RECEIPT
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
    assert!(!bounded.limit_exceeded, "llm digest first program hit output limit");
    assert!(
        bounded.output.status.success(),
        "llm digest first program failed:\n{}",
        String::from_utf8_lossy(&bounded.output.stderr)
    );
    assert_eq!(bounded.output.stdout, expected, "first-program transcript drifted");
}
