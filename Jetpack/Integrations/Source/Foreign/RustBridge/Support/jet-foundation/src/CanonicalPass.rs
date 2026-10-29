//! Opt-in compiler-owned pass-boundary evidence used by the proof observer.
//!
//! The journal is deliberately inert unless the existing adapter evidence
//! environment is active. Passes append their real input/output snapshots;
//! `jet inspect decisions --json` projects the records without reconstructing
//! a route from a digest or a decision row.

use crate::AST::{Item, ProgramBundle};
use crate::JSON::json_escape;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;

pub const SCHEMA: &str = "jet.canonical-pass-record.v1";
pub const PROTOCOL: &str = "jet.canonical-pass.v1";
pub const BASE_PREMISES: &[&str] = &[
    "source_origin_is_retained",
    "typed_failure_is_retained",
    "mutation_is_retained",
    "input_consumption_is_exact",
    "ordered_effects_are_retained",
    "cleanup_edges_are_retained",
    "resource_premises_are_explicit",
    "target_applicability_is_checked",
];

#[derive(Clone, Debug)]
pub struct Record {
    pub stage: String,
    pub operation_id: String,
    pub occurrence: usize,
    pub order: usize,
    pub source: String,
    pub input_representation: String,
    pub input_payload: String,
    pub input_identity: String,
    pub output_representation: String,
    pub output_payload: String,
    pub output_identity: String,
    pub premises: Vec<String>,
    pub disposition: String,
}

thread_local! {
    static RECORDS: RefCell<Vec<Record>> = const { RefCell::new(Vec::new()) };
}

/// Capture is tied to the already-existing adapter evidence environment. It
/// is not a public compiler switch and remains disabled for ordinary builds.
pub fn enabled() -> bool {
    std::env::var_os("JET_ADAPTER_MODE").is_some_and(|mode| !mode.is_empty())
        && std::env::var_os("JET_ADAPTER_OPERATION_IDS").is_some_and(|ids| !ids.is_empty())
}

pub fn clear() {
    RECORDS.with(|records| records.borrow_mut().clear());
}

pub fn record(
    stage: &str,
    operation_id: &str,
    source: &str,
    input_representation: &str,
    input_payload: String,
    input_identity: String,
    output_representation: &str,
    output_payload: String,
    output_identity: String,
    disposition: &str,
) {
    if !enabled()
        || !std::env::var("JET_ADAPTER_OPERATION_IDS")
            .ok()
            .is_some_and(|ids| ids.split(',').any(|candidate| candidate == operation_id))
    {
        return;
    }
    RECORDS.with(|records| {
        let mut records = records.borrow_mut();
        let occurrence = records
            .iter()
            .filter(|record| record.operation_id == operation_id)
            .count()
            + 1;
        let order = records.len() + 1;
        records.push(Record {
            stage: stage.to_string(),
            operation_id: operation_id.to_string(),
            occurrence,
            order,
            source: source.to_string(),
            input_representation: input_representation.to_string(),
            input_payload,
            input_identity,
            output_representation: output_representation.to_string(),
            output_payload,
            output_identity,
            premises: BASE_PREMISES.iter().map(|value| (*value).to_string()).collect(),
            disposition: disposition.to_string(),
        });
    });
}

pub fn take() -> Vec<Record> {
    RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()))
}


