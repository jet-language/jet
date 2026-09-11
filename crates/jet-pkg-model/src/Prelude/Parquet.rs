//! Official Arrow Rust Parquet reader behind Jet's provider boundary.
//!
//! This source is embedded only in the hidden package bridge.  It never emits
//! JSON or another Jet wire format: a bounded Parquet read is retained as
//! Arrow `ArrayData`, exported as one Arrow C schema/array pair, and validated
//! and owned by `jet-foundation::ArrowFileReader`.

use arrow_array::{Array, RecordBatch};
use arrow_data::ArrayData;
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use arrow_select::concat::concat_batches;
use bytes::Bytes;
use parquet::arrow::arrow_reader::{ArrowReaderOptions, ParquetRecordBatchReaderBuilder};
use parquet::basic::Type as PhysicalType;
use parquet::column::reader::get_typed_column_reader;
use parquet::data_type::{Int96, Int96Type};
use parquet::file::reader::{FileReader, RowGroupReader, SerializedFileReader};
use parquet::schema::types::SchemaDescriptor;
use std::ffi::{c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use jet_foundation::ArrowData::{ArrowArray, ArrowSchema, ARROW_FLAG_NULLABLE};
use jet_foundation::ArrowFileReader::{
    register, ProviderLimits, FORMAT_ARROW_C, FORMAT_PARQUET, PROVIDER_COMPRESSION,
    PROVIDER_FAILURE, PROVIDER_FORMAT, PROVIDER_INPUT, PROVIDER_LIMIT, PROVIDER_METADATA,
    PROVIDER_NESTING, PROVIDER_OFFSETS, PROVIDER_OK, PROVIDER_TYPE,
};


/// Dependency/provenance contract for this provider.  The hidden bridge owns
/// the pinned closure; this source only consumes the Foundation C-data ABI.
pub const PARQUET_READER_PROVENANCE: &str =
    "parquet=59.3.0;arrow-array=59.3.0;arrow-data=59.3.0;arrow-schema=59.3.0;arrow-select=59.3.0;codecs=brotli,deflate,lz4,snappy,zstd;license=Apache-2.0;source=https://crates.io/crates/parquet/59.3.0";

const MAX_BATCH_ROWS: usize = 8192;

struct ExportedBatch {
    schema: Box<SchemaTree>,
    array: Box<ArrayTree>,
}

struct SchemaTree {
    raw: ArrowSchema,
    root_format: CString,
    root_name: CString,
    children: Vec<*mut ArrowSchema>,
    nodes: Vec<Box<SchemaNode>>,
}

struct SchemaNode {
    raw: ArrowSchema,
    format: CString,
    name: CString,
    metadata: Option<CString>,
    dictionary: Option<Box<SchemaNode>>,
}

struct ArrayTree {
    raw: ArrowArray,
    buffers: Vec<*const c_void>,
    children: Vec<*mut ArrowArray>,
    nodes: Vec<Box<ArrayNode>>,
}

struct ArrayNode {
    raw: ArrowArray,
    buffers: Vec<*const c_void>,
    dictionary: Option<Box<ArrayNode>>,
    data: ArrayData,
}

/// Install this bridge's official reader into the Foundation provider slot.
#[no_mangle]
pub extern "C" fn jet_data_reader_register() -> i32 {
    match register(jet_data_read) {
        Ok(()) => PROVIDER_OK,
        Err(_) => PROVIDER_FAILURE,
    }
}

/// Read Parquet bytes and transfer a validated candidate Arrow C export.
///
/// The output records are released by Foundation's `ArrowImported` owner.  A
/// nonzero result leaves both output pointers null and this function retains no
/// producer allocation.
#[no_mangle]
pub unsafe extern "C" fn jet_data_read(
    format: u8,
    input: *const u8,
    input_len: usize,
    limits: *const ProviderLimits,
    out_schema: *mut *mut ArrowSchema,
    out_array: *mut *mut ArrowArray,
) -> i32 {
    if out_schema.is_null() || out_array.is_null() || limits.is_null() {
        return PROVIDER_INPUT;
    }
    // SAFETY: the output pointers were checked non-null and are owned by the
    // caller for this ABI invocation; initialize them before any fallible work.
    *out_schema = std::ptr::null_mut();
    *out_array = std::ptr::null_mut();
    if input_len > 0 && input.is_null() {
        return PROVIDER_INPUT;
    }
    let limits = *limits;
    if !valid_limits(&limits) {
        return PROVIDER_LIMIT;
    }
    let bytes = if input_len == 0 {
        &[]
    } else {
        // SAFETY: the caller supplied a non-null immutable byte range for the
        // duration of this callback, as required by Provider's contract.
        std::slice::from_raw_parts(input, input_len)
    };
    let result = catch_unwind(AssertUnwindSafe(|| match format {
        FORMAT_PARQUET => read_parquet(bytes, &limits),
        FORMAT_ARROW_C => Err(PROVIDER_FORMAT),
        _ => Err(PROVIDER_FORMAT),
    }));
    match result {
        Ok(Ok(exported)) => {
            // Move each complete tree into its root private-data token before
            // publishing the root pointer. The matching release callback then
            // owns the same allocation and all child storage exactly once.
            let schema_tree = Box::into_raw(exported.schema);
            let array_tree = Box::into_raw(exported.array);
            unsafe {
                (*schema_tree).raw.private_data = schema_tree as *mut c_void;
                (*array_tree).raw.private_data = array_tree as *mut c_void;
                *out_schema = &mut (*schema_tree).raw;
                *out_array = &mut (*array_tree).raw;
            }
            PROVIDER_OK
        }
        Ok(Err(status)) => status,
        Err(_) => PROVIDER_FAILURE,
    }
}

fn valid_limits(limits: &ProviderLimits) -> bool {
    limits.max_rows >= 0
        && limits.max_columns >= 0
        && limits.max_children >= 0
        && limits.max_dictionary_values >= 0
        && limits.max_buffer_bytes > 0
        && limits.max_total_bytes > 0
}

fn int96_physical_columns(
    schema: &Schema,
    parquet_schema: &SchemaDescriptor,
) -> Result<Vec<(usize, usize)>, i32> {
    let mut fields = vec![None; schema.fields().len()];
    for (column_index, column) in parquet_schema.columns().iter().enumerate() {
        if column.physical_type() != PhysicalType::INT96 {
            continue;
        }
        let field_name = column
            .path()
            .parts()
            .first()
            .ok_or(PROVIDER_METADATA)?;
        let field_index = schema
            .fields()
            .iter()
            .position(|field| field.name() == field_name.as_str())
            .ok_or(PROVIDER_METADATA)?;
        if fields[field_index].replace(column_index).is_some() {
            return Err(PROVIDER_TYPE);
        }
    }
    Ok(fields
        .into_iter()
        .enumerate()
        .filter_map(|(field_index, column_index)| {
            column_index.map(|column_index| (field_index, column_index))
        })
        .collect())
}

fn read_int96_columns(
    bytes: &Bytes,
    columns: &[(usize, usize)],
) -> Result<Vec<(usize, Vec<Option<Int96>>)>, i32> {
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    let file_reader = SerializedFileReader::new(bytes.clone()).map_err(classify_error)?;
    let schema = file_reader.metadata().file_metadata().schema_descr();
    let expected_rows = usize::try_from(file_reader.metadata().file_metadata().num_rows())
        .map_err(|_| PROVIDER_METADATA)?;
    let mut output = Vec::with_capacity(columns.len());
    for &(field_index, column_index) in columns {
        let mut values_out = Vec::with_capacity(expected_rows);
        let descriptor = schema.column(column_index);
        if descriptor.physical_type() != PhysicalType::INT96
            || descriptor.max_rep_level() != 0
        {
            return Err(PROVIDER_TYPE);
        }
        let max_def_level = descriptor.max_def_level();
        for row_group_index in 0..file_reader.num_row_groups() {
            let row_group = file_reader
                .get_row_group(row_group_index)
                .map_err(classify_error)?;
            let row_count = usize::try_from(row_group.metadata().num_rows())
                .map_err(|_| PROVIDER_METADATA)?;
            let mut reader =
                get_typed_column_reader::<Int96Type>(
                    row_group
                        .get_column_reader(column_index)
                        .map_err(classify_error)?,
                );
            let mut remaining = row_count;
            let mut def_levels = Vec::new();
            let mut values = Vec::new();
            while remaining > 0 {
                def_levels.clear();
                values.clear();
                let (records_read, values_read, levels_read) = reader
                    .read_records(
                        remaining,
                        (max_def_level != 0).then_some(&mut def_levels),
                        None,
                        &mut values,
                    )
                    .map_err(classify_error)?;
                if records_read == 0
                    || levels_read == 0
                    || records_read > remaining
                    || values.len() != values_read
                {
                    return Err(PROVIDER_METADATA);
                }
                if max_def_level == 0 {
                    if levels_read != records_read || values_read != records_read {
                        return Err(PROVIDER_METADATA);
                    }
                    values_out.extend(values.iter().copied().map(Some));
                } else {
                    if def_levels.len() != levels_read {
                        return Err(PROVIDER_METADATA);
                    }
                    let mut value_index = 0usize;
                    for definition in def_levels.iter().copied() {
                        if definition < 0 || definition > max_def_level {
                            return Err(PROVIDER_METADATA);
                        }
                        if definition == max_def_level {
                            let value = values
                                .get(value_index)
                                .copied()
                                .ok_or(PROVIDER_METADATA)?;
                            values_out.push(Some(value));
                            value_index = value_index
                                .checked_add(1)
                                .ok_or(PROVIDER_METADATA)?;
                        } else {
                            values_out.push(None);
                        }
                    }
                    if value_index != values_read {
                        return Err(PROVIDER_METADATA);
                    }
                }
                remaining -= records_read;
            }
        }
        if values_out.len() != expected_rows {
            return Err(PROVIDER_METADATA);
        }
        output.push((field_index, values_out));
    }
    Ok(output)
}

fn int96_total_nanos(value: &Int96) -> Result<i128, i32> {
    const JULIAN_DAY_OF_EPOCH: i64 = 2_440_588;
    const NANOS_PER_DAY: u64 = 86_400_000_000_000;
    let words = value.data();
    let nanos_of_day = (u64::from(words[1]) << 32) | u64::from(words[0]);
    if nanos_of_day >= NANOS_PER_DAY {
        return Err(PROVIDER_METADATA);
    }
    let day_delta = i64::from(words[2])
        .checked_sub(JULIAN_DAY_OF_EPOCH)
        .ok_or(PROVIDER_TYPE)?;
    i128::from(day_delta)
        .checked_mul(i128::from(NANOS_PER_DAY))
        .and_then(|days| days.checked_add(i128::from(nanos_of_day)))
        .ok_or(PROVIDER_TYPE)
}

fn int96_resolution(values: &[Option<Int96>]) -> Result<TimeUnit, i32> {
    let mut fits_nanoseconds = true;
    let mut fits_microseconds = true;
    for value in values.iter().flatten() {
        let total_nanos = int96_total_nanos(value)?;
        if !(i128::from(i64::MIN)..=i128::from(i64::MAX)).contains(&total_nanos) {
            fits_nanoseconds = false;
        }
        if total_nanos % 1_000 != 0
            || !(i128::from(i64::MIN)..=i128::from(i64::MAX))
                .contains(&(total_nanos / 1_000))
        {
            fits_microseconds = false;
        }
    }
    if fits_nanoseconds {
        Ok(TimeUnit::Nanosecond)
    } else if fits_microseconds {
        Ok(TimeUnit::Microsecond)
    } else {
        Err(PROVIDER_TYPE)
    }
}

fn int96_schema(
    schema: &Schema,
    resolutions: &[Option<TimeUnit>],
) -> Result<Option<Schema>, i32> {
    let mut changed = false;
    let fields = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(field_index, field)| {
            let data_type = match resolutions
                .get(field_index)
                .and_then(Option::as_ref)
            {
                Some(resolution) => int96_data_type(field.data_type(), resolution)
                    .ok_or(PROVIDER_TYPE)?,
                None => field.data_type().clone(),
            };
            changed |= data_type != field.data_type().clone();
            Ok(field.as_ref().clone().with_data_type(data_type))
        })
        .collect::<Result<Vec<_>, i32>>()?;
    Ok(changed.then(|| Schema::new_with_metadata(fields, schema.metadata.clone())))
}

