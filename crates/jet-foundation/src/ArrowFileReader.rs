//! Registered byte-source boundary for official Arrow/Parquet providers.
//!
//! Foundation owns the provider contract, input and layout limits, error
//! mapping, and `ArrowImported` ownership.  The provider itself is deliberately
//! outside this crate: it receives a bounded byte slice and returns one Arrow C
//! schema/array pair whose release callbacks remain valid until Foundation has
//! finished validating and claiming the pair.

use crate::ArrowData::{
    ArrowArray, ArrowCell, ArrowError, ArrowErrorCode, ArrowFieldSpec, ArrowImported,
    ArrowImportMode, ArrowLimits, ArrowSchema, ColumnBatch,
};
use crate::DataTree::DataTree;
use std::sync::{Arc, LazyLock, Mutex};

/// Parquet byte-source format code used by the registered provider.
pub const FORMAT_PARQUET: u8 = 0;
/// Arrow C-data byte-source format code used by the registered provider.
///
/// This is not Arrow IPC.  An IPC reader must register a distinct provider
/// once its format has a ratified boundary.
pub const FORMAT_ARROW_C: u8 = 1;

/// Provider result code for a successful export.
pub const PROVIDER_OK: i32 = 0;
/// Provider result code for malformed file metadata.
pub const PROVIDER_METADATA: i32 = 1;
/// Provider result code for invalid offsets or buffers.
pub const PROVIDER_OFFSETS: i32 = 2;
/// Provider result code for unsupported or failed compression.
pub const PROVIDER_COMPRESSION: i32 = 3;
/// Provider result code for unsupported nesting.
pub const PROVIDER_NESTING: i32 = 4;
/// Provider result code for a configured limit.
pub const PROVIDER_LIMIT: i32 = 5;
/// Provider result code for an unsupported logical or physical type.
pub const PROVIDER_TYPE: i32 = 6;
/// Provider result code for an internal provider panic or failure.
pub const PROVIDER_FAILURE: i32 = 7;
/// Provider result code for a malformed input byte slice.
pub const PROVIDER_INPUT: i32 = 8;
/// Provider result code for an unsupported format code.
pub const PROVIDER_FORMAT: i32 = 9;

/// Stable limits passed over the hidden bridge's C ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderLimits {
    pub max_rows: i64,
    pub max_columns: i64,
    pub max_children: i64,
    pub max_dictionary_values: i64,
    pub max_buffer_bytes: usize,
    pub max_total_bytes: usize,
}

impl From<&ArrowLimits> for ProviderLimits {
    fn from(limits: &ArrowLimits) -> Self {
        Self {
            max_rows: limits.max_rows,
            max_columns: limits.max_columns,
            max_children: limits.max_children,
            max_dictionary_values: limits.max_dictionary_values,
            max_buffer_bytes: limits.max_buffer_bytes,
            max_total_bytes: limits.max_total_bytes,
        }
    }
}

/// Hidden-bridge provider ABI.
///
/// On success, the provider must write both output pointers.  Each pointer
/// must identify a live Arrow C record with a release callback.  Foundation
/// invokes no callback itself until `ArrowImported` owns the pair; provider
/// failure must leave both output pointers null and release any temporary
/// allocation before returning.
pub type Provider = unsafe extern "C" fn(
    format: u8,
    input: *const u8,
    input_len: usize,
    limits: *const ProviderLimits,
    out_schema: *mut *mut ArrowSchema,
    out_array: *mut *mut ArrowArray,
) -> i32;

