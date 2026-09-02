use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const VALID_OUTPUT: &[u8] = br#"{"count":2,"name":"Ada","note":"line \"one\""}
"#;

struct MalformedCase {
    fixture: &'static str,
    line: usize,
    reason: &'static str,
}

const MALFORMED_CASES: &[MalformedCase] = &[
    MalformedCase {
        fixture: "truncated.tsv",
        line: 3,
        reason: "what=malformed TSV row; why=expected exactly two tab-separated fields; fix=write <key><tab><value>",
    },
    MalformedCase {
        fixture: "empty-key.tsv",
        line: 3,
        reason: "what=empty TSV key; why=each row needs a named report field; fix=write name, count, or note before the tab",
    },
    MalformedCase {
        fixture: "extra-column.tsv",
        line: 3,
        reason: "what=malformed TSV row; why=expected exactly two tab-separated fields; fix=write <key><tab><value>",
    },
    MalformedCase {
        fixture: "unknown-key.tsv",
        line: 3,
        reason: "what=unknown TSV key; why=only name, count, and note are valid report fields; fix=use one of the required keys",
    },
    MalformedCase {
        fixture: "duplicate-key.tsv",
        line: 3,
        reason: "what=duplicate TSV key; why=each report field must appear once; fix=remove the repeated row",
    },
    MalformedCase {
        fixture: "invalid-count.tsv",
        line: 2,
        reason: "what=invalid count value; why=count must be a decimal integer; fix=write a whole-number count",
    },
    MalformedCase {
        fixture: "missing-required-key.tsv",
        line: 3,
        reason: "what=missing required TSV key; why=report needs name, count, and note; fix=add the missing row",
    },
];

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tier: &str) -> Self {
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "jet-fresh-agent-t4-{}-{tier}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create T4 scratch directory");
        Self { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fresh_agent_t4")
}

fn copy_fixture_project(root: &Path, scratch: &Path) {
    fs::copy(root.join("package.jet"), scratch.join("package.jet"))
        .expect("copy T4 package manifest");
    fs::copy(root.join("run.jet"), scratch.join("run.jet")).expect("copy T4 Jet program");
}

fn run_tier(scratch: &Scratch, command_args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(command_args)
        .args(["run.jet", "--", "report.tsv"])
        .current_dir(&scratch.path)
        .env("JET_STORE_DIR", scratch.path.join("cache"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run T4 Jet fixture")
}

#[test]
fn fresh_agent_t4_rejects_malformed_rows_on_all_applicable_tiers() {
    let root = fixture_root();
    let mut tiers: Vec<(&str, &[&str])> = vec![
        ("default", &["run"]),
        ("dev", &["dev", "--watch=off"]),
        ("interpreter", &["run", "--interpret"]),
    ];
    let has_rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if has_rustc {
        tiers.push(("aot", &["run", "--release"]));
    }

    for (tier, tier_args) in tiers {
        let scratch = Scratch::new(tier);
        copy_fixture_project(&root, &scratch.path);

        fs::copy(root.join("report.tsv"), scratch.path.join("report.tsv"))
            .expect("copy valid T4 input");
        let valid = run_tier(&scratch, tier_args);
        assert!(
            valid.status.success(),
            "{tier} valid T4 run failed: {}",
            String::from_utf8_lossy(&valid.stderr)
        );
        assert_eq!(valid.stdout, VALID_OUTPUT, "{tier} valid output drifted");
        assert!(
            valid.stderr.is_empty(),
            "{tier} valid run wrote stderr: {}",
            String::from_utf8_lossy(&valid.stderr)
        );

        for case in MALFORMED_CASES {
            fs::copy(
                root.join("inputs").join(case.fixture),
                scratch.path.join("report.tsv"),
            )
            .expect("copy malformed T4 input");
            let malformed = run_tier(&scratch, tier_args);
            assert_eq!(
                malformed.status.code(),
                Some(1),
                "{tier} {} returned the wrong exit code",
                case.fixture
            );
            assert!(
                malformed.stdout.is_empty(),
                "{tier} {} wrote stdout: {}",
                case.fixture,
                String::from_utf8_lossy(&malformed.stdout)
            );
            let expected = format!(
                "report.tsv:{}:{}\n",
                case.line, case.reason
            );
            assert_eq!(
                String::from_utf8_lossy(&malformed.stderr),
                expected,
                "{tier} {} diagnostic drifted",
                case.fixture
            );
        }
    }
}
