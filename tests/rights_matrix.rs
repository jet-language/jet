//! Phase-A differential matrix for the canonical rights-row walker.
//!
//! Each legacy oracle is deliberately test-local: it records the old verdict
//! relation without creating another production checker. `RightsRow::observe`
//! compares that relation with the row used by sema, so a future row change
//! identifies the exact right and callable that drifted.

use jet_foundation::Authority::{self, Holds, Verdict};
use jet_foundation::sema::{RightsProvenance, RightsRow};

struct Case {
    name: &'static str,
    row: RightsRow,
    effects: Holds,
    legacy: fn(&str) -> Verdict,
}

fn legacy_pure(_right: &str) -> Verdict {
    Verdict::Missing
}

fn legacy_replayable(right: &str) -> Verdict {
    match Authority::root(right) {
        "Time" | "Rand" | "Net" | "IO" => Verdict::Denied,
        _ => Verdict::Allowed,
    }
}

fn legacy_comptime(right: &str) -> Verdict {
    if Authority::covers("Mem", right) {
        Verdict::Allowed
    } else {
        Verdict::Missing
    }
}

fn legacy_invocation(right: &str) -> Verdict {
    if Authority::covers("FS.Write", right) {
        Verdict::Denied
    } else if Authority::covers("FS", right) || Authority::covers("Time", right) {
        Verdict::Allowed
    } else {
        Verdict::Missing
    }
}

fn case_rows() -> Vec<Case> {
    vec![
        Case {
            name: "pure-empty-row",
            row: RightsRow::pure(RightsProvenance::new(
                "rights_matrix::pure",
                "legacy purity oracle",
            )),
            effects: Holds::from(["IO".to_string()]),
            legacy: legacy_pure,
        },
        Case {
            name: "replayable-ambient-denials",
            row: RightsRow::replayable(RightsProvenance::new(
                "rights_matrix::replayable",
                "legacy replayability oracle",
            )),
            effects: Holds::from([
                "Time".to_string(),
                "Rand.Draw".to_string(),
                "Net.HTTP.Get".to_string(),
                "IO".to_string(),
                "FS.Read".to_string(),
            ]),
            legacy: legacy_replayable,
        },
        Case {
            name: "comptime-memory-only",
            row: RightsRow::comptime(RightsProvenance::new(
                "rights_matrix::comptime",
                "legacy comptime oracle",
            )),
            effects: Holds::from(["Mem.Alloc".to_string(), "FS.Read".to_string()]),
            legacy: legacy_comptime,
        },
        Case {
            name: "invocation-allow-deny",
            row: RightsRow::invocation(
                ["FS", "Time"],
                ["FS.Write"],
                RightsProvenance::new("rights_matrix::invocation", "legacy invocation oracle"),
            ),
            effects: Holds::from([
                "FS.Read".to_string(),
                "FS.Write".to_string(),
                "Time".to_string(),
                "Net".to_string(),
            ]),
            legacy: legacy_invocation,
        },
    ]
}

#[test]
fn canonical_rows_match_legacy_rights_verdicts() {
    for case in case_rows() {
        let call_chain = vec![case.name.to_string()];
        let observed = case
            .row
            .observe(
                case.name,
                &case.effects,
                &call_chain,
                &[],
                case.legacy,
            )
            .unwrap_or_else(|mismatch| panic!("rights row drift: {mismatch:#?}"));
        let walked = case.row.walk(&case.effects, &call_chain, &[]);
        assert_eq!(observed, walked, "observed row changed for {}", case.name);
        for right in &case.effects {
            assert_eq!(case.row.decide(right), (case.legacy)(right), "right {right}");
        }
    }
}

#[test]
fn invocation_deny_wins_after_leaf_expansion() {
    let row = RightsRow::invocation(
        ["FS"],
        ["FS.Write"],
        RightsProvenance::new("rights_matrix::precedence", "deny-wins oracle"),
    );
    assert_eq!(row.decide("FS.Read"), Verdict::Allowed);
    assert_eq!(row.decide("FS.Write"), Verdict::Denied);
    assert_eq!(row.decide("FS.Write:/tmp/out"), Verdict::Denied);
}