fn int96_data_type(data_type: &DataType, resolution: &TimeUnit) -> Option<DataType> {
    match data_type {
        DataType::Timestamp(_, timezone) => Some(DataType::Timestamp(
            resolution.clone(),
            timezone.clone(),
        )),
        DataType::Dictionary(key, value) => Some(DataType::Dictionary(
            key.clone(),
            Box::new(int96_data_type(value.as_ref(), resolution)?),
        )),
        _ => None,
    }
}

fn read_parquet(input: &[u8], limits: &ProviderLimits) -> Result<ExportedBatch, i32> {
    if input.len() > limits.max_total_bytes {
        return Err(PROVIDER_LIMIT);
    }
    let max_rows = usize::try_from(limits.max_rows).map_err(|_| PROVIDER_LIMIT)?;
    let max_columns = usize::try_from(limits.max_columns).map_err(|_| PROVIDER_LIMIT)?;
    let max_children = usize::try_from(limits.max_children).map_err(|_| PROVIDER_LIMIT)?;
    let max_dictionary_values =
        usize::try_from(limits.max_dictionary_values).map_err(|_| PROVIDER_LIMIT)?;
    let bytes = Bytes::copy_from_slice(input);
    let native_builder =
        ParquetRecordBatchReaderBuilder::try_new(bytes.clone()).map_err(classify_error)?;
    let metadata = native_builder.metadata();
    let file_rows = metadata.file_metadata().num_rows();
    if file_rows < 0 || usize::try_from(file_rows).map_err(|_| PROVIDER_LIMIT)? > max_rows {
        return Err(PROVIDER_LIMIT);
    }
    let native_schema = native_builder.schema().clone();
    if native_schema.fields().len() > max_columns {
        return Err(PROVIDER_LIMIT);
    }
    validate_schema_types(&native_schema, max_children, max_dictionary_values)?;
    let physical_columns =
        int96_physical_columns(&native_schema, native_builder.parquet_schema())?;
    let mut resolutions = vec![None; native_schema.fields().len()];
    for (field_index, values) in read_int96_columns(&bytes, &physical_columns)? {
        resolutions[field_index] = Some(int96_resolution(&values)?);
    }
    let requested_schema = int96_schema(&native_schema, &resolutions)?;
    let mut builder = if let Some(schema) = requested_schema {
        let options = ArrowReaderOptions::new().with_schema(Arc::new(schema));
        ParquetRecordBatchReaderBuilder::try_new_with_options(bytes.clone(), options)
            .map_err(classify_error)?
    } else {
        native_builder
    };
    let schema = builder.schema().clone();
    validate_schema_types(&schema, max_children, max_dictionary_values)?;
    let batch_size = max_rows.max(1).min(MAX_BATCH_ROWS);
    let max_batches = max_rows
        .checked_add(batch_size - 1)
        .and_then(|rows| rows.checked_div(batch_size))
        .and_then(|batches| batches.checked_add(1))
        .ok_or(PROVIDER_LIMIT)?;
    builder = builder.with_batch_size(batch_size);
    if max_rows > 0 {
        builder = builder.with_limit(max_rows);
    }
    let reader = builder.build().map_err(classify_error)?;
    let mut batches = Vec::with_capacity(max_batches.min(1024));
    let mut rows = 0usize;
    let mut materialized_bytes = 0usize;
    for batch in reader {
        let batch = batch.map_err(classify_error)?;
        if batch.num_columns() != schema.fields().len() {
            return Err(PROVIDER_METADATA);
        }
        rows = rows
            .checked_add(batch.num_rows())
            .ok_or(PROVIDER_LIMIT)?;
        if rows > max_rows || batches.len() >= max_batches {
            return Err(PROVIDER_LIMIT);
        }
        let batch_bytes = batch_memory_bytes(&batch, max_children)?;
        materialized_bytes = materialized_bytes
            .checked_add(batch_bytes)
            .ok_or(PROVIDER_LIMIT)?;
        // A single batch is exported without concat's duplicate allocation.
        // Once there are multiple inputs, keep their retained side below
        // half the total budget so the merged result remains bounded.
        let merge_budget = if batches.is_empty() {
            limits.max_total_bytes
        } else {
            limits.max_total_bytes / 2
        };
        if materialized_bytes > merge_budget {
            return Err(PROVIDER_LIMIT);
        }
        batches.push(batch);
    }
    let batch = if batches.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else if batches.len() == 1 {
        batches.pop().expect("one batch was checked")
    } else {
        concat_batches(&schema, batches.iter()).map_err(classify_error)?
    };
    if batch.num_rows() > max_rows
        || batch_memory_bytes(&batch, max_children)? > limits.max_total_bytes
    {
        return Err(PROVIDER_LIMIT);
    }
    export_batch(batch, limits, max_children)
}

