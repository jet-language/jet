//! Checked ownership boundary for the Arrow C data interface (D-COLUMNAR-BOUNDARY1=A).
//!
//! This module deliberately has no Arrow crate dependency.  The C ABI types are
//! reproduced exactly, while ordinary callers use `ArrowSchemaSpec`,
//! `ArrowArraySpec`, and the immutable `ArrowImported` owner.  The C ABI remains
//! inside a vetted adapter.  The only unsafe operation exposed to callers is
//! `import_c`: the caller has opted into the foreign ABI contract and promises
//! that the two pointers describe live C records.  Private unsafe helpers
//! validate each representable layout before exposing a report or retaining or
//! copying the owner.

use std::any::Any;
use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr};
use std::fmt;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::{Arc, LazyLock, Mutex};
/// Arrow C schema nullable flag (`ARROW_FLAG_NULLABLE` in the C data ABI).
pub const ARROW_FLAG_NULLABLE: i64 = 2;
/// Arrow C schema dictionary ordering flag.
pub const ARROW_FLAG_DICTIONARY_ORDERED: i64 = 1;
/// Arrow C schema map-key ordering flag.
pub const ARROW_FLAG_MAP_KEYS_SORTED: i64 = 4;
/// Maximum C string inspected at the checked foreign boundary.
pub const ARROW_MAX_C_STRING_BYTES: usize = 4096;

/// A C data-interface schema record.  Jet never exposes this record to safe
/// user code; it is consumed by `unsafe import_c` and immediately reduced to a
/// checked schema specification.
#[repr(C)]
pub struct ArrowSchema {
    pub format: *const c_char,
    pub name: *const c_char,
    pub metadata: *const c_char,
    pub flags: i64,
    pub n_children: i64,
    pub children: *mut *mut ArrowSchema,
    pub dictionary: *mut ArrowSchema,
    pub release: Option<unsafe extern "C" fn(*mut ArrowSchema)>,
    pub private_data: *mut c_void,
}

/// A C data-interface array record.
#[repr(C)]
pub struct ArrowArray {
    pub length: i64,
    pub null_count: i64,
    pub offset: i64,
    pub n_buffers: i64,
    pub n_children: i64,
    pub buffers: *const *const c_void,
    pub children: *mut *mut ArrowArray,
    pub dictionary: *mut ArrowArray,
    pub release: Option<unsafe extern "C" fn(*mut ArrowArray)>,
    pub private_data: *mut c_void,
}

/// Stable registered result codes for malformed or refused Arrow imports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrowErrorCode {
    /// Supported schema or physical representation is absent.
    UnsupportedSchema,
    /// A length, offset, null count, child count, or buffer layout is invalid.
    MalformedLayout,
    /// The foreign owner cannot be claimed or has no release callback.
    Ownership,
    /// A configured row, child, dictionary, or byte ceiling was exceeded.
    Limit,
    /// An explicit no-copy request would require conversion or materialization.
    CopyRefused,
    /// The producer handed over an incomplete or already-released export.
    ProducerFailure,
}

impl ArrowErrorCode {
    /// Machine-readable diagnostic registration used by every Arrow result.
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedSchema => "E4301",
            Self::MalformedLayout => "E4302",
            Self::Ownership => "E4303",
            Self::Limit => "E4304",
            Self::CopyRefused => "E4305",
            Self::ProducerFailure => "E4306",
        }
    }
}

/// A declared Jet result rather than a panic or backend failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowError {
    pub code: &'static str,
    pub kind: ArrowErrorCode,
    pub operation: String,
    pub reason: String,
}

impl ArrowError {
    pub(crate) fn new(kind: ArrowErrorCode, operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            code: kind.code(),
            kind,
            operation: operation.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ArrowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.operation, self.reason)
    }
}

/// Limits applied before an imported layout can become a safe view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowLimits {
    pub max_rows: i64,
    pub max_columns: i64,
    pub max_children: i64,
    pub max_dictionary_values: i64,
    pub max_buffer_bytes: usize,
    pub max_total_bytes: usize,
}

impl ArrowLimits {
    pub fn safe() -> Self {
        Self {
            max_rows: 1_000_000,
            max_columns: 1024,
            max_children: 1024,
            max_dictionary_values: 1_000_000,
            max_buffer_bytes: 512 * 1024 * 1024,
            max_total_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

impl Default for ArrowLimits {
    fn default() -> Self {
        Self::safe()
    }
}

/// The explicit copy policy at the boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrowImportMode {
    /// Retain producer buffers and expose immutable views.
    Shared,
    /// Retain producer buffers only when no checked conversion is needed.
    NoCopy,
    /// Materialize independent Jet storage and disclose the measured bytes.
    Copy,
}

/// One physical scalar representation supported by the checked adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArrowPhysicalType {
    Boolean,
    Signed { bits: u16 },
    Unsigned { bits: u16 },
    Float { bits: u16 },
    Utf8,
    Binary,
}

impl ArrowPhysicalType {
    fn width(self) -> Option<usize> {
        match self {
            Self::Boolean => Some(1),
            Self::Signed { bits } | Self::Unsigned { bits } | Self::Float { bits } => {
                Some(usize::from(bits / 8))
            }
            Self::Utf8 | Self::Binary => None,
        }
    }

    fn parse(format: &str) -> Option<Self> {
        match format {
            "b" => Some(Self::Boolean),
            "c" => Some(Self::Signed { bits: 8 }),
            "s" => Some(Self::Signed { bits: 16 }),
            "i" => Some(Self::Signed { bits: 32 }),
            "l" => Some(Self::Signed { bits: 64 }),
            "C" => Some(Self::Unsigned { bits: 8 }),
            "S" => Some(Self::Unsigned { bits: 16 }),
            "I" => Some(Self::Unsigned { bits: 32 }),
            "L" => Some(Self::Unsigned { bits: 64 }),
            "e" => Some(Self::Float { bits: 16 }),
            "f" => Some(Self::Float { bits: 32 }),
            "g" => Some(Self::Float { bits: 64 }),
            "u" => Some(Self::Utf8),
            "z" => Some(Self::Binary),
            _ => None,
        }
    }
}

/// One logical schema field.  Dictionary fields store their value schema in
/// `dictionary`; their own `format` is the integer index representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowFieldSpec {
    pub name: String,
    pub format: String,
    pub nullable: bool,
    pub metadata: Option<String>,
    pub dictionary: Option<Box<ArrowFieldSpec>>,
}

impl ArrowFieldSpec {
    pub fn new(name: impl Into<String>, format: impl Into<String>, nullable: bool) -> Self {
        Self {
            name: name.into(),
            format: format.into(),
            nullable,
            metadata: None,
            dictionary: None,
        }
    }

    pub fn physical_type(&self) -> Result<ArrowPhysicalType, ArrowError> {
        ArrowPhysicalType::parse(&self.format).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::UnsupportedSchema,
                "arrow.schema",
                format!("unsupported physical format `{}`", self.format),
            )
        })
    }
}

/// Checked schema root retained by an import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowSchemaSpec {
    pub fields: Vec<ArrowFieldSpec>,
    pub metadata: Option<String>,
}

impl ArrowSchemaSpec {
    pub fn new(fields: Vec<ArrowFieldSpec>) -> Self {
        Self {
            fields,
            metadata: None,
        }
    }
}

/// A descriptor used by safe adapters and deterministic tests.  `bytes` is a
/// declared readable extent supplied by the adapter that owns the allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowBufferSpec {
    pub present: bool,
    pub bytes: usize,
}

impl ArrowBufferSpec {
    pub fn absent() -> Self {
        Self {
            present: false,
            bytes: 0,
        }
    }

    pub fn present(bytes: usize) -> Self {
        Self { present: true, bytes }
    }
}

/// A safe structural description of one Arrow array.  Raw C arrays are reduced
/// to this shape before validation; no safe user operation can index a pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowArraySpec {
    pub length: i64,
    pub null_count: i64,
    pub offset: i64,
    pub buffers: Vec<ArrowBufferSpec>,
    pub children: Vec<ArrowArraySpec>,
    pub dictionary: Option<Box<ArrowArraySpec>>,
    /// UTF-8/binary offsets, when the adapter can disclose them.  Raw imports
    /// fill this from the C offsets buffer and validate monotonicity.
    pub offsets: Option<Vec<i64>>,
}

impl ArrowArraySpec {
    pub fn primitive(length: i64, offset: i64, null_count: i64, value_bytes: usize) -> Self {
        Self {
            length,
            null_count,
            offset,
            buffers: vec![ArrowBufferSpec::absent(), ArrowBufferSpec::present(value_bytes)],
            children: Vec::new(),
            dictionary: None,
            offsets: None,
        }
    }

