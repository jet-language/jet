//! Encoding stream hosts (#729) — `include!` canonical EncodingStream + HostileIo.
//! File create/open plus JSON/JSONL/CSV/CBOR/XML reader/writer handles.

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
use super::Concurrency;
use super::Encoding::{alloc_datatree, read_datatree};
use crate::Marshal::{clone_string, result_err_msg, result_ok};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module as _;
use jet_codegen::AST::{CtKey, CtReport, CtValue, Type};
use jet_codegen::Diagnostics::{Diagnostic, Span};
use std::cell::RefCell;

/// Canonical stream runtime (jet_std types + EncodingStream algorithm).
#[allow(dead_code, unused_imports, unused_variables, clippy::all)]
pub(crate) mod runtime {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    trait JetShow {
        fn jet_show(&self) -> String;
    }
    trait JetDisplay {
        fn jet_display(&self) -> String;
    }

    pub struct JetFileReader {
        pub(crate) inner: std::io::BufReader<std::fs::File>,
        pub(crate) path: String,
    }
    pub struct JetFileWriter {
        pub(crate) inner: std::io::BufWriter<std::fs::File>,
        pub(crate) path: String,
    }

    pub mod jet_std {
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;
        use super::{JetDisplay, JetShow};

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct EncodingLimits {
            pub buffer_bytes: i64,
            pub max_depth: i64,
            pub max_item_bytes: i64,
            pub max_total_bytes: JetOutcome<i64, JetAbsent>,
            pub max_expansion_depth: i64,
            pub max_expansion_bytes: i64,
        }
        impl EncodingLimits {
            pub fn safe() -> Self {
                Self {
                    buffer_bytes: 65536,
                    max_depth: 256,
                    max_item_bytes: 16777216,
                    max_total_bytes: Err(JetAbsent),
                    max_expansion_depth: 32,
                    max_expansion_bytes: 8388608,
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum EncodingFormat {
            JSON,
            JSONL,
            CSV,
            XML,
            CBOR,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum EncodingErrorKind {
            Syntax,
            Truncated,
            Unsupported,
            Limit,
            IO,
            State,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct EncodingCause {
            pub kind: String,
            pub os_code: JetOutcome<i64, JetAbsent>,
            pub message: String,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct EncodingError {
            pub format: EncodingFormat,
            pub kind: EncodingErrorKind,
            pub byte_offset: i64,
            pub line: JetOutcome<i64, JetAbsent>,
            pub column: JetOutcome<i64, JetAbsent>,
            pub path: String,
            pub reason: String,
            pub cause: JetOutcome<EncodingCause, JetAbsent>,
        }
        impl EncodingError {
            pub fn cause(&self) -> JetOutcome<EncodingCause, JetAbsent> {
                self.cause.clone()
            }
            fn display_text(&self) -> String {
                let mut out = format!("{:?} {:?} at byte {}", self.format, self.kind, self.byte_offset);
                if let Ok(line) = self.line {
                    out.push_str(&format!(", line {line}"));
                }
                if let Ok(column) = self.column {
                    out.push_str(&format!(", column {column}"));
                }
                if !self.path.is_empty() {
                    out.push_str(&format!(", path {}", self.path));
                }
                out.push_str(&format!(": {}", self.reason));
                out
            }
        }
        impl std::fmt::Display for EncodingError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.display_text())
            }
        }
        impl super::JetShow for EncodingError {
            fn jet_show(&self) -> String {
                self.display_text()
            }
        }
        impl super::JetDisplay for EncodingError {
            fn jet_display(&self) -> String {
                self.display_text()
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum DataEvent {
            Null,
            Bool(bool),
            Int(i64),
            Float(f64),
            Text(String),
            Bytes(Vec<u8>),
            ArrayStart,
            ArrayEnd,
            ObjectStart,
            Key(String),
            ObjectEnd,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum DataTree {
            Null,
            Bool(bool),
            Int(i64),
            Float(f64),
            Text(String),
            Bytes(Vec<u8>),
            Array(Vec<DataTree>),
            Object(Vec<(String, DataTree)>),
        }

        // #1636: one home for the limb-arithmetic bigint routine.
        // `jet_foundation::Numeric::CtBigInt` (already a direct dependency of
        // jet-jit) is the canonical compiled implementation; `EncodingStream.rs`
        // below just borrows the name locally so its `jet_std::JetBigInt` call
        // sites read unchanged. The Prelude's own `JetBigInt` (CommonTypes.rs)
        // stays a separate, hand-mirrored copy — AOT output is a standalone
        // Rust program that can't link back into the compiler, the same
        // constraint `JetDecimal`/`CtDecimal` already document there.
        pub use jet_foundation::Numeric::CtBigInt as JetBigInt;

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum XMLReason {
            InvalidEncoding,
            Malformed,
            MismatchedTag,
            InvalidName,
            Namespace,
            DuplicateAttribute,
            Entity,
            EntityCycle,
            Limit,
            Canonicalization,
            Shape,
            Unsupported,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct XMLError {
            pub kind: XMLReason,
            pub byte_offset: Option<i64>,
            pub line: JetOutcome<i64, JetAbsent>,
            pub column: JetOutcome<i64, JetAbsent>,
            pub path: String,
            pub reason: String,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct XMLLimits {
            pub max_depth: i64,
            pub max_nodes: i64,
            pub max_attributes_per_element: i64,
            pub max_name_bytes: i64,
            pub max_text_bytes: i64,
            pub max_entity_declarations: i64,
            pub max_entity_depth: i64,
            pub max_entity_replacement_bytes: i64,
        }
        impl XMLLimits {
            pub fn safe() -> Self {
                Self {
                    max_depth: 256,
                    max_nodes: 1_000_000,
                    max_attributes_per_element: 1024,
                    max_name_bytes: 4096,
                    max_text_bytes: 16_777_216,
                    max_entity_declarations: 1024,
                    max_entity_depth: 32,
                    max_entity_replacement_bytes: 8_388_608,
                }
            }
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum XMLEntityPolicy {
            Preserve,
            Reject,
            Resolve(std::collections::BTreeMap<String, String>),
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct XMLParseOptions {
            pub entities: XMLEntityPolicy,
            pub limits: XMLLimits,
        }
        impl XMLParseOptions {
            pub fn safe() -> Self {
                Self {
                    entities: XMLEntityPolicy::Preserve,
                    limits: XMLLimits::safe(),
                }
            }
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum XMLEncoding {
            UTF8,
            UTF8BOM,
            UTF16LE,
            UTF16BE,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum XMLLexicalPolicy {
            PreserveValid,
            Deterministic,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct XMLRenderOptions {
            pub encoding: XMLEncoding,
            pub lexical: XMLLexicalPolicy,
        }
        impl XMLRenderOptions {
            pub fn safe() -> Self {
                Self {
                    encoding: XMLEncoding::UTF8,
                    lexical: XMLLexicalPolicy::PreserveValid,
                }
            }
        }

        pub struct JSONReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) total: i64,
            pub(crate) offset: i64,
            pub(crate) line: i64,
            pub(crate) column: i64,
            pub(crate) lookahead: Option<u8>,
            pub(crate) frames: Vec<super::JetJSONReadFrame>,
            pub(crate) root_started: bool,
            pub(crate) root_done: bool,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) record_mode: bool,
            pub(crate) allocation_budget: Option<super::JetJSONAllocationBudget>,
        }
        pub struct JSONWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) frames: Vec<super::JetJSONWriteFrame>,
            pub(crate) root_written: bool,
            pub(crate) finished: bool,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) canonical: bool,
            pub(crate) canonical_frames: Vec<super::JetJSONCanonicalFrame>,
            pub(crate) canonical_retained: usize,
        }
        pub struct JSONLReader {
            pub(crate) json: JSONReader,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) record_index: i64,
        }
        pub struct JSONLWriter {
            pub(crate) json: JSONWriter,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) record_index: i64,
            pub(crate) finished: bool,
            pub(crate) pending_lf: bool,
        }
        pub struct CSVReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) total: i64,
            pub(crate) offset: i64,
            pub(crate) line: i64,
            pub(crate) column: i64,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) record_index: i64,
        }
        pub struct CSVWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) record_index: i64,
            pub(crate) finished: bool,
            pub(crate) pending_crlf: bool,
        }
        pub struct XMLReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) scanner: crate::jet_xml_pull::StreamScanner,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) eof: bool,
            pub(crate) allocation: super::JetJSONAllocationBudget,
        }
        pub struct XMLWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) renderer: crate::jet_xml_pull::StreamWriter,
            pub(crate) buffer: Vec<u8>,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) finished: bool,
        }
        pub struct CBORReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) total: i64,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) root_done: bool,
            pub(crate) lookahead: Option<u8>,
            pub(crate) frames: Vec<super::JetCBORReadFrame>,
            pub(crate) retained: usize,
            pub(crate) workspace: usize,
            pub(crate) allocation: super::JetJSONAllocationBudget,
        }
        pub struct CBORWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) frames: Vec<super::JetCBORWriteFrame>,
            pub(crate) root_written: bool,
            pub(crate) finished: bool,
            pub(crate) retained: usize,
            pub(crate) workspace: usize,
            pub(crate) allocation: super::JetJSONAllocationBudget,
        }
    }

    // EncodingCodecs helpers EncodingStream calls for XML.
    fn jet_xml_to_data_tree(value: crate::jet_xml_pull::Value) -> jet_std::DataTree {
        match value {
            crate::jet_xml_pull::Value::Null => jet_std::DataTree::Null,
            crate::jet_xml_pull::Value::Bool(value) => jet_std::DataTree::Bool(value),
            crate::jet_xml_pull::Value::Int(value) => jet_std::DataTree::Int(value),
            crate::jet_xml_pull::Value::Text(value) => jet_std::DataTree::Text(value),
            crate::jet_xml_pull::Value::Array(values) => {
                jet_std::DataTree::Array(values.into_iter().map(jet_xml_to_data_tree).collect())
            }
            crate::jet_xml_pull::Value::Object(entries) => jet_std::DataTree::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, jet_xml_to_data_tree(value)))
                    .collect(),
            ),
        }
    }

    fn jet_xml_from_data_tree(value: &jet_std::DataTree) -> Result<crate::jet_xml_pull::Value, String> {
        match value {
            jet_std::DataTree::Null => Ok(crate::jet_xml_pull::Value::Null),
            jet_std::DataTree::Bool(value) => Ok(crate::jet_xml_pull::Value::Bool(*value)),
            jet_std::DataTree::Int(value) => Ok(crate::jet_xml_pull::Value::Int(*value)),
            jet_std::DataTree::Text(value) => Ok(crate::jet_xml_pull::Value::Text(value.clone())),
            jet_std::DataTree::Array(values) => Ok(crate::jet_xml_pull::Value::Array(
                values
                    .iter()
                    .map(jet_xml_from_data_tree)
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            jet_std::DataTree::Object(entries) => Ok(crate::jet_xml_pull::Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), jet_xml_from_data_tree(value)?)))
                    .collect::<Result<Vec<_>, String>>()?,
            )),
            jet_std::DataTree::Float(_) | jet_std::DataTree::Bytes(_) => {
                Err("XML tree cannot contain Float or Bytes values".to_string())
            }
        }
    }

    fn jet_xml_options(options: &jet_std::XMLParseOptions) -> crate::jet_xml_pull::ParseOptions {
        let number = |value: i64| usize::try_from(value).unwrap_or(usize::MAX);
        let entities = match &options.entities {
            jet_std::XMLEntityPolicy::Preserve => crate::jet_xml_pull::EntityPolicy::Preserve,
            jet_std::XMLEntityPolicy::Reject => crate::jet_xml_pull::EntityPolicy::Reject,
            jet_std::XMLEntityPolicy::Resolve(values) => {
                crate::jet_xml_pull::EntityPolicy::Resolve(values.clone())
            }
        };
        crate::jet_xml_pull::ParseOptions {
            entities,
            limits: crate::jet_xml_pull::Limits {
                max_depth: number(options.limits.max_depth),
                max_nodes: number(options.limits.max_nodes),
                max_attributes_per_element: number(options.limits.max_attributes_per_element),
                max_name_bytes: number(options.limits.max_name_bytes),
                max_text_bytes: number(options.limits.max_text_bytes),
                max_entity_declarations: number(options.limits.max_entity_declarations),
                max_entity_depth: number(options.limits.max_entity_depth),
                max_entity_replacement_bytes: number(options.limits.max_entity_replacement_bytes),
            },
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/EncodingHostileIo.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/EncodingStream.rs");

    pub(crate) fn enc_json_writer(
        output: JetFileWriter,
        limits: jet_std::EncodingLimits,
        canonical: bool,
    ) -> Result<jet_std::JSONWriter, jet_std::EncodingError> {
        jet_enc_json_writer(output, limits, canonical)
    }
    pub(crate) fn enc_json_canonical(
        value: &jet_std::DataTree,
        limits: &jet_std::EncodingLimits,
    ) -> Result<String, jet_std::EncodingError> {
        jet_enc_json_canonical(value, limits)
    }
    pub(crate) fn enc_json_reader(
        input: JetFileReader,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::JSONReader, jet_std::EncodingError> {
        jet_enc_json_reader(input, limits)
    }
    pub(crate) fn enc_json_writer_write(
        writer: &mut jet_std::JSONWriter,
        event: jet_std::DataEvent,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_json_writer_write(writer, event)
    }
    pub(crate) fn enc_json_writer_flush(
        writer: &mut jet_std::JSONWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_json_writer_flush(writer)
    }
    pub(crate) fn enc_json_writer_finish(
        writer: &mut jet_std::JSONWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_json_writer_finish(writer)
    }
    pub(crate) fn enc_json_reader_next(
        reader: &mut jet_std::JSONReader,
    ) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        jet_enc_json_reader_next(reader).map(|found| found.ok())
    }
    pub(crate) fn enc_jsonl_writer(
        output: JetFileWriter,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::JSONLWriter, jet_std::EncodingError> {
        jet_enc_jsonl_writer(output, limits)
    }
    pub(crate) fn enc_jsonl_reader(
        input: JetFileReader,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::JSONLReader, jet_std::EncodingError> {
        jet_enc_jsonl_reader(input, limits)
    }
    pub(crate) fn enc_jsonl_writer_write(
        writer: &mut jet_std::JSONLWriter,
        value: jet_std::DataTree,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_jsonl_writer_write(writer, value)
    }
    pub(crate) fn enc_jsonl_writer_flush(
        writer: &mut jet_std::JSONLWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_jsonl_writer_flush(writer)
    }
    pub(crate) fn enc_jsonl_writer_finish(
        writer: &mut jet_std::JSONLWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_jsonl_writer_finish(writer)
    }
    pub(crate) fn enc_jsonl_reader_next(
        reader: &mut jet_std::JSONLReader,
    ) -> Result<Option<jet_std::DataTree>, jet_std::EncodingError> {
        jet_enc_jsonl_reader_next(reader).map(|found| found.ok())
    }
    pub(crate) fn enc_csv_writer(
        output: JetFileWriter,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::CSVWriter, jet_std::EncodingError> {
        jet_enc_csv_writer(output, limits)
    }
    pub(crate) fn enc_csv_reader(
        input: JetFileReader,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::CSVReader, jet_std::EncodingError> {
        jet_enc_csv_reader(input, limits)
    }
    pub(crate) fn enc_csv_writer_write(
        writer: &mut jet_std::CSVWriter,
        row: Vec<String>,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_csv_writer_write(writer, row)
    }
    pub(crate) fn enc_csv_writer_flush(
        writer: &mut jet_std::CSVWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_csv_writer_flush(writer)
    }
    pub(crate) fn enc_csv_writer_finish(
        writer: &mut jet_std::CSVWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_csv_writer_finish(writer)
    }
    pub(crate) fn enc_csv_reader_next(
        reader: &mut jet_std::CSVReader,
    ) -> Result<Option<Vec<String>>, jet_std::EncodingError> {
        jet_enc_csv_reader_next(reader).map(|found| found.ok())
    }
    pub(crate) fn enc_cbor_writer(
        output: JetFileWriter,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::CBORWriter, jet_std::EncodingError> {
        jet_enc_cbor_writer(output, limits)
    }
    pub(crate) fn enc_cbor_reader(
        input: JetFileReader,
        limits: jet_std::EncodingLimits,
    ) -> Result<jet_std::CBORReader, jet_std::EncodingError> {
        jet_enc_cbor_reader(input, limits)
    }
    pub(crate) fn enc_cbor_writer_write(
        writer: &mut jet_std::CBORWriter,
        event: jet_std::DataEvent,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_cbor_writer_write(writer, event)
    }
    pub(crate) fn enc_cbor_writer_flush(
        writer: &mut jet_std::CBORWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_cbor_writer_flush(writer)
    }
    pub(crate) fn enc_cbor_writer_finish(
        writer: &mut jet_std::CBORWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_cbor_writer_finish(writer)
    }
    pub(crate) fn enc_cbor_reader_next(
        reader: &mut jet_std::CBORReader,
    ) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        jet_enc_cbor_reader_next(reader).map(|found| found.ok())
    }
    pub(crate) fn enc_xml_writer(
        output: JetFileWriter,
        limits: jet_std::EncodingLimits,
        xml: jet_std::XMLRenderOptions,
    ) -> Result<jet_std::XMLWriter, jet_std::EncodingError> {
        jet_enc_xml_writer(output, limits, xml)
    }
    pub(crate) fn enc_xml_reader(
        input: JetFileReader,
        limits: jet_std::EncodingLimits,
        xml: jet_std::XMLParseOptions,
    ) -> Result<jet_std::XMLReader, jet_std::EncodingError> {
        jet_enc_xml_reader(input, limits, xml)
    }
    pub(crate) fn enc_xml_writer_write(
        writer: &mut jet_std::XMLWriter,
        event: jet_std::DataTree,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_xml_writer_write(writer, event)
    }
    pub(crate) fn enc_xml_writer_flush(
        writer: &mut jet_std::XMLWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_xml_writer_flush(writer)
    }
    pub(crate) fn enc_xml_writer_finish(
        writer: &mut jet_std::XMLWriter,
    ) -> Result<(), jet_std::EncodingError> {
        jet_enc_xml_writer_finish(writer)
    }
    pub(crate) fn enc_xml_reader_next(
        reader: &mut jet_std::XMLReader,
    ) -> Result<Option<jet_std::DataTree>, jet_std::EncodingError> {
        jet_enc_xml_reader_next(reader).map(|found| found.ok())
    }
}

// ── TIR deopt stream bridge ─────────────────────────────────────────────────
//
// The resident JIT stores these handles in JitRuntime. Whole-program deopt
// runs on the evaluator worker instead, so its CtValue handles use this
// thread-local table. Both tables call the same Prelude stream kernel.
enum AmbientStream {
    FileReader(runtime::JetFileReader),
    FileWriter(runtime::JetFileWriter),
    JSONReader(runtime::jet_std::JSONReader),
    JSONWriter(runtime::jet_std::JSONWriter),
    JSONLReader(runtime::jet_std::JSONLReader),
    JSONLWriter(runtime::jet_std::JSONLWriter),
    CSVReader(runtime::jet_std::CSVReader),
    CSVWriter(runtime::jet_std::CSVWriter),
    XMLReader(runtime::jet_std::XMLReader),
    XMLWriter(runtime::jet_std::XMLWriter),
    CBORReader(runtime::jet_std::CBORReader),
    CBORWriter(runtime::jet_std::CBORWriter),
}

thread_local! {
    static AMBIENT_STREAMS: RefCell<Vec<Option<AmbientStream>>> =
        const { RefCell::new(Vec::new()) };
}

fn ambient_stream_insert(stream: AmbientStream) -> i64 {
    AMBIENT_STREAMS.with(|slots| {
        let mut slots = slots.borrow_mut();
        slots.push(Some(stream));
        slots.len() as i64
    })
}

fn ambient_stream_id(value: &CtValue) -> Option<usize> {
    match value {
        CtValue::Int(id) if *id > 0 => Some((*id - 1) as usize),
        _ => None,
    }
}

fn ambient_stream_with<R>(
    handle: i64,
    f: impl FnOnce(&mut AmbientStream) -> Result<R, String>,
) -> Result<R, String> {
    let Some(index) = ambient_stream_id(&CtValue::Int(handle)) else {
        return Err("bad stream handle".to_string());
    };
    AMBIENT_STREAMS.with(|slots| {
        let mut slots = slots.borrow_mut();
        match slots.get_mut(index) {
            Some(Some(stream)) => f(stream),
            Some(None) => Err("stream handle already moved".to_string()),
            None => Err("bad stream handle".to_string()),
        }
    })
}

fn ambient_stream_take_file_reader(handle: i64) -> Result<runtime::JetFileReader, String> {
    let Some(index) = ambient_stream_id(&CtValue::Int(handle)) else {
        return Err("bad FileReader".to_string());
    };
    AMBIENT_STREAMS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let slot = slots
            .get_mut(index)
            .ok_or_else(|| "bad FileReader".to_string())?;
        let stream = slot
            .take()
            .ok_or_else(|| "FileReader already moved".to_string())?;
        match stream {
            AmbientStream::FileReader(reader) => Ok(reader),
            other => {
                *slot = Some(other);
                Err("expected FileReader".to_string())
            }
        }
    })
}

/// Move a core.files reader into a shared Prelude body adapter.
pub(crate) fn take_file_reader_for_http(handle: i64) -> Result<runtime::JetFileReader, String> {
    ambient_stream_take_file_reader(handle)
}

fn ambient_stream_take_file_writer(handle: i64) -> Result<runtime::JetFileWriter, String> {
    let Some(index) = ambient_stream_id(&CtValue::Int(handle)) else {
        return Err("bad FileWriter".to_string());
    };
    AMBIENT_STREAMS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let slot = slots
            .get_mut(index)
            .ok_or_else(|| "bad FileWriter".to_string())?;
        let stream = slot
            .take()
            .ok_or_else(|| "FileWriter already moved".to_string())?;
        match stream {
            AmbientStream::FileWriter(writer) => Ok(writer),
            other => {
                *slot = Some(other);
                Err("expected FileWriter".to_string())
            }
        }
    })
}

/// Move a core.files writer into a shared Prelude body adapter.
pub(crate) fn take_file_writer_for_http(handle: i64) -> Result<runtime::JetFileWriter, String> {
    ambient_stream_take_file_writer(handle)
}

fn ambient_ok(value: CtValue) -> CtValue {
    CtValue::Present(Box::new(value))
}

fn ambient_failed(value: CtValue) -> CtValue {
    CtValue::failed(Box::new(value))
}

fn ambient_path(value: &CtValue) -> Option<String> {
    match value {
        CtValue::Str(path) => Some(path.clone()),
        CtValue::Struct { type_name, fields } if type_name == "Path" => fields
            .iter()
            .find_map(|(name, value)| {
                (name == "inner").then(|| match value {
                    CtValue::Str(path) => Some(path.clone()),
                    _ => None,
                })
            })
            .flatten(),
        _ => None,
    }
}

fn ambient_limits(value: Option<&CtValue>) -> runtime::jet_std::EncodingLimits {
    let mut limits = runtime::jet_std::EncodingLimits::safe();
    let Some(CtValue::Struct { type_name, fields }) = value else {
        return limits;
    };
    if type_name != "EncodingLimits" {
        return limits;
    }
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("buffer_bytes", CtValue::Int(value)) => limits.buffer_bytes = *value,
            ("max_depth", CtValue::Int(value)) => limits.max_depth = *value,
            ("max_item_bytes", CtValue::Int(value)) => limits.max_item_bytes = *value,
            ("max_total_bytes", CtValue::Present(inner)) => {
                if let CtValue::Int(value) = inner.as_ref() {
                    limits.max_total_bytes = Ok(*value);
                }
            }
            ("max_total_bytes", CtValue::Int(value)) => limits.max_total_bytes = Ok(*value),
            ("max_total_bytes", CtValue::Failed(CtReport::Clean(_))) => {
                limits.max_total_bytes = Err(JetAbsent)
            }
            ("max_expansion_depth", CtValue::Int(value)) => limits.max_expansion_depth = *value,
            ("max_expansion_bytes", CtValue::Int(value)) => limits.max_expansion_bytes = *value,
            _ => {}
        }
    }
    limits
}