fn record_json(record: &Record) -> String {
    let premises = record
        .premises
        .iter()
        .map(|premise| format!("\"{}\"", json_escape(premise)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"{}\",\"protocol\":\"{}\",\"stage\":\"{}\",\"operation_id\":\"{}\",\"occurrence\":{},\"order\":{},\"source\":{{\"path\":\"{}\"}},\"input\":{{\"representation\":\"{}\",\"canonical_payload\":{},\"identity\":{}}},\"output\":{{\"representation\":\"{}\",\"canonical_payload\":{},\"identity\":{}}},\"premises\":[{}],\"disposition\":\"{}\"}}",
        SCHEMA,
        PROTOCOL,
        json_escape(&record.stage),
        json_escape(&record.operation_id),
        record.occurrence,
        record.order,
        json_escape(&record.source),
        json_escape(&record.input_representation),
        record.input_payload,
        record.input_identity,
        json_escape(&record.output_representation),
        record.output_payload,
        record.output_identity,
        premises,
        json_escape(&record.disposition),
    )
}

/// Persist one compiler-process journal for evidence commands that do not
/// project `CanonicalPass::take()` through inspect JSON. The file is append
/// only and deliberately best-effort: evidence capture must never alter
/// compiler semantics when its scratch path is unavailable.
pub fn persist_process(process: &str) {
    if std::env::var("JET_ADAPTER_CANONICAL_PASS_PROCESS").ok().as_deref() != Some(process) {
        return;
    }
    let Some(path) = std::env::var_os("JET_ADAPTER_CANONICAL_PASS_PATH") else {
        return;
    };
    let records = take();
    if records.is_empty() {
        return;
    }
    let rows = records.iter().map(record_json).collect::<Vec<_>>().join(",");
    let payload = format!(
        "{{\"schema\":\"jet.canonical-pass-journal.v1\",\"protocol\":\"{}\",\"process\":\"{}\",\"records\":[{}]}}\n",
        PROTOCOL,
        json_escape(process),
        rows,
    );
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = file.write_all(payload.as_bytes());
}
/// A structured, source-backed AST snapshot. The pass journal intentionally
/// retains node kinds and order instead of reducing the program to one hash.
pub fn ast_payload(bundle: &ProgramBundle) -> String {
    let modules = bundle
        .modules
        .iter()
        .map(|module| {
            let items = module
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "{{\"index\":{index},\"kind\":\"{}\"}}",
                        item_kind(item)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"display\":\"{}\",\"alias\":\"{}\",\"source_length\":{},\"items\":[{}]}}",
                json_escape(&module.display),
                json_escape(&module.alias),
                module.source.len(),
                items
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"representation\":\"ast\",\"entry\":{},\"modules\":[{}]}}",
        bundle.entry, modules
    )
}

pub fn ast_identity(bundle: &ProgramBundle) -> String {
    let modules = bundle
        .modules
        .iter()
        .map(|module| {
            format!(
                "{{\"display\":\"{}\",\"items\":{}}}",
                json_escape(&module.display),
                module.items.len()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"representation\":\"ast\",\"entry\":{},\"modules\":[{}]}}",
        bundle.entry, modules
    )
}

fn item_kind(item: &Item) -> &'static str {
    match item {
        Item::Func(_) => "Func",
        Item::Struct(_) => "Struct",
        Item::Enum(_) => "Enum",
        Item::Distinct(_) => "Distinct",
        Item::TypeAlias(_) => "TypeAlias",
        Item::UnitFamily(_) => "UnitFamily",
        Item::Trait(_) => "Trait",
        Item::Tag(_) => "Tag",
        Item::EffectDecl(_) => "EffectDecl",
        Item::Impl(_) => "Impl",
        Item::Const(_) => "Const",
        Item::Test(_) => "Test",
        Item::ExternRust(_) => "ExternRust",
        Item::Module(_) => "Module",
        Item::CModule(_) => "CModule",
        Item::CodeModule(_) => "CodeModule",
        Item::ErrorConv(_) => "ErrorConv",
        Item::Migration(_) => "Migration",
        Item::ProtocolDecl(_) => "ProtocolDecl",
        Item::UserDerive(_) => "UserDerive",
        Item::TemplateLoop(_) => "TemplateLoop",
        Item::GenericModule(_) => "GenericModule",
        Item::ModuleAlias(_) => "ModuleAlias",
        Item::MarkerDecl(_) => "MarkerDecl",
        Item::FactDecl(_) => "FactDecl",
    }
}
pub fn ast_items_payload(items: &[Item]) -> String {
    let rows = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            format!(
                "{{\"index\":{index},\"kind\":\"{}\"}}",
                item_kind(item)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"representation\":\"ast\",\"items\":[{rows}]}}")
}

pub fn ast_items_identity(items: &[Item]) -> String {
    format!(
        "{{\"representation\":\"ast\",\"item_count\":{}}}",
        items.len()
    )
}

pub fn debug_payload<T: std::fmt::Debug>(
    representation: &str,
    operation_id: &str,
    value: &T,
) -> String {
    format!(
        "{{\"representation\":\"{}\",\"operation_id\":\"{}\",\"debug\":\"{}\"}}",
        json_escape(representation),
        json_escape(operation_id),
        json_escape(&format!("{value:?}")),
    )
}

pub fn debug_identity<T: std::fmt::Debug>(
    representation: &str,
    operation_id: &str,
    value: &T,
) -> String {
    format!(
        "{{\"representation\":\"{}\",\"operation_id\":\"{}\",\"identity\":\"{}\"}}",
        json_escape(representation),
        json_escape(operation_id),
        json_escape(&format!("{value:?}")),
    )
}