    pub fn utf8(
        length: i64,
        offset: i64,
        null_count: i64,
        offsets: Vec<i64>,
        data_bytes: usize,
    ) -> Self {
        Self {
            length,
            null_count,
            offset,
            buffers: vec![
                ArrowBufferSpec::absent(),
                ArrowBufferSpec::present(offsets.len().saturating_mul(4)),
                ArrowBufferSpec::present(data_bytes),
            ],
            children: Vec::new(),
            dictionary: None,
            offsets: Some(offsets),
        }
    }
}

/// Truthful accounting for the storage decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArrowCopyDisposition {
    Shared { retained_bytes: usize },
    Copied { copied_bytes: usize, reason: String },
}

impl ArrowCopyDisposition {
    pub fn is_shared(&self) -> bool {
        matches!(self, Self::Shared { .. })
    }
}

/// Measured result of one checked import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrowImportReport {
    pub rows: usize,
    pub columns: usize,
    pub dictionary_columns: usize,
    pub buffers_bytes: usize,
    pub disposition: ArrowCopyDisposition,
}

impl ArrowImportReport {
    pub fn copy_label(&self) -> &'static str {
        if self.disposition.is_shared() {
            "shared"
        } else {
            "copied"
        }
    }
}
/// One immutable scalar view into a validated Arrow array buffer.
///
/// The view borrows the checked batch.  It never owns or copies producer
/// storage; callers that need independent bytes must request
/// `ArrowImportMode::Copy`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArrowCell<'a> {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Utf8(&'a str),
    Binary(&'a [u8]),
}

#[derive(Clone, Debug, PartialEq)]
enum ArrowOwnedCell {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Utf8(String),
    Binary(Vec<u8>),
}

impl ArrowOwnedCell {
    fn as_cell(&self) -> ArrowCell<'_> {
        match self {
            Self::Null => ArrowCell::Null,
            Self::Bool(value) => ArrowCell::Bool(*value),
            Self::Signed(value) => ArrowCell::Signed(*value),
            Self::Unsigned(value) => ArrowCell::Unsigned(*value),
            Self::Float(value) => ArrowCell::Float(*value),
            Self::Utf8(value) => ArrowCell::Utf8(value),
            Self::Binary(value) => ArrowCell::Binary(value),
        }
    }
}

/// Materialized values for an explicit copy import.  The foreign owner is
/// released before this storage is exposed, so copied cells never borrow C
/// memory.
#[derive(Clone, Debug, PartialEq)]
struct ArrowOwnedStorage {
    rows: Vec<Vec<ArrowOwnedCell>>,
}

/// A checked immutable owner.  Cloning this handle only clones the owner token;
#[derive(Clone)]
pub struct ArrowImported<Row = ()> {
    pub schema: ArrowSchemaSpec,
    pub report: ArrowImportReport,
    owner: Option<Arc<ArrowOwner>>,
    /// Keeps a dynamically loaded provider library alive while shared Arrow
    /// release callbacks may still run.
    provider_owner: Option<Arc<dyn Any + Send + Sync>>,
    owned: Option<Arc<ArrowOwnedStorage>>,
    limits: ArrowLimits,
    _row: PhantomData<*const Row>,
}

/// Canonical typed-boundary spelling used by data adapters.
pub type ColumnBatch<Row = ()> = ArrowImported<Row>;
/// A row model supplies the typed read hook used after boundary validation.
///
/// Core query code implements this once for each row model.  The Arrow
/// boundary never invents field defaults: a decoder must either construct the
/// requested row or return the registered Arrow error.
pub trait ArrowRow: Sized {
    fn from_arrow(batch: &ColumnBatch<Self>, row: usize) -> Result<Self, ArrowError>;
}

impl ArrowRow for () {
    fn from_arrow(batch: &ColumnBatch<Self>, row: usize) -> Result<Self, ArrowError> {
        if row >= batch.report.rows {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.row",
                "row index is outside the checked batch",
            ));
        }
        Ok(())
    }
}

impl<Row> ArrowImported<Row> {
    /// Decode one checked row through the row model selected by `ColumnBatch<Row>`.
    pub fn row(&self, row: usize) -> Result<Row, ArrowError>
    where
        Row: ArrowRow,
    {
        if row >= self.report.rows {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.row",
                "row index is outside the checked batch",
            ));
        }
        Row::from_arrow(self, row)
    }
}
impl<Row> ArrowImported<Row> {
    /// Read one scalar from the checked batch.
    ///
    /// Shared imports borrow producer storage.  Copy imports borrow only the
    /// materialized Jet storage retained by this handle; they never expose a
    /// pointer after the producer owner has been released.
    pub fn cell(&self, row: usize, column: usize) -> Result<ArrowCell<'_>, ArrowError> {
        if row >= self.report.rows {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.cell",
                "row index is outside the checked batch",
            ));
        }
        let field = self.schema.fields.get(column).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.cell",
                "column index is outside the checked batch",
            )
        })?;
        if let Some(owned) = self.owned.as_ref() {
            let value = owned
                .rows
                .get(row)
                .and_then(|values| values.get(column))
                .ok_or_else(|| {
                    ArrowError::new(
                        ArrowErrorCode::ProducerFailure,
                        "arrow.cell",
                        "copied batch does not contain the checked cell",
                    )
                })?;
            return Ok(value.as_cell());
        }
        let owner = self.owner.as_ref().ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::ProducerFailure,
                "arrow.cell",
                "safe structural descriptors do not carry value buffers",
            )
        })?;
        let root = owner.array.ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::ProducerFailure,
                "arrow.cell",
                "imported batch has no root array",
            )
        })?;
        // SAFETY: `owner` exists only for `import_c` imports.  That path
        // claimed both base pointers, validated the root child count and
        // retained the producer release state in this Arc.  The borrow of
        // `self` keeps the base unreleased for the whole scalar view.
        let child = unsafe {
            if root.as_ref().n_children < 0
                || column >= usize::try_from(root.as_ref().n_children).unwrap_or(0)
                || root.as_ref().children.is_null()
            {
                return Err(ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.cell",
                    "root array has no checked column child",
                ));
            }
            if root.as_ref().n_buffers > 0 {
                if root.as_ref().n_buffers != 1 || root.as_ref().buffers.is_null() {
                    return Err(ArrowError::new(
                        ArrowErrorCode::MalformedLayout,
                        "arrow.cell",
                        "root validity buffer table is invalid",
                    ));
                }
                let validity = *root.as_ref().buffers;
                if !validity.is_null() {
                    let validity_index = usize::try_from(root.as_ref().offset)
                        .ok()
                        .and_then(|offset| offset.checked_add(row))
                        .ok_or_else(|| {
                            ArrowError::new(
                                ArrowErrorCode::MalformedLayout,
                                "arrow.cell",
                                "root validity index overflows",
                            )
                        })?;
                    let byte = *((validity as *const u8).add(validity_index / 8));
                    if byte & (1u8 << (validity_index % 8)) == 0 {
                        return Ok(ArrowCell::Null);
                    }
                }
            }
            let child = *root.as_ref().children.add(column);
            NonNull::new(child).ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.cell",
                    "root array column child is null",
                )
            })?
        };
        // SAFETY: the child pointer was checked above and remains owned by
        // the same Arc; `read_arrow_cell` checks this offset against the
        // child length before touching any value buffer.
        let index = unsafe {
            usize::try_from(child.as_ref().offset)
                .ok()
                .and_then(|offset| offset.checked_add(row))
                .ok_or_else(|| {
                    ArrowError::new(
                        ArrowErrorCode::MalformedLayout,
                        "arrow.cell",
                        "column row offset overflows",
                    )
                })?
        };
        // SAFETY: import_c checked schema, offsets, lengths, nullability,
        // dictionary declarations, and buffer presence; the helper repeats
        // index/range checks immediately before each read.
        unsafe { read_arrow_cell(child.as_ptr(), field, index, 0, &self.limits) }
    }
}

