//! Import the one-shot local-Nix compatibility document into Jetpack facts.
//!
//! The fallback producer may use Nix to evaluate a locked request, but this
//! seam keeps the result after that process exits. The complete document and
//! each required projection are canonicalized before they enter the producer
//! record, so later Jetpack phases do not need the Nix process or its output
//! formatting.

use crate::JSON::{self, JSONValue};
use crate::SHA256;
use std::collections::BTreeMap;
use std::path::Path;

#[path = "NixFallbackPolicy.rs"]
mod policy;

#[path = "NixIdentity.rs"]
mod identity;
// Re-exported for the import-on-miss consumer on card #2162; the fallback
// entry point that uses it has not shipped yet.
#[allow(unused_imports)]
pub(crate) use self::identity::NixFallbackIdentity;

const IMPORT_SCHEMA: &str = "jetpack.nix-fallback.v1";

const GRAPH_FIELDS: &[&str] = &["closedGraph", "derivationGraph", "graph", "closure"];
const OUTPUT_FIELDS: &[&str] = &["selectedOutputs", "outputs"];
const DEPENDENCY_FIELDS: &[&str] = &["dependencies", "inputDrvs", "inputs"];
const SOURCE_FIELDS: &[&str] = &["sources", "sourceInputs", "inputSrcs"];
const HASH_FIELDS: &[&str] = &["hashes", "outputHashes", "digests"];
const LOSS_FIELDS: &[&str] = &["losses", "lossReport", "unsupported"];
const PROOF_FIELDS: &[&str] = &["proof", "evaluationProof", "attestation"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportedNixState {
    facts: BTreeMap<String, String>,
}

impl ImportedNixState {
    pub(crate) fn facts(&self) -> &BTreeMap<String, String> {
        &self.facts
    }
}

/// Import a rich fallback record. `None` means the input is the old pinned
/// `nix build --json` shape and has no fallback projections to retain.
pub(crate) fn import_record(record: &JSONValue) -> Result<Option<ImportedNixState>, String> {
    let object = record.as_object()?;
    let import = object
        .get("jetpackImport")
        .or_else(|| object.get("jetpack"))
        .unwrap_or(record);
    let import = import.as_object()?;
    if !looks_like_fallback(object, import) {
        return Ok(None);
    }

    let graph = required(import, GRAPH_FIELDS, "closed derivation graph")?;
    let outputs = required(import, OUTPUT_FIELDS, "selected outputs")?;
    let dependencies = required(import, DEPENDENCY_FIELDS, "dependencies")?;
    let sources = required(import, SOURCE_FIELDS, "sources")?;
    let hashes = required(import, HASH_FIELDS, "hashes")?;
    let losses = required(import, LOSS_FIELDS, "losses")?;
    let proof = required(import, PROOF_FIELDS, "proof")?;

    let document = canonical(record)?;
    let projections = [
        ("nix.fallback.graph", graph),
        ("nix.fallback.selected_outputs", outputs),
        ("nix.fallback.outputs", outputs),
        ("nix.fallback.dependencies", dependencies),
        ("nix.fallback.sources", sources),
        ("nix.fallback.hashes", hashes),
        ("nix.fallback.losses", losses),
        ("nix.fallback.proof", proof),
    ];
    let mut facts = BTreeMap::from([
        ("nix.fallback.schema".to_string(), IMPORT_SCHEMA.to_string()),
        ("nix.fallback.document".to_string(), document.clone()),
        (
            "nix.fallback.document.sha256".to_string(),
            SHA256::sha256_hex(document.as_bytes()),
        ),
        (
            "nix.fallback.proof.sha256".to_string(),
            SHA256::sha256_hex(canonical(proof)?.as_bytes()),
        ),
    ]);
    for (key, value) in projections {
        facts.insert(key.to_string(), canonical(value)?);
    }
    for (fact_name, source_name) in [("recipe", "recipe"), ("lock", "lock")] {
        if let Some(value) = import.get(source_name) {
            facts.insert(
                format!("nix.fallback.{fact_name}"),
                canonical(value)?,
            );
        }
    }
    Ok(Some(ImportedNixState { facts }))
}

fn looks_like_fallback(
    root: &BTreeMap<String, JSONValue>,
    import: &BTreeMap<String, JSONValue>,
) -> bool {
    root.contains_key("jetpackImport")
        || root.contains_key("jetpack")
        || root.contains_key("fallback")
        || GRAPH_FIELDS
            .iter()
            .chain(DEPENDENCY_FIELDS)
            .chain(SOURCE_FIELDS)
            .chain(HASH_FIELDS)
            .chain(LOSS_FIELDS)
            .chain(PROOF_FIELDS)
            .any(|key| import.contains_key(*key))
}

fn required<'a>(
    object: &'a BTreeMap<String, JSONValue>,
    names: &[&str],
    label: &str,
) -> Result<&'a JSONValue, String> {
    let mut found = None;
    for name in names {
        let Some(value) = object.get(*name) else {
            continue;
        };
        if found.is_some() {
            return Err(format!(
                "fallback document has more than one {label} field"
            ));
        }
        found = Some(value);
    }
    found.ok_or_else(|| format!("fallback document has no {label} field"))
}

fn canonical(value: &JSONValue) -> Result<String, String> {
    let mut output = String::new();
    write_canonical(value, &mut output)?;
    Ok(output)
}

fn write_canonical(value: &JSONValue, output: &mut String) -> Result<(), String> {
    match value {
        JSONValue::Null => output.push_str("null"),
        JSONValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        JSONValue::Number(value) => output.push_str(&value.to_string()),
        JSONValue::Flt(value) if value.is_finite() => output.push_str(&value.to_string()),
        JSONValue::Flt(_) => return Err("fallback document contains a non-finite number".into()),
        JSONValue::String(value) => output.push_str(&JSON::quote(value)),
        JSONValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_canonical(value, output)?;
            }
            output.push(']');
        }
        JSONValue::Object(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push_str(&JSON::quote(key));
                output.push(':');
                write_canonical(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

pub(crate) use policy::FallbackRun;

pub(crate) fn is_allowed(offline: bool) -> bool {
    policy::allowed_from_environment(offline)
}

pub(crate) fn evaluate(
    project: &Path,
    source_name: &str,
    key: &crate::NixIndex::IndexKey,
) -> Result<FallbackRun, String> {
    policy::run(
        project,
        source_name,
        &key.revision,
        &key.system,
        &key.attrpath,
        false,
    )
}
