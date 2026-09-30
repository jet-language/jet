//! D-BUILD-NOCHANGE1=A: persisted Receipt records.
//!
//! A Receipt is keyed on the invocation closure digest: the verb, the options
//! that change what the verb checks, the compiler identity, and the digest of
//! every leaf the check read. Its payload is typed: diagnostics keep their
//! code, severity, spans, origins, fixes, and causes so each invocation renders
//! them for its own terminal mode (color, width, `--json`) instead of replaying
//! bytes captured from an earlier terminal.
//!
//! The record uses the one compiler record codec (`RecordCodec`) and lives in
//! the machine store as an immutable blob. A small mutable pointer per closure
//! key, kept in the workspace-root `.jet/receipts/`, names the newest record,
//! because inputs discovered while checking (compile-time reads) are verified
//! at use time rather than known before the lookup.

use super::diagnostic_record::{decode_diagnostics, encode_diagnostics};
use super::{Digest, Store, StoreError};
use jet_foundation::Diagnostics::Diagnostic;
use jet_foundation::RecordCodec::{
    encode_record, record_map, RecordReader, RecordSection, RecordValue,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const SECTION_HEADER: u64 = 1;
const SECTION_MODULES: u64 = 2;
const SECTION_INPUTS: u64 = 3;
const SECTION_ORIGINS: u64 = 4;
const SECTION_DIAGNOSTICS: u64 = 5;
const SECTION_FACTS: u64 = 6;

/// One leaf read by a recorded check: a project-relative path (or a stable
/// label such as `<corelib>/…`) and the SHA-256 of its exact bytes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReceiptInput {
    pub path: String,
    pub digest: String,
}

/// The typed result of one recorded invocation.
#[derive(Clone, Debug)]
pub struct ReceiptRecord {
    pub schema: String,
    pub verb: String,
    /// The closure digest the record answers.
    pub closure: String,
    /// Module sources in the closure. Each is one `Check` node.
    pub modules: Vec<ReceiptInput>,
    /// Inputs discovered while checking (for example compile-time file reads).
    /// A hit requires every one to still hash to the recorded digest.
    pub inputs: Vec<ReceiptInput>,
    pub diagnostics: Vec<Diagnostic>,
    /// Verb-owned typed projections of the checked program, by name.
    pub facts: BTreeMap<String, String>,
}

impl ReceiptRecord {
    pub const SCHEMA: &'static str = "jet.receipt/v1";

    pub fn new(
        verb: impl Into<String>,
        closure: impl Into<String>,
        mut modules: Vec<ReceiptInput>,
        mut inputs: Vec<ReceiptInput>,
        diagnostics: Vec<Diagnostic>,
        facts: BTreeMap<String, String>,
    ) -> Self {
        modules.sort();
        modules.dedup();
        inputs.sort();
        inputs.dedup();
        Self {
            schema: Self::SCHEMA.to_string(),
            verb: verb.into(),
            closure: closure.into(),
            modules,
            inputs,
            diagnostics,
            facts,
        }
    }

