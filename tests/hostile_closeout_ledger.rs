//! The #2394 closeout ledger must account for every observed diagnostic owned
//! by a hostile-audit card before that card can be closed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const LEDGER: &str = include_str!("../docs/audits/dogfood-jet-experience-5-of-5.md");

struct EvidenceFile {
    path: &'static str,
    markers: &'static [&'static str],
}

struct CloseoutFinding {
    code: Option<&'static str>,
    finding: &'static str,
    owner: &'static str,
    raw_witness: &'static str,
    fixture: &'static str,
    evidence: &'static [EvidenceFile],
    row_requirements: &'static [&'static str],
}

// Keep this table limited to findings that have a dedicated #2394 owner.
// Raw receipt codes, compiler ICEs, and malformed-input coverage all stay in
// one table so a missing or mismatched disposition row fails closed.
const HOSTILE_CLOSEOUT_FINDINGS: &[CloseoutFinding] = &[
    CloseoutFinding {
        code: Some("L0520"),
        finding: "F50",
        owner: "#2417",
        raw_witness: "docs/audits/raw/2393-r1/A03/T4/jet.json",
        fixture: "tests/ui/ioerror_display_migration.jet",
        evidence: &[
            EvidenceFile {
                path: "docs/audits/raw/2393-r1/A03/T4/jet.json",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/ui/ioerror_display_migration.jet",
                markers: &[],
            },
        ],
        row_requirements: &[],
    },
    CloseoutFinding {
        code: Some("L0503"),
        finding: "F51",
        owner: "#2416",
        raw_witness: "docs/audits/raw/2393-r1/A01/T2/jet.json",
        fixture: "tests/ui_lint/prefer_compound_assign.jet",
        evidence: &[
            EvidenceFile {
                path: "docs/audits/raw/2393-r1/A01/T2/jet.json",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/ui_lint/prefer_compound_assign.jet",
                markers: &[],
            },
        ],
        row_requirements: &[],
    },
    CloseoutFinding {
        code: None,
        finding: "F52",
        owner: "#2415",
        raw_witness: "docs/audits/raw/2393-r1/A01/T1/jet.json",
        fixture: "tests/marker_declarations.rs",
        evidence: &[
            EvidenceFile {
                path: "docs/audits/raw/2393-r1/A01/T1/jet.json",
                markers: &[
                    "fact registry law violation: `Scheduler` is registered twice; one table means one row per name",
                ],
            },
            EvidenceFile {
                path: "tests/marker_declarations.rs",
                markers: &[
                    "repeated_scheduler_fact_registration_is_stable_and_conflicts_are_diagnostic",
                    "fact Scheduler(@name: \"Scheduler\"",
                    "fact TargetScheduler(@name: \"Target.Scheduler\"",
                ],
            },
        ],
        row_requirements: &[
            "duplicate Scheduler fact registration",
            "Scheduler",
            "Target.Scheduler",
            "check",
            "AOT",
            "default",
            "dev",
            "interpreter",
            "no ICE",
            "byte-identical output",
            "#2393",
            "#2394",
        ],
    },
    CloseoutFinding {
        code: None,
        finding: "F53",
        owner: "#2419",
        raw_witness: "docs/audits/raw/2393-r1/A10/T4/jet.json",
        fixture: "tests/fixtures/fresh_agent_t4/**",
        evidence: &[
            EvidenceFile {
                path: "docs/audits/raw/2393-r1/A10/T4/jet.json",
                markers: &[
                    "\"participant\": \"A10\"",
                    "\"task\": \"T4\"",
                    "\"arm\": \"Jet\"",
                ],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/package.jet",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/run.jet",
                markers: &[
                    "what=malformed TSV row",
                    "what=empty TSV key",
                    "what=unknown TSV key",
                    "what=duplicate TSV key",
                    "what=invalid count value",
                    "what=missing required TSV key",
                ],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/report.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/truncated.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/empty-key.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/extra-column.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/unknown-key.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/duplicate-key.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/invalid-count.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fixtures/fresh_agent_t4/inputs/missing-required-key.tsv",
                markers: &[],
            },
            EvidenceFile {
                path: "tests/fresh_agent_t4.rs",
                markers: &[
                    "fresh_agent_t4_rejects_malformed_rows_on_all_applicable_tiers",
                    "MALFORMED_CASES",
                    "truncated.tsv",
                    "empty-key.tsv",
                    "extra-column.tsv",
                    "unknown-key.tsv",
                    "duplicate-key.tsv",
                    "invalid-count.tsv",
                    "missing-required-key.tsv",
                    "has_rustc",
                    "(\"aot\", &[\"run\", \"--release\"])",
                    "malformed.stdout.is_empty()",
                ],
            },
        ],
        row_requirements: &[
            "truncated.tsv",
            "empty-key.tsv",
            "extra-column.tsv",
            "unknown-key.tsv",
            "duplicate-key.tsv",
            "invalid-count.tsv",
            "missing-required-key.tsv",
            "default/dev/interpreter/AOT",
            "AOT when `rustc` exists",
            "exits 1",
            "empty stdout",
            "deterministic line-aware stderr",
            "matched Jet/Rust",
            "#2393",
            "#2394",
        ],
    },
];