fn ambient_bytes(value: &CtValue) -> Result<Vec<u8>, String> {
    match value {
        CtValue::Bytes(bytes) => Ok(bytes.clone()),
        CtValue::List(items) => items
            .iter()
            .map(|value| match value {
                CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                _ => Err("expected byte list".to_string()),
            })
            .collect(),
        _ => Err("expected bytes".to_string()),
    }
}

fn ambient_enum_arg<'a>(
    value: &'a CtValue,
    type_name: &str,
) -> Result<(&'a str, &'a [(Option<String>, CtValue)]), String> {
    let CtValue::Enum {
        type_name: actual,
        variant,
        args,
    } = value
    else {
        return Err(format!("expected {type_name}"));
    };
    if actual != type_name && !actual.ends_with(&format!(".{type_name}")) {
        return Err(format!("expected {type_name}"));
    }
    Ok((variant, args))
}

fn ambient_event(value: &CtValue) -> Result<runtime::jet_std::DataEvent, String> {
    let (variant, args) = ambient_enum_arg(value, "DataEvent")?;
    let arg = || {
        args.first()
            .map(|(_, value)| value)
            .ok_or_else(|| format!("DataEvent.{variant} needs a value"))
    };
    match variant {
        "Null" => Ok(runtime::jet_std::DataEvent::Null),
        "Bool" => match arg()? {
            CtValue::Bool(value) => Ok(runtime::jet_std::DataEvent::Bool(*value)),
            _ => Err("DataEvent.Bool expects Bool".to_string()),
        },
        "Int" => match arg()? {
            CtValue::Int(value) => Ok(runtime::jet_std::DataEvent::Int(*value)),
            _ => Err("DataEvent.Int expects Int".to_string()),
        },
        "Float" => match arg()? {
            CtValue::Float(value) => Ok(runtime::jet_std::DataEvent::Float(value.as_f64())),
            CtValue::Int(value) => Ok(runtime::jet_std::DataEvent::Float(*value as f64)),
            _ => Err("DataEvent.Float expects Float".to_string()),
        },
        "Text" => match arg()? {
            CtValue::Str(value) => Ok(runtime::jet_std::DataEvent::Text(value.clone())),
            _ => Err("DataEvent.Text expects String".to_string()),
        },
        "Bytes" => Ok(runtime::jet_std::DataEvent::Bytes(ambient_bytes(arg()?)?)),
        "ArrayStart" => Ok(runtime::jet_std::DataEvent::ArrayStart),
        "ArrayEnd" => Ok(runtime::jet_std::DataEvent::ArrayEnd),
        "ObjectStart" => Ok(runtime::jet_std::DataEvent::ObjectStart),
        "Key" => match arg()? {
            CtValue::Str(value) => Ok(runtime::jet_std::DataEvent::Key(value.clone())),
            _ => Err("DataEvent.Key expects String".to_string()),
        },
        "ObjectEnd" => Ok(runtime::jet_std::DataEvent::ObjectEnd),
        _ => Err(format!("unsupported DataEvent.{variant}")),
    }
}

