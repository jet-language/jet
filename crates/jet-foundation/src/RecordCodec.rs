//! The one std-only binary record codec for compiler records (#2517).
//!
//! Every persisted compiler record (package checks, interfaces, typed
//! diagnostics, item graphs, program records, and the invocation Receipt)
//! uses this byte format. I6 rules out serde; the layout follows Go's Unified
//! IR shape so a reader can decode one element without decoding the rest.
//!
//! Layout (all integers are unsigned LEB128 `uvarint`s in their minimal form):
//!
//! ```text
//! "JETR"                         magic, 4 bytes
//! uvarint format                 RECORD_FORMAT (1)
//! str schema                     for example "jet.iface/v1"
//! uvarint n, n × str             string table, first-use order
//! uvarint sections               section count
//! per section, ascending tag:
//!   uvarint tag
//!   uvarint elements
//!   elements × uvarint length    element index
//!   element bytes, concatenated
//! 32 bytes                       SHA-256 of every preceding byte
//! ```
//!
//! `str` is a uvarint byte length followed by UTF-8. The string table is
//! interned by walking sections in tag order, elements in order, and each
//! value depth-first (a map key before its value). Values are self-describing:
//!
//! | tag | value |
//! |---|---|
//! | 0 | null |
//! | 1 | false |
//! | 2 | true |
//! | 3 | integer, zigzag-encoded `i64` as a uvarint |
//! | 4 | string, uvarint string-table index |
//! | 5 | bytes, uvarint length and raw bytes |
//! | 6 | list, uvarint count and values |
//! | 7 | map, uvarint count and (uvarint key index, value) pairs sorted by key bytes |
//! | 8 | digest, 32 raw bytes |
//!
//! The encoding is canonical: one value has exactly one byte form. The
//! decoder rejects anything else (overlong varints, unsorted or duplicate map
//! keys or section tags, out-of-range string indexes, trailing bytes, and a
//! checksum mismatch). A caller treats every decode error as a cache miss.
//! JetFoundation implements the same bytes; a conformance test compares them.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::SHA256::sha256;

pub const RECORD_MAGIC: [u8; 4] = *b"JETR";
pub const RECORD_FORMAT: u64 = 1;
const CHECKSUM_BYTES: usize = 32;
/// Nesting bound so a hostile record cannot exhaust the decoder's stack.
const MAX_DEPTH: usize = 128;

const TAG_NULL: u8 = 0;
const TAG_FALSE: u8 = 1;
const TAG_TRUE: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_STR: u8 = 4;
const TAG_BYTES: u8 = 5;
const TAG_LIST: u8 = 6;
const TAG_MAP: u8 = 7;
const TAG_DIGEST: u8 = 8;

/// One self-describing record value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordValue {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<RecordValue>),
    Map(BTreeMap<String, RecordValue>),
    Digest([u8; 32]),
}

impl RecordValue {
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    pub fn optional_str(value: Option<&str>) -> Self {
        value.map_or(Self::Null, Self::str)
    }