fn batch_memory_bytes(batch: &RecordBatch, max_children: usize) -> Result<usize, i32> {
    batch.columns().iter().try_fold(0usize, |total, array| {
        let bytes = array_data_memory(&array.to_data(), 0, max_children)?;
        total.checked_add(bytes).ok_or(PROVIDER_LIMIT)
    })
}

fn array_data_memory(data: &ArrayData, depth: usize, max_children: usize) -> Result<usize, i32> {
    if depth > max_children {
        return Err(PROVIDER_NESTING);
    }
    let mut total = 0usize;
    for buffer in data.buffers() {
        total = total.checked_add(buffer.len()).ok_or(PROVIDER_LIMIT)?;
    }
    if let Some(nulls) = data.nulls() {
        total = total
            .checked_add(nulls.buffer().len())
            .ok_or(PROVIDER_LIMIT)?;
    }
    for child in data.child_data() {
        total = total
            .checked_add(array_data_memory(child, depth + 1, max_children)?)
            .ok_or(PROVIDER_LIMIT)?;
    }
    Ok(total)
}

fn validate_schema_types(
    schema: &Schema,
    max_children: usize,
    max_dictionary_values: usize,
) -> Result<(), i32> {
    for field in schema.fields() {
        validate_type(field.data_type(), 0, max_children, max_dictionary_values)?;
    }
    Ok(())
}

