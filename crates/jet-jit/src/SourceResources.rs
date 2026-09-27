//! Invocation-scoped typed owners for Source/Core resource iteration.
//!
//! This is deliberately the only JIT-side table for Source resource leases.  A
//! slot is keyed by an arena-issued raw capability plus the checked MIR handle
//! identity and exact resource kind.  Raw values are opaque capabilities: they
//! are never cast to pointers, passed to a Rust semantic evaluator, or treated
//! as an item value.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use jet_codegen::Codegen::NativeLoopCursor::{
    NativeIter, NativeLoopResourceFactory, NativeLoopResourceKey, NativeLoopResourceOutcome,
};
use jet_foundation::MIR::{
    stable_id, MirCoreOwner, MirHandleId, MirLoopSourceKind, MirNativeCursor,
    MirNativeCursorError, MirNativeOwned, MirOwnershipMode, MirRuntimeValue, MirType, MirTypeKind,
};

/// The checked handle identity emitted for the private Prelude cursor carrier.
pub fn loop_cursor_handle_id() -> MirHandleId {
    stable_id("mir-handle", "core.prelude::loop_iter_cursor")
}

/// Exact kind of a Source-owned resource slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceResourceKind {
    PlainStream,
    LinesFile,
    /// A typed writer lease is releasable by the Source arena but is not a
    /// loop producer.  It exists so Core file creation never falls back to a
    /// raw legacy slot or a reopened path.
    FileWriter,
    LinesStdin,
    LinesProcessStream,
    ChannelReceiver,
    /// A canonical sender endpoint paired with exactly one receiver owner.
    ChannelSender,
    EncodingReader { reader_type: String },
    /// A typed encoding writer lease. It is release-only, not a loop source.
    EncodingWriter { writer_type: String },
    /// A non-serializable cursor carrier produced by the shared cursor kernel.
    NativeCursor,
}
impl SourceResourceKind {
    pub fn from_loop_source(source: &MirLoopSourceKind) -> Result<Self, String> {
        match source {
            MirLoopSourceKind::Plain => Ok(Self::PlainStream),
            MirLoopSourceKind::LinesFile => Ok(Self::LinesFile),
            MirLoopSourceKind::LinesStdin => Ok(Self::LinesStdin),
            MirLoopSourceKind::LinesProcessStream => Ok(Self::LinesProcessStream),
            MirLoopSourceKind::ChannelReceiver => Ok(Self::ChannelReceiver),
            MirLoopSourceKind::EncodingReader { reader_type } => {
                Ok(Self::EncodingReader {
                    reader_type: reader_type.clone(),
                })
            }
            MirLoopSourceKind::Chars => Err("character iteration is not a resource slot".to_string()),
            MirLoopSourceKind::Iterable { .. } => {
                Err("UserIterable is not a native resource slot".to_string())
            }
        }
    }

    fn matches_loop_source(&self, source: &MirLoopSourceKind) -> bool {
        match (self, source) {
            (Self::PlainStream, MirLoopSourceKind::Plain)
            | (Self::LinesFile, MirLoopSourceKind::LinesFile)
            | (Self::LinesStdin, MirLoopSourceKind::LinesStdin)
            | (Self::LinesProcessStream, MirLoopSourceKind::LinesProcessStream)
            | (Self::ChannelReceiver, MirLoopSourceKind::ChannelReceiver) => true,
            (
                Self::EncodingReader { reader_type: left },
                MirLoopSourceKind::EncodingReader { reader_type: right },
            ) => left == right,
            _ => false,
        }
    }
    pub fn as_loop_source(&self) -> Result<MirLoopSourceKind, String> {
        match self {
            Self::PlainStream => Ok(MirLoopSourceKind::Plain),
            Self::LinesFile => Ok(MirLoopSourceKind::LinesFile),
            Self::FileWriter => Err("file writer is not a loop producer kind".to_string()),
            Self::LinesStdin => Ok(MirLoopSourceKind::LinesStdin),
            Self::LinesProcessStream => Ok(MirLoopSourceKind::LinesProcessStream),
            Self::ChannelReceiver => Ok(MirLoopSourceKind::ChannelReceiver),
            Self::EncodingWriter { .. } => {
                Err("encoding writer is not a loop producer kind".to_string())
            }
            Self::NativeCursor => Err("native cursor is not a loop producer kind".to_string()),
        }
    }

    /// Map one checked Core owner registration and its checked MIR instance to
    /// the physical Source owner kind.
    ///
    /// The registration is the canonical authority for the owner name/key and
    /// ownership mode.  Its nominal identity must match the Apply spelling;
    /// an optional MIR instance identity may differ from the registration id.
    /// The live `(handle, raw, kind)` capability is validated separately by
    /// `take_owned_with_value`.
    pub fn from_core_owner_type(
        registration: &MirCoreOwner,
        owner_type: &MirType,
    ) -> Result<Self, String> {
        if registration.id.0 == 0 || registration.nominal_id.0 == 0 {
            return Err("checked Core owner registration has a zero identity".to_string());
        }
        if registration.key.is_empty() || registration.name.is_empty() {
            return Err("checked Core owner registration has an empty key or name".to_string());
        }
        if !matches!(
            registration.ownership,
            MirOwnershipMode::Owned | MirOwnershipMode::Move
        ) {
            return Err("checked Core owner registration is not owned".to_string());
        }
        let MirTypeKind::Apply { name, args } = &owner_type.kind else {
            return Err("checked Core resource owner is not a nominal Apply".to_string());
        };
        if name.id != registration.nominal_id {
            return Err(format!(
                "checked Core owner `{}` identity disagrees with its nominal registration",
                registration.name
            ));
        }
        if name.name != registration.name {
            return Err(format!(
                "checked Core owner registration `{}` disagrees with Apply name `{}`",
                registration.name, name.name
            ));
        }

        let (kind, expected_arity): (Self, usize) = match registration.name.as_str() {
            "Stream" => (Self::PlainStream, 1),
            "FileReader" => (Self::LinesFile, 0),
            "FileWriter" => (Self::FileWriter, 0),
            "StdinHandle" => (Self::LinesStdin, 0),
            "ProcessStdoutStream" | "ProcessStderrStream" => {
                (Self::LinesProcessStream, 0)
            }
            "Receiver" => (Self::ChannelReceiver, 1),
            "Sender" => (Self::ChannelSender, 1),
            "JSONReader" => (
                Self::EncodingReader {
                    reader_type: "JSONReader".to_string(),
                },
                0,
            ),
            "JSONWriter" => (
                Self::EncodingWriter {
                    writer_type: "JSONWriter".to_string(),
                },
                0,
            ),
            "JSONLReader" => (
                Self::EncodingReader {
                    reader_type: "JSONLReader".to_string(),
                },
                0,
            ),
            "JSONLWriter" => (
                Self::EncodingWriter {
                    writer_type: "JSONLWriter".to_string(),
                },
                0,
            ),
            "CSVReader" => (
                Self::EncodingReader {
                    reader_type: "CSVReader".to_string(),
                },
                0,
            ),
            "CSVWriter" => (
                Self::EncodingWriter {
                    writer_type: "CSVWriter".to_string(),
                },
                0,
            ),
            "XMLReader" => (
                Self::EncodingReader {
                    reader_type: "XMLReader".to_string(),
                },
                0,
            ),
            "XMLWriter" => (
                Self::EncodingWriter {
                    writer_type: "XMLWriter".to_string(),
                },
                0,
            ),
            "CBORReader" => (
                Self::EncodingReader {
                    reader_type: "CBORReader".to_string(),
                },
                0,
            ),
            "CBORWriter" => (
                Self::EncodingWriter {
                    writer_type: "CBORWriter".to_string(),
                },
                0,
            ),
            _ => {
                return Err(format!(
                    "unsupported checked Core resource owner `{}`",
                    registration.name
                ))
            }
        };
        if args.len() != expected_arity {
            return Err(format!(
                "checked Core resource owner `{}` has {} type arguments; expected {}",
                registration.name,
                args.len(),
                expected_arity
            ));
        }
        Ok(kind)
    }

}

impl SourceResourceHandle {
    pub fn native_loop_key(&self) -> Result<NativeLoopResourceKey, String> {
        Ok(NativeLoopResourceKey {
            handle: self.handle,
            raw: self.raw,
            source_kind: self.kind.as_loop_source()?,
        })
    }
}

/// An arena-issued, invocation-scoped capability.  `generation` is retained
/// for diagnostics and stale-lease checks; the raw slot is never recycled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceResourceHandle {
    pub handle: MirHandleId,
    pub raw: i64,
    pub kind: SourceResourceKind,
    pub generation: u32,
}

/// Result of consuming one Source capability into another typed owner.
///
/// `consumed` is `None` when the source capability failed pre-take
/// validation and remains releasable. It is `Some` once the physical owner
/// has been taken; that source token is stale thereafter, including when
/// codec construction or destination registration returns an error.
#[derive(Debug, PartialEq)]
pub struct SourceResourceTransferOutcome {
    pub consumed: Option<SourceResourceHandle>,
    pub result: Result<SourceResourceHandle, MirNativeCursorError>,
}

/// A physical owner detached from one checked Source capability.  The lease
/// carries the consumed receipt privately; no raw capability bits are placed
/// in the queued runtime carrier.
pub struct SourceOwnedLease {
    owner: Arc<Mutex<BackendOwner>>,
    consumed: SourceResourceHandle,
    kind: SourceResourceKind,
}

impl SourceOwnedLease {
    /// Attach the checked runtime payload before moving this lease into a
    /// scheduler value.
    pub fn with_value(self, value: MirRuntimeValue) -> SourceOwnedValue {
        SourceOwnedValue {
            value,
            owner: self.owner,
            consumed: self.consumed,
            kind: self.kind,
        }
    }
    fn into_owner(self) -> Arc<Mutex<BackendOwner>> {
        self.owner
    }

    pub fn kind(&self) -> &SourceResourceKind {
        &self.kind
    }
}

/// A checked runtime payload plus the physical Source owner that was moved
/// with it.  This is the only payload accepted by `adopt_owned`; callers
/// cannot construct one without first taking a live arena capability.
pub struct SourceOwnedValue {
    owner: Arc<Mutex<BackendOwner>>,
    value: MirRuntimeValue,
    consumed: SourceResourceHandle,
    kind: SourceResourceKind,
}

/// Typed payload object stored inside `MirNativeOwned`.  It is public only so
/// native boundary code can use the MIR carrier's typed downcast; its fields
/// remain private and it can only be built from a real Source-owned value.
pub struct SourceOwnedPayload {
    value: SourceOwnedValue,
}

impl SourceOwnedPayload {
    pub fn new(value: SourceOwnedValue) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &SourceOwnedValue {
        &self.value
    }

    pub fn into_value(self) -> SourceOwnedValue {
        self.value
    }
}

impl SourceOwnedValue {
    pub fn value(&self) -> &MirRuntimeValue {
        &self.value
    }


    pub fn kind(&self) -> &SourceResourceKind {
        &self.kind
    }

    /// Wrap the payload and lease in the genuine non-serializable MIR carrier.
    pub fn into_runtime_value(self) -> MirRuntimeValue {
        MirRuntimeValue::NativeOwned(MirNativeOwned::new(SourceOwnedPayload::new(self)))
    }
}

/// Recover one uniquely-owned Source payload from the MIR native-owned
/// carrier.  Aliased carriers are rejected instead of cloning a physical
/// owner or inventing a second logical capability.
pub fn source_owned_from_runtime_value(
    value: MirRuntimeValue,
) -> Result<SourceOwnedValue, MirNativeCursorError> {
    let MirRuntimeValue::NativeOwned(carrier) = value else {
        return Err(MirNativeCursorError::internal(
            "expected a native-owned Source payload",
        ));
    };
    let carrier = carrier
        .downcast::<SourceOwnedPayload>()
        .map_err(|_| MirNativeCursorError::internal("native-owned payload type mismatch"))?;
    std::sync::Arc::try_unwrap(carrier)
        .map(SourceOwnedPayload::into_value)
        .map_err(|_| {
            MirNativeCursorError::internal(
                "native-owned Source payload is aliased and cannot be adopted",
            )
        })
}

/// Receipt returned after a detached payload is adopted by a destination
/// handle.  `consumed` is always stale after `take_owned`; `result` records
/// the destination commit and carries the original logical payload back to
/// the evaluator.
#[derive(Debug, PartialEq)]
pub struct SourceOwnedTransferOutcome {
    pub consumed: SourceResourceHandle,
    pub result: Result<(SourceResourceHandle, MirRuntimeValue), MirNativeCursorError>,
}

/// Cursor acquisition outcome with the source receipt preserved after a
/// by-value owner is detached.  `consumed` is stale once present; it remains
/// available when iterator construction or cursor-slot installation fails.
#[derive(Debug, PartialEq)]
pub struct SourceCursorInitOutcome {
    pub consumed: Option<SourceResourceHandle>,
    pub result: Result<(SourceResourceHandle, MirRuntimeValue), MirNativeCursorError>,
}

/// A typed file owner adopted from the canonical FileStream Prelude leaf.
pub struct SourceFileReader(crate::enc_stream::runtime::JetFileReader);

impl SourceFileReader {
    pub fn open(path: &str) -> Result<Self, MirNativeCursorError> {
        crate::enc_stream::source_file_open(path)
            .map(Self)
            .map_err(file_error)
    }

    /// Consume a legacy JIT FileReader slot at the typed owner boundary.
    pub(crate) fn from_handle(handle: i64) -> Result<Self, String> {
        crate::enc_stream::source_take_file_reader(handle).map(Self)
    }
}

/// A typed writer owner.  Source keeps this owner in the same invocation
/// arena as readers so create/append never reopen a path or expose a slot ID.
pub struct SourceFileWriter(crate::enc_stream::runtime::JetFileWriter);

impl SourceFileWriter {
    pub fn create(path: &str) -> Result<Self, MirNativeCursorError> {
        crate::enc_stream::source_file_create(path)
            .map(Self)
            .map_err(file_error)
    }

    pub fn append(path: &str) -> Result<Self, MirNativeCursorError> {
        crate::enc_stream::source_file_append(path)
            .map(Self)
            .map_err(file_error)
    }

    /// Consume a legacy JIT FileWriter slot at the typed owner boundary.
    pub(crate) fn from_handle(handle: i64) -> Result<Self, String> {
        crate::enc_stream::source_take_file_writer(handle).map(Self)
    }
}

/// A process stdout/stderr owner whose reader lease is safe to move into a
/// native cursor task.  Construction from a checked ProcessChild stays in the
/// ProcessPrelude owner module.
pub struct SourceProcessStream(crate::ProcessPrelude::SourceProcessReader);

impl SourceProcessStream {
    pub(crate) fn from_child(
        child: &crate::ProcessPrelude::ProcessChild,
        stream: crate::ProcessPrelude::SourceProcessStreamKind,
    ) -> Result<Self, String> {
        crate::ProcessPrelude::source_take_process_stream(child, stream).map(Self)
    }
}
/// Public projection of the canonical encoding limits used when an owner is
/// acquired.  The runtime reader keeps the typed `JetOutcome` carrier
/// internally; Source callers only supply the checked scalar facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEncodingLimits {
    pub buffer_bytes: i64,
    pub max_depth: i64,
    pub max_item_bytes: i64,
    pub max_total_bytes: Option<i64>,
    pub max_expansion_depth: i64,
    pub max_expansion_bytes: i64,
}