fn ambient_tree(value: &CtValue) -> Result<runtime::jet_std::DataTree, String> {
    let (variant, args) = ambient_enum_arg(value, "DataTree")?;
    let arg = || {
        args.first()
            .map(|(_, value)| value)
            .ok_or_else(|| format!("DataTree.{variant} needs a value"))
    };
    match variant {
        "Null" => Ok(runtime::jet_std::DataTree::Null),
        "Bool" => match arg()? {
            CtValue::Bool(value) => Ok(runtime::jet_std::DataTree::Bool(*value)),
            _ => Err("DataTree.Bool expects Bool".to_string()),
        },
        "Int" => match arg()? {
            CtValue::Int(value) => Ok(runtime::jet_std::DataTree::Int(*value)),
            _ => Err("DataTree.Int expects Int".to_string()),
        },
        "Float" => match arg()? {
            CtValue::Float(value) => Ok(runtime::jet_std::DataTree::Float(value.as_f64())),
            CtValue::Int(value) => Ok(runtime::jet_std::DataTree::Float(*value as f64)),
            _ => Err("DataTree.Float expects Float".to_string()),
        },
        "Text" => match arg()? {
            CtValue::Str(value) => Ok(runtime::jet_std::DataTree::Text(value.clone())),
            _ => Err("DataTree.Text expects String".to_string()),
        },
        "Bytes" => Ok(runtime::jet_std::DataTree::Bytes(ambient_bytes(arg()?)?)),
        "Array" => match arg()? {
            CtValue::List(values) => values
                .iter()
                .map(ambient_tree)
                .collect::<Result<Vec<_>, _>>()
                .map(runtime::jet_std::DataTree::Array),
            _ => Err("DataTree.Array expects a list".to_string()),
        },
        "Object" => {
            let value = arg()?;
            let fields: Vec<(String, CtValue)> = match value {
                CtValue::Struct { type_name, fields } if type_name == "JSONObject" => fields.clone(),
                CtValue::Map(fields) => fields
                    .iter()
                    .map(|(key, value)| match key {
                        CtKey::Str(key) => Ok((key.clone(), value.clone())),
                        _ => Err("DataTree.Object key must be String".to_string()),
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err("DataTree.Object expects an object".to_string()),
            };
            Ok(runtime::jet_std::DataTree::Object(
                fields
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), ambient_tree(value)?)))
                    .collect::<Result<_, String>>()?,
            ))
        }
        _ => Err(format!("unsupported DataTree.{variant}")),
    }
}

