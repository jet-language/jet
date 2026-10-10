//! Encoding stream hosts (#729) — `include!` canonical EncodingStream + HostileIo.
//! File create/open plus JSON/JSONL/CSV/CBOR/XML reader/writer handles.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;

mod fs_ops_kernel {
    use crate::Collections::authority_semantics::JetFileScope;
    include!("../../../jet-codegen/src/Prelude/Core/FSOps.rs");
}
/// FileStream uses the same checked Prelude IOError carrier as process and
/// terminal adapters.  Keeping this alias canonical prevents a file read from
/// becoming an adapter-only string before it reaches the Source cursor.
pub(crate) type SourceIoError =
    crate::ProcessPrelude::process_prelude::jet_std::IOError;

mod stdin_reader {
    use crate::fault_injection::jet_fault_should_fail;

    mod jet_std {
        pub(super) use crate::ProcessPrelude::process_prelude::jet_std::{
            io_error_at, IOError, IOOperation
        };
    }

    include!("../../../jet-codegen/src/Prelude/Core/StdinReader.rs");
}

pub(crate) use stdin_reader::JetStdinReader as SourceStdinReader;
pub(crate) use stdin_reader::jet_stdin_line_raw;

pub(crate) fn source_stdin_reader() -> SourceStdinReader {
    stdin_reader::jet_std_io_stdin()
}

pub(crate) fn source_stdin_next_line(
    reader: &mut SourceStdinReader,
) -> Result<Option<String>, SourceIoError> {
    stdin_reader::jet_std_io_stdin_read_line(reader)
}

mod file_stream {
    use super::runtime::{JetFileReader, JetFileWriter};
    use crate::fault_injection::jet_fault_should_fail;

    mod jet_std {
        pub(super) use crate::ProcessPrelude::process_prelude::jet_std::{
            io_error_at, jet_fs_link_fd_at_raw, jet_fs_open_at, jet_fs_open_nonblocking, jet_fs_stat_file, IOContext, IOError, IOOperation
        };

        pub(super) fn jet_std_files_writer_refusal(path: &str) -> Option<IOError> {
            crate::CoreHost::os_rt::jet_mapped_writer_refusal(path).map(|refusal| {
                IOError::other(
                    IOOperation::Write,
                    Some(refusal.path),
                    refusal.reason,
                )
            })
        }
    }

    fn jet_fs_open(path: &str) -> std::io::Result<std::fs::File> {
        super::fs_ops_kernel::jet_fs_open(path)
    }

    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/FileStream.rs");
}

// #4821 private physical seed shared with AOT. No crypto policy lives here.
pub(crate) mod crypto_file_io {
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/CryptoFileIo.rs");
}

/// Canonical stream runtime (jet_std types + EncodingStream algorithm).
#[allow(unused_imports, unused_variables, clippy::all)]
pub(crate) mod runtime {
    use crate::Time::time_rt::{JetDateTime, JetZone, JetZonedDateTime};
    pub(crate) use jet_foundation::CsvKernel as jet_csv_kernel;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    // Encoding errors retain the crate-wide display/show seams across stream
    // and web adapters, rather than implementing a stream-local trait copy.
    use crate::{JetDisplay, JetShow};

    pub struct JetFileReader {
        pub(crate) inner: std::sync::Mutex<std::io::BufReader<std::fs::File>>,
        pub(crate) path: String
    }
    pub struct JetFileWriter {
        pub(crate) inner: std::sync::Mutex<std::io::BufWriter<std::fs::File>>,
        pub(crate) path: String
    }

    pub mod jet_std {
        use super::{JetDisplay, JetShow};
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;

        pub(crate) use crate::Encoding::json_rt::{
            EncodingCause, EncodingError, EncodingErrorKind, EncodingFormat, EncodingLimits
        };

        impl EncodingError {
            pub fn cause(&self) -> JetOutcome<EncodingCause, JetAbsent> {
                self.cause.clone()
            }
            fn display_text(&self) -> String {
                let mut out = format!(
                    "{:?} {:?} at byte {}",
                    self.format, self.kind, self.byte_offset
                );
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
            // D-JSON-EXACTNUM1: internal exact-number carrier, mirrored from
            // EncodingTypes.rs so the JIT host and AOT agree on the variant set.
            Number(String),
            Text(String),
            Bytes(Vec<u8>),
            DateTime(crate::Time::time_rt::JetZonedDateTime),
            ArrayStart,
            ArrayEnd,
            ObjectStart,
            Key(String),
            ObjectEnd
        }

        pub(crate) use jet_foundation::Dyn::Dyn;
        // `EncodingStream.rs` validates exact JSON number tokens through
        // `jet_std::validate_json_number`; the JIT host reads the same
        // foundation implementation the AOT Prelude embeds.
        pub use jet_foundation::JSONNumber::validate_json_number;

