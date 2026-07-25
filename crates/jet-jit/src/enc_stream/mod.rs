//! Encoding stream hosts (#729) — `include!` canonical EncodingStream + HostileIo.
//! File create/open plus JSON/JSONL/CSV/CBOR/XML reader/writer handles.

use super::Concurrency;
use super::Encoding::{alloc_datatree, clone_heap_string, read_datatree, result_err_msg, result_ok_bits};

/// Canonical stream runtime (jet_std types + EncodingStream algorithm).
#[allow(dead_code, unused_imports, unused_variables, clippy::all)]
pub(crate) mod runtime {
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
        use super::{JetDisplay, JetShow};

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct EncodingLimits {
            pub buffer_bytes: i64,
            pub max_depth: i64,
            pub max_item_bytes: i64,
            pub max_total_bytes: Option<i64>,
            pub max_expansion_depth: i64,
            pub max_expansion_bytes: i64,
        }
        impl EncodingLimits {
            pub fn safe() -> Self {
                Self {
                    buffer_bytes: 65536,
                    max_depth: 256,
                    max_item_bytes: 16777216,
                    max_total_bytes: None,
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
            pub os_code: Option<i64>,
            pub message: String,
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct EncodingError {
            pub format: EncodingFormat,
            pub kind: EncodingErrorKind,
            pub byte_offset: i64,
            pub line: Option<i64>,
            pub column: Option<i64>,
            pub path: String,
            pub reason: String,
            pub cause: Option<EncodingCause>,
        }
        impl EncodingError {
            pub fn cause(&self) -> Option<EncodingCause> {
                self.cause.clone()
            }
            fn display_text(&self) -> String {
                let mut out = format!("{:?} {:?} at byte {}", self.format, self.kind, self.byte_offset);
                if let Some(line) = self.line {
                    out.push_str(&format!(", line {line}"));
                }
                if let Some(column) = self.column {
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

        include!("jet_bigint_snip.rs");

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
            pub line: Option<i64>,
            pub column: Option<i64>,
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
            pub(crate) frames: Vec<super::JetJsonReadFrame>,
            pub(crate) root_started: bool,
            pub(crate) root_done: bool,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) record_mode: bool,
            pub(crate) allocation_budget: Option<super::JetJsonAllocationBudget>,
        }
        pub struct JSONWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) frames: Vec<super::JetJsonWriteFrame>,
            pub(crate) root_written: bool,
            pub(crate) finished: bool,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) canonical: bool,
            pub(crate) canonical_frames: Vec<super::JetJsonCanonicalFrame>,
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
            pub(crate) allocation: super::JetJsonAllocationBudget,
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
            pub(crate) frames: Vec<super::JetCborReadFrame>,
            pub(crate) retained: usize,
            pub(crate) workspace: usize,
            pub(crate) allocation: super::JetJsonAllocationBudget,
        }
        pub struct CBORWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) frames: Vec<super::JetCborWriteFrame>,
            pub(crate) root_written: bool,
            pub(crate) finished: bool,
            pub(crate) retained: usize,
            pub(crate) workspace: usize,
            pub(crate) allocation: super::JetJsonAllocationBudget,
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

    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/EncodingHostileIo.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/EncodingStream.rs");

    pub(crate) fn enc_json_writer(
        output: JetFileWriter,
        limits: jet_std::EncodingLimits,
        canonical: bool,
    ) -> Result<jet_std::JSONWriter, jet_std::EncodingError> {
        jet_enc_json_writer(output, limits, canonical)
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
        jet_enc_json_reader_next(reader)
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
        jet_enc_jsonl_reader_next(reader)
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
        jet_enc_csv_reader_next(reader)
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
        jet_enc_cbor_reader_next(reader)
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
        jet_enc_xml_reader_next(reader)
    }
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
    JsonReaderSlot => runtime::jet_std::JSONReader,
    JsonWriterSlot => runtime::jet_std::JSONWriter,
    JsonlReaderSlot => runtime::jet_std::JSONLReader,
    JsonlWriterSlot => runtime::jet_std::JSONLWriter,
    CsvReaderSlot => runtime::jet_std::CSVReader,
    CsvWriterSlot => runtime::jet_std::CSVWriter,
    XmlReaderSlot => runtime::jet_std::XMLReader,
    XmlWriterSlot => runtime::jet_std::XMLWriter,
    CborReaderSlot => runtime::jet_std::CBORReader,
    CborWriterSlot => runtime::jet_std::CBORWriter,
}