fn validate_type(
    data_type: &DataType,
    depth: usize,
    max_children: usize,
    max_dictionary_values: usize,
) -> Result<(), i32> {
    if depth > max_children {
        return Err(PROVIDER_NESTING);
    }
    match data_type {
        DataType::Dictionary(key, value) => {
            if !matches!(
                key.as_ref(),
                DataType::Int8
                    | DataType::Int16
                    | DataType::Int32
                    | DataType::Int64
                    | DataType::UInt8
                    | DataType::UInt16
                    | DataType::UInt32
                    | DataType::UInt64
            ) {
                return Err(PROVIDER_TYPE);
            }
            validate_type(value, depth + 1, max_children, max_dictionary_values)
        }
        DataType::Boolean
        | DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float16
        | DataType::Float32
        | DataType::Float64
        | DataType::Date32
        | DataType::Date64
        | DataType::Time32(_)
        | DataType::Time64(_)
        | DataType::Timestamp(_, _)
        | DataType::Duration(_)
        | DataType::Utf8
        | DataType::Binary => Ok(()),
        _ => Err(PROVIDER_TYPE),
    }
}

fn export_batch(
    batch: RecordBatch,
    limits: &ProviderLimits,
    max_children: usize,
) -> Result<ExportedBatch, i32> {
    let schema = build_schema_tree(batch.schema().as_ref())?;
    let array = build_array_tree(&batch, limits, max_children)?;
    Ok(ExportedBatch { schema, array })
}