        // #1636: one home for the limb-arithmetic bigint routine.
        // The Foundation implementation is the canonical compiled routine;
        // this local name keeps the shared EncodingStream call sites stable.
        pub use jet_foundation::Numeric::CtBigInt as JetBigInt;

        include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/XmlOptions.rs");

        pub struct JSONReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) total: i64,
            pub(crate) offset: i64,
            pub(crate) line: i64,
            pub(crate) column: i64,
            pub(crate) lookahead: Option<u8>,
            pub(super) frames: Vec<super::JetJSONReadFrame>,
            pub(crate) root_started: bool,
            pub(crate) root_done: bool,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) record_mode: bool,
            pub(crate) typed_numbers: bool,
            pub(super) allocation_budget: Option<super::JetEncodingAllocationBudget>,
            pub(crate) output_heap: usize
        }
        pub struct JSONWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(super) frames: Vec<super::JetJSONWriteFrame>,
            pub(crate) root_written: bool,
            pub(crate) finished: bool,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) canonical: bool,
            pub(super) canonical_frames: Vec<super::JetJSONCanonicalFrame>,
            pub(crate) canonical_retained: usize
        }
        pub struct JSONLReader {
            pub(crate) json: JSONReader,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) record_index: i64
        }
        pub struct JSONLWriter {
            pub(crate) json: JSONWriter,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) record_index: i64,
            pub(crate) finished: bool,
            pub(crate) pending_lf: bool
        }
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct CSVRow {
            pub fields: Vec<String>,
            pub line: i64
        }
        pub struct CSVReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) delimiter: char,
            pub(crate) header: bool,
            pub(crate) skip_blank: bool,
            pub(crate) parser: super::jet_csv_kernel::CsvParser,
            pub(super) allocation: super::JetEncodingAllocationBudget,
            pub(crate) utf8: [u8; 4],
            pub(crate) utf8_len: usize,
            pub(crate) total: i64,
            pub(crate) offset: i64,
            pub(crate) line: i64,
            pub(crate) column: i64,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) record_index: i64
        }
        pub struct XMLReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) scanner: crate::jet_xml_pull::StreamScanner,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) eof: bool,
            pub(super) allocation: super::JetEncodingAllocationBudget
        }
        pub struct XMLWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) renderer: crate::jet_xml_pull::StreamWriter,
            pub(crate) buffer: Vec<u8>,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(crate) finished: bool,
            pub(super) allocation: super::JetEncodingAllocationBudget
        }
        pub struct CBORReader {
            pub(crate) input: super::JetFileReader,
            pub(crate) limits: EncodingLimits,
            pub(crate) total: i64,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) eof: bool,
            pub(crate) root_done: bool,
            pub(crate) lookahead: Option<u8>,
            pub(super) frames: Vec<super::JetCBORReadFrame>,
            pub(crate) retained: usize,
            pub(crate) workspace: usize,
            pub(super) allocation: super::JetEncodingAllocationBudget
        }
        pub struct CBORWriter {
            pub(crate) output: super::JetFileWriter,
            pub(crate) limits: EncodingLimits,
            pub(crate) terminal: Option<EncodingError>,
            pub(crate) total: i64,
            pub(super) frames: Vec<super::JetCBORWriteFrame>,
            pub(crate) root_written: bool,
            pub(crate) finished: bool,
            pub(crate) retained: usize,
            pub(crate) workspace: usize,
            pub(super) allocation: super::JetEncodingAllocationBudget
        }
    }

    // EncodingCodecs helpers EncodingStream calls for XML.
    fn jet_xml_to_dyn(value: crate::jet_xml_pull::Value) -> jet_std::Dyn {
        match value {
            crate::jet_xml_pull::Value::Null => jet_std::Dyn::Null,
            crate::jet_xml_pull::Value::Bool(value) => jet_std::Dyn::Bool(value),
            crate::jet_xml_pull::Value::Int(value) => jet_std::Dyn::Int((value).into()),
            crate::jet_xml_pull::Value::Text(value) => jet_std::Dyn::Text(value),
            crate::jet_xml_pull::Value::Array(values) => {
                jet_std::Dyn::Array(values.into_iter().map(jet_xml_to_dyn).collect())
            }
            crate::jet_xml_pull::Value::Object(entries) => jet_std::Dyn::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, jet_xml_to_dyn(value)))
                    .collect(),
            )
        }
    }

    fn jet_xml_from_dyn(
        value: &jet_std::Dyn,
    ) -> Result<crate::jet_xml_pull::Value, String> {
        match value {
            jet_std::Dyn::Null => Ok(crate::jet_xml_pull::Value::Null),
            jet_std::Dyn::Bool(value) => Ok(crate::jet_xml_pull::Value::Bool(*value)),
            jet_std::Dyn::Int(value) => value.to_i64().map(crate::jet_xml_pull::Value::Int)
                .ok_or_else(|| "XML integer is outside I64".to_string()),
            jet_std::Dyn::Number(text) => Ok(crate::jet_xml_pull::Value::Text(text.clone())),
            jet_std::Dyn::Text(value) => Ok(crate::jet_xml_pull::Value::Text(value.clone())),
            jet_std::Dyn::Array(values) => Ok(crate::jet_xml_pull::Value::Array(
                values
                    .iter()
                    .map(jet_xml_from_dyn)
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            jet_std::Dyn::Object(entries) => Ok(crate::jet_xml_pull::Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), jet_xml_from_dyn(value)?)))
                    .collect::<Result<Vec<_>, String>>()?,
            )),
            jet_std::Dyn::Float(_) | jet_std::Dyn::Bytes(_) | jet_std::Dyn::DateTime(_) => {
                Err("XML tree cannot contain Float, Bytes or DateTime values".to_string())
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
                max_entity_replacement_bytes: number(options.limits.max_entity_replacement_bytes)
            }
        }
    }

    use crate::fault_injection::jet_fault_should_fail;
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
        value: jet_std::Dyn,
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
    ) -> Result<Option<jet_std::Dyn>, jet_std::EncodingError> {
        jet_enc_jsonl_reader_next(reader).map(|found| found.ok())
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
        event: jet_std::Dyn,
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
    ) -> Result<Option<jet_std::Dyn>, jet_std::EncodingError> {
        jet_enc_xml_reader_next(reader).map(|found| found.ok())
    }
}
/// Source resource acquisition and iteration use the same FileStream leaf as
/// generated Prelude code.  The returned error is the canonical Prelude
/// `IOError`; SourceResources projects it without rendering or changing its
/// enum/context shape.
pub(crate) fn source_file_open(
    path: &str,
) -> Result<runtime::JetFileReader, SourceIoError> {
    file_stream::jet_std_files_open(&path.to_string())
}

