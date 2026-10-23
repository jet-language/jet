//! Deterministic diagnostic repair-coverage facts and ratchet validation.

use std::collections::BTreeMap;

use crate::Report::{NoFixReasonKind, ReportEnvelope};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageEntry {
    pub plane: String,
    pub fix_edits: bool,
    pub no_fix_reason: Option<NoFixReasonKind>,
}

impl CoverageEntry {
    pub fn new(
        plane: impl Into<String>,
        fix_edits: bool,
        no_fix_reason: Option<NoFixReasonKind>,
    ) -> Self {
        Self {
            plane: plane.into(),
            fix_edits,
            no_fix_reason,
        }
    }

    pub fn from_report(plane: impl Into<String>, report: &ReportEnvelope) -> Self {
        Self::new(
            plane,
            !report.fix_edits().is_empty(),
            report.no_fix_reason().map(|reason| reason.kind),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverageFact {
    pub plane: String,
    pub rows: usize,
    pub edit_covered: usize,
    pub reason_covered: usize,
    pub behavior_reasons: usize,
    pub design_reasons: usize,
    pub ambiguous_reasons: usize,
    pub uncovered: usize,
}

impl CoverageFact {
    pub fn is_closed(&self) -> bool {
        self.uncovered == 0
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"plane\":{},\"rows\":{},\"edit_covered\":{},\"reason_covered\":{},\"behavior_reasons\":{},\"design_reasons\":{},\"ambiguous_reasons\":{},\"uncovered\":{}}}",
            crate::JSON::quote(&self.plane),
            self.rows,
            self.edit_covered,
            self.reason_covered,
            self.behavior_reasons,
            self.design_reasons,
            self.ambiguous_reasons,
            self.uncovered,
        )
    }
}

pub fn try_facts(entries: impl IntoIterator<Item = CoverageEntry>) -> Result<Vec<CoverageFact>, String> {
    let mut facts = BTreeMap::<String, CoverageFact>::new();
    for entry in entries {
        if entry.plane.trim().is_empty() {
            return Err("diagnostic coverage plane must not be empty".to_string());
        }
        if entry.fix_edits && entry.no_fix_reason.is_some() {
            return Err(format!(
                "coverage row in plane `{}` has both fix_edits and no_fix_reason",
                entry.plane
            ));
        }
        let fact = facts.entry(entry.plane.clone()).or_insert_with(|| CoverageFact {
            plane: entry.plane,
            ..CoverageFact::default()
        });
        fact.rows += 1;
        if entry.fix_edits {
            fact.edit_covered += 1;
        } else if let Some(kind) = entry.no_fix_reason {
            fact.reason_covered += 1;
            match kind {
                NoFixReasonKind::Behavior => fact.behavior_reasons += 1,
                NoFixReasonKind::Design => fact.design_reasons += 1,
                NoFixReasonKind::Ambiguous => fact.ambiguous_reasons += 1,
            }
        } else {
            fact.uncovered += 1;
        }
    }
    Ok(facts.into_values().collect())
}

pub fn facts(entries: &[CoverageEntry]) -> Vec<CoverageFact> {
    try_facts(entries.iter().cloned()).expect("invalid diagnostic coverage row")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoverageBaselineRow {
    pub edit_covered: usize,
    pub reason_covered: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverageBaseline {
    pub rows: BTreeMap<String, CoverageBaselineRow>,
}

impl CoverageBaseline {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut baseline = Self::default();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 4 {
                return Err(format!(
                    "coverage baseline line {} must contain plane, edit count, reason count, and total",
                    index + 1
                ));
            }
            let row = CoverageBaselineRow {
                edit_covered: fields[1]
                    .parse()
                    .map_err(|_| format!("invalid edit count on coverage baseline line {}", index + 1))?,
                reason_covered: fields[2].parse().map_err(|_| {
                    format!("invalid reason count on coverage baseline line {}", index + 1)
                })?,
                total: fields[3]
                    .parse()
                    .map_err(|_| format!("invalid total on coverage baseline line {}", index + 1))?,
            };
            if row.edit_covered + row.reason_covered > row.total {
                return Err(format!(
                    "coverage baseline line {} counts more covered rows than total",
                    index + 1
                ));
            }
            if baseline.rows.insert(fields[0].to_string(), row).is_some() {
                return Err(format!("duplicate coverage baseline plane `{}`", fields[0]));
            }
        }
        Ok(baseline)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for (plane, row) in &self.rows {
            out.push_str(&format!(
                "{} {} {} {}\n",
                plane, row.edit_covered, row.reason_covered, row.total
            ));
        }
        out
    }
    /// Project the ratified floor as reportable facts without changing the
    /// stored baseline. This keeps edit coverage and reviewed-reason coverage
    /// visible as separate counters.
    pub fn facts(&self) -> Vec<CoverageFact> {
        self.rows
            .iter()
            .map(|(plane, row)| CoverageFact {
                plane: plane.clone(),
                rows: row.total,
                edit_covered: row.edit_covered,
                reason_covered: row.reason_covered,
                uncovered: row
                    .total
                    .saturating_sub(row.edit_covered + row.reason_covered),
                ..CoverageFact::default()
            })
            .collect()
    }


    pub fn from_facts(facts: &[CoverageFact]) -> Self {
        let rows = facts
            .iter()
            .map(|fact| {
                (
                    fact.plane.clone(),
                    CoverageBaselineRow {
                        edit_covered: fact.edit_covered,
                        reason_covered: fact.reason_covered,
                        total: fact.rows,
                    },
                )
            })
            .collect();
        Self { rows }
    }

    pub fn validate(&self, current: &[CoverageFact]) -> Result<(), String> {
        let current = current
            .iter()
            .map(|fact| (fact.plane.as_str(), fact))
            .collect::<BTreeMap<_, _>>();
        for (plane, previous) in &self.rows {
            let Some(fact) = current.get(plane.as_str()) else {
                return Err(format!("coverage plane `{plane}` disappeared"));
            };
            let current_row = CoverageBaselineRow {
                edit_covered: fact.edit_covered,
                reason_covered: fact.reason_covered,
                total: fact.rows,
            };
            validate_non_decreasing_row(plane, *previous, current_row)?;
            let previous_uncovered = previous
                .total
                .saturating_sub(previous.edit_covered + previous.reason_covered);
            if current_row.total > previous.total {
                let current_uncovered = current_row
                    .total
                    .saturating_sub(current_row.edit_covered + current_row.reason_covered);
                if current_uncovered > previous_uncovered {
                    return Err(format!(
                        "new row in open coverage plane `{plane}` has neither fix_edits nor no_fix_reason"
                    ));
                }
            }
        }
        for fact in current.values() {
            if !self.rows.contains_key(fact.plane.as_str()) && fact.uncovered > 0 {
                return Err(format!(
                    "new coverage plane `{}` has a row with neither fix_edits nor no_fix_reason",
                    fact.plane
                ));
            }
        }
        Ok(())
    }
}

pub fn validate_non_decreasing(previous: &CoverageFact, current: &CoverageFact) -> Result<(), String> {
    if previous.plane != current.plane {
        return Err("coverage ratchet compares different planes".to_string());
    }
    validate_non_decreasing_row(
        &current.plane,
        CoverageBaselineRow {
            edit_covered: previous.edit_covered,
            reason_covered: previous.reason_covered,
            total: previous.rows,
        },
        CoverageBaselineRow {
            edit_covered: current.edit_covered,
            reason_covered: current.reason_covered,
            total: current.rows,
        },
    )
}

fn validate_non_decreasing_row(
    plane: &str,
    previous: CoverageBaselineRow,
    current: CoverageBaselineRow,
) -> Result<(), String> {
    if current.edit_covered < previous.edit_covered
        || current.reason_covered < previous.reason_covered
        || current.total < previous.total
    {
        return Err(format!("coverage baseline decreased for plane `{plane}`"));
    }
    if current.edit_covered + current.reason_covered > current.total {
        return Err(format!("coverage counts exceed total for plane `{plane}`"));
    }
    Ok(())
}

pub fn validate_registry_coverage(
    entries: impl IntoIterator<Item = CoverageEntry>,
    baseline: &CoverageBaseline,
) -> Result<Vec<CoverageFact>, String> {
    let facts = try_facts(entries)?;
    baseline.validate(&facts)?;
    Ok(facts)
}