fn root() -> PathBuf {
    PathBuf::from(option_env!("CARGO_MANIFEST_DIR").unwrap_or("."))
}

fn collect_jet_receipts(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            collect_jet_receipts(&path, out);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("jet.json") {
            out.push(path);
        }
    }
}

fn raw_code_counts() -> BTreeMap<String, usize> {
    let raw_root = root().join("docs/audits/raw/2393-r1");
    let mut files = Vec::new();
    collect_jet_receipts(&raw_root, &mut files);
    files.sort();

    let mut counts = BTreeMap::new();
    for path in files {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let mut in_codes = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if !in_codes {
                if trimmed.starts_with("\"codes\": [") {
                    in_codes = true;
                }
                continue;
            }
            if trimmed == "]" || trimmed == "]," {
                in_codes = false;
                continue;
            }
            let Some(code) = trimmed.strip_prefix('"').and_then(|rest| rest.split('"').next())
            else {
                continue;
            };
            *counts.entry(code.to_string()).or_insert(0) += 1;
        }
    }
    counts
}

fn ledger_rows<'a>(ledger: &'a str, finding: &str) -> Vec<&'a str> {
    ledger
        .lines()
        .filter(|line| {
            let mut columns = line.split('|');
            let _empty = columns.next();
            columns
                .next()
                .is_some_and(|column| column.trim().starts_with(finding))
        })
        .collect()
}

fn validate_evidence(spec: &CloseoutFinding) -> Result<(), String> {
    for evidence in spec.evidence {
        let path = root().join(evidence.path);
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", evidence.path))?;
        for marker in evidence.markers.iter().copied() {
            if !text.contains(marker) {
                return Err(format!(
                    "evidence {} for {} lacks {marker:?}",
                    evidence.path, spec.finding
                ));
            }
        }
    }
    Ok(())
}

fn validate_hostile_closeout_ledger(
    ledger: &str,
    observed: &BTreeMap<String, usize>,
) -> Result<(), String> {
    for spec in HOSTILE_CLOSEOUT_FINDINGS {
        let observed_name = spec.code.unwrap_or(spec.finding);
        if let Some(code) = spec.code {
            let count = observed.get(code).copied().unwrap_or(0);
            if count == 0 {
                return Err(format!(
                    "closeout diagnostic {code} was not observed in the fresh-agent receipts"
                ));
            }
        }
        validate_evidence(spec)?;

        let rows = ledger_rows(ledger, spec.finding);
        if rows.is_empty() {
            return Err(format!(
                "observed finding {observed_name} has no canonical ledger row {}",
                spec.finding
            ));
        }
        if rows.len() != 1 {
            return Err(format!(
                "observed finding {} must have one canonical ledger row, found {}",
                spec.finding,
                rows.len()
            ));
        }
        let row = rows[0];
        let base_requirements = [spec.owner, spec.raw_witness, spec.fixture];
        for required in base_requirements
            .iter()
            .copied()
            .chain(spec.row_requirements.iter().copied())
        {
            if !row.contains(required) {
                return Err(format!(
                    "ledger row {} for observed {} lacks {required:?}",
                    spec.finding, observed_name
                ));
            }
        }
        let closed_by_owner = format!("Closed by `{}`", spec.owner);
        if !row.contains("State: `open`") && !row.contains(&closed_by_owner) {
            return Err(format!(
                "ledger row {} for observed {} must stay open until focused proof or close under {}",
                spec.finding, observed_name, spec.owner
            ));
        }
    }
    Ok(())
}