    pub fn list_of_str<'a>(values: impl IntoIterator<Item = &'a String>) -> Self {
        Self::List(values.into_iter().map(|value| Self::Str(value.clone())).collect())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_str(&self) -> Result<&str, RecordError> {
        match self {
            Self::Str(value) => Ok(value),
            other => Err(RecordError::shape("string", other)),
        }
    }

    pub fn as_optional_str(&self) -> Result<Option<&str>, RecordError> {
        match self {
            Self::Null => Ok(None),
            other => other.as_str().map(Some),
        }
    }

    pub fn as_int(&self) -> Result<i64, RecordError> {
        match self {
            Self::Int(value) => Ok(*value),
            other => Err(RecordError::shape("integer", other)),
        }
    }

    pub fn as_usize(&self) -> Result<usize, RecordError> {
        usize::try_from(self.as_int()?)
            .map_err(|_| RecordError::Malformed("negative or oversized count".to_string()))
    }

    pub fn as_bool(&self) -> Result<bool, RecordError> {
        match self {
            Self::Bool(value) => Ok(*value),
            other => Err(RecordError::shape("boolean", other)),
        }
    }

    pub fn as_list(&self) -> Result<&[RecordValue], RecordError> {
        match self {
            Self::List(values) => Ok(values),
            other => Err(RecordError::shape("list", other)),
        }
    }

    pub fn as_map(&self) -> Result<&BTreeMap<String, RecordValue>, RecordError> {
        match self {
            Self::Map(fields) => Ok(fields),
            other => Err(RecordError::shape("map", other)),
        }
    }

    pub fn as_digest(&self) -> Result<[u8; 32], RecordError> {
        match self {
            Self::Digest(digest) => Ok(*digest),
            other => Err(RecordError::shape("digest", other)),
        }
    }

    /// A required map field.
    pub fn field(&self, name: &str) -> Result<&RecordValue, RecordError> {
        self.as_map()?
            .get(name)
            .ok_or_else(|| RecordError::Malformed(format!("field `{name}` is missing")))
    }

    pub fn str_list(&self) -> Result<Vec<String>, RecordError> {
        self.as_list()?
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect()
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Int(_) => "integer",
            Self::Str(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::Digest(_) => "digest",
        }
    }
}

/// Build a map value from `(name, value)` pairs.
pub fn record_map<const N: usize>(fields: [(&str, RecordValue); N]) -> RecordValue {
    RecordValue::Map(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

/// One tagged section: an indexed list of elements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordSection {
    pub tag: u64,
    pub elements: Vec<RecordValue>,
}

impl RecordSection {
    pub fn new(tag: u64, elements: Vec<RecordValue>) -> Self {
        Self { tag, elements }
    }
}

/// A fully decoded record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub schema: String,
    pub sections: Vec<RecordSection>,
    /// The trailing SHA-256; it is the record's content digest.
    pub digest: [u8; 32],
}

impl Record {
    pub fn section(&self, tag: u64) -> Option<&RecordSection> {
        self.sections.iter().find(|section| section.tag == tag)
    }

    /// The elements of a section, or an error naming the missing tag.
    pub fn elements(&self, tag: u64) -> Result<&[RecordValue], RecordError> {
        self.section(tag)
            .map(|section| section.elements.as_slice())
            .ok_or(RecordError::MissingSection(tag))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordError {
    Truncated,
    BadMagic,
    UnsupportedFormat(u64),
    Checksum,
    DuplicateSection(u64),
    MissingSection(u64),
    Schema { expected: String, found: String },
    Malformed(String),
}

impl RecordError {
    fn shape(expected: &str, found: &RecordValue) -> Self {
        Self::Malformed(format!("expected {expected}, found {}", found.kind()))
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("record is truncated"),
            Self::BadMagic => f.write_str("record magic is not `JETR`"),
            Self::UnsupportedFormat(format) => write!(f, "record format {format} is not supported"),
            Self::Checksum => f.write_str("record checksum does not match its bytes"),
            Self::DuplicateSection(tag) => write!(f, "record section {tag} is duplicated or out of order"),
            Self::MissingSection(tag) => write!(f, "record section {tag} is missing"),
            Self::Schema { expected, found } => {
                write!(f, "record schema is `{found}`, expected `{expected}`")
            }
            Self::Malformed(reason) => write!(f, "record is malformed: {reason}"),
        }
    }
}

impl std::error::Error for RecordError {}

// ---------------------------------------------------------------- encoding

fn put_uvarint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    put_uvarint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

#[derive(Default)]
struct StringTable<'a> {
    order: Vec<&'a str>,
    index: HashMap<&'a str, u64>,
}

impl<'a> StringTable<'a> {
    fn intern(&mut self, value: &'a str) {
        if !self.index.contains_key(value) {
            self.index.insert(value, self.order.len() as u64);
            self.order.push(value);
        }
    }