fn push_ok_handle(handle: i64) -> i64 {
    result_ok_bits(handle as u64)
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
            max_total_bytes: if total == 0 { None } else { Some(total) },
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
    let p = clone_heap_string(path);
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
    let p = clone_heap_string(path);
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
                push_codec!(rt, json_writers, JsonWriterSlot, writer)
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
                push_codec!(rt, json_readers, JsonReaderSlot, reader)
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
                push_codec!(rt, csv_writers, CsvWriterSlot, writer)
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
                push_codec!(rt, csv_readers, CsvReaderSlot, reader)
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
                push_codec!(rt, cbor_writers, CborWriterSlot, writer)
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
                push_codec!(rt, cbor_readers, CborReaderSlot, reader)
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
    match with_writer!(json_writers, JsonWriterSlot, handle, |w| {
        runtime::enc_json_writer_write(w, ev.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_json_writer_flush(handle: i64) -> i64 {
    match with_writer!(json_writers, JsonWriterSlot, handle, |w| {
        runtime::enc_json_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_json_writer_finish(handle: i64) -> i64 {
    match with_writer!(json_writers, JsonWriterSlot, handle, |w| {
        runtime::enc_json_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad JSONWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_json_reader_next(handle: i64) -> i64 {
    match with_writer!(json_readers, JsonReaderSlot, handle, |r| {
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
    match with_writer!(csv_writers, CsvWriterSlot, handle, |w| {
        runtime::enc_csv_writer_write(w, cells.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_writer_flush(handle: i64) -> i64 {
    match with_writer!(csv_writers, CsvWriterSlot, handle, |w| {
        runtime::enc_csv_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_writer_finish(handle: i64) -> i64 {
    match with_writer!(csv_writers, CsvWriterSlot, handle, |w| {
        runtime::enc_csv_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CSVWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_csv_reader_next(handle: i64) -> i64 {
    match with_writer!(csv_readers, CsvReaderSlot, handle, |r| {
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
    match with_writer!(cbor_writers, CborWriterSlot, handle, |w| {
        runtime::enc_cbor_writer_write(w, ev.clone())
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_writer_flush(handle: i64) -> i64 {
    match with_writer!(cbor_writers, CborWriterSlot, handle, |w| {
        runtime::enc_cbor_writer_flush(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_writer_finish(handle: i64) -> i64 {
    match with_writer!(cbor_writers, CborWriterSlot, handle, |w| {
        runtime::enc_cbor_writer_finish(w)
    }) {
        Some(Ok(())) => push_ok_handle(0),
        Some(Err(e)) => result_err_msg(&e.to_string()),
        None => result_err_msg("bad CBORWriter"),
    }
}

pub(crate) extern "C" fn jet_jit_cbor_reader_next(handle: i64) -> i64 {
    match with_writer!(cbor_readers, CborReaderSlot, handle, |r| {
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

// ── symbol registration ──────────────────────────────────────────────────────

pub(crate) fn register_stream_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_fs_create", jet_jit_fs_create as *const u8);
    builder.symbol("jet_jit_fs_open", jet_jit_fs_open as *const u8);
    builder.symbol("jet_jit_json_writer", jet_jit_json_writer as *const u8);
    builder.symbol("jet_jit_json_reader", jet_jit_json_reader as *const u8);
    builder.symbol("jet_jit_jsonl_writer", jet_jit_jsonl_writer as *const u8);
    builder.symbol("jet_jit_jsonl_reader", jet_jit_jsonl_reader as *const u8);
    builder.symbol("jet_jit_csv_writer", jet_jit_csv_writer as *const u8);
    builder.symbol("jet_jit_csv_reader", jet_jit_csv_reader as *const u8);
    builder.symbol("jet_jit_cbor_writer", jet_jit_cbor_writer as *const u8);
    builder.symbol("jet_jit_cbor_reader", jet_jit_cbor_reader as *const u8);
    builder.symbol("jet_jit_xml_writer", jet_jit_xml_writer as *const u8);
    builder.symbol("jet_jit_xml_reader", jet_jit_xml_reader as *const u8);
    builder.symbol("jet_jit_json_writer_write", jet_jit_json_writer_write as *const u8);
    builder.symbol("jet_jit_json_writer_flush", jet_jit_json_writer_flush as *const u8);
    builder.symbol("jet_jit_json_writer_finish", jet_jit_json_writer_finish as *const u8);
    builder.symbol("jet_jit_json_reader_next", jet_jit_json_reader_next as *const u8);
    builder.symbol("jet_jit_jsonl_writer_write", jet_jit_jsonl_writer_write as *const u8);
    builder.symbol("jet_jit_jsonl_writer_flush", jet_jit_jsonl_writer_flush as *const u8);
    builder.symbol("jet_jit_jsonl_writer_finish", jet_jit_jsonl_writer_finish as *const u8);
    builder.symbol("jet_jit_jsonl_reader_next", jet_jit_jsonl_reader_next as *const u8);
    builder.symbol("jet_jit_csv_writer_write", jet_jit_csv_writer_write as *const u8);
    builder.symbol("jet_jit_csv_writer_flush", jet_jit_csv_writer_flush as *const u8);
    builder.symbol("jet_jit_csv_writer_finish", jet_jit_csv_writer_finish as *const u8);
    builder.symbol("jet_jit_csv_reader_next", jet_jit_csv_reader_next as *const u8);
    builder.symbol("jet_jit_cbor_writer_write", jet_jit_cbor_writer_write as *const u8);
    builder.symbol("jet_jit_cbor_writer_flush", jet_jit_cbor_writer_flush as *const u8);
    builder.symbol("jet_jit_cbor_writer_finish", jet_jit_cbor_writer_finish as *const u8);
    builder.symbol("jet_jit_cbor_reader_next", jet_jit_cbor_reader_next as *const u8);
    builder.symbol("jet_jit_xml_writer_write", jet_jit_xml_writer_write as *const u8);
    builder.symbol("jet_jit_xml_writer_flush", jet_jit_xml_writer_flush as *const u8);
    builder.symbol("jet_jit_xml_writer_finish", jet_jit_xml_writer_finish as *const u8);
    builder.symbol("jet_jit_xml_reader_next", jet_jit_xml_reader_next as *const u8);
}

pub(crate) struct StreamHostFns {
    pub fs_create: cranelift_module::FuncId,
    pub fs_open: cranelift_module::FuncId,
    pub json_writer: cranelift_module::FuncId,
    pub json_reader: cranelift_module::FuncId,
    pub jsonl_writer: cranelift_module::FuncId,
    pub jsonl_reader: cranelift_module::FuncId,
    pub csv_writer: cranelift_module::FuncId,
    pub csv_reader: cranelift_module::FuncId,
    pub cbor_writer: cranelift_module::FuncId,
    pub cbor_reader: cranelift_module::FuncId,
    pub xml_writer: cranelift_module::FuncId,
    pub xml_reader: cranelift_module::FuncId,
    pub json_writer_write: cranelift_module::FuncId,
    pub json_writer_flush: cranelift_module::FuncId,
    pub json_writer_finish: cranelift_module::FuncId,
    pub json_reader_next: cranelift_module::FuncId,
    pub jsonl_writer_write: cranelift_module::FuncId,
    pub jsonl_writer_flush: cranelift_module::FuncId,
    pub jsonl_writer_finish: cranelift_module::FuncId,
    pub jsonl_reader_next: cranelift_module::FuncId,
    pub csv_writer_write: cranelift_module::FuncId,
    pub csv_writer_flush: cranelift_module::FuncId,
    pub csv_writer_finish: cranelift_module::FuncId,
    pub csv_reader_next: cranelift_module::FuncId,
    pub cbor_writer_write: cranelift_module::FuncId,
    pub cbor_writer_flush: cranelift_module::FuncId,
    pub cbor_writer_finish: cranelift_module::FuncId,
    pub cbor_reader_next: cranelift_module::FuncId,
    pub xml_writer_write: cranelift_module::FuncId,
    pub xml_writer_flush: cranelift_module::FuncId,
    pub xml_writer_finish: cranelift_module::FuncId,
    pub xml_reader_next: cranelift_module::FuncId,
}

pub(crate) fn declare_stream_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<StreamHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_unary = Signature::new(cc);
    sig_unary.params.push(AbiParam::new(types::I64));
    sig_unary.returns.push(AbiParam::new(types::I64));
    let mut sig_binary = Signature::new(cc);
    sig_binary.params.push(AbiParam::new(types::I64));
    sig_binary.params.push(AbiParam::new(types::I64));
    sig_binary.returns.push(AbiParam::new(types::I64));
    let mut sig_ternary = Signature::new(cc);
    sig_ternary.params.push(AbiParam::new(types::I64));
    sig_ternary.params.push(AbiParam::new(types::I64));
    sig_ternary.params.push(AbiParam::new(types::I64));
    sig_ternary.returns.push(AbiParam::new(types::I64));
    // json.writer(file, limits, canonical:bool as i8) — use i64 for all.
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(StreamHostFns {
        fs_create: import("jet_jit_fs_create", &sig_unary)?,
        fs_open: import("jet_jit_fs_open", &sig_unary)?,
        json_writer: import("jet_jit_json_writer", &sig_ternary)?,
        json_reader: import("jet_jit_json_reader", &sig_binary)?,
        jsonl_writer: import("jet_jit_jsonl_writer", &sig_binary)?,
        jsonl_reader: import("jet_jit_jsonl_reader", &sig_binary)?,
        csv_writer: import("jet_jit_csv_writer", &sig_binary)?,
        csv_reader: import("jet_jit_csv_reader", &sig_binary)?,
        cbor_writer: import("jet_jit_cbor_writer", &sig_binary)?,
        cbor_reader: import("jet_jit_cbor_reader", &sig_binary)?,
        xml_writer: import("jet_jit_xml_writer", &sig_binary)?,
        xml_reader: import("jet_jit_xml_reader", &sig_binary)?,
        json_writer_write: import("jet_jit_json_writer_write", &sig_binary)?,
        json_writer_flush: import("jet_jit_json_writer_flush", &sig_unary)?,
        json_writer_finish: import("jet_jit_json_writer_finish", &sig_unary)?,
        json_reader_next: import("jet_jit_json_reader_next", &sig_unary)?,
        jsonl_writer_write: import("jet_jit_jsonl_writer_write", &sig_binary)?,
        jsonl_writer_flush: import("jet_jit_jsonl_writer_flush", &sig_unary)?,
        jsonl_writer_finish: import("jet_jit_jsonl_writer_finish", &sig_unary)?,
        jsonl_reader_next: import("jet_jit_jsonl_reader_next", &sig_unary)?,
        csv_writer_write: import("jet_jit_csv_writer_write", &sig_binary)?,
        csv_writer_flush: import("jet_jit_csv_writer_flush", &sig_unary)?,
        csv_writer_finish: import("jet_jit_csv_writer_finish", &sig_unary)?,
        csv_reader_next: import("jet_jit_csv_reader_next", &sig_unary)?,
        cbor_writer_write: import("jet_jit_cbor_writer_write", &sig_binary)?,
        cbor_writer_flush: import("jet_jit_cbor_writer_flush", &sig_unary)?,
        cbor_writer_finish: import("jet_jit_cbor_writer_finish", &sig_unary)?,
        cbor_reader_next: import("jet_jit_cbor_reader_next", &sig_unary)?,
        xml_writer_write: import("jet_jit_xml_writer_write", &sig_binary)?,
        xml_writer_flush: import("jet_jit_xml_writer_flush", &sig_unary)?,
        xml_writer_finish: import("jet_jit_xml_writer_finish", &sig_unary)?,
        xml_reader_next: import("jet_jit_xml_reader_next", &sig_unary)?,
    })
}