unsafe fn read_arrow_cell<'a>(
    raw: *mut ArrowArray,
    field: &ArrowFieldSpec,
    index: usize,
    depth: usize,
    limits: &'a ArrowLimits,
) -> Result<ArrowCell<'a>, ArrowError> {
    let length = usize::try_from((*raw).length).map_err(|_| {
        ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.cell",
            "array length is negative or too large",
        )
    })?;
    let offset = usize::try_from((*raw).offset).map_err(|_| {
        ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.cell",
            "array offset is negative or too large",
        )
    })?;
    let end = offset.checked_add(length).ok_or_else(|| {
        ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.cell",
            "array index range overflows",
        )
    })?;
    if index < offset || index >= end {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.cell",
            "array index is outside the checked extent",
        ));
    }
    if depth > limits.max_children.max(0) as usize {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.cell",
            "dictionary nesting exceeds configured limit",
        ));
    }
    if let Some(dictionary) = field.dictionary.as_deref() {
        let key = read_arrow_cell_inner(raw, field, index)?;
        let key = match key {
            ArrowCell::Signed(value) if value >= 0 => usize::try_from(value).ok(),
            ArrowCell::Unsigned(value) => usize::try_from(value).ok(),
            _ => None,
        }
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.cell",
                "dictionary index is not a non-negative integer",
            )
        })?;
        let dictionary_raw = NonNull::new((*raw).dictionary).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.cell",
                "dictionary array is missing",
            )
        })?;
        let dictionary_index = usize::try_from((*dictionary_raw.as_ptr()).offset)
            .ok()
            .and_then(|offset| offset.checked_add(key))
            .ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.cell",
                    "dictionary index overflows",
                )
            })?;
        return read_arrow_cell(
            dictionary_raw.as_ptr(),
            dictionary,
            dictionary_index,
            depth + 1,
            limits,
        );
    }
    read_arrow_cell_inner(raw, field, index)
}

unsafe fn read_arrow_cell_inner<'a>(
    raw: *mut ArrowArray,
    field: &ArrowFieldSpec,
    index: usize,
) -> Result<ArrowCell<'a>, ArrowError> {
    let buffer = |number: usize| -> Result<*const std::ffi::c_void, ArrowError> {
        if (*raw).n_buffers < 0 || number >= (*raw).n_buffers as usize || (*raw).buffers.is_null() {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.cell",
                "array buffer table is invalid",
            ));
        }
        Ok(*(*raw).buffers.add(number))
    };
    let validity = buffer(0)?;
    if !validity.is_null() {
        let byte = *((validity as *const u8).add(index / 8));
        if byte & (1u8 << (index % 8)) == 0 {
            return Ok(ArrowCell::Null);
        }
    }
    let physical = field.physical_type()?;
    let values = buffer(1)?;
    if values.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.cell",
            "array value buffer is missing",
        ));
    }
    let cell = match physical {
        ArrowPhysicalType::Boolean => {
            let byte = *((values as *const u8).add(index / 8));
            ArrowCell::Bool(byte & (1u8 << (index % 8)) != 0)
        }
        ArrowPhysicalType::Signed { bits: 8 } => {
            ArrowCell::Signed(std::ptr::read_unaligned((values as *const i8).add(index)) as i64)
        }
        ArrowPhysicalType::Signed { bits: 16 } => ArrowCell::Signed(
            std::ptr::read_unaligned((values as *const i16).add(index)) as i64,
        ),
        ArrowPhysicalType::Signed { bits: 32 } => ArrowCell::Signed(
            std::ptr::read_unaligned((values as *const i32).add(index)) as i64,
        ),
        ArrowPhysicalType::Signed { bits: 64 } => {
            ArrowCell::Signed(std::ptr::read_unaligned((values as *const i64).add(index)))
        }
        ArrowPhysicalType::Unsigned { bits: 8 } => ArrowCell::Unsigned(
            std::ptr::read_unaligned((values as *const u8).add(index)) as u64,
        ),
        ArrowPhysicalType::Unsigned { bits: 16 } => ArrowCell::Unsigned(
            std::ptr::read_unaligned((values as *const u16).add(index)) as u64,
        ),
        ArrowPhysicalType::Unsigned { bits: 32 } => ArrowCell::Unsigned(
            std::ptr::read_unaligned((values as *const u32).add(index)) as u64,
        ),
        ArrowPhysicalType::Unsigned { bits: 64 } => {
            ArrowCell::Unsigned(std::ptr::read_unaligned((values as *const u64).add(index)))
        }
        ArrowPhysicalType::Float { bits: 16 } => {
            ArrowCell::Float(f16_to_f64(std::ptr::read_unaligned(
                (values as *const u16).add(index),
            )))
        }
        ArrowPhysicalType::Float { bits: 32 } => ArrowCell::Float(
            std::ptr::read_unaligned((values as *const f32).add(index)) as f64,
        ),
        ArrowPhysicalType::Float { bits: 64 } => {
            ArrowCell::Float(std::ptr::read_unaligned((values as *const f64).add(index)))
        }
        ArrowPhysicalType::Utf8 | ArrowPhysicalType::Binary => {
            let offsets = values;
            let next = index.checked_add(1).ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.cell",
                    "offset index overflows",
                )
            })?;
            let start = i64::from(std::ptr::read_unaligned(
                (offsets as *const i32).add(index),
            ));
            let end = i64::from(std::ptr::read_unaligned(
                (offsets as *const i32).add(next),
            ));
            if start < 0 || end < start {
                return Err(ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.cell",
                    "offsets are not monotonic",
                ));
            }
            let data = buffer(2)?;
            if data.is_null() {
                return Err(ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.cell",
                    "variable-width data buffer is missing",
                ));
            }
            let bytes = std::slice::from_raw_parts(
                (data as *const u8).add(usize::try_from(start).map_err(|_| {
                    ArrowError::new(
                        ArrowErrorCode::MalformedLayout,
                        "arrow.cell",
                        "offset is too large",
                    )
                })?),
                usize::try_from(end - start).map_err(|_| {
                    ArrowError::new(
                        ArrowErrorCode::MalformedLayout,
                        "arrow.cell",
                        "value length is too large",
                    )
                })?,
            );
            match physical {
                ArrowPhysicalType::Utf8 => {
                    ArrowCell::Utf8(std::str::from_utf8(bytes).map_err(|_| {
                        ArrowError::new(
                            ArrowErrorCode::MalformedLayout,
                            "arrow.cell",
                            "UTF-8 value buffer is invalid",
                        )
                    })?)
                }
                ArrowPhysicalType::Binary => ArrowCell::Binary(bytes),
                _ => unreachable!(),
            }
        }
        _ => {
            return Err(ArrowError::new(
                ArrowErrorCode::UnsupportedSchema,
                "arrow.cell",
                "physical type is not readable through the scalar view",
            ))
        }
    };
    Ok(cell)
}

fn f16_to_f64(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let fraction = (bits & 0x03ff) as u32;
    match exponent {
        0 if fraction == 0 => sign * 0.0,
        0 => sign * f64::from(fraction) * 2f64.powi(-24),
        0x1f if fraction == 0 => sign * f64::INFINITY,
        0x1f => f64::NAN,
        exponent => sign * (1.0 + f64::from(fraction) / 1024.0) * 2f64.powi(exponent - 15),
    }
}


