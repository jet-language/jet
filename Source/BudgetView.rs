//! Read-only projections over canonical performance budget reports.
//!
//! D-PERFBUDGET-INTEGRATION1: consumers never evaluate policy or measure.
//! They verify reports produced by the one budget engine, then project only
//! reports whose recorded source digests still match the requested source.

use jet_foundation::PerformanceBudget::{verify_budget_report, CanonicalJson};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetFact {
    pub report_id: String,
    pub evidence_id: String,
    pub budget_id: String,
    pub evidence: String,
    pub outcome: String,
    pub enforcement: String,
    pub statistical: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BudgetProjection {
    pub facts: Vec<BudgetFact>,
    pub rejected: Vec<String>,
}

impl BudgetProjection {
    /// Stable read-only wire used by dossier, Canvas, and LSP. Projection
    /// never invents a second decision: every row retains report/evidence ids.
    pub fn to_json(&self) -> String {
        let facts = self.facts.iter().map(|fact| format!(
            "{{\"budget_id\":{},\"enforcement\":{},\"evidence\":{},\"evidence_id\":{},\"outcome\":{},\"report_id\":{},\"statistical\":{}}}",
            json(&fact.budget_id), json(&fact.enforcement), json(&fact.evidence),
            json(&fact.evidence_id), json(&fact.outcome), json(&fact.report_id), fact.statistical,
        )).collect::<Vec<_>>().join(",");
        let rejected = self.rejected.iter().map(|reason| json(reason)).collect::<Vec<_>>().join(",");
        format!("{{\"mode\":\"read_only\",\"rejected\":[{rejected}],\"reports\":[{facts}]}}")
    }

    pub fn render_text(&self) -> String {
        let mut out = String::from("Performance budgets (read-only)\n");
        if self.facts.is_empty() { out.push_str("  no compatible canonical report\n"); }
        for fact in &self.facts {
            out.push_str(&format!("  {}  {} ({})  report {}  evidence {}\n", fact.budget_id, fact.outcome, fact.evidence, &fact.report_id[..12], &fact.evidence_id[..12]));
        }
        for reason in &self.rejected { out.push_str(&format!("  rejected: {reason}\n")); }
        out
    }
}

/// Reads canonical reports without creating, refreshing, or rewriting any file.
/// Newest compatible report wins per budget id; identity remains report-owned.
pub fn read_compatible(root: &Path, sources: &[(String, String)]) -> BudgetProjection {
    let dir = root.join(".jet/perf/reports");
    let Ok(entries) = fs::read_dir(&dir) else { return BudgetProjection::default() };
    let wanted = sources.iter().map(|(path, digest)| (normalize(path), digest.as_str())).collect::<BTreeMap<_, _>>();
    let mut paths = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json")).collect::<Vec<_>>();
    paths.sort();
    let mut by_budget = BTreeMap::<String, BudgetFact>::new();
    let mut rejected = Vec::new();
    for path in paths {
        let label = path.file_name().and_then(|name| name.to_str()).unwrap_or("report").to_string();
        let metadata = match fs::symlink_metadata(&path) { Ok(metadata) => metadata, Err(error) => { rejected.push(format!("{label}: unreadable: {error}")); continue; } };
        if metadata.file_type().is_symlink() || !metadata.is_file() { rejected.push(format!("{label}: not a regular no-follow report")); continue; }
        if metadata.len() > 16 * 1024 * 1024 { rejected.push(format!("{label}: report exceeds 16 MiB projection limit")); continue; }
        let bytes = match fs::read(&path) { Ok(bytes) => bytes, Err(error) => { rejected.push(format!("{label}: unreadable: {error}")); continue; } };
        let report = match verify_budget_report(&bytes) { Ok(report) => report, Err(error) => { rejected.push(format!("{label}: {error}")); continue; } };
        let Ok((report_id, evidence_id, members, measurements)) = report_parts(&report) else { rejected.push(format!("{label}: report projection fields are invalid")); continue; };
        if members.is_empty() || !members.iter().all(|(path, digest)| wanted.get(&normalize(path)).is_some_and(|wanted| *wanted == digest)) { continue; }
        for measurement in measurements {
            let Ok(fact) = measurement_fact(&report_id, &evidence_id, measurement) else { rejected.push(format!("{label}: measurement projection fields are invalid")); continue; };
            by_budget.insert(fact.budget_id.clone(), fact);
        }
    }
    BudgetProjection { facts: by_budget.into_values().collect(), rejected }
}

fn report_parts(value: &CanonicalJson) -> Result<(String, String, Vec<(String, String)>, &[CanonicalJson]), ()> {
    let report = object(value)?;
    let report_id = text(report.get("report_id"))?.to_string();
    let content = object(report.get("content").ok_or(())?)?;
    let evidence_id = text(content.get("evidence_id"))?.to_string();
    let subject = object(content.get("subject").ok_or(())?)?;
    let members = array(subject.get("member_sources"))?.iter().map(|value| {
        let member = object(value)?;
        Ok((text(member.get("path"))?.to_string(), text(member.get("sha256"))?.to_string()))
    }).collect::<Result<Vec<_>, ()>>()?;
    Ok((report_id, evidence_id, members, array(content.get("measurements"))?))
}

fn measurement_fact(report_id: &str, evidence_id: &str, value: &CanonicalJson) -> Result<BudgetFact, ()> {
    let measurement = object(value)?;
    let decision = object(measurement.get("decision").ok_or(())?)?;
    let comparison = object(measurement.get("comparison").ok_or(())?)?;
    Ok(BudgetFact {
        report_id: report_id.to_string(),
        evidence_id: evidence_id.to_string(),
        budget_id: text(measurement.get("budget_id"))?.to_string(),
        evidence: text(decision.get("evidence"))?.to_string(),
        outcome: text(decision.get("policy_outcome"))?.to_string(),
        enforcement: text(measurement.get("enforcement"))?.to_string(),
        statistical: text(comparison.get("kind"))? != "absolute",
    })
}

fn object(value: &CanonicalJson) -> Result<&BTreeMap<String, CanonicalJson>, ()> { if let CanonicalJson::Object(value) = value { Ok(value) } else { Err(()) } }
fn array(value: Option<&CanonicalJson>) -> Result<&[CanonicalJson], ()> { if let Some(CanonicalJson::Array(value)) = value { Ok(value) } else { Err(()) } }
fn text(value: Option<&CanonicalJson>) -> Result<&str, ()> { if let Some(CanonicalJson::String(value)) = value { Ok(value) } else { Err(()) } }
fn normalize(path: &str) -> String { path.trim_start_matches("./").replace('\\', "/") }
fn json(value: &str) -> String { format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")) }