impl SourceEncodingLimits {
    pub fn safe() -> Self {
        let limits = crate::enc_stream::runtime::jet_std::EncodingLimits::safe();
        Self {
            buffer_bytes: limits.buffer_bytes,
            max_depth: limits.max_depth,
            max_item_bytes: limits.max_item_bytes,
            max_total_bytes: match limits.max_total_bytes {
                Ok(value) => Some(value),
                Err(_) => None,
            },
            max_expansion_depth: limits.max_expansion_depth,
            max_expansion_bytes: limits.max_expansion_bytes,
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::EncodingLimits {
        crate::enc_stream::runtime::jet_std::EncodingLimits {
            buffer_bytes: self.buffer_bytes,
            max_depth: self.max_depth,
            max_item_bytes: self.max_item_bytes,
            max_total_bytes: self
                .max_total_bytes
                .map(Ok)
                .unwrap_or(Err(jet_foundation::Outcome::JetAbsent)),
            max_expansion_depth: self.max_expansion_depth,
            max_expansion_bytes: self.max_expansion_bytes,
        }
    }
}

/// Checked XML parser limits projected from the shared Prelude carrier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceXmlLimits {
    pub max_depth: i64,
    pub max_nodes: i64,
    pub max_attributes_per_element: i64,
    pub max_name_bytes: i64,
    pub max_text_bytes: i64,
    pub max_entity_declarations: i64,
    pub max_entity_depth: i64,
    pub max_entity_replacement_bytes: i64,
}

impl SourceXmlLimits {
    fn from_runtime(
        limits: crate::enc_stream::runtime::jet_std::XMLLimits,
    ) -> Self {
        Self {
            max_depth: limits.max_depth,
            max_nodes: limits.max_nodes,
            max_attributes_per_element: limits.max_attributes_per_element,
            max_name_bytes: limits.max_name_bytes,
            max_text_bytes: limits.max_text_bytes,
            max_entity_declarations: limits.max_entity_declarations,
            max_entity_depth: limits.max_entity_depth,
            max_entity_replacement_bytes: limits.max_entity_replacement_bytes,
        }
    }

