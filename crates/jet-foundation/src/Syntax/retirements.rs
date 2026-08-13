//! D-ONCE-RETIRE1=C: a retirement is code, not a memo.
//!
//! Greenfield law deletes the replaced form in the same change. Before this
//! table there was no inventory, so a retired spelling could stay alive in the
//! corpus behind a silent fallback: no diagnostic, no count, no end date. The
//! ruling makes a retirement ship as three things at once, and this file is the
//! one home for the first of them — the row that says what was retired, what
//! replaced it, which decision retired it, and how the compiler answers a file
//! still on the old form.
//!
//! The two categories come from the ruling:
//!
//! * A **rename** is the same meaning under a new spelling. `jet fmt` and
//!   `jet fix` rewrite it mechanically and print a notice naming the row. A
//!   rename row names no diagnostic code, because nothing is refused.
//! * A **semantic change** means the old form no longer means anything. It is a
//!   hard error with what, why and fix, and the row names the registered code.
//!
//! The third piece is the adoption ratchet. Every row here has a matching
//! ceiling in `tests/retirement_ratchet.rs`: the number of repository files
//! still on the retired form. That count may fall and never rise, and the
//! retirement is finished only when it reaches zero.
//!
//! The spellings are read from the constants that already own them. This table
//! pairs them; it never restates them.

use super::{
    COMPTIME_MARK, DEFAULT_ENTRY_FILE, INTERPOLATION_SELECTOR_EXAMPLE, LEGACY_ENTRY_FILE,
    PACKAGE_FILE, PAYLOAD_FILE, RETIRED_COMPTIME_MARK, RETIRED_TARGET_PLUGIN,
    RETIRED_INTERPOLATION_SELECTOR_EXAMPLE, TARGET_SANDBOX,
};

/// How the compiler answers a file still written in the retired form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetirementKind {
    /// Same meaning, new spelling. `jet fmt` and `jet fix` rewrite it and print
    /// a notice. Nothing is refused, so the row names no code.
    Rename,
    /// The old form has no meaning left. It is refused with what, why and fix,
    /// and the row names the registered code.
    Semantic,
}

/// One retired form and the form that replaced it.
#[derive(Debug, Clone, Copy)]
pub struct Retirement {
    /// Stable name for the row. The ratchet table keys on it.
    pub id: &'static str,
    /// What a file still on the old form is written with.
    pub retired: &'static str,
    /// What replaces it.
    pub canonical: &'static str,
    pub kind: RetirementKind,
    /// The decision that retired the old form.
    pub decision: &'static str,
    /// The date that decision was ratified, as `YYYY-MM-DD`.
    pub since: &'static str,
    /// The registered diagnostic code. `Some` for a semantic change, `None` for
    /// a rename.
    pub code: Option<&'static str>,
}

/// Every retirement the compiler knows about.
pub const RETIREMENTS: &[Retirement] = &[
    Retirement {
        id: "entry-file",
        retired: LEGACY_ENTRY_FILE,
        canonical: DEFAULT_ENTRY_FILE,
        kind: RetirementKind::Rename,
        decision: "D-VERDICT-678-1",
        since: "2026-07-17",
        code: None,
    },
    Retirement {
        id: "manifest-file",
        retired: PAYLOAD_FILE,
        canonical: PACKAGE_FILE,
        kind: RetirementKind::Rename,
        decision: "D-ECO-FILEROOT1",
        since: "2026-06-30",
        code: None,
    },
    Retirement {
        id: "manifest-identity",
        retired: "payload: {",
        canonical: "name:",
        kind: RetirementKind::Semantic,
        decision: "D-CONF-NAME1",
        since: "2026-08-06",
        code: Some("E1206"),
    },
    Retirement {
        id: "package-ref-order",
        retired: "provider@target",
        canonical: "target@provider",
        kind: RetirementKind::Semantic,
        decision: "D-JPK-REF1",
        since: "2026-06-18",
        code: Some("E1317"),
    },
    Retirement {
        id: "interpolation-selector-rail",
        retired: RETIRED_INTERPOLATION_SELECTOR_EXAMPLE,
        canonical: INTERPOLATION_SELECTOR_EXAMPLE,
        kind: RetirementKind::Rename,
        decision: "D-ONCE-HASH1",
        since: "2026-08-07",
        code: None,
    },
    Retirement {
        id: "comptime-mark",
        retired: RETIRED_COMPTIME_MARK,
        canonical: COMPTIME_MARK,
        kind: RetirementKind::Rename,
        decision: "D-ONCE-AT1",
        since: "2026-08-07",
        code: None,
    },
    Retirement {
        id: "set-take",
        retired: "Set.take",
        canonical: "Set.pop",
        kind: RetirementKind::Rename,
        decision: "D-ONCE-VERB1",
        since: "2026-08-07",
        code: None,
    },
    Retirement {
        id: "map-replace",
        retired: "Map.replace",
        canonical: "Map.add",
        kind: RetirementKind::Rename,
        decision: "D-ONCE-VERB1",
        since: "2026-08-07",
        code: None,
    },
    Retirement {
        id: "set-replace",
        retired: "Set.replace",
        canonical: "Set.add",
        kind: RetirementKind::Rename,
        decision: "D-ONCE-VERB1",
        since: "2026-08-07",
        code: None,
    },
    Retirement {
        id: "allow-impure",
        retired: "--allow-impure",
        canonical: "--gate impure=allow",
        kind: RetirementKind::Semantic,
        decision: "D-ONCE-GATE1=A",
        since: "2026-08-07",
        code: Some("E1343"),
    },
    Retirement {
        id: "core-path-free-functions",
        retired: "core.path.join/parent/extension/normalize",
        canonical: "Path.from(value).join(part), .parent(), .extension(), .normalize()",
        kind: RetirementKind::Semantic,
        decision: "D-CORE-PATH1",
        since: "2026-08-06",
        code: Some("E1001"),
    },
    Retirement {
        id: "target-plugin",
        retired: RETIRED_TARGET_PLUGIN,
        canonical: TARGET_SANDBOX,
        kind: RetirementKind::Rename,
        decision: "D-ONCE-SANDBOX1=A",
        since: "2026-08-07",
        code: None,
    },
];