fn build_schema_tree(schema: &Schema) -> Result<Box<SchemaTree>, i32> {
    let mut nodes = Vec::with_capacity(schema.fields().len());
    for field in schema.fields() {
        nodes.push(build_schema_node(field.as_ref(), 0)?);
    }
    let children = nodes
        .iter_mut()
        .map(|node| &mut node.raw as *mut ArrowSchema)
        .collect::<Vec<_>>();
    let root_format = CString::new("+s").map_err(|_| PROVIDER_METADATA)?;
    let root_name = CString::new("").map_err(|_| PROVIDER_METADATA)?;
    let child_count = i64::try_from(children.len()).map_err(|_| PROVIDER_LIMIT)?;
    let mut tree = Box::new(SchemaTree {
        raw: ArrowSchema {
            format: std::ptr::null(),
            name: std::ptr::null(),
            metadata: std::ptr::null(),
            flags: 0,
            n_children: child_count,
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_schema),
            private_data: std::ptr::null_mut(),
        },
        root_format,
        root_name,
        children,
        nodes,
    });
    tree.raw.format = tree.root_format.as_ptr() as *const _;
    tree.raw.name = tree.root_name.as_ptr() as *const _;
    tree.raw.children = tree.children.as_mut_ptr();
    Ok(tree)
}