    pub fn safe() -> Self {
        Self::from_runtime(
            crate::enc_stream::runtime::jet_std::XMLLimits::safe(),
        )
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLLimits {
        crate::enc_stream::runtime::jet_std::XMLLimits {
            max_depth: self.max_depth,
            max_nodes: self.max_nodes,
            max_attributes_per_element: self.max_attributes_per_element,
            max_name_bytes: self.max_name_bytes,
            max_text_bytes: self.max_text_bytes,
            max_entity_declarations: self.max_entity_declarations,
            max_entity_depth: self.max_entity_depth,
            max_entity_replacement_bytes: self.max_entity_replacement_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceXmlEntityPolicy {
    Preserve,
    Reject,
    Resolve(std::collections::BTreeMap<String, String>),
}

impl SourceXmlEntityPolicy {
    fn from_runtime(
        policy: crate::enc_stream::runtime::jet_std::XMLEntityPolicy,
    ) -> Self {
        match policy {
            crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Preserve => Self::Preserve,
            crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Reject => Self::Reject,
            crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Resolve(values) => {
                Self::Resolve(values)
            }
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLEntityPolicy {
        match self {
            Self::Preserve => crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Preserve,
            Self::Reject => crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Reject,
            Self::Resolve(values) => {
                crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Resolve(values)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceXmlParseOptions {
    pub entities: SourceXmlEntityPolicy,
    pub limits: SourceXmlLimits,
}

impl SourceXmlParseOptions {
    pub fn safe() -> Self {
        let options = crate::enc_stream::runtime::jet_std::XMLParseOptions::safe();
        Self {
            entities: SourceXmlEntityPolicy::from_runtime(options.entities),
            limits: SourceXmlLimits::from_runtime(options.limits),
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLParseOptions {
        crate::enc_stream::runtime::jet_std::XMLParseOptions {
            entities: self.entities.into_runtime(),
            limits: self.limits.into_runtime(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceXmlEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl SourceXmlEncoding {
    fn from_runtime(
        encoding: crate::enc_stream::runtime::jet_std::XMLEncoding,
    ) -> Self {
        match encoding {
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8 => Self::Utf8,
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8BOM => Self::Utf8Bom,
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16LE => Self::Utf16Le,
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16BE => Self::Utf16Be,
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLEncoding {
        match self {
            Self::Utf8 => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8,
            Self::Utf8Bom => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8BOM,
            Self::Utf16Le => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16LE,
            Self::Utf16Be => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16BE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceXmlLexicalPolicy {
    PreserveValid,
    Deterministic,
}

impl SourceXmlLexicalPolicy {
    fn from_runtime(
        policy: crate::enc_stream::runtime::jet_std::XMLLexicalPolicy,
    ) -> Self {
        match policy {
            crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::PreserveValid => {
                Self::PreserveValid
            }
            crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::Deterministic => {
                Self::Deterministic
            }
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLLexicalPolicy {
        match self {
            Self::PreserveValid => {
                crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::PreserveValid
            }
            Self::Deterministic => {
                crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::Deterministic
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceXmlRenderOptions {
    pub encoding: SourceXmlEncoding,
    pub lexical: SourceXmlLexicalPolicy,
}

impl SourceXmlRenderOptions {
    pub fn safe() -> Self {
        let options = crate::enc_stream::runtime::jet_std::XMLRenderOptions::safe();
        Self {
            encoding: SourceXmlEncoding::from_runtime(options.encoding),
            lexical: SourceXmlLexicalPolicy::from_runtime(options.lexical),
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLRenderOptions {
        crate::enc_stream::runtime::jet_std::XMLRenderOptions {
            encoding: self.encoding.into_runtime(),
            lexical: self.lexical.into_runtime(),
        }
    }
}


/// An encoding reader owner.  The concrete reader remains private so callers
/// can only obtain it through the canonical typed constructors below.
pub struct SourceEncodingReader(EncodingReader);

enum EncodingReader {
    Json(crate::enc_stream::runtime::jet_std::JSONReader),
    Jsonl(crate::enc_stream::runtime::jet_std::JSONLReader),
    Csv(crate::enc_stream::runtime::jet_std::CSVReader),
    Xml(crate::enc_stream::runtime::jet_std::XMLReader),
    Cbor(crate::enc_stream::runtime::jet_std::CBORReader),
}

impl SourceEncodingReader {
    fn validate_type(reader_type: &str) -> Result<(), MirNativeCursorError> {
        if matches!(
            reader_type,
            "JSONReader" | "JSONLReader" | "CSVReader" | "XMLReader" | "CBORReader"
        ) {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(format!(
                "unknown checked encoding reader type `{reader_type}`"
            )))
        }
    }

    pub fn from_file(
        file: SourceFileReader,
        reader_type: &str,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_limits(file, reader_type, SourceEncodingLimits::safe())
    }

    pub fn from_file_with_limits(
        file: SourceFileReader,
        reader_type: &str,
        limits: SourceEncodingLimits,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_options(file, reader_type, limits, ",", false, false)
    }

    pub fn from_file_with_options(
        file: SourceFileReader,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_xml_options(
            file,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
            SourceXmlParseOptions::safe(),
        )
    }

    pub fn from_file_with_xml_options(
        file: SourceFileReader,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
        xml_options: SourceXmlParseOptions,
    ) -> Result<Self, MirNativeCursorError> {
        let limits = limits.into_runtime();
        let xml_options = xml_options.into_runtime();
        let reader = match reader_type {
            "JSONReader" => crate::enc_stream::runtime::enc_json_reader(file.0, limits.clone())
                .map(EncodingReader::Json),
            "JSONLReader" => crate::enc_stream::runtime::enc_jsonl_reader(file.0, limits.clone())
                .map(EncodingReader::Jsonl),
            "CSVReader" => crate::enc_stream::runtime::enc_csv_reader(
                file.0,
                limits.clone(),
                delimiter.to_string(),
                header,
                skip_blank,
            )
            .map(EncodingReader::Csv),
            "XMLReader" => crate::enc_stream::runtime::enc_xml_reader(
                file.0,
                limits.clone(),
                xml_options,
            )
            .map(EncodingReader::Xml),
            "CBORReader" => crate::enc_stream::runtime::enc_cbor_reader(file.0, limits)
                .map(EncodingReader::Cbor),
            _ => {
                return Err(MirNativeCursorError::internal(format!(
                    "unknown checked encoding reader type `{reader_type}`"
                )))
            }
        };
        reader
            .map(Self)
            .map_err(|error| encoding_error(&error))
    }

    /// Consume a resident typed codec reader without reopening its input
    /// file.  The slot is moved exactly once by the enc_stream boundary.
    pub(crate) fn from_handle(handle: i64, reader_type: &str) -> Result<Self, String> {
        let reader = match reader_type {
            "JSONReader" => crate::enc_stream::source_take_json_reader(handle)
                .map(EncodingReader::Json),
            "JSONLReader" => crate::enc_stream::source_take_jsonl_reader(handle)
                .map(EncodingReader::Jsonl),
            "CSVReader" => crate::enc_stream::source_take_csv_reader(handle)
                .map(EncodingReader::Csv),
            "XMLReader" => crate::enc_stream::source_take_xml_reader(handle)
                .map(EncodingReader::Xml),
            "CBORReader" => crate::enc_stream::source_take_cbor_reader(handle)
                .map(EncodingReader::Cbor),
            _ => return Err(format!("unknown checked encoding reader type `{reader_type}`")),
        }?;
        Ok(Self(reader))
    }

    fn reader_type(&self) -> &'static str {
        match &self.0 {
            EncodingReader::Json(_) => "JSONReader",
            EncodingReader::Jsonl(_) => "JSONLReader",
            EncodingReader::Csv(_) => "CSVReader",
            EncodingReader::Xml(_) => "XMLReader",
            EncodingReader::Cbor(_) => "CBORReader",
        }
    }
}

/// An encoding writer owner. The output file lease is moved into the codec
/// exactly once; no path is reopened and no legacy integer slot is retained.
pub struct SourceEncodingWriter(EncodingWriter);

enum EncodingWriter {
    Json(crate::enc_stream::runtime::jet_std::JSONWriter),
    Jsonl(crate::enc_stream::runtime::jet_std::JSONLWriter),
    Csv(crate::enc_stream::runtime::jet_std::CSVWriter),
    Xml(crate::enc_stream::runtime::jet_std::XMLWriter),
    Cbor(crate::enc_stream::runtime::jet_std::CBORWriter),
}

impl SourceEncodingWriter {
    fn validate_type(writer_type: &str) -> Result<(), MirNativeCursorError> {
        if matches!(
            writer_type,
            "JSONWriter" | "JSONLWriter" | "CSVWriter" | "XMLWriter" | "CBORWriter"
        ) {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(format!(
                "unknown checked encoding writer type `{writer_type}`"
            )))
        }
    }

    pub fn from_file(
        file: SourceFileWriter,
        writer_type: &str,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_options(file, writer_type, SourceEncodingLimits::safe(), false)
    }

    pub fn from_file_with_limits(
        file: SourceFileWriter,
        writer_type: &str,
        limits: SourceEncodingLimits,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_options(file, writer_type, limits, false)
    }

    pub fn from_file_with_options(
        file: SourceFileWriter,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_xml_options(
            file,
            writer_type,
            limits,
            canonical,
            SourceXmlRenderOptions::safe(),
        )
    }

    pub fn from_file_with_xml_options(
        file: SourceFileWriter,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
        xml_options: SourceXmlRenderOptions,
    ) -> Result<Self, MirNativeCursorError> {
        let limits = limits.into_runtime();
        let xml_options = xml_options.into_runtime();
        let writer = match writer_type {
            "JSONWriter" => crate::enc_stream::runtime::enc_json_writer(file.0, limits.clone(), canonical)
                .map(EncodingWriter::Json),
            "JSONLWriter" => crate::enc_stream::runtime::enc_jsonl_writer(file.0, limits.clone())
                .map(EncodingWriter::Jsonl),
            "CSVWriter" => crate::enc_stream::runtime::enc_csv_writer(file.0, limits.clone())
                .map(EncodingWriter::Csv),
            "XMLWriter" => crate::enc_stream::runtime::enc_xml_writer(
                file.0,
                limits.clone(),
                xml_options,
            )
            .map(EncodingWriter::Xml),
            "CBORWriter" => crate::enc_stream::runtime::enc_cbor_writer(file.0, limits)
                .map(EncodingWriter::Cbor),
            _ => {
                return Err(MirNativeCursorError::internal(format!(
                    "unknown checked encoding writer type `{writer_type}`"
                )))
            }
        };
        writer
            .map(Self)
            .map_err(|error| encoding_error(&error))
    }

    fn writer_type(&self) -> &'static str {
        match &self.0 {
            EncodingWriter::Json(_) => "JSONWriter",
            EncodingWriter::Jsonl(_) => "JSONLWriter",
            EncodingWriter::Csv(_) => "CSVWriter",
            EncodingWriter::Xml(_) => "XMLWriter",
            EncodingWriter::Cbor(_) => "CBORWriter",
        }
    }
}

/// One owner admitted to the invocation-scoped arena.
enum BackendOwner {
    PlainStream(jet_codegen::scheduler::JetStream<MirRuntimeValue>),
    File(SourceFileReader),
    FileWriter(SourceFileWriter),
    Stdin(crate::enc_stream::SourceStdinReader),
    Process(SourceProcessStream),
    ChannelReceiver(jet_codegen::scheduler::JetSchedulerChannel<MirRuntimeValue>),
    ChannelSender(jet_codegen::scheduler::JetSchedulerSender<MirRuntimeValue>),
    Encoding(SourceEncodingReader),
    EncodingWriter(SourceEncodingWriter),

}
impl BackendOwner {
    fn kind(&self) -> SourceResourceKind {
        match self {
            Self::PlainStream(_) => SourceResourceKind::PlainStream,
            Self::File(_) => SourceResourceKind::LinesFile,
            Self::FileWriter(_) => SourceResourceKind::FileWriter,
            Self::Stdin(_) => SourceResourceKind::LinesStdin,
            Self::Process(_) => SourceResourceKind::LinesProcessStream,
            Self::ChannelReceiver(_) => SourceResourceKind::ChannelReceiver,
            Self::ChannelSender(_) => SourceResourceKind::ChannelSender,
            Self::Encoding(reader) => SourceResourceKind::EncodingReader {
                reader_type: reader.reader_type().to_string(),
            },
            Self::EncodingWriter(writer) => SourceResourceKind::EncodingWriter {
                writer_type: writer.writer_type().to_string(),
            },
        }
    }
}

trait FromBackendOwner: Sized {
    fn from_backend_owner(owner: BackendOwner) -> Result<Self, BackendOwner>;
}

impl FromBackendOwner for SourceFileReader {
    fn from_backend_owner(owner: BackendOwner) -> Result<Self, BackendOwner> {
        match owner {
            BackendOwner::File(reader) => Ok(reader),
            owner => Err(owner),
        }
    }
}

impl FromBackendOwner for SourceFileWriter {
    fn from_backend_owner(owner: BackendOwner) -> Result<Self, BackendOwner> {
        match owner {
            BackendOwner::FileWriter(writer) => Ok(writer),
            owner => Err(owner),
        }
    }
}

enum SlotEntry {
    Backend(Arc<Mutex<BackendOwner>>),
    Cursor(MirNativeCursor),
}

struct Slot {
    handle: MirHandleId,
    kind: SourceResourceKind,
    generation: u32,
    entry: SlotEntry,
}

// Capability generations are globally monotonic, but this atomic is only a
// nonce allocator: owners remain in the invocation-local arena below.
static NEXT_CAPABILITY_GENERATION: AtomicU32 = AtomicU32::new(0);

struct ArenaState {
    next_slot: u32,
    slots: HashMap<u32, Slot>,
    /// Logical retirement has been requested by the owning session.  Existing
    /// retained roots may continue using the table until their last explicit
    /// lease is released.
    retirement_requested: bool,
    /// Explicit Source callback/task roots, never inferred from Arc clones.
    retained_roots: usize,
    retired: bool,
}

thread_local! {
    static ACTIVE_SOURCE_ARENA: RefCell<Option<SourceResourceArena>> =
        const { RefCell::new(None) };
}

/// Sole invocation-scoped source resource authority. Clones share the slot
/// table and therefore retain the same concrete owners until explicit
/// capability release. An explicit `SourceResourceLease`, not an arena
/// `Arc` clone, keeps the table alive across session retirement.
#[derive(Clone)]
pub struct SourceResourceArena {
    state: Arc<Mutex<ArenaState>>,
}

/// Session ownership for callbacks that return live Source capabilities.
/// Individual outcome handles are released through `resource_release` or
/// `release_capability`.  `retire` requests logical retirement; an explicit
/// `SourceResourceLease` keeps the physical arena usable until its last
/// release.
#[derive(Clone)]
pub struct SourceResourceSession {
    arena: SourceResourceArena,
}

struct SourceResourceLeaseInner {
    arena: SourceResourceArena,
}

/// An explicit retained physical-root lease for one Source callback/task
/// graph. Arena `Arc` clones are lookup/activation handles only and do not
/// keep an arena alive after retirement. Lease aliases share one explicit
/// retained-root count; only the final alias releases it.
pub struct SourceResourceLease {
    inner: Arc<SourceResourceLeaseInner>,
}

/// Thread-local activation only changes lookup context. It never retires or
/// clears the activated arena; the owning session controls that lifecycle.
pub struct SourceResourceActivation {
    _current: SourceResourceArena,
    previous: Option<SourceResourceArena>,
}

impl Drop for SourceResourceActivation {
    fn drop(&mut self) {
        ACTIVE_SOURCE_ARENA.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

impl Default for SourceResourceArena {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceResourceSession {
    pub fn new() -> Self {
        Self {
            arena: SourceResourceArena::new(),
        }
    }

    pub fn arena(&self) -> SourceResourceArena {
        self.arena.clone()
    }

    /// Retain one explicit physical-root lease for an escaping callback/task
    /// graph. The lease must be released by Source semantic cleanup before
    /// the arena can finish a pending retirement request.
    pub fn retain_root(&self) -> Result<SourceResourceLease, String> {
        self.arena.retain_root()
    }

    pub fn activate(&self) -> SourceResourceActivation {
        activate_source_resource_arena(&self.arena)
    }

    pub fn retire(&self) -> Result<(), String> {
        self.arena.retire()
    }

    pub fn is_retirement_requested(&self) -> Result<bool, String> {
        self.arena.is_retirement_requested()
    }

    pub fn retained_root_count(&self) -> Result<usize, String> {
        self.arena.retained_root_count()
    }
}

impl SourceResourceLease {
    /// Obtain the shared arena retained by this physical-root lease.
    pub fn arena(&self) -> SourceResourceArena {
        self.inner.arena.clone()
    }

    /// Activate the retained arena for one owner-pump callback.
    pub fn activate(&self) -> SourceResourceActivation {
        activate_source_resource_arena(&self.inner.arena)
    }

    pub fn is_retired(&self) -> Result<bool, String> {
        self.inner.arena.is_retired()
    }
}

impl Clone for SourceResourceLease {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for SourceResourceLeaseInner {
    fn drop(&mut self) {
        self.arena.release_root();
    }
}

impl Default for SourceResourceSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Activate an existing session-owned arena and restore the prior activation
/// on normal return or unwind.
pub fn activate_source_resource_arena(
    arena: &SourceResourceArena,
) -> SourceResourceActivation {
    let previous = ACTIVE_SOURCE_ARENA.with(|slot| slot.borrow_mut().replace(arena.clone()));
    SourceResourceActivation {
        _current: arena.clone(),
        previous,
    }
}

/// Establish a fresh, callback-local arena. Callers returning live
/// capabilities must instead retain a `SourceResourceSession`, obtain an
/// explicit `SourceResourceLease`, and activate its shared arena around each
/// callback.
pub fn with_source_resource_arena<R>(
    body: impl FnOnce(&SourceResourceArena) -> R,
) -> R {
    let arena = SourceResourceArena::new();
    let _activation = activate_source_resource_arena(&arena);
    body(&arena)
}

/// Obtain the active invocation arena for a plain bootstrap function pointer.
/// The clone shares the scoped table; it does not create a second registry.
pub fn active_source_resource_arena() -> Option<SourceResourceArena> {
    ACTIVE_SOURCE_ARENA.with(|slot| slot.borrow().clone())
}

/// Borrow the currently active arena without extending its lifetime. This is
/// the preferred accessor for plain bootstrap function pointers.
pub fn with_active_source_resource_arena<R>(
    body: impl FnOnce(Option<&SourceResourceArena>) -> R,
) -> R {
    ACTIVE_SOURCE_ARENA.with(|slot| body(slot.borrow().as_ref()))
}

impl SourceResourceArena {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ArenaState {
                next_slot: 0,
                slots: HashMap::new(),
                retirement_requested: false,
                retained_roots: 0,
                retired: false,
            })),
        }
    }

    /// Retain one explicit physical-root lease. Existing arena clones do not
    /// count as roots and cannot keep a pending retirement alive.
    fn retain_root(&self) -> Result<SourceResourceLease, String> {
        let mut state = self.lock()?;
        if state.retired {
            return Err("Source resource arena is retired".to_string());
        }
        state.retained_roots = state
            .retained_roots
            .checked_add(1)
            .ok_or_else(|| "Source resource retained-root count exhausted".to_string())?;
        drop(state);
        Ok(SourceResourceLease {
            inner: Arc::new(SourceResourceLeaseInner {
                arena: self.clone(),
            }),
        })
    }

    /// Request logical retirement. Physical owner clearing is deferred while
    /// explicit callback/task roots remain live.
    pub fn retire(&self) -> Result<(), String> {
        let slots = {
            let mut state = self.lock()?;
            state.retirement_requested = true;
            if state.retired || state.retained_roots != 0 {
                None
            } else {
                state.retired = true;
                Some(std::mem::take(&mut state.slots))
            }
        };
        // Slot owners may run semantic cleanup; never drop them under the
        // arena mutex.
        drop(slots);
        Ok(())
    }

    pub fn is_retired(&self) -> Result<bool, String> {
        Ok(self.lock()?.retired)
    }

    pub fn is_retirement_requested(&self) -> Result<bool, String> {
        Ok(self.lock()?.retirement_requested)
    }

    pub fn retained_root_count(&self) -> Result<usize, String> {
        Ok(self.lock()?.retained_roots)
    }

    fn release_root(&self) {
        let slots = {
            let Ok(mut state) = self.lock() else {
                return;
            };
            if state.retained_roots == 0 {
                return;
            }
            state.retained_roots -= 1;
            if state.retained_roots == 0
                && state.retirement_requested
                && !state.retired
            {
                state.retired = true;
                Some(std::mem::take(&mut state.slots))
            } else {
                None
            }
        };
        // As in explicit retire, physical owners are dropped outside the
        // state mutex so their cleanup may safely re-enter the arena.
        drop(slots);
    }


    fn lock(&self) -> Result<MutexGuard<'_, ArenaState>, String> {
        self.state
            .lock()
            .map_err(|_| "Source resource arena lock is poisoned".to_string())
    }

    fn allocate_slot(
        state: &mut ArenaState,
        handle: MirHandleId,
        kind: SourceResourceKind,
        entry: SlotEntry,
    ) -> Result<SourceResourceHandle, String> {
        if state.retired {
            return Err("Source resource arena is retired".to_string());
        }
        state.next_slot = state
            .next_slot
            .checked_add(1)
            .ok_or_else(|| "Source resource slot space exhausted".to_string())?;
        let generation = Self::allocate_generation()?;
        let slot_id = state.next_slot;
        let raw_bits = (u64::from(generation) << 32) | u64::from(slot_id);
        let raw = i64::from_ne_bytes(raw_bits.to_ne_bytes());
        state.slots.insert(
            slot_id,
            Slot {
                handle,
                kind: kind.clone(),
                generation,
                entry,
            },
        );
        Ok(SourceResourceHandle {
            handle,
            raw,
            kind,
            generation,
        })
    }

    fn decode_raw(raw: i64) -> Result<(u32, u32), String> {
        let bits = u64::from_ne_bytes(raw.to_ne_bytes());
        let slot_id = bits as u32;
        let generation = (bits >> 32) as u32;
        if slot_id == 0 || generation == 0 {
            return Err("invalid or stale Source resource capability".to_string());
        }
        Ok((slot_id, generation))
    }
    fn allocate_generation() -> Result<u32, String> {
        let mut current = NEXT_CAPABILITY_GENERATION.load(Ordering::Relaxed);
        loop {
            let next = current
                .checked_add(1)
                .ok_or_else(|| "Source resource generation space exhausted".to_string())?;
            match NEXT_CAPABILITY_GENERATION.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(next),
                Err(observed) => current = observed,
            }
        }
    }

    fn register_backend(
        &self,
        handle: MirHandleId,
        owner: BackendOwner,
    ) -> Result<SourceResourceHandle, String> {
        let kind = owner.kind();
        self.register_backend_arc(handle, kind, Arc::new(Mutex::new(owner)))
    }

    fn register_backend_arc(
        &self,
        handle: MirHandleId,
        kind: SourceResourceKind,
        owner: Arc<Mutex<BackendOwner>>,
    ) -> Result<SourceResourceHandle, String> {
        let result = {
            let mut state = self.lock()?;
            Self::allocate_slot(
                &mut state,
                handle,
                kind,
                SlotEntry::Backend(Arc::clone(&owner)),
            )
        };
        drop(owner);
        result
    }

    /// Register a canonical file reader owner and receive its scoped
    /// capability.
    pub fn register_file(
        &self,
        handle: MirHandleId,
        reader: SourceFileReader,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::File(reader))
    }

    /// Adopt a typed reader from the legacy JIT table without retaining the
    /// integer slot as a second owner.
    pub fn register_file_handle(
        &self,
        handle: MirHandleId,
        reader_handle: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_file(handle, SourceFileReader::from_handle(reader_handle)?)
    }

    /// Open and register a file through the canonical FileStream leaf.
    pub fn open_file(
        &self,
        handle: MirHandleId,
        path: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        self.register_file(handle, SourceFileReader::open(path)?)
            .map_err(MirNativeCursorError::internal)
    }

    /// Register a canonical file writer owner.  Writers are release-only
    /// leases and intentionally cannot be projected as loop producers.
    pub fn register_file_writer(
        &self,
        handle: MirHandleId,
        writer: SourceFileWriter,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::FileWriter(writer))
    }

    /// Adopt a typed writer from the legacy JIT table.
    pub fn register_file_writer_handle(
        &self,
        handle: MirHandleId,
        writer_handle: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_file_writer(handle, SourceFileWriter::from_handle(writer_handle)?)
    }

    pub fn open_file_writer(
        &self,
        handle: MirHandleId,
        path: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        self.register_file_writer(handle, SourceFileWriter::create(path)?)
            .map_err(MirNativeCursorError::internal)
    }

    pub fn append_file_writer(
        &self,
        handle: MirHandleId,
        path: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        self.register_file_writer(handle, SourceFileWriter::append(path)?)
            .map_err(MirNativeCursorError::internal)
    }

    /// Register the typed canonical stdin line producer.
    pub fn register_stdin(
        &self,
        handle: MirHandleId,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(
            handle,
            BackendOwner::Stdin(crate::enc_stream::source_stdin_reader()),
        )
    }

    /// Register an already-adopted process stdout/stderr reader.
    pub fn register_process_stream(
        &self,
        handle: MirHandleId,
        reader: SourceProcessStream,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::Process(reader))
    }
    /// Adopt a process child output reader from the canonical Process owner
    /// table, then issue the Source arena capability for that stream.
    pub fn register_process_stream_handle(
        &self,
        handle: MirHandleId,
        child_raw: i64,
        stdout: bool,
    ) -> Result<SourceResourceHandle, String> {
        let stream = if stdout {
            crate::ProcessPrelude::SourceProcessStreamKind::Stdout
        } else {
            crate::ProcessPrelude::SourceProcessStreamKind::Stderr
        };
        let reader = crate::Process::source_take_process_stream(child_raw, stream)?;
        self.register_process_stream(handle, SourceProcessStream(reader))
    }


    /// Register a scheduler channel receiver carrying Source runtime values.
    pub fn register_channel_receiver(
        &self,
        handle: MirHandleId,
        receiver: jet_codegen::scheduler::JetSchedulerChannel<MirRuntimeValue>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::ChannelReceiver(receiver))
    }

    /// Register the matching canonical sender endpoint.
    pub fn register_channel_sender(
        &self,
        handle: MirHandleId,
        sender: jet_codegen::scheduler::JetSchedulerSender<MirRuntimeValue>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::ChannelSender(sender))
    }

    /// Clone a receiver endpoint into a fresh logical arena capability while
    /// retaining the scheduler's physical receiver count.
    pub fn clone_channel_receiver(
        &self,
        source_handle: MirHandleId,
        source_raw: i64,
        target_handle: MirHandleId,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let owner = self.owner_for_raw(
            source_handle,
            source_raw,
            &SourceResourceKind::ChannelReceiver,
        )?;
        let receiver = {
            let owner = owner.lock().map_err(|_| {
                MirNativeCursorError::internal("Source resource owner lock is poisoned")
            })?;
            let BackendOwner::ChannelReceiver(receiver) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Source channel receiver capability payload mismatch",
                ));
            };
            receiver.clone()
        };
        self.register_channel_receiver(target_handle, receiver)
            .map_err(MirNativeCursorError::internal)
    }

    /// Clone a sender endpoint into a fresh logical arena capability while
    /// retaining the scheduler's physical sender count.
    pub fn clone_channel_sender(
        &self,
        source_handle: MirHandleId,
        source_raw: i64,
        target_handle: MirHandleId,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let owner = self.owner_for_raw(
            source_handle,
            source_raw,
            &SourceResourceKind::ChannelSender,
        )?;
        let sender = {
            let owner = owner.lock().map_err(|_| {
                MirNativeCursorError::internal("Source resource owner lock is poisoned")
            })?;
            let BackendOwner::ChannelSender(sender) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Source channel sender capability payload mismatch",
                ));
            };
            sender.clone()
        };
        self.register_channel_sender(target_handle, sender)
            .map_err(MirNativeCursorError::internal)
    }

    /// Create one canonical sender/receiver endpoint pair. Each endpoint gets
    /// its own arena capability and scheduler reference count.
    pub fn register_new_channel(
        &self,
        sender_handle: MirHandleId,
        receiver_handle: MirHandleId,
        capacity: Option<i64>,
    ) -> Result<(SourceResourceHandle, SourceResourceHandle), String> {
        let receiver = match capacity {
            Some(capacity) => jet_codegen::scheduler::JetSchedulerChannel::bounded(capacity),
            None => jet_codegen::scheduler::JetSchedulerChannel::new(),
        };
        let sender = receiver.sender();
        let sender_capability = self.register_channel_sender(sender_handle, sender)?;
        match self.register_channel_receiver(receiver_handle, receiver) {
            Ok(receiver_capability) => Ok((sender_capability, receiver_capability)),
            Err(error) => {
                let _ = self.release_capability(&sender_capability);
                Err(error)
            }
        }
    }

    pub fn register_timer_channel(
        &self,
        handle: MirHandleId,
        delay_ms: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_channel_receiver(
            handle,
            jet_codegen::scheduler::JetSchedulerChannel::timer(delay_ms),
        )
    }

    pub fn register_interval_channel(
        &self,
        handle: MirHandleId,
        delay_ms: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_channel_receiver(
            handle,
            jet_codegen::scheduler::JetSchedulerChannel::interval(delay_ms),
        )
    }


    /// Register a scheduler stream carrying Source runtime values.
    pub fn register_plain_stream(
        &self,
        handle: MirHandleId,
        stream: jet_codegen::scheduler::JetStream<MirRuntimeValue>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::PlainStream(stream))
    }
    /// Create a canonical pull-gated stream and register its consumer owner.
    pub fn register_new_plain_stream(
        &self,
        handle: MirHandleId,
    ) -> Result<
        (
            SourceResourceHandle,
            jet_codegen::scheduler::JetStreamSender<MirRuntimeValue>,
        ),
        String,
    > {
        let (sender, stream) = jet_codegen::scheduler::jet_stream();
        let capability = self.register_plain_stream(handle, stream)?;
        Ok((capability, sender))
    }

    /// Register an already-constructed canonical encoding reader.
    pub fn register_encoding_reader(
        &self,
        handle: MirHandleId,
        reader: SourceEncodingReader,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::Encoding(reader))
    }

    /// Adopt a resident typed codec reader without reopening the underlying
    /// file or retaining a second integer slot owner.
    pub fn register_encoding_reader_handle(
        &self,
        handle: MirHandleId,
        reader_handle: i64,
        reader_type: &str,
    ) -> Result<SourceResourceHandle, String> {
        self.register_encoding_reader(
            handle,
            SourceEncodingReader::from_handle(reader_handle, reader_type)?,
        )
    }

    /// Register an already-constructed canonical encoding writer.
    pub fn register_encoding_writer(
        &self,
        handle: MirHandleId,
        writer: SourceEncodingWriter,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::EncodingWriter(writer))
    }

    /// Consume a live file-writer capability into a typed codec writer and
    /// register the resulting lease under a caller-selected capability.
    /// The file capability is moved before codec construction; constructor
    /// failure therefore drops the physical writer instead of reopening it.
    pub fn register_encoding_writer_from_file(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        writer_type: &str,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_writer_from_file_with_options(
            output_handle,
            file_handle,
            file_raw,
            writer_type,
            SourceEncodingLimits::safe(),
            false,
        )
    }

    /// Consume a live file-reader capability into a typed codec reader and
    /// register the resulting lease under a caller-selected capability.
    pub fn register_encoding_reader_from_file(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        reader_type: &str,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_reader_from_file_with_options(
            output_handle,
            file_handle,
            file_raw,
            reader_type,
            SourceEncodingLimits::safe(),
            ",",
            false,
            false,
        )
    }

    pub fn register_encoding_reader_from_file_with_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_reader_from_file_with_xml_options(
            output_handle,
            file_handle,
            file_raw,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
            SourceXmlParseOptions::safe(),
        )
    }

    pub fn register_encoding_reader_from_file_with_xml_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
        xml_options: SourceXmlParseOptions,
    ) -> SourceResourceTransferOutcome {
        if let Err(error) = SourceEncodingReader::validate_type(reader_type) {
            return SourceResourceTransferOutcome {
                consumed: None,
                result: Err(error),
            };
        }
        let (source, file) = match self
            .take_file_owner::<SourceFileReader>(
                file_handle,
                file_raw,
                SourceResourceKind::LinesFile,
            ) {
            Ok(owners) => owners,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: None,
                    result: Err(error),
                }
            }
        };
        let reader = match SourceEncodingReader::from_file_with_xml_options(
            file,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
            xml_options,
        ) {
            Ok(reader) => reader,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: Some(source),
                    result: Err(error),
                }
            }
        };
        let result = self
            .register_encoding_reader(output_handle, reader)
            .map_err(MirNativeCursorError::internal);
        SourceResourceTransferOutcome {
            consumed: Some(source),
            result,
        }
    }

    pub fn register_encoding_writer_from_file_with_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_writer_from_file_with_xml_options(
            output_handle,
            file_handle,
            file_raw,
            writer_type,
            limits,
            canonical,
            SourceXmlRenderOptions::safe(),
        )
    }

    pub fn register_encoding_writer_from_file_with_xml_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
        xml_options: SourceXmlRenderOptions,
    ) -> SourceResourceTransferOutcome {
        if let Err(error) = SourceEncodingWriter::validate_type(writer_type) {
            return SourceResourceTransferOutcome {
                consumed: None,
                result: Err(error),
            };
        }
        let (source, file) = match self
            .take_file_owner::<SourceFileWriter>(
                file_handle,
                file_raw,
                SourceResourceKind::FileWriter,
            ) {
            Ok(owners) => owners,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: None,
                    result: Err(error),
                }
            }
        };
        let writer = match SourceEncodingWriter::from_file_with_xml_options(
            file,
            writer_type,
            limits,
            canonical,
            xml_options,
        ) {
            Ok(writer) => writer,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: Some(source),
                    result: Err(error),
                }
            }
        };
        let result = self
            .register_encoding_writer(output_handle, writer)
            .map_err(MirNativeCursorError::internal);
        SourceResourceTransferOutcome {
            consumed: Some(source),
            result,
        }
    }

    /// Open a canonical file and construct one of the checked encoding reader
    /// owners before registering it in this invocation's table.
    pub fn open_encoding_reader(
        &self,
        handle: MirHandleId,
        path: &str,
        reader_type: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let file = SourceFileReader::open(path)?;
        let reader = SourceEncodingReader::from_file(file, reader_type)?;
        self.register_encoding_reader(handle, reader)
            .map_err(MirNativeCursorError::internal)
    }

    pub fn open_encoding_reader_with_limits(
        &self,
        handle: MirHandleId,
        path: &str,
        reader_type: &str,
        limits: SourceEncodingLimits,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let file = SourceFileReader::open(path)?;
        let reader = SourceEncodingReader::from_file_with_limits(file, reader_type, limits)?;
        self.register_encoding_reader(handle, reader)
            .map_err(MirNativeCursorError::internal)
    }

    pub fn open_encoding_reader_with_options(
        &self,
        handle: MirHandleId,
        path: &str,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let file = SourceFileReader::open(path)?;
        let reader = SourceEncodingReader::from_file_with_options(
            file,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
        )?;
        self.register_encoding_reader(handle, reader)
            .map_err(MirNativeCursorError::internal)
    }



    /// Insert a native cursor carrier under the private checked cursor handle.
    /// The raw slot and generation are allocated by this arena, not supplied by
    /// a caller or copied from a backend integer handle.
    pub fn insert_cursor(
        &self,
        handle: MirHandleId,
        cursor: MirNativeCursor,
    ) -> Result<SourceResourceHandle, String> {
        if handle != loop_cursor_handle_id() {
            return Err("native cursor uses the checked Prelude cursor handle identity".to_string());
        }
        let result = {
            let mut state = self.lock()?;
            Self::allocate_slot(
                &mut state,
                handle,
                SourceResourceKind::NativeCursor,
                SlotEntry::Cursor(cursor.clone()),
            )
        };
        drop(cursor);
        result
    }
    /// Adopt an already-created native cursor value without exposing a
    /// serializable integer representation of its payload.
    pub fn insert_cursor_value(
        &self,
        handle: MirHandleId,
        value: &MirRuntimeValue,
    ) -> Result<SourceResourceHandle, String> {
        let MirRuntimeValue::NativeCursor(cursor) = value else {
            return Err("cursor slot requires the private NativeCursor carrier".to_string());
        };
        self.insert_cursor(handle, cursor.clone())
    }

    /// Resolve a cursor carrier only through the exact private cursor handle
    /// and the arena-issued raw slot.
    pub fn lookup_cursor(
        &self,

        handle: MirHandleId,
        raw: i64,
    ) -> Result<MirNativeCursor, String> {
        if handle != loop_cursor_handle_id() {
            return Err("native cursor handle identity mismatch".to_string());
        }
        let state = self.lock()?;
        let (slot_id, generation) = Self::decode_raw(raw)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown native cursor capability".to_string())?;
        if slot.generation != generation
            || slot.handle != handle
            || slot.kind != SourceResourceKind::NativeCursor
        {
            return Err("native cursor capability generation, handle, or kind mismatch".to_string());
        }
        match &slot.entry {
            SlotEntry::Cursor(cursor) => Ok(cursor.clone()),
            SlotEntry::Backend(_) => Err("native cursor capability payload mismatch".to_string()),
        }
    }

    /// Release a cursor slot after validating its exact identity.
    pub fn release_cursor(&self, handle: MirHandleId, raw: i64) -> Result<(), String> {
        self.release(handle, raw, &SourceResourceKind::NativeCursor)
    }


    /// Release any Source owner when the checked callback already carries the
    /// producer handle and opaque capability.  The stored slot supplies the
    /// exact kind; callers never re-infer or spoof it.
    pub fn release_resource(&self, handle: MirHandleId, raw: i64) -> Result<(), String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(raw)?;
            let slot = state
                .slots
                .get(&slot_id)
                .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
            if slot.generation != generation || slot.handle != handle {
                return Err("Source resource generation or handle mismatch".to_string());
            }
            state.slots.remove(&slot_id).ok_or_else(|| {
                "Source resource capability disappeared".to_string()
            })?
        };
        drop(removed);
        Ok(())
    }

    /// Native callback spelling for the shared Core/Cursor release route.
    pub fn resource_release(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        self.release_resource(handle, raw)
            .map_err(MirNativeCursorError::internal)
    }
    /// Release any backend owner through the exact checked triple.
    pub fn release(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
    ) -> Result<(), String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(raw)?;
            let slot = state
                .slots
                .get(&slot_id)
                .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
            if slot.generation != generation
                || slot.handle != handle
                || &slot.kind != expected_kind
            {
                return Err("Source resource generation, handle, or kind mismatch".to_string());
            }
            state.slots.remove(&slot_id).ok_or_else(|| {
                "Source resource capability disappeared".to_string()
            })?
        };
        drop(removed);
        Ok(())
    }

    /// Release using the generation-bearing capability returned by an insert.
    pub fn release_capability(&self, capability: &SourceResourceHandle) -> Result<(), String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(capability.raw)?;
            if generation != capability.generation {
                return Err("stale Source resource generation mismatch".to_string());
            }
            let slot = state
                .slots
                .get(&slot_id)
                .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
            if slot.generation != generation
                || slot.handle != capability.handle
                || slot.kind != capability.kind
            {
                return Err("stale Source resource generation or kind mismatch".to_string());
            }
            state.slots.remove(&slot_id).ok_or_else(|| {
                "Source resource capability disappeared".to_string()
            })?
        };
        drop(removed);
        Ok(())
    }

    /// Validate an opaque `(handle, raw)` packet against this arena and return
    /// its generation-bearing capability.  Native hosts must use this lookup
    /// rather than decoding the raw value or manufacturing a resource kind.
    pub fn lookup_capability(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceResourceHandle, String> {
        let state = self.lock()?;
        let (slot_id, generation) = Self::decode_raw(raw)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
        if slot.generation != generation || slot.handle != handle {
            return Err("Source resource generation or handle mismatch".to_string());
        }
        Ok(SourceResourceHandle {
            handle: slot.handle,
            raw,
            kind: slot.kind.clone(),
            generation: slot.generation,
        })
    }

    fn owner_for_raw(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
    ) -> Result<Arc<Mutex<BackendOwner>>, MirNativeCursorError> {
        let state = self.lock().map_err(MirNativeCursorError::internal)?;
        let (slot_id, generation) = Self::decode_raw(raw)
            .map_err(MirNativeCursorError::internal)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("stale or unknown Source resource capability")
            })?;
        if slot.generation != generation
            || slot.handle != handle
            || &slot.kind != expected_kind
        {
            return Err(MirNativeCursorError::internal(
                "Source resource generation, handle, or kind mismatch",
            ));
        }
        match &slot.entry {
            SlotEntry::Backend(owner) => Ok(Arc::clone(owner)),
            SlotEntry::Cursor(_) => Err(MirNativeCursorError::internal(
                "Source resource capability payload mismatch",
            )),
        }
    }

    /// Atomically consume one sole-owner backend slot into a physical lease.
    /// The returned capability is committed stale; the lease is the only
    /// remaining owner and carries no serializable raw capability payload.
    pub fn take_owned(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
    ) -> Result<(SourceResourceHandle, SourceOwnedLease), MirNativeCursorError> {
        let mut state = self.lock().map_err(MirNativeCursorError::internal)?;
        let (slot_id, generation) =
            Self::decode_raw(raw).map_err(MirNativeCursorError::internal)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("stale or unknown Source resource capability")
            })?;
        if slot.generation != generation
            || slot.handle != handle
            || &slot.kind != expected_kind
        {
            return Err(MirNativeCursorError::internal(
                "Source resource generation, handle, or kind mismatch",
            ));
        }
        let consumed = SourceResourceHandle {
            handle: slot.handle,
            raw,
            kind: slot.kind.clone(),
            generation: slot.generation,
        };
        let slot = state
            .slots
            .remove(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("Source resource capability disappeared")
            })?;
        let Slot {
            handle: slot_handle,
            kind: slot_kind,
            generation: slot_generation,
            entry,
        } = slot;
        let owner = match entry {
            SlotEntry::Backend(owner) => match Arc::try_unwrap(owner) {
                Ok(owner) => owner,
                Err(owner) => {
                    state.slots.insert(
                        slot_id,
                        Slot {
                            handle: slot_handle,
                            kind: slot_kind,
                            generation: slot_generation,
                            entry: SlotEntry::Backend(owner),
                        },
                    );
                    return Err(MirNativeCursorError::internal(
                        "Source resource owner is still aliased",
                    ));
                }
            },
            SlotEntry::Cursor(cursor) => {
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Cursor(cursor),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource capability payload mismatch",
                ));
            }
        };
        let owner = match owner.into_inner() {
            Ok(owner) => owner,
            Err(poison) => {
                let owner = poison.into_inner();
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Backend(Arc::new(Mutex::new(owner))),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource owner lock is poisoned",
                ));
            }
        };
        let lease = SourceOwnedLease {
            owner: Arc::new(Mutex::new(owner)),
            consumed: consumed.clone(),
            kind: slot_kind,
        };
        Ok((consumed, lease))
    }

    /// Convenience form for a checked transfer whose logical payload is
    /// already available at the native boundary.
    pub fn take_owned_with_value(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
        value: MirRuntimeValue,
    ) -> Result<(SourceResourceHandle, SourceOwnedValue), MirNativeCursorError> {
        let (consumed, lease) = self.take_owned(handle, raw, expected_kind)?;
        Ok((consumed, lease.with_value(value)))
    }
    /// Consume a checked Core owner after deriving its Source kind from the
    /// canonical owner registration and nominal type. Kind validation happens
    /// before the capability is taken, while `take_owned_with_value` validates
    /// the live `(handle, raw, kind)` triple and sole-owner state.
    pub fn take_owned_with_core_owner_type(
        &self,
        handle: MirHandleId,
        raw: i64,
        registration: &MirCoreOwner,
        owner_type: &MirType,
        value: MirRuntimeValue,
    ) -> Result<(SourceResourceHandle, SourceOwnedValue), MirNativeCursorError> {
        let expected_kind = SourceResourceKind::from_core_owner_type(registration, owner_type)
            .map_err(MirNativeCursorError::internal)?;
        self.take_owned_with_value(handle, raw, &expected_kind, value)
    }

    /// Commit one physical lease into a fresh destination slot.  The source
    /// receipt inside the packet is stale regardless of destination success;
    /// on failure the detached owner drops after this method has left the
    /// arena lock.
    pub fn adopt_owned(
        &self,
        target_handle: MirHandleId,
        expected_kind: &SourceResourceKind,
        owned: SourceOwnedValue,
    ) -> SourceOwnedTransferOutcome {
        let SourceOwnedValue {
            value,
            owner,
            consumed,
            kind,
        } = owned;
        if &kind != expected_kind {
            drop(owner);
            return SourceOwnedTransferOutcome {
                consumed,
                result: Err(MirNativeCursorError::internal(
                    "Source owned payload kind mismatch",
                )),
            };
        }
        let result = self
            .register_backend_arc(target_handle, kind, owner)
            .map(|capability| (capability, value))
            .map_err(MirNativeCursorError::internal);
        SourceOwnedTransferOutcome { consumed, result }
    }

    /// Consume a live file-reader capability into its typed Rust owner.
    /// Aliased owners are rejected and the slot remains installed.
    pub fn take_file_reader(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceFileReader, MirNativeCursorError> {
        self.take_file_owner(handle, raw, SourceResourceKind::LinesFile)
            .map(|(_, reader)| reader)
    }

    /// Consume a live file-writer capability into its typed Rust owner.
    /// Aliased owners are rejected and the slot remains installed.
    pub fn take_file_writer(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceFileWriter, MirNativeCursorError> {
        self.take_file_owner(handle, raw, SourceResourceKind::FileWriter)
            .map(|(_, writer)| writer)
    }

    fn take_file_owner<T>(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: SourceResourceKind,
    ) -> Result<(SourceResourceHandle, T), MirNativeCursorError>
    where
        T: FromBackendOwner,
    {
        let mut state = self.lock().map_err(MirNativeCursorError::internal)?;
        let (slot_id, generation) = Self::decode_raw(raw)
            .map_err(MirNativeCursorError::internal)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("stale or unknown Source resource capability")
            })?;
        if slot.generation != generation
            || slot.handle != handle
            || slot.kind != expected_kind
        {
            return Err(MirNativeCursorError::internal(
                "Source resource generation, handle, or kind mismatch",
            ));
        }
        let source = SourceResourceHandle {
            handle: slot.handle,
            raw,
            kind: slot.kind.clone(),
            generation: slot.generation,
        };
        let slot = state
            .slots
            .remove(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("Source resource capability disappeared")
            })?;
        let Slot {
            handle: slot_handle,
            kind: slot_kind,
            generation: slot_generation,
            entry,
        } = slot;
        let owner = match entry {
            SlotEntry::Backend(owner) => match Arc::try_unwrap(owner) {
                Ok(owner) => owner,
                Err(owner) => {
                    state.slots.insert(
                        slot_id,
                        Slot {
                            handle: slot_handle,
                            kind: slot_kind,
                            generation: slot_generation,
                            entry: SlotEntry::Backend(owner),
                        },
                    );
                    return Err(MirNativeCursorError::internal(
                        "Source resource owner is still aliased",
                    ));
                }
            },
            SlotEntry::Cursor(cursor) => {
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Cursor(cursor),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource capability payload mismatch",
                ));
            }
        };
        let owner = match owner.into_inner() {
            Ok(owner) => owner,
            Err(poison) => {
                let owner = poison.into_inner();
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Backend(Arc::new(Mutex::new(owner))),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource owner lock is poisoned",
                ));
            }
        };
        match T::from_backend_owner(owner) {
            Ok(owner) => Ok((source, owner)),
            Err(owner) => {
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Backend(Arc::new(Mutex::new(owner))),
                    },
                );
                Err(MirNativeCursorError::internal(
                    "Source resource capability payload mismatch",
                ))
            }
        }
    }

    pub fn file_writer_write_line(
        &self,
        handle: MirHandleId,
        raw: i64,
        line: &String,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::FileWriter)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::FileWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source file writer capability payload mismatch",
            ));
        };
        crate::enc_stream::source_file_writer_write_line(&mut writer.0, line)
            .map_err(file_error)
    }

    pub fn file_writer_flush(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::FileWriter)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::FileWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source file writer capability payload mismatch",
            ));
        };
        crate::enc_stream::source_file_writer_flush(&mut writer.0).map_err(file_error)
    }

    pub fn file_writer_path(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<String, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::FileWriter)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::FileWriter(writer) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source file writer capability payload mismatch",
            ));
        };
        Ok(crate::enc_stream::source_file_writer_path(&writer.0))
    }

    pub fn file_reader_read_line(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<String>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::LinesFile)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::File(reader) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source file reader capability payload mismatch",
            ));
        };
        crate::enc_stream::source_file_next_line(&mut reader.0)
            .map_err(file_error)
    }

    pub fn file_reader_path(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<String, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::LinesFile)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::File(reader) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source file reader capability payload mismatch",
            ));
        };
        Ok(reader.0.path.clone())
    }

    pub fn stdin_read_line(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<String>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::LinesStdin)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::Stdin(reader) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source stdin capability payload mismatch",
            ));
        };
        crate::enc_stream::source_stdin_next_line(reader)
            .map_err(process_error)
    }

    pub fn channel_sender_send(
        &self,
        handle: MirHandleId,
        raw: i64,
        value: MirRuntimeValue,
    ) -> Result<bool, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelSender)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelSender(sender) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel sender capability payload mismatch",
            ));
        };
        Ok(sender.send(value))
    }

    pub fn channel_sender_close(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelSender)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelSender(sender) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel sender capability payload mismatch",
            ));
        };
        sender.close();
        Ok(())
    }

    pub fn channel_receiver_receive(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<MirRuntimeValue>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(receiver.receive())
    }

    pub fn channel_receiver_try_receive(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<MirRuntimeValue>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(receiver.try_receive())
    }

    pub fn channel_receiver_is_timer(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_timer())
    }

    pub fn channel_receiver_is_interval(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_interval())
    }

    pub fn channel_receiver_is_cancelled(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_cancelled())
    }

    pub fn channel_receiver_is_ready(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_ready())
    }

    pub fn channel_receiver_delay_ms(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<i64, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(receiver.delay_ms())
    }

    pub fn channel_receiver_close(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        receiver.close();
        Ok(())
    }

    fn channel_receiver_flag(
        &self,
        handle: MirHandleId,
        raw: i64,
        flag: impl FnOnce(&jet_codegen::scheduler::JetSchedulerChannel<MirRuntimeValue>) -> bool,
    ) -> Result<bool, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(flag(receiver))
    }

    pub fn encoding_writer_write(
        &self,
        handle: MirHandleId,
        raw: i64,
        value: &MirRuntimeValue,
    ) -> Result<(), MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        match &mut writer.0 {
            EncodingWriter::Json(writer) => {
                let event = data_event_from_value(value)?;
                crate::enc_stream::runtime::enc_json_writer_write(writer, event)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Jsonl(writer) => {
                let tree = data_tree_from_value(value)?;
                crate::enc_stream::runtime::enc_jsonl_writer_write(writer, tree)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Csv(writer) => {
                let row = csv_row_from_value(value)?;
                crate::enc_stream::runtime::enc_csv_writer_write(writer, row)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Xml(writer) => {
                let tree = data_tree_from_value(value)?;
                crate::enc_stream::runtime::enc_xml_writer_write(writer, tree)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Cbor(writer) => {
                let event = data_event_from_value(value)?;
                crate::enc_stream::runtime::enc_cbor_writer_write(writer, event)
                    .map_err(|error| encoding_error(&error))
            }
        }
    }

    pub fn encoding_writer_flush(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        match &mut writer.0 {
            EncodingWriter::Json(writer) => crate::enc_stream::runtime::enc_json_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Jsonl(writer) => crate::enc_stream::runtime::enc_jsonl_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Csv(writer) => crate::enc_stream::runtime::enc_csv_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Xml(writer) => crate::enc_stream::runtime::enc_xml_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Cbor(writer) => crate::enc_stream::runtime::enc_cbor_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
        }
    }

    pub fn encoding_writer_finish(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        match &mut writer.0 {
            EncodingWriter::Json(writer) => crate::enc_stream::runtime::enc_json_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Jsonl(writer) => crate::enc_stream::runtime::enc_jsonl_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Csv(writer) => crate::enc_stream::runtime::enc_csv_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Xml(writer) => crate::enc_stream::runtime::enc_xml_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Cbor(writer) => crate::enc_stream::runtime::enc_cbor_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
        }
    }

    pub fn encoding_writer_path(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<String, MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        let path = match &writer.0 {
            EncodingWriter::Json(writer) => &writer.output.path,
            EncodingWriter::Jsonl(writer) => &writer.json.output.path,
            EncodingWriter::Csv(writer) => &writer.output.path,
            EncodingWriter::Xml(writer) => &writer.output.path,
            EncodingWriter::Cbor(writer) => &writer.output.path,
        };
        Ok(path.clone())
    }

    /// Build a checked cursor from a registered backend owner and retain the
    /// resulting non-serializable cursor in this same slot authority.
    ///
    /// A by-value source returns its stale source receipt even when iterator
    /// construction or cursor-slot installation fails.  Callers must carry
    /// that receipt through the native transfer outcome; they must not probe
    /// this arena after an acquisition failure.
    pub fn init_cursor(
        &self,
        key: &NativeLoopResourceKey,
        step_value: i64,
        has_step: bool,
        by_value: bool,
    ) -> SourceCursorInitOutcome {
        let acquisition = jet_codegen::Codegen::NativeLoopCursor::init_from_resource_factory(
            self, key, step_value, has_step, by_value,
        );
        let consumed = acquisition
            .consumed
            .as_ref()
            .map(Self::source_receipt_from_loop_key);
        let value = match acquisition.result {
            Ok(value) => value,
            Err(error) => {
                return SourceCursorInitOutcome {
                    consumed,
                    result: Err(error),
                }
            }
        };
        let MirRuntimeValue::NativeCursor(cursor) = value.clone() else {
            return SourceCursorInitOutcome {
                consumed,
                result: Err(MirNativeCursorError::internal(
                    "canonical resource cursor did not produce a native cursor",
                )),
            };
        };
        let capability = match self.insert_cursor(loop_cursor_handle_id(), cursor) {
            Ok(capability) => capability,
            Err(error) => {
                return SourceCursorInitOutcome {
                    consumed,
                    result: Err(MirNativeCursorError::internal(error)),
                }
            }
        };
        SourceCursorInitOutcome {
            consumed,
            result: Ok((capability, value)),
        }
    }

    fn source_receipt_from_loop_key(key: &NativeLoopResourceKey) -> SourceResourceHandle {
        let (_, generation) = Self::decode_raw(key.raw)
            .expect("committed native loop receipt must contain a valid capability");
        let kind = SourceResourceKind::from_loop_source(&key.source_kind)
            .expect("committed native loop receipt must contain a resource kind");
        SourceResourceHandle {
            handle: key.handle,
            raw: key.raw,
            kind,
            generation,
        }
    }

    fn validate_slot<'a>(
        state: &'a ArenaState,
        key: &NativeLoopResourceKey,
        expected: &SourceResourceKind,
    ) -> Result<&'a Slot, String> {
        let (slot_id, generation) = Self::decode_raw(key.raw)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
        if slot.generation != generation || slot.handle != key.handle || &slot.kind != expected {
            return Err("Source resource handle, generation, or kind mismatch".to_string());
        }
        if !expected.matches_loop_source(&key.source_kind) {
            return Err("Source resource kind does not match checked loop source".to_string());
        }
        Ok(slot)
    }

    fn take_owner(
        &self,
        key: &NativeLoopResourceKey,
        expected: SourceResourceKind,
    ) -> NativeLoopResourceOutcome<Arc<Mutex<BackendOwner>>> {
        if !expected.matches_loop_source(&key.source_kind) {
            return NativeLoopResourceOutcome {
                consumed: None,
                result: Err(MirNativeCursorError::internal(
                    "Source resource kind does not match checked loop source",
                )),
            };
        }
        match self.take_owned(key.handle, key.raw, &expected) {
            Ok((consumed, lease)) => NativeLoopResourceOutcome {
                consumed: Some(NativeLoopResourceKey {
                    handle: consumed.handle,
                    raw: consumed.raw,
                    source_kind: key.source_kind.clone(),
                }),
                result: Ok(lease.into_owner()),
            },
            Err(error) => NativeLoopResourceOutcome {
                consumed: None,
                result: Err(error),
            },
        }
    }

    fn borrow_owner(
        &self,
        key: &NativeLoopResourceKey,
        expected: SourceResourceKind,
    ) -> Result<Arc<Mutex<BackendOwner>>, MirNativeCursorError> {
        let state = self.lock().map_err(MirNativeCursorError::internal)?;
        let slot = Self::validate_slot(&state, key, &expected)
            .map_err(MirNativeCursorError::internal)?;
        match &slot.entry {
            SlotEntry::Backend(owner) => Ok(Arc::clone(owner)),
            SlotEntry::Cursor(_) => Err(MirNativeCursorError::internal(
                "native cursor is not a backend producer",
            )),
        }
    }

    fn iterator_for(
        &self,
        key: &NativeLoopResourceKey,
        expected: SourceResourceKind,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        let acquisition = if by_value {
            self.take_owner(key, expected)
        } else {
            NativeLoopResourceOutcome {
                consumed: None,
                result: self.borrow_owner(key, expected),
            }
        };
        let NativeLoopResourceOutcome { consumed, result } = acquisition;
        NativeLoopResourceOutcome {
            consumed,
            result: result.and_then(|owner| resource_iterator(owner, &key.source_kind)),
        }
    }
}