fn ambient_event_value(value: runtime::jet_std::DataEvent) -> CtValue {
    let (variant, arg) = match value {
        runtime::jet_std::DataEvent::Null => ("Null", None),
        runtime::jet_std::DataEvent::Bool(value) => ("Bool", Some(CtValue::Bool(value))),
        runtime::jet_std::DataEvent::Int(value) => ("Int", Some(CtValue::Int(value))),
        runtime::jet_std::DataEvent::Float(value) => (
            "Float",
            Some(CtValue::Float(jet_codegen::AST::CtFloat::f64(value))),
        ),
        runtime::jet_std::DataEvent::Text(value) => ("Text", Some(CtValue::Str(value))),
        runtime::jet_std::DataEvent::Bytes(value) => ("Bytes", Some(CtValue::Bytes(value))),
        runtime::jet_std::DataEvent::ArrayStart => ("ArrayStart", None),
        runtime::jet_std::DataEvent::ArrayEnd => ("ArrayEnd", None),
        runtime::jet_std::DataEvent::ObjectStart => ("ObjectStart", None),
        runtime::jet_std::DataEvent::Key(value) => ("Key", Some(CtValue::Str(value))),
        runtime::jet_std::DataEvent::ObjectEnd => ("ObjectEnd", None),
    };
    CtValue::Enum {
        type_name: "DataEvent".to_string(),
        variant: variant.to_string(),
        args: arg.into_iter().map(|value| (None, value)).collect(),
    }
}

fn ambient_tree_value(value: runtime::jet_std::DataTree) -> CtValue {
    let (variant, arg) = match value {
        runtime::jet_std::DataTree::Null => ("Null", None),
        runtime::jet_std::DataTree::Bool(value) => ("Bool", Some(CtValue::Bool(value))),
        runtime::jet_std::DataTree::Int(value) => ("Int", Some(CtValue::Int(value))),
        runtime::jet_std::DataTree::Float(value) => (
            "Float",
            Some(CtValue::Float(jet_codegen::AST::CtFloat::f64(value))),
        ),
        runtime::jet_std::DataTree::Text(value) => ("Text", Some(CtValue::Str(value))),
        runtime::jet_std::DataTree::Bytes(value) => ("Bytes", Some(CtValue::Bytes(value))),
        runtime::jet_std::DataTree::Array(values) => (
            "Array",
            Some(CtValue::List(values.into_iter().map(ambient_tree_value).collect())),
        ),
        runtime::jet_std::DataTree::Object(fields) => (
            "Object",
            Some(CtValue::Struct {
                type_name: "JSONObject".to_string(),
                fields: fields
                    .into_iter()
                    .map(|(key, value)| (key, ambient_tree_value(value)))
                    .collect(),
            }),
        ),
    };
    CtValue::Enum {
        type_name: "DataTree".to_string(),
        variant: variant.to_string(),
        args: arg.into_iter().map(|value| (None, value)).collect(),
    }
}

fn ambient_encoding_error(error: &runtime::jet_std::EncodingError) -> CtValue {
    let format = match error.format {
        runtime::jet_std::EncodingFormat::JSON => "JSON",
        runtime::jet_std::EncodingFormat::JSONL => "JSONL",
        runtime::jet_std::EncodingFormat::CSV => "CSV",
        runtime::jet_std::EncodingFormat::XML => "XML",
        runtime::jet_std::EncodingFormat::CBOR => "CBOR",
    };
    let kind = match error.kind {
        runtime::jet_std::EncodingErrorKind::Syntax => "Syntax",
        runtime::jet_std::EncodingErrorKind::Truncated => "Truncated",
        runtime::jet_std::EncodingErrorKind::Unsupported => "Unsupported",
        runtime::jet_std::EncodingErrorKind::Limit => "Limit",
        runtime::jet_std::EncodingErrorKind::IO => "IO",
        runtime::jet_std::EncodingErrorKind::State => "State",
    };
    let optional = |value: Result<i64, JetAbsent>| match value {
        Ok(value) => ambient_ok(CtValue::Int(value)),
        Err(JetAbsent) => CtValue::absent(Type::Int),
    };
    let cause = match &error.cause {
        Ok(cause) => ambient_ok(CtValue::Struct {
            type_name: "EncodingCause".to_string(),
            fields: vec![
                ("kind".to_string(), CtValue::Str(cause.kind.clone())),
                (
                    "os_code".to_string(),
                    match cause.os_code {
                        Ok(value) => ambient_ok(CtValue::Int(value)),
                        Err(JetAbsent) => CtValue::absent(Type::Int),
                    },
                ),
                ("message".to_string(), CtValue::Str(cause.message.clone())),
            ],
        }),
        Err(JetAbsent) => CtValue::absent(Type::Named("EncodingCause".to_string())),
    };
    CtValue::Struct {
        type_name: "EncodingError".to_string(),
        fields: vec![
            (
                "format".to_string(),
                CtValue::Enum {
                    type_name: "EncodingFormat".to_string(),
                    variant: format.to_string(),
                    args: vec![],
                },
            ),
            (
                "kind".to_string(),
                CtValue::Enum {
                    type_name: "EncodingErrorKind".to_string(),
                    variant: kind.to_string(),
                    args: vec![],
                },
            ),
            ("byte_offset".to_string(), CtValue::Int(error.byte_offset)),
            ("line".to_string(), optional(error.line)),
            ("column".to_string(), optional(error.column)),
            ("path".to_string(), CtValue::Str(error.path.clone())),
            ("reason".to_string(), CtValue::Str(error.reason.clone())),
            ("cause".to_string(), cause),
        ],
    }
}

fn ambient_io_error(operation: &str, path: &str, error: impl Into<String>) -> CtValue {
    CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: "Other".to_string(),
        args: vec![(
            None,
            CtValue::Struct {
                type_name: "IOContext".to_string(),
                fields: vec![
                    (
                        "operation".to_string(),
                        CtValue::Enum {
                            type_name: "IOOperation".to_string(),
                            variant: operation.to_string(),
                            args: vec![],
                        },
                    ),
                    (
                        "resource".to_string(),
                        ambient_ok(CtValue::Str(path.to_string())),
                    ),
                    ("os_code".to_string(), CtValue::absent(Type::Int)),
                    (
                        "cause".to_string(),
                        ambient_ok(CtValue::Str(error.into())),
                    ),
                ],
            },
        )],
    }
}

fn ambient_next<T>(
    result: Result<Option<T>, runtime::jet_std::EncodingError>,
    absent_type: Type,
    convert: impl FnOnce(T) -> CtValue,
) -> CtValue {
    match result {
        Ok(Some(value)) => ambient_ok(ambient_ok(convert(value))),
        Ok(None) => ambient_ok(CtValue::absent(absent_type)),
        Err(error) => ambient_failed(ambient_encoding_error(&error)),
    }
}

fn ambient_unit(result: Result<(), runtime::jet_std::EncodingError>) -> CtValue {
    match result {
        Ok(()) => ambient_ok(CtValue::Unit),
        Err(error) => ambient_failed(ambient_encoding_error(&error)),
    }
}

fn ambient_unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::e0956_unsupported(what, span)
}