pub(crate) fn source_file_create(
    path: &str,
) -> Result<runtime::JetFileWriter, SourceIoError> {
    file_stream::jet_std_files_create(&path.to_string())
}

pub(crate) fn source_file_append(
    path: &str,
) -> Result<runtime::JetFileWriter, SourceIoError> {
    file_stream::jet_std_files_append(&path.to_string())
}

pub(crate) fn source_file_next_line(
    reader: &mut runtime::JetFileReader,
) -> Result<Option<String>, SourceIoError> {
    file_stream::jet_std_file_reader_read_line(reader)
}

pub(crate) fn source_file_writer_write_line(
    writer: &mut runtime::JetFileWriter,
    line: &String,
) -> Result<(), SourceIoError> {
    file_stream::jet_std_file_writer_write_line(writer, line)
}

pub(crate) fn source_file_writer_flush(
    writer: &mut runtime::JetFileWriter,
) -> Result<(), SourceIoError> {
    file_stream::jet_std_file_writer_flush(writer)
}

pub(crate) fn source_file_writer_path(writer: &runtime::JetFileWriter) -> String {
    file_stream::jet_std_file_writer_path(writer)
}

#[cfg(test)]
mod tests {
    struct ErrorReader;

    impl std::io::Read for ErrorReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "stdin denied",
            ))
        }
    }

    impl std::io::BufRead for ErrorReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "stdin denied",
            ))
        }

        fn consume(&mut self, _amount: usize) {}
    }

    #[test]
    fn shared_stdin_reader_trims_crlf_and_preserves_eof() {
        let mut reader = std::io::Cursor::new(b"first\r\nsecond\n".to_vec());
        assert_eq!(
            super::stdin_reader::jet_std_io_read_line(&mut reader).expect("first line"),
            Some("first".to_string())
        );
        assert_eq!(
            super::stdin_reader::jet_std_io_read_line(&mut reader).expect("second line"),
            Some("second".to_string())
        );
        assert_eq!(
            super::stdin_reader::jet_std_io_read_line(&mut reader).expect("eof"),
            None
        );
    }

    #[test]
    fn shared_stdin_kernel_preserves_following_line_after_sentinel_drop() {
        let mut reader = std::io::Cursor::new(b"stop\nfollow-up\n".to_vec());
        {
            let mut lines = std::iter::from_fn(|| {
                super::stdin_reader::jet_std_io_read_line(&mut reader)
                    .expect("sentinel line")
            });
            assert_eq!(lines.next(), Some("stop".to_string()));
            drop(lines);
        }
        assert_eq!(
            super::stdin_reader::jet_std_io_read_line(&mut reader).expect("follow-up line"),
            Some("follow-up".to_string())
        );
    }

    #[test]
    fn shared_stdin_reader_maps_permission_errors_canonically() {
        let mut reader = ErrorReader;
        let error = super::stdin_reader::jet_std_io_read_line(&mut reader)
            .expect_err("reader must fail");
        assert!(matches!(
            error,
            crate::ProcessPrelude::process_prelude::jet_std::IOError::PermissionDenied(_)
        ));
    }
}