impl NativeLoopResourceFactory for SourceResourceArena {
    fn plain_stream(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::PlainStream, by_value)
    }

    fn lines_file(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::LinesFile, by_value)
    }

    fn lines_stdin(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::LinesStdin, by_value)
    }

    fn lines_process_stream(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::LinesProcessStream, by_value)
    }

    fn channel_receiver(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::ChannelReceiver, by_value)
    }

    fn encoding_reader(
        &self,
        key: &NativeLoopResourceKey,
        reader_type: &str,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(
            key,
            SourceResourceKind::EncodingReader {
                reader_type: reader_type.to_string(),
            },
            by_value,
        )
    }
}

/// Retain one typed owner lease in every cursor.  A by-value source removes
/// the arena slot before this function, while a borrow leaves the slot in
/// place; both paths share this same `Arc` lease and therefore stay lazy
/// without a second owner or a materialized fallback.
fn resource_iterator(
    owner: Arc<Mutex<BackendOwner>>,
    source: &MirLoopSourceKind,
) -> Result<NativeIter, MirNativeCursorError> {
    let valid = owner
        .lock()
        .map_err(|_| cursor_error("Source resource owner lock is poisoned"))?
        .kind()
        .as_loop_source()
        .ok()
        .is_some_and(|kind| &kind == source);
    if !valid {
        return Err(cursor_error(
            "Source resource owner does not match checked loop source",
        ));
    }
    match source {
        MirLoopSourceKind::Plain => Ok(Box::new(PlainIter { owner })),
        MirLoopSourceKind::LinesFile => Ok(Box::new(FileIter { owner })),
        MirLoopSourceKind::LinesStdin => Ok(Box::new(StdinIter { owner })),
        MirLoopSourceKind::LinesProcessStream => Ok(Box::new(ProcessIter { owner })),
        MirLoopSourceKind::ChannelReceiver => Ok(Box::new(ChannelIter { owner })),
        MirLoopSourceKind::EncodingReader { .. } => Ok(Box::new(EncodingIter { owner })),
        MirLoopSourceKind::Chars | MirLoopSourceKind::Iterable { .. } => Err(cursor_error(
            "Source resource owner does not match checked loop source",
        )),
    }
}
fn runtime_error(
    type_name: impl Into<String>,
    fields: Vec<(String, MirRuntimeValue)>,
) -> MirNativeCursorError {
    MirNativeCursorError::from_runtime_value(MirRuntimeValue::Struct {
        type_name: type_name.into(),
        fields,
    })
}