pub(crate) fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    match (module, method) {
        ("core.files", "create" | "append") => {
            let Some(path) = args.first().and_then(ambient_path) else {
                return Some(Err(ambient_unsupported(
                    "core.files.create path",
                    span,
                )));
            };
            let file = if method == "append" {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
            } else {
                std::fs::File::create(&path)
            };
            Some(Ok(match file {
                Ok(file) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::FileWriter(runtime::JetFileWriter {
                        inner: std::io::BufWriter::new(file),
                        path,
                    }),
                ))),
                Err(error) => ambient_failed(ambient_io_error("Write", &path, error.to_string())),
            }))
        }
        ("core.files", "open") => {
            let Some(path) = args.first().and_then(ambient_path) else {
                return Some(Err(ambient_unsupported(
                    "core.files.open path",
                    span,
                )));
            };
            Some(Ok(match std::fs::File::open(&path) {
                Ok(file) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::FileReader(runtime::JetFileReader {
                        inner: std::io::BufReader::new(file),
                        path,
                    }),
                ))),
                Err(error) => ambient_failed(ambient_io_error("Read", &path, error.to_string())),
            }))
        }
        ("core.encoding.json", "reader") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.json.reader FileReader",
                    span,
                )));
            };
            let reader = match ambient_stream_take_file_reader(*file) {
                Ok(reader) => reader,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.json.reader: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_json_reader(reader, ambient_limits(args.get(1))) {
                Ok(reader) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::JSONReader(reader),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.json", "writer") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.json.writer FileWriter",
                    span,
                )));
            };
            let writer = match ambient_stream_take_file_writer(*file) {
                Ok(writer) => writer,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.json.writer: {error}"),
                        span,
                    )))
                }
            };
            let canonical = args.get(2).and_then(|value| match value {
                CtValue::Bool(value) => Some(*value),
                _ => None,
            }).unwrap_or(false);
            Some(Ok(match runtime::enc_json_writer(
                writer,
                ambient_limits(args.get(1)),
                canonical,
            ) {
                Ok(writer) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::JSONWriter(writer),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.jsonl", "reader") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.jsonl.reader FileReader",
                    span,
                )));
            };
            let reader = match ambient_stream_take_file_reader(*file) {
                Ok(reader) => reader,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.jsonl.reader: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_jsonl_reader(reader, ambient_limits(args.get(1))) {
                Ok(reader) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::JSONLReader(reader),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.jsonl", "writer") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.jsonl.writer FileWriter",
                    span,
                )));
            };
            let writer = match ambient_stream_take_file_writer(*file) {
                Ok(writer) => writer,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.jsonl.writer: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_jsonl_writer(writer, ambient_limits(args.get(1))) {
                Ok(writer) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::JSONLWriter(writer),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.csv", "reader") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.csv.reader FileReader",
                    span,
                )));
            };
            let reader = match ambient_stream_take_file_reader(*file) {
                Ok(reader) => reader,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.csv.reader: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_csv_reader(reader, ambient_limits(args.get(1))) {
                Ok(reader) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::CSVReader(reader),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.csv", "writer") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.csv.writer FileWriter",
                    span,
                )));
            };
            let writer = match ambient_stream_take_file_writer(*file) {
                Ok(writer) => writer,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.csv.writer: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_csv_writer(writer, ambient_limits(args.get(1))) {
                Ok(writer) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::CSVWriter(writer),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.xml", "reader") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.xml.reader FileReader",
                    span,
                )));
            };
            let reader = match ambient_stream_take_file_reader(*file) {
                Ok(reader) => reader,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.xml.reader: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_xml_reader(
                reader,
                ambient_limits(args.get(1)),
                runtime::jet_std::XMLParseOptions::safe(),
            ) {
                Ok(reader) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::XMLReader(reader),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.xml", "writer") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.xml.writer FileWriter",
                    span,
                )));
            };
            let writer = match ambient_stream_take_file_writer(*file) {
                Ok(writer) => writer,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.xml.writer: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_xml_writer(
                writer,
                ambient_limits(args.get(1)),
                runtime::jet_std::XMLRenderOptions::safe(),
            ) {
                Ok(writer) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::XMLWriter(writer),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.cbor", "reader") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.cbor.reader FileReader",
                    span,
                )));
            };
            let reader = match ambient_stream_take_file_reader(*file) {
                Ok(reader) => reader,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.cbor.reader: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_cbor_reader(reader, ambient_limits(args.get(1))) {
                Ok(reader) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::CBORReader(reader),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        ("core.encoding.cbor", "writer") => {
            let Some(CtValue::Int(file)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "core.encoding.cbor.writer FileWriter",
                    span,
                )));
            };
            let writer = match ambient_stream_take_file_writer(*file) {
                Ok(writer) => writer,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("core.encoding.cbor.writer: {error}"),
                        span,
                    )))
                }
            };
            Some(Ok(match runtime::enc_cbor_writer(writer, ambient_limits(args.get(1))) {
                Ok(writer) => ambient_ok(CtValue::Int(ambient_stream_insert(
                    AmbientStream::CBORWriter(writer),
                ))),
                Err(error) => ambient_failed(ambient_encoding_error(&error)),
            }))
        }
        _ => None,
    }
}

pub(crate) fn ambient_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(
        op,
        "FileReaderReadLine"
            | "FileWriterWriteLine"
            | "FileWriterFlush"
            | "JSONReaderNext"
            | "JSONWriterWrite"
            | "JSONWriterFlush"
            | "JSONWriterFinish"
            | "JSONLReaderNext"
            | "JSONLWriterWrite"
            | "JSONLWriterFlush"
            | "JSONLWriterFinish"
            | "CSVReaderNext"
            | "CSVWriterWrite"
            | "CSVWriterFlush"
            | "CSVWriterFinish"
            | "XMLReaderNext"
            | "XMLWriterWrite"
            | "XMLWriterFlush"
            | "XMLWriterFinish"
            | "CBORReaderNext"
            | "CBORWriterWrite"
            | "CBORWriterFlush"
            | "CBORWriterFinish"
    ) {
        return None;
    }
    let Some(handle) = (match recv {
        CtValue::Int(handle) if *handle > 0 => Some(*handle),
        _ => None,
    }) else {
        return Some(Err(ambient_unsupported("stream handle receiver", span)));
    };
    use std::io::{BufRead, Write};

    let result: Result<CtValue, String> = match op {
        "FileReaderReadLine" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::FileReader(reader) => {
                let mut line = String::new();
                match reader.inner.read_line(&mut line) {
                    Ok(0) => Ok(ambient_ok(CtValue::absent(Type::String))),
                    Ok(_) => {
                        while line.ends_with('\n') || line.ends_with('\r') {
                            line.pop();
                        }
                        Ok(ambient_ok(ambient_ok(CtValue::Str(line))))
                    }
                    Err(error) => Ok(ambient_failed(ambient_io_error(
                        "Read",
                        &reader.path,
                        error.to_string(),
                    ))),
                }
            }
            _ => Err("expected FileReader".to_string()),
        }),
        "FileWriterWriteLine" => {
            let Some(CtValue::Str(line)) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "FileWriter.write_line argument",
                    span,
                )));
            };
            let line = line.clone();
            ambient_stream_with(handle, |stream| match stream {
                AmbientStream::FileWriter(writer) => match writer
                    .inner
                    .write_all(line.as_bytes())
                    .and_then(|_| writer.inner.write_all(b"\n"))
                {
                    Ok(()) => Ok(ambient_ok(CtValue::Unit)),
                    Err(error) => Ok(ambient_failed(ambient_io_error(
                        "Write",
                        &writer.path,
                        error.to_string(),
                    ))),
                },
                _ => Err("expected FileWriter".to_string()),
            })
        }
        "FileWriterFlush" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::FileWriter(writer) => match writer.inner.flush() {
                Ok(()) => Ok(ambient_ok(CtValue::Unit)),
                Err(error) => Ok(ambient_failed(ambient_io_error(
                    "Flush",
                    &writer.path,
                    error.to_string(),
                ))),
            },
            _ => Err("expected FileWriter".to_string()),
        }),
        "JSONReaderNext" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::JSONReader(reader) => Ok(ambient_next(
                runtime::enc_json_reader_next(reader),
                Type::Named("DataEvent".to_string()),
                ambient_event_value,
            )),
            _ => Err("expected JSONReader".to_string()),
        }),
        "JSONWriterWrite" => {
            let Some(value) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "JSONWriter.write argument",
                    span,
                )));
            };
            let event = match ambient_event(value) {
                Ok(event) => event,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("JSONWriter.write: {error}"),
                        span,
                    )))
                }
            };
            ambient_stream_with(handle, |stream| match stream {
                AmbientStream::JSONWriter(writer) => {
                    Ok(ambient_unit(runtime::enc_json_writer_write(writer, event)))
                }
                _ => Err("expected JSONWriter".to_string()),
            })
        }
        "JSONWriterFlush" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::JSONWriter(writer) => {
                Ok(ambient_unit(runtime::enc_json_writer_flush(writer)))
            }
            _ => Err("expected JSONWriter".to_string()),
        }),
        "JSONWriterFinish" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::JSONWriter(writer) => {
                Ok(ambient_unit(runtime::enc_json_writer_finish(writer)))
            }
            _ => Err("expected JSONWriter".to_string()),
        }),
        "JSONLReaderNext" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::JSONLReader(reader) => Ok(ambient_next(
                runtime::enc_jsonl_reader_next(reader),
                Type::Named("DataTree".to_string()),
                ambient_tree_value,
            )),
            _ => Err("expected JSONLReader".to_string()),
        }),
        "JSONLWriterWrite" => {
            let Some(value) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "JSONLWriter.write argument",
                    span,
                )));
            };
            let tree = match ambient_tree(value) {
                Ok(tree) => tree,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("JSONLWriter.write: {error}"),
                        span,
                    )))
                }
            };
            ambient_stream_with(handle, |stream| match stream {
                AmbientStream::JSONLWriter(writer) => {
                    Ok(ambient_unit(runtime::enc_jsonl_writer_write(writer, tree)))
                }
                _ => Err("expected JSONLWriter".to_string()),
            })
        }
        "JSONLWriterFlush" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::JSONLWriter(writer) => {
                Ok(ambient_unit(runtime::enc_jsonl_writer_flush(writer)))
            }
            _ => Err("expected JSONLWriter".to_string()),
        }),
        "JSONLWriterFinish" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::JSONLWriter(writer) => {
                Ok(ambient_unit(runtime::enc_jsonl_writer_finish(writer)))
            }
            _ => Err("expected JSONLWriter".to_string()),
        }),
        "CSVReaderNext" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::CSVReader(reader) => Ok(ambient_next(
                runtime::enc_csv_reader_next(reader),
                Type::List(Box::new(Type::String)),
                |row| CtValue::List(row.into_iter().map(CtValue::Str).collect()),
            )),
            _ => Err("expected CSVReader".to_string()),
        }),
        "CSVWriterWrite" => {
            let Some(CtValue::List(values)) = args.first() else {
                return Some(Err(ambient_unsupported("CSVWriter.write row", span)));
            };
            let row = match values
                .iter()
                .map(|value| match value {
                    CtValue::Str(value) => Ok(value.clone()),
                    _ => Err("CSVWriter.write expects [String]".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(row) => row,
                Err(error) => {
                    return Some(Err(ambient_unsupported(&error, span)));
                }
            };
            ambient_stream_with(handle, |stream| match stream {
                AmbientStream::CSVWriter(writer) => {
                    Ok(ambient_unit(runtime::enc_csv_writer_write(writer, row)))
                }
                _ => Err("expected CSVWriter".to_string()),
            })
        }
        "CSVWriterFlush" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::CSVWriter(writer) => {
                Ok(ambient_unit(runtime::enc_csv_writer_flush(writer)))
            }
            _ => Err("expected CSVWriter".to_string()),
        }),
        "CSVWriterFinish" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::CSVWriter(writer) => {
                Ok(ambient_unit(runtime::enc_csv_writer_finish(writer)))
            }
            _ => Err("expected CSVWriter".to_string()),
        }),
        "XMLReaderNext" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::XMLReader(reader) => Ok(ambient_next(
                runtime::enc_xml_reader_next(reader),
                Type::Named("DataTree".to_string()),
                ambient_tree_value,
            )),
            _ => Err("expected XMLReader".to_string()),
        }),
        "XMLWriterWrite" => {
            let Some(value) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "XMLWriter.write argument",
                    span,
                )));
            };
            let tree = match ambient_tree(value) {
                Ok(tree) => tree,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("XMLWriter.write: {error}"),
                        span,
                    )))
                }
            };
            ambient_stream_with(handle, |stream| match stream {
                AmbientStream::XMLWriter(writer) => {
                    Ok(ambient_unit(runtime::enc_xml_writer_write(writer, tree)))
                }
                _ => Err("expected XMLWriter".to_string()),
            })
        }
        "XMLWriterFlush" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::XMLWriter(writer) => {
                Ok(ambient_unit(runtime::enc_xml_writer_flush(writer)))
            }
            _ => Err("expected XMLWriter".to_string()),
        }),
        "XMLWriterFinish" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::XMLWriter(writer) => {
                Ok(ambient_unit(runtime::enc_xml_writer_finish(writer)))
            }
            _ => Err("expected XMLWriter".to_string()),
        }),
        "CBORReaderNext" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::CBORReader(reader) => Ok(ambient_next(
                runtime::enc_cbor_reader_next(reader),
                Type::Named("DataEvent".to_string()),
                ambient_event_value,
            )),
            _ => Err("expected CBORReader".to_string()),
        }),
        "CBORWriterWrite" => {
            let Some(value) = args.first() else {
                return Some(Err(ambient_unsupported(
                    "CBORWriter.write argument",
                    span,
                )));
            };
            let event = match ambient_event(value) {
                Ok(event) => event,
                Err(error) => {
                    return Some(Err(ambient_unsupported(
                        &format!("CBORWriter.write: {error}"),
                        span,
                    )))
                }
            };
            ambient_stream_with(handle, |stream| match stream {
                AmbientStream::CBORWriter(writer) => {
                    Ok(ambient_unit(runtime::enc_cbor_writer_write(writer, event)))
                }
                _ => Err("expected CBORWriter".to_string()),
            })
        }
        "CBORWriterFlush" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::CBORWriter(writer) => {
                Ok(ambient_unit(runtime::enc_cbor_writer_flush(writer)))
            }
            _ => Err("expected CBORWriter".to_string()),
        }),
        "CBORWriterFinish" => ambient_stream_with(handle, |stream| match stream {
            AmbientStream::CBORWriter(writer) => {
                Ok(ambient_unit(runtime::enc_cbor_writer_finish(writer)))
            }
            _ => Err("expected CBORWriter".to_string()),
        }),
        _ => unreachable!("stream op filtered above"),
    };
    Some(result.map_err(|error| {
        ambient_unsupported(&format!("stream handle {op}: {error}"), span)
    }))
}