impl<Row> fmt::Debug for ArrowImported<Row> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArrowImported")
            .field("schema", &self.schema)
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl<Row> ArrowImported<Row> {
    /// Import a safe descriptor.  This is the path used by host adapters that
    /// already own their bytes; it never creates a foreign pointer or callback.
    pub fn from_spec(
        schema: ArrowSchemaSpec,
        array: ArrowArraySpec,
        expected: Option<&[ArrowFieldSpec]>,
        limits: &ArrowLimits,
        mode: ArrowImportMode,
    ) -> Result<Self, ArrowError> {
        let report = validate_import(&schema, &array, expected, limits, mode)?;
        Ok(Self {
            schema,
            report,
            owner: None,
            provider_owner: None,
            owned: None,
            limits: limits.clone(),
            _row: PhantomData,
        })
    }

    /// Import one producer-owned C export.
    ///
    /// # Safety
    /// `schema` and `array` must each point to a live Arrow C record whose
    /// pointers and callback obey the C data-interface contract until the
    /// returned owner is dropped.  The callback must release the complete
    /// record tree and must not release the top-level record twice.  This is
    /// the sole unsafe operation exposed by the adapter.
    pub unsafe fn import_c(
        schema: *mut ArrowSchema,
        array: *mut ArrowArray,
        expected: Option<&[ArrowFieldSpec]>,
        limits: &ArrowLimits,
        mode: ArrowImportMode,
    ) -> Result<Self, ArrowError> {
        Self::import_c_with_provider_owner(schema, array, expected, limits, mode, None)
    }

    pub(crate) unsafe fn import_c_with_provider_owner(
        schema: *mut ArrowSchema,
        array: *mut ArrowArray,
        expected: Option<&[ArrowFieldSpec]>,
        limits: &ArrowLimits,
        mode: ArrowImportMode,
        provider_owner: Option<Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self, ArrowError> {
        let schema = match NonNull::new(schema) {
            Some(schema) => schema,
            None => {
                release_unclaimed_records(std::ptr::null_mut(), array);
                return Err(ArrowError::new(
                    ArrowErrorCode::ProducerFailure,
                    "arrow.import",
                    "producer returned a null schema",
                ));
            }
        };
        let array = match NonNull::new(array) {
            Some(array) => array,
            None => {
                release_unclaimed_records(schema.as_ptr(), std::ptr::null_mut());
                return Err(ArrowError::new(
                    ArrowErrorCode::ProducerFailure,
                    "arrow.import",
                    "producer returned a null array",
                ));
            }
        };
        if (*schema.as_ptr()).release.is_none() || (*array.as_ptr()).release.is_none() {
            release_unclaimed_records(schema.as_ptr(), array.as_ptr());
            return Err(ArrowError::new(
                ArrowErrorCode::Ownership,
                "arrow.import",
                "producer export has no release callback",
            ));
        }
        if let Err(error) = claim_pointer(schema.as_ptr() as usize, "schema") {
            if schema.as_ptr().cast::<c_void>() != array.as_ptr().cast::<c_void>()
                && !pointer_is_claimed(array.as_ptr() as usize)
            {
                release_unclaimed_records(std::ptr::null_mut(), array.as_ptr());
            }
            return Err(error);
        }
        if let Err(error) = claim_pointer(array.as_ptr() as usize, "array") {
            release_claim(schema.as_ptr() as usize);
            release_unclaimed_records(schema.as_ptr(), std::ptr::null_mut());
            return Err(error);
        }
        // Claim ownership before any fallible schema/layout work.  An import
        // rejection must still release both producer records exactly once.
        let owner = Arc::new(ArrowOwner {
            schema: Some(schema),
            array: Some(array),
        });
        let schema_spec = read_schema(schema.as_ptr(), limits)?;
        let array_spec = read_root_array(array.as_ptr(), &schema_spec, limits)?;
        let mut report = validate_import(&schema_spec, &array_spec, expected, limits, mode)?;
        let (owner, owned, provider_owner) = if matches!(mode, ArrowImportMode::Copy) {
            let storage = copy_owned_cells(array.as_ptr(), &schema_spec, &array_spec, limits)?;
            drop(owner);
            (
                None,
                Some(Arc::new(storage)),
                None,
            )
        } else {
            (Some(owner), None, provider_owner)
        };
        if matches!(mode, ArrowImportMode::Copy) {
            // `buffers_bytes` remains the producer's declared source extent;
            // the disposition explicitly records that values were materialized
            // before the producer owner was released.
            if let ArrowCopyDisposition::Copied { reason, .. } = &mut report.disposition {
                *reason = "materialized scalar values before releasing producer buffers".to_string();
            }
        }
        Ok(Self {
            schema: schema_spec,
            report,
            owner,
            provider_owner,
            owned,
            limits: limits.clone(),
            _row: PhantomData,
        })
    }
}

/// Release records that never entered the global claim registry.  This is only
/// used for producer handoff failures, before an `ArrowOwner` exists.
unsafe fn release_unclaimed_records(
    schema: *mut ArrowSchema,
    array: *mut ArrowArray,
) {
    if !schema.is_null() {
        if let Some(release) = (*schema).release {
            release(schema);
        }
    }
    if !array.is_null() && array.cast::<c_void>() != schema.cast::<c_void>() {
        if let Some(release) = (*array).release {
            release(array);
        }
    }
}

impl<Row> Drop for ArrowImported<Row> {
    fn drop(&mut self) {
        // The Arc is the sole owner of release state.  Cloned handles do not
        // call callbacks themselves and therefore cannot double-release.
        let _ = self.owner.take();
        let _ = self.provider_owner.take();
    }
}

struct ArrowOwner {
    schema: Option<NonNull<ArrowSchema>>,
    array: Option<NonNull<ArrowArray>>,
}

// The owner never moves the records or sends them to another thread.  Keep this
// type !Send/!Sync by construction; `ArrowImported` is a process-local view.
impl Drop for ArrowOwner {
    fn drop(&mut self) {
        if let Some(schema) = self.schema.take() {
            // SAFETY: the producer supplied a live schema and release callback;
            // this is the one owner Drop path and the pointer is claimed in the
            // process registry until this callback returns.
            unsafe {
                if let Some(release) = (*schema.as_ptr()).release {
                    release(schema.as_ptr());
                }
            }
            release_claim(schema.as_ptr() as usize);
        }
        if let Some(array) = self.array.take() {
            // SAFETY: same argument as for the schema; child callbacks are
            // intentionally not called here because the producer's base release
            // callback owns that recursive release rule.
            unsafe {
                if let Some(release) = (*array.as_ptr()).release {
                    release(array.as_ptr());
                }
            }
            release_claim(array.as_ptr() as usize);
        }
    }
}

static CLAIMED: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn claimed_pointers() -> &'static Mutex<HashSet<usize>> {
    &CLAIMED
}

fn claim_pointer(pointer: usize, what: &str) -> Result<(), ArrowError> {
    let mut claimed = claimed_pointers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !claimed.insert(pointer) {
        return Err(ArrowError::new(
            ArrowErrorCode::Ownership,
            "arrow.import",
            format!("{what} is already owned by another Arrow import"),
        ));
    }
    Ok(())
}

fn pointer_is_claimed(pointer: usize) -> bool {
    claimed_pointers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&pointer)
}

fn release_claim(pointer: usize) {
    let mut claimed = claimed_pointers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    claimed.remove(&pointer);
}

/// Validate a safe descriptor and return truthful copy/lifetime accounting.
pub fn validate_import(
    schema: &ArrowSchemaSpec,
    array: &ArrowArraySpec,
    expected: Option<&[ArrowFieldSpec]>,
    limits: &ArrowLimits,
    mode: ArrowImportMode,
) -> Result<ArrowImportReport, ArrowError> {
    validate_schema(
        schema,
        expected,
        limits,
        matches!(mode, ArrowImportMode::Copy),
    )?;
    if array.length < 0 || array.offset < 0 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "length and offset must be non-negative",
        ));
    }
    if array.length > limits.max_rows {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            format!("row count {} exceeds limit {}", array.length, limits.max_rows),
        ));
    }
    if array.children.len() > usize::try_from(limits.max_children).unwrap_or(0) {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            "child count exceeds configured limit",
        ));
    }
    if array.children.len() != schema.fields.len() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!(
                "root child count {} does not match schema field count {}",
                array.children.len(),
                schema.fields.len()
            ),
        ));
    }
    if array.null_count < -1 || array.null_count > array.length {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root array has an invalid null count",
        ));
    }
    if array.buffers.len() > 1 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root struct array exposes more than one buffer",
        ));
    }
    if array.null_count > 0
        && !array
            .buffers
            .first()
            .is_some_and(|buffer| buffer.present)
    {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root array has nulls but no validity buffer",
        ));
    }
    let mut total_bytes = 0usize;
    for buffer in &array.buffers {
        if buffer.present && buffer.bytes > limits.max_buffer_bytes {
            return Err(ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "root validity buffer exceeds byte limit",
            ));
        }
        total_bytes = total_bytes.checked_add(buffer.bytes).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "root buffer byte total overflow",
            )
        })?;
    }
    if let Some(validity) = array.buffers.first().filter(|buffer| buffer.present) {
        let count = usize::try_from(array.offset)
            .ok()
            .and_then(|offset| usize::try_from(array.length).ok()?.checked_add(offset))
            .ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.array",
                    "root validity range overflows",
                )
            })?;
        let required = count.checked_add(7).map(|bits| bits / 8).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "root validity byte count overflows",
            )
        })?;
        if validity.bytes < required {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                "root validity buffer is shorter than its logical range",
            ));
        }
    }
    let mut dictionary_columns = 0usize;
    for (field, child) in schema.fields.iter().zip(&array.children) {
        let bytes = validate_array(field, child, limits, 0)?;
        total_bytes = total_bytes.checked_add(bytes).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "buffer byte total overflow",
            )
        })?;
        if field.dictionary.is_some() {
            dictionary_columns += 1;
        }
    }
    if total_bytes > limits.max_total_bytes {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            format!("buffer bytes {total_bytes} exceed limit {}", limits.max_total_bytes),
        ));
    }
    let disposition = match mode {
        ArrowImportMode::Shared | ArrowImportMode::NoCopy => {
            ArrowCopyDisposition::Shared {
                retained_bytes: total_bytes,
            }
        }
        ArrowImportMode::Copy => ArrowCopyDisposition::Copied {
            copied_bytes: total_bytes,
            reason: "explicit owned import requested".to_string(),
        },
    };
    Ok(ArrowImportReport {
        rows: usize::try_from(array.length).map_err(|_| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                "row count does not fit the host index type",
            )
        })?,
        columns: schema.fields.len(),
        dictionary_columns,
        buffers_bytes: total_bytes,
        disposition,
    })
}