    /// Encode the record. Diagnostic origins whose source is one of the
    /// recorded modules are stored by revision only; the reader supplies the
    /// bytes from the same closure. Any other origin source is embedded.
    ///
    /// Returns an error for a diagnostic payload the codec does not carry; the
    /// caller then leaves the invocation unrecorded so the next one checks
    /// fresh.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let revisions = self
            .modules
            .iter()
            .map(|module| module.digest.as_str())
            .collect::<BTreeSet<_>>();
        let diagnostics = encode_diagnostics(&self.diagnostics, &revisions)?;
        let sections = [
            RecordSection::new(
                SECTION_HEADER,
                vec![record_map([
                    ("verb", RecordValue::str(self.verb.as_str())),
                    ("closure", RecordValue::str(self.closure.as_str())),
                ])],
            ),
            RecordSection::new(SECTION_MODULES, encode_inputs(&self.modules)),
            RecordSection::new(SECTION_INPUTS, encode_inputs(&self.inputs)),
            RecordSection::new(SECTION_ORIGINS, diagnostics.origins),
            RecordSection::new(SECTION_DIAGNOSTICS, diagnostics.diagnostics),
            RecordSection::new(
                SECTION_FACTS,
                self.facts
                    .iter()
                    .map(|(name, value)| {
                        record_map([
                            ("name", RecordValue::str(name.as_str())),
                            ("value", RecordValue::str(value.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ];
        encode_record(Self::SCHEMA, &sections).map_err(|error| error.to_string())
    }

    /// Decode a record. `source_for_revision` returns the exact source text
    /// for a module revision in the caller's current closure.
    pub fn from_bytes(
        bytes: &[u8],
        source_for_revision: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let error = |error: jet_foundation::RecordCodec::RecordError| error.to_string();
        let reader = RecordReader::open_schema(bytes, Self::SCHEMA).map_err(error)?;
        let header = reader.element(SECTION_HEADER, 0).map_err(error)?;
        let verb = header.field("verb").and_then(|v| v.as_str().map(str::to_string)).map_err(error)?;
        let closure = header
            .field("closure")
            .and_then(|v| v.as_str().map(str::to_string))
            .map_err(error)?;
        let modules = decode_inputs(&reader.elements(SECTION_MODULES).map_err(error)?)?;
        let inputs = decode_inputs(&reader.elements(SECTION_INPUTS).map_err(error)?)?;
        let diagnostics = decode_diagnostics(
            &reader.elements(SECTION_ORIGINS).map_err(error)?,
            &reader.elements(SECTION_DIAGNOSTICS).map_err(error)?,
            source_for_revision,
        )?;
        let mut facts = BTreeMap::new();
        for fact in reader.elements(SECTION_FACTS).map_err(error)? {
            let name = fact.field("name").and_then(|v| v.as_str().map(str::to_string)).map_err(error)?;
            let value = fact.field("value").and_then(|v| v.as_str().map(str::to_string)).map_err(error)?;
            facts.insert(name, value);
        }
        Ok(Self {
            schema: Self::SCHEMA.to_string(),
            verb,
            closure,
            modules,
            inputs,
            diagnostics,
            facts,
        })
    }
}

fn encode_inputs(inputs: &[ReceiptInput]) -> Vec<RecordValue> {
    inputs
        .iter()
        .map(|input| {
            record_map([
                ("path", RecordValue::str(input.path.as_str())),
                ("digest", RecordValue::str(input.digest.as_str())),
            ])
        })
        .collect()
}

fn decode_inputs(values: &[RecordValue]) -> Result<Vec<ReceiptInput>, String> {
    values
        .iter()
        .map(|value| {
            let text = |name: &str| {
                value
                    .field(name)
                    .and_then(|field| field.as_str().map(str::to_string))
                    .map_err(|error| error.to_string())
            };
            Ok(ReceiptInput {
                path: text("path")?,
                digest: text("digest")?,
            })
        })
        .collect()
}

impl Store {
    /// Publish the typed record for `closure` and point the closure at it.
    /// The record blob is immutable; only the per-closure pointer moves.
    pub fn publish_receipt(
        &self,
        workspace_root: &Path,
        record: &ReceiptRecord,
    ) -> Result<(), StoreError> {
        let payload = record
            .to_bytes()
            .map_err(|reason| StoreError::Config(format!("receipt is not recordable: {reason}")))?;
        let object = self.publish_blob(&payload)?;
        let pointer = receipt_pointer_path(workspace_root, &record.closure);
        super::project_state::write_atomic(&pointer, object.to_string().as_bytes())?;
        Ok(())
    }

    /// Load the newest record for `closure`. A missing pointer or blob is a
    /// miss; a record that fails to decode is removed from the pointer so the
    /// next invocation checks fresh and republishes.
    pub fn receipt(
        &self,
        workspace_root: &Path,
        closure: &str,
        source_for_revision: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Option<ReceiptRecord>, StoreError> {
        let pointer = receipt_pointer_path(workspace_root, closure);
        let object = match self.read_build_pointer(&pointer) {
            Ok(Some(object)) => object,
            Ok(None) => return Ok(None),
            Err(_) => {
                let _ = std::fs::remove_file(&pointer);
                return Ok(None);
            }
        };
        let Some(bytes) = self.get_blob(&object)? else {
            return Ok(None);
        };
        match ReceiptRecord::from_bytes(&bytes, source_for_revision) {
            Ok(record) if record.closure == closure => Ok(Some(record)),
            _ => {
                let _ = std::fs::remove_file(&pointer);
                Ok(None)
            }
        }
    }

}

fn receipt_pointer_path(workspace_root: &Path, closure: &str) -> PathBuf {
    super::project_state::state_dir(workspace_root)
        .join("receipts")
        .join(Digest::hash(closure.as_bytes()).to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::Diagnostics::{
        CryptoMisuseReason, DiagnosticCause, DiagnosticLabel, DiagnosticOrigin, FixApplicability,
        FixSafety, ReportMoment, Severity, Span, StructuredDiagnostic, TextEdit,
    };
    use std::sync::Arc;

    fn temp_store(label: &str) -> Store {
        let root = std::env::temp_dir().join(format!(
            "jet-store-receipt-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        Store::new(root).unwrap()
    }

    fn rich_diagnostic(origin: Arc<DiagnosticOrigin>, core: Arc<DiagnosticOrigin>) -> Diagnostic {
        Diagnostic {
            moment: ReportMoment::Compile,
            severity: Severity::Lint,
            code: "L2510".to_string(),
            what: "this implicit clone repeats \"inside\" a loop\n".to_string(),
            why: "why".to_string(),
            fix: "fix".to_string(),
            span: Some(Span::new(3, 9)),
            origin: Some(origin.clone()),
            labels: vec![DiagnosticLabel {
                span: Span::new(1, 2),
                message: "first use".to_string(),
            }],
            cause: vec![DiagnosticCause {
                code: "E0301".to_string(),
                span: None,
                origin: Some(core),
            }],
            edit: Some(TextEdit {
                span: Span::new(3, 9),
                new_text: "x.clone()".to_string(),
            }),
            applicability: Some(FixApplicability::Suggested),
            safety: Some(FixSafety::BehaviorPreserving),
            no_fix_reason: None,
            detail: Some("detail\twith tab".to_string()),
            structured: Some(StructuredDiagnostic::CryptoMisuse {
                reason: CryptoMisuseReason::NonceLength,
                operation: "seal".to_string(),
                expected: Some("12".to_string()),
                actual: Some(i128::MIN),
            }),
            decision_row: None,
            denial_kind: Some("effect".to_string()),
            call_chain: vec!["run".to_string(), "helper".to_string()],
            scope_chain: Vec::new(),
            nearest_granting_scope: None,
        }
    }

    /// I4: a replayed diagnostic renders exactly like the fresh one, so the
    /// codec must round-trip every typed field, including origins resolved
    /// from the caller's closure and origins embedded in the record.
    #[test]
    fn receipt_round_trips_typed_diagnostics_through_the_store() {
        let store = temp_store("round-trip");
        let module_source = "fn run() {\n    x := 1\n}\n".to_string();
        let origin = Arc::new(DiagnosticOrigin::new("main.jet", "/p/main.jet", module_source.clone()));
        let core = Arc::new(DiagnosticOrigin::new("math", "<corelib>/math", "pub fn one() {}\n"));
        let module = ReceiptInput {
            path: "main.jet".to_string(),
            digest: origin.revision.clone(),
        };
        let record = ReceiptRecord::new(
            "check",
            "a".repeat(64),
            vec![module],
            Vec::new(),
            vec![rich_diagnostic(origin.clone(), core.clone())],
            BTreeMap::from([("goal_report".to_string(), "goal: main.jet:2\n".to_string())]),
        );
        let workspace = std::env::temp_dir().join(format!(
            "jet-store-receipt-workspace-{}",
            std::process::id()
        ));
        store.publish_receipt(&workspace, &record).unwrap();

        let resolve = |revision: &str| (revision == origin.revision).then(|| module_source.clone());
        let loaded = store.receipt(&workspace, &record.closure, &resolve).unwrap().unwrap();
        assert_eq!(loaded.to_bytes().unwrap(), record.to_bytes().unwrap());
        assert_eq!(loaded.diagnostics[0].origin.as_deref(), Some(&*origin));
        assert_eq!(loaded.diagnostics[0].cause[0].origin.as_deref(), Some(&*core));
        // The payload is a codec record; the module source is stored by
        // revision only, while the Core origin is embedded.
        let payload = record.to_bytes().unwrap();
        let json = jet_foundation::RecordCodec::record_json(&payload).unwrap();
        assert!(json.starts_with("{\"schema\":\"jet.receipt/v1\""));
        assert!(!json.contains("x := 1"));
        assert!(json.contains("pub fn one() {}"));

        // A closure whose module bytes changed cannot resolve the stored
        // origin: that is a miss, never a replay against different bytes.
        let unresolved = |_: &str| None;
        assert!(store.receipt(&workspace, &record.closure, &unresolved).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&workspace);
    }
}