struct ProviderSlot {
    provider: Provider,
    /// Dynamic bridge registrations hold one lease while the bridge handle is
    /// live.  Imported batches retain the owner token separately in ArrowData.
    leases: usize,
    persistent: bool,
    owner: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

static PROVIDER: LazyLock<Mutex<Option<ProviderSlot>>> = LazyLock::new(|| Mutex::new(None));

/// A registration lease for a provider-backed dynamic bridge.
///
/// Dropping the last lease removes a non-persistent provider from the
/// Foundation slot.  Any shared Arrow batch may still retain its owner token,
/// so the bridge library remains live until that batch is dropped.
pub struct ProviderLease {
    provider: Provider,
}

impl Drop for ProviderLease {
    fn drop(&mut self) {
        let mut slot = PROVIDER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(current) = slot.as_mut() else {
            return;
        };
        if current.provider as usize != self.provider as usize {
            return;
        }
        current.leases = current.leases.saturating_sub(1);
        if current.leases == 0 && !current.persistent {
            *slot = None;
        }
    }
}

/// Register the one process-local official reader provider.
///
/// Re-registering the exact provider is idempotent so generated AOT setup and
/// interpreter setup can both install the same bridge.  Replacing a different
/// provider is refused while a lease or persistent registration remains.
pub fn register(provider: Provider) -> Result<(), ArrowError> {
    let mut slot = PROVIDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match slot.as_mut() {
        Some(existing) if existing.provider as usize != provider as usize => {
            Err(ArrowError::new(
                ArrowErrorCode::Ownership,
                "arrow.provider.register",
                "a different reader provider is already registered",
            ))
        }
        Some(existing) => {
            if existing.leases == 0 {
                existing.persistent = true;
            }
            Ok(())
        }
        None => {
            *slot = Some(ProviderSlot {
                provider,
                leases: 0,
                persistent: true,
                owner: None,
            });
            Ok(())
        }
    }
}

/// Register a provider together with its dynamic-library owner and acquire a
/// lease that must live through every call using the provider.
pub fn register_owned(
    provider: Provider,
    owner: Arc<dyn std::any::Any + Send + Sync>,
) -> Result<ProviderLease, ArrowError> {
    let mut slot = PROVIDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match slot.as_mut() {
        Some(existing) if existing.provider as usize != provider as usize => {
            Err(ArrowError::new(
                ArrowErrorCode::Ownership,
                "arrow.provider.register",
                "a different reader provider is already registered",
            ))
        }
        Some(existing) => {
            existing.leases = existing.leases.saturating_add(1);
            if existing.owner.is_none() {
                existing.owner = Some(owner);
            }
            Ok(ProviderLease { provider })
        }
        None => {
            *slot = Some(ProviderSlot {
                provider,
                leases: 1,
                persistent: false,
                owner: Some(owner),
            });
            Ok(ProviderLease { provider })
        }
    }
}

/// Remove a persistent registration after all imported batches are dropped.
///
/// A leased provider remains in the slot until its last lease is released;
/// this prevents replacing the callback while a dynamic bridge may still be
/// called by an active reader or an escaped Arrow owner.
pub fn unregister() {
    let mut slot = PROVIDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(current) = slot.as_mut() {
        current.persistent = false;
        if current.leases == 0 {
            *slot = None;
        }
    }
}

/// Read one supported byte source through the registered provider and return a
/// validated, immutable Arrow column batch.
pub fn read<Row>(
    format: u8,
    input: &[u8],
    expected: Option<&[ArrowFieldSpec]>,
    limits: &ArrowLimits,
    mode: ArrowImportMode,
) -> Result<ColumnBatch<Row>, ArrowError> {
    if format != FORMAT_PARQUET && format != FORMAT_ARROW_C {
        return Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.provider.read",
            format!("unsupported format code {format}"),
        ));
    }
    if input.len() > limits.max_total_bytes {
        return Err(ArrowError::new(
            ArrowErrorCode::Limit,
            "arrow.provider.read",
            format!(
                "input bytes {} exceed limit {}",
                input.len(), limits.max_total_bytes
            ),
        ));
    }
    let (provider, provider_owner) = {
        let slot = PROVIDER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = slot.as_ref().ok_or_else(|| {
            ArrowError::new(
                ArrowErrorCode::ProducerFailure,
                "arrow.provider.read",
                "no official reader provider is registered",
            )
        })?;
        (current.provider, current.owner.clone())
    };
    let provider_limits = ProviderLimits::from(limits);
    let mut schema = std::ptr::null_mut();
    let mut array = std::ptr::null_mut();
    // SAFETY: `provider` is installed through `register`, and this call obeys
    // its ABI contract.  The input and limits point to immutable values owned
    // by this stack frame for the duration of the callback.  On success the
    // provider transfers both live records to `import_c`, which validates and
    // claims their base pointers before retaining release state.
    let status = unsafe {
        provider(
            format,
            input.as_ptr(),
            input.len(),
            &provider_limits,
            &mut schema,
            &mut array,
        )
    };
    if status != PROVIDER_OK {
        // SAFETY: a provider that fails may have allocated one side before
        // discovering the error.  Reclaim each unclaimed C record here so a
        // malformed provider cannot leak or expose it to the next call.
        unsafe { release_provider_outputs(schema, array) };
        return Err(provider_error(status));
    }
    if schema.is_null() || array.is_null() {
        // SAFETY: success still requires both records.  Any partial transfer
        // remains provider-owned because `import_c` was never called.
        unsafe { release_provider_outputs(schema, array) };
        return Err(ArrowError::new(
            ArrowErrorCode::ProducerFailure,
            "arrow.provider.read",
            "provider returned success without schema and array records",
        ));
    }
    // SAFETY: the provider's successful transfer contract is checked again by
    // import_c, including release callbacks, pointer uniqueness, schema,
    // offsets, lengths, dictionaries, nullability, and configured limits.
    unsafe {
        ArrowImported::import_c_with_provider_owner(
            schema,
            array,
            expected,
            limits,
            mode,
            provider_owner,
        )
    }
}

