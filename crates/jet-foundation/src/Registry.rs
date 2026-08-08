//! D-META-REG1=A: one registration table.
//!
//! A marker rule, a knowledge plane, a right, and a build fact are rows of the
//! same table, separated only by what they attach to. Reflection, `jet explain`,
//! and the drift guards are written once here and serve all four kinds; nothing
//! downstream keeps a second table or a second guard per kind.
//!
//! D-FACT-LAW1=B puts the law on the row itself. A fact moves toward safety
//! silently; every move away is one written word at the site. So every row
//! states its safe direction and the gate words that move it the other way. A
//! row with no meaningful direction states `None` and names no gate;
//! `law_violations` fails a row that states one without the other.
//!
//! D-FACT-OWN1=A adds one row shape for a fact a prover publishes. The
//! ownership prover is never a plane: it publishes sendability, view
//! provenance, and moved-ness as read-only rows with no plane algebra, so tools
//! and other planes read them like any other fact.

use std::sync::LazyLock;

use crate::Policy::{AppliedRule, RuleSite, APPLIED_RULES};

/// What a row attaches to. This is the whole difference between the four uses
/// of the one table, so `RowKind` is read off the target rather than stated
/// twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowTarget {
    /// A rule on written code. The sites are the row's legal attachment points.
    Code(&'static [RuleSite]),
    /// Knowledge about a value.
    Value,
    /// What a scope may do.
    Scope,
    /// What the build knows.
    Build,
}

/// The four uses of the one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Marker,
    Plane,
    Right,
    Fact,
}

impl RowKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Marker => "marker",
            Self::Plane => "plane",
            Self::Right => "right",
            Self::Fact => "fact",
        }
    }
}

impl RowTarget {
    pub const fn kind(self) -> RowKind {
        match self {
            Self::Code(_) => RowKind::Marker,
            Self::Value => RowKind::Plane,
            Self::Scope => RowKind::Right,
            Self::Build => RowKind::Fact,
        }
    }
}

/// D-FACT-LAW1=B / D-FACT-WORD1=A: the direction a row's facts move for free.
/// The law reads "tighten" and "loosen" in every diagnostic and doc; this column
/// says what tightening *is* for this row, because it is a different act per
/// plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeDirection {
    /// Learning more is free. Exactness, flow facts, taint.
    Gain,
    /// Giving up power is free. Rights, package policy, build settings.
    Shrink,
    /// Finishing the job is free. Duty: a bound handle owes `join`.
    Discharge,
    /// This row holds no fact that moves, so it states no direction. A rule on
    /// written code is the ordinary case; so is a read-only prover row.
    None,
}

impl SafeDirection {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gain => "gain",
            Self::Shrink => "shrink",
            Self::Discharge => "discharge",
            Self::None => "none",
        }
    }
}

/// One row of the one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryRow {
    pub name: &'static str,
    /// What the row attaches to. Also names its kind.
    pub target: RowTarget,
    /// D-FACT-LAW1=B: which way this row's facts move for free.
    pub safe_direction: SafeDirection,
    /// D-FACT-LAW1=B: the written words that move them the other way.
    pub gates: &'static [&'static str],
    /// D-FACT-OWN1=A: the prover that publishes this row, for a read-only row
    /// that carries no plane algebra. `None` for a declared row.
    pub published_by: Option<&'static str>,
    /// The marker signature, for a row whose target is written code.
    pub rule: Option<&'static AppliedRule>,
    /// The ratified decision this row answers to.
    pub decision: &'static str,
}

impl RegistryRow {
    pub const fn kind(&self) -> RowKind {
        self.target.kind()
    }

    /// True for a read-only row a prover publishes (D-FACT-OWN1=A).
    pub const fn is_prover_supplied(&self) -> bool {
        self.published_by.is_some()
    }
}

/// D-META-REG1=A: a marker row states no direction. A rule on written code says
/// what a writer may attach and where; it holds no fact that moves toward or
/// away from safety, so it states `none` and names no gate. The moving facts a
/// marker *writes* belong to the plane, right, or build row that holds them —
/// `#Caps` and `#Grant` are gate words on the `Rights` row, not directions of
/// their own. Stated once here for every marker row, so no row can drift.
fn marker_row(rule: &'static AppliedRule) -> RegistryRow {
    RegistryRow {
        name: rule.name,
        target: RowTarget::Code(rule.sites),
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: None,
        rule: Some(rule),
        decision: "D-VERDICT-1455-1",
    }
}