/// The known package providers, in the order a ref may not put them.
/// `package-ref-order` reads this to spot a provider written first. This is
/// the same list `jet-pkg-model`'s `RefSpec::Source::is_builtin` reads —
/// `super::REF_SOURCE_PROVIDERS` (`Syntax/effects_surface.rs`) is the one
/// home; this is not a second copy.
pub const REF_PROVIDERS: &[&str] = super::REF_SOURCE_PROVIDERS;

/// The row with this id, if there is one.
pub fn retirement(id: &str) -> Option<&'static Retirement> {
    RETIREMENTS.iter().find(|row| row.id == id)
}

/// What `jet fmt` and `jet fix` rewrite a retired spelling to, when the
/// retirement is a rename. A semantic change answers `None`: it is refused, not
/// rewritten.
pub fn rename_target(retired: &str) -> Option<&'static str> {
    RETIREMENTS
        .iter()
        .find(|row| row.kind == RetirementKind::Rename && row.retired == retired)
        .map(|row| row.canonical)
}

/// The drift guard for this table. Every row must state a decision, a date, and
/// the answer its category requires; no two rows may claim the same id or the
/// same retired spelling. An empty answer means the table holds.
pub fn law_violations() -> Vec<String> {
    law_violations_of(RETIREMENTS)
}

/// The guard itself, over any table. Taking the rows as an argument is what
/// lets the guard be run against a table that carries a second copy, so the
/// test proves the guard catches one instead of only proving today's table is
/// clean.
pub fn law_violations_of(rows: &[Retirement]) -> Vec<String> {
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.retired.is_empty() || row.canonical.is_empty() {
            out.push(format!("{}: a row names an empty spelling", row.id));
        }
        if row.retired == row.canonical {
            out.push(format!("{}: retires the form it names as canonical", row.id));
        }
        if row.decision.is_empty() {
            out.push(format!("{}: names no decision", row.id));
        }
        if row.since.len() != 10 || row.since.match_indices('-').count() != 2 {
            out.push(format!("{}: `{}` is not a YYYY-MM-DD date", row.id, row.since));
        }
        match (row.kind, row.code) {
            (RetirementKind::Rename, Some(code)) => out.push(format!(
                "{}: a rename rewrites and refuses nothing, so it must name no code (`{code}`)",
                row.id
            )),
            (RetirementKind::Semantic, None) => out.push(format!(
                "{}: a semantic change is refused, so it must name its code",
                row.id
            )),
            (RetirementKind::Semantic, Some(code)) => {
                let registered = code.len() == 5
                    && code.starts_with('E')
                    && code[1..].chars().all(|c| c.is_ascii_digit());
                if !registered {
                    out.push(format!("{}: `{code}` is not a registered code", row.id));
                }
            }
            (RetirementKind::Rename, None) => {}
        }
        for other in &rows[index + 1..] {
            if other.id == row.id {
                out.push(format!("{}: two rows claim this id", row.id));
            }
            if other.retired == row.retired {
                out.push(format!(
                    "{}: `{}` is retired twice, so the two rows can disagree",
                    row.id, row.retired
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_obeys_its_own_law() {
        assert!(law_violations().is_empty(), "{:#?}", law_violations());
    }

    #[test]
    fn a_rename_rewrites_and_a_semantic_change_does_not() {
        assert_eq!(rename_target(LEGACY_ENTRY_FILE), Some(DEFAULT_ENTRY_FILE));
        assert_eq!(rename_target(PAYLOAD_FILE), Some(PACKAGE_FILE));
        assert_eq!(
            rename_target(RETIRED_INTERPOLATION_SELECTOR_EXAMPLE),
            Some(INTERPOLATION_SELECTOR_EXAMPLE)
        );
        assert_eq!(rename_target(RETIRED_COMPTIME_MARK), Some(COMPTIME_MARK));
        assert_eq!(rename_target("payload: {"), None);
        assert_eq!(rename_target("provider@target"), None);
    }

    #[test]
    fn every_semantic_retirement_names_the_code_that_refuses_it() {
        for row in RETIREMENTS {
            if row.kind == RetirementKind::Semantic {
                assert!(row.code.is_some(), "{} names no code", row.id);
            }
        }
    }

    #[test]
    fn a_second_copy_of_a_row_fails_the_guard() {
        // The guard counts definition sites, so a second copy is caught even
        // when both copies say the same thing today.
        let mut doubled = RETIREMENTS.to_vec();
        doubled.push(RETIREMENTS[0]);
        let violations = law_violations_of(&doubled);
        assert!(
            violations.iter().any(|line| line.contains("retired twice")),
            "{violations:#?}"
        );
    }

    #[test]
    fn a_rename_that_names_a_code_fails_the_guard() {
        let mut wrong = vec![RETIREMENTS[0]];
        wrong[0].code = Some("E1206");
        assert!(!law_violations_of(&wrong).is_empty());
    }
}