fn validate_schema(
    schema: &ArrowSchemaSpec,
    expected: Option<&[ArrowFieldSpec]>,
    limits: &ArrowLimits,
    allow_conversion: bool,
) -> Result<(), ArrowError> {
    if schema.fields.len() > usize::try_from(limits.max_columns).unwrap_or(0) {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.schema",
            "column count exceeds configured limit",
        ));
    }
    let mut names = HashSet::new();
    for field in &schema.fields {
        if field.name.is_empty() || !names.insert(field.name.clone()) {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.schema",
                "field names must be non-empty and unique",
            ));
        }
        field.physical_type()?;
        if let Some(dictionary) = &field.dictionary {
            let index = field.physical_type()?;
            if !matches!(index, ArrowPhysicalType::Signed { .. } | ArrowPhysicalType::Unsigned { .. })
            {
                return Err(ArrowError::new(
                    ArrowErrorCode::UnsupportedSchema,
                    "arrow.schema",
                    format!("dictionary index `{}` is not an integer", field.format),
                ));
            }
            if dictionary.dictionary.is_some() {
                return Err(ArrowError::new(
                    ArrowErrorCode::UnsupportedSchema,
                    "arrow.schema",
                    "nested dictionaries are not supported",
                ));
            }
            dictionary.physical_type()?;
        }
    }
    if let Some(expected) = expected {
        if expected.len() != schema.fields.len() {
            if !allow_conversion {
                return Err(ArrowError::new(
                    ArrowErrorCode::CopyRefused,
                    "arrow.schema",
                    "requested row schema has a different column count",
                ));
            }
        } else {
            for (actual, wanted) in schema.fields.iter().zip(expected) {
                if actual.name != wanted.name || actual.format != wanted.format {
                    if !allow_conversion {
                        return Err(ArrowError::new(
                            ArrowErrorCode::CopyRefused,
                            "arrow.schema",
                            format!(
                                "field `{}` requires conversion from `{}` to `{}`",
                                actual.name, actual.format, wanted.format
                            ),
                        ));
                    }
                }
                if !wanted.nullable && actual.nullable && !allow_conversion {
                    return Err(ArrowError::new(
                        ArrowErrorCode::CopyRefused,
                        "arrow.schema",
                        format!("nullable field `{}` cannot fill a non-nullable row", actual.name),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_array(
    field: &ArrowFieldSpec,
    array: &ArrowArraySpec,
    limits: &ArrowLimits,
    depth: i64,
) -> Result<usize, ArrowError> {
    if depth > limits.max_children {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            "nested child depth exceeds configured limit",
        ));
    }
    if array.length < 0 || array.offset < 0 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has a negative length or offset", field.name),
        ));
    }
    if array.length > limits.max_rows {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            format!("field `{}` exceeds row limit", field.name),
        ));
    }
    if array.null_count < -1 || array.null_count > array.length {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has an invalid null count", field.name),
        ));
    }
    if array.null_count > 0 && !array.buffers.first().is_some_and(|buffer| buffer.present) {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has nulls but no validity buffer", field.name),
        ));
    }
    let physical = field.physical_type()?;
    let expected_buffers = if matches!(physical, ArrowPhysicalType::Utf8 | ArrowPhysicalType::Binary) {
        3
    } else {
        2
    };
    if array.buffers.len() != expected_buffers {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!(
                "field `{}` has {} buffers; expected {expected_buffers}",
                field.name,
                array.buffers.len()
            ),
        ));
    }
    let mut bytes = 0usize;
    for buffer in &array.buffers {
        if buffer.present && buffer.bytes > limits.max_buffer_bytes {
            return Err(ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("field `{}` buffer exceeds byte limit", field.name),
            ));
        }
        bytes = bytes.checked_add(buffer.bytes).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "buffer byte total overflow",
            )
        })?;
    }
    if let Some(width) = physical.width() {
        let count = usize::try_from(array.offset)
            .ok()
            .and_then(|offset| usize::try_from(array.length).ok()?.checked_add(offset))
            .ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.array",
                    format!("field `{}` index range overflows", field.name),
                )
            })?;
        let required = if matches!(physical, ArrowPhysicalType::Boolean) {
            count.checked_add(7).map(|rounded| rounded / 8)
        } else {
            count.checked_mul(width)
        }
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("field `{}` value byte count overflows", field.name),
            )
        })?;
        if required > 0 && (!array.buffers[1].present || array.buffers[1].bytes < required) {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` values buffer is shorter than its logical range", field.name),
            ));
        }
    } else {
        let offsets = array.offsets.as_ref().ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` does not disclose variable-width offsets", field.name),
            )
        })?;
        let count = usize::try_from(array.offset)
            .ok()
            .and_then(|offset| usize::try_from(array.length).ok()?.checked_add(offset))
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.array",
                    format!("field `{}` offset range overflows", field.name),
                )
            })?;
        if offsets.len() < count {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` offsets buffer is too short", field.name),
            ));
        }
        let start = usize::try_from(array.offset).map_err(|_| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` offset does not fit host index", field.name),
            )
        })?;
        let mut previous = offsets[start];
        if previous < 0 {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` has a negative first offset", field.name),
            ));
        }
        for &next in &offsets[start + 1..count] {
            if next < previous {
                return Err(ArrowError::new(
                    ArrowErrorCode::MalformedLayout,
                    "arrow.array",
                    format!("field `{}` offsets are not monotonic", field.name),
                ));
            }
            previous = next;
        }
        let data_bytes = usize::try_from(previous).map_err(|_| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` final offset is too large", field.name),
            )
        })?;
        let offsets_bytes = count.checked_mul(4).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("field `{}` offsets byte count overflows", field.name),
            )
        })?;
        if offsets_bytes > 0 && (!array.buffers[1].present || array.buffers[1].bytes < offsets_bytes) {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` offsets buffer is shorter than its logical range", field.name),
            ));
        }
        if data_bytes > 0 && (!array.buffers[2].present || array.buffers[2].bytes < data_bytes) {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` data buffer is shorter than its final offset", field.name),
            ));
        }
    }
    if array.children.len() != 0 {
        return Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.array",
            format!("field `{}` has unsupported child arrays", field.name),
        ));
    }
    if let Some(dictionary_field) = &field.dictionary {
        let dictionary = array.dictionary.as_ref().ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("dictionary field `{}` has no dictionary array", field.name),
            )
        })?;
        if dictionary.length > limits.max_dictionary_values {
            return Err(ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("dictionary field `{}` exceeds value limit", field.name),
            ));
        }
        bytes = bytes.checked_add(validate_array(dictionary_field, dictionary, limits, depth + 1)?).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "dictionary byte total overflow",
            )
        })?;
    } else if array.dictionary.is_some() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has an undeclared dictionary", field.name),
        ));
    }
    Ok(bytes)
}

unsafe fn read_schema(raw: *mut ArrowSchema, limits: &ArrowLimits) -> Result<ArrowSchemaSpec, ArrowError> {
    let format = read_c_string((*raw).format, "schema format")?;
    let name = read_c_string((*raw).name, "schema name")?;
    let metadata = if (*raw).metadata.is_null() {
        None
    } else {
        Some(read_c_string((*raw).metadata, "schema metadata")?)
    };
    if format != "+s" && !format.is_empty() {
        return Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.schema",
            "root schema must use the Arrow struct format `+s`",
        ));
    }
    if (*raw).n_children < 0 || (*raw).n_children > limits.max_columns {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.schema",
            "schema child count exceeds configured limit",
        ));
    }
    if (*raw).n_children > 0 && (*raw).children.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.schema",
            "schema declares children without a child pointer",
        ));
    }
    let mut fields = Vec::with_capacity((*raw).n_children as usize);
    for index in 0..(*raw).n_children as usize {
        let child = *(*raw).children.add(index);
        let child = NonNull::new(child).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.schema",
                format!("schema child {index} is null"),
            )
        })?;
        fields.push(read_field(child.as_ptr(), limits, 0)?);
    }
    let _ = name;
    Ok(ArrowSchemaSpec { fields, metadata })
}

unsafe fn read_field(
    raw: *mut ArrowSchema,
    limits: &ArrowLimits,
    depth: i64,
) -> Result<ArrowFieldSpec, ArrowError> {
    if depth > limits.max_children {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.schema",
            "dictionary nesting exceeds configured limit",
        ));
    }
    let format = read_c_string((*raw).format, "field format")?;
    let name = read_c_string((*raw).name, "field name")?;
    let metadata = if (*raw).metadata.is_null() {
        None
    } else {
        Some(read_c_string((*raw).metadata, "field metadata")?)
    };
    let dictionary = if (*raw).dictionary.is_null() {
        None
    } else {
        Some(Box::new(read_field((*raw).dictionary, limits, depth + 1)?))
    };
    Ok(ArrowFieldSpec {
        name,
        format,
        nullable: (*raw).flags & ARROW_FLAG_NULLABLE != 0,
        metadata,
        dictionary,
    })
}

