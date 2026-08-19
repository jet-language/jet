//! #2025: every declared test target belongs to exactly one named verification set.
//!
//! Three targets — `golden`, `ban_bare_panic` and `jit_run` — were each found red
//! while sitting outside every set an agent routinely ran. None of them was
//! broken by the session that found them; they had simply been red for a while
//! with nothing watching. The defect is not the three targets, it is that suite
//! MEMBERSHIP was remembered rather than enforced: a target nobody runs is a
//! target whose failures are invisible, and "add these three to the list" repeats
//! the same defect one layer up.
//!
//! So membership is derived and checked here, in the shape
//! `tests/dev_parts/support.rs` already uses for the corpus-gate ledger:
//!
//! - the DENOMINATOR is walked from the filesystem on every run, never listed —
//!   cargo builds one test binary per `tests/*.rs` file, so the walk IS the
//!   target inventory and a new file cannot be born outside it;
//! - `tests/suites.txt` states which set each target belongs to;
//! - the pins live HERE, outside the file they guard, so a hand-edit cannot green
//!   the check by deleting the row that fails;
//! - `suite_ledger_audit_fires_on_a_target_in_no_set` proves the law can fail,
//!   because a pin nobody has watched fail is a pin nobody should trust.
//!
//! Scope: integration test targets, in this package and every workspace crate.
//! Package lib/bin unit tests ride with their package under `--workspace` and
//! cannot fall out of a set on their own, which is the failure mode this file
//! exists to stop.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The one list of set names. The ledger parser, the writer of any message here
/// and `scripts/agent/time-suites.sh` all read the sections out of the ledger
/// rather than keeping a private copy, so a new set cannot exist in one reader
/// and not another (AGENTS.md I8).
const SUITE_ORDER: [&str; 7] = [
    "guards",
    "dev_loop",
    "language",
    "runtime",
    "tooling",
    "surface",
    "host_gated",
];

/// The one section that is not executable, so the one section that owes a reason.
const HOST_GATED: &str = "host_gated";

/// How many test targets `tests/suites.txt` must assign (#2025).
///
/// A floor: rows may only GROW. State the polarity plainly, because both
/// directions are bugs — a row that VANISHES is a target that left the sets and
/// went back to being unwatched, which is the defect this file pins; a target
/// with no row raises `unassigned` and fails against the ceiling below.
///
/// Measured on 2026-08-17: 289 `tests/*.rs` targets in this package, 8 in
/// workspace crates, plus this file. Lower it only in the same reviewed diff as
/// a deleted test file.
const TEST_TARGET_FLOOR: usize = 298;

/// How many declared targets may belong to NO set (#2025).
///
/// Zero, and shrink-only cannot go below zero, so this is the whole point of the
/// card expressed as a number: there is no legitimate "in no set". A target that
/// genuinely cannot run on a normal host is parked in `host_gated:` WITH its
/// observed gate, which costs an edit here as well — it is never simply absent.
const UNASSIGNED_CEILING: usize = 0;

/// The targets that cannot run in any executable set, with the observed gate and
/// the card that owes the fix. Sorted, and compared EXACTLY against the
/// `host_gated:` section for the same reason `AOT_BROKEN_HELD_OUT` is compared
/// exactly against its class: a newly parked target fails until someone writes
/// the reason down, and an un-parked one fails until its row leaves both places.
///
/// Empty today, and empty is the goal. This is armed, not decorative: the
/// negative control below drives it with synthetic input.
const HOST_GATED_HELD_OUT: &[(&str, &str, &str)] = &[];

/// One ledger row: a repo-relative target path, its set, and (host_gated only)
/// the observed gate.
#[derive(Clone, Debug)]
struct SuiteRow {
    target: String,
    set: &'static str,
    observed: String,
}