#[test]
fn every_observed_hostile_closeout_finding_has_one_owner_row() {
    let observed = raw_code_counts();
    validate_hostile_closeout_ledger(LEDGER, &observed)
        .unwrap_or_else(|error| panic!("hostile closeout ledger is incomplete: {error}"));
}

#[test]
fn closeout_rejects_an_observed_finding_when_its_owner_row_is_missing() {
    let observed = raw_code_counts();
    for spec in HOSTILE_CLOSEOUT_FINDINGS {
        let without_row = LEDGER
            .lines()
            .filter(|line| !line.contains(&format!("| {} ", spec.finding)))
            .collect::<Vec<_>>()
            .join("\n");
        let error = validate_hostile_closeout_ledger(&without_row, &observed)
            .expect_err("an observed finding without its owner row must fail closed");
        let observed_name = spec.code.unwrap_or(spec.finding);
        assert!(
            error.contains(observed_name) && error.contains(spec.finding),
            "missing-row error lost identity for {}: {error}",
            spec.finding
        );
    }
}

#[test]
fn closeout_rejects_a_mismatched_owner_or_witness() {
    let observed = raw_code_counts();
    for spec in HOSTILE_CLOSEOUT_FINDINGS {
        let row = ledger_rows(LEDGER, spec.finding)
            .into_iter()
            .next()
            .expect("every finding has a source row");
        for required in [spec.owner, spec.raw_witness, spec.fixture] {
            let mismatched_row = row.replace(required, "__mismatched__");
            let mismatched_ledger = LEDGER
                .lines()
                .map(|line| {
                    if line == row {
                        mismatched_row.as_str()
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let error = validate_hostile_closeout_ledger(&mismatched_ledger, &observed)
                .expect_err("a mismatched owner or witness must fail closed");
            assert!(
                error.contains(spec.finding) && error.contains(required),
                "mismatch error lost {} identity: {error}",
                spec.finding
            );
        }
    }
}

#[test]
fn l0503_current_fixture_keeps_subject_span_and_teaching_snapshot() {
    let fixture = fs::read_to_string(root().join("tests/ui_lint/prefer_compound_assign.jet"))
        .expect("L0503 fixture");
    let snapshot = fs::read_to_string(root().join("tests/ui_lint/prefer_compound_assign.warn"))
        .expect("L0503 snapshot");

    for expected in [
        "p.hp = p.hp - amount",
        "n = (n + 1)",
        "Indexed compound is E0164",
    ] {
        assert!(fixture.contains(expected), "L0503 fixture lost {expected:?}");
    }
    assert_eq!(
        snapshot.matches("Warning [L0503]").count(),
        4,
        "L0503 snapshot warning count changed: {snapshot}"
    );
    for expected in [
        "prefer `p.hp -= …` instead of repeating the left side",
        "prefer `p.hp += …` instead of repeating the left side",
        "Why: compound assignment updates a place in one step without restating it",
        "Fix: write `p.hp -= …`",
        "More: jet-lang.dev/e/L0503",
    ] {
        assert!(snapshot.contains(expected), "L0503 snapshot lost {expected:?}");
    }
}