unsafe fn release_provider_outputs(
    schema: *mut ArrowSchema,
    array: *mut ArrowArray,
) {
    if !schema.is_null() {
        if let Some(release) = (*schema).release {
            release(schema);
        }
    }
    if !array.is_null()
        && (schema as *mut std::ffi::c_void) != (array as *mut std::ffi::c_void)
    {
        if let Some(release) = (*array).release {
            release(array);
        }
    }
}
/// Read one provider-owned column batch into the canonical ordered data tree.
///
/// This is the only tree projection at the Arrow boundary.  It keeps the
/// validated batch alive while borrowed cell views are copied into owned
/// `DataTree` values, so callers never retain a foreign-lifetime view.
pub fn read_tree(
    format: u8,
    input: &[u8],
    limits: &ArrowLimits,
) -> Result<DataTree, ArrowError> {
    let batch = read::<()>(format, input, None, limits, ArrowImportMode::Shared)?;
    let mut rows = Vec::with_capacity(batch.report.rows);
    for row in 0..batch.report.rows {
        let fields = batch
            .schema
            .fields
            .iter()
            .enumerate()
            .map(|(column, field)| {
                let value = tree_value(field, batch.cell(row, column)?)?;
                Ok((field.name.clone(), value))
            })
            .collect::<Result<Vec<_>, ArrowError>>()?;
        rows.push(DataTree::Object(fields));
    }
    Ok(DataTree::Array(rows))
}

fn tree_value(field: &ArrowFieldSpec, cell: ArrowCell<'_>) -> Result<DataTree, ArrowError> {
    if let Some(value) = temporal_tree_value(field, cell)? {
        return Ok(value);
    }
    Ok(match cell {
        ArrowCell::Null => DataTree::Null,
        ArrowCell::Bool(value) => DataTree::Bool(value),
        ArrowCell::Signed(value) => DataTree::Int(value),
        ArrowCell::Unsigned(value) => i64::try_from(value)
            .map(DataTree::Int)
            .unwrap_or_else(|_| DataTree::Number(value.to_string())),
        ArrowCell::Float(value) => DataTree::Float(value),
        ArrowCell::Utf8(value) => DataTree::Text(value.to_owned()),
        ArrowCell::Binary(value) => DataTree::Bytes(value.to_vec()),
    })
}

/// Preserve the logical temporal annotation emitted by the official provider.
/// Arrow's C data format carries these values as signed integers; projecting
/// them as `DataTree::Int` silently changes Date/Time/Duration/DateTime fields
/// into unrelated user integers.
fn temporal_tree_value(
    field: &ArrowFieldSpec,
    cell: ArrowCell<'_>,
) -> Result<Option<DataTree>, ArrowError> {
    let Some(metadata) = field.metadata.as_deref() else {
        return Ok(None);
    };
    if !metadata.starts_with("jet.logical=") {
        return Ok(None);
    }
    let Some(value) = (match cell {
        ArrowCell::Null => return Ok(None),
        ArrowCell::Signed(value) => Some(value),
        _ => None,
    }) else {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.tree.temporal",
            format!(
                "logical temporal field `{}` did not export a signed Arrow cell",
                field.name
            ),
        ));
    };

    let text = if metadata == "jet.logical=date32" {
        format_date_from_days(value.into())?
    } else if metadata == "jet.logical=date64" {
        format_date_from_days(i128::from(value).div_euclid(86_400_000))?
    } else if let Some(unit) = metadata.strip_prefix("jet.logical=time32:") {
        format_time_from_nanos(scale_temporal(value, unit)?)?
    } else if let Some(unit) = metadata.strip_prefix("jet.logical=time64:") {
        format_time_from_nanos(scale_temporal(value, unit)?)?
    } else if let Some(unit_and_zone) = metadata.strip_prefix("jet.logical=timestamp:") {
        let unit = unit_and_zone.split_once(':').map_or(unit_and_zone, |(u, _)| u);
        format_datetime_from_nanos(scale_temporal(value, unit)?)?
    } else if let Some(unit) = metadata.strip_prefix("jet.logical=duration:") {
        let nanos = scale_temporal(value, unit)?;
        let nanos = i64::try_from(nanos).map_err(|_| {
            ArrowError::new(
                ArrowErrorCode::UnsupportedSchema,
                "arrow.tree.temporal",
                format!("duration value {value} is outside Jet's nanosecond range"),
            )
        })?;
        return Ok(Some(DataTree::Int(nanos)));
    } else if metadata.starts_with("jet.logical=") {
        return Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.tree.temporal",
            format!("unsupported logical Arrow annotation `{metadata}`"),
        ));
    } else {
        return Ok(None);
    };
    Ok(Some(DataTree::TypedText(text)))
}