fn mir_named(name: &str) -> jet_foundation::MIR::MirType {
    jet_foundation::MIR::MirType::from_kind(
        jet_foundation::MIR::MirTypeKind::Apply {
            name: jet_foundation::MIR::MirNominalRef::from_name(name),
            args: Vec::new(),
        },
    )
}

fn mir_absent_nominal(name: &str) -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: mir_named(name),
    }
}

fn mir_absent_int() -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: jet_foundation::MIR::MirType::from_kind(
            jet_foundation::MIR::MirTypeKind::Int,
        ),
    }
}

fn optional_int<E>(value: &Result<i64, E>) -> MirRuntimeValue {
    match value {
        Ok(value) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(*value))),
        Err(_) => mir_absent_int(),
    }
}


fn mir_enum(type_name: &str, variant: &str) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn encoding_format_value(
    format: crate::enc_stream::runtime::jet_std::EncodingFormat,
) -> MirRuntimeValue {
    mir_enum("EncodingFormat", format.as_str())
}

fn encoding_kind_value(
    kind: &crate::enc_stream::runtime::jet_std::EncodingErrorKind,
) -> MirRuntimeValue {
    use crate::enc_stream::runtime::jet_std::EncodingErrorKind;
    let variant = match kind {
        EncodingErrorKind::Syntax => "Syntax",
        EncodingErrorKind::Truncated => "Truncated",
        EncodingErrorKind::Unsupported => "Unsupported",
        EncodingErrorKind::Limit => "Limit",
        EncodingErrorKind::IO => "IO",
        EncodingErrorKind::State => "State",
    };
    mir_enum("EncodingErrorKind", variant)
}

