//! The single reader for Jet's JSON machine-output door.

use crate::DataTree::DataTree;
use crate::Report::{NoFixReasonKind, REPORT_SCHEMA, STATUS_SCHEMA};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineRecord {
    Report,
    Status,
    BrowserRelay,
}

pub fn read_machine_line(text: &str) -> Result<MachineRecord, String> {
    let value = crate::JSON::parse(text.trim())?;
    let object = value.as_object()?;
    let schema = field(object, "schema")
        .ok_or_else(|| "machine record is missing `schema`".to_owned())?
        .as_str()?;
    match schema {
        REPORT_SCHEMA => {
            validate_report(object)?;
            Ok(MachineRecord::Report)
        }
        STATUS_SCHEMA => {
            validate_status(object)?;
            let action = field(object, "action")
                .and_then(|value| value.as_str().ok())
                .unwrap_or_default();
            if action == "browser.relay" {
                Ok(MachineRecord::BrowserRelay)
            } else {
                Ok(MachineRecord::Status)
            }
        }
        _ => Err(format!(
            "machine record uses `{schema}`; expected `{REPORT_SCHEMA}` or `{STATUS_SCHEMA}`"
        )),
    }
}

fn field<'a>(object: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}
fn validate_report(object: &[(String, DataTree)]) -> Result<(), String> {
    for key in ["moment", "severity", "code", "what", "why", "fix"] {
        field(object, key)
            .ok_or_else(|| format!("report record is missing `{key}`"))?
            .as_str()?;
    }
    let has_edits = field(object, "fix_edits").is_some();
    let has_reason = field(object, "no_fix_reason").is_some();
    if has_edits && has_reason {
        return Err("report record cannot contain both `fix_edits` and `no_fix_reason`".to_owned());
    }
    if let Some(edits) = field(object, "fix_edits") {
        let edits = edits.as_array()?;
        for edit in edits {
            let edit = edit.as_object()?;
            field(edit, "file")
                .ok_or_else(|| "report edit is missing `file`".to_owned())?
                .as_str()?;
            let span = field(edit, "span")
                .ok_or_else(|| "report edit is missing `span`".to_owned())?
                .as_object()?;
            for key in ["start", "end"] {
                field(span, key)
                    .ok_or_else(|| format!("report edit span is missing `{key}"))?;
            }
            field(edit, "new_text")
                .ok_or_else(|| "report edit is missing `new_text`".to_owned())?
                .as_str()?;
            field(edit, "safety")
                .ok_or_else(|| "report edit is missing `safety`".to_owned())?
                .as_str()?;
        }
    }
    if let Some(reason) = field(object, "no_fix_reason") {
        let reason = reason.as_object()?;
        let kind = field(reason, "kind")
            .ok_or_else(|| "no-fix reason is missing `kind`".to_owned())?
            .as_str()?;
        NoFixReasonKind::parse(kind)?;
        let next = field(reason, "next")
            .ok_or_else(|| "no-fix reason is missing `next`".to_owned())?
            .as_str()?;
        if next.trim().is_empty() || next.chars().any(char::is_control) {
            return Err("no-fix reason `next` must be a reviewed printable action".to_owned());
        }
    }
    Ok(())
}

fn validate_status(object: &[(String, DataTree)]) -> Result<(), String> {
    let action = field(object, "action")
        .ok_or_else(|| "status record is missing `action`".to_owned())?
        .as_str()?;
    if action.is_empty() || action.chars().any(char::is_control) {
        return Err("status record `action` must be a non-empty printable string".to_owned());
    }
    if field(object, "status").is_some() {
        return Err("status record must not contain the legacy `status` key".to_owned());
    }
    match field(object, "ok") {
        Some(DataTree::Bool(_)) => {}
        Some(_) => return Err("status record field `ok` is not boolean".to_owned()),
        None => return Err("status record is missing `ok`".to_owned()),
    }
    let reports = field(object, "reports")
        .ok_or_else(|| "status record is missing `reports`".to_owned())?
        .as_array()?;
    for report in reports {
        let report = report.as_object()?;
        let schema = field(report, "schema")
            .ok_or_else(|| "status report is missing `schema`".to_owned())?
            .as_str()?;
        if schema != REPORT_SCHEMA {
            return Err(format!(
                "status report uses `{schema}`; expected `{REPORT_SCHEMA}`"
            ));
        }
        validate_report(report)?;
    }
    Ok(())
}