/// What the law found. Every field is a list of NAMES, never a bare count: a
/// count tells a reader that something is wrong and nothing about what.
#[derive(Debug, Default)]
struct SuiteLedgerAudit {
    /// Rows that name a target that exists. Ghost rows classify nothing, so they
    /// do not count toward the floor.
    assigned: usize,
    /// Discovered targets no section names — the defect this file pins.
    unassigned: Vec<String>,
    /// Rows naming a path with no `.rs` file: a stale row to delete, never a set
    /// change that failed.
    ghosts: Vec<String>,
    /// Targets claimed by more than one section, with both claimants named.
    duplicated: Vec<String>,
    /// `host_gated:` rows that park a target without saying what gates it.
    reasonless: Vec<String>,
    /// Executable sets with no members: a set name nobody can run is a name that
    /// will be cited as coverage it does not have.
    empty_sets: Vec<&'static str>,
    /// The observed `host_gated:` section, for the exact compare.
    host_gated: Vec<(String, String)>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Walk the target inventory. Cargo makes one test binary per `tests/*.rs` file
/// in every workspace package, so this walk is the inventory itself — nothing
/// here reads a list, which is exactly why a target cannot be born outside it.
fn discover_test_targets() -> Vec<String> {
    let root = repo_root();
    let mut targets = integration_targets(&root.join("tests"), "tests");
    let crates = root.join("crates");
    let mut crate_dirs: Vec<PathBuf> = fs::read_dir(&crates)
        .expect("crates dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    crate_dirs.sort();
    for crate_dir in crate_dirs {
        let name = crate_dir
            .file_name()
            .expect("crate dir name")
            .to_string_lossy()
            .into_owned();
        let tests = crate_dir.join("tests");
        if tests.is_dir() {
            targets.extend(integration_targets(&tests, &format!("crates/{name}/tests")));
        }
    }
    targets.sort();
    targets
}

fn integration_targets(dir: &Path, prefix: &str) -> Vec<String> {
    let mut targets: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".rs")
                .map(|stem| format!("{prefix}/{stem}"))
        })
        .collect();
    targets.sort();
    targets
}

fn parse_suite_ledger(text: &str) -> Vec<SuiteRow> {
    let mut section: Option<&'static str> = None;
    let mut rows = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(name) = trimmed.strip_suffix(':') {
            section = Some(
                SUITE_ORDER
                    .into_iter()
                    .find(|known| *known == name)
                    .unwrap_or_else(|| {
                        panic!(
                            "tests/suites.txt names the set `{name}:`, which is not one of \
                             {SUITE_ORDER:?}. A set the runner cannot name is a set nobody runs."
                        )
                    }),
            );
            continue;
        }
        let (target, observed) = match trimmed.split_once(": ") {
            Some((target, observed)) => (target.to_string(), observed.to_string()),
            None => (trimmed.to_string(), String::new()),
        };
        let set = section.unwrap_or_else(|| {
            panic!("tests/suites.txt row outside any section: {trimmed}")
        });
        rows.push(SuiteRow {
            target,
            set,
            observed,
        });
    }
    rows
}

/// The whole law as a pure function, so the negative control can prove it fires.
///
/// `every_test_target_belongs_to_a_named_set` is the only caller that reads the
/// real ledger and the real filesystem; this half takes both sides as arguments
/// so a synthetic inventory with a target no section names can be handed to it.
fn audit_suite_ledger(ledger: &[SuiteRow], discovered: &[String]) -> SuiteLedgerAudit {
    let existing: std::collections::HashSet<&str> =
        discovered.iter().map(String::as_str).collect();

    let mut claims: BTreeMap<&str, Vec<&'static str>> = BTreeMap::new();
    for row in ledger {
        claims.entry(row.target.as_str()).or_default().push(row.set);
    }

    let ghosts: Vec<String> = claims
        .keys()
        .filter(|target| !existing.contains(**target))
        .map(|target| (*target).to_string())
        .collect();
    let duplicated: Vec<String> = claims
        .iter()
        .filter(|(_, sets)| sets.len() > 1)
        .map(|(target, sets)| format!("  {target}: {}", sets.join(", ")))
        .collect();
    let unassigned: Vec<String> = discovered
        .iter()
        .filter(|target| !claims.contains_key(target.as_str()))
        .cloned()
        .collect();
    let reasonless: Vec<String> = ledger
        .iter()
        .filter(|row| row.set == HOST_GATED && row.observed.is_empty())
        .map(|row| format!("  {}: parked in `{HOST_GATED}:` with no gate named", row.target))
        .collect();
    let empty_sets: Vec<&'static str> = SUITE_ORDER
        .into_iter()
        .filter(|set| *set != HOST_GATED)
        .filter(|set| !ledger.iter().any(|row| row.set == *set))
        .collect();
    let mut host_gated: Vec<(String, String)> = ledger
        .iter()
        .filter(|row| row.set == HOST_GATED)
        .map(|row| (row.target.clone(), row.observed.clone()))
        .collect();
    host_gated.sort();

    SuiteLedgerAudit {
        assigned: claims.len() - ghosts.len(),
        unassigned,
        ghosts,
        duplicated,
        reasonless,
        empty_sets,
        host_gated,
    }
}