unsafe fn read_root_array(
    raw: *mut ArrowArray,
    schema: &ArrowSchemaSpec,
    limits: &ArrowLimits,
) -> Result<ArrowArraySpec, ArrowError> {
    if (*raw).n_buffers < 0 || (*raw).n_buffers > 1 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root struct array must expose zero or one validity buffer",
        ));
    }
    if (*raw).n_buffers == 1 && (*raw).buffers.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root struct array has no validity buffer table",
        ));
    }
    if (*raw).length < 0 || (*raw).offset < 0 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root length and offset must be non-negative",
        ));
    }
    if (*raw).length > limits.max_rows {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            "root row count exceeds configured limit",
        ));
    }
    let root_count = usize::try_from((*raw).offset)
        .ok()
        .and_then(|offset| usize::try_from((*raw).length).ok()?.checked_add(offset))
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                "root index range overflows",
            )
        })?;
    let max_index = usize::try_from(limits.max_rows)
        .ok()
        .and_then(|rows| rows.checked_add(1))
        .unwrap_or(0);
    if root_count > max_index {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            "root offset range exceeds configured limit",
        ));
    }
    if (*raw).null_count < -1 || (*raw).null_count > (*raw).length {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root array has an invalid null count",
        ));
    }
    let root_buffers = if (*raw).n_buffers == 1 {
        let pointer = *(*raw).buffers;
        let validity_bytes = root_count.checked_add(7).map(|bits| bits / 8).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                "root validity byte count overflows",
            )
        })?;
        if (*raw).null_count > 0 && pointer.is_null() {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                "root array has nulls but no validity buffer",
            ));
        }
        vec![if pointer.is_null() {
            ArrowBufferSpec::absent()
        } else {
            ArrowBufferSpec::present(validity_bytes)
        }]
    } else {
        Vec::new()
    };
    if (*raw).n_children < 0 || (*raw).n_children as usize != schema.fields.len() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root child count does not match the checked schema",
        ));
    }
    if (*raw).n_children > 0 && (*raw).children.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            "root declares children without a child pointer",
        ));
    }
    let mut children = Vec::with_capacity(schema.fields.len());
    for (index, field) in schema.fields.iter().enumerate() {
        let child = *(*raw).children.add(index);
        let child = NonNull::new(child).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("root child {index} is null"),
            )
        })?;
        children.push(read_array(child.as_ptr(), field, limits)?);
    }
    Ok(ArrowArraySpec {
        length: (*raw).length,
        null_count: (*raw).null_count,
        offset: (*raw).offset,
        buffers: root_buffers,
        children,
        dictionary: None,
        offsets: None,
    })
}
unsafe fn read_array(
    raw: *mut ArrowArray,
    field: &ArrowFieldSpec,
    limits: &ArrowLimits,
) -> Result<ArrowArraySpec, ArrowError> {
    let physical = field.physical_type()?;
    let expected_buffers: usize =
        if matches!(physical, ArrowPhysicalType::Utf8 | ArrowPhysicalType::Binary) {
            3
        } else {
            2
        };
    if (*raw).n_buffers != expected_buffers as i64 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!(
                "field `{}` has {} buffers; expected {expected_buffers}",
                field.name, (*raw).n_buffers
            ),
        ));
    }
    if (*raw).n_children != 0 {
        return Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.array",
            format!("field `{}` has unsupported child arrays", field.name),
        ));
    }
    if (*raw).buffers.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has no buffer pointer", field.name),
        ));
    }
    if (*raw).length < 0 || (*raw).offset < 0 {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has a negative length or offset", field.name),
        ));
    }
    if (*raw).null_count < -1 || (*raw).null_count > (*raw).length {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has an invalid null count", field.name),
        ));
    }
    if (*raw).length > limits.max_rows {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            format!("field `{}` exceeds row limit", field.name),
        ));
    }
    let count = usize::try_from((*raw).offset)
        .ok()
        .and_then(|offset| usize::try_from((*raw).length).ok()?.checked_add(offset))
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` index range overflows", field.name),
            )
        })?;
    let max_index = usize::try_from(limits.max_rows)
        .ok()
        .and_then(|rows| rows.checked_add(1))
        .unwrap_or(0);
    if count > max_index {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            format!("field `{}` offset range exceeds configured limit", field.name),
        ));
    }
    let validity_bytes = count.checked_add(7).map(|bits| bits / 8).ok_or_else(|| {
        ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.array",
            format!("field `{}` validity byte count overflows", field.name),
        )
    })?;
    let mut buffers = Vec::with_capacity(expected_buffers);
    for index in 0..expected_buffers {
        let pointer = *(*raw).buffers.add(index);
        buffers.push(if pointer.is_null() {
            ArrowBufferSpec::absent()
        } else if index == 0 {
            ArrowBufferSpec::present(validity_bytes)
        } else {
            ArrowBufferSpec::present(0)
        });
    }
    if (*raw).null_count > 0 && !buffers[0].present {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has nulls but no validity buffer", field.name),
        ));
    }
    let mut offsets = None;
    if matches!(physical, ArrowPhysicalType::Utf8 | ArrowPhysicalType::Binary) {
        let offset_pointer = *(*raw).buffers.add(1) as *const i32;
        if offset_pointer.is_null() {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` has no offsets buffer", field.name),
            ));
        }
        let offset_count = count.checked_add(1).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("field `{}` offset count overflows", field.name),
            )
        })?;
        let mut values = Vec::with_capacity(offset_count);
        for index in 0..offset_count {
            values.push(i64::from(*offset_pointer.add(index)));
        }
        buffers[1].bytes = offset_count.checked_mul(4).ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("field `{}` offsets byte count overflows", field.name),
            )
        })?;
        let data_pointer = *(*raw).buffers.add(2);
        let final_offset = values.last().copied().unwrap_or(0);
        if data_pointer.is_null() && final_offset > 0 {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` has no data buffer", field.name),
            ));
        }
        buffers[2].bytes = usize::try_from(final_offset).map_err(|_| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` final offset is negative or too large", field.name),
            )
        })?;
        offsets = Some(values);
    } else {
        let value_pointer = *(*raw).buffers.add(1);
        if value_pointer.is_null() && count > 0 {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("field `{}` has values but no values buffer", field.name),
            ));
        }
        let value_bytes = match physical {
            ArrowPhysicalType::Boolean => count.checked_add(7).map(|bits| bits / 8),
            other => other.width().and_then(|width| count.checked_mul(width)),
        }
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.array",
                format!("field `{}` value byte count overflows", field.name),
            )
        })?;
        buffers[1].bytes = value_bytes;
    }
    let dictionary = if let Some(dictionary_field) = field.dictionary.as_deref() {
        let raw_dictionary = (*raw).dictionary;
        if raw_dictionary.is_null() {
            return Err(ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.array",
                format!("dictionary field `{}` has no dictionary array", field.name),
            ));
        }
        Some(Box::new(read_array(raw_dictionary, dictionary_field, limits)?))
    } else if (*raw).dictionary.is_null() {
        None
    } else {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.array",
            format!("field `{}` has an undeclared dictionary", field.name),
        ));
    };
    Ok(ArrowArraySpec {
        length: (*raw).length,
        null_count: (*raw).null_count,
        offset: (*raw).offset,
        buffers,
        children: Vec::new(),
        dictionary,
        offsets,
    })
}