// ── Handle tables (1-based) ──────────────────────────────────────────────────

pub(crate) enum FileReaderSlot {
    Live(runtime::JetFileReader),
    Taken,
}
pub(crate) enum FileWriterSlot {
    Live(runtime::JetFileWriter),
    Taken,
}

macro_rules! codec_slots {
    ($($name:ident => $ty:ty),* $(,)?) => {
        $(
            pub(crate) enum $name {
                Live($ty),
                Taken,
            }
        )*
    };
}

codec_slots! {
    JSONReaderSlot => runtime::jet_std::JSONReader,
    JSONWriterSlot => runtime::jet_std::JSONWriter,
    JsonlReaderSlot => runtime::jet_std::JSONLReader,
    JsonlWriterSlot => runtime::jet_std::JSONLWriter,
    CSVReaderSlot => runtime::jet_std::CSVReader,
    CSVWriterSlot => runtime::jet_std::CSVWriter,
    XmlReaderSlot => runtime::jet_std::XMLReader,
    XmlWriterSlot => runtime::jet_std::XMLWriter,
    CBORReaderSlot => runtime::jet_std::CBORReader,
    CBORWriterSlot => runtime::jet_std::CBORWriter,
}

fn push_ok_handle(handle: i64) -> i64 {
    result_ok(handle as u64)
}

fn take_file_writer(handle: i64) -> Result<runtime::JetFileWriter, String> {
    let mut out: Option<Result<runtime::JetFileWriter, String>> = None;
    Concurrency::with_runtime_mut(|rt| {
        let idx = match (handle as usize).checked_sub(1) {
            Some(i) => i,
            None => {
                out = Some(Err("bad FileWriter".into()));
                return;
            }
        };
        out = Some(match rt.file_writers.get_mut(idx) {
            Some(FileWriterSlot::Live(_)) => {
                let slot = std::mem::replace(&mut rt.file_writers[idx], FileWriterSlot::Taken);
                match slot {
                    FileWriterSlot::Live(w) => Ok(w),
                    FileWriterSlot::Taken => Err("FileWriter already moved".into()),
                }
            }
            _ => Err("bad FileWriter".into()),
        });
    });
    out.unwrap_or_else(|| Err("no active JIT runtime".into()))
}

fn take_file_reader(handle: i64) -> Result<runtime::JetFileReader, String> {
    let mut out: Option<Result<runtime::JetFileReader, String>> = None;
    Concurrency::with_runtime_mut(|rt| {
        let idx = match (handle as usize).checked_sub(1) {
            Some(i) => i,
            None => {
                out = Some(Err("bad FileReader".into()));
                return;
            }
        };
        out = Some(match rt.file_readers.get_mut(idx) {
            Some(FileReaderSlot::Live(_)) => {
                let slot = std::mem::replace(&mut rt.file_readers[idx], FileReaderSlot::Taken);
                match slot {
                    FileReaderSlot::Live(r) => Ok(r),
                    FileReaderSlot::Taken => Err("FileReader already moved".into()),
                }
            }
            _ => Err("bad FileReader".into()),
        });
    });
    out.unwrap_or_else(|| Err("no active JIT runtime".into()))
}

/// Drop a FileWriter handle (BufWriter Drop flushes). Used by `close` / resource cleanup.
pub(crate) extern "C" fn jet_jit_file_writer_close(handle: i64) {
    let _ = take_file_writer(handle);
}

/// Drop a FileReader handle.
pub(crate) extern "C" fn jet_jit_file_reader_close(handle: i64) {
    let _ = take_file_reader(handle);
}

fn read_limits(handle: i64) -> runtime::jet_std::EncodingLimits {
    let mut lim = runtime::jet_std::EncodingLimits::safe();
    Concurrency::with_runtime_mut(|rt| {
        if handle <= 0 {
            return;
        }
        let get = |i| rt.heap.record_get_int(handle, i).unwrap_or(0);
        let total = get(3);
        lim = runtime::jet_std::EncodingLimits {
            buffer_bytes: get(0),
            max_depth: get(1),
            max_item_bytes: get(2),
            max_total_bytes: if total == 0 { Err(JetAbsent) } else { Ok(total) },
            max_expansion_depth: get(4),
            max_expansion_bytes: get(5),
        };
    });
    lim
}


fn to_stream_tree(tree: &super::Encoding::json_rt::DataTree) -> runtime::jet_std::DataTree {
    match tree {
        super::Encoding::json_rt::DataTree::Null => runtime::jet_std::DataTree::Null,
        super::Encoding::json_rt::DataTree::Bool(b) => runtime::jet_std::DataTree::Bool(*b),
        super::Encoding::json_rt::DataTree::Int(n) => runtime::jet_std::DataTree::Int(*n),
        super::Encoding::json_rt::DataTree::Float(f) => runtime::jet_std::DataTree::Float(*f),
        super::Encoding::json_rt::DataTree::Text(s) => runtime::jet_std::DataTree::Text(s.clone()),
        super::Encoding::json_rt::DataTree::Bytes(b) => runtime::jet_std::DataTree::Bytes(b.clone()),
        super::Encoding::json_rt::DataTree::Array(items) => {
            runtime::jet_std::DataTree::Array(items.iter().map(to_stream_tree).collect())
        }
        super::Encoding::json_rt::DataTree::Object(entries) => runtime::jet_std::DataTree::Object(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), to_stream_tree(v)))
                .collect(),
        ),
    }
}

fn result_err_encoding(error: &runtime::jet_std::EncodingError) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let format = match error.format {
            runtime::jet_std::EncodingFormat::JSON => 0,
            runtime::jet_std::EncodingFormat::JSONL => 1,
            runtime::jet_std::EncodingFormat::CSV => 2,
            runtime::jet_std::EncodingFormat::XML => 3,
            runtime::jet_std::EncodingFormat::CBOR => 4,
        };
        let kind = match error.kind {
            runtime::jet_std::EncodingErrorKind::Syntax => 0,
            runtime::jet_std::EncodingErrorKind::Truncated => 1,
            runtime::jet_std::EncodingErrorKind::Unsupported => 2,
            runtime::jet_std::EncodingErrorKind::Limit => 3,
            runtime::jet_std::EncodingErrorKind::IO => 4,
            runtime::jet_std::EncodingErrorKind::State => 5,
        };
        let h = rt.heap.alloc_record(8);
        let _ = rt.heap.record_set_int(h, 0, format);
        let _ = rt.heap.record_set_int(h, 1, kind);
        let _ = rt.heap.record_set_int(h, 2, error.byte_offset);
        let _ = rt
            .heap
            .record_set_int(h, 3, error.line.map(|line| line + 1).unwrap_or(0));
        let _ = rt
            .heap
            .record_set_int(h, 4, error.column.map(|column| column + 1).unwrap_or(0));
        let path = rt.heap.alloc_string(error.path.clone());
        let _ = rt.heap.record_set_string(h, 5, path);
        let reason = rt.heap.alloc_string(error.reason.clone());
        let _ = rt.heap.record_set_string(h, 6, reason);
        let _ = rt.heap.record_set_int(h, 7, 0);
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: h as u64,
        });
        rt.results.len() as i64
    })
}