fn encoding_error(
    error: &crate::enc_stream::runtime::jet_std::EncodingError,
) -> MirNativeCursorError {
    let cause = match &error.cause {
        Ok(cause) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Struct {
            type_name: "EncodingCause".to_string(),
            fields: vec![
                ("kind".to_string(), MirRuntimeValue::String(cause.kind.clone())),
                ("os_code".to_string(), optional_int(&cause.os_code)),
                (
                    "message".to_string(),
                    MirRuntimeValue::String(cause.message.clone()),
                ),
            ],
        })),
        Err(_) => mir_absent_nominal("EncodingCause"),
    };
    runtime_error(
        "EncodingError",
        vec![
            ("format".to_string(), encoding_format_value(error.format)),
            ("kind".to_string(), encoding_kind_value(&error.kind)),
            ("byte_offset".to_string(), MirRuntimeValue::Int(error.byte_offset)),
            ("line".to_string(), optional_int(&error.line)),
            ("column".to_string(), optional_int(&error.column)),
            ("path".to_string(), MirRuntimeValue::String(error.path.clone())),
            (
                "reason".to_string(),
                MirRuntimeValue::String(error.reason.clone()),
            ),
            ("cause".to_string(), cause),
        ],
    )
}


fn process_error(
    error: crate::ProcessPrelude::process_prelude::jet_std::IOError,
) -> MirNativeCursorError {
    MirNativeCursorError::from_runtime_value(crate::Process::mir_io_error(error))
}

fn file_error(error: crate::enc_stream::SourceIoError) -> MirNativeCursorError {
    MirNativeCursorError::from_runtime_value(crate::Process::mir_io_error(error))
}



type OwnerLease = Arc<Mutex<BackendOwner>>;

fn owner_lock(
    owner: &OwnerLease,
) -> Result<MutexGuard<'_, BackendOwner>, MirNativeCursorError> {
    owner
        .lock()
        .map_err(|_| cursor_error("Source resource owner lock is poisoned"))
}

struct PlainIter {
    owner: OwnerLease,
}

impl Iterator for PlainIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        match &mut *owner {
            BackendOwner::PlainStream(stream) => stream.pull_checked().map(Ok),
            _ => Some(Err(cursor_error("plain stream owner payload mismatch"))),
        }
    }
}

struct FileIter {
    owner: OwnerLease,
}

impl Iterator for FileIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::File(reader) = &mut *owner else {
            return Some(Err(cursor_error("file owner payload mismatch")));
        };
        match crate::enc_stream::source_file_next_line(&mut reader.0) {
            Ok(Some(value)) => Some(Ok(MirRuntimeValue::String(value))),
            Ok(None) => None,
            Err(error) => Some(Err(file_error(error))),
        }
    }
}

struct StdinIter {
    owner: OwnerLease,
}

impl Iterator for StdinIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::Stdin(reader) = &mut *owner else {
            return Some(Err(cursor_error("stdin owner payload mismatch")));
        };
        match crate::enc_stream::source_stdin_next_line(reader) {
            Ok(Some(value)) => Some(Ok(MirRuntimeValue::String(value))),
            Ok(None) => None,
            Err(error) => Some(Err(process_error(error))),
        }
    }
}

struct ProcessIter {
    owner: OwnerLease,
}

impl Iterator for ProcessIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::Process(reader) = &mut *owner else {
            return Some(Err(cursor_error("process stream owner payload mismatch")));
        };
        match crate::ProcessPrelude::source_process_stream_next_line(&reader.0) {
            Ok(Some(value)) => Some(Ok(MirRuntimeValue::String(value))),
            Ok(None) => None,
            Err(error) => Some(Err(process_error(error))),
        }
    }
}

struct ChannelIter {
    owner: OwnerLease,
}

impl Iterator for ChannelIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::ChannelReceiver(receiver) = &mut *owner else {
            return Some(Err(cursor_error("channel owner payload mismatch")));
        };
        receiver.receive().map(Ok)
    }
}

struct EncodingIter {
    owner: OwnerLease,
}

impl Iterator for EncodingIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::Encoding(reader) = &mut *owner else {
            return Some(Err(cursor_error("encoding owner payload mismatch")));
        };
        match encoding_next(&mut reader.0) {
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        }
    }
}

fn encoding_next(
    reader: &mut EncodingReader,
) -> Result<Option<MirRuntimeValue>, MirNativeCursorError> {
    match reader {
        EncodingReader::Json(reader) => crate::enc_stream::runtime::enc_json_reader_next(reader)
            .map(|value| value.map(data_event_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Jsonl(reader) => crate::enc_stream::runtime::enc_jsonl_reader_next(reader)
            .map(|value| value.map(data_tree_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Csv(reader) => crate::enc_stream::runtime::enc_csv_reader_next(reader)
            .map(|value| value.map(csv_row_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Xml(reader) => crate::enc_stream::runtime::enc_xml_reader_next(reader)
            .map(|value| value.map(data_tree_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Cbor(reader) => crate::enc_stream::runtime::enc_cbor_reader_next(reader)
            .map(|value| value.map(data_event_value))
            .map_err(|error| encoding_error(&error)),
    }
}

fn data_event_value(event: crate::enc_stream::runtime::jet_std::DataEvent) -> MirRuntimeValue {
    use crate::enc_stream::runtime::jet_std::DataEvent;
    let (variant, args) = match event {
        DataEvent::Null => ("Null", Vec::new()),
        DataEvent::Bool(value) => ("Bool", vec![MirRuntimeValue::Bool(value)]),
        DataEvent::Int(value) => ("Int", vec![MirRuntimeValue::Int(value)]),
        DataEvent::Float(value) => (
            "Float",
            vec![MirRuntimeValue::Float { value, f32: false }],
        ),
        DataEvent::Number(value) => ("Number", vec![MirRuntimeValue::String(value)]),
        DataEvent::Text(value) => ("Text", vec![MirRuntimeValue::String(value)]),
        DataEvent::Bytes(value) => ("Bytes", vec![MirRuntimeValue::Bytes(value)]),
        DataEvent::ArrayStart => ("ArrayStart", Vec::new()),
        DataEvent::ArrayEnd => ("ArrayEnd", Vec::new()),
        DataEvent::ObjectStart => ("ObjectStart", Vec::new()),
        DataEvent::Key(value) => ("Key", vec![MirRuntimeValue::String(value)]),
        DataEvent::ObjectEnd => ("ObjectEnd", Vec::new()),
    };
    MirRuntimeValue::Enum {
        type_name: "DataEvent".to_string(),
        variant: variant.to_string(),
        args: args.into_iter().map(|value| (None, value)).collect(),
    }
}

fn data_tree_value(tree: crate::enc_stream::runtime::jet_std::DataTree) -> MirRuntimeValue {
    use crate::enc_stream::runtime::jet_std::DataTree;
    let (variant, args) = match tree {
        DataTree::Null => ("Null", Vec::new()),
        DataTree::Bool(value) => ("Bool", vec![MirRuntimeValue::Bool(value)]),
        DataTree::Int(value) => ("Int", vec![MirRuntimeValue::Int(value)]),
        DataTree::Float(value) => (
            "Float",
            vec![MirRuntimeValue::Float { value, f32: false }],
        ),
        DataTree::Number(value) => ("Number", vec![MirRuntimeValue::String(value)]),
        DataTree::TypedText(value) => ("TypedText", vec![MirRuntimeValue::String(value)]),
        DataTree::Text(value) => ("Text", vec![MirRuntimeValue::String(value)]),
        DataTree::Bytes(value) => ("Bytes", vec![MirRuntimeValue::Bytes(value)]),
        DataTree::Array(values) => (
            "Array",
            vec![MirRuntimeValue::List(
                values.into_iter().map(data_tree_value).collect(),
            )],
        ),
        DataTree::Object(values) => (
            "Object",
            vec![MirRuntimeValue::List(
                values
                    .into_iter()
                    .map(|(key, value)| MirRuntimeValue::Struct {
                        type_name: "Tuple".to_string(),
                        fields: vec![
                            ("key".to_string(), MirRuntimeValue::String(key)),
                            ("value".to_string(), data_tree_value(value)),
                        ],
                    })
                    .collect(),
            )],
        ),
    };
    MirRuntimeValue::Enum {
        type_name: "DataTree".to_string(),
        variant: variant.to_string(),
        args: args.into_iter().map(|value| (None, value)).collect(),
    }
}

fn csv_row_value(row: crate::enc_stream::runtime::jet_std::CSVRow) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "CSVRow".to_string(),
        fields: vec![
            (
                "fields".to_string(),
                MirRuntimeValue::List(
                    row.fields
                        .into_iter()
                        .map(MirRuntimeValue::String)
                        .collect(),
                ),
            ),
            ("line".to_string(), MirRuntimeValue::Int(row.line)),
        ],
    }
}

fn data_event_from_value(
    value: &MirRuntimeValue,
) -> Result<crate::enc_stream::runtime::jet_std::DataEvent, MirNativeCursorError> {
    use crate::enc_stream::runtime::jet_std::DataEvent;
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(MirNativeCursorError::internal(
            "encoding writer expects a DataEvent carrier",
        ));
    };
    if type_name != "DataEvent" {
        return Err(MirNativeCursorError::internal(format!(
            "encoding writer expects DataEvent, got `{type_name}`"
        )));
    }
    let arg = |index: usize| {
        args.get(index)
            .map(|(_, value)| value)
            .ok_or_else(|| MirNativeCursorError::internal("DataEvent carrier is missing an argument"))
    };
    let unit = || {
        if args.is_empty() {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(
                "unit DataEvent variant has unexpected arguments",
            ))
        }
    };
    match variant.as_str() {
        "Null" => {
            unit()?;
            Ok(DataEvent::Null)
        }
        "ArrayStart" => {
            unit()?;
            Ok(DataEvent::ArrayStart)
        }
        "ArrayEnd" => {
            unit()?;
            Ok(DataEvent::ArrayEnd)
        }
        "ObjectStart" => {
            unit()?;
            Ok(DataEvent::ObjectStart)
        }
        "ObjectEnd" => {
            unit()?;
            Ok(DataEvent::ObjectEnd)
        }
        "Bool" => match arg(0)? {
            MirRuntimeValue::Bool(value) if args.len() == 1 => Ok(DataEvent::Bool(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Bool carrier has the wrong payload",
            )),
        },
        "Int" => match arg(0)? {
            MirRuntimeValue::Int(value) if args.len() == 1 => Ok(DataEvent::Int(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Int carrier has the wrong payload",
            )),
        },
        "Float" => match arg(0)? {
            MirRuntimeValue::Float { value, .. } if args.len() == 1 => {
                Ok(DataEvent::Float(*value))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Float carrier has the wrong payload",
            )),
        },
        "Number" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataEvent::Number(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Number carrier has the wrong payload",
            )),
        },
        "Text" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataEvent::Text(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Text carrier has the wrong payload",
            )),
        },
        "Key" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataEvent::Key(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Key carrier has the wrong payload",
            )),
        },
        "Bytes" => match arg(0)? {
            MirRuntimeValue::Bytes(value) if args.len() == 1 => {
                Ok(DataEvent::Bytes(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Bytes carrier has the wrong payload",
            )),
        },
        _ => Err(MirNativeCursorError::internal(format!(
            "unknown DataEvent variant `{variant}`"
        ))),
    }
}

