//! Bounded, canonical projections for typed sections carried by a receipt.
//!
//! A section is a named `#Codable` record serialized as one canonical JSON
//! object. The store owns the bytes and the section name; this module owns the
//! validation, deterministic ordering, query projection, and field diff.

use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256::sha256_hex;
use std::collections::{BTreeMap, BTreeSet};

/// Wire marker for the optional section extension in `jet-receipt-v2`.
pub const RECEIPT_SECTION_WIRE_MAGIC: &[u8] = b"jet-receipt-sections-v1\0";
/// Wire marker for an append-only typed section record.
pub const RECEIPT_SECTION_RECORD_WIRE_MAGIC: &[u8] = b"jet-receipt-section-record-v1\0";
/// Domain separator for append-only section identity.
pub const RECEIPT_SECTION_RECORD_ID_MAGIC: &[u8] = b"jet-receipt-section-id-v1\0";
/// Maximum number of named sections in one receipt.
pub const MAX_RECEIPT_SECTIONS: usize = 256;
/// Maximum UTF-8 byte length of a section name.
pub const MAX_RECEIPT_SECTION_NAME_BYTES: usize = 128;
/// Maximum UTF-8 byte length of a section type name.
pub const MAX_RECEIPT_SECTION_TYPE_BYTES: usize = 256;
/// Maximum bytes in one canonical section value.
pub const MAX_RECEIPT_SECTION_BYTES: usize = 64 * 1024 * 1024;
/// Maximum bytes across all canonical section values in one receipt.
pub const MAX_RECEIPT_SECTION_BYTES_TOTAL: usize = 64 * 1024 * 1024;
/// Maximum encoded append-only section record.
pub const MAX_RECEIPT_SECTION_RECORD_BYTES: usize = MAX_RECEIPT_SECTION_BYTES + 4096;

#[derive(Clone, PartialEq, Eq)]
pub struct ReceiptSection {
    /// Stable name supplied by the declaration, such as `regression`.
    pub name: String,
    /// Canonical source type name of the `#Codable` record.
    pub type_name: String,
    /// Digest of the checked source schema identity.
    pub schema_digest: String,
    /// Digest of `bytes`.
    pub payload_digest: String,
    /// Canonical JSON bytes for the record value, including one trailing LF.
    pub bytes: Vec<u8>,
}

/// One append-only section record. An empty `parent_digest` is reserved for
/// the child-process staging handoff before the immutable parent is published.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiptSectionRecord {
    pub claim_key: String,
    pub parent_digest: String,
    pub section: ReceiptSection,
}

impl std::fmt::Debug for ReceiptSection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReceiptSection")
            .field("name", &self.name)
            .field("type_name", &self.type_name)
            .field("schema_digest", &self.schema_digest)
            .field("payload_digest", &self.payload_digest)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for ReceiptSectionRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReceiptSectionRecord")
            .field("claim_key", &self.claim_key)
            .field("parent_digest", &self.parent_digest)
            .field("section", &self.section)
            .finish()
    }
}

impl ReceiptSection {
    /// Validate and construct one named section from canonical JSON bytes.
    pub fn new(
        name: impl Into<String>,
        type_name: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        let type_name = type_name.into();
        Self::new_with_schema_digest(
            name,
            type_name.clone(),
            schema_digest_for_type(&type_name),
            bytes,
        )
    }

    /// Construct a section with the checked source schema identity.
    pub fn new_with_schema_digest(
        name: impl Into<String>,
        type_name: impl Into<String>,
        schema_digest: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        let bytes = bytes.as_ref().to_vec();
        Self::new_with_digests(
            name,
            type_name,
            schema_digest,
            payload_digest(&bytes),
            bytes,
        )
    }

    /// Construct a section after checking both supplied integrity digests.
    pub fn new_with_digests(
        name: impl Into<String>,
        type_name: impl Into<String>,
        schema_digest: impl Into<String>,
        payload_digest: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        let section = Self {
            name: name.into(),
            type_name: type_name.into(),
            schema_digest: schema_digest.into(),
            payload_digest: payload_digest.into(),
            bytes: bytes.as_ref().to_vec(),
        };
        validate_section(&section)?;
        Ok(section)
    }

    /// Construct a section from canonical JSON bytes.
    pub fn from_bytes(
        name: impl Into<String>,
        type_name: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        Self::new(name, type_name, bytes)
    }