    fn intern_value(&mut self, value: &'a RecordValue) {
        match value {
            RecordValue::Str(text) => self.intern(text),
            RecordValue::List(values) => values.iter().for_each(|value| self.intern_value(value)),
            RecordValue::Map(fields) => {
                for (name, value) in fields {
                    self.intern(name);
                    self.intern_value(value);
                }
            }
            RecordValue::Null
            | RecordValue::Bool(_)
            | RecordValue::Int(_)
            | RecordValue::Bytes(_)
            | RecordValue::Digest(_) => {}
        }
    }

    fn put_ref(&self, out: &mut Vec<u8>, value: &str) {
        put_uvarint(out, self.index[value]);
    }

    fn put_value(&self, out: &mut Vec<u8>, value: &RecordValue) {
        match value {
            RecordValue::Null => out.push(TAG_NULL),
            RecordValue::Bool(false) => out.push(TAG_FALSE),
            RecordValue::Bool(true) => out.push(TAG_TRUE),
            RecordValue::Int(number) => {
                out.push(TAG_INT);
                put_uvarint(out, zigzag(*number));
            }
            RecordValue::Str(text) => {
                out.push(TAG_STR);
                self.put_ref(out, text);
            }
            RecordValue::Bytes(bytes) => {
                out.push(TAG_BYTES);
                put_uvarint(out, bytes.len() as u64);
                out.extend_from_slice(bytes);
            }
            RecordValue::List(values) => {
                out.push(TAG_LIST);
                put_uvarint(out, values.len() as u64);
                values.iter().for_each(|value| self.put_value(out, value));
            }
            RecordValue::Map(fields) => {
                out.push(TAG_MAP);
                put_uvarint(out, fields.len() as u64);
                for (name, value) in fields {
                    self.put_ref(out, name);
                    self.put_value(out, value);
                }
            }
            RecordValue::Digest(digest) => {
                out.push(TAG_DIGEST);
                out.extend_from_slice(digest);
            }
        }
    }
}

/// Encode a record. Sections may be passed in any order; they are written in
/// ascending tag order. A repeated tag is an error.
pub fn encode_record(schema: &str, sections: &[RecordSection]) -> Result<Vec<u8>, RecordError> {
    let mut ordered = sections.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|section| section.tag);
    for pair in ordered.windows(2) {
        if pair[0].tag == pair[1].tag {
            return Err(RecordError::DuplicateSection(pair[0].tag));
        }
    }
    let mut table = StringTable::default();
    for section in &ordered {
        section.elements.iter().for_each(|value| table.intern_value(value));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&RECORD_MAGIC);
    put_uvarint(&mut out, RECORD_FORMAT);
    put_str(&mut out, schema);
    put_uvarint(&mut out, table.order.len() as u64);
    for text in &table.order {
        put_str(&mut out, text);
    }
    put_uvarint(&mut out, ordered.len() as u64);
    let mut element = Vec::new();
    for section in ordered {
        put_uvarint(&mut out, section.tag);
        put_uvarint(&mut out, section.elements.len() as u64);
        let mut body = Vec::new();
        for value in &section.elements {
            element.clear();
            table.put_value(&mut element, value);
            put_uvarint(&mut out, element.len() as u64);
            body.extend_from_slice(&element);
        }
        out.extend_from_slice(&body);
    }
    let checksum = sha256(&out);
    out.extend_from_slice(&checksum);
    Ok(out)
}

/// The content digest a record would have, without keeping the bytes.
pub fn record_digest(schema: &str, sections: &[RecordSection]) -> Result<[u8; 32], RecordError> {
    let bytes = encode_record(schema, sections)?;
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&bytes[bytes.len() - CHECKSUM_BYTES..]);
    Ok(digest)
}