fn data_tree_from_value(
    value: &MirRuntimeValue,
) -> Result<crate::enc_stream::runtime::jet_std::DataTree, MirNativeCursorError> {
    use crate::enc_stream::runtime::jet_std::DataTree;
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(MirNativeCursorError::internal(
            "encoding writer expects a DataTree carrier",
        ));
    };
    if type_name != "DataTree" {
        return Err(MirNativeCursorError::internal(format!(
            "encoding writer expects DataTree, got `{type_name}`"
        )));
    }
    let arg = |index: usize| {
        args.get(index)
            .map(|(_, value)| value)
            .ok_or_else(|| MirNativeCursorError::internal("DataTree carrier is missing an argument"))
    };
    let unit = || {
        if args.is_empty() {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(
                "unit DataTree variant has unexpected arguments",
            ))
        }
    };
    match variant.as_str() {
        "Null" => {
            unit()?;
            Ok(DataTree::Null)
        }
        "Bool" => match arg(0)? {
            MirRuntimeValue::Bool(value) if args.len() == 1 => Ok(DataTree::Bool(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Bool carrier has the wrong payload",
            )),
        },
        "Int" => match arg(0)? {
            MirRuntimeValue::Int(value) if args.len() == 1 => Ok(DataTree::Int(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Int carrier has the wrong payload",
            )),
        },
        "Float" => match arg(0)? {
            MirRuntimeValue::Float { value, .. } if args.len() == 1 => {
                Ok(DataTree::Float(*value))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Float carrier has the wrong payload",
            )),
        },
        "Number" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataTree::Number(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Number carrier has the wrong payload",
            )),
        },
        "TypedText" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataTree::TypedText(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.TypedText carrier has the wrong payload",
            )),
        },
        "Text" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataTree::Text(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Text carrier has the wrong payload",
            )),
        },
        "Bytes" => match arg(0)? {
            MirRuntimeValue::Bytes(value) if args.len() == 1 => {
                Ok(DataTree::Bytes(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Bytes carrier has the wrong payload",
            )),
        },
        "Array" => {
            let MirRuntimeValue::List(values) = arg(0)? else {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Array carrier has the wrong payload",
                ));
            };
            if args.len() != 1 {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Array carrier has unexpected arguments",
                ));
            }
            values
                .iter()
                .map(data_tree_from_value)
                .collect::<Result<Vec<_>, _>>()
                .map(DataTree::Array)
        }
        "Object" => {
            let MirRuntimeValue::List(values) = arg(0)? else {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Object carrier has the wrong payload",
                ));
            };
            if args.len() != 1 {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Object carrier has unexpected arguments",
                ));
            }
            let mut entries = Vec::with_capacity(values.len());
            for value in values {
                let MirRuntimeValue::Struct { type_name, fields } = value else {
                    return Err(MirNativeCursorError::internal(
                        "DataTree.Object carrier contains a non-tuple entry",
                    ));
                };
                if type_name != "Tuple" {
                    return Err(MirNativeCursorError::internal(
                        "DataTree.Object carrier entry is not a Tuple",
                    ));
                }
                let key = named_field(fields, "key")?;
                let value = named_field(fields, "value")?;
                let MirRuntimeValue::String(key) = key else {
                    return Err(MirNativeCursorError::internal(
                        "DataTree.Object carrier key is not a String",
                    ));
                };
                entries.push((key.clone(), data_tree_from_value(value)?));
            }
            Ok(DataTree::Object(entries))
        }
        _ => Err(MirNativeCursorError::internal(format!(
            "unknown DataTree variant `{variant}`"
        ))),
    }
}

fn named_field<'a>(
    fields: &'a [(String, MirRuntimeValue)],
    name: &str,
) -> Result<&'a MirRuntimeValue, MirNativeCursorError> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| {
            MirNativeCursorError::internal(format!("carrier is missing `{name}` field"))
        })
}