fn build_schema_node(field: &Field, depth: usize) -> Result<Box<SchemaNode>, i32> {
    let format = format_for_type(field.data_type())?;
    let name = CString::new(field.name().as_str()).map_err(|_| PROVIDER_METADATA)?;
    let format_text = CString::new(format).map_err(|_| PROVIDER_METADATA)?;
    let metadata = logical_metadata(field.data_type())
        .map(|text| CString::new(text).map_err(|_| PROVIDER_METADATA))
        .transpose()?;
    let dictionary = match field.data_type() {
        DataType::Dictionary(_, value) => Some(build_schema_node(
            &Field::new("", value.as_ref().clone(), true),
            depth + 1,
        )?),
        _ => None,
    };
    let mut node = Box::new(SchemaNode {
        raw: ArrowSchema {
            format: std::ptr::null(),
            name: std::ptr::null(),
            metadata: std::ptr::null(),
            flags: if field.is_nullable() {
                ARROW_FLAG_NULLABLE
            } else {
                0
            },
            n_children: 0,
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: None,
            private_data: std::ptr::null_mut(),
        },
        format: format_text,
        name,
        metadata,
        dictionary,
    });
    node.raw.format = node.format.as_ptr();
    node.raw.name = node.name.as_ptr();
    node.raw.metadata = node
        .metadata
        .as_ref()
        .map_or(std::ptr::null(), |metadata| metadata.as_ptr());
    if let Some(dictionary) = node.dictionary.as_mut() {
        node.raw.dictionary = &mut dictionary.raw;
    }
    Ok(node)
}

fn logical_metadata(data_type: &DataType) -> Option<String> {
    match data_type {
        DataType::Date32 => Some("jet.logical=date32".to_string()),
        DataType::Date64 => Some("jet.logical=date64".to_string()),
        DataType::Time32(unit) => Some(format!("jet.logical=time32:{unit:?}")),
        DataType::Time64(unit) => Some(format!("jet.logical=time64:{unit:?}")),
        DataType::Timestamp(unit, timezone) => Some(format!(
            "jet.logical=timestamp:{unit:?}:{}",
            timezone.as_deref().unwrap_or("")
        )),
        DataType::Duration(unit) => Some(format!("jet.logical=duration:{unit:?}")),
        DataType::Dictionary(_, value) => logical_metadata(value),
        _ => None,
    }
}