/// The gate words that are not markers: Prelude calls and settings a writer
/// spells at the site to loosen a fact. Every other gate word is a marker row of
/// this table. `law_violations` fails a row that names a word from neither list,
/// so no row can invent a gate that nothing spells.
const PRELUDE_GATES: &[&str] = &[
    // D-TYPE2-EXACT1: the certainty gates.
    "approx",
    "raw",
    "wrapping",
    // D-CONC-JOIN1: the duty gates.
    "detach",
    "drop",
    // D-CONF-MERGE1: the audited build exception.
    "Force",
];

/// The rows that attach to something other than written code: the named
/// instances of the one law (D-FACT-LAW1=B) and the read-only rows the
/// ownership prover publishes (D-FACT-OWN1=A).
const NON_CODE_ROWS: &[RegistryRow] = &[
    RegistryRow {
        name: "Exactness",
        target: RowTarget::Value,
        safe_direction: SafeDirection::Gain,
        gates: &["approx", "raw", "wrapping"],
        published_by: None,
        rule: None,
        decision: "D-TYPE2-EXACT1",
    },
    // A narrowed optional gains certainty for free and the fact ends at the
    // branch boundary, so nothing loosens it and this row names no gate.
    RegistryRow {
        name: "Flow",
        target: RowTarget::Value,
        safe_direction: SafeDirection::Gain,
        gates: &[],
        published_by: None,
        rule: None,
        decision: "D-FLOWTYPE1",
    },
    RegistryRow {
        name: "Taint",
        target: RowTarget::Value,
        safe_direction: SafeDirection::Gain,
        gates: &["Scrub"],
        published_by: None,
        rule: None,
        decision: "D-TAG-SURFACE1",
    },
    RegistryRow {
        name: "Duty",
        target: RowTarget::Value,
        safe_direction: SafeDirection::Discharge,
        gates: &["detach", "drop"],
        published_by: None,
        rule: None,
        decision: "D-CONC-JOIN1",
    },
    RegistryRow {
        name: "Rights",
        target: RowTarget::Scope,
        safe_direction: SafeDirection::Shrink,
        gates: &["Unsafe", "Impure", "Grant"],
        published_by: None,
        rule: None,
        decision: "D-AUTHORITY-MODEL1",
    },
    RegistryRow {
        name: "PackagePolicy",
        target: RowTarget::Scope,
        safe_direction: SafeDirection::Shrink,
        gates: &["Unsafe", "Grant"],
        published_by: None,
        rule: None,
        decision: "D-PACKAGE-POLICY-SCOPE1",
    },
    RegistryRow {
        name: "BuildSettings",
        target: RowTarget::Build,
        safe_direction: SafeDirection::Shrink,
        gates: &["Force"],
        published_by: None,
        rule: None,
        decision: "D-CONF-MERGE1",
    },
    // D-FACT-OWN1=A: the ownership prover is not a plane. These three rows are
    // what it publishes, read-only, with no plane algebra and no gate — a
    // window is closed by the prover, never loosened by a written word.
    RegistryRow {
        name: "Sendability",
        target: RowTarget::Value,
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: Some("ownership"),
        rule: None,
        decision: "D-FACT-OWN1",
    },
    RegistryRow {
        name: "ViewProvenance",
        target: RowTarget::Value,
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: Some("ownership"),
        rule: None,
        decision: "D-MEMPROVENANCE3",
    },
    RegistryRow {
        name: "Movedness",
        target: RowTarget::Value,
        safe_direction: SafeDirection::None,
        gates: &[],
        published_by: Some("ownership"),
        rule: None,
        decision: "D-MEM1",
    },
];

/// The one table. Marker rows come from the marker registry, every other kind
/// from `NON_CODE_ROWS`; nothing else may hold a row.
static REGISTRY: LazyLock<Vec<RegistryRow>> = LazyLock::new(|| {
    APPLIED_RULES
        .iter()
        .map(marker_row)
        .chain(NON_CODE_ROWS.iter().copied())
        .collect()
});

/// Every registered row, of every kind.
pub fn rows() -> &'static [RegistryRow] {
    &REGISTRY
}

/// One lookup for every kind. Row names are unique across the table
/// (`law_violations` proves it), so a name is enough.
pub fn row(name: &str) -> Option<&'static RegistryRow> {
    rows().iter().find(|row| row.name == name)
}

