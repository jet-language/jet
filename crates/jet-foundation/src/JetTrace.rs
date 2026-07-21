//! D-PERFSESSION1=D / D-ARTIFACT-EXT1=A: versioned `.jettrace` artifact identity.
//!
//! Reuses the PerformanceBudget canonical-JSON substrate so budget reports and
//! performance traces share one encoding/hash law. Capture payloads grow later;
//! schema identity and verify are the durable seam.

use crate::PerformanceBudget::{stable_id, verify_stable_id, CanonicalJson};
use crate::Syntax::ARTIFACT_EXT_TRACE;
use std::collections::BTreeMap;

pub const TRACE_SCHEMA: &str = "jet.trace";
pub const TRACE_VERSION: &str = "1";
pub const CAPTURE_POLICY_SCHEMA: &str = "1";

/// Default privacy exclusions from D-PERFSESSION1 (sorted for A-canonical bytes).
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    "arguments",
    "credentials",
    "environment",
    "request_bodies",
    "response_bodies",
    "secret_types",
    "sql",
    "urls",
    "values",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceToolchain {
    pub jet_version: String,
    pub compiler_build_id: String,
    pub stdlib_id: String,
    pub runner_id: String,
}

impl TraceToolchain {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        let digest_content = CanonicalJson::object([
            ("compiler_build_id".into(), CanonicalJson::String(self.compiler_build_id.clone())),
            ("jet_version".into(), CanonicalJson::String(self.jet_version.clone())),
            ("runner_id".into(), CanonicalJson::String(self.runner_id.clone())),
            ("stdlib_id".into(), CanonicalJson::String(self.stdlib_id.clone())),
        ])?;
        let digest = stable_id(&digest_content);
        CanonicalJson::object([
            ("compiler_build_id".into(), CanonicalJson::String(self.compiler_build_id.clone())),
            ("digest".into(), CanonicalJson::String(digest)),
            ("jet_version".into(), CanonicalJson::String(self.jet_version.clone())),
            ("runner_id".into(), CanonicalJson::String(self.runner_id.clone())),
            ("stdlib_id".into(), CanonicalJson::String(self.stdlib_id.clone())),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturePolicy {
    pub allowlist: Vec<String>,
}

impl CapturePolicy {
    pub fn default_exclusions() -> Self {
        Self { allowlist: Vec::new() }
    }

    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        let mut allowlist = self.allowlist.clone();
        allowlist.sort();
        allowlist.dedup();
        let exclusions = DEFAULT_EXCLUSIONS
            .iter()
            .map(|item| CanonicalJson::String((*item).into()))
            .collect::<Vec<_>>();
        CanonicalJson::object([
            (
                "allowlist".into(),
                CanonicalJson::Array(allowlist.into_iter().map(CanonicalJson::String).collect()),
            ),
            ("default_exclusions".into(), CanonicalJson::Array(exclusions)),
            ("schema".into(), CanonicalJson::Integer(CAPTURE_POLICY_SCHEMA.into())),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSkeleton {
    pub command: String,
    pub argv: Vec<String>,
    pub toolchain: TraceToolchain,
    pub capture_policy: CapturePolicy,
}

impl TraceSkeleton {
    pub fn content_json(&self) -> Result<CanonicalJson, String> {
        let argv = CanonicalJson::Array(self.argv.iter().cloned().map(CanonicalJson::String).collect());
        CanonicalJson::object([
            ("allocations".into(), CanonicalJson::Array(Vec::new())),
            ("argv".into(), argv),
            ("browser".into(), CanonicalJson::Array(Vec::new())),
            ("capture_policy".into(), self.capture_policy.to_json()?),
            ("command".into(), CanonicalJson::String(self.command.clone())),
            ("io".into(), CanonicalJson::Array(Vec::new())),
            ("locks".into(), CanonicalJson::Array(Vec::new())),
            ("native".into(), CanonicalJson::Array(Vec::new())),
            ("samples".into(), CanonicalJson::Array(Vec::new())),
            ("source_identity".into(), CanonicalJson::Array(Vec::new())),
            ("spans".into(), CanonicalJson::Array(Vec::new())),
            ("tasks".into(), CanonicalJson::Array(Vec::new())),
            ("toolchain".into(), self.toolchain.to_json()?),
        ])
    }
}

pub fn jettrace_artifact(content: CanonicalJson) -> CanonicalJson {
    let trace_id = stable_id(&content);
    CanonicalJson::object([
        ("content".into(), content),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("trace_id".into(), CanonicalJson::String(trace_id)),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("fixed jettrace wrapper keys are unique")
}

pub fn build_skeleton_bytes(skeleton: &TraceSkeleton) -> Result<Vec<u8>, String> {
    let content = skeleton.content_json()?;
    Ok(jettrace_artifact(content).bytes())
}

pub fn verify_jettrace(bytes: &[u8]) -> Result<CanonicalJson, String> {
    let report = CanonicalJson::parse_canonical(bytes)?;
    let fields = match &report {
        CanonicalJson::Object(fields) => fields,
        _ => return Err("jettrace wrapper is not an object".into()),
    };
    let expected = ["content", "schema", "trace_id", "version"];
    if fields.len() != expected.len() || !expected.iter().all(|key| fields.contains_key(*key)) {
        return Err("jettrace wrapper has missing or unknown keys".into());
    }
    if fields.get("schema") != Some(&CanonicalJson::String(TRACE_SCHEMA.into())) {
        return Err(format!("unsupported jettrace schema (need {TRACE_SCHEMA})"));
    }
    match fields.get("version") {
        Some(CanonicalJson::Integer(version)) if version == TRACE_VERSION => {}
        Some(CanonicalJson::Integer(version)) => {
            return Err(format!(
                "jettrace version {version} needs a newer jet toolchain (this reader supports {TRACE_VERSION})"
            ));
        }
        _ => return Err("jettrace version is not an integer".into()),
    }
    let content = fields.get("content").expect("checked content key");
    validate_content(content)?;
    let claimed = match fields.get("trace_id") {
        Some(CanonicalJson::String(id)) => id,
        _ => return Err("jettrace trace_id is not text".into()),
    };
    if claimed.len() != 64 || !claimed.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("jettrace trace_id is not lowercase Hex64".into());
    }
    verify_stable_id(content, claimed)?;
    Ok(report)
}

fn validate_content(value: &CanonicalJson) -> Result<(), String> {
    let fields = object_keys(
        value,
        "content",
        &[
            "allocations",
            "argv",
            "browser",
            "capture_policy",
            "command",
            "io",
            "locks",
            "native",
            "samples",
            "source_identity",
            "spans",
            "tasks",
            "toolchain",
        ],
    )?;
    text(&fields["command"], "content.command")?;
    match &fields["argv"] {
        CanonicalJson::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                text(item, &format!("content.argv[{i}]"))?;
            }
        }
        _ => return Err("content.argv is not an array".into()),
    }
    for key in [
        "allocations",
        "browser",
        "io",
        "locks",
        "native",
        "samples",
        "source_identity",
        "spans",
        "tasks",
    ] {
        match &fields[key] {
            CanonicalJson::Array(_) => {}
            _ => return Err(format!("content.{key} is not an array")),
        }
    }
    validate_capture_policy(&fields["capture_policy"])?;
    validate_toolchain(&fields["toolchain"])?;
    Ok(())
}

fn validate_capture_policy(value: &CanonicalJson) -> Result<(), String> {
    let fields = object_keys(value, "capture_policy", &["allowlist", "default_exclusions", "schema"])?;
    if fields["schema"] != CanonicalJson::Integer(CAPTURE_POLICY_SCHEMA.into()) {
        return Err("unsupported capture_policy schema".into());
    }
    let exclusions = match &fields["default_exclusions"] {
        CanonicalJson::Array(items) => items,
        _ => return Err("capture_policy.default_exclusions is not an array".into()),
    };
    let expected = DEFAULT_EXCLUSIONS
        .iter()
        .map(|item| CanonicalJson::String((*item).into()))
        .collect::<Vec<_>>();
    if exclusions != &expected {
        return Err("capture_policy.default_exclusions does not match D-PERFSESSION1 defaults".into());
    }
    match &fields["allowlist"] {
        CanonicalJson::Array(items) => {
            let mut prior: Option<&str> = None;
            for item in items {
                let text = text(item, "capture_policy.allowlist item")?;
                if prior.is_some_and(|p| p > text) {
                    return Err("capture_policy.allowlist is not sorted".into());
                }
                prior = Some(text);
            }
        }
        _ => return Err("capture_policy.allowlist is not an array".into()),
    }
    Ok(())
}

fn validate_toolchain(value: &CanonicalJson) -> Result<(), String> {
    let fields = object_keys(
        value,
        "toolchain",
        &["compiler_build_id", "digest", "jet_version", "runner_id", "stdlib_id"],
    )?;
    for key in ["compiler_build_id", "jet_version", "runner_id", "stdlib_id"] {
        text(&fields[key], &format!("toolchain.{key}"))?;
    }
    let digest = text(&fields["digest"], "toolchain.digest")?;
    if digest.len() != 64 || !digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("toolchain.digest is not lowercase Hex64".into());
    }
    let digest_content = CanonicalJson::object([
        ("compiler_build_id".into(), fields["compiler_build_id"].clone()),
        ("jet_version".into(), fields["jet_version"].clone()),
        ("runner_id".into(), fields["runner_id"].clone()),
        ("stdlib_id".into(), fields["stdlib_id"].clone()),
    ])?;
    verify_stable_id(&digest_content, digest).map_err(|_| "toolchain digest mismatch".to_string())?;
    Ok(())
}

fn object_keys<'a>(
    value: &'a CanonicalJson,
    label: &str,
    keys: &[&str],
) -> Result<&'a BTreeMap<String, CanonicalJson>, String> {
    let fields = match value {
        CanonicalJson::Object(fields) => fields,
        _ => return Err(format!("{label} is not an object")),
    };
    if fields.len() != keys.len() || !keys.iter().all(|key| fields.contains_key(*key)) {
        return Err(format!("{label} has missing or unknown keys"));
    }
    Ok(fields)
}

fn text<'a>(value: &'a CanonicalJson, label: &str) -> Result<&'a str, String> {
    match value {
        CanonicalJson::String(text) => Ok(text),
        _ => Err(format!("{label} is not text")),
    }
}