fn scale_temporal(value: i64, unit: &str) -> Result<i128, ArrowError> {
    let factor = match unit {
        "Second" => 1_000_000_000i128,
        "Millisecond" => 1_000_000i128,
        "Microsecond" => 1_000i128,
        "Nanosecond" => 1i128,
        _ => {
            return Err(ArrowError::new(
                ArrowErrorCode::UnsupportedSchema,
                "arrow.tree.temporal",
                format!("unsupported logical temporal unit `{unit}`"),
            ))
        }
    };
    Ok(i128::from(value) * factor)
}

fn format_date_from_days(days: i128) -> Result<String, ArrowError> {
    // Civil date conversion from days since 1970-01-01.  The arithmetic stays
    // in i128 so malformed extreme Arrow values cannot wrap before the range
    // check against Jet's textual temporal codec.
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * mp + 2).div_euclid(5) + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    if !(0..=i128::from(i64::MAX)).contains(&year) {
        return Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.tree.temporal",
            "logical date is outside Jet's supported textual year range",
        ));
    }
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn format_time_from_nanos(nanos: i128) -> Result<String, ArrowError> {
    if !(0..86_400_000_000_000i128).contains(&nanos) {
        return Err(ArrowError::new(
            ArrowErrorCode::MalformedLayout,
            "arrow.tree.temporal",
            "logical time value is outside one day",
        ));
    }
    let seconds = nanos.div_euclid(1_000_000_000);
    let fraction = nanos.rem_euclid(1_000_000_000);
    let hour = seconds / 3_600;
    let minute = seconds.rem_euclid(3_600) / 60;
    let second = seconds.rem_euclid(60);
    if fraction == 0 {
        Ok(format!("{hour:02}:{minute:02}:{second:02}"))
    } else {
        let mut fraction = format!("{fraction:09}");
        while fraction.ends_with('0') {
            fraction.pop();
        }
        Ok(format!("{hour:02}:{minute:02}:{second:02}.{fraction}"))
    }
}

fn format_datetime_from_nanos(nanos: i128) -> Result<String, ArrowError> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let fraction = nanos.rem_euclid(1_000_000_000);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let date = format_date_from_days(days)?;
    let time = format_time_from_nanos(day_seconds * 1_000_000_000 + fraction)?;
    Ok(format!("{date}T{time}Z"))
}

/// Convert a textual data format accepted by the Prelude to this boundary's
/// explicit format code.  Arrow IPC aliases are intentionally not accepted.
pub fn format_code(format: &str) -> Result<u8, ArrowError> {
    match format {
        "parquet" => Ok(FORMAT_PARQUET),
        "arrow" => Ok(FORMAT_ARROW_C),
        _ => Err(ArrowError::new(
            ArrowErrorCode::UnsupportedSchema,
            "arrow.provider.format",
            format!("format `{format}` is not an Arrow C-data provider format"),
        )),
    }
}

fn provider_error(status: i32) -> ArrowError {
    let kind = match status {
        PROVIDER_METADATA | PROVIDER_OFFSETS | PROVIDER_INPUT => ArrowErrorCode::MalformedLayout,
        PROVIDER_COMPRESSION | PROVIDER_NESTING | PROVIDER_TYPE | PROVIDER_FORMAT => {
            ArrowErrorCode::UnsupportedSchema
        }
        PROVIDER_LIMIT => ArrowErrorCode::Limit,
        PROVIDER_FAILURE => ArrowErrorCode::ProducerFailure,
        _ => ArrowErrorCode::ProducerFailure,
    };
    ArrowError::new(
        kind,
        "arrow.provider.read",
        format!("provider returned status {status}"),
    )
}