// ---------------------------------------------------------------- decoding

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], RecordError> {
        let end = self.at.checked_add(len).ok_or(RecordError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(RecordError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, RecordError> {
        Ok(self.take(1)?[0])
    }

    fn uvarint(&mut self) -> Result<u64, RecordError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.byte()?;
            let low = u64::from(byte & 0x7f);
            if shift == 63 && low > 1 {
                return Err(RecordError::Malformed("varint overflows u64".to_string()));
            }
            value |= low << shift;
            if byte & 0x80 == 0 {
                if byte == 0 && shift > 0 {
                    return Err(RecordError::Malformed("varint is not minimal".to_string()));
                }
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                return Err(RecordError::Malformed("varint overflows u64".to_string()));
            }
        }
    }

    fn len(&mut self) -> Result<usize, RecordError> {
        let value = self.uvarint()?;
        let len = usize::try_from(value).map_err(|_| RecordError::Truncated)?;
        // Every counted thing occupies at least one byte, so a count larger
        // than the remaining input is truncation, never an allocation request.
        if len > self.bytes.len() - self.at {
            return Err(RecordError::Truncated);
        }
        Ok(len)
    }

    fn str(&mut self) -> Result<&'a str, RecordError> {
        let len = self.len()?;
        std::str::from_utf8(self.take(len)?)
            .map_err(|_| RecordError::Malformed("string is not UTF-8".to_string()))
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// A verified record whose elements decode on demand.
pub struct RecordReader<'a> {
    schema: &'a str,
    strings: Vec<&'a str>,
    /// `(tag, element slices)` in ascending tag order.
    sections: Vec<(u64, Vec<&'a [u8]>)>,
    digest: [u8; 32],
}

impl<'a> RecordReader<'a> {
    /// Verify the checksum and framing and index every section. Element
    /// payloads are not decoded until asked for.
    pub fn open(bytes: &'a [u8]) -> Result<Self, RecordError> {
        if bytes.len() < RECORD_MAGIC.len() + CHECKSUM_BYTES {
            return Err(RecordError::Truncated);
        }
        let (body, checksum) = bytes.split_at(bytes.len() - CHECKSUM_BYTES);
        if body[..RECORD_MAGIC.len()] != RECORD_MAGIC {
            return Err(RecordError::BadMagic);
        }
        if sha256(body)[..] != checksum[..] {
            return Err(RecordError::Checksum);
        }
        let mut digest = [0u8; 32];
        digest.copy_from_slice(checksum);
        let mut cursor = Cursor::new(body);
        cursor.take(RECORD_MAGIC.len())?;
        let format = cursor.uvarint()?;
        if format != RECORD_FORMAT {
            return Err(RecordError::UnsupportedFormat(format));
        }
        let schema = cursor.str()?;
        let count = cursor.len()?;
        let mut strings = Vec::with_capacity(count);
        for _ in 0..count {
            strings.push(cursor.str()?);
        }
        let section_count = cursor.len()?;
        let mut sections: Vec<(u64, Vec<&'a [u8]>)> = Vec::with_capacity(section_count);
        for _ in 0..section_count {
            let tag = cursor.uvarint()?;
            if sections.last().is_some_and(|(previous, _)| *previous >= tag) {
                return Err(RecordError::DuplicateSection(tag));
            }
            let elements = cursor.len()?;
            let mut lengths = Vec::with_capacity(elements);
            for _ in 0..elements {
                lengths.push(cursor.len()?);
            }
            let mut slices = Vec::with_capacity(elements);
            for len in lengths {
                slices.push(cursor.take(len)?);
            }
            sections.push((tag, slices));
        }
        if !cursor.done() {
            return Err(RecordError::Malformed("trailing bytes before the checksum".to_string()));
        }
        Ok(Self {
            schema,
            strings,
            sections,
            digest,
        })
    }

    /// Open and require `schema`.
    pub fn open_schema(bytes: &'a [u8], schema: &str) -> Result<Self, RecordError> {
        let reader = Self::open(bytes)?;
        if reader.schema != schema {
            return Err(RecordError::Schema {
                expected: schema.to_string(),
                found: reader.schema.to_string(),
            });
        }
        Ok(reader)
    }

    pub fn schema(&self) -> &'a str {
        self.schema
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    fn slices(&self, tag: u64) -> Option<&[&'a [u8]]> {
        self.sections
            .binary_search_by_key(&tag, |(section, _)| *section)
            .ok()
            .map(|index| self.sections[index].1.as_slice())
    }

    /// Number of elements in a section; `None` when the section is absent.
    pub fn element_count(&self, tag: u64) -> Option<usize> {
        self.slices(tag).map(<[_]>::len)
    }

    /// Decode one element of one section.
    pub fn element(&self, tag: u64, index: usize) -> Result<RecordValue, RecordError> {
        let slices = self.slices(tag).ok_or(RecordError::MissingSection(tag))?;
        let bytes = slices
            .get(index)
            .ok_or_else(|| RecordError::Malformed(format!("section {tag} has no element {index}")))?;
        let mut cursor = Cursor::new(bytes);
        let value = self.value(&mut cursor, 0)?;
        if !cursor.done() {
            return Err(RecordError::Malformed("element has trailing bytes".to_string()));
        }
        Ok(value)
    }

    /// Decode every element of one section.
    pub fn elements(&self, tag: u64) -> Result<Vec<RecordValue>, RecordError> {
        let count = self.element_count(tag).ok_or(RecordError::MissingSection(tag))?;
        (0..count).map(|index| self.element(tag, index)).collect()
    }

    /// Decode the whole record.
    pub fn decode(&self) -> Result<Record, RecordError> {
        let sections = self
            .sections
            .iter()
            .map(|(tag, _)| {
                Ok(RecordSection {
                    tag: *tag,
                    elements: self.elements(*tag)?,
                })
            })
            .collect::<Result<Vec<_>, RecordError>>()?;
        Ok(Record {
            schema: self.schema.to_string(),
            sections,
            digest: self.digest,
        })
    }

    fn string(&self, cursor: &mut Cursor<'_>) -> Result<&'a str, RecordError> {
        let index = cursor.uvarint()?;
        usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get(index).copied())
            .ok_or_else(|| RecordError::Malformed(format!("string index {index} is out of range")))
    }

    fn value(&self, cursor: &mut Cursor<'_>, depth: usize) -> Result<RecordValue, RecordError> {
        if depth > MAX_DEPTH {
            return Err(RecordError::Malformed("value nesting is too deep".to_string()));
        }
        Ok(match cursor.byte()? {
            TAG_NULL => RecordValue::Null,
            TAG_FALSE => RecordValue::Bool(false),
            TAG_TRUE => RecordValue::Bool(true),
            TAG_INT => RecordValue::Int(unzigzag(cursor.uvarint()?)),
            TAG_STR => RecordValue::Str(self.string(cursor)?.to_string()),
            TAG_BYTES => {
                let len = cursor.len()?;
                RecordValue::Bytes(cursor.take(len)?.to_vec())
            }
            TAG_LIST => {
                let count = cursor.len()?;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.value(cursor, depth + 1)?);
                }
                RecordValue::List(values)
            }
            TAG_MAP => {
                let count = cursor.len()?;
                let mut fields = BTreeMap::new();
                let mut previous: Option<&str> = None;
                for _ in 0..count {
                    let name = self.string(cursor)?;
                    if previous.is_some_and(|previous| previous.as_bytes() >= name.as_bytes()) {
                        return Err(RecordError::Malformed(format!(
                            "map key `{name}` is duplicated or out of order"
                        )));
                    }
                    previous = Some(name);
                    fields.insert(name.to_string(), self.value(cursor, depth + 1)?);
                }
                RecordValue::Map(fields)
            }
            TAG_DIGEST => {
                let mut digest = [0u8; 32];
                digest.copy_from_slice(cursor.take(32)?);
                RecordValue::Digest(digest)
            }
            other => return Err(RecordError::Malformed(format!("unknown value tag {other}"))),
        })
    }
}