fn build_array_tree(
    batch: &RecordBatch,
    limits: &ProviderLimits,
    max_children: usize,
) -> Result<Box<ArrayTree>, i32> {
    let mut nodes = Vec::with_capacity(batch.num_columns());
    let mut total_bytes = 0usize;
    for array in batch.columns() {
        nodes.push(build_array_node(
            array.to_data(),
            limits,
            0,
            max_children,
            &mut total_bytes,
        )?);
    }
    let children = nodes
        .iter_mut()
        .map(|node| &mut node.raw as *mut ArrowArray)
        .collect::<Vec<_>>();
    let child_count = i64::try_from(children.len()).map_err(|_| PROVIDER_LIMIT)?;
    let length = i64::try_from(batch.num_rows()).map_err(|_| PROVIDER_LIMIT)?;
    let buffers = vec![std::ptr::null()];
    let mut tree = Box::new(ArrayTree {
        raw: ArrowArray {
            length,
            null_count: 0,
            offset: 0,
            n_buffers: 1,
            n_children: child_count,
            buffers: std::ptr::null(),
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: Some(release_array),
            private_data: std::ptr::null_mut(),
        },
        buffers,
        children,
        nodes,
    });
    tree.raw.buffers = tree.buffers.as_ptr();
    tree.raw.children = tree.children.as_mut_ptr();
    Ok(tree)
}

fn build_array_node(
    data: ArrayData,
    limits: &ProviderLimits,
    depth: usize,
    max_children: usize,
    total_bytes: &mut usize,
) -> Result<Box<ArrayNode>, i32> {
    if depth > max_children {
        return Err(PROVIDER_NESTING);
    }
    format_for_type(data.data_type())?;
    let variable = matches!(data.data_type(), DataType::Utf8 | DataType::Binary);
    if !data.child_data().is_empty() && !matches!(data.data_type(), DataType::Dictionary(_, _)) {
        return Err(PROVIDER_NESTING);
    }
    let mut dictionary = None;
    if matches!(data.data_type(), DataType::Dictionary(_, _)) {
        if data.child_data().len() != 1 {
            return Err(PROVIDER_METADATA);
        }
        let max_dictionary_values =
            usize::try_from(limits.max_dictionary_values).map_err(|_| PROVIDER_LIMIT)?;
        if data.child_data()[0].len() > max_dictionary_values {
            return Err(PROVIDER_LIMIT);
        }
        dictionary = Some(build_array_node(
            data.child_data()[0].clone(),
            limits,
            depth + 1,
            max_children,
            total_bytes,
        )?);
    }
    let buffers = data.buffers();
    let required = if variable { 2 } else { 1 };
    if buffers.len() != required {
        return Err(PROVIDER_OFFSETS);
    }
    let mut node_bytes = 0usize;
    if let Some(nulls) = data.nulls() {
        node_bytes = node_bytes
            .checked_add(nulls.buffer().len())
            .ok_or(PROVIDER_LIMIT)?;
    }
    for buffer in buffers {
        node_bytes = node_bytes
            .checked_add(buffer.len())
            .ok_or(PROVIDER_LIMIT)?;
    }
    if node_bytes > limits.max_buffer_bytes {
        return Err(PROVIDER_LIMIT);
    }
    *total_bytes = total_bytes
        .checked_add(node_bytes)
        .ok_or(PROVIDER_LIMIT)?;
    if *total_bytes > limits.max_total_bytes {
        return Err(PROVIDER_LIMIT);
    }
    let mut c_buffers = Vec::with_capacity(if variable { 3 } else { 2 });
    c_buffers.push(
        data.nulls()
            .map(|nulls| nulls.buffer().as_ptr() as *const c_void)
            .unwrap_or(std::ptr::null()),
    );
    if variable {
        c_buffers.push(buffers[0].as_ptr() as *const c_void);
        c_buffers.push(buffers[1].as_ptr() as *const c_void);
    } else {
        c_buffers.push(buffers[0].as_ptr() as *const c_void);
    }
    let raw_length = i64::try_from(data.len()).map_err(|_| PROVIDER_LIMIT)?;
    let raw_offset = i64::try_from(data.offset()).map_err(|_| PROVIDER_LIMIT)?;
    let raw_null_count = i64::try_from(data.null_count()).map_err(|_| PROVIDER_LIMIT)?;
    let n_buffers = i64::try_from(c_buffers.len()).map_err(|_| PROVIDER_LIMIT)?;
    let mut node = Box::new(ArrayNode {
        raw: ArrowArray {
            length: raw_length,
            null_count: raw_null_count,
            offset: raw_offset,
            n_buffers,
            n_children: 0,
            buffers: std::ptr::null(),
            children: std::ptr::null_mut(),
            dictionary: std::ptr::null_mut(),
            release: None,
            private_data: std::ptr::null_mut(),
        },
        buffers: c_buffers,
        dictionary,
        data,
    });
    node.raw.buffers = node.buffers.as_ptr();
    if let Some(dictionary) = node.dictionary.as_mut() {
        node.raw.dictionary = &mut dictionary.raw;
    }
    Ok(node)
}