unsafe fn copy_owned_cells(
    raw_root: *mut ArrowArray,
    schema: &ArrowSchemaSpec,
    root: &ArrowArraySpec,
    limits: &ArrowLimits,
) -> Result<ArrowOwnedStorage, ArrowError> {
    if (*raw_root).n_children < 0
        || (*raw_root).n_children as usize != schema.fields.len()
        || ((*raw_root).n_children > 0 && (*raw_root).children.is_null())
    {
        return Err(ArrowError::new(
            ArrowErrorCode::ProducerFailure,
            "arrow.copy",
            "producer child table changed during value copy",
        ));
    }
    let rows = usize::try_from(root.length).map_err(|_| {
        ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.copy",
            "row count does not fit the host index type",
        )
    })?;
    let cells = rows.checked_mul(schema.fields.len()).ok_or_else(|| {
        ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.copy",
            "copied cell count overflows",
        )
    })?;
    let row_storage = rows
        .checked_mul(std::mem::size_of::<Vec<ArrowOwnedCell>>())
        .and_then(|bytes| {
            cells
                .checked_mul(std::mem::size_of::<ArrowOwnedCell>())
                .and_then(|cell_bytes| bytes.checked_add(cell_bytes))
        })
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::Limit,
                "arrow.copy",
                "copied cell storage size overflows",
            )
        })?;
    if row_storage > limits.max_total_bytes {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.copy",
            "copied cell storage exceeds total byte limit",
        ));
    }
    let mut rows_out = Vec::with_capacity(rows);
    let mut owned_value_bytes = 0usize;
    for row_index in 0..rows {
        let mut values = Vec::with_capacity(schema.fields.len());
        let root_valid = root_row_is_valid(raw_root, row_index)?;
        for (column, field) in schema.fields.iter().enumerate() {
            if !root_valid {
                values.push(ArrowOwnedCell::Null);
                continue;
            }
            let child = NonNull::new(*(*raw_root).children.add(column)).ok_or_else(|| {
                ArrowError::new(
                    ArrowErrorCode::ProducerFailure,
                    "arrow.copy",
                    format!("producer child {column} disappeared during value copy"),
                )
            })?;
            let index = usize::try_from((*child.as_ptr()).offset)
                .ok()
                .and_then(|offset| offset.checked_add(row_index))
                .ok_or_else(|| {
                    ArrowError::new(
                        ArrowErrorCode::MalformedLayout,
                        "arrow.copy",
                        "column row offset overflows",
                    )
                })?;
            let cell = read_arrow_cell(child.as_ptr(), field, index, 0, limits)?;
            let owned = match cell {
                ArrowCell::Null => ArrowOwnedCell::Null,
                ArrowCell::Bool(value) => ArrowOwnedCell::Bool(value),
                ArrowCell::Signed(value) => ArrowOwnedCell::Signed(value),
                ArrowCell::Unsigned(value) => ArrowOwnedCell::Unsigned(value),
                ArrowCell::Float(value) => ArrowOwnedCell::Float(value),
                ArrowCell::Utf8(value) => {
                    owned_value_bytes = owned_value_bytes
                        .checked_add(value.len())
                        .ok_or_else(|| {
                            ArrowError::new(
                                ArrowErrorCode::Limit,
                                "arrow.copy",
                                "copied text byte total overflows",
                            )
                        })?;
                    ArrowOwnedCell::Utf8(value.to_owned())
                }
                ArrowCell::Binary(value) => {
                    owned_value_bytes = owned_value_bytes
                        .checked_add(value.len())
                        .ok_or_else(|| {
                            ArrowError::new(
                                ArrowErrorCode::Limit,
                                "arrow.copy",
                                "copied binary byte total overflows",
                            )
                        })?;
                    ArrowOwnedCell::Binary(value.to_vec())
                }
            };
            values.push(owned);
        }
        rows_out.push(values);
    }
    if row_storage
        .checked_add(owned_value_bytes)
        .is_none_or(|bytes| bytes > limits.max_total_bytes)
    {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.copy",
            "copied values exceed total byte limit",
        ));
    }
    Ok(ArrowOwnedStorage { rows: rows_out })
}

unsafe fn root_row_is_valid(raw: *mut ArrowArray, row: usize) -> Result<bool, ArrowError> {
    if (*raw).n_buffers == 0 {
        return Ok(true);
    }
    if (*raw).n_buffers != 1 || (*raw).buffers.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.copy",
            "root validity buffer table is invalid",
        ));
    }
    let validity = *(*raw).buffers;
    if validity.is_null() {
        return Ok(true);
    }
    let index = usize::try_from((*raw).offset)
        .ok()
        .and_then(|offset| offset.checked_add(row))
        .ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::MalformedLayout,
                "arrow.copy",
                "root validity index overflows",
            )
        })?;
    let byte = *((validity as *const u8).add(index / 8));
    Ok(byte & (1u8 << (index % 8)) != 0)
}