    /// Construct a section from canonical JSON bytes and schema identity.
    pub fn from_bytes_with_schema_digest(
        name: impl Into<String>,
        type_name: impl Into<String>,
        schema_digest: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<Self, String> {
        Self::new_with_schema_digest(name, type_name, schema_digest, bytes)
    }

    /// Construct a section from a canonical JSON value.
    pub fn from_json(
        name: impl Into<String>,
        type_name: impl Into<String>,
        value: impl std::borrow::Borrow<CanonicalJson>,
    ) -> Result<Self, String> {
        Self::new(name, type_name, value.borrow().bytes())
    }

    /// Construct a section from a canonical JSON value and schema identity.
    pub fn from_json_with_schema_digest(
        name: impl Into<String>,
        type_name: impl Into<String>,
        schema_digest: impl Into<String>,
        value: impl std::borrow::Borrow<CanonicalJson>,
    ) -> Result<Self, String> {
        Self::new_with_schema_digest(name, type_name, schema_digest, value.borrow().bytes())
    }

    /// Decode the typed value after validating its canonical JSON envelope.
    pub fn value(&self) -> Result<CanonicalJson, String> {
        canonical_object(&self.bytes)
    }

    /// Alias used by projection callers that prefer JSON terminology.
    pub fn json(&self) -> Result<CanonicalJson, String> {
        self.value()
    }
}

impl ReceiptSectionRecord {
    pub fn new(
        claim_key: impl Into<String>,
        parent_digest: impl Into<String>,
        section: ReceiptSection,
    ) -> Result<Self, String> {
        let record = Self {
            claim_key: claim_key.into(),
            parent_digest: parent_digest.into(),
            section,
        };
        validate_section_record(&record)?;
        Ok(record)
    }
}

/// Digest of the canonical type identity used when a caller has no checked
/// schema digest. Checked `receipt.attach` calls pass their own digest.
pub fn schema_digest_for_type(type_name: &str) -> String {
    sha256_hex(type_name.as_bytes())
}

/// Digest of the exact canonical section bytes.
pub fn payload_digest(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

/// Content identity for one staged or published append-only section record.
pub fn section_record_identity(record: &ReceiptSectionRecord) -> String {
    let mut identity = Vec::new();
    identity.extend_from_slice(RECEIPT_SECTION_RECORD_ID_MAGIC);
    record_frame(&mut identity, record.claim_key.as_bytes());
    record_frame(&mut identity, record.parent_digest.as_bytes());
    record_frame(&mut identity, record.section.name.as_bytes());
    record_frame(&mut identity, record.section.type_name.as_bytes());
    record_frame(&mut identity, record.section.schema_digest.as_bytes());
    record_frame(&mut identity, record.section.payload_digest.as_bytes());
    sha256_hex(&identity)
}

/// Encode an append-only record without exposing the section value in paths.
pub fn encode_section_record(record: &ReceiptSectionRecord) -> Result<Vec<u8>, String> {
    validate_section_record(record)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(RECEIPT_SECTION_RECORD_WIRE_MAGIC);
    record_frame(&mut bytes, record.claim_key.as_bytes());
    record_frame(&mut bytes, record.parent_digest.as_bytes());
    record_frame(&mut bytes, record.section.name.as_bytes());
    record_frame(&mut bytes, record.section.type_name.as_bytes());
    record_frame(&mut bytes, record.section.schema_digest.as_bytes());
    record_frame(&mut bytes, record.section.payload_digest.as_bytes());
    record_frame(&mut bytes, &record.section.bytes);
    if bytes.len() > MAX_RECEIPT_SECTION_RECORD_BYTES {
        return Err(format!(
            "receipt section record is too large (maximum {} bytes)",
            MAX_RECEIPT_SECTION_RECORD_BYTES
        ));
    }
    Ok(bytes)
}

/// Decode and authenticate one append-only section record.
pub fn decode_section_record(bytes: &[u8]) -> Result<ReceiptSectionRecord, String> {
    if bytes.len() > MAX_RECEIPT_SECTION_RECORD_BYTES {
        return Err("receipt section record is too large".into());
    }
    if !bytes.starts_with(RECEIPT_SECTION_RECORD_WIRE_MAGIC) {
        return Err("receipt section record magic is invalid".into());
    }
    let mut cursor = RECEIPT_SECTION_RECORD_WIRE_MAGIC.len();
    let claim_key = String::from_utf8(record_take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt section claim key is not UTF-8".to_string())?;
    let parent_digest = String::from_utf8(record_take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt section parent digest is not UTF-8".to_string())?;
    let name = String::from_utf8(record_take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt section name is not UTF-8".to_string())?;
    let type_name = String::from_utf8(record_take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt section type name is not UTF-8".to_string())?;
    let schema_digest = String::from_utf8(record_take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt section schema digest is not UTF-8".to_string())?;
    let payload_digest = String::from_utf8(record_take_frame(bytes, &mut cursor)?)
        .map_err(|_| "receipt section payload digest is not UTF-8".to_string())?;
    let value = record_take_frame(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err("receipt section record has trailing bytes".into());
    }
    ReceiptSectionRecord::new(
        claim_key,
        parent_digest,
        ReceiptSection::new_with_digests(
            name,
            type_name,
            schema_digest,
            payload_digest,
            value,
        )?,
    )
}


/// One changed, added, or removed field in a named section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptFieldDiff {
    pub section: String,
    /// Dot-separated object path. `@type` denotes a changed source type name;
    /// `$` denotes a whole section added or removed without object fields.
    pub field: String,
    pub before: Option<CanonicalJson>,
    pub after: Option<CanonicalJson>,
}

impl ReceiptFieldDiff {
    pub fn path(&self) -> String {
        if self.field == "$" {
            self.section.clone()
        } else if self.field == "@type" {
            format!("{}.@type", self.section)
        } else {
            format!("{}.{}", self.section, self.field)
        }
    }

    pub fn to_json(&self) -> CanonicalJson {
        CanonicalJson::object([
            (
                "after".into(),
                self.after.clone().unwrap_or(CanonicalJson::Null),
            ),
            ("before".into(), self.before.clone().unwrap_or(CanonicalJson::Null)),
            ("field".into(), CanonicalJson::String(self.field.clone())),
            (
                "section".into(),
                CanonicalJson::String(self.section.clone()),
            ),
        ])
        .expect("receipt field diff keys are unique")
    }

    /// Stable human projection used by receipt and performance comparisons.
    pub fn render(&self) -> String {
        format!(
            "{}: {} -> {}",
            self.path(),
            render_value(self.before.as_ref()),
            render_value(self.after.as_ref()),
        )
    }
}

/// Validate, sort, and clone a section list for receipt encoding.
pub fn normalized_sections(sections: &[ReceiptSection]) -> Result<Vec<ReceiptSection>, String> {
    if sections.len() > MAX_RECEIPT_SECTIONS {
        return Err(format!(
            "receipt has too many sections (maximum {})",
            MAX_RECEIPT_SECTIONS
        ));
    }
    let mut total = 0usize;
    let mut normalized = sections.to_vec();
    for section in &normalized {
        validate_section(section)?;
        total = total
            .checked_add(section.bytes.len())
            .ok_or_else(|| "receipt section byte count overflows".to_string())?;
    }
    if total > MAX_RECEIPT_SECTION_BYTES_TOTAL {
        return Err(format!(
            "receipt sections are too large (maximum {} bytes)",
            MAX_RECEIPT_SECTION_BYTES_TOTAL
        ));
    }
    normalized.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.type_name.cmp(&right.type_name))
            .then(left.bytes.cmp(&right.bytes))
    });
    for pair in normalized.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(format!("receipt section `{}` is duplicated", pair[0].name));
        }
    }
    Ok(normalized)
}

/// Project selected section values as a canonical object keyed by section name.
/// An empty selection means all sections. The output is sorted by name through
/// `CanonicalJson`'s object representation, independent of input order.
pub fn query_sections(
    sections: &[ReceiptSection],
    names: Option<&[String]>,
) -> Result<CanonicalJson, String> {
    let normalized = normalized_sections(sections)?;
    let requested = names
        .filter(|names| !names.is_empty())
        .map(|names| {
            let mut requested = BTreeSet::new();
            for name in names {
                validate_section_name(name)?;
                if !requested.insert(name.clone()) {
                    return Err(format!("receipt section `{name}` is duplicated in query"));
                }
            }
            Ok(requested)
        })
        .transpose()?;
    let mut values = Vec::new();
    for section in normalized {
        if requested
            .as_ref()
            .is_none_or(|requested| requested.contains(&section.name))
        {
            let value = section.value()?;
            values.push((section.name, value));
        }
    }
    CanonicalJson::object(values)
}

/// Return deterministic per-field changes between two receipt section lists.
pub fn diff_sections(
    before: &[ReceiptSection],
    after: &[ReceiptSection],
) -> Result<Vec<ReceiptFieldDiff>, String> {
    let before = normalized_sections(before)?;
    let after = normalized_sections(after)?;
    let before = before
        .into_iter()
        .map(|section| {
            let name = section.name.clone();
            Ok((name, section))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let after = after
        .into_iter()
        .map(|section| {
            let name = section.name.clone();
            Ok((name, section))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;

    let names = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut diffs = Vec::new();
    for name in names {
        match (before.get(&name), after.get(&name)) {
            (Some(before), Some(after)) => {
                if before.type_name != after.type_name {
                    diffs.push(ReceiptFieldDiff {
                        section: name.clone(),
                        field: "@type".into(),
                        before: Some(CanonicalJson::String(before.type_name.clone())),
                        after: Some(CanonicalJson::String(after.type_name.clone())),
                    });
                }
                let before_value = before.value()?;
                let after_value = after.value()?;
                diff_object_fields(&name, "", &before_value, &after_value, &mut diffs)?;
            }
            (Some(before), None) => {
                let value = before.value()?;
                diff_missing_section(&name, &value, true, &mut diffs);
            }
            (None, Some(after)) => {
                let value = after.value()?;
                diff_missing_section(&name, &value, false, &mut diffs);
            }
            (None, None) => unreachable!("section name set came from one of the maps"),
        }
    }
    Ok(diffs)
}

/// Encode field diffs as the shared canonical JSON array projection.
pub fn diff_projection(diffs: &[ReceiptFieldDiff]) -> CanonicalJson {
    CanonicalJson::Array(diffs.iter().map(ReceiptFieldDiff::to_json).collect())
}


fn validate_section(section: &ReceiptSection) -> Result<(), String> {
    validate_section_name(&section.name)?;
    validate_type_name(&section.type_name)?;
    validate_digest(&section.schema_digest, "schema")?;
    validate_digest(&section.payload_digest, "payload")?;
    if section.bytes.len() > MAX_RECEIPT_SECTION_BYTES {
        return Err(format!(
            "receipt section `{}` is too large (maximum {} bytes)",
            section.name, MAX_RECEIPT_SECTION_BYTES
        ));
    }
    let actual_payload = payload_digest(&section.bytes);
    if section.payload_digest != actual_payload {
        return Err(format!(
            "receipt section `{}` payload digest does not match its bytes",
            section.name
        ));
    }
    canonical_object(&section.bytes).map(|_| ()).map_err(|error| {
        format!(
            "receipt section `{}` has an invalid schema: {error}",
            section.name
        )
    })
}

fn validate_section_record(record: &ReceiptSectionRecord) -> Result<(), String> {
    validate_digest(&record.claim_key, "claim")?;
    if !record.parent_digest.is_empty() {
        validate_digest(&record.parent_digest, "parent")?;
    }
    validate_section(&record.section)
}

fn validate_digest(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("receipt section {label} digest is malformed"));
    }
    Ok(())
}

fn validate_section_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("receipt section name is empty".into());
    }
    if name.len() > MAX_RECEIPT_SECTION_NAME_BYTES {
        return Err(format!(
            "receipt section name is too long (maximum {} bytes)",
            MAX_RECEIPT_SECTION_NAME_BYTES
        ));
    }
    if !name
        .chars()
        .all(|character| !character.is_control() && character != '/' && character != '\\')
    {
        return Err(format!("receipt section name `{name}` is invalid"));
    }
    Ok(())
}

fn validate_type_name(type_name: &str) -> Result<(), String> {
    if type_name.is_empty() {
        return Err("receipt section type name is empty".into());
    }
    if type_name.len() > MAX_RECEIPT_SECTION_TYPE_BYTES {
        return Err(format!(
            "receipt section type name is too long (maximum {} bytes)",
            MAX_RECEIPT_SECTION_TYPE_BYTES
        ));
    }
    if !type_name.chars().all(|character| !character.is_control()) {
        return Err("receipt section type name is invalid".into());
    }
    Ok(())
}

fn canonical_object(bytes: &[u8]) -> Result<CanonicalJson, String> {
    let value = CanonicalJson::parse_canonical(bytes)
        .map_err(|error| format!("canonical JSON is invalid: {error}"))?;
    if !matches!(&value, CanonicalJson::Object(_)) {
        return Err("typed section value must be a canonical JSON object".into());
    }
    Ok(value)
}

fn diff_missing_section(
    section: &str,
    value: &CanonicalJson,
    removed: bool,
    diffs: &mut Vec<ReceiptFieldDiff>,
) {
    let CanonicalJson::Object(fields) = value else {
        diffs.push(ReceiptFieldDiff {
            section: section.into(),
            field: "$".into(),
            before: removed.then_some(value.clone()),
            after: (!removed).then_some(value.clone()),
        });
        return;
    };
    if fields.is_empty() {
        diffs.push(ReceiptFieldDiff {
            section: section.into(),
            field: "$".into(),
            before: removed.then_some(value.clone()),
            after: (!removed).then_some(value.clone()),
        });
        return;
    }
    for (field, child) in fields {
        let path = field.clone();
        if let CanonicalJson::Object(children) = child {
            if children.is_empty() {
                diffs.push(ReceiptFieldDiff {
                    section: section.into(),
                    field: path,
                    before: removed.then_some(child.clone()),
                    after: (!removed).then_some(child.clone()),
                });
            } else {
                diff_missing_object_fields(section, &path, children, removed, diffs);
            }
        } else {
            diffs.push(ReceiptFieldDiff {
                section: section.into(),
                field: path,
                before: removed.then_some(child.clone()),
                after: (!removed).then_some(child.clone()),
            });
        }
    }
}

fn diff_missing_object_fields(
    section: &str,
    prefix: &str,
    fields: &BTreeMap<String, CanonicalJson>,
    removed: bool,
    diffs: &mut Vec<ReceiptFieldDiff>,
) {
    for (field, child) in fields {
        let path = format!("{prefix}.{field}");
        if let CanonicalJson::Object(children) = child {
            if children.is_empty() {
                diffs.push(ReceiptFieldDiff {
                    section: section.into(),
                    field: path,
                    before: removed.then_some(child.clone()),
                    after: (!removed).then_some(child.clone()),
                });
            } else {
                diff_missing_object_fields(section, &path, children, removed, diffs);
            }
        } else {
            diffs.push(ReceiptFieldDiff {
                section: section.into(),
                field: path,
                before: removed.then_some(child.clone()),
                after: (!removed).then_some(child.clone()),
            });
        }
    }
}

fn diff_object_fields(
    section: &str,
    prefix: &str,
    before: &CanonicalJson,
    after: &CanonicalJson,
    diffs: &mut Vec<ReceiptFieldDiff>,
) -> Result<(), String> {
    let (CanonicalJson::Object(before), CanonicalJson::Object(after)) = (before, after) else {
        if before != after {
            diffs.push(ReceiptFieldDiff {
                section: section.into(),
                field: if prefix.is_empty() { "$".into() } else { prefix.into() },
                before: Some(before.clone()),
                after: Some(after.clone()),
            });
        }
        return Ok(());
    };
    let fields = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for field in fields {
        let path = if prefix.is_empty() {
            field.clone()
        } else {
            format!("{prefix}.{field}")
        };
        match (before.get(&field), after.get(&field)) {
            (Some(before), Some(after)) => {
                if matches!(before, CanonicalJson::Object(_))
                    && matches!(after, CanonicalJson::Object(_))
                {
                    diff_object_fields(section, &path, before, after, diffs)?;
                } else if before != after {
                    diffs.push(ReceiptFieldDiff {
                        section: section.into(),
                        field: path,
                        before: Some(before.clone()),
                        after: Some(after.clone()),
                    });
                }
            }
            (Some(before), None) => {
                diffs.push(ReceiptFieldDiff {
                    section: section.into(),
                    field: path,
                    before: Some(before.clone()),
                    after: None,
                });
            }
            (None, Some(after)) => {
                diffs.push(ReceiptFieldDiff {
                    section: section.into(),
                    field: path,
                    before: None,
                    after: Some(after.clone()),
                });
            }
            (None, None) => unreachable!("field set came from one of the objects"),
        }
    }
    Ok(())
}

fn render_value(value: Option<&CanonicalJson>) -> String {
    let Some(value) = value else {
        return "<missing>".into();
    };
    String::from_utf8(value.bytes())
        .map(|value| value.trim_end_matches('\n').to_string())
        .unwrap_or_else(|_| "<invalid>".into())
}
fn record_frame(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

fn record_take_frame(bytes: &[u8], cursor: &mut usize) -> Result<Vec<u8>, String> {
    let header_end = cursor
        .checked_add(8)
        .ok_or_else(|| "receipt section record frame length overflows".to_string())?;
    if header_end > bytes.len() {
        return Err("receipt section record frame length is truncated".into());
    }
    let mut length = [0u8; 8];
    length.copy_from_slice(&bytes[*cursor..header_end]);
    *cursor = header_end;
    let length = u64::from_be_bytes(length);
    let length = usize::try_from(length)
        .map_err(|_| "receipt section record frame is too large".to_string())?;
    if length > MAX_RECEIPT_SECTION_RECORD_BYTES {
        return Err("receipt section record frame is too large".into());
    }
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| "receipt section record frame end overflows".to_string())?;
    if end > bytes.len() {
        return Err("receipt section record frame is truncated".into());
    }
    let value = bytes[*cursor..end].to_vec();
    *cursor = end;
    Ok(value)
}