/// The drift guard for the two law columns (D-FACT-LAW1=B), and for the one
/// name space the table keeps. One implementation: the law-zero coverage guard
/// calls this, and no kind gets a second one.
///
/// A row cannot state neither column — both are fields, so the build fails at
/// the row itself. What this reads is everything a compiler cannot: a gate named
/// with no direction to loosen, a prover row that claims plane algebra, a gate
/// word nothing spells, and a name registered twice.
pub fn law_violations() -> Vec<String> {
    check(rows())
}

fn check(rows: &[RegistryRow]) -> Vec<String> {
    let mut violations = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for row in rows {
        if seen.contains(&row.name) {
            violations.push(format!(
                "`{}` is registered twice; one table means one row per name",
                row.name
            ));
        } else {
            seen.push(row.name);
        }
        if row.safe_direction == SafeDirection::None && !row.gates.is_empty() {
            violations.push(format!(
                "`{}` ({}) names a gate word but states no safe direction; \
                 a gate loosens a direction, so say which way tightens",
                row.name,
                row.kind().name()
            ));
        }
        for gate in row.gates {
            let spelled = PRELUDE_GATES.contains(gate)
                || rows
                    .iter()
                    .any(|candidate| candidate.name == *gate && candidate.rule.is_some());
            if !spelled {
                violations.push(format!(
                    "`{}` names the gate word `{gate}`, which nothing spells; \
                     a gate is a registered marker or a Prelude gate",
                    row.name
                ));
            }
        }
        if row.is_prover_supplied() && row.safe_direction != SafeDirection::None {
            violations.push(format!(
                "`{}` is published by a prover, so it carries no plane algebra \
                 and must state safe direction `none` (D-FACT-OWN1=A)",
                row.name
            ));
        }
        if matches!(row.target, RowTarget::Code(_)) != row.rule.is_some() {
            violations.push(format!(
                "`{}` attaches to written code exactly when it carries a marker signature",
                row.name
            ));
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::{law_violations, row, rows, RowKind, RowTarget, SafeDirection};

    #[test]
    fn the_one_table_holds_all_four_kinds() {
        for kind in [RowKind::Marker, RowKind::Plane, RowKind::Right, RowKind::Fact] {
            assert!(
                rows().iter().any(|row| row.kind() == kind),
                "no {} row in the one table",
                kind.name()
            );
        }
    }

    #[test]
    fn a_target_names_its_kind() {
        assert_eq!(RowTarget::Code(&[]).kind(), RowKind::Marker);
        assert_eq!(RowTarget::Value.kind(), RowKind::Plane);
        assert_eq!(RowTarget::Scope.kind(), RowKind::Right);
        assert_eq!(RowTarget::Build.kind(), RowKind::Fact);
    }

    #[test]
    fn every_row_obeys_the_one_way_law() {
        assert_eq!(law_violations(), Vec::<String>::new());
    }

    /// The guard has to catch a bad row, not only pass a good table.
    #[test]
    fn the_guard_names_a_row_that_breaks_the_law() {
        use super::{check, RegistryRow};

        let gate_with_no_direction = RegistryRow {
            name: "Wrong",
            target: RowTarget::Value,
            safe_direction: SafeDirection::None,
            gates: &["approx"],
            published_by: None,
            rule: None,
            decision: "D-TEST",
        };
        assert_eq!(check(&[gate_with_no_direction]).len(), 1);

        let unspelled_gate = RegistryRow {
            gates: &["Trust"],
            safe_direction: SafeDirection::Gain,
            ..gate_with_no_direction
        };
        assert!(check(&[unspelled_gate])[0].contains("nothing spells"));

        let prover_with_algebra = RegistryRow {
            gates: &[],
            safe_direction: SafeDirection::Gain,
            published_by: Some("ownership"),
            ..gate_with_no_direction
        };
        assert!(check(&[prover_with_algebra])[0].contains("no plane algebra"));

        assert_eq!(check(&[gate_with_no_direction, gate_with_no_direction]).len(), 3);
    }

    #[test]
    fn a_prover_row_is_read_only() {
        let sendability = row("Sendability").expect("the prover publishes Sendability");
        assert!(sendability.is_prover_supplied());
        assert_eq!(sendability.safe_direction, SafeDirection::None);
        assert!(sendability.gates.is_empty());
    }
}