pub fn trace_id(value: &CanonicalJson) -> Result<&str, String> {
    let fields = match value {
        CanonicalJson::Object(fields) => fields,
        _ => return Err("jettrace wrapper is not an object".into()),
    };
    text(fields.get("trace_id").ok_or("jettrace missing trace_id")?, "trace_id")
}

pub fn artifact_extension() -> &'static str {
    ARTIFACT_EXT_TRACE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skeleton() -> TraceSkeleton {
        TraceSkeleton {
            command: "run".into(),
            argv: vec!["run".into(), "app.jet".into()],
            toolchain: TraceToolchain {
                jet_version: "0.0.0-test".into(),
                compiler_build_id: "build-test".into(),
                stdlib_id: "stdlib-test".into(),
                runner_id: "runner-test".into(),
            },
            capture_policy: CapturePolicy::default_exclusions(),
        }
    }

    #[test]
    fn skeleton_round_trips_with_schema_identity() {
        let bytes = build_skeleton_bytes(&sample_skeleton()).unwrap();
        let verified = verify_jettrace(&bytes).unwrap();
        assert_eq!(
            match &verified {
                CanonicalJson::Object(fields) => fields.get("schema"),
                _ => None,
            },
            Some(&CanonicalJson::String(TRACE_SCHEMA.into()))
        );
        let id = trace_id(&verified).unwrap();
        assert_eq!(id.len(), 64);
        assert!(artifact_extension().ends_with("jettrace"));
    }

    #[test]
    fn forged_trace_id_is_rejected() {
        let mut bytes = build_skeleton_bytes(&sample_skeleton()).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let forged = text.replacen(
            &stable_id(&sample_skeleton().content_json().unwrap()),
            &"0".repeat(64),
            1,
        );
        bytes = forged.into_bytes();
        assert!(verify_jettrace(&bytes).unwrap_err().contains("content hash mismatch"));
    }

    #[test]
    fn newer_major_version_names_required_toolchain() {
        let content = sample_skeleton().content_json().unwrap();
        let wrapper = CanonicalJson::object([
            ("content".into(), content.clone()),
            ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
            ("trace_id".into(), CanonicalJson::String(stable_id(&content))),
            ("version".into(), CanonicalJson::Integer("2".into())),
        ])
        .unwrap();
        let err = verify_jettrace(&wrapper.bytes()).unwrap_err();
        assert!(err.contains("newer jet toolchain"), "{err}");
    }
}