/// Decode a whole record.
pub fn decode_record(bytes: &[u8]) -> Result<Record, RecordError> {
    RecordReader::open(bytes)?.decode()
}

/// Lowercase hex of a digest.
pub fn digest_hex(digest: &[u8; 32]) -> String {
    use std::fmt::Write;
    digest.iter().fold(String::with_capacity(64), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Parse a 64-character lowercase hex digest.
pub fn parse_digest_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut digest = [0u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(digest)
}

/// Render any record as JSON for tests and debugging. This is not a user
/// surface: a record view command needs its own ballot.
pub fn record_json(bytes: &[u8]) -> Result<String, RecordError> {
    let record = decode_record(bytes)?;
    let mut out = format!(
        "{{\"schema\":\"{}\",\"digest\":\"{}\",\"sections\":{{",
        crate::JSON::json_escape(&record.schema),
        digest_hex(&record.digest)
    );
    for (index, section) in record.sections.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{}\":[", section.tag));
        for (element, value) in section.elements.iter().enumerate() {
            if element > 0 {
                out.push(',');
            }
            value_json(&mut out, value);
        }
        out.push(']');
    }
    out.push_str("}}");
    Ok(out)
}

fn value_json(out: &mut String, value: &RecordValue) {
    match value {
        RecordValue::Null => out.push_str("null"),
        RecordValue::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        RecordValue::Int(number) => out.push_str(&number.to_string()),
        RecordValue::Str(text) => {
            out.push('"');
            out.push_str(&crate::JSON::json_escape(text));
            out.push('"');
        }
        RecordValue::Bytes(bytes) => {
            out.push_str("{\"bytes\":\"");
            for byte in bytes {
                out.push_str(&format!("{byte:02x}"));
            }
            out.push_str("\"}");
        }
        RecordValue::List(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                value_json(out, value);
            }
            out.push(']');
        }
        RecordValue::Map(fields) => {
            out.push('{');
            for (index, (name, value)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(&crate::JSON::json_escape(name));
                out.push_str("\":");
                value_json(out, value);
            }
            out.push('}');
        }
        RecordValue::Digest(digest) => {
            out.push_str("{\"digest\":\"");
            out.push_str(&digest_hex(digest));
            out.push_str("\"}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<RecordSection> {
        vec![
            RecordSection::new(
                7,
                vec![
                    record_map([
                        ("name", RecordValue::str("core.text")),
                        ("count", RecordValue::Int(-3)),
                        ("digest", RecordValue::Digest([7; 32])),
                    ]),
                    RecordValue::List(vec![
                        RecordValue::Null,
                        RecordValue::Bool(true),
                        RecordValue::Bool(false),
                        RecordValue::Bytes(vec![0, 255, 16]),
                        RecordValue::Int(i64::MIN),
                        RecordValue::Int(i64::MAX),
                    ]),
                ],
            ),
            RecordSection::new(1, vec![RecordValue::str("core.text"), RecordValue::str("é")]),
            RecordSection::new(3, Vec::new()),
        ]
    }

    #[test]
    fn record_codec_round_trips_every_value_kind() {
        let bytes = encode_record("jet.test/v1", &sample()).expect("encodes");
        let record = decode_record(&bytes).expect("decodes");
        assert_eq!(record.schema, "jet.test/v1");
        assert_eq!(record.sections.iter().map(|s| s.tag).collect::<Vec<_>>(), [1, 3, 7]);
        let mut expected = sample();
        expected.sort_by_key(|section| section.tag);
        assert_eq!(record.sections, expected);
        assert_eq!(encode_record("jet.test/v1", &record.sections).unwrap(), bytes);
        assert_eq!(record.digest, record_digest("jet.test/v1", &sample()).unwrap());
    }

    #[test]
    fn record_codec_reads_one_element_lazily() {
        let bytes = encode_record("jet.test/v1", &sample()).unwrap();
        let reader = RecordReader::open_schema(&bytes, "jet.test/v1").unwrap();
        assert_eq!(reader.element_count(7), Some(2));
        assert_eq!(reader.element_count(2), None);
        assert_eq!(
            reader.element(7, 0).unwrap().field("name").unwrap().as_str().unwrap(),
            "core.text"
        );
        assert!(matches!(
            RecordReader::open_schema(&bytes, "jet.other/v1"),
            Err(RecordError::Schema { .. })
        ));
    }

    #[test]
    fn record_codec_bytes_are_pinned() {
        let bytes = encode_record(
            "s",
            &[RecordSection::new(
                2,
                vec![record_map([("k", RecordValue::Int(-1)), ("a", RecordValue::str("k"))])],
            )],
        )
        .unwrap();
        // magic, format 1, schema "s", strings ["a","k"] (map keys in byte
        // order, first use), one section tag 2 with one 8-byte element.
        let body = [
            b'J', b'E', b'T', b'R', 1, 1, b's', 2, 1, b'a', 1, b'k', 1, 2, 1, 8, TAG_MAP, 2, 0,
            TAG_STR, 1, 1, TAG_INT, 1,
        ];
        assert_eq!(&bytes[..bytes.len() - 32], &body);
        assert_eq!(&bytes[bytes.len() - 32..], &sha256(&body));
    }

    #[test]
    fn record_codec_rejects_every_non_canonical_or_damaged_form() {
        let bytes = encode_record("jet.test/v1", &sample()).unwrap();
        let mut flipped = bytes.clone();
        flipped[10] ^= 1;
        assert_eq!(decode_record(&flipped), Err(RecordError::Checksum));
        assert_eq!(decode_record(&bytes[..bytes.len() - 1]), Err(RecordError::Checksum));
        assert_eq!(decode_record(b"JET"), Err(RecordError::Truncated));
        assert!(matches!(
            encode_record("x", &[RecordSection::new(1, vec![]), RecordSection::new(1, vec![])]),
            Err(RecordError::DuplicateSection(1))
        ));
        let reseal = |body: &[u8]| {
            let mut out = body.to_vec();
            out.extend_from_slice(&sha256(body));
            out
        };
        // Overlong varint for the format number.
        assert!(matches!(
            decode_record(&reseal(&[b'J', b'E', b'T', b'R', 0x81, 0x00, 0, 0, 0])),
            Err(RecordError::Malformed(_))
        ));
        // Unsupported format.
        assert_eq!(
            decode_record(&reseal(&[b'J', b'E', b'T', b'R', 2, 0, 0, 0])),
            Err(RecordError::UnsupportedFormat(2))
        );
        // Map keys out of order: strings ["k","a"], map {k, a}.
        let unsorted = reseal(&[
            b'J', b'E', b'T', b'R', 1, 1, b's', 2, 1, b'k', 1, b'a', 1, 2, 1, 6, TAG_MAP, 2, 0,
            TAG_NULL, 1, TAG_NULL,
        ]);
        assert!(matches!(decode_record(&unsorted), Err(RecordError::Malformed(_))));
        // Sections out of order.
        let sections = reseal(&[b'J', b'E', b'T', b'R', 1, 1, b's', 0, 2, 5, 0, 4, 0]);
        assert_eq!(decode_record(&sections), Err(RecordError::DuplicateSection(4)));
        // String index out of range.
        let index = reseal(&[b'J', b'E', b'T', b'R', 1, 1, b's', 0, 1, 1, 1, 2, TAG_STR, 0]);
        assert!(matches!(decode_record(&index), Err(RecordError::Malformed(_))));
    }

    #[test]
    fn record_json_renders_the_decoded_tree() {
        let bytes = encode_record(
            "jet.test/v1",
            &[RecordSection::new(
                1,
                vec![record_map([
                    ("b", RecordValue::Bytes(vec![1, 2])),
                    ("n", RecordValue::Null),
                    ("s", RecordValue::str("q\"")),
                ])],
            )],
        )
        .unwrap();
        let json = record_json(&bytes).unwrap();
        assert!(json.ends_with(
            "\"sections\":{\"1\":[{\"b\":{\"bytes\":\"0102\"},\"n\":null,\"s\":\"q\\\"\"}]}}"
        ));
    }
}