/// D-JSONCANON1=A edition-2027 whole-value JCS — same `jet_enc_json_canonical`.
pub(crate) fn json_canonical_checked(tree: i64, limits: i64) -> i64 {
    let Some(tree) = read_datatree(tree) else {
        return result_err_msg("bad DataTree");
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_json_canonical(&to_stream_tree(&tree), &lim) {
        Ok(text) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text));
            result_ok(sid as u64)
        }
        Err(error) => result_err_encoding(&error),
    }
}

fn from_stream_tree(tree: &runtime::jet_std::DataTree) -> super::Encoding::json_rt::DataTree {
    match tree {
        runtime::jet_std::DataTree::Null => super::Encoding::json_rt::DataTree::Null,
        runtime::jet_std::DataTree::Bool(b) => super::Encoding::json_rt::DataTree::Bool(*b),
        runtime::jet_std::DataTree::Int(n) => super::Encoding::json_rt::DataTree::Int(*n),
        runtime::jet_std::DataTree::Float(f) => super::Encoding::json_rt::DataTree::Float(*f),
        runtime::jet_std::DataTree::Text(s) => super::Encoding::json_rt::DataTree::Text(s.clone()),
        runtime::jet_std::DataTree::Bytes(b) => super::Encoding::json_rt::DataTree::Bytes(b.clone()),
        runtime::jet_std::DataTree::Array(items) => {
            super::Encoding::json_rt::DataTree::Array(items.iter().map(from_stream_tree).collect())
        }
        runtime::jet_std::DataTree::Object(entries) => super::Encoding::json_rt::DataTree::Object(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), from_stream_tree(v)))
                .collect(),
        ),
    }
}

/// JIT DataEvent ABI (types_meta order) → stream DataEvent.
fn read_data_event(packed: i64) -> Result<runtime::jet_std::DataEvent, String> {
    // Float payloads are heap records [disc, f64].
    let (disc, payload_bits, text, bytes, float_val) = Concurrency::with_runtime_mut(|rt| {
        // Heuristic: if packed looks like a small disc (0..15) or packed scalar,
        // use packed form. Float always uses heap record (handle >= 1 with 2 fields).
        if packed >= 1 {
            if let Some(disc) = rt.heap.record_get_int(packed, 0) {
                if (5..=10).contains(&disc) || disc == 7 {
                    if disc == 7 {
                        let f = rt.heap.record_get_float(packed, 1).unwrap_or(0.0);
                        return (disc, 0i64, None, None, Some(f));
                    }
                }
            }
        }
        let disc = packed & 0xff;
        let payload = packed >> 8;
        match disc {
            8 | 10 => {
                let s = rt.heap.clone_string(payload).unwrap_or_default();
                (disc, payload, Some(s), None, None)
            }
            9 => {
                let len = rt.heap.list_len(payload).unwrap_or(0);
                let mut out = Vec::with_capacity(len as usize);
                for i in 0..len {
                    out.push(rt.heap.list_get_int(payload, i).unwrap_or(0) as u8);
                }
                (disc, payload, None, Some(out), None)
            }
            _ => (disc, payload, None, None, None),
        }
    });
    Ok(match disc {
        0 => runtime::jet_std::DataEvent::Null,
        1 => runtime::jet_std::DataEvent::ArrayStart,
        2 => runtime::jet_std::DataEvent::ArrayEnd,
        3 => runtime::jet_std::DataEvent::ObjectStart,
        4 => runtime::jet_std::DataEvent::ObjectEnd,
        5 => runtime::jet_std::DataEvent::Bool(payload_bits != 0),
        6 => runtime::jet_std::DataEvent::Int(payload_bits),
        7 => runtime::jet_std::DataEvent::Float(float_val.unwrap_or(0.0)),
        8 => runtime::jet_std::DataEvent::Text(text.unwrap_or_default()),
        9 => runtime::jet_std::DataEvent::Bytes(bytes.unwrap_or_default()),
        10 => runtime::jet_std::DataEvent::Key(text.unwrap_or_default()),
        _ => return Err(format!("bad DataEvent disc {disc}")),
    })
}

fn pack_data_event(ev: runtime::jet_std::DataEvent) -> i64 {
    use runtime::jet_std::DataEvent::*;
    match ev {
        Null => 0,
        ArrayStart => 1,
        ArrayEnd => 2,
        ObjectStart => 3,
        ObjectEnd => 4,
        Bool(b) => 5 | ((i64::from(b)) << 8),
        Int(n) => 6 | (n << 8),
        Float(f) => Concurrency::with_runtime_mut(|rt| {
            let h = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(h, 0, 7);
            let _ = rt.heap.record_set_float(h, 1, f);
            h
        }),
        Text(s) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
            8 | (sid << 8)
        }
        Bytes(b) => {
            let list = Concurrency::with_runtime_mut(|rt| {
                let list = rt.heap.alloc_empty_list();
                for byte in b {
                    let _ = rt.heap.list_push_int(list, byte as i64);
                }
                list
            });
            9 | (list << 8)
        }
        Key(s) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
            10 | (sid << 8)
        }
    }
}

fn option_bits(opt: Option<i64>) -> u64 {
    match opt {
        None => 0,
        Some(v) => (v as u64).wrapping_add(1),
    }
}

// ── core.files create / open ─────────────────────────────────────────────────