fn csv_row_from_value(value: &MirRuntimeValue) -> Result<Vec<String>, MirNativeCursorError> {
    let fields = match value {
        MirRuntimeValue::List(values) => values,
        MirRuntimeValue::Struct { type_name, fields } if type_name == "CSVRow" => {
            match named_field(fields, "fields")? {
                MirRuntimeValue::List(values) => values,
                _ => {
                    return Err(MirNativeCursorError::internal(
                        "CSVRow.fields is not a list",
                    ))
                }
            }
        }
        _ => {
            return Err(MirNativeCursorError::internal(
                "CSV writer expects a CSVRow or string list",
            ))
        }
    };
    fields
        .iter()
        .map(|value| match value {
            MirRuntimeValue::String(value) => Ok(value.clone()),
            _ => Err(MirNativeCursorError::internal(
                "CSV writer row contains a non-string field",
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enc_stream::runtime::jet_std::{
        EncodingCause, EncodingError, EncodingErrorKind, EncodingFormat,
    };
    use crate::ProcessPrelude::process_prelude::jet_std::{
        IOContext, IOError, IOOperation, ProcessResourceLimit,
    };
    use jet_foundation::Outcome::JetAbsent;
    use std::path::PathBuf;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    fn test_handle(value: u64) -> MirHandleId {
        MirHandleId(value)
    }

    fn test_path(label: &str) -> PathBuf {
        static NEXT_PATH: AtomicU32 = AtomicU32::new(0);
        let suffix = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        let root = std::env::var_os("JET_SCRATCH")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".cache").join("jet-luna"))
            })
            .expect("JET_SCRATCH or HOME must select the Jet scratch directory");
        std::fs::create_dir_all(&root).expect("create Jet scratch directory");
        root.join(format!(
            "source-resources-{label}-{}-{suffix}.tmp",
            std::process::id()
        ))
    }

    struct ReentrantLeaseDrop {
        lease: Option<SourceResourceLease>,
        arena: SourceResourceArena,
        reentered_after_release: Arc<AtomicUsize>,
    }

    impl Drop for ReentrantLeaseDrop {
        fn drop(&mut self) {
            drop(self.lease.take());
            if self.arena.is_retired().unwrap_or(false) {
                self.reentered_after_release.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    fn value_of(error: MirNativeCursorError) -> MirRuntimeValue {
        match error {
            MirNativeCursorError::Value(value) => *value,
            MirNativeCursorError::Internal(message) => {
                panic!("expected a checked value error, got adapter failure: {message}")
            }
        }
    }

    #[test]
    fn source_file_iterator_is_lazy_and_owner_is_typed() {
        let path = test_path("lazy");
        std::fs::write(&path, b"").expect("create empty source file");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file(test_handle(1), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed reader");
        let key = capability.native_loop_key().expect("file loop key");
        let mut iterator = arena
            .lines_file(&key, false)
            .result
            .expect("borrow typed file owner");

        // No read occurs during cursor initialization.  Data written after
        // admission is visible to the first pull.
        std::fs::write(&path, b"late\n").expect("publish source data");
        assert_eq!(
            iterator.next(),
            Some(Ok(MirRuntimeValue::String("late".to_string())))
        );
        assert_eq!(iterator.next(), None);
        let _ = arena.release_capability(&capability);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn borrowed_file_iterator_survives_release_and_moved_close() {
        let path = test_path("lease");
        std::fs::write(&path, b"borrowed\nmoved\n").expect("create source file");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file(test_handle(2), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed reader");
        let key = capability.native_loop_key().expect("file loop key");
        let mut borrowed = arena
            .lines_file(&key, false)
            .result
            .expect("borrow typed file owner");
        arena
            .release_capability(&capability)
            .expect("release source slot while lease is live");
        assert_eq!(
            borrowed.next(),
            Some(Ok(MirRuntimeValue::String("borrowed".to_string())))
        );

        let moved_capability = arena
            .open_file(test_handle(3), path.to_str().expect("utf8 scratch path"))
            .expect("admit second typed reader");
        let moved_key = moved_capability.native_loop_key().expect("file loop key");
        let mut moved = arena
            .lines_file(&moved_key, true)
            .result
            .expect("move typed file owner");
        assert!(arena.release_capability(&moved_capability).is_err());
        assert_eq!(
            moved.next(),
            Some(Ok(MirRuntimeValue::String("borrowed".to_string())))
        );
        drop(borrowed);
        drop(moved);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_file_writer_is_a_typed_release_lease() {
        let path = test_path("writer");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file_writer(test_handle(6), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed writer");
        assert_eq!(capability.kind, SourceResourceKind::FileWriter);
        assert!(capability.native_loop_key().is_err());
        let line = "writer line".to_string();
        arena
            .file_writer_write_line(capability.handle, capability.raw, &line)
            .expect("write through typed writer lease");
        arena
            .file_writer_flush(capability.handle, capability.raw)
            .expect("flush through typed writer lease");
        assert_eq!(
            arena
                .file_writer_path(capability.handle, capability.raw)
                .expect("writer path"),
            path.to_str().expect("utf8 scratch path")
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read writer output"),
            "writer line\n"
        );
        arena
            .release_resource(capability.handle, capability.raw)
            .expect("release typed writer");
        assert!(arena.release_resource(capability.handle, capability.raw).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_capability_lookup_returns_stored_generation_and_kind() {
        let arena = SourceResourceArena::new();
        let capability = arena.register_stdin(test_handle(11)).expect("stdin owner");
        let looked_up = arena
            .lookup_capability(capability.handle, capability.raw)
            .expect("lookup canonical source capability");
        assert_eq!(looked_up, capability);
        assert!(arena
            .lookup_capability(test_handle(12), capability.raw)
            .is_err());
        arena
            .release_resource(capability.handle, capability.raw)
            .expect("release stdin owner");
    }

    #[test]
    fn taking_file_reader_rolls_back_when_cursor_aliases_owner() {
        let path = test_path("take-reader");

        std::fs::write(&path, b"line\n").expect("create source file");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file(test_handle(13), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed reader");
        let key = capability.native_loop_key().expect("file loop key");
        let borrowed = arena
            .lines_file(&key, false)
            .result
            .expect("borrow file owner");
        assert!(arena
            .take_file_reader(capability.handle, capability.raw)
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(capability.handle, capability.raw)
                .expect("rolled back file capability")
                .kind,
            SourceResourceKind::LinesFile
        );
        drop(borrowed);
        let reader = arena
            .take_file_reader(capability.handle, capability.raw)
            .expect("consume unaliased file owner");
        assert!(arena
            .lookup_capability(capability.handle, capability.raw)
            .is_err());
        drop(reader);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn taking_file_writer_rolls_back_when_owner_is_aliased() {
        let path = test_path("take-writer");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file_writer(test_handle(20), path.to_str().expect("utf8 scratch path"))
            .expect("admit file writer");
        let alias = arena
            .owner_for_raw(
                capability.handle,
                capability.raw,
                &SourceResourceKind::FileWriter,
            )
            .expect("borrow writer owner");
        assert!(arena
            .take_file_writer(capability.handle, capability.raw)
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(capability.handle, capability.raw)
                .expect("rolled back writer capability")
                .kind,
            SourceResourceKind::FileWriter
        );
        drop(alias);
        let writer = arena
            .take_file_writer(capability.handle, capability.raw)
            .expect("consume unaliased file writer");
        drop(writer);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn canonical_channel_endpoints_keep_sender_and_receiver_leases_distinct() {
        let arena = SourceResourceArena::new();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(16), test_handle(17), Some(1))
            .expect("admit channel endpoints");
        assert_eq!(sender.kind, SourceResourceKind::ChannelSender);
        assert_eq!(receiver.kind, SourceResourceKind::ChannelReceiver);
        assert!(arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(9))
            .expect("send channel value"));
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver.handle, receiver.raw)
                .expect("receive channel value"),
            Some(MirRuntimeValue::Int(9))
        );
        arena
            .channel_sender_close(sender.handle, sender.raw)
            .expect("close channel sender");
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver.handle, receiver.raw)
                .expect("observe closed receiver"),
            None
        );
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release sender endpoint");
        arena
            .release_resource(receiver.handle, receiver.raw)
            .expect("release receiver endpoint");
    }

    #[test]
    fn cloned_channel_endpoints_keep_physical_counts_until_last_drop() {
        let arena = SourceResourceArena::new();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(21), test_handle(22), None)
            .expect("admit channel endpoints");
        let sender_clone = arena
            .clone_channel_sender(sender.handle, sender.raw, test_handle(23))
            .expect("clone sender endpoint");
        let receiver_clone = arena
            .clone_channel_receiver(receiver.handle, receiver.raw, test_handle(24))
            .expect("clone receiver endpoint");
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release original sender endpoint");
        arena
            .release_resource(receiver.handle, receiver.raw)
            .expect("release original receiver endpoint");
        assert!(arena
            .channel_sender_send(sender_clone.handle, sender_clone.raw, MirRuntimeValue::Int(11))
            .expect("send with cloned sender"));
        assert_eq!(
            arena
                .channel_receiver_try_receive(
                    receiver_clone.handle,
                    receiver_clone.raw,
                )
                .expect("receive with cloned receiver"),
            Some(MirRuntimeValue::Int(11))
        );
        arena
            .channel_sender_close(sender_clone.handle, sender_clone.raw)
            .expect("close final sender endpoint");
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver_clone.handle, receiver_clone.raw)
                .expect("observe final sender close"),
            None
        );
        arena
            .release_resource(sender_clone.handle, sender_clone.raw)
            .expect("release cloned sender endpoint");
        arena
            .release_resource(receiver_clone.handle, receiver_clone.raw)
            .expect("release cloned receiver endpoint");
    }

    #[test]
    fn encoding_writer_consumes_file_writer_without_reopening_path() {
        let path = test_path("encoding-writer");
        let arena = SourceResourceArena::new();
        let file = arena
            .open_file_writer(test_handle(14), path.to_str().expect("utf8 scratch path"))
            .expect("admit file writer");
        let transfer = arena.register_encoding_writer_from_file(
            test_handle(15),
            file.handle,
            file.raw,
            "JSONLWriter",
        );
        assert!(transfer.consumed.is_some());
        let writer = transfer.result.expect("admit encoding writer");
        assert_eq!(
            writer.kind,
            SourceResourceKind::EncodingWriter {
                writer_type: "JSONLWriter".to_string()
            }
        );
        let value = MirRuntimeValue::Enum {
            type_name: "DataTree".to_string(),
            variant: "Int".to_string(),
            args: vec![(None, MirRuntimeValue::Int(7))],
        };
        arena
            .encoding_writer_write(writer.handle, writer.raw, &value)
            .expect("write encoded value");
        arena
            .encoding_writer_finish(writer.handle, writer.raw)
            .expect("finish encoded writer");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read encoded output"),
            "7\n"
        );
        arena
            .release_resource(writer.handle, writer.raw)
            .expect("release encoding writer");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn encoding_reader_consumes_existing_file_reader_lease() {
        let path = test_path("encoding-reader");
        std::fs::write(&path, b"7\n").expect("create encoded source");
        let arena = SourceResourceArena::new();
        let file = arena
            .open_file(test_handle(18), path.to_str().expect("utf8 scratch path"))
            .expect("admit file reader");
        let transfer = arena.register_encoding_reader_from_file(
            test_handle(19),
            file.handle,
            file.raw,
            "JSONLReader",
        );
        assert!(transfer.consumed.is_some());
        let reader = transfer.result.expect("admit encoding reader");
        assert!(arena.lookup_capability(file.handle, file.raw).is_err());
        let key = reader.native_loop_key().expect("encoding loop key");
        let mut iterator = arena
            .encoding_reader(&key, false)
            .result
            .expect("borrow encoding reader");
        assert_eq!(
            iterator.next(),
            Some(Ok(MirRuntimeValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Int".to_string(),
                args: vec![(None, MirRuntimeValue::Int(7))],
            }))
        );
        assert_eq!(iterator.next(), None);
        drop(iterator);
        arena
            .release_resource(reader.handle, reader.raw)
            .expect("release encoding reader");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn encoding_transfer_outcome_distinguishes_preflight_and_post_take_failures() {
        let preflight_path = test_path("encoding-transfer-preflight");
        std::fs::write(&preflight_path, b"7\n").expect("create preflight source");
        let arena = SourceResourceArena::new();
        let preflight_file = arena
            .open_file(
                test_handle(25),
                preflight_path.to_str().expect("utf8 scratch path"),
            )
            .expect("admit preflight reader");
        let preflight = arena.register_encoding_reader_from_file(
            test_handle(26),
            preflight_file.handle,
            preflight_file.raw,
            "UnknownReader",
        );
        assert!(preflight.consumed.is_none());
        assert!(preflight.result.is_err());
        assert!(arena
            .lookup_capability(preflight_file.handle, preflight_file.raw)
            .is_ok());
        arena
            .release_resource(preflight_file.handle, preflight_file.raw)
            .expect("release preflight reader");
        let _ = std::fs::remove_file(preflight_path);

        let post_take_path = test_path("encoding-transfer-post-take");
        std::fs::write(&post_take_path, b"a,b\n").expect("create post-take source");
        let post_take_file = arena
            .open_file(
                test_handle(27),
                post_take_path.to_str().expect("utf8 scratch path"),
            )
            .expect("admit post-take reader");
        let post_take = arena.register_encoding_reader_from_file_with_options(
            test_handle(28),
            post_take_file.handle,
            post_take_file.raw,
            "CSVReader",
            SourceEncodingLimits::safe(),
            ",,",
            false,
            false,
        );
        assert!(post_take.consumed.is_some());
        assert!(post_take.result.is_err());
        assert!(arena
            .lookup_capability(post_take_file.handle, post_take_file.raw)
            .is_err());
        let _ = std::fs::remove_file(post_take_path);
    }

    #[test]
    fn source_writer_refuses_live_mapped_path() {
        let path = test_path("mapped-writer");
        let path_text = path.to_str().expect("utf8 scratch path");
        std::fs::write(&path, b"mapped\n").expect("create mapped source");
        let mapped = match crate::CoreHost::os_rt::jet_std::jet_std_files_map(
            &path_text.to_string(),
        ) {
            Ok(mapped) => mapped,
            Err(_) => panic!("mapped source must be admitted"),
        };
        let result = SourceResourceArena::new().open_file_writer(test_handle(10), path_text);
        drop(mapped);
        let error = match result {
            Ok(_) => panic!("writer must refuse a path held by a mapped reader"),
            Err(error) => error,
        };
        let value = value_of(error);
        assert!(matches!(
            value,
            MirRuntimeValue::Enum { type_name, variant, args }
                if type_name == "IOError"
                    && variant == "Other"
                    && matches!(
                        &args[0].1,
                        MirRuntimeValue::Struct { type_name, fields }
                            if type_name == "IOContext"
                                && matches!(
                                    &fields[0].1,
                                    MirRuntimeValue::Enum { type_name, variant, .. }
                                        if type_name == "IOOperation" && variant == "Write"
                                )
                                && matches!(
                                    &fields[3].1,
                                    MirRuntimeValue::Present(value)
                                        if matches!(
                                            value.as_ref(),
                                            MirRuntimeValue::String(message)
                                                if message == "file is mapped read-only"
                                        )
                                )
                    )
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_resource_rejects_wrong_scope_stale_and_kind() {
        let arena = SourceResourceArena::new();
        let capability = arena.register_stdin(test_handle(4)).expect("stdin owner");
        let key = capability.native_loop_key().expect("stdin loop key");
        let wrong_kind = NativeLoopResourceKey {
            handle: key.handle,
            raw: key.raw,
            source_kind: MirLoopSourceKind::Plain,
        };
        assert!(arena.lines_file(&wrong_kind, false).result.is_err());

        let other_arena = SourceResourceArena::new();
        assert!(other_arena.lines_stdin(&key, false).result.is_err());
        arena
            .release_resource(capability.handle, capability.raw)
            .expect("release exact source slot");
        assert!(arena.lines_stdin(&key, false).result.is_err());
        assert!(arena.release_resource(capability.handle, capability.raw).is_err());
    }

    #[test]
    fn source_activation_restores_nested_and_unwind_without_implicit_retirement() {
        let outer_session = SourceResourceSession::new();
        let outer_arena = outer_session.arena();
        let outer_capability = outer_arena.register_stdin(test_handle(5)).unwrap();
        let retained_capability = outer_arena.register_stdin(test_handle(6)).unwrap();
        let inner_session = SourceResourceSession::new();
        let inner_arena = inner_session.arena();
        let inner_capability = inner_arena.register_stdin(test_handle(7)).unwrap();

        {
            let _outer_activation = outer_session.activate();
            let active = active_source_resource_arena().expect("outer activation");
            assert!(Arc::ptr_eq(&active.state, &outer_arena.state));
            let result = catch_unwind(AssertUnwindSafe(|| {
                let _inner_activation = inner_session.activate();
                let active = active_source_resource_arena().expect("inner activation");
                assert!(Arc::ptr_eq(&active.state, &inner_arena.state));
                panic!("abort nested source callback");
            }));
            assert!(result.is_err());
            let active = active_source_resource_arena().expect("outer restoration");
            assert!(Arc::ptr_eq(&active.state, &outer_arena.state));
        }
        assert!(active_source_resource_arena().is_none());
        assert!(outer_arena.release_capability(&outer_capability).is_ok());
        assert!(inner_arena.release_capability(&inner_capability).is_ok());
        let retained_key = retained_capability.native_loop_key().unwrap();
        outer_session.retire().expect("explicit source retirement");
        assert!(outer_arena.release_capability(&retained_capability).is_err());
        assert!(outer_arena.register_stdin(test_handle(9)).is_err());
        assert!(outer_arena.lines_stdin(&retained_key, false).result.is_err());

    }

    #[test]
    fn source_open_preserves_canonical_io_error_value() {
        let path = test_path("missing");
        let _ = std::fs::remove_file(&path);
        let error = SourceResourceArena::new()
            .open_file(test_handle(7), path.to_str().expect("utf8 scratch path"))
            .expect_err("missing source file must be a typed IO error");
        let value = value_of(error);
        assert!(matches!(
            value,
            MirRuntimeValue::Enum { type_name, variant, args }
                if type_name == "IOError"
                    && variant == "NotFound"
                    && matches!(
                        &args[0].1,
                        MirRuntimeValue::Struct { type_name, fields }
                            if type_name == "IOContext"
                                && matches!(
                                    &fields[0].1,
                                    MirRuntimeValue::Enum { type_name, variant, .. }
                                        if type_name == "IOOperation" && variant == "Read"
                                )
                    )
        ));
    }

    #[test]
    fn source_error_shapes_preserve_canonical_enums_and_context() {
        let context = IOContext::new(
            IOOperation::Read,
            Some("source.dat".to_string()),
            Some(2),
            Some("denied".to_string()),
        );
        let file_value = value_of(file_error(IOError::PermissionDenied(context.clone())));
        let MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } = file_value
        else {
            panic!("file error was not an IOError enum");
        };
        assert_eq!(type_name, "IOError");
        assert_eq!(variant, "PermissionDenied");
        assert!(matches!(
            &args[0].1,
            MirRuntimeValue::Struct { type_name, fields }
                if type_name == "IOContext"
                    && matches!(
                        &fields[0].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "IOOperation" && variant == "Read"
                    )
        ));

        let limit_value = value_of(process_error(IOError::ResourceLimit(
            ProcessResourceLimit::Memory,
        )));
        assert!(matches!(
            limit_value,
            MirRuntimeValue::Enum { type_name, variant, args }
                if type_name == "IOError"
                    && variant == "ResourceLimit"
                    && matches!(
                        &args[0].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "ProcessResourceLimit" && variant == "Memory"
                    )
        ));

        let encoding = EncodingError::new(
            EncodingFormat::JSON,
            EncodingErrorKind::Syntax,
            9,
            Ok(3),
            Err(JetAbsent),
            "source.dat",
            "bad token",
        )
        .with_cause(EncodingCause {
            kind: "os".to_string(),
            os_code: Ok(2),
            message: "parse".to_string(),
        });
        let encoding_value = value_of(encoding_error(&encoding));
        assert!(matches!(
            encoding_value,
            MirRuntimeValue::Struct { type_name, fields }
                if type_name == "EncodingError"
                    && matches!(
                        &fields[0].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "EncodingFormat" && variant == "JSON"
                    )
                    && matches!(
                        &fields[1].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "EncodingErrorKind" && variant == "Syntax"
                    )
        ));
    }

    #[test]
    fn cursor_by_value_preserves_consumed_receipt_on_success() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(43)).expect("admit stdin owner");
        let key = source.native_loop_key().expect("stdin loop key");
        let outcome = arena.init_cursor(&key, 1, false, true);
        assert_eq!(outcome.consumed, Some(source.clone()));
        let (cursor, value) = outcome.result.expect("install native cursor");
        assert_eq!(cursor.kind, SourceResourceKind::NativeCursor);
        assert!(matches!(value, MirRuntimeValue::NativeCursor(_)));
        assert!(arena.lookup_capability(source.handle, source.raw).is_err());
        arena
            .release_capability(&cursor)
            .expect("release native cursor");
    }

    #[test]
    fn cursor_by_value_preserves_consumed_receipt_on_slot_failure() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(44)).expect("admit stdin owner");
        let key = source.native_loop_key().expect("stdin loop key");
        arena
            .state
            .lock()
            .expect("lock source arena")
            .next_slot = u32::MAX;
        let outcome = arena.init_cursor(&key, 1, false, true);
        assert_eq!(outcome.consumed, Some(source.clone()));
        assert!(outcome.result.is_err());
        assert!(arena.lookup_capability(source.handle, source.raw).is_err());
    }

    #[test]
    fn owned_payload_take_carrier_adopt_commits_and_stales_source() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(40)).expect("admit stdin owner");
        let (consumed, lease) = arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("take sole source owner");
        assert_eq!(consumed, source);
        let carrier = lease
            .with_value(MirRuntimeValue::String("owned".to_string()))
            .into_runtime_value();
        let alias = carrier.clone();
        assert!(source_owned_from_runtime_value(alias).is_err());
        let owned = source_owned_from_runtime_value(carrier).expect("unwrap owned carrier");
        let outcome = arena.adopt_owned(
            test_handle(41),
            &SourceResourceKind::LinesStdin,
            owned,
        );
        assert_eq!(outcome.consumed, source);
        let (destination, value) = outcome.result.expect("commit destination owner");
        assert_eq!(value, MirRuntimeValue::String("owned".to_string()));
        assert!(arena
            .lookup_capability(source.handle, source.raw)
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(destination.handle, destination.raw)
                .expect("lookup adopted owner")
                .kind,
            SourceResourceKind::LinesStdin
        );
        arena
            .release_capability(&destination)
            .expect("release adopted owner");
    }

    #[test]
    fn owned_payload_take_rejects_alias_and_preserves_source_slot() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(42)).expect("admit stdin owner");
        let alias = arena
            .owner_for_raw(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("borrow source owner");
        assert!(arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(source.handle, source.raw)
                .expect("source remains after alias rejection"),
            source
        );
        drop(alias);
        let (_, lease) = arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("take after alias release");
        drop(lease);
    }
    #[test]
    fn retained_root_defers_retirement_until_last_lease_and_clears_slots() {
        let session = SourceResourceSession::new();
        let root = session.retain_root().expect("retain Source callback root");
        let arena = root.arena();
        let capability = arena
            .register_stdin(test_handle(51))
            .expect("register retained callback resource");

        session.retire().expect("retirement request");
        assert!(session.is_retirement_requested().expect("retirement state"));
        assert!(!arena.is_retired().expect("deferred retirement state"));
        assert!(arena.lookup_capability(capability.handle, capability.raw).is_ok());

        let alias = root.clone();
        drop(root);
        assert_eq!(session.retained_root_count().expect("root count"), 1);
        assert!(!arena.is_retired().expect("last alias still live"));
        drop(alias);

        assert_eq!(session.retained_root_count().expect("root count"), 0);
        assert!(arena.is_retired().expect("last root retires arena"));
        assert!(arena.lookup_capability(capability.handle, capability.raw).is_err());
    }

    #[test]
    fn retained_root_allows_work_after_request_and_rejects_after_last_release() {
        let session = SourceResourceSession::new();
        let root = session.retain_root().expect("retain Source callback root");
        let arena = root.arena();
        session.retire().expect("retirement request");

        let nested = session
            .retain_root()
            .expect("valid retained-root work after retirement request");
        let capability = arena
            .register_stdin(test_handle(52))
            .expect("existing root remains physically usable");
        arena
            .release_capability(&capability)
            .expect("release existing-root capability");
        assert!(!arena.is_retired().expect("roots still live"));

        drop(nested);
        assert!(!arena.is_retired().expect("original root still live"));
        drop(root);
        assert!(arena.is_retired().expect("last root retires arena"));
        assert!(session.retain_root().is_err());
    }

    #[test]
    fn last_root_release_allows_reentrant_arena_cleanup() {
        let session = SourceResourceSession::new();
        let root = session.retain_root().expect("retain Source callback root");
        let arena = root.arena();
        let capability = arena
            .register_stdin(test_handle(53))
            .expect("register reentrant cleanup resource");
        session.retire().expect("retirement request");

        let reentered_after_release = Arc::new(AtomicUsize::new(0));
        let cleanup = ReentrantLeaseDrop {
            lease: Some(root),
            arena: arena.clone(),
            reentered_after_release: reentered_after_release.clone(),
        };
        drop(cleanup);

        assert_eq!(
            reentered_after_release.load(Ordering::SeqCst),
            1,
            "last-root cleanup must unlock before reentrant arena access"
        );
        assert!(arena.is_retired().expect("retired arena"));
        assert!(arena.lookup_capability(capability.handle, capability.raw).is_err());
    }
}