unsafe fn read_c_string(pointer: *const c_char, what: &str) -> Result<String, ArrowError> {
    if pointer.is_null() {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.schema",
            format!("{what} pointer is null"),
        ));
    }
    // SAFETY: `import_c` requires a valid NUL-terminated C record.  CStr keeps
    // this operation inside the vetted ABI boundary; safe callers never invoke
    // it directly.
    let bytes = CStr::from_ptr(pointer).to_bytes();
    if bytes.len() > ARROW_MAX_C_STRING_BYTES {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.schema",
            format!("{what} exceeds {} bytes", ARROW_MAX_C_STRING_BYTES),
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.schema",
            format!("{what} is not UTF-8"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(fields: Vec<ArrowFieldSpec>) -> ArrowSchemaSpec {
        ArrowSchemaSpec::new(fields)
    }

    #[test]
    fn explicit_copy_materializes_declared_values() {
        let format_root = b"+s\0";
        let format_id = b"l\0";
        let name_root = b"root\0";
        let name_id = b"id\0";
        unsafe extern "C" fn release_schema(pointer: *mut ArrowSchema) {
            (*pointer).release = None;
        }
        unsafe extern "C" fn release_array(pointer: *mut ArrowArray) {
            (*pointer).release = None;
        }
        let mut child_schema = ArrowSchema {
            format: format_id.as_ptr().cast(),
            name: name_id.as_ptr().cast(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: 0,
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: None,
            private_data: std::ptr::null_mut(),
        };
        let mut schema_children = vec![&mut child_schema as *mut ArrowSchema];
        let mut root_schema = ArrowSchema {
            format: format_root.as_ptr().cast(),
            name: name_root.as_ptr().cast(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: 1,
            children: schema_children.as_mut_ptr(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_schema),
            private_data: std::ptr::null_mut(),
        };
        let mut values = [3_i64, 5_i64];
        let buffers = [std::ptr::null(), values.as_ptr().cast()];
        let mut child_array = ArrowArray {
            length: 2,
            null_count: 0,
            offset: 0,
            n_buffers: 2,
            n_children: 0,
            buffers: buffers.as_ptr(),
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: None,
            private_data: std::ptr::null_mut(),
        };
        let mut array_children = vec![&mut child_array as *mut ArrowArray];
        let mut root_array = ArrowArray {
            length: 2,
            null_count: 0,
            offset: 0,
            n_buffers: 0,
            n_children: 1,
            buffers: std::ptr::null(),
            children: array_children.as_mut_ptr(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_array),
            private_data: std::ptr::null_mut(),
        };
        let imported = unsafe {
            ArrowImported::<()>::import_c(
                &mut root_schema,
                &mut root_array,
                None,
                &ArrowLimits::safe(),
                ArrowImportMode::Copy,
            )
        }
        .expect("copy import materializes scalar values");
        values.copy_from_slice(&[101_i64, 103_i64]);
        assert_eq!(imported.cell(0, 0), Ok(ArrowCell::Signed(3)));
        assert_eq!(imported.cell(1, 0), Ok(ArrowCell::Signed(5)));
        let _ = root_schema;
    }

    #[test]
    fn shared_import_reports_retained_bytes_without_copy() {
        let schema = schema(vec![ArrowFieldSpec::new("id", "l", false)]);
        let array = ArrowArraySpec::primitive(2, 0, 0, 16);
        let imported: ColumnBatch<()> = ArrowImported::<()>::from_spec(
            schema,
            ArrowArraySpec {
                children: vec![array],
                length: 2,
                null_count: 0,
                offset: 0,
                buffers: vec![],
                dictionary: None,
                offsets: None,
            },
            None,
            &ArrowLimits::safe(),
            ArrowImportMode::Shared,
        )
        .expect("valid shared import");
        assert_eq!(imported.report.copy_label(), "shared");
        assert_eq!(imported.report.buffers_bytes, 16);
        assert_eq!(imported.row(0), Ok(()));
        assert_eq!(imported.row(1), Ok(()));
        assert_eq!(imported.row(2).expect_err("row bound must be checked").code, "E4302");
    }

    #[test]
    fn malformed_offsets_are_declared_results() {
        let schema = schema(vec![ArrowFieldSpec::new("name", "u", true)]);
        let child = ArrowArraySpec::utf8(2, 0, 0, vec![0, 4, 3], 4);
        let array = ArrowArraySpec {
            length: 2,
            null_count: 0,
            offset: 0,
            buffers: vec![],
            children: vec![child],
            dictionary: None,
            offsets: None,
        };
        let error = ArrowImported::<()>::from_spec(
            schema,
            array,
            None,
            &ArrowLimits::safe(),
            ArrowImportMode::NoCopy,
        )
        .expect_err("offset reversal must be rejected");
        assert_eq!(error.code, "E4302");
    }

    #[test]
    fn no_copy_refuses_schema_conversion_but_copy_discloses_bytes() {
        let actual = schema(vec![ArrowFieldSpec::new("id", "i", false)]);
        let wanted = [ArrowFieldSpec::new("id", "l", false)];
        let array = ArrowArraySpec {
            length: 1,
            null_count: 0,
            offset: 0,
            buffers: vec![],
            children: vec![ArrowArraySpec::primitive(1, 0, 0, 4)],
            dictionary: None,
            offsets: None,
        };
        let refused = ArrowImported::<()>::from_spec(
            actual.clone(),
            array.clone(),
            Some(&wanted),
            &ArrowLimits::safe(),
            ArrowImportMode::NoCopy,
        )
        .expect_err("no-copy conversion must refuse");
        assert_eq!(refused.code, "E4305");
        let copied = ArrowImported::<()>::from_spec(
            actual.clone(),
            array.clone(),
            None,
            &ArrowLimits::safe(),
            ArrowImportMode::Copy,
        )
        .expect("explicit copy remains available");
        assert_eq!(copied.report.copy_label(), "copied");
        assert_eq!(copied.report.buffers_bytes, 4);
        let converted = ArrowImported::<()>::from_spec(
            actual,
            array,
            Some(&wanted),
            &ArrowLimits::safe(),
            ArrowImportMode::Copy,
        )
        .expect("explicit copy may convert a compatible physical field");
        assert_eq!(converted.report.copy_label(), "copied");
    }

    #[test]
    fn dictionary_layout_and_limits_are_checked() {
        let mut field = ArrowFieldSpec::new("kind", "i", true);
        field.dictionary = Some(Box::new(ArrowFieldSpec::new("value", "u", false)));
        let schema = schema(vec![field]);
        let mut child = ArrowArraySpec::primitive(2, 0, 0, 8);
        child.dictionary = Some(Box::new(ArrowArraySpec::utf8(2, 0, 0, vec![0, 1, 2], 2)));
        let array = ArrowArraySpec {
            length: 2,
            null_count: 0,
            offset: 0,
            buffers: vec![],
            children: vec![child],
            dictionary: None,
            offsets: None,
        };
        let mut limits = ArrowLimits::safe();
        limits.max_dictionary_values = 1;
        let error = ArrowImported::<()>::from_spec(schema, array, None, &limits, ArrowImportMode::Shared)
            .expect_err("dictionary limit must be enforced");
        assert_eq!(error.code, "E4304");
    }

    #[test]
    fn c_owner_releases_base_once_and_never_releases_children() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static ARRAY_RELEASES: AtomicUsize = AtomicUsize::new(0);
        static SCHEMA_RELEASES: AtomicUsize = AtomicUsize::new(0);
        static CHILD_ARRAY_RELEASES: AtomicUsize = AtomicUsize::new(0);
        static CHILD_SCHEMA_RELEASES: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn release_array(pointer: *mut ArrowArray) {
            ARRAY_RELEASES.fetch_add(1, Ordering::SeqCst);
            (*pointer).release = None;
        }
        unsafe extern "C" fn release_schema(pointer: *mut ArrowSchema) {
            SCHEMA_RELEASES.fetch_add(1, Ordering::SeqCst);
            (*pointer).release = None;
        }
        unsafe extern "C" fn release_child_array(pointer: *mut ArrowArray) {
            CHILD_ARRAY_RELEASES.fetch_add(1, Ordering::SeqCst);
            (*pointer).release = None;
        }
        unsafe extern "C" fn release_child_schema(pointer: *mut ArrowSchema) {
            CHILD_SCHEMA_RELEASES.fetch_add(1, Ordering::SeqCst);
            (*pointer).release = None;
        }

        let format_root = b"\0";
        let format_id = b"l\0";
        let name_root = b"root\0";
        let name_id = b"id\0";
        let mut child_schema = ArrowSchema {
            format: format_id.as_ptr().cast(),
            name: name_id.as_ptr().cast(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: 0,
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_child_schema),
            private_data: std::ptr::null_mut(),
        };
        let mut schema_children = vec![&mut child_schema as *mut ArrowSchema];
        let mut root_schema = ArrowSchema {
            format: format_root.as_ptr().cast(),
            name: name_root.as_ptr().cast(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: 1,
            children: schema_children.as_mut_ptr(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_schema),
            private_data: std::ptr::null_mut(),
        };
        let values = [11_i64, 22_i64];
        let buffers = [std::ptr::null(), values.as_ptr().cast()];
        let mut child_array = ArrowArray {
            length: 2,
            null_count: 0,
            offset: 0,
            n_buffers: 2,
            n_children: 0,
            buffers: buffers.as_ptr(),
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_child_array),
            private_data: std::ptr::null_mut(),
        };
        let mut array_children = vec![&mut child_array as *mut ArrowArray];
        let mut root_array = ArrowArray {
            length: 2,
            null_count: 0,
            offset: 0,
            n_buffers: 0,
            n_children: 1,
            buffers: std::ptr::null(),
            children: array_children.as_mut_ptr(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_array),
            private_data: std::ptr::null_mut(),
        };
        let imported = unsafe {
            ArrowImported::<()>::import_c(
                &mut root_schema,
                &mut root_array,
                None,
                &ArrowLimits::safe(),
                ArrowImportMode::Shared,
            )
        }
        .expect("valid C export");
        let alias = imported.clone();
        let duplicate = unsafe {
            ArrowImported::<()>::import_c(
                &mut root_schema,
                &mut root_array,
                None,
                &ArrowLimits::safe(),
                ArrowImportMode::Shared,
            )
        }
        .expect_err("an aliased base cannot be imported twice");
        assert_eq!(duplicate.code, "E4303");
        drop(imported);
        assert_eq!(ARRAY_RELEASES.load(Ordering::SeqCst), 0);
        assert_eq!(SCHEMA_RELEASES.load(Ordering::SeqCst), 0);
        drop(alias);
        assert_eq!(ARRAY_RELEASES.load(Ordering::SeqCst), 1);
        assert_eq!(SCHEMA_RELEASES.load(Ordering::SeqCst), 1);
        assert_eq!(CHILD_ARRAY_RELEASES.load(Ordering::SeqCst), 0);
        assert_eq!(CHILD_SCHEMA_RELEASES.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn malformed_c_layout_releases_claimed_base_before_returning_error() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static ARRAY_RELEASES: AtomicUsize = AtomicUsize::new(0);
        static SCHEMA_RELEASES: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn release_array(pointer: *mut ArrowArray) {
            ARRAY_RELEASES.fetch_add(1, Ordering::SeqCst);
            (*pointer).release = None;
        }
        unsafe extern "C" fn release_schema(pointer: *mut ArrowSchema) {
            SCHEMA_RELEASES.fetch_add(1, Ordering::SeqCst);
            (*pointer).release = None;
        }
        let format_root = b"\0";
        let format_id = b"l\0";
        let name_root = b"root\0";
        let name_id = b"id\0";
        let mut child_schema = ArrowSchema {
            format: format_id.as_ptr().cast(),
            name: name_id.as_ptr().cast(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: 0,
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: None,
            private_data: std::ptr::null_mut(),
        };
        let mut schema_children = vec![&mut child_schema as *mut ArrowSchema];
        let mut root_schema = ArrowSchema {
            format: format_root.as_ptr().cast(),
            name: name_root.as_ptr().cast(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: 1,
            children: schema_children.as_mut_ptr(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_schema),
            private_data: std::ptr::null_mut(),
        };
        let buffers = [std::ptr::null(), std::ptr::null()];
        let mut child_array = ArrowArray {
            length: 1,
            null_count: 0,
            offset: 0,
            n_buffers: 2,
            n_children: 0,
            buffers: buffers.as_ptr(),
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: None,
            private_data: std::ptr::null_mut(),
        };
        let mut array_children = vec![&mut child_array as *mut ArrowArray];
        let mut root_array = ArrowArray {
            length: 1,
            null_count: 0,
            offset: 0,
            n_buffers: 0,
            n_children: 1,
            buffers: std::ptr::null(),
            children: array_children.as_mut_ptr(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_array),
            private_data: std::ptr::null_mut(),
        };
        let error = unsafe {
            ArrowImported::<()>::import_c(
                &mut root_schema,
                &mut root_array,
                None,
                &ArrowLimits::safe(),
                ArrowImportMode::NoCopy,
            )
        }
        .expect_err("null values pointer must be rejected");
        assert_eq!(error.code, "E4302");
        assert_eq!(ARRAY_RELEASES.load(Ordering::SeqCst), 1);
        assert_eq!(SCHEMA_RELEASES.load(Ordering::SeqCst), 1);
    }
}
