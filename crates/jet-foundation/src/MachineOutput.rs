//! The single reader for Jet's JSON machine-output door.

use crate::Report::REPORT_SCHEMA;
use crate::JSON::JSONValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineRecord {
    Report,
    Status,
    BrowserRelay,
}

pub fn read_machine_line(text: &str) -> Result<MachineRecord, String> {
    let value = crate::JSON::parse(text.trim())?;
    let object = value.as_object()?;
    let schema = object
        .get("schema")
        .ok_or_else(|| "machine record is missing `schema`".to_owned())?
        .as_str()?;
    if schema != REPORT_SCHEMA {
        return Err(format!(
            "machine record uses `{schema}`; expected `{REPORT_SCHEMA}`"
        ));
    }
    object
        .get("moment")
        .ok_or_else(|| "machine record is missing `moment`".to_owned())?
        .as_str()?;
    if object.contains_key("severity") {
        for key in ["code", "what", "why", "fix"] {
            object
                .get(key)
                .ok_or_else(|| format!("report record is missing `{key}`"))?
                .as_str()?;
        }
        Ok(MachineRecord::Report)
    } else {
        object
            .get("status")
            .ok_or_else(|| "status record is missing `status`".to_owned())?
            .as_str()?;
        let action = object
            .get("action")
            .ok_or_else(|| "status record is missing `action`".to_owned())?
            .as_str()?;
        match object.get("ok") {
            Some(JSONValue::Bool(_)) if action == "browser.relay" => {
                Ok(MachineRecord::BrowserRelay)
            }
            Some(JSONValue::Bool(_)) => Ok(MachineRecord::Status),
            Some(_) => Err("status record field `ok` is not boolean".to_owned()),
            None => Err("status record is missing `ok`".to_owned()),
        }
    }
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
    use crate::Report::{render_status_json, ReportEnvelope};

    #[test]
    fn reader_accepts_status_and_report_json_lines() {
        let status = ReportEnvelope::status_record("tool", "ok", true, "facts")
            .with_fields(",\"facts\":[]")
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
    fn reader_rejects_status_records_without_the_shared_envelope_fields() {
        let error = read_machine_line(
            r#"{"schema":"jet.report/v1","moment":"tool","status":"ok","ok":true}"#,
        )
        .unwrap_err();
        assert!(error.contains("missing `action`"), "{error}");
    }

    #[test]
    fn reader_accepts_the_shared_browser_relay_envelope() {
        let relay = render_status_json(
            "started",
            true,
            "browser.relay",
            ",\"nonce\":\"nonce\",\"pid\":42,\"started\":\"1\",\"sources\":\"00\"",
        );
        let row = render_status_json(
            "row",
            true,
            "browser.relay.row",
            ",\"start_ns\":10,\"duration_ns\":5,\"class\":\"event\",\"symbol\":\"load\"",
        );
        let truncated = render_status_json("truncated", true, "browser.relay.truncated", "");
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
            .map(|(action, fields)| render_status_json("ok", true, action, fields))
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
}
