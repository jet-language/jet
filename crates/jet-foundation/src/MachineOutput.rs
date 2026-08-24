//! The single reader for Jet's JSON machine-output door.

use crate::JSON::JSONValue;
use crate::Report::REPORT_SCHEMA;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineRecord {
    Report,
    Status,
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
        match object.get("ok") {
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
        let status = render_status_json("ok", true, "facts", ",\"facts\":[]");
        let report = ReportEnvelope::new("compile", "error", "E0001", "what", "why", "fix")
            .json();
        assert_eq!(read_machine_line(&status), Ok(MachineRecord::Status));
        assert_eq!(read_machine_line(&report), Ok(MachineRecord::Report));
        assert_eq!(read_machine_output(&format!("{status}\n{report}\n")).unwrap().len(), 2);
    }

    #[test]
    fn reader_rejects_parallel_schema_versions() {
        let error = read_machine_line(r#"{"schema_version":1,"facts":[]}"#).unwrap_err();
        assert!(error.contains("missing `schema`"), "{error}");
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
        ];
        let output = surfaces
            .iter()
            .map(|(action, fields)| render_status_json("ok", true, action, fields))
            .collect::<Vec<_>>()
            .join("\n");
        let records = read_machine_output(&output).unwrap();
        assert_eq!(records, vec![MachineRecord::Status; surfaces.len()]);
    }
}