/// #2025 c4/c5: membership is enforced, not remembered.
///
/// Reads `tests/suites.txt` and walks the test directories. No cargo metadata, no
/// build, no run — it fires on any host in milliseconds, which matters because
/// the checks that failed to notice three red targets were the expensive ones
/// that return green early where the host is unsupported.
#[test]
fn every_test_target_belongs_to_a_named_set() {
    let ledger = parse_suite_ledger(include_str!("suites.txt"));
    let discovered = discover_test_targets();
    let audit = audit_suite_ledger(&ledger, &discovered);

    // Printed on every run, pass or fail: the denominator is stated, never
    // implied.
    eprintln!(
        "suite membership: {} of {} declared test target(s) in a named set; {} in none:",
        audit.assigned,
        discovered.len(),
        audit.unassigned.len()
    );
    for target in &audit.unassigned {
        eprintln!("  {target}");
    }

    // Named first, because a row naming a file that does not exist reads exactly
    // like a set assignment that changed.
    assert!(
        audit.ghosts.is_empty(),
        "tests/suites.txt names {} target(s) with no matching `.rs` file: {:?}. A nonexistent \
         target is a stale row to delete, never a set that failed.",
        audit.ghosts.len(),
        audit.ghosts
    );
    assert!(
        audit.duplicated.is_empty(),
        "{} target(s) appear in more than one section of tests/suites.txt, breaking that file's \
         own invariant that every target belongs to exactly one set:\n{}",
        audit.duplicated.len(),
        audit.duplicated.join("\n")
    );
    assert!(
        audit.reasonless.is_empty(),
        "{} target(s) are parked in `{HOST_GATED}:` without saying what gates them:\n{}\nA target \
         held out states its gate, or it is not held out — it is hidden, which is the whole \
         defect (#2025).",
        audit.reasonless.len(),
        audit.reasonless.join("\n")
    );
    assert!(
        audit.empty_sets.is_empty(),
        "these named sets have no members: {:?}. An empty set is a name that will be cited as \
         coverage it does not have — delete the name or give it members.",
        audit.empty_sets
    );

    // The pins below only mean something while this identity holds: with no
    // ghosts and no duplicates, assigned + unassigned IS the inventory, so
    // neither pin can be satisfied by moving a target out of the accounting.
    assert_eq!(
        audit.assigned + audit.unassigned.len(),
        discovered.len(),
        "suite accounting is broken: {} assigned + {} unassigned != {} discovered target(s).",
        audit.assigned,
        audit.unassigned.len(),
        discovered.len()
    );
    assert!(
        audit.unassigned.len() <= UNASSIGNED_CEILING,
        "{} declared test target(s) belong to NO verification set (ceiling \
         {UNASSIGNED_CEILING}):\n  {}\nThat is how `golden`, `ban_bare_panic` and `jit_run` were \
         each found red long after they broke. Put the target in the set it belongs to, or park \
         it in `{HOST_GATED}:` with the observed gate AND a row in HOST_GATED_HELD_OUT.",
        audit.unassigned.len(),
        audit.unassigned.join("\n  ")
    );
    assert!(
        audit.assigned >= TEST_TARGET_FLOOR,
        "tests/suites.txt assigns {} target(s) but the ratchet floor is {TEST_TARGET_FLOOR}; rows \
         may only GROW. A target that lost its row did not change set — it left the sets and went \
         back to being unwatched. Lower the floor only in the same diff as a deleted test file.",
        audit.assigned
    );

    // Exact, not a ceiling: a newly parked target fails until its gate is written
    // down here, and a target that can run again fails until its row leaves both
    // places, so neither list can outlive its defect.
    let held_out: Vec<(String, String)> = HOST_GATED_HELD_OUT
        .iter()
        .map(|(target, observed, _)| ((*target).to_string(), (*observed).to_string()))
        .collect();
    assert_eq!(
        audit.host_gated,
        held_out,
        "the `{HOST_GATED}:` section of tests/suites.txt and HOST_GATED_HELD_OUT disagree. A new \
         entry is a target that stopped being routine; a vanished entry is one that can run again \
         and must leave both places in the same diff. Held out: {}",
        HOST_GATED_HELD_OUT
            .iter()
            .map(|(target, observed, why)| format!("{target} ({observed}) — {why}"))
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// #2025 negative control: the membership law actually fires.
///
/// The check above passes today only because every target happens to carry a
/// row, so "it passed" says nothing about whether it CAN fail — and a ledger that
/// stayed green through three falsifications is exactly the history
/// `tests/jit_gaps.txt` has. Hand the audit an inventory with a target no section
/// names, a target two sections claim, a row with no file, and a parked row with
/// no gate, and require it to name each one.
#[test]
fn suite_ledger_audit_fires_on_a_target_in_no_set() {
    let row = |target: &str, set: &'static str, observed: &str| SuiteRow {
        target: target.to_string(),
        set,
        observed: observed.to_string(),
    };
    let ledger = vec![
        row("tests/golden", "guards", ""),
        row("tests/dev", "dev_loop", ""),
        row("tests/dev", "language", ""),
        row("tests/gone_yesterday", "guards", ""),
        row("tests/os_interrupt_windows", HOST_GATED, ""),
    ];
    let discovered = vec![
        "tests/dev".to_string(),
        "tests/golden".to_string(),
        "tests/jit_run".to_string(),
        "tests/os_interrupt_windows".to_string(),
    ];

    let audit = audit_suite_ledger(&ledger, &discovered);

    assert_eq!(
        audit.unassigned,
        vec!["tests/jit_run".to_string()],
        "a target in no set must be named, not counted away"
    );
    assert_eq!(
        audit.ghosts,
        vec!["tests/gone_yesterday".to_string()],
        "a row with no file must read as a stale row, never as a set change"
    );
    assert_eq!(audit.duplicated.len(), 1, "{:?}", audit.duplicated);
    assert!(
        audit.duplicated[0].contains("tests/dev")
            && audit.duplicated[0].contains("dev_loop")
            && audit.duplicated[0].contains("language"),
        "the duplicate must name both claiming sets: {:?}",
        audit.duplicated
    );
    assert_eq!(audit.reasonless.len(), 1, "{:?}", audit.reasonless);
    assert!(
        audit.reasonless[0].contains("tests/os_interrupt_windows"),
        "a parked row with no gate must be named: {:?}",
        audit.reasonless
    );
    assert_eq!(
        audit.empty_sets,
        vec!["runtime", "tooling", "surface"],
        "a named set with no members must be named too"
    );
    // Ghost rows classify nothing, so they do not pay for a target that fell out
    // of the ledger: three real targets are assigned, not four.
    assert_eq!(
        audit.assigned, 3,
        "only targets that exist count toward the floor"
    );
    assert_eq!(
        audit.assigned + audit.unassigned.len(),
        discovered.len(),
        "the identity the pins rest on must hold on synthetic input too"
    );
    assert_eq!(
        audit.host_gated,
        vec![("tests/os_interrupt_windows".to_string(), String::new())],
        "the parked section is reported for the exact compare"
    );
}

/// #2025: the sets are executed, not described.
///
/// A ledger no runner reads is a second place to remember membership, which is
/// the defect wearing a different hat. `scripts/agent/time-suites.sh` takes
/// `--set <name>` and derives the target list from this file, so there is one
/// list of sets and one list of members (AGENTS.md I8).
#[test]
fn the_suite_runner_reads_the_ledger() {
    let runner = include_str!("../scripts/agent/time-suites.sh");
    assert!(
        runner.contains("tests/suites.txt"),
        "scripts/agent/time-suites.sh no longer reads tests/suites.txt, so the named sets are \
         decorative again and a target can be in a set nothing runs"
    );
    // `host_gated` is the one name the runner may say, because it is the one
    // section it must NOT run. Every executable set name comes out of the ledger,
    // so the runner cannot know a set the ledger does not (AGENTS.md I8).
    assert!(
        runner.contains(HOST_GATED),
        "scripts/agent/time-suites.sh must name `{HOST_GATED}` to exclude it from `--set all`; \
         otherwise `all` tries to run the targets that are parked precisely because they cannot"
    );
    for set in SUITE_ORDER {
        if set == HOST_GATED {
            continue;
        }
        assert!(
            !runner.contains(set),
            "scripts/agent/time-suites.sh hardcodes the set name `{set}`; executable set names \
             come from tests/suites.txt so the runner and the ledger cannot disagree"
        );
    }
}