pub(crate) extern "C" fn jet_jit_fs_create(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::File::create(&p) {
        Ok(f) => {
            let w = runtime::JetFileWriter {
                inner: std::io::BufWriter::new(f),
                path: p,
            };
            let h = Concurrency::with_runtime_mut(|rt| {
                rt.file_writers.push(FileWriterSlot::Live(w));
                rt.file_writers.len() as i64
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&format!("create {p}: {e}")),
    }
}

pub(crate) extern "C" fn jet_jit_fs_open(path: i64) -> i64 {
    let p = clone_string(path);
    match std::fs::File::open(&p) {
        Ok(f) => {
            let r = runtime::JetFileReader {
                inner: std::io::BufReader::new(f),
                path: p,
            };
            let h = Concurrency::with_runtime_mut(|rt| {
                rt.file_readers.push(FileReaderSlot::Live(r));
                rt.file_readers.len() as i64
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&format!("open {p}: {e}")),
    }
}

// ── writers / readers ────────────────────────────────────────────────────────

macro_rules! push_codec {
    ($rt:ident, $field:ident, $slot:ident, $val:expr) => {{
        $rt.$field.push($slot::Live($val));
        $rt.$field.len() as i64
    }};
}

pub(crate) extern "C" fn jet_jit_json_writer(file: i64, limits: i64, canonical: i64) -> i64 {
    let w = match take_file_writer(file) {
        Ok(w) => w,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_json_writer(w, lim, canonical != 0) {
        Ok(writer) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, json_writers, JSONWriterSlot, writer)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_json_reader(file: i64, limits: i64) -> i64 {
    let r = match take_file_reader(file) {
        Ok(r) => r,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_json_reader(r, lim) {
        Ok(reader) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, json_readers, JSONReaderSlot, reader)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_jsonl_writer(file: i64, limits: i64) -> i64 {
    let w = match take_file_writer(file) {
        Ok(w) => w,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_jsonl_writer(w, lim) {
        Ok(writer) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, jsonl_writers, JsonlWriterSlot, writer)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_jsonl_reader(file: i64, limits: i64) -> i64 {
    let r = match take_file_reader(file) {
        Ok(r) => r,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_jsonl_reader(r, lim) {
        Ok(reader) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, jsonl_readers, JsonlReaderSlot, reader)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_csv_writer(file: i64, limits: i64) -> i64 {
    let w = match take_file_writer(file) {
        Ok(w) => w,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_csv_writer(w, lim) {
        Ok(writer) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, csv_writers, CSVWriterSlot, writer)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_csv_reader(file: i64, limits: i64) -> i64 {
    let r = match take_file_reader(file) {
        Ok(r) => r,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_csv_reader(r, lim) {
        Ok(reader) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, csv_readers, CSVReaderSlot, reader)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_writer(file: i64, limits: i64) -> i64 {
    let w = match take_file_writer(file) {
        Ok(w) => w,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_cbor_writer(w, lim) {
        Ok(writer) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, cbor_writers, CBORWriterSlot, writer)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_reader(file: i64, limits: i64) -> i64 {
    let r = match take_file_reader(file) {
        Ok(r) => r,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    match runtime::enc_cbor_reader(r, lim) {
        Ok(reader) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, cbor_readers, CBORReaderSlot, reader)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_xml_writer(file: i64, limits: i64) -> i64 {
    let w = match take_file_writer(file) {
        Ok(w) => w,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    let xml = runtime::jet_std::XMLRenderOptions::safe();
    match runtime::enc_xml_writer(w, lim, xml) {
        Ok(writer) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, xml_writers, XmlWriterSlot, writer)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

pub(crate) extern "C" fn jet_jit_xml_reader(file: i64, limits: i64) -> i64 {
    let r = match take_file_reader(file) {
        Ok(r) => r,
        Err(e) => return result_err_msg(&e),
    };
    let lim = if limits == 0 {
        runtime::jet_std::EncodingLimits::safe()
    } else {
        read_limits(limits)
    };
    let xml = runtime::jet_std::XMLParseOptions::safe();
    match runtime::enc_xml_reader(r, lim, xml) {
        Ok(reader) => {
            let h = Concurrency::with_runtime_mut(|rt| {
                push_codec!(rt, xml_readers, XmlReaderSlot, reader)
            });
            push_ok_handle(h)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

// ── handle methods ───────────────────────────────────────────────────────────

macro_rules! with_writer {
    ($field:ident, $slot:ident, $handle:expr, $body:expr) => {{
        Concurrency::with_runtime_mut(|rt| {
            let idx = ($handle as usize).checked_sub(1)?;
            match rt.$field.get_mut(idx)? {
                $slot::Live(w) => Some($body(w)),
                $slot::Taken => None,
            }
        })
    }};
}

pub(crate) extern "C" fn jet_jit_json_writer_write(handle: i64, event: i64) -> i64 {
    let ev = match read_data_event(event) {
        Ok(e) => e,
        Err(e) => return result_err_msg(&e),
    };
    match with_writer!(json_writers, JSONWriterSlot, handle, |w| {
        runtime::enc_json_writer_write(w, ev.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_json_writer_flush(handle: i64) -> i64 {
    match with_writer!(json_writers, JSONWriterSlot, handle, |w| {
        runtime::enc_json_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_json_writer_finish(handle: i64) -> i64 {
    match with_writer!(json_writers, JSONWriterSlot, handle, |w| {
        runtime::enc_json_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_json_reader_next(handle: i64) -> i64 {
    match with_writer!(json_readers, JSONReaderSlot, handle, |r| {
        runtime::enc_json_reader_next(r)
    }) {
        Some(Ok(None)) => push_ok_handle(0),
        Some(Ok(Some(ev))) => {
            let bits = pack_data_event(ev);
            push_ok_handle(option_bits(Some(bits)) as i64)
        }
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONReader"),
    }
}

pub(crate) extern "C" fn jet_jit_jsonl_writer_write(handle: i64, tree: i64) -> i64 {
    let Some(dt) = read_datatree(tree) else {
        return result_err_msg("bad DataTree");
    };
    let st = to_stream_tree(&dt);
    match with_writer!(jsonl_writers, JsonlWriterSlot, handle, |w| {
        runtime::enc_jsonl_writer_write(w, st.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONLWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_jsonl_writer_flush(handle: i64) -> i64 {
    match with_writer!(jsonl_writers, JsonlWriterSlot, handle, |w| {
        runtime::enc_jsonl_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONLWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_jsonl_writer_finish(handle: i64) -> i64 {
    match with_writer!(jsonl_writers, JsonlWriterSlot, handle, |w| {
        runtime::enc_jsonl_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONLWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_jsonl_reader_next(handle: i64) -> i64 {
    match with_writer!(jsonl_readers, JsonlReaderSlot, handle, |r| {
        runtime::enc_jsonl_reader_next(r)
    }) {
        Some(Ok(None)) => push_ok_handle(0),
        Some(Ok(Some(tree))) => {
            let h = alloc_datatree(&from_stream_tree(&tree));
            push_ok_handle(option_bits(Some(h)) as i64)
        }
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONLReader"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_writer_write(handle: i64, row: i64) -> i64 {
    let cells = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(row).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(row, i).unwrap_or(0);
            out.push(rt.heap.clone_string(sid).unwrap_or_default());
        }
        out
    });
    match with_writer!(csv_writers, CSVWriterSlot, handle, |w| {
        runtime::enc_csv_writer_write(w, cells.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_writer_flush(handle: i64) -> i64 {
    match with_writer!(csv_writers, CSVWriterSlot, handle, |w| {
        runtime::enc_csv_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_writer_finish(handle: i64) -> i64 {
    match with_writer!(csv_writers, CSVWriterSlot, handle, |w| {
        runtime::enc_csv_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_reader_next(handle: i64) -> i64 {
    match with_writer!(csv_readers, CSVReaderSlot, handle, |r| {
        runtime::enc_csv_reader_next(r)
    }) {
        Some(Ok(None)) => push_ok_handle(0),
        Some(Ok(Some(row))) => {
            let list = Concurrency::with_runtime_mut(|rt| {
                let list = rt.heap.alloc_empty_list();
                for cell in row {
                    let sid = rt.heap.alloc_string(cell);
                    let _ = rt.heap.list_push_int(list, sid);
                }
                list
            });
            push_ok_handle(option_bits(Some(list)) as i64)
        }
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVReader"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_writer_write(handle: i64, event: i64) -> i64 {
    let ev = match read_data_event(event) {
        Ok(e) => e,
        Err(e) => return result_err_msg(&e),
    };
    match with_writer!(cbor_writers, CBORWriterSlot, handle, |w| {
        runtime::enc_cbor_writer_write(w, ev.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_writer_flush(handle: i64) -> i64 {
    match with_writer!(cbor_writers, CBORWriterSlot, handle, |w| {
        runtime::enc_cbor_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_writer_finish(handle: i64) -> i64 {
    match with_writer!(cbor_writers, CBORWriterSlot, handle, |w| {
        runtime::enc_cbor_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_reader_next(handle: i64) -> i64 {
    match with_writer!(cbor_readers, CBORReaderSlot, handle, |r| {
        runtime::enc_cbor_reader_next(r)
    }) {
        Some(Ok(None)) => push_ok_handle(0),
        Some(Ok(Some(ev))) => {
            let bits = pack_data_event(ev);
            push_ok_handle(option_bits(Some(bits)) as i64)
        }
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORReader"),
    }
}

pub(crate) extern "C" fn jet_jit_xml_writer_write(handle: i64, tree: i64) -> i64 {
    let Some(dt) = read_datatree(tree) else {
        return result_err_msg("bad DataTree");
    };
    let st = to_stream_tree(&dt);
    match with_writer!(xml_writers, XmlWriterSlot, handle, |w| {
        runtime::enc_xml_writer_write(w, st.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad XMLWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_xml_writer_flush(handle: i64) -> i64 {
    match with_writer!(xml_writers, XmlWriterSlot, handle, |w| {
        runtime::enc_xml_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad XMLWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_xml_writer_finish(handle: i64) -> i64 {
    match with_writer!(xml_writers, XmlWriterSlot, handle, |w| {
        runtime::enc_xml_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad XMLWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_xml_reader_next(handle: i64) -> i64 {
    match with_writer!(xml_readers, XmlReaderSlot, handle, |r| {
        runtime::enc_xml_reader_next(r)
    }) {
        Some(Ok(None)) => push_ok_handle(0),
        Some(Ok(Some(tree))) => {
            let h = alloc_datatree(&from_stream_tree(&tree));
            push_ok_handle(option_bits(Some(h)) as i64)
        }
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad XMLReader"),
    }
}

// ── symbol registration (#1633: one host_fns! listing) ───────────────────────

host_fns! {
    struct StreamHostFns;
    register: register_stream_symbols;
    declare: declare_stream_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));
        // json.writer(file, limits, canonical:bool as i8) — use i64 for all.
        let mut sig_ternary = Signature::new(cc);
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.returns.push(AbiParam::new(types::I64));
    }
    fs_create: "jet_jit_fs_create" => jet_jit_fs_create: sig_unary;
    fs_open: "jet_jit_fs_open" => jet_jit_fs_open: sig_unary;
    json_writer: "jet_jit_json_writer" => jet_jit_json_writer: sig_ternary;
    json_reader: "jet_jit_json_reader" => jet_jit_json_reader: sig_binary;
    jsonl_writer: "jet_jit_jsonl_writer" => jet_jit_jsonl_writer: sig_binary;
    jsonl_reader: "jet_jit_jsonl_reader" => jet_jit_jsonl_reader: sig_binary;
    csv_writer: "jet_jit_csv_writer" => jet_jit_csv_writer: sig_binary;
    csv_reader: "jet_jit_csv_reader" => jet_jit_csv_reader: sig_binary;
    cbor_writer: "jet_jit_cbor_writer" => jet_jit_cbor_writer: sig_binary;
    cbor_reader: "jet_jit_cbor_reader" => jet_jit_cbor_reader: sig_binary;
    xml_writer: "jet_jit_xml_writer" => jet_jit_xml_writer: sig_binary;
    xml_reader: "jet_jit_xml_reader" => jet_jit_xml_reader: sig_binary;
    json_writer_write: "jet_jit_json_writer_write" => jet_jit_json_writer_write: sig_binary;
    json_writer_flush: "jet_jit_json_writer_flush" => jet_jit_json_writer_flush: sig_unary;
    json_writer_finish: "jet_jit_json_writer_finish" => jet_jit_json_writer_finish: sig_unary;
    json_reader_next: "jet_jit_json_reader_next" => jet_jit_json_reader_next: sig_unary;
    jsonl_writer_write: "jet_jit_jsonl_writer_write" => jet_jit_jsonl_writer_write: sig_binary;
    jsonl_writer_flush: "jet_jit_jsonl_writer_flush" => jet_jit_jsonl_writer_flush: sig_unary;
    jsonl_writer_finish: "jet_jit_jsonl_writer_finish" => jet_jit_jsonl_writer_finish: sig_unary;
    jsonl_reader_next: "jet_jit_jsonl_reader_next" => jet_jit_jsonl_reader_next: sig_unary;
    csv_writer_write: "jet_jit_csv_writer_write" => jet_jit_csv_writer_write: sig_binary;
    csv_writer_flush: "jet_jit_csv_writer_flush" => jet_jit_csv_writer_flush: sig_unary;
    csv_writer_finish: "jet_jit_csv_writer_finish" => jet_jit_csv_writer_finish: sig_unary;
    csv_reader_next: "jet_jit_csv_reader_next" => jet_jit_csv_reader_next: sig_unary;
    cbor_writer_write: "jet_jit_cbor_writer_write" => jet_jit_cbor_writer_write: sig_binary;
    cbor_writer_flush: "jet_jit_cbor_writer_flush" => jet_jit_cbor_writer_flush: sig_unary;
    cbor_writer_finish: "jet_jit_cbor_writer_finish" => jet_jit_cbor_writer_finish: sig_unary;
    cbor_reader_next: "jet_jit_cbor_reader_next" => jet_jit_cbor_reader_next: sig_unary;
    xml_writer_write: "jet_jit_xml_writer_write" => jet_jit_xml_writer_write: sig_binary;
    xml_writer_flush: "jet_jit_xml_writer_flush" => jet_jit_xml_writer_flush: sig_unary;
    xml_writer_finish: "jet_jit_xml_writer_finish" => jet_jit_xml_writer_finish: sig_unary;
    xml_reader_next: "jet_jit_xml_reader_next" => jet_jit_xml_reader_next: sig_unary;
}