fn format_for_type(data_type: &DataType) -> Result<&'static str, i32> {
    match data_type {
        DataType::Boolean => Ok("b"),
        DataType::Int8 => Ok("c"),
        DataType::Int16 => Ok("s"),
        DataType::Int32 | DataType::Date32 | DataType::Time32(_) => Ok("i"),
        DataType::Int64
        | DataType::Date64
        | DataType::Time64(_)
        | DataType::Timestamp(_, _)
        | DataType::Duration(_) => Ok("l"),
        DataType::UInt8 => Ok("C"),
        DataType::UInt16 => Ok("S"),
        DataType::UInt32 => Ok("I"),
        DataType::UInt64 => Ok("L"),
        DataType::Float16 => Ok("e"),
        DataType::Float32 => Ok("f"),
        DataType::Float64 => Ok("g"),
        DataType::Utf8 => Ok("u"),
        DataType::Binary => Ok("z"),
        DataType::Dictionary(key, _) => format_for_type(key),
        _ => Err(PROVIDER_TYPE),
    }
}

fn classify_error(error: impl std::fmt::Display) -> i32 {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("compression")
        || message.contains("codec")
        || message.contains("snappy")
        || message.contains("zstd")
        || message.contains("brotli")
        || message.contains("lz4")
    {
        PROVIDER_COMPRESSION
    } else if message.contains("offset")
        || message.contains("buffer")
        || message.contains("length")
        || message.contains("utf-8")
    {
        PROVIDER_OFFSETS
    } else if message.contains("metadata") || message.contains("schema") {
        PROVIDER_METADATA
    } else {
        PROVIDER_FAILURE
    }
}

unsafe extern "C" fn release_schema(raw: *mut ArrowSchema) {
    if raw.is_null() {
        return;
    }
    // SAFETY: only the root callback is exposed.  private_data was installed
    // by this module and is the unique Box allocation for the complete tree.
    let tree = (*raw).private_data as *mut SchemaTree;
    (*raw).release = None;
    if !tree.is_null() {
        drop(Box::from_raw(tree));
    }
}

unsafe extern "C" fn release_array(raw: *mut ArrowArray) {
    if raw.is_null() {
        return;
    }
    // SAFETY: only the root callback is exposed.  private_data was installed
    // by this module and owns every ArrayData buffer and child pointer.
    let tree = (*raw).private_data as *mut ArrayTree;
    (*raw).release = None;
    if !tree.is_null() {
        drop(Box::from_raw(tree));
    }
}