pub fn read_machine_output(text: &str) -> Result<Vec<MachineRecord>, String> {
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = read_machine_line(line)
            .map_err(|error| format!("machine record {}: {error}", index + 1))?;
        records.push(record);
    }
    if records.is_empty() {
        return Err("machine output contains no records".to_owned());
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::{read_machine_line, read_machine_output, MachineRecord};
    use crate::Diagnostics::Severity;
    use crate::Registry::diagnostic_rows;
    use crate::Report::{ReportEnvelope, StatusEnvelope, StatusValue};

    #[test]
    fn reader_accepts_status_and_report_json_lines() {
        let status = StatusEnvelope::new("facts", true)
            .with_field("facts", StatusValue::array(Vec::new()))
            .json();
        let report = ReportEnvelope::new("compile", "error", "E0001", "what", "why", "fix").json();
        assert_eq!(read_machine_line(&status), Ok(MachineRecord::Status));
        assert_eq!(read_machine_line(&report), Ok(MachineRecord::Report));
        assert_eq!(
            read_machine_output(&format!("{status}\n{report}\n"))
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn reader_rejects_parallel_schema_versions() {
        let error = read_machine_line(r#"{"schema_version":1,"facts":[]}"#).unwrap_err();
        assert!(error.contains("missing `schema`"), "{error}");
    }

    #[test]
    fn reader_rejects_legacy_report_schema() {
        let error = read_machine_line(
            r#"{"schema":"jet.report/v1","moment":"tool","status":"ok","ok":true}"#,
        )
        .unwrap_err();
        assert!(error.contains("expected `jet.report/v2`"), "{error}");
    }

    #[test]
    fn reader_accepts_the_shared_browser_relay_envelope() {
        let relay = StatusEnvelope::new("browser.relay", true)
            .with_field("nonce", "nonce")
            .with_field("pid", 42usize)
            .with_field("started", "1")
            .with_field("sources", "00")
            .json();
        let row = StatusEnvelope::new("browser.relay.row", true)
            .with_field("start_ns", 10usize)
            .with_field("duration_ns", 5usize)
            .with_field("class", "event")
            .with_field("symbol", "load")
            .json();
        let truncated = StatusEnvelope::new("browser.relay.truncated", true).json();
        assert_eq!(
            read_machine_output(&format!("{relay}\n{row}\n{truncated}\n")),
            Ok(vec![
                MachineRecord::BrowserRelay,
                MachineRecord::Status,
                MachineRecord::Status,
            ])
        );
    }

    #[test]
    fn every_registered_machine_surface_uses_one_reader() {
        let surfaces = [
            ("diagnostics", ",\"diagnostics\":[]"),
            ("semindex", ",\"semindex\":{}"),
            ("compiler", ",\"compiler\":{}"),
            ("canvas", ",\"canvas\":{}"),
            ("structural_merge", ",\"structural_merge\":{}"),
            ("budget", ",\"budget\":{}"),
            ("perf", ",\"perf\":{}"),
            ("gc", ",\"gc\":{}"),
            ("browser.relay", ",\"browser\":{\"relay\":{}}"),
            ("status", ",\"status_report\":{}"),
            ("review", ",\"review\":{}"),
            ("fill", ",\"fill\":{}"),
            ("import", ",\"import\":{}"),
            ("inspect.dossier", ",\"dossier\":{}"),
            ("inspect.impact", ",\"impact\":{}"),
            ("inspect.live", ",\"live\":{}"),
            ("fmt", ",\"fmt\":{}"),
            ("eval", ",\"value\":null"),
            ("coverage", ",\"coverage\":{}"),
            ("test", ",\"test\":{}"),
            ("inspect.build", ",\"build\":{}"),
            ("inspect.parts", ",\"parts\":[]"),
            ("inspect.gates", ",\"gates\":{}"),
            ("inspect.unsafe", ",\"gates\":[]"),
            ("find", ",\"matches\":[]"),
            ("audit.copies", ",\"copies\":[]"),
            ("try", ",\"name\":\"example\""),
            ("remote.bind", ",\"builder\":\"local\""),
            ("remote.list", ",\"builders\":[]"),
        ];
        let output = surfaces
            .iter()
            .map(|(action, fields)| {
                let fields = StatusValue::parse(&format!("{{{}}}", fields.trim_start_matches(',')))
                    .unwrap();
                StatusEnvelope::new(*action, true)
                    .with_field("payload", fields)
                    .json()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let records = read_machine_output(&output).unwrap();
        let expected = surfaces
            .iter()
            .map(|(action, _)| {
                if *action == "browser.relay" {
                    MachineRecord::BrowserRelay
                } else {
                    MachineRecord::Status
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(records, expected);
    }

    #[test]
    fn reader_accepts_every_registered_report_row() {
        let output = diagnostic_rows()
            .iter()
            .map(|row| {
                let severity = match row.severity {
                    Severity::Error => "error",
                    Severity::Lint => "warning",
                };
                ReportEnvelope::new(
                    row.moment.as_str(),
                    severity,
                    row.code,
                    row.what,
                    row.why,
                    row.fix,
                )
                .json()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let records = read_machine_output(&output).unwrap();
        assert_eq!(records.len(), diagnostic_rows().len());
        assert!(records
            .iter()
            .all(|record| *record == MachineRecord::Report));
    }
}
