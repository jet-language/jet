//! `core.data` host shims — same checked analytics rules as DataFlow.rs.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};

#[allow(unused_imports)]
use jet_foundation::Outcome::*;

// #1657 / I9: the one `core.data` statistics, bar-plot and bridge-status
// kernel, included from the exact Prelude source AOT embeds and comptime
// includes. Only the `jet_std` value types are declared here; every rule lives
// in the included file. A second copy of this math in the JIT host is an I9
// violation.
#[allow(dead_code)]
mod data_kernel {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;

    pub(crate) mod jet_std {
        #[allow(unused_imports)]
        pub use jet_foundation::Outcome::*;

        #[allow(dead_code)]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) enum DataErrorKind {
            Decode,
            Limit,
            IO,
            Empty,
            InvalidArgument,
            NonFinite,
            Overflow,
            State,
            Bridge,
        }

        /// `DataError.cause` stays optional; when present it uses the shared
        /// canonical encoding error carrier.
        pub(crate) use crate::Encoding::json_rt::EncodingError;

        #[derive(Clone, Debug)]
        pub(crate) struct DataError {
            pub(crate) kind: DataErrorKind,
            pub(crate) operation: String,
            pub(crate) row: JetOutcome<i64, JetAbsent>,
            pub(crate) column: JetOutcome<i64, JetAbsent>,
            pub(crate) index: JetOutcome<i64, JetAbsent>,
            pub(crate) reason: String,
            pub(crate) cause: JetOutcome<EncodingError, JetAbsent>,
        }


        #[derive(Clone, Debug, PartialEq)]
        pub(crate) struct GroupValue<K, V> {
            pub(crate) key: K,
            pub(crate) value: V,
        }

        #[derive(Clone, Debug)]
        pub(crate) struct DataSummary {
            pub(crate) count: i64,
            pub(crate) sum: f64,
            pub(crate) mean: f64,
            pub(crate) min: f64,
            pub(crate) max: f64,
            pub(crate) median: f64,
            pub(crate) variance: f64,
            pub(crate) stddev: f64,
        }

        #[derive(Clone, Debug)]
        pub(crate) struct DataStatus {
            pub(crate) step: String,
            pub(crate) path: String,
            pub(crate) copy: String,
            pub(crate) ownership: String,
            pub(crate) trust: String,
            pub(crate) fallback: String,
            pub(crate) replacement: String,
        }
        pub(crate) use crate::Encoding::data_query_rt::{
            jet_table_plan_from_descriptors, JetTableCallable, JetTablePlan, JetTableStreamMode,
            JetTableTypeId,
        };

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct DataLimits {
            pub(crate) max_groups: i64,
            pub(crate) max_sort_rows: i64,
            pub(crate) max_join_rows: i64,
            pub(crate) max_output_rows: i64,
        }
        impl DataLimits {
            pub(crate) fn safe() -> Self {
                Self {
                    max_groups: 100_000,
                    max_sort_rows: 1_000_000,
                    max_join_rows: 1_000_000,
                    max_output_rows: 1_000_000,
                }
            }
        }


        /// D-DATA-PLOT1=A: shared options for the deterministic line renderers.
        /// `Default` by hand: an empty reference line is a clean absence, which
        /// the carrier spells rather than derives.
        #[derive(Clone, Debug)]
        pub(crate) struct DataLineOptions {
            pub(crate) title: String,
            pub(crate) x_label: String,
            pub(crate) y_label: String,
            pub(crate) markers: bool,
            pub(crate) reference: JetOutcome<f64, JetAbsent>,
            pub(crate) style: String,
            pub(crate) color: String,
            pub(crate) legend: String,
        }
        impl Default for DataLineOptions {
            fn default() -> Self {
                Self {
                    title: String::new(),
                    x_label: String::new(),
                    y_label: String::new(),
                    markers: false,
                    reference: Err(JetAbsent),
                    style: String::new(),
                    color: String::new(),
                    legend: String::new(),
                }
            }
        }
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs");
}
impl crate::JetShow for String {
    fn jet_show(&self) -> String {
        self.clone()
    }
}

mod data_plot_rt {
    pub(crate) use super::data_kernel::jet_std;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/DataPlot.rs");
}

/// Erased row word passed to the checked universal column callback.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DataPlotRow(i64);

#[derive(Clone)]
struct DataPlotColumnSlot {
    field: data_plot_rt::JetDataPlotField,
    callback: crate::runtime_host::JitCallableSlot,
}

/// Runtime-owned typed plot carrier. `plot` is always the canonical Prelude
/// plot; `columns` retains checked callback pairs for future transformations.
#[derive(Clone)]
pub(crate) struct DataPlotSlot {
    pub(crate) plot: data_plot_rt::JetDataPlot<DataPlotRow>,
    columns: Vec<DataPlotColumnSlot>,
}

fn data_error(operation: &str, reason: impl Into<String>) -> DataError {
    err(DataErrorKind::InvalidArgument, operation, reason)
}

fn data_decode_error(operation: &str, reason: impl Into<String>) -> DataError {
    err(DataErrorKind::Decode, operation, reason)
}

fn plan_index(handle: i64) -> Option<usize> {
    usize::try_from(handle.checked_sub(1)?).ok()
}



fn row_word(
    rt: &mut crate::JitRuntime,
    value: jet_rt::JetVal,
    operation: &str,
) -> Result<i64, DataError> {
    match value {
        jet_rt::JetVal::Int(value) | jet_rt::JetVal::RecordRef(value) => Ok(value),
        jet_rt::JetVal::Float(value) => Ok(value.to_bits() as i64),
        jet_rt::JetVal::Bool(value) => Ok(i64::from(value)),
        jet_rt::JetVal::Char(value) => Ok(i64::from(u32::from(value))),
        jet_rt::JetVal::String(value) => Ok(rt.heap.alloc_string(value)),
        jet_rt::JetVal::StringView { owner, start, end } => {
            let text = rt
                .heap
                .get_string(owner)
                .and_then(|text| text.get(start..end))
                .ok_or_else(|| {
                    data_decode_error(operation, "table row contains an invalid string view")
                })?
                .to_string();
            Ok(rt.heap.alloc_string(text))
        }
        _ => Err(data_decode_error(
            operation,
            "table row has no checked scalar ABI",
        )),
    }
}



fn store_plot(rt: &mut crate::JitRuntime, slot: DataPlotSlot) -> Result<i64, DataError> {
    rt.data_plots.push(slot);
    i64::try_from(rt.data_plots.len())
        .map_err(|_| data_error("data.plot", "too many JetDataPlot values"))
}

fn plot_slot(
    rt: &mut crate::JitRuntime,
    handle: i64,
    operation: &str,
) -> Result<DataPlotSlot, DataError> {
    let Some(index) = plan_index(handle) else {
        return Err(data_error(operation, "invalid JetDataPlot handle"));
    };
    rt.data_plots
        .get(index)
        .cloned()
        .ok_or_else(|| data_error(operation, "unknown JetDataPlot handle"))
}

use data_kernel::jet_std::{DataError, DataErrorKind, DataLineOptions, DataStatus, GroupValue};

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = format!("{:?} {}", self.kind, self.operation);
        if let Ok(index) = self.index {
            out.push_str(&format!(", index {index}"));
        }
        out.push_str(&format!(": {}", self.reason));
        f.write_str(&out)
    }
}

fn err(kind: DataErrorKind, operation: &str, reason: impl Into<String>) -> DataError {
    data_kernel::jet_data_error(kind, operation, reason)
}

fn float_list(handle: i64) -> Vec<f64> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(handle).unwrap_or(0);
        (0..len)
            .filter_map(|i| rt.heap.list_get_float(handle, i))
            .collect()
    })
}

fn result_f64(r: Result<f64, DataError>) -> i64 {
    match r {
        Ok(v) => Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, v.to_bits())
        }),
        Err(e) => result_data_err(e),
    }
}

fn pack_data_error(rt: &mut crate::runtime_host::JitRuntime, e: DataError) -> i64 {
    let h = rt.heap.alloc_record(7);
    let kind = rt.heap.alloc_string(format!("{:?}", e.kind));
    let _ = rt.heap.record_set_string(h, 0, kind);
    let op = rt.heap.alloc_string(e.operation.clone());
    let _ = rt.heap.record_set_string(h, 1, op);
    let absent = crate::runtime_host::alloc_jit_result(rt, false, 0);
    let _ = rt.heap.record_set_int(h, 2, absent);
    let _ = rt.heap.record_set_int(h, 3, absent);
    let index = match e.index {
        Ok(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
        Err(_) => absent,
    };
    let _ = rt.heap.record_set_int(h, 4, index);
    let reason = rt.heap.alloc_string(e.reason.clone());
    let _ = rt.heap.record_set_string(h, 5, reason);
    let _ = rt.heap.record_set_int(h, 6, absent);
    crate::runtime_host::alloc_jit_result(rt, false, h as u64)
}

fn result_data_err(e: DataError) -> i64 {
    Concurrency::with_runtime_mut(|rt| pack_data_error(rt, e))
}

fn jet_jit_data_error_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let kind = rt
            .heap
            .record_get_string(handle, 0)
            .and_then(|s| rt.heap.clone_string(s))
            .unwrap_or_default();
        let op = rt
            .heap
            .record_get_string(handle, 1)
            .and_then(|s| rt.heap.clone_string(s))
            .unwrap_or_default();
        let reason = rt
            .heap
            .record_get_string(handle, 5)
            .and_then(|s| rt.heap.clone_string(s))
            .unwrap_or_default();
        let idx = rt.heap.record_get_int(handle, 4).unwrap_or(0);
        let text = if idx > 0 {
            format!("{kind} {op}, index {}: {reason}", idx - 1)
        } else {
            format!("{kind} {op}: {reason}")
        };
        rt.heap.alloc_string(text)
    })
}

fn result_unit(r: Result<(), DataError>) -> i64 {
    match r {
        Ok(()) => {
            Concurrency::with_runtime_mut(|rt| crate::runtime_host::alloc_jit_result(rt, true, 0))
        }
        Err(e) => result_data_err(e),
    }
}

fn pack_status(rows: Vec<DataStatus>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let mut handles = Vec::new();
        for row in rows {
            let h = rt.heap.alloc_record(7);
            let fields = [
                row.step,
                row.path,
                row.copy,
                row.ownership,
                row.trust,
                row.fallback,
                row.replacement,
            ];
            for (i, s) in fields.into_iter().enumerate() {
                let sid = rt.heap.alloc_string(s);
                let _ = rt.heap.record_set_string(h, i as i64, sid);
            }
            handles.push(h);
        }
        rt.heap.alloc_int_list(handles)
    })
}

fn jet_jit_data_status() -> i64 {
    pack_status(data_kernel::jet_data_status())
}

fn jet_jit_data_require_bridge(provider: i64) -> i64 {
    let name =
        Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(provider).unwrap_or_default());
    result_unit(data_kernel::jet_data_require_bridge(&name))
}

fn jet_jit_data_stat(values: i64, op: i64) -> i64 {
    let vals = float_list(values);
    let r = match op {
        0 => data_kernel::jet_data_mean_checked(&vals),
        1 => data_kernel::jet_data_sum_checked(&vals),
        2 => data_kernel::jet_data_min_checked(&vals),
        3 => data_kernel::jet_data_max_checked(&vals),
        4 => data_kernel::jet_data_median_checked(&vals),
        5 => data_kernel::jet_data_variance_checked(&vals),
        _ => data_kernel::jet_data_stddev_checked(&vals),
    };
    result_f64(r)
}

fn jet_jit_data_quantile(values: i64, q_bits: i64) -> i64 {
    let vals = float_list(values);
    let q = f64::from_bits(q_bits as u64);
    result_f64(data_kernel::jet_data_quantile_checked(&vals, q))
}

fn jet_jit_data_describe(values: i64) -> i64 {
    let vals = float_list(values);
    Concurrency::with_runtime_mut(|rt| match data_kernel::jet_data_describe_checked(&vals) {
        Ok(s) => {
            let h = rt.heap.alloc_record(8);
            let _ = rt.heap.record_set_int(h, 0, s.count);
            let _ = rt.heap.record_set_float(h, 1, s.sum);
            let _ = rt.heap.record_set_float(h, 2, s.mean);
            let _ = rt.heap.record_set_float(h, 3, s.min);
            let _ = rt.heap.record_set_float(h, 4, s.max);
            let _ = rt.heap.record_set_float(h, 5, s.median);
            let _ = rt.heap.record_set_float(h, 6, s.variance);
            let _ = rt.heap.record_set_float(h, 7, s.stddev);
            crate::runtime_host::alloc_jit_result(rt, true, h as u64)
        }
        Err(e) => {
            let sid = rt.heap.alloc_string(e.to_string());
            crate::runtime_host::alloc_jit_result(rt, false, sid as u64)
        }
    })
}

fn load_count_groups(groups: i64) -> Vec<GroupValue<String, i64>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(groups).unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..len {
            let h = rt.heap.list_get_int(groups, i).unwrap_or(0);
            let key = rt
                .heap
                .record_get_string(h, 0)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_default();
            out.push(GroupValue {
                key,
                value: rt.heap.record_get_int(h, 1).unwrap_or(0),
            });
        }
        out
    })
}

fn load_value_groups(groups: i64) -> Vec<GroupValue<String, f64>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(groups).unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..len {
            let h = rt.heap.list_get_int(groups, i).unwrap_or(0);
            let key = rt
                .heap
                .record_get_string(h, 0)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_default();
            out.push(GroupValue {
                key,
                value: rt.heap.record_get_float(h, 1).unwrap_or(0.0),
            });
        }
        out
    })
}

fn jet_jit_data_bar_text(groups: i64) -> i64 {
    let groups = load_count_groups(groups);
    Concurrency::with_runtime_mut(|rt| match data_kernel::jet_data_bar_text_checked(&groups) {
        Ok(s) => {
            let sid = rt.heap.alloc_string(s);
            crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
        }
        Err(e) => {
            let sid = rt.heap.alloc_string(e.to_string());
            crate::runtime_host::alloc_jit_result(rt, false, sid as u64)
        }
    })
}
fn jet_jit_data_bar_svg(groups: i64) -> i64 {
    let groups = load_count_groups(groups);
    Concurrency::with_runtime_mut(|rt| match data_kernel::jet_data_bar_svg_checked(&groups) {
        Ok(s) => {
            let sid = rt.heap.alloc_string(s);
            crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
        }
        Err(e) => {
            let sid = rt.heap.alloc_string(e.to_string());
            crate::runtime_host::alloc_jit_result(rt, false, sid as u64)
        }
    })
}

fn load_line_options(options: i64) -> DataLineOptions {
    Concurrency::with_runtime_mut(|rt| {
        let title = rt
            .heap
            .record_get_string(options, 0)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        let x_label = rt
            .heap
            .record_get_string(options, 1)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        let y_label = rt
            .heap
            .record_get_string(options, 2)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        let style = rt
            .heap
            .record_get_string(options, 5)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        let color = rt
            .heap
            .record_get_string(options, 6)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        let legend = rt
            .heap
            .record_get_string(options, 7)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        let reference = jet_outcome_of(
            rt.heap
                .record_get_int(options, 4)
                .and_then(|raw| (raw != 0).then(|| f64::from_bits(raw.wrapping_sub(1) as u64))),
        );
        let markers = rt.heap.record_get_bool(options, 3).unwrap_or(false);
        DataLineOptions {
            title,
            x_label,
            y_label,
            markers,
            reference,
            style,
            color,
            legend,
        }
    })
}

fn result_data_plot_err(error: data_plot_rt::DataPlotError) -> i64 {
    let kind = match error.kind {
        "NonFinite" => DataErrorKind::NonFinite,
        _ => DataErrorKind::InvalidArgument,
    };
    result_data_err(err_at(kind, error.operation, error.index, error.reason))
}

fn jet_jit_data_line_text(groups: i64, options: i64) -> i64 {
    let groups = load_value_groups(groups);
    let options = load_line_options(options);
    Concurrency::with_runtime_mut(|rt| {
        match data_plot_rt::jet_data_line_text_plot_checked(&groups, &options) {
            Ok(s) => {
                let sid = rt.heap.alloc_string(s);
                crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
            }
            Err(e) => result_data_plot_err(e),
        }
    })
}

fn jet_jit_data_line_svg(groups: i64, options: i64) -> i64 {
    let groups = load_value_groups(groups);
    let options = load_line_options(options);
    Concurrency::with_runtime_mut(|rt| {
        match data_plot_rt::jet_data_line_svg_plot_checked(&groups, &options) {
            Ok(s) => {
                let sid = rt.heap.alloc_string(s);
                crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
            }
            Err(e) => result_data_plot_err(e),
        }
    })
}


fn data_kernel_limits() -> data_kernel::JetDataKernelLimits {
    let limits = data_kernel::jet_std::DataLimits::safe();
    data_kernel::jet_data_kernel_limits(
        limits.max_groups,
        limits.max_sort_rows,
        limits.max_join_rows,
        limits.max_output_rows,
    )
}

fn data_row_words(rows: i64, operation: &str) -> Result<Vec<i64>, DataError> {
    Concurrency::with_runtime_mut(|rt| {
        Some(
            rt.heap
                .clone_list_values(rows)
                .ok_or_else(|| data_decode_error(operation, "data source is not a list"))
                .and_then(|values| {
                    values
                        .into_iter()
                        .map(|value| row_word(rt, value, operation))
                        .collect()
                }),
        )
    })
    .ok_or_else(|| data_decode_error(operation, "resident runtime is unavailable"))?
}

fn validate_data_callback(callback: i64, operation: &str) -> Result<(), DataError> {
    let Some(slot) =
        Concurrency::with_runtime_mut(|rt| crate::runtime_host::jit_callable_parts(rt, callback))
    else {
        return Err(data_decode_error(
            operation,
            "data callback handle is unknown",
        ));
    };
    if callback >= 0
        || slot.raw_unary.is_none()
        || slot.raw_pair.is_some()
        || slot.raw_many.is_some()
    {
        return Err(data_decode_error(
            operation,
            "data callback is not a unary callable",
        ));
    }
    Ok(())
}

fn data_callback_key(callback: i64, row: i64, operation: &str) -> Result<String, DataError> {
    let value = query_callback(callback, row, operation)?;
    query_callback_string(value, operation)
}

fn data_callback_float(callback: i64, row: i64, operation: &str) -> Result<f64, DataError> {
    let value = query_callback(callback, row, operation)?;
    Ok(f64::from_bits(value as u64))
}

fn jet_jit_data_inner_join(
    left: i64,
    right: i64,
    left_key_callback: i64,
    right_key_callback: i64,
) -> i64 {
    let operation = "inner_join";
    let left_rows = match data_row_words(left, operation) {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    let right_rows = match data_row_words(right, operation) {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    for callback in [left_key_callback, right_key_callback] {
        if let Err(error) = validate_data_callback(callback, operation) {
            return result_data_err(error);
        }
    }
    let left_keys = match left_rows
        .iter()
        .copied()
        .map(|row| data_callback_key(left_key_callback, row, operation))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(keys) => keys,
        Err(error) => return result_data_err(error),
    };
    let right_keys = match right_rows
        .iter()
        .copied()
        .map(|row| data_callback_key(right_key_callback, row, operation))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(keys) => keys,
        Err(error) => return result_data_err(error),
    };
    let indices = match data_kernel::jet_data_join_indices(
        &left_keys,
        &right_keys,
        false,
        &data_kernel_limits(),
    ) {
        Ok(indices) => indices,
        Err(error) => return result_data_err(error),
    };
    let rows = indices
        .into_iter()
        .map(|index| {
            let right_index = index.right.ok_or_else(|| {
                data_decode_error(
                    operation,
                    "inner join kernel returned an absent right row",
                )
            })?;
            Ok((left_rows[index.left], right_rows[right_index]))
        })
        .collect::<Result<Vec<_>, DataError>>();
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    Concurrency::with_runtime_mut(|rt| {
        let mut out = Vec::with_capacity(rows.len());
        for (left_row, right_row) in rows {
            let h = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(h, 0, left_row);
            let _ = rt.heap.record_set_int(h, 1, right_row);
            out.push(h);
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

fn jet_jit_data_left_join(
    left: i64,
    right: i64,
    left_key_callback: i64,
    right_key_callback: i64,
) -> i64 {
    let operation = "left_join";
    let left_rows = match data_row_words(left, operation) {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    let right_rows = match data_row_words(right, operation) {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    for callback in [left_key_callback, right_key_callback] {
        if let Err(error) = validate_data_callback(callback, operation) {
            return result_data_err(error);
        }
    }
    let left_keys = match left_rows
        .iter()
        .copied()
        .map(|row| data_callback_key(left_key_callback, row, operation))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(keys) => keys,
        Err(error) => return result_data_err(error),
    };
    let right_keys = match right_rows
        .iter()
        .copied()
        .map(|row| data_callback_key(right_key_callback, row, operation))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(keys) => keys,
        Err(error) => return result_data_err(error),
    };
    let indices = match data_kernel::jet_data_join_indices(
        &left_keys,
        &right_keys,
        true,
        &data_kernel_limits(),
    ) {
        Ok(indices) => indices,
        Err(error) => return result_data_err(error),
    };
    let rows = indices
        .into_iter()
        .map(|index| Ok((left_rows[index.left], index.right.map(|right_index| right_rows[right_index]))))
        .collect::<Result<Vec<_>, DataError>>();
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    Concurrency::with_runtime_mut(|rt| {
        let mut out = Vec::with_capacity(rows.len());
        for (left_row, right_row) in rows {
            let h = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(h, 0, left_row);
            let right = right_row
                .map(|value| crate::runtime_host::alloc_jit_result(rt, true, value as u64))
                .unwrap_or_else(|| crate::runtime_host::alloc_jit_result(rt, false, 0));
            let _ = rt.heap.record_set_int(h, 1, right);
            out.push(h);
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

fn jet_jit_data_pivot_sum(
    rows: i64,
    row_key_callback: i64,
    col_key_callback: i64,
    value_callback: i64,
) -> i64 {
    let operation = "pivot_sum";
    let rows = match data_row_words(rows, operation) {
        Ok(rows) => rows,
        Err(error) => return result_data_err(error),
    };
    for callback in [row_key_callback, col_key_callback, value_callback] {
        if let Err(error) = validate_data_callback(callback, operation) {
            return result_data_err(error);
        }
    }
    let mut values = Vec::with_capacity(rows.len());
    for row in rows.iter().copied() {
        let row_key = match data_callback_key(row_key_callback, row, operation) {
            Ok(key) => key,
            Err(error) => return result_data_err(error),
        };
        let column_key = match data_callback_key(col_key_callback, row, operation) {
            Ok(key) => key,
            Err(error) => return result_data_err(error),
        };
        let value = match data_callback_float(value_callback, row, operation) {
            Ok(value) => value,
            Err(error) => return result_data_err(error),
        };
        values.push((row_key, column_key, value));
    }
    let cells = match data_kernel::jet_data_pivot_sum_values(&values, &data_kernel_limits()) {
        Ok(cells) => cells,
        Err(error) => return result_data_err(error),
    };
    Concurrency::with_runtime_mut(|rt| {
        let mut out = Vec::with_capacity(cells.len());
        for cell in cells {
            let h = rt.heap.alloc_record(5);
            let row_key = rt.heap.alloc_string(cell.row_key);
            let column_key = rt.heap.alloc_string(cell.column_key);
            let _ = rt.heap.record_set_string(h, 0, row_key);
            let _ = rt.heap.record_set_string(h, 1, column_key);
            let _ = rt.heap.record_set_int(h, 2, cell.count);
            let _ = rt.heap.record_set_float(h, 3, cell.sum);
            let _ = rt.heap.record_set_float(h, 4, cell.mean);
            out.push(h);
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}
fn jet_jit_data_rolling_mean(values: i64, width: i64) -> i64 {
    let vals = float_list(values);
    let out = match data_kernel::jet_data_rolling_mean_checked(&vals, width) {
        Ok(out) => out,
        Err(error) => return result_data_err(error),
    };
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for v in out {
            let _ = rt.heap.list_push_float(list, v);
        }
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}


/// Resident owner for a typed `DataLoader<T>`.  The heap carrier is only the
/// checked public projection; lifecycle state and the row type remain here so
/// mutable calls cannot manufacture a second cache or lifetime registry.
#[derive(Clone)]
pub(crate) struct DataLoaderSlot {
    pub(crate) state: jet_foundation::PreludeDataFlow::LoaderState,
    pub(crate) type_key: String,
}

type LoaderTree = crate::Encoding::json_rt::DataTree;
type LoaderFormat = crate::Encoding::data_query_rt::jet_std::DataFormat;

fn loader_error(error: jet_foundation::PreludeDataFlow::KernelError) -> DataError {
    let kind = match error.kind {
        jet_foundation::PreludeDataFlow::ErrorKind::Limit => DataErrorKind::Limit,
        jet_foundation::PreludeDataFlow::ErrorKind::State => DataErrorKind::State,
        jet_foundation::PreludeDataFlow::ErrorKind::Bridge => DataErrorKind::Bridge,
        jet_foundation::PreludeDataFlow::ErrorKind::InvalidArgument => {
            DataErrorKind::InvalidArgument
        }
    };
    err(kind, &error.operation, error.reason)
}

fn loader_string(rt: &crate::JitRuntime, handle: i64, operation: &str) -> Result<String, DataError> {
    rt.heap
        .clone_string(handle)
        .ok_or_else(|| data_decode_error(operation, "expected a String handle"))
}

fn loader_string_list(
    rt: &mut crate::JitRuntime,
    handle: i64,
    operation: &str,
) -> Result<Vec<String>, DataError> {
    let Some(length) = rt.heap.list_len(handle) else {
        return Err(data_decode_error(operation, "expected a String list"));
    };
    (0..length)
        .map(|index| {
            rt.heap
                .list_get_string(handle, index)
                .ok_or_else(|| data_decode_error(operation, "String list contains an invalid item"))
        })
        .collect()
}

fn loader_type_key(
    rt: &crate::JitRuntime,
    handle: i64,
    operation: &str,
) -> Result<String, DataError> {
    loader_string(rt, handle, operation)
}

fn loader_descriptor(
    rt: &crate::JitRuntime,
    key: &str,
) -> Option<crate::runtime_host::RuntimeTypeDescriptor> {
    key.strip_prefix("id:")
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|id| rt.runtime_type_descriptor(id).cloned())
        .or_else(|| rt.runtime_type_descriptor_by_name(key).cloned())
}

fn loader_limits(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<jet_foundation::PreludeDataFlow::Limits, DataError> {
    let encoding = rt
        .heap
        .record_get_record(record, 0)
        .or_else(|| rt.heap.record_get_int(record, 0))
        .ok_or_else(|| data_decode_error(operation, "DataLimits has no EncodingLimits"))?;
    let get = |index| {
        rt.heap
            .record_get_int(encoding, index)
            .ok_or_else(|| data_decode_error(operation, "EncodingLimits has an invalid field"))
    };
    let max_total_handle = rt
        .heap
        .record_get_int(encoding, 3)
        .ok_or_else(|| data_decode_error(operation, "EncodingLimits max_total_bytes is invalid"))?;
    let max_total_bytes = crate::runtime_host::jit_result_parts(rt, max_total_handle)
        .ok_or_else(|| data_decode_error(operation, "EncodingLimits max_total_bytes is invalid"))?
        .0
        .then(|| crate::runtime_host::jit_result_i64(rt, max_total_handle).unwrap_or(0));
    let limits = jet_foundation::PreludeDataFlow::Limits {
        buffer_bytes: get(0)?,
        max_depth: get(1)?,
        max_item_bytes: get(2)?,
        max_total_bytes,
        max_expansion_depth: get(4)?,
        max_expansion_bytes: get(5)?,
        max_groups: rt
            .heap
            .record_get_int(record, 1)
            .ok_or_else(|| data_decode_error(operation, "DataLimits max_groups is invalid"))?,
        max_sort_rows: rt
            .heap
            .record_get_int(record, 2)
            .ok_or_else(|| data_decode_error(operation, "DataLimits max_sort_rows is invalid"))?,
        max_join_rows: rt
            .heap
            .record_get_int(record, 3)
            .ok_or_else(|| data_decode_error(operation, "DataLimits max_join_rows is invalid"))?,
        max_output_rows: rt
            .heap
            .record_get_int(record, 4)
            .ok_or_else(|| data_decode_error(operation, "DataLimits max_output_rows is invalid"))?,
    };
    jet_foundation::PreludeDataFlow::validate_limits(&limits).map_err(loader_error)?;
    Ok(limits)
}

fn loader_kind_disc(kind: jet_foundation::PreludeDataFlow::LoaderKind) -> i64 {
    match kind {
        jet_foundation::PreludeDataFlow::LoaderKind::File => 0,
        jet_foundation::PreludeDataFlow::LoaderKind::Url => 1,
        jet_foundation::PreludeDataFlow::LoaderKind::Database => 2,
        jet_foundation::PreludeDataFlow::LoaderKind::Value => 3,
    }
}

fn loader_format_disc(format: &str) -> i64 {
    match format {
        "csv" => 0,
        "json" => 1,
        "jsonl" => 2,
        "parquet" => 3,
        "arrow" => 4,
        _ => 0,
    }
}

fn loader_freshness_disc(
    freshness: jet_foundation::PreludeDataFlow::Freshness,
) -> i64 {
    match freshness {
        jet_foundation::PreludeDataFlow::Freshness::Pending => 0,
        jet_foundation::PreludeDataFlow::Freshness::Fresh => 1,
        jet_foundation::PreludeDataFlow::Freshness::Stale => 2,
        jet_foundation::PreludeDataFlow::Freshness::Error => 3,
        jet_foundation::PreludeDataFlow::Freshness::Offline => 4,
        jet_foundation::PreludeDataFlow::Freshness::Cancelled => 5,
    }
}

fn loader_invalidation_disc(
    cause: jet_foundation::PreludeDataFlow::InvalidationCause,
) -> i64 {
    match cause {
        jet_foundation::PreludeDataFlow::InvalidationCause::None => 0,
        jet_foundation::PreludeDataFlow::InvalidationCause::Loader => 1,
        jet_foundation::PreludeDataFlow::InvalidationCause::Input => 2,
        jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember => 3,
        jet_foundation::PreludeDataFlow::InvalidationCause::Parameters => 4,
        jet_foundation::PreludeDataFlow::InvalidationCause::Credential => 5,
        jet_foundation::PreludeDataFlow::InvalidationCause::Capability => 6,
        jet_foundation::PreludeDataFlow::InvalidationCause::Manual => 7,
    }
}

fn loader_enum(rt: &mut crate::JitRuntime, discriminant: i64) -> i64 {
    let value = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(value, 0, discriminant);
    value
}

fn loader_source_carrier(
    rt: &mut crate::JitRuntime,
    source: &jet_foundation::PreludeDataFlow::SourceIdentity,
) -> i64 {
    let value = rt.heap.alloc_record(4);
    let kind = loader_enum(rt, loader_kind_disc(source.kind));
    let _ = rt.heap.record_set_record(value, 0, kind);
    let locator = rt.heap.alloc_string(source.locator.clone());
    let _ = rt.heap.record_set_string(value, 1, locator);
    let member = rt.heap.alloc_string(source.member.clone());
    let _ = rt.heap.record_set_string(value, 2, member);
    let parameter_ids = source
        .parameters
        .iter()
        .map(|value| rt.heap.alloc_string(value.clone()))
        .collect();
    let parameters = rt.heap.alloc_int_list(parameter_ids);
    let _ = rt.heap.record_set_int(value, 3, parameters);
    value
}

fn loader_authority_carrier(
    rt: &mut crate::JitRuntime,
    authority: &jet_foundation::PreludeDataFlow::Authority,
) -> i64 {
    let value = rt.heap.alloc_record(2);
    let scope = rt.heap.alloc_string(authority.scope.clone());
    let revision = rt.heap.alloc_string(authority.revision.clone());
    let _ = rt.heap.record_set_string(value, 0, scope);
    let _ = rt.heap.record_set_string(value, 1, revision);
    value
}

fn loader_limits_carrier(
    rt: &mut crate::JitRuntime,
    limits: &jet_foundation::PreludeDataFlow::Limits,
) -> i64 {
    let encoding = rt.heap.alloc_record(6);
    let _ = rt.heap.record_set_int(encoding, 0, limits.buffer_bytes);
    let _ = rt.heap.record_set_int(encoding, 1, limits.max_depth);
    let _ = rt.heap.record_set_int(encoding, 2, limits.max_item_bytes);
    let max_total = match limits.max_total_bytes {
        Some(value) => crate::runtime_host::alloc_jit_result(rt, true, value as u64),
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
    };
    let _ = rt.heap.record_set_int(encoding, 3, max_total);
    let _ = rt.heap.record_set_int(encoding, 4, limits.max_expansion_depth);
    let _ = rt.heap.record_set_int(encoding, 5, limits.max_expansion_bytes);
    let value = rt.heap.alloc_record(5);
    let _ = rt.heap.record_set_record(value, 0, encoding);
    let _ = rt.heap.record_set_int(value, 1, limits.max_groups);
    let _ = rt.heap.record_set_int(value, 2, limits.max_sort_rows);
    let _ = rt.heap.record_set_int(value, 3, limits.max_join_rows);
    let _ = rt.heap.record_set_int(value, 4, limits.max_output_rows);
    value
}

fn loader_status_carrier(
    rt: &mut crate::JitRuntime,
    status: &jet_foundation::PreludeDataFlow::Status,
) -> i64 {
    let value = rt.heap.alloc_record(8);
    let identity = rt.heap.alloc_string(status.identity.clone());
    let error = rt.heap.alloc_string(status.error.clone());
    let cleanup = rt.heap.alloc_string(status.cleanup.clone());
    let _ = rt.heap.record_set_string(value, 0, identity);
    let freshness = loader_enum(rt, loader_freshness_disc(status.freshness));
    let invalidation = loader_enum(rt, loader_invalidation_disc(status.invalidated_by));
    let _ = rt.heap.record_set_record(value, 1, freshness);
    let _ = rt.heap.record_set_record(value, 2, invalidation);
    let _ = rt.heap.record_set_string(value, 3, error);
    let _ = rt.heap.record_set_string(value, 4, cleanup);
    let _ = rt.heap.record_set_int(value, 5, status.buffered_bytes);
    let _ = rt.heap.record_set_bool(value, 6, status.backpressure);
    let _ = rt.heap.record_set_bool(value, 7, status.last_good);
    value
}

fn loader_bytes_carrier(rt: &mut crate::JitRuntime, bytes: Option<&[u8]>) -> i64 {
    let list = match bytes {
        Some(bytes) => rt
            .heap
            .alloc_int_list(bytes.iter().map(|value| i64::from(*value)).collect()),
        None => rt.heap.alloc_int_list(Vec::new()),
    };
    crate::runtime_host::alloc_jit_result(rt, bytes.is_some(), list as u64)
}

fn fill_loader_carrier(
    rt: &mut crate::JitRuntime,
    carrier: i64,
    slot: usize,
    state: &jet_foundation::PreludeDataFlow::LoaderState,
) {
    let source = loader_source_carrier(rt, &state.source);
    let authority = loader_authority_carrier(rt, &state.authority);
    let limits = loader_limits_carrier(rt, &state.limits);
    let status = loader_status_carrier(rt, &state.status);
    let format = loader_enum(rt, loader_format_disc(&state.format));
    let payload = loader_bytes_carrier(rt, state.payload.as_deref());
    let last_good = loader_bytes_carrier(rt, state.last_good.as_deref());
    let raw_locator = rt.heap.alloc_string(state.raw_locator.clone());
    let _ = rt.heap.record_set_record(carrier, 0, source);
    let _ = rt.heap.record_set_record(carrier, 1, format);
    let _ = rt.heap.record_set_record(carrier, 2, authority);
    let _ = rt.heap.record_set_record(carrier, 3, limits);
    let _ = rt.heap.record_set_int(carrier, 4, payload);
    let _ = rt.heap.record_set_int(carrier, 5, last_good);
    let _ = rt.heap.record_set_bool(carrier, 6, state.cancelled);
    let _ = rt.heap.record_set_bool(carrier, 7, state.offline);
    let _ = rt.heap.record_set_record(carrier, 8, status);
    let _ = rt.heap.record_set_string(carrier, 9, raw_locator);
    let _ = rt.heap.record_set_int(carrier, 10, (slot.saturating_add(1)) as i64);
}

fn loader_slot_index(
    rt: &crate::JitRuntime,
    carrier: i64,
    operation: &str,
) -> Result<usize, DataError> {
    let slot = rt
        .heap
        .record_get_int(carrier, 10)
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| data_error(operation, "invalid DataLoader carrier"))?;
    if rt.data_loaders.get(slot).is_none() {
        return Err(data_error(operation, "unknown DataLoader owner"));
    }
    Ok(slot)
}

fn loader_place_handle(
    rt: &crate::JitRuntime,
    place: i64,
    operation: &str,
) -> Result<i64, DataError> {
    if place == 0 {
        return Err(data_error(operation, "null mutable DataLoader place"));
    }
    // SAFETY: mutable Core arguments are addresses of resident JIT stack
    // slots produced by `address_of`; the caller never supplies this ABI by
    // hand and the load is used only for the duration of the host call.
    let carrier = unsafe { (place as *const i64).read() };
    if carrier == 0 {
        return Err(data_error(operation, "mutable DataLoader place is empty"));
    }
    Ok(carrier)
}

fn loader_format(format: &str, operation: &str) -> Result<LoaderFormat, DataError> {
    LoaderFormat::from_str(format)
        .ok_or_else(|| err(DataErrorKind::Bridge, operation, format!("format `{format}` is provider-owned")))
}

fn loader_format_for_locator(locator: &str) -> Result<LoaderFormat, DataError> {
    let path = locator
        .split(|character| character == '?' || character == '#')
        .next()
        .unwrap_or(locator);
    let path = path.rsplit_once('/').map_or(path, |(_, value)| value);
    let extension = path
        .rsplit_once('.')
        .map(|(_, value)| value)
        .ok_or_else(|| err(DataErrorKind::Bridge, "data.load", "source has no core format"))?;
    loader_format(extension, "data.load")
}

fn loader_new(
    rt: &mut crate::JitRuntime,
    kind: jet_foundation::PreludeDataFlow::LoaderKind,
    locator: String,
    member: String,
    parameters: Vec<String>,
    format: LoaderFormat,
    limits: jet_foundation::PreludeDataFlow::Limits,
    authority: jet_foundation::PreludeDataFlow::Authority,
    type_key: String,
) -> Result<i64, DataError> {
    let state = jet_foundation::PreludeDataFlow::new_loader(
        kind,
        locator,
        member,
        parameters,
        format.as_str().to_string(),
        limits,
        authority,
    )
    .map_err(loader_error)?;
    let slot = rt.data_loaders.len();
    rt.data_loaders.push(DataLoaderSlot {
        state,
        type_key,
    });
    let carrier = rt.heap.alloc_record(11);
    let state = rt.data_loaders[slot].state.clone();
    fill_loader_carrier(rt, carrier, slot, &state);
    Ok(carrier)
}

fn loader_payload(
    state: &jet_foundation::PreludeDataFlow::LoaderState,
) -> Result<(Vec<u8>, bool), DataError> {
    if let Some(payload) =
        jet_foundation::PreludeDataFlow::payload(state).map_err(loader_error)?
    {
        return Ok(payload);
    }
    if state.source.kind == jet_foundation::PreludeDataFlow::LoaderKind::File
        && state.source.member.is_empty()
    {
        let bytes = std::fs::read(&state.raw_locator)
            .map_err(|error| err(DataErrorKind::IO, "data.loader", error.to_string()))?;
        jet_foundation::PreludeDataFlow::check_payload(&state.limits, bytes.len(), "data.loader")
            .map_err(loader_error)?;
        return Ok((bytes, false));
    }
    Err(err(
        DataErrorKind::Bridge,
        "data.loader",
        "provider payload is not bound",
    ))
}

fn loader_tree(
    payload: &[u8],
    format: LoaderFormat,
    limits: &jet_foundation::PreludeDataFlow::Limits,
) -> Result<LoaderTree, DataError> {
    let text = || {
        std::str::from_utf8(payload)
            .map_err(|error| err(DataErrorKind::Decode, "data.loader", error.to_string()))
    };
    let tree = match format {
        LoaderFormat::JSON => crate::Encoding::json_rt::parse_datatree_typed_ordered(text()?)
            .map_err(|error| err(DataErrorKind::Decode, "data.loader.json", error.to_string()))?,
        LoaderFormat::JSONL => {
            let mut rows = Vec::new();
            for (line, value) in text()?.lines().enumerate() {
                if value.trim().is_empty() {
                    continue;
                }
                let row = crate::Encoding::json_rt::parse_datatree_typed_ordered(value)
                    .map_err(|error| {
                        err(
                            DataErrorKind::Decode,
                            "data.loader.jsonl",
                            format!("line {}: {}", line + 1, error),
                        )
                    })?;
                rows.push(row);
            }
            LoaderTree::Array(rows)
        }
        LoaderFormat::CSV => {
            let records = crate::Encoding::csv_parse(text()?, ",", false, false)
                .map_err(|reason| err(DataErrorKind::Decode, "data.loader.csv", reason))?;
            let mut records = records.into_iter();
            let Some(header) = records.next() else {
                return Ok(LoaderTree::Array(Vec::new()));
            };
            let mut header_names = std::collections::BTreeSet::new();
            for name in &header.fields {
                jet_foundation::PreludeDataFlow::validate_public_text("CSV header", name, false)
                    .map_err(loader_error)?;
                if !header_names.insert(name.clone()) {
                    return Err(err(
                        DataErrorKind::Decode,
                        "data.loader.csv",
                        format!("duplicate CSV header `{name}`"),
                    ));
                }
            }
            let headers = header.fields;
            let mut rows = Vec::new();
            for record in records {
                if rows.len() as i64 >= limits.max_output_rows {
                    return Err(err(
                        DataErrorKind::Limit,
                        "data.loader.csv",
                        format!("max_output_rows {} exceeded", limits.max_output_rows),
                    ));
                }
                if record.fields.len() > headers.len() {
                    return Err(err(
                        DataErrorKind::Decode,
                        "data.loader.csv",
                        format!(
                            "CSV row has {} fields; header has {}",
                            record.fields.len(),
                            headers.len()
                        ),
                    ));
                }
                rows.push(LoaderTree::Object(
                    headers
                        .iter()
                        .enumerate()
                        .map(|(index, name)| {
                            (
                                name.clone(),
                                record
                                    .fields
                                    .get(index)
                                    .cloned()
                                    .map(LoaderTree::Text)
                                    .unwrap_or(LoaderTree::Null),
                            )
                        })
                        .collect(),
                ));
            }
            LoaderTree::Array(rows)
        }
        LoaderFormat::Parquet => {
            let max_total = limits
                .max_total_bytes
                .unwrap_or_else(|| limits.max_item_bytes.max(limits.max_expansion_bytes));
            let arrow_limits = jet_foundation::ArrowData::ArrowLimits {
                max_rows: limits.max_output_rows,
                max_columns: 1024,
                max_children: limits.max_depth,
                max_dictionary_values: limits.max_output_rows,
                max_buffer_bytes: usize::try_from(limits.max_expansion_bytes).unwrap_or(usize::MAX),
                max_total_bytes: usize::try_from(max_total).unwrap_or(usize::MAX),
            };
            jet_foundation::ArrowFileReader::read_tree(
                jet_foundation::ArrowFileReader::FORMAT_PARQUET,
                payload,
                &arrow_limits,
            )
            .map_err(|error| err(DataErrorKind::Decode, "data.loader.parquet", error.to_string()))?
        }
        LoaderFormat::Arrow => {
            return Err(err(
                DataErrorKind::Bridge,
                "data.loader.arrow",
                "Arrow IPC requires a distinct provider",
            ))
        }
    };
    if let LoaderTree::Array(rows) = &tree {
        if rows.len() as i64 > limits.max_output_rows {
            return Err(err(
                DataErrorKind::Limit,
                "data.loader",
                format!("max_output_rows {} exceeded", limits.max_output_rows),
            ));
        }
    }
    Ok(tree)
}
fn loader_safe_limits() -> jet_foundation::PreludeDataFlow::Limits {
    jet_foundation::PreludeDataFlow::Limits {
        buffer_bytes: 65_536,
        max_depth: 256,
        max_item_bytes: 16_777_216,
        max_total_bytes: None,
        max_expansion_depth: 32,
        max_expansion_bytes: 8_388_608,
        max_groups: 100_000,
        max_sort_rows: 1_000_000,
        max_join_rows: 1_000_000,
        max_output_rows: 1_000_000,
    }
}

fn loader_rows(tree: LoaderTree) -> Vec<LoaderTree> {
    match tree {
        LoaderTree::Array(rows) => rows,
        value => vec![value],
    }
}

fn loader_decode_error(
    operation: &str,
    errors: Vec<crate::Encoding::json_rt::FieldError>,
    row: Option<i64>,
) -> DataError {
    let mut error = err(
        DataErrorKind::Decode,
        operation,
        errors
            .into_iter()
            .map(|value| {
                if value.path.is_empty() {
                    value.reason
                } else {
                    format!("{}: {}", value.path, value.reason)
                }
            })
            .collect::<Vec<_>>()
            .join("; "),
    );
    error.row = row.ok_or(JetAbsent);
    error
}

fn loader_decode_rows(
    tree: LoaderTree,
    type_key: &str,
    operation: &str,
) -> Result<Vec<i64>, DataError> {
    loader_rows(tree)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            crate::Encoding::decode_datatree_for_type(&row, type_key)
                .map_err(|errors| loader_decode_error(operation, errors, Some(index as i64)))
        })
        .collect()
}

fn loader_snapshot_carrier(
    rt: &mut crate::JitRuntime,
    value: i64,
    state: &jet_foundation::PreludeDataFlow::LoaderState,
    facts: &jet_foundation::PreludeDataFlow::SnapshotFacts,
    schema: &crate::Encoding::data_query_rt::jet_std::DataSchema,
    canonical: &[u8],
) -> i64 {
    let identity = rt.heap.alloc_record(5);
    let id = rt
        .heap
        .alloc_string(format!("snapshot-{}", facts.snapshot_id));
    let source = rt.heap.alloc_string(format!("source-{}", facts.source_id));
    let content = rt.heap.alloc_string(facts.content_id.clone());
    let schema_id = rt.heap.alloc_string(facts.schema_id.clone());
    let identity_format = loader_enum(rt, loader_format_disc(schema.format.as_str()));
    let _ = rt.heap.record_set_string(identity, 0, id);
    let _ = rt.heap.record_set_string(identity, 1, source);
    let _ = rt.heap.record_set_string(identity, 2, content);
    let _ = rt.heap.record_set_string(identity, 3, schema_id);
    let _ = rt.heap.record_set_record(identity, 4, identity_format);

    let provenance = rt.heap.alloc_record(3);
    let provenance_source = loader_source_carrier(rt, &state.source);
    let provenance_format = loader_enum(rt, loader_format_disc(schema.format.as_str()));
    let provenance_authority = loader_authority_carrier(rt, &state.authority);
    let _ = rt.heap.record_set_record(provenance, 0, provenance_source);
    let _ = rt.heap.record_set_record(provenance, 1, provenance_format);
    let _ = rt.heap.record_set_record(provenance, 2, provenance_authority);

    let mut column_ids = Vec::with_capacity(schema.columns.len());
    for column in &schema.columns {
        let field = rt.heap.alloc_record(4);
        let id = rt.heap.alloc_string(column.id.clone());
        let name = rt.heap.alloc_string(column.name.clone());
        let ty = rt.heap.alloc_string(column.type_name.clone());
        let _ = rt.heap.record_set_string(field, 0, id);
        let _ = rt.heap.record_set_string(field, 1, name);
        let _ = rt.heap.record_set_string(field, 2, ty);
        let _ = rt.heap.record_set_bool(field, 3, column.nullable);
        column_ids.push(field);
    }
    let columns = rt.heap.alloc_int_list(column_ids);
    let schema_carrier = rt.heap.alloc_record(4);
    let schema_identity = rt.heap.alloc_string(schema.identity.clone());
    let schema_format = loader_enum(rt, loader_format_disc(schema.format.as_str()));
    let projection = crate::runtime_host::alloc_jit_result(rt, false, 0);
    let _ = rt.heap.record_set_string(schema_carrier, 0, schema_identity);
    let _ = rt.heap.record_set_record(schema_carrier, 1, schema_format);
    let _ = rt.heap.record_set_int(schema_carrier, 2, columns);
    let _ = rt.heap.record_set_int(schema_carrier, 3, projection);

    let status = loader_status_carrier(rt, &facts.status);
    let content = rt
        .heap
        .alloc_int_list(canonical.iter().map(|value| i64::from(*value)).collect());
    let snapshot = rt.heap.alloc_record(6);
    let _ = rt.heap.record_set_int(snapshot, 0, value);
    let _ = rt.heap.record_set_record(snapshot, 1, identity);
    let _ = rt.heap.record_set_record(snapshot, 2, provenance);
    let _ = rt.heap.record_set_record(snapshot, 3, schema_carrier);
    let _ = rt.heap.record_set_record(snapshot, 4, status);
    let _ = rt.heap.record_set_int(snapshot, 5, content);
    snapshot
}

fn loader_result(value: Result<i64, DataError>) -> i64 {
    match value {
        Ok(value) => Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, value as u64)
        }),
        Err(error) => result_data_err(error),
    }
}

fn loader_make(
    kind: jet_foundation::PreludeDataFlow::LoaderKind,
    locator: i64,
    member: Option<i64>,
    format: i64,
    parameters: Option<i64>,
    authority: Option<i64>,
    limits: i64,
    type_key: i64,
) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.loader", "no resident runtime"),
        |rt| {
            let locator = loader_string(rt, locator, "data.loader")?;
            let member = member
                .map(|value| loader_string(rt, value, "data.loader"))
                .transpose()?
                .unwrap_or_default();
            let format = if kind == jet_foundation::PreludeDataFlow::LoaderKind::Database {
                loader_format("json", "data.loader")?
            } else {
                let format = loader_string(rt, format, "data.loader")?;
                loader_format(&format, "data.loader")?
            };
            let parameters = parameters
                .map(|value| loader_string_list(rt, value, "data.loader"))
                .transpose()?
                .unwrap_or_default();
            let authority = authority
                .map(|value| loader_string(rt, value, "data.loader"))
                .transpose()?
                .unwrap_or_else(|| if kind == jet_foundation::PreludeDataFlow::LoaderKind::Url {
                    "network".to_string()
                } else {
                    "local".to_string()
                });
            let authority = jet_foundation::PreludeDataFlow::authority(&authority, "")
                .map_err(loader_error)?;
            let limits = loader_limits(rt, limits, "data.loader")?;
            let type_key = loader_type_key(rt, type_key, "data.loader")?;
            loader_new(rt, kind, locator, member, parameters, format, limits, authority, type_key)
        },
    );
    loader_result(result)
}

fn jet_data_loader_load(locator: i64, limits: i64, type_key: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.load", "no resident runtime"),
        |rt| {
            let locator_text = loader_string(rt, locator, "data.load")?;
            let format = loader_format_for_locator(&locator_text)?;
            let kind = if locator_text.starts_with("http://") || locator_text.starts_with("https://") {
                jet_foundation::PreludeDataFlow::LoaderKind::Url
            } else {
                jet_foundation::PreludeDataFlow::LoaderKind::File
            };
            let limits = loader_limits(rt, limits, "data.load")?;
            let type_key = loader_type_key(rt, type_key, "data.load")?;
            let authority = if kind == jet_foundation::PreludeDataFlow::LoaderKind::Url {
                "network"
            } else {
                "local"
            };
            let authority = jet_foundation::PreludeDataFlow::authority(authority, "")
                .map_err(loader_error)?;
            loader_new(
                rt,
                kind,
                locator_text,
                String::new(),
                Vec::new(),
                format,
                limits,
                authority,
                type_key,
            )
        },
    );
    loader_result(result)
}

fn jet_data_loader_load_default(locator: i64, type_key: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.load_default", "no resident runtime"),
        |rt| {
            let locator_text = loader_string(rt, locator, "data.load_default")?;
            let format = loader_format_for_locator(&locator_text)?;
            let limits = loader_safe_limits();
            let type_key = loader_type_key(rt, type_key, "data.load_default")?;
            let kind = if locator_text.starts_with("http://")
                || locator_text.starts_with("https://")
            {
                jet_foundation::PreludeDataFlow::LoaderKind::Url
            } else {
                jet_foundation::PreludeDataFlow::LoaderKind::File
            };
            let authority = if kind == jet_foundation::PreludeDataFlow::LoaderKind::Url {
                "network"
            } else {
                "local"
            };
            let authority = jet_foundation::PreludeDataFlow::authority(authority, "")
                .map_err(loader_error)?;
            loader_new(
                rt,
                kind,
                locator_text,
                String::new(),
                Vec::new(),
                format,
                limits,
                authority,
                type_key,
            )
        },
    );
    loader_result(result)
}

fn jet_data_loader_file(path: i64, format: i64, limits: i64, type_key: i64) -> i64 {
    jet_data_loader_file_member(path, 0, format, limits, type_key)
}

fn jet_data_loader_file_member(
    path: i64,
    member: i64,
    format: i64,
    limits: i64,
    type_key: i64,
) -> i64 {
    loader_make(
        jet_foundation::PreludeDataFlow::LoaderKind::File,
        path,
        (member != 0).then_some(member),
        format,
        None,
        None,
        limits,
        type_key,
    )
}

fn jet_data_loader_url(url: i64, format: i64, authority: i64, limits: i64, type_key: i64) -> i64 {
    loader_make(
        jet_foundation::PreludeDataFlow::LoaderKind::Url,
        url,
        None,
        format,
        None,
        Some(authority),
        limits,
        type_key,
    )
}

fn jet_data_loader_database(
    query: i64,
    parameters: i64,
    authority: i64,
    limits: i64,
    type_key: i64,
) -> i64 {
    loader_make(
        jet_foundation::PreludeDataFlow::LoaderKind::Database,
        query,
        None,
        0,
        Some(parameters),
        Some(authority),
        limits,
        type_key,
    )
}

fn jet_data_loader_value(value: i64, limits: i64, type_key: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.value", "no resident runtime"),
        |rt| {
            let limits = loader_limits(rt, limits, "data.value")?;
            let type_key = loader_type_key(rt, type_key, "data.value")?;
            let descriptor = loader_descriptor(rt, &type_key)
                .ok_or_else(|| data_error("data.value", "unknown checked value type"))?;
            let tree = crate::Receipt::encode_jit_value(rt, value, &descriptor)
                .map_err(|reason| data_decode_error("data.value", reason))?;
            let format = loader_format("json", "data.value")?;
            let authority = jet_foundation::PreludeDataFlow::authority("local", "")
                .map_err(loader_error)?;
            let mut state = jet_foundation::PreludeDataFlow::new_loader(
                jet_foundation::PreludeDataFlow::LoaderKind::Value,
                "value".to_string(),
                String::new(),
                Vec::new(),
                format.as_str().to_string(),
                limits,
                authority,
            )
            .map_err(loader_error)?;
            let bytes = crate::Encoding::json_rt::render_datatree_json(&tree, false, 0).into_bytes();
            jet_foundation::PreludeDataFlow::check_payload(&state.limits, bytes.len(), "data.value")
                .map_err(loader_error)?;
            state.payload = Some(bytes);
            let slot = rt.data_loaders.len();
            rt.data_loaders.push(DataLoaderSlot { state, type_key });
            let carrier = rt.heap.alloc_record(11);
            let state = rt.data_loaders[slot].state.clone();
            fill_loader_carrier(rt, carrier, slot, &state);
            Ok(carrier)
        },
    );
    loader_result(result)
}

fn jet_data_loader_snapshot(place: i64, type_key: i64) -> i64 {
    let extracted = Concurrency::with_runtime_result(
        data_error("data.snapshot", "no resident runtime"),
        |rt| {
            let carrier = loader_place_handle(rt, place, "data.snapshot")?;
            let slot = loader_slot_index(rt, carrier, "data.snapshot")?;
            let slot = rt.data_loaders[slot].clone();
            let metadata = loader_type_key(rt, type_key, "data.snapshot")?;
            if metadata != slot.type_key {
                return Err(data_error("data.snapshot", "DataLoader type metadata does not match owner"));
            }
            if slot.state.cancelled {
                return Err(err(
                    DataErrorKind::State,
                    "data.snapshot",
                    "loader was cancelled before snapshot",
                ));
            }
            Ok((carrier, slot))
        },
    );
    let (carrier, mut slot) = match extracted {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let (payload, offline) = match loader_payload(&slot.state) {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let format = match loader_format(&slot.state.format, "data.snapshot") {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let tree = match loader_tree(&payload, format, &slot.state.limits) {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let value = match crate::Encoding::decode_datatree_for_type(&tree, &slot.type_key) {
        Ok(value) => value,
        Err(errors) => return result_data_err(loader_decode_error("data.snapshot", errors, None)),
    };
    let result = Concurrency::with_runtime_result(
        data_error("data.snapshot", "no resident runtime"),
        |rt| {
            let descriptor = loader_descriptor(rt, &slot.type_key)
                .ok_or_else(|| data_error("data.snapshot", "unknown checked snapshot type"))?;
            let encoded = crate::Receipt::encode_jit_value(rt, value, &descriptor)
                .map_err(|reason| data_decode_error("data.snapshot", reason))?;
            let canonical = crate::Encoding::json_rt::render_datatree_json(&encoded, false, 0);
            let mut schema = crate::Encoding::data_query_rt::jet_std::DataSchema::infer(&tree, format);
            schema.identity = jet_foundation::PreludeDataFlow::digest(schema.identity.as_bytes());
            let facts = jet_foundation::PreludeDataFlow::commit_snapshot(
                &mut slot.state,
                payload,
                canonical.as_bytes(),
                &schema.identity,
                offline,
            )
            .map_err(loader_error)?;
            let slot_index = loader_slot_index(rt, carrier, "data.snapshot")?;
            rt.data_loaders[slot_index] = slot.clone();
            fill_loader_carrier(rt, carrier, slot_index, &slot.state);
            Ok(loader_snapshot_carrier(
                rt,
                value,
                &slot.state,
                &facts,
                &schema,
                canonical.as_bytes(),
            ))
        },
    );
    loader_result(result)
}

fn jet_data_loader_stream(place: i64, type_key: i64) -> i64 {
    let extracted = Concurrency::with_runtime_result(
        data_error("data.stream", "no resident runtime"),
        |rt| {
            let carrier = loader_place_handle(rt, place, "data.stream")?;
            let slot_index = loader_slot_index(rt, carrier, "data.stream")?;
            let slot = rt.data_loaders[slot_index].clone();
            let metadata = loader_type_key(rt, type_key, "data.stream")?;
            if metadata != slot.type_key {
                return Err(data_error("data.stream", "DataLoader type metadata does not match owner"));
            }
            if slot.state.cancelled {
                return Err(err(
                    DataErrorKind::State,
                    "data.stream",
                    "loader was cancelled",
                ));
            }
            Ok((carrier, slot_index, slot))
        },
    );
    let (carrier, slot_index, slot) = match extracted {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let (payload, _) = match loader_payload(&slot.state) {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let format = match loader_format(&slot.state.format, "data.stream") {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let tree = match loader_tree(&payload, format, &slot.state.limits) {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    let rows = match loader_decode_rows(tree, &slot.type_key, "data.stream") {
        Ok(value) => value,
        Err(error) => return result_data_err(error),
    };
    Concurrency::with_runtime_mut(|rt| {
        let stream_type = slot.type_key.clone();
        rt.data_streams.push(DataStreamSlot {
            rows,
            index: 0,
            max_groups: slot.state.limits.max_output_rows,
            type_key: stream_type,
            cancelled: false,
        });
        let stream_handle = rt.data_streams.len() as u64;
        let stream = crate::runtime_host::alloc_jit_result(rt, true, stream_handle);
        let state = &mut rt.data_loaders[slot_index].state;
        state.status.buffered_bytes = i64::try_from(payload.len()).unwrap_or(i64::MAX);
        state.status.backpressure = state.status.buffered_bytes > state.limits.buffer_bytes;
        let state = state.clone();
        fill_loader_carrier(rt, carrier, slot_index, &state);
        stream
    })
}
/// Typed pull stream for `core.data.csv_reader` and `core.data.stream`.
pub(crate) struct DataStreamSlot {
    rows: Vec<i64>,
    index: usize,
    max_groups: i64,
    type_key: String,
    cancelled: bool,
}
fn loader_stream_list(
    rt: &mut crate::JitRuntime,
    rows: &[i64],
    type_key: &str,
    operation: &str,
) -> Result<i64, DataError> {
    let descriptor = loader_descriptor(rt, type_key)
        .ok_or_else(|| data_decode_error(operation, "unknown stream element type"))?;
    let values = rows
        .iter()
        .copied()
        .map(|raw| match descriptor.abi {
            crate::runtime_host::RuntimeValueAbi::Float => {
                jet_rt::JetVal::Float(f64::from_bits(raw as u64))
            }
            crate::runtime_host::RuntimeValueAbi::Float32 => {
                jet_rt::JetVal::Float(f32::from_bits(raw as u32) as f64)
            }
            crate::runtime_host::RuntimeValueAbi::Bool => jet_rt::JetVal::Bool(raw != 0),
            crate::runtime_host::RuntimeValueAbi::Char => {
                jet_rt::JetVal::Char(char::from_u32(raw as u32).unwrap_or('\0'))
            }
            _ => jet_rt::JetVal::Int(raw),
        })
        .collect();
    Ok(rt.heap.alloc_list_values(values))
}

fn jet_data_stream_next(place: i64, type_key: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.stream.next", "no resident runtime"),
        |rt| {
            let type_key = loader_type_key(rt, type_key, "data.stream.next")?;
            let handle = loader_place_handle(rt, place, "data.stream.next")?;
            let index = plan_index(handle)
                .ok_or_else(|| data_error("data.stream.next", "invalid DataStream handle"))?;
            let (row, done) = {
                let stream = rt
                    .data_streams
                    .get_mut(index)
                    .ok_or_else(|| data_error("data.stream.next", "unknown DataStream handle"))?;
                if stream.type_key != type_key {
                    return Err(data_error(
                        "data.stream.next",
                        "DataStream type metadata does not match owner",
                    ));
                }
                if stream.cancelled {
                    return Err(err(
                        DataErrorKind::State,
                        "data.stream.next",
                        "stream was cancelled",
                    ));
                }
                if stream.index >= stream.rows.len() {
                    (0, true)
                } else {
                    let row = stream.rows[stream.index];
                    stream.index += 1;
                    (row, false)
                }
            };
            let option = if done {
                crate::runtime_host::alloc_jit_result(rt, false, 0)
            } else {
                crate::runtime_host::alloc_jit_result(rt, true, row as u64)
            };
            Ok(crate::runtime_host::alloc_jit_result(
                rt,
                true,
                option as u64,
            ))
        },
    );
    match result {
        Ok(value) => value,
        Err(error) => result_data_err(error),
    }
}
fn jet_data_stream_collect(place: i64, type_key: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.stream.collect", "no resident runtime"),
        |rt| {
            let type_key = loader_type_key(rt, type_key, "data.stream.collect")?;
            let handle = loader_place_handle(rt, place, "data.stream.collect")?;
            let index = plan_index(handle)
                .ok_or_else(|| data_error("data.stream.collect", "invalid DataStream handle"))?;
            let rows = {
                let stream = rt
                    .data_streams
                    .get_mut(index)
                    .ok_or_else(|| data_error("data.stream.collect", "unknown DataStream handle"))?;
                if stream.type_key != type_key {
                    return Err(data_error(
                        "data.stream.collect",
                        "DataStream type metadata does not match owner",
                    ));
                }
                if stream.cancelled {
                    return Err(err(
                        DataErrorKind::State,
                        "data.stream.collect",
                        "stream was cancelled",
                    ));
                }
                let remaining = stream.rows.len().saturating_sub(stream.index);
                if remaining as i64 > stream.max_groups {
                    return Err(err(
                        DataErrorKind::Limit,
                        "data.stream.collect",
                        format!("max_output_rows {} exceeded", stream.max_groups),
                    ));
                }
                let rows = stream.rows[stream.index..].to_vec();
                stream.index = stream.rows.len();
                rows
            };
            loader_stream_list(rt, &rows, &type_key, "data.stream.collect")
        },
    );
    match result {
        Ok(value) => Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, value as u64)
        }),
        Err(error) => result_data_err(error),
    }
}

fn jet_data_stream_cancel(place: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.stream.cancel", "no resident runtime"),
        |rt| {
            let handle = loader_place_handle(rt, place, "data.stream.cancel")?;
            let index = plan_index(handle)
                .ok_or_else(|| data_error("data.stream.cancel", "invalid DataStream handle"))?;
            let stream = rt
                .data_streams
                .get_mut(index)
                .ok_or_else(|| data_error("data.stream.cancel", "unknown DataStream handle"))?;
            stream.cancelled = true;
            Ok(())
        },
    );
    result_unit(result)
}

fn loader_mutate<F>(place: i64, operation: &str, mutate: F) -> Result<(), DataError>
where
    F: FnOnce(&mut jet_foundation::PreludeDataFlow::LoaderState) -> Result<(), DataError>,
{
    Concurrency::with_runtime_result(data_error(operation, "no resident runtime"), |rt| {
        let carrier = loader_place_handle(rt, place, operation)?;
        let index = loader_slot_index(rt, carrier, operation)?;
        {
            let state = &mut rt.data_loaders[index].state;
            mutate(state)?;
        }
        let state = rt.data_loaders[index].state.clone();
        fill_loader_carrier(rt, carrier, index, &state);
        Ok(())
    })
}

fn jet_data_loader_bind(place: i64, payload: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.loader.bind", "no resident runtime"),
        |rt| {
            let bytes = {
                let length = rt
                    .heap
                    .list_len(payload)
                    .ok_or_else(|| data_decode_error("data.loader.bind", "payload is not a byte list"))?;
                (0..length)
                    .map(|index| {
                        let value = rt.heap.list_get_int(payload, index).ok_or_else(|| {
                            data_decode_error("data.loader.bind", "payload has an invalid byte")
                        })?;
                        u8::try_from(value).map_err(|_| {
                            data_decode_error("data.loader.bind", "payload byte is out of range")
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            let carrier = loader_place_handle(rt, place, "data.loader.bind")?;
            let index = loader_slot_index(rt, carrier, "data.loader.bind")?;
            jet_foundation::PreludeDataFlow::bind(&mut rt.data_loaders[index].state, bytes)
                .map_err(loader_error)?;
            let state = rt.data_loaders[index].state.clone();
            fill_loader_carrier(rt, carrier, index, &state);
            Ok(())
        },
    );
    result_unit(result)
}

fn jet_data_loader_bind_text(place: i64, payload: i64) -> i64 {
    let result = Concurrency::with_runtime_result(
        data_error("data.loader.bind_text", "no resident runtime"),
        |rt| {
            let text = loader_string(rt, payload, "data.loader.bind_text")?;
            let carrier = loader_place_handle(rt, place, "data.loader.bind_text")?;
            let index = loader_slot_index(rt, carrier, "data.loader.bind_text")?;
            jet_foundation::PreludeDataFlow::bind(
                &mut rt.data_loaders[index].state,
                text.into_bytes(),
            )
            .map_err(loader_error)?;
            let state = rt.data_loaders[index].state.clone();
            fill_loader_carrier(rt, carrier, index, &state);
            Ok(())
        },
    );
    result_unit(result)
}

fn jet_data_loader_cancel(place: i64) -> i64 {
    let result = loader_mutate(place, "data.loader.cancel", |state| {
        jet_foundation::PreludeDataFlow::cancel(state);
        Ok(())
    });
    result_unit(result)
}

fn jet_data_loader_offline(place: i64, enabled: i64) -> i64 {
    let result = loader_mutate(place, "data.loader.offline", |state| {
        jet_foundation::PreludeDataFlow::set_offline(state, enabled != 0);
        Ok(())
    });
    result_unit(result)
}

fn jet_data_loader_invalidate(place: i64, cause: i64) -> i64 {
    let result = loader_mutate(place, "data.loader.invalidate", |state| {
        let cause = match cause {
            0 => jet_foundation::PreludeDataFlow::InvalidationCause::None,
            1 => jet_foundation::PreludeDataFlow::InvalidationCause::Loader,
            2 => jet_foundation::PreludeDataFlow::InvalidationCause::Input,
            3 => jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember,
            4 => jet_foundation::PreludeDataFlow::InvalidationCause::Parameters,
            5 => jet_foundation::PreludeDataFlow::InvalidationCause::Credential,
            6 => jet_foundation::PreludeDataFlow::InvalidationCause::Capability,
            7 => jet_foundation::PreludeDataFlow::InvalidationCause::Manual,
            _ => return Err(data_error("data.loader.invalidate", "invalid invalidation cause")),
        };
        jet_foundation::PreludeDataFlow::invalidate(state, cause);
        Ok(())
    });
    result_unit(result)
}

fn jet_data_loader_needs_refresh(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Ok(index) = loader_slot_index(rt, handle, "data.loader.needs_refresh") else {
            return 0;
        };
        i64::from(jet_foundation::PreludeDataFlow::needs_refresh(
            &rt.data_loaders[index].state,
        ))
    })
}

fn jet_data_loader_ready(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Ok(index) = loader_slot_index(rt, handle, "data.loader.ready") else {
            return 0;
        };
        i64::from(jet_foundation::PreludeDataFlow::ready(
            &rt.data_loaders[index].state,
        ))
    })
}

fn jet_data_loader_status(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Ok(index) = loader_slot_index(rt, handle, "data.loader.status") else {
            return rt.heap.alloc_record(8);
        };
        let status = jet_foundation::PreludeDataFlow::status(&rt.data_loaders[index].state);
        loader_status_carrier(rt, &status)
    })
}

fn jet_data_loader_source_identity(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Ok(index) = loader_slot_index(rt, handle, "data.loader.source_identity") else {
            return rt.heap.alloc_record(4);
        };
        let source = rt.data_loaders[index].state.source.clone();
        loader_source_carrier(rt, &source)
    })
}

fn jet_data_loader_authority_of(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Ok(index) = loader_slot_index(rt, handle, "data.loader.authority_of") else {
            return rt.heap.alloc_record(2);
        };
        let authority = rt.data_loaders[index].state.authority.clone();
        loader_authority_carrier(rt, &authority)
    })
}

fn jet_data_snapshot_reusable(previous: i64, current: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(previous_identity) = rt.heap.record_get_record(previous, 1) else {
            return 0;
        };
        let Some(current_identity) = rt.heap.record_get_record(current, 1) else {
            return 0;
        };
        let strings_equal = (0..4).all(|index| {
            rt.heap.record_clone_string(previous_identity, index)
                == rt.heap.record_clone_string(current_identity, index)
        });
        let formats_equal = rt
            .heap
            .record_get_record(previous_identity, 4)
            .and_then(|format| rt.heap.record_get_int(format, 0))
            == rt
                .heap
                .record_get_record(current_identity, 4)
                .and_then(|format| rt.heap.record_get_int(format, 0));
        i64::from(strings_equal && formats_equal)
    })
}

/// Decode CSV `service,latency_ms` rows into Event records.
fn jet_jit_data_csv_reader(file: i64, encoding: i64, max_groups: i64) -> i64 {
    let delimiter = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(","));
    let csv = crate::enc_stream::jet_jit_csv_reader(file, encoding, delimiter, 1, 0);
    let (ok, handle) = Concurrency::with_runtime_mut(|rt| {
        let ok = crate::runtime_host::jit_result_is_ok(rt, csv).unwrap_or(false);
        let h = crate::runtime_host::jit_result_i64(rt, csv).unwrap_or(0);
        (ok, h)
    });
    if !ok {
        return csv;
    }
    let mut decoded = Vec::new();
    loop {
        let next = crate::enc_stream::jet_jit_csv_reader_next(handle);
        let (ok, bits, cells) = Concurrency::with_runtime_mut(|rt| {
            let ok = crate::runtime_host::jit_result_is_ok(rt, next).unwrap_or(false);
            if !ok {
                return (false, 0_i64, Vec::new());
            }
            let bits = crate::runtime_host::jit_result_i64(rt, next).unwrap_or(0);
            if bits == 0 {
                return (true, 0, Vec::new());
            }
            let row = (bits as u64).wrapping_sub(1) as i64;
            let list = rt.heap.record_get_record(row, 0).unwrap_or(0);
            let n = rt.heap.list_len(list).unwrap_or(0);
            let mut cells = Vec::with_capacity(n as usize);
            for i in 0..n {
                let sid = rt.heap.list_get_int(list, i).unwrap_or(0);
                cells.push(rt.heap.clone_string(sid).unwrap_or_default());
            }
            (true, bits, cells)
        });
        if !ok {
            return next;
        }
        if bits == 0 {
            break;
        }
        if cells.len() < 2 {
            continue;
        }
        let latency = cells[1].parse::<f64>().unwrap_or(0.0);
        let row = Concurrency::with_runtime_mut(|rt| {
            let h = rt.heap.alloc_record(2);
            let svc = rt.heap.alloc_string(cells[0].clone());
            let _ = rt.heap.record_set_string(h, 0, svc);
            let _ = rt.heap.record_set_float(h, 1, latency);
            h
        });
        decoded.push(row);
    }
    Concurrency::with_runtime_mut(|rt| {
        rt.data_streams.push(DataStreamSlot {
            rows: decoded,
            index: 0,
            max_groups,
            type_key: String::new(),
            cancelled: false,
        });
        let h = rt.data_streams.len() as i64;
        crate::runtime_host::alloc_jit_result(rt, true, h as u64)
    })
}

fn jet_jit_data_stream_next(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = match (handle as usize).checked_sub(1) {
            Some(i) => i,
            None => {
                let e = err(DataErrorKind::InvalidArgument, "next", "bad DataStream");
                return pack_data_error(rt, e);
            }
        };
        let Some(stream) = rt.data_streams.get_mut(idx) else {
            let e = err(DataErrorKind::InvalidArgument, "next", "bad DataStream");
            return pack_data_error(rt, e);
        };
        if stream.index >= stream.rows.len() {
            return crate::runtime_host::alloc_jit_result(rt, true, 0);
        }
        let row = stream.rows[stream.index];
        stream.index += 1;
        crate::runtime_host::alloc_jit_result(rt, true, option_bits(Some(row)))
    })
}



const PLOT_VALUE_TEXT: i64 = 0;
const PLOT_VALUE_INTEGER: i64 = 1;
const PLOT_VALUE_NUMBER: i64 = 2;
const PLOT_VALUE_BOOLEAN: i64 = 3;
const PLOT_VALUE_NULL: i64 = 4;

fn option_bits(opt: Option<i64>) -> u64 {
    match opt {
        None => 0,
        Some(v) => (v as u64).wrapping_add(1),
    }
}

fn decode_plot_value(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotValue, DataError> {
    let disc = rt
        .heap
        .record_get_int(record, 0)
        .ok_or_else(|| data_decode_error(operation, "plot value has no discriminant"))?;
    match disc {
        PLOT_VALUE_TEXT => rt
            .heap
            .record_clone_string(record, 1)
            .map(data_plot_rt::JetDataPlotValue::Text)
            .ok_or_else(|| data_decode_error(operation, "plot Text payload is not a string")),
        PLOT_VALUE_INTEGER => rt
            .heap
            .record_get_int(record, 1)
            .map(data_plot_rt::JetDataPlotValue::Integer)
            .ok_or_else(|| data_decode_error(operation, "plot Integer payload is not an integer")),
        PLOT_VALUE_NUMBER => rt
            .heap
            .record_get_float(record, 1)
            .map(data_plot_rt::JetDataPlotValue::Number)
            .ok_or_else(|| data_decode_error(operation, "plot Number payload is not a float")),
        PLOT_VALUE_BOOLEAN => rt
            .heap
            .record_get_bool(record, 1)
            .map(data_plot_rt::JetDataPlotValue::Boolean)
            .ok_or_else(|| data_decode_error(operation, "plot Boolean payload is not a bool")),
        PLOT_VALUE_NULL => Ok(data_plot_rt::JetDataPlotValue::Null),
        _ => Err(data_decode_error(
            operation,
            format!("plot value has unknown discriminant {disc}"),
        )),
    }
}

fn decode_plot_field(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotField, DataError> {
    let id = rt
        .heap
        .record_clone_string(record, 0)
        .ok_or_else(|| data_decode_error(operation, "plot field id is not a string"))?;
    let name = rt
        .heap
        .record_clone_string(record, 1)
        .ok_or_else(|| data_decode_error(operation, "plot field name is not a string"))?;
    let type_name = rt
        .heap
        .record_clone_string(record, 2)
        .ok_or_else(|| data_decode_error(operation, "plot field type is not a string"))?;
    let field = data_plot_rt::JetDataPlotField::with_id(id, name, type_name);
    field
        .validate()
        .map_err(|error| data_decode_error(operation, error.to_string()))?;
    Ok(field)
}

fn decode_plot_column(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<DataPlotColumnSlot, DataError> {
    let field_record = rt
        .heap
        .record_get_record(record, 0)
        .ok_or_else(|| data_decode_error(operation, "plot column field is not a record"))?;
    let field = decode_plot_field(rt, field_record, operation)?;
    let callback_handle = rt
        .heap
        .record_get_int(record, 1)
        .ok_or_else(|| data_decode_error(operation, "plot column callback is not an integer"))?;
    if callback_handle >= 0 {
        return Err(data_decode_error(
            operation,
            "plot column callback is not an opaque callable handle",
        ));
    }
    let callback = crate::runtime_host::jit_callable_parts(rt, callback_handle).ok_or_else(|| {
        data_decode_error(operation, "plot column callback handle is unknown")
    })?;
    if callback.raw_unary.is_none() || callback.raw_pair.is_some() {
        return Err(data_decode_error(
            operation,
            "plot column callback does not have the unary raw ABI",
        ));
    }
    Ok(DataPlotColumnSlot { field, callback })
}

fn materialize_plot_column(
    column: &DataPlotColumnSlot,
) -> data_plot_rt::JetDataPlotColumn<DataPlotRow> {
    let field = column.field.clone();

    let callback = column.callback;
    data_plot_rt::JetDataPlotColumn::new(field, move |row: &DataPlotRow| {
        let Some(value) = crate::runtime_host::invoke_universal_unary(callback, row.0) else {
            return data_plot_rt::JetDataPlotValue::Null;
        };
        Concurrency::with_runtime_mut(|rt| {
            Some(match decode_plot_value(rt, value, "data.plot.column") {
                Ok(value) => value,
                Err(error) => {
                    rt.set_host_fault(format!("data.plot.column: {error}"));
                    data_plot_rt::JetDataPlotValue::Null
                }
            })
        })
        .unwrap_or(data_plot_rt::JetDataPlotValue::Null)
    })
}
fn jet_jit_data_plot_column(field: i64, accessor: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if decode_plot_field(rt, field, "data.plot.column").is_err() {
            rt.set_host_fault("data.plot.column: invalid field metadata");
            return 0;
        }
        if accessor >= 0
            || crate::runtime_host::jit_callable_parts(rt, accessor)
                .map_or(true, |slot| slot.raw_unary.is_none() || slot.raw_pair.is_some())
        {
            rt.set_host_fault("data.plot.column: accessor lacks unary raw ABI");
            return 0;
        }
        let column = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_record(column, 0, field);
        let _ = rt.heap.record_set_int(column, 1, accessor);
        column
    })
}

fn plot_has_field(plot: &data_plot_rt::JetDataPlot<DataPlotRow>, field: &data_plot_rt::JetDataPlotField) -> bool {
    plot.schema().column_id(&field.id).is_some()
}

fn ensure_plot_column(
    slot: &mut DataPlotSlot,
    column: DataPlotColumnSlot,
    operation: &str,
) -> Result<DataPlotColumnSlot, DataError> {
    if !plot_has_field(&slot.plot, &column.field) {
        return Err(data_decode_error(
            operation,
            format!("plot column `{}` is not in the table schema", column.field.name),
        ));
    }
    if let Some(existing) = slot
        .columns
        .iter()
        .find(|existing| existing.field.id == column.field.id)
    {
        if existing.field != column.field {
            return Err(data_decode_error(
                operation,
                format!("plot column `{}` does not match its schema", column.field.name),
            ));
        }
    } else {
        slot.columns.push(column.clone());
    }
    Ok(column)
}

fn alloc_plot_string_result(
    rt: &mut crate::JitRuntime,
    value: Result<String, DataError>,
) -> i64 {
    match value {
        Ok(value) => {
            let handle = rt.heap.alloc_string(value);
            crate::runtime_host::alloc_jit_result(rt, true, handle as u64)
        }
        Err(error) => result_data_err(error),
    }
}

fn jet_jit_data_count(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.list_len(value).unwrap_or(0))
}

fn jet_jit_data_plot(rows: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = match rt.heap.clone_list_values(rows) {
            Some(values) => values,
            None => return result_data_err(data_decode_error("data.plot", "plot rows are not a valid list")),
        };
        let mut plot_rows = Vec::with_capacity(values.len());
        for value in values {
            match row_word(rt, value, "data.plot") {
                Ok(word) => plot_rows.push(DataPlotRow(word)),
                Err(error) => return result_data_err(error),
            }
        }
        let row_type = "List".to_string();
        let identity = "data.list".to_string();
        let schema = data_plot_rt::JetDataPlotSchema::new(identity.clone(), row_type.clone(), Vec::new());
        let source = data_plot_rt::JetDataPlotSourceFacts::new(
            identity.clone(),
            identity.clone(),
            identity.clone(),
            row_type,
            i64::try_from(plot_rows.len()).unwrap_or(i64::MAX),
            identity,
            "jit:data.list",
        );
        let plot = match data_plot_rt::JetDataPlot::from_rows_with_limits(
            plot_rows,
            source,
            schema,
            data_kernel::jet_std::DataLimits::safe(),
        ) {
            Ok(plot) => plot,
            Err(error) => return result_data_err(error.into_data_error()),
        };
        match store_plot(rt, DataPlotSlot { plot, columns: Vec::new() }) {
            Ok(handle) => crate::runtime_host::alloc_jit_result(rt, true, handle as u64),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_line(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.line") {
        Ok(slot) => match data_plot_rt::jet_data_plot_line(&slot.plot) {
            next => store_plot(
                rt,
                DataPlotSlot {
                    plot: next,
                    columns: slot.columns,
                },
            )
            .unwrap_or(0),
        },
        Err(_) => 0,
    })
}

fn jet_jit_data_plot_bar(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.bar") {
        Ok(slot) => {
            let next = data_plot_rt::jet_data_plot_bar(&slot.plot);
            store_plot(
                rt,
                DataPlotSlot {
                    plot: next,
                    columns: slot.columns,
                },
            )
            .unwrap_or(0)
        }
        Err(_) => 0,
    })
}

fn jet_jit_data_plot_point(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.point") {
        Ok(slot) => {
            let next = data_plot_rt::jet_data_plot_point(&slot.plot);
            store_plot(
                rt,
                DataPlotSlot {
                    plot: next,
                    columns: slot.columns,
                },
            )
            .unwrap_or(0)
        }
        Err(_) => 0,
    })
}

fn jet_jit_data_plot_inspect(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.inspect") {
        Ok(slot) => match data_plot_rt::jet_data_plot_inspect(&slot.plot) {
            Ok(value) => {
                let value = pack_inspection(rt, &value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    })
}

fn jet_jit_data_plot_inspect_json(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.inspect_json") {
        Ok(slot) => match data_plot_rt::jet_data_plot_inspect_json(&slot.plot) {
            Ok(value) => {
                let value = rt.heap.alloc_string(value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    })
}

fn jet_jit_data_plot_text(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.text") {
        Ok(slot) => match data_plot_rt::jet_data_plot_text(&slot.plot) {
            Ok(value) => {
                let value = rt.heap.alloc_string(value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    })
}

fn jet_jit_data_plot_svg(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.svg") {
        Ok(slot) => match data_plot_rt::jet_data_plot_svg(&slot.plot) {
            Ok(value) => {
                let value = rt.heap.alloc_string(value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    })
}

fn jet_jit_data_plot_show(plot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match plot_slot(rt, plot, "data.plot.show") {
        Ok(slot) => match data_plot_rt::jet_data_plot_show(&slot.plot) {
            Ok(value) => {
                let value = pack_render(rt, &value);
                crate::runtime_host::alloc_jit_result(rt, true, value as u64)
            }
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    })
}

fn jet_jit_data_plot_render(plot: i64, backend: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let backend = match decode_backend(rt, backend, "data.plot.render") {
            Ok(backend) => backend,
            Err(error) => return result_data_err(error),
        };
        match plot_slot(rt, plot, "data.plot.render") {
            Ok(slot) => match data_plot_rt::jet_data_plot_render(&slot.plot, backend) {
                Ok(value) => {
                    let value = pack_render(rt, &value);
                    crate::runtime_host::alloc_jit_result(rt, true, value as u64)
                }
                Err(error) => result_data_err(error),
            },
            Err(error) => result_data_err(error),
        }
    })
}
fn enum_discriminant(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<i64, DataError> {
    rt.heap
        .record_get_int(record, 0)
        .ok_or_else(|| data_decode_error(operation, "enum carrier has no discriminant"))
}

fn decode_mark(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotMark, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotMark::Line),
        1 => Ok(data_plot_rt::JetDataPlotMark::Bar),
        2 => Ok(data_plot_rt::JetDataPlotMark::Point),
        value => Err(data_decode_error(operation, format!("unknown mark {value}"))),
    }
}

fn decode_channel(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotChannel, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotChannel::X),
        1 => Ok(data_plot_rt::JetDataPlotChannel::Y),
        2 => Ok(data_plot_rt::JetDataPlotChannel::Color),
        3 => Ok(data_plot_rt::JetDataPlotChannel::Size),
        4 => Ok(data_plot_rt::JetDataPlotChannel::Text),
        5 => Ok(data_plot_rt::JetDataPlotChannel::Detail),
        value => Err(data_decode_error(operation, format!("unknown channel {value}"))),
    }
}

fn decode_aggregate(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotAggregate, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotAggregate::None),
        1 => Ok(data_plot_rt::JetDataPlotAggregate::Count),
        2 => Ok(data_plot_rt::JetDataPlotAggregate::Sum),
        3 => Ok(data_plot_rt::JetDataPlotAggregate::Mean),
        4 => Ok(data_plot_rt::JetDataPlotAggregate::Min),
        5 => Ok(data_plot_rt::JetDataPlotAggregate::Max),
        value => Err(data_decode_error(operation, format!("unknown aggregate {value}"))),
    }
}

fn decode_filter_op(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotFilterOp, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotFilterOp::Equal),
        1 => Ok(data_plot_rt::JetDataPlotFilterOp::NotEqual),
        2 => Ok(data_plot_rt::JetDataPlotFilterOp::Less),
        3 => Ok(data_plot_rt::JetDataPlotFilterOp::LessEqual),
        4 => Ok(data_plot_rt::JetDataPlotFilterOp::Greater),
        5 => Ok(data_plot_rt::JetDataPlotFilterOp::GreaterEqual),
        value => Err(data_decode_error(operation, format!("unknown filter operation {value}"))),
    }
}

fn decode_scale_kind(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotScaleKind, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotScaleKind::Linear),
        1 => Ok(data_plot_rt::JetDataPlotScaleKind::Log),
        2 => Ok(data_plot_rt::JetDataPlotScaleKind::Band),
        3 => Ok(data_plot_rt::JetDataPlotScaleKind::Point),
        value => Err(data_decode_error(operation, format!("unknown scale kind {value}"))),
    }
}

fn decode_legend_position(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotLegendPosition, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotLegendPosition::Top),
        1 => Ok(data_plot_rt::JetDataPlotLegendPosition::Right),
        2 => Ok(data_plot_rt::JetDataPlotLegendPosition::Bottom),
        3 => Ok(data_plot_rt::JetDataPlotLegendPosition::Left),
        value => Err(data_decode_error(operation, format!("unknown legend position {value}"))),
    }
}

fn decode_facet_kind(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotFacetKind, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotFacetKind::Row),
        1 => Ok(data_plot_rt::JetDataPlotFacetKind::Column),
        value => Err(data_decode_error(operation, format!("unknown facet kind {value}"))),
    }
}

fn decode_interaction(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotInteraction, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotInteraction::Hover),
        1 => Ok(data_plot_rt::JetDataPlotInteraction::Select),
        2 => Ok(data_plot_rt::JetDataPlotInteraction::Zoom),
        3 => Ok(data_plot_rt::JetDataPlotInteraction::Pan),
        4 => Ok(data_plot_rt::JetDataPlotInteraction::Brush),
        value => Err(data_decode_error(operation, format!("unknown interaction {value}"))),
    }
}

fn decode_backend(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotBackend, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotBackend::Terminal),
        1 => Ok(data_plot_rt::JetDataPlotBackend::Browser),
        2 => Ok(data_plot_rt::JetDataPlotBackend::Native),
        3 => Ok(data_plot_rt::JetDataPlotBackend::Export),
        value => Err(data_decode_error(operation, format!("unknown backend {value}"))),
    }
}

fn decode_domain(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotDomain, DataError> {
    match enum_discriminant(rt, record, operation)? {
        0 => Ok(data_plot_rt::JetDataPlotDomain::Auto),
        1 => {
            let min = rt.heap.record_get_float(record, 1).ok_or_else(|| {
                data_decode_error(operation, "numeric domain min is not a float")
            })?;
            let max = rt.heap.record_get_float(record, 2).ok_or_else(|| {
                data_decode_error(operation, "numeric domain max is not a float")
            })?;
            Ok(data_plot_rt::JetDataPlotDomain::Numeric { min, max })
        }
        2 => {
            let values = rt.heap.record_get_int(record, 1).ok_or_else(|| {
                data_decode_error(operation, "category domain is not a list")
            })?;
            let values = clone_string_list(rt, values, operation)?;
            Ok(data_plot_rt::JetDataPlotDomain::Categories(values))
        }
        value => Err(data_decode_error(operation, format!("unknown plot domain {value}"))),
    }
}

fn clone_string_list(
    rt: &mut crate::JitRuntime,
    list: i64,
    operation: &str,
) -> Result<Vec<String>, DataError> {
    let length = rt
        .heap
        .list_len(list)
        .ok_or_else(|| data_decode_error(operation, "expected string list"))?;
    (0..length)
        .map(|index| {
            rt.heap
                .list_get_string(list, index)
                .ok_or_else(|| data_decode_error(operation, "string list contains a non-string"))
        })
        .collect()
}

fn decode_encoding(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotEncoding, DataError> {
    let channel = rt
        .heap
        .record_get_record(record, 0)
        .ok_or_else(|| data_decode_error(operation, "encoding channel is not an enum"))?;
    let field = rt
        .heap
        .record_get_record(record, 1)
        .ok_or_else(|| data_decode_error(operation, "encoding field is not a record"))?;
    let aggregate = rt
        .heap
        .record_get_record(record, 2)
        .ok_or_else(|| data_decode_error(operation, "encoding aggregate is not an enum"))?;
    Ok(data_plot_rt::JetDataPlotEncoding::new(
        decode_channel(rt, channel, operation)?,
        decode_plot_field(rt, field, operation)?,
        decode_aggregate(rt, aggregate, operation)?,
    ))
}

fn decode_transform_field(
    rt: &mut crate::JitRuntime,
    record: i64,
    index: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotField, DataError> {
    let field = rt
        .heap
        .record_get_record(record, index)
        .ok_or_else(|| data_decode_error(operation, "transform field is not a record"))?;
    decode_plot_field(rt, field, operation)
}

fn decode_transform(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotTransform, DataError> {
    let disc = enum_discriminant(rt, record, operation)?;
    match disc {
        0 => {
            let op_record = rt
                .heap
                .record_get_record(record, 2)
                .ok_or_else(|| data_decode_error(operation, "filter operator is not an enum"))?;
            let value_record = rt
                .heap
                .record_get_record(record, 3)
                .ok_or_else(|| data_decode_error(operation, "filter value is not a record"))?;
            Ok(data_plot_rt::JetDataPlotTransform::Filter {
                field: decode_transform_field(rt, record, 1, operation)?,
                op: decode_filter_op(rt, op_record, operation)?,
                value: decode_plot_value(rt, value_record, operation)?,
            })
        }
        1 => {
            let descending = rt
                .heap
                .record_get_bool(record, 2)
                .ok_or_else(|| data_decode_error(operation, "sort direction is not bool"))?;
            Ok(data_plot_rt::JetDataPlotTransform::Sort {
                field: decode_transform_field(rt, record, 1, operation)?,
                descending,
            })
        }
        2 => {
            let step = rt
                .heap
                .record_get_float(record, 2)
                .ok_or_else(|| data_decode_error(operation, "bin step is not float"))?;
            Ok(data_plot_rt::JetDataPlotTransform::Bin {
                field: decode_transform_field(rt, record, 1, operation)?,
                step,
            })
        }
        3 => {
            let groups = rt
                .heap
                .record_get_int(record, 1)
                .ok_or_else(|| data_decode_error(operation, "aggregate groups are not a list"))?;
            let groups = clone_field_list(rt, groups, operation)?;
            let field = decode_transform_field(rt, record, 2, operation)?;
            let aggregate = rt
                .heap
                .record_get_record(record, 3)
                .ok_or_else(|| data_decode_error(operation, "aggregate kind is not an enum"))?;
            Ok(data_plot_rt::JetDataPlotTransform::Aggregate {
                group_by: groups,
                field,
                aggregate: decode_aggregate(rt, aggregate, operation)?,
            })
        }
        value => Err(data_decode_error(operation, format!("unknown transform {value}"))),
    }
}

fn clone_field_list(
    rt: &mut crate::JitRuntime,
    list: i64,
    operation: &str,
) -> Result<Vec<data_plot_rt::JetDataPlotField>, DataError> {
    let values = rt
        .heap
        .clone_list_values(list)
        .ok_or_else(|| data_decode_error(operation, "field list is not a list"))?;
    values
        .into_iter()
        .map(|value| match value {
            jet_rt::JetVal::RecordRef(record) => decode_plot_field(rt, record, operation),
            _ => Err(data_decode_error(operation, "field list contains a non-record")),
        })
        .collect()
}

fn decode_scale(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotScale, DataError> {
    let channel = rt
        .heap
        .record_get_record(record, 0)
        .ok_or_else(|| data_decode_error(operation, "scale channel is not an enum"))?;
    let kind = rt
        .heap
        .record_get_record(record, 1)
        .ok_or_else(|| data_decode_error(operation, "scale kind is not an enum"))?;
    let domain = rt
        .heap
        .record_get_record(record, 2)
        .ok_or_else(|| data_decode_error(operation, "scale domain is not an enum"))?;
    let mut scale = data_plot_rt::JetDataPlotScale::new(
        decode_channel(rt, channel, operation)?,
        decode_scale_kind(rt, kind, operation)?,
        decode_domain(rt, domain, operation)?,
    );
    scale.clamp = rt
        .heap
        .record_get_bool(record, 3)
        .ok_or_else(|| data_decode_error(operation, "scale clamp is not bool"))?;
    scale.reverse = rt
        .heap
        .record_get_bool(record, 4)
        .ok_or_else(|| data_decode_error(operation, "scale reverse is not bool"))?;
    Ok(scale)
}

fn decode_axis(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotAxis, DataError> {
    let channel = rt
        .heap
        .record_get_record(record, 0)
        .ok_or_else(|| data_decode_error(operation, "axis channel is not an enum"))?;
    let title = rt
        .heap
        .record_clone_string(record, 1)
        .ok_or_else(|| data_decode_error(operation, "axis title is not a string"))?;
    Ok(data_plot_rt::JetDataPlotAxis {
        channel: decode_channel(rt, channel, operation)?,
        title,
        visible: rt
            .heap
            .record_get_bool(record, 2)
            .ok_or_else(|| data_decode_error(operation, "axis visible is not bool"))?,
        grid: rt
            .heap
            .record_get_bool(record, 3)
            .ok_or_else(|| data_decode_error(operation, "axis grid is not bool"))?,
        ticks: rt
            .heap
            .record_get_int(record, 4)
            .ok_or_else(|| data_decode_error(operation, "axis ticks is not integer"))?,
    })
}

fn decode_legend(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotLegend, DataError> {
    let channel = rt
        .heap
        .record_get_record(record, 0)
        .ok_or_else(|| data_decode_error(operation, "legend channel is not an enum"))?;
    let title = rt
        .heap
        .record_clone_string(record, 1)
        .ok_or_else(|| data_decode_error(operation, "legend title is not a string"))?;
    let position = rt
        .heap
        .record_get_record(record, 2)
        .ok_or_else(|| data_decode_error(operation, "legend position is not an enum"))?;
    Ok(data_plot_rt::JetDataPlotLegend {
        channel: decode_channel(rt, channel, operation)?,
        title,
        position: decode_legend_position(rt, position, operation)?,
        visible: rt
            .heap
            .record_get_bool(record, 3)
            .ok_or_else(|| data_decode_error(operation, "legend visible is not bool"))?,
    })
}

fn decode_facet(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotFacet, DataError> {
    let field = rt
        .heap
        .record_get_record(record, 0)
        .ok_or_else(|| data_decode_error(operation, "facet field is not a record"))?;
    let kind = rt
        .heap
        .record_get_record(record, 1)
        .ok_or_else(|| data_decode_error(operation, "facet kind is not an enum"))?;
    Ok(data_plot_rt::JetDataPlotFacet {
        field: decode_plot_field(rt, field, operation)?,
        kind: decode_facet_kind(rt, kind, operation)?,
        title: rt
            .heap
            .record_clone_string(record, 2)
            .ok_or_else(|| data_decode_error(operation, "facet title is not a string"))?,
        columns: rt
            .heap
            .record_get_int(record, 3)
            .ok_or_else(|| data_decode_error(operation, "facet columns is not integer"))?,
        rows: rt
            .heap
            .record_get_int(record, 4)
            .ok_or_else(|| data_decode_error(operation, "facet rows is not integer"))?,
    })
}

fn decode_layer(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotLayer, DataError> {
    let name = rt
        .heap
        .record_clone_string(record, 0)
        .ok_or_else(|| data_decode_error(operation, "layer name is not a string"))?;
    let mark = rt
        .heap
        .record_get_record(record, 1)
        .ok_or_else(|| data_decode_error(operation, "layer mark is not an enum"))?;
    let encodings = rt
        .heap
        .record_get_int(record, 2)
        .ok_or_else(|| data_decode_error(operation, "layer encodings are not a list"))?;
    let transforms = rt
        .heap
        .record_get_int(record, 3)
        .ok_or_else(|| data_decode_error(operation, "layer transforms are not a list"))?;
    let encodings = clone_record_list(rt, encodings, operation, decode_encoding)?;
    let transforms = clone_record_list(rt, transforms, operation, decode_transform)?;
    Ok(data_plot_rt::JetDataPlotLayer {
        name,
        mark: decode_mark(rt, mark, operation)?,
        encodings,
        transforms,
        opacity: rt
            .heap
            .record_get_float(record, 4)
            .ok_or_else(|| data_decode_error(operation, "layer opacity is not float"))?,
    })
}

fn clone_record_list<T>(
    rt: &mut crate::JitRuntime,
    list: i64,
    operation: &str,
    decode: fn(&mut crate::JitRuntime, i64, &str) -> Result<T, DataError>,
) -> Result<Vec<T>, DataError> {
    let values = rt
        .heap
        .clone_list_values(list)
        .ok_or_else(|| data_decode_error(operation, "expected record list"))?;
    values
        .into_iter()
        .map(|value| match value {
            jet_rt::JetVal::RecordRef(record) => decode(rt, record, operation),
            _ => Err(data_decode_error(operation, "record list contains a non-record")),
        })
        .collect()
}

fn decode_accessibility(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotAccessibility, DataError> {
    Ok(data_plot_rt::JetDataPlotAccessibility {
        title: rt
            .heap
            .record_clone_string(record, 0)
            .ok_or_else(|| data_decode_error(operation, "accessibility title is not a string"))?,
        description: rt
            .heap
            .record_clone_string(record, 1)
            .ok_or_else(|| data_decode_error(operation, "accessibility description is not a string"))?,
        summary: rt
            .heap
            .record_clone_string(record, 2)
            .ok_or_else(|| data_decode_error(operation, "accessibility summary is not a string"))?,
        keyboard: rt
            .heap
            .record_get_bool(record, 3)
            .ok_or_else(|| data_decode_error(operation, "accessibility keyboard is not bool"))?,
        announce_selection: rt
            .heap
            .record_get_bool(record, 4)
            .ok_or_else(|| data_decode_error(operation, "accessibility announce_selection is not bool"))?,
    })
}

fn decode_layout(
    rt: &mut crate::JitRuntime,
    record: i64,
    operation: &str,
) -> Result<data_plot_rt::JetDataPlotLayout, DataError> {
    Ok(data_plot_rt::JetDataPlotLayout {
        width: rt
            .heap
            .record_get_float(record, 0)
            .ok_or_else(|| data_decode_error(operation, "layout width is not float"))?,
        height: rt
            .heap
            .record_get_float(record, 1)
            .ok_or_else(|| data_decode_error(operation, "layout height is not float"))?,
        margin_top: rt
            .heap
            .record_get_float(record, 2)
            .ok_or_else(|| data_decode_error(operation, "layout margin_top is not float"))?,
        margin_right: rt
            .heap
            .record_get_float(record, 3)
            .ok_or_else(|| data_decode_error(operation, "layout margin_right is not float"))?,
        margin_bottom: rt
            .heap
            .record_get_float(record, 4)
            .ok_or_else(|| data_decode_error(operation, "layout margin_bottom is not float"))?,
        margin_left: rt
            .heap
            .record_get_float(record, 5)
            .ok_or_else(|| data_decode_error(operation, "layout margin_left is not float"))?,
    })
}

fn pack_enum(rt: &mut crate::JitRuntime, discriminant: i64) -> i64 {
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    record
}

fn pack_plot_field(
    rt: &mut crate::JitRuntime,
    field: &data_plot_rt::JetDataPlotField,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    let id = rt.heap.alloc_string(field.id.clone());
    let name = rt.heap.alloc_string(field.name.clone());
    let type_name = rt.heap.alloc_string(field.type_name.clone());
    let _ = rt.heap.record_set_string(record, 0, id);
    let _ = rt.heap.record_set_string(record, 1, name);
    let _ = rt.heap.record_set_string(record, 2, type_name);
    record
}

fn pack_plot_value(
    rt: &mut crate::JitRuntime,
    value: &data_plot_rt::JetDataPlotValue,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    match value {
        data_plot_rt::JetDataPlotValue::Text(value) => {
            let _ = rt.heap.record_set_int(record, 0, PLOT_VALUE_TEXT);
            let value = rt.heap.alloc_string(value.clone());
            let _ = rt.heap.record_set_string(record, 1, value);
        }
        data_plot_rt::JetDataPlotValue::Integer(value) => {
            let _ = rt.heap.record_set_int(record, 0, PLOT_VALUE_INTEGER);
            let _ = rt.heap.record_set_int(record, 1, *value);
        }
        data_plot_rt::JetDataPlotValue::Number(value) => {
            let _ = rt.heap.record_set_int(record, 0, PLOT_VALUE_NUMBER);
            let _ = rt.heap.record_set_float(record, 1, *value);
        }
        data_plot_rt::JetDataPlotValue::Boolean(value) => {
            let _ = rt.heap.record_set_int(record, 0, PLOT_VALUE_BOOLEAN);
            let _ = rt.heap.record_set_bool(record, 1, *value);
        }
        data_plot_rt::JetDataPlotValue::Null => {
            let _ = rt.heap.record_set_int(record, 0, PLOT_VALUE_NULL);
            let _ = rt.heap.record_set_int(record, 1, 0);
        }
    }
    record
}

fn pack_source(
    rt: &mut crate::JitRuntime,
    source: &data_plot_rt::JetDataPlotSourceFacts,
) -> i64 {
    let record = rt.heap.alloc_record(7);
    for (index, value) in [
        source.table_plan_identity.clone(),
        source.source_identity.clone(),
        source.schema_identity.clone(),
        source.row_type.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        let value = rt.heap.alloc_string(value);
        let _ = rt.heap.record_set_string(record, index as i64, value);
    }
    let _ = rt.heap.record_set_int(record, 4, source.rows);
    let data_identity = rt.heap.alloc_string(source.data_identity.clone());
    let provenance = rt.heap.alloc_string(source.provenance.clone());
    let _ = rt.heap.record_set_string(record, 5, data_identity);
    let _ = rt.heap.record_set_string(record, 6, provenance);
    record
}

fn pack_limits(
    rt: &mut crate::JitRuntime,
    limits: &data_plot_rt::jet_std::DataLimits,
) -> i64 {
    let record = rt.heap.alloc_record(4);
    let _ = rt.heap.record_set_int(record, 0, limits.max_groups);
    let _ = rt.heap.record_set_int(record, 1, limits.max_sort_rows);
    let _ = rt.heap.record_set_int(record, 2, limits.max_join_rows);
    let _ = rt.heap.record_set_int(record, 3, limits.max_output_rows);
    record
}

fn pack_plot_schema(
    rt: &mut crate::JitRuntime,
    schema: &data_plot_rt::JetDataPlotSchema,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    let identity = rt.heap.alloc_string(schema.identity.clone());
    let row_type = rt.heap.alloc_string(schema.row_type.clone());
    let fields = schema
        .columns
        .iter()
        .map(|field| jet_rt::JetVal::RecordRef(pack_plot_field(rt, field)))
        .collect();
    let fields = rt.heap.alloc_list_values(fields);
    let _ = rt.heap.record_set_string(record, 0, identity);
    let _ = rt.heap.record_set_string(record, 1, row_type);
    let _ = rt.heap.record_set_int(record, 2, fields);
    record
}

fn pack_encoding(
    rt: &mut crate::JitRuntime,
    encoding: &data_plot_rt::JetDataPlotEncoding,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    let channel = pack_enum(rt, encoding.channel as i64);
    let field = pack_plot_field(rt, &encoding.field);
    let aggregate = pack_enum(rt, encoding.aggregate as i64);
    let _ = rt.heap.record_set_record(record, 0, channel);
    let _ = rt.heap.record_set_record(record, 1, field);
    let _ = rt.heap.record_set_record(record, 2, aggregate);
    record
}

fn pack_transform(
    rt: &mut crate::JitRuntime,
    transform: &data_plot_rt::JetDataPlotTransform,
) -> i64 {
    match transform {
        data_plot_rt::JetDataPlotTransform::Filter { field, op, value } => {
            let record = rt.heap.alloc_record(4);
            let field = pack_plot_field(rt, field);
            let op = pack_enum(rt, *op as i64);
            let value = pack_plot_value(rt, value);
            let _ = rt.heap.record_set_int(record, 0, 0);
            let _ = rt.heap.record_set_record(record, 1, field);
            let _ = rt.heap.record_set_record(record, 2, op);
            let _ = rt.heap.record_set_record(record, 3, value);
            record
        }
        data_plot_rt::JetDataPlotTransform::Sort { field, descending } => {
            let record = rt.heap.alloc_record(3);
            let field = pack_plot_field(rt, field);
            let _ = rt.heap.record_set_int(record, 0, 1);
            let _ = rt.heap.record_set_record(record, 1, field);
            let _ = rt.heap.record_set_bool(record, 2, *descending);
            record
        }
        data_plot_rt::JetDataPlotTransform::Bin { field, step } => {
            let record = rt.heap.alloc_record(3);
            let field = pack_plot_field(rt, field);
            let _ = rt.heap.record_set_int(record, 0, 2);
            let _ = rt.heap.record_set_record(record, 1, field);
            let _ = rt.heap.record_set_float(record, 2, *step);
            record
        }
        data_plot_rt::JetDataPlotTransform::Aggregate {
            group_by,
            field,
            aggregate,
        } => {
            let record = rt.heap.alloc_record(4);
            let groups = group_by
                .iter()
                .map(|field| jet_rt::JetVal::RecordRef(pack_plot_field(rt, field)))
                .collect();
            let groups = rt.heap.alloc_list_values(groups);
            let field = pack_plot_field(rt, field);
            let aggregate = pack_enum(rt, *aggregate as i64);
            let _ = rt.heap.record_set_int(record, 0, 3);
            let _ = rt.heap.record_set_int(record, 1, groups);
            let _ = rt.heap.record_set_record(record, 2, field);
            let _ = rt.heap.record_set_record(record, 3, aggregate);
            record
        }
    }
}

fn pack_scale(
    rt: &mut crate::JitRuntime,
    scale: &data_plot_rt::JetDataPlotScale,
) -> i64 {
    let record = rt.heap.alloc_record(5);
    let domain = match &scale.domain {
        data_plot_rt::JetDataPlotDomain::Auto => pack_enum(rt, 0),
        data_plot_rt::JetDataPlotDomain::Numeric { min, max } => {
            let domain = rt.heap.alloc_record(3);
            let _ = rt.heap.record_set_int(domain, 0, 1);
            let _ = rt.heap.record_set_float(domain, 1, *min);
            let _ = rt.heap.record_set_float(domain, 2, *max);
            domain
        }
        data_plot_rt::JetDataPlotDomain::Categories(values) => {
            let values = values
                .iter()
                .map(|value| jet_rt::JetVal::String(value.clone()))
                .collect();
            let values = rt.heap.alloc_list_values(values);
            let domain = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_int(domain, 0, 2);
            let _ = rt.heap.record_set_int(domain, 1, values);
            domain
        }
    };
    let channel = pack_enum(rt, scale.channel as i64);
    let kind = pack_enum(rt, scale.kind as i64);
    let _ = rt.heap.record_set_record(record, 0, channel);
    let _ = rt.heap.record_set_record(record, 1, kind);
    let _ = rt.heap.record_set_record(record, 2, domain);
    let _ = rt.heap.record_set_bool(record, 3, scale.clamp);
    let _ = rt.heap.record_set_bool(record, 4, scale.reverse);
    record
}

fn pack_axis(rt: &mut crate::JitRuntime, axis: &data_plot_rt::JetDataPlotAxis) -> i64 {
    let record = rt.heap.alloc_record(5);
    let channel = pack_enum(rt, axis.channel as i64);
    let title = rt.heap.alloc_string(axis.title.clone());
    let _ = rt.heap.record_set_record(record, 0, channel);
    let _ = rt.heap.record_set_string(record, 1, title);
    let _ = rt.heap.record_set_bool(record, 2, axis.visible);
    let _ = rt.heap.record_set_bool(record, 3, axis.grid);
    let _ = rt.heap.record_set_int(record, 4, axis.ticks);
    record
}

fn pack_legend(
    rt: &mut crate::JitRuntime,
    legend: &data_plot_rt::JetDataPlotLegend,
) -> i64 {
    let record = rt.heap.alloc_record(4);
    let channel = pack_enum(rt, legend.channel as i64);
    let title = rt.heap.alloc_string(legend.title.clone());
    let position = pack_enum(rt, legend.position as i64);
    let _ = rt.heap.record_set_record(record, 0, channel);
    let _ = rt.heap.record_set_string(record, 1, title);
    let _ = rt.heap.record_set_record(record, 2, position);
    let _ = rt.heap.record_set_bool(record, 3, legend.visible);
    record
}

fn pack_facet(rt: &mut crate::JitRuntime, facet: &data_plot_rt::JetDataPlotFacet) -> i64 {
    let record = rt.heap.alloc_record(5);
    let field = pack_plot_field(rt, &facet.field);
    let kind = pack_enum(rt, facet.kind as i64);
    let title = rt.heap.alloc_string(facet.title.clone());
    let _ = rt.heap.record_set_record(record, 0, field);
    let _ = rt.heap.record_set_record(record, 1, kind);
    let _ = rt.heap.record_set_string(record, 2, title);
    let _ = rt.heap.record_set_int(record, 3, facet.columns);
    let _ = rt.heap.record_set_int(record, 4, facet.rows);
    record
}

fn pack_layer(rt: &mut crate::JitRuntime, layer: &data_plot_rt::JetDataPlotLayer) -> i64 {
    let record = rt.heap.alloc_record(5);
    let name = rt.heap.alloc_string(layer.name.clone());
    let encodings = layer
        .encodings
        .iter()
        .map(|encoding| jet_rt::JetVal::RecordRef(pack_encoding(rt, encoding)))
        .collect();
    let transforms = layer
        .transforms
        .iter()
        .map(|transform| jet_rt::JetVal::RecordRef(pack_transform(rt, transform)))
        .collect();
    let encodings = rt.heap.alloc_list_values(encodings);
    let transforms = rt.heap.alloc_list_values(transforms);
    let mark = pack_enum(rt, layer.mark as i64);
    let _ = rt.heap.record_set_string(record, 0, name);
    let _ = rt.heap.record_set_record(record, 1, mark);
    let _ = rt.heap.record_set_int(record, 2, encodings);
    let _ = rt.heap.record_set_int(record, 3, transforms);
    let _ = rt.heap.record_set_float(record, 4, layer.opacity);
    record
}
fn pack_accessibility(
    rt: &mut crate::JitRuntime,
    accessibility: &data_plot_rt::JetDataPlotAccessibility,
) -> i64 {
    let record = rt.heap.alloc_record(5);
    for (index, value) in [
        accessibility.title.clone(),
        accessibility.description.clone(),
        accessibility.summary.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        let value = rt.heap.alloc_string(value);
        let _ = rt.heap.record_set_string(record, index as i64, value);
    }
    let _ = rt.heap.record_set_bool(record, 3, accessibility.keyboard);
    let _ = rt
        .heap
        .record_set_bool(record, 4, accessibility.announce_selection);
    record
}

fn pack_layout(rt: &mut crate::JitRuntime, layout: &data_plot_rt::JetDataPlotLayout) -> i64 {
    let record = rt.heap.alloc_record(6);
    for (index, value) in [
        layout.width,
        layout.height,
        layout.margin_top,
        layout.margin_right,
        layout.margin_bottom,
        layout.margin_left,
    ]
    .into_iter()
    .enumerate()
    {
        let _ = rt.heap.record_set_float(record, index as i64, value);
    }
    record
}

fn pack_plot_plan(rt: &mut crate::JitRuntime, plan: &data_plot_rt::JetDataPlotPlan) -> i64 {
    let record = rt.heap.alloc_record(14);
    let source = pack_source(rt, &plan.source);
    let schema = pack_plot_schema(rt, &plan.schema);
    let limits = pack_limits(rt, &plan.limits);
    let mark = pack_enum(rt, plan.mark as i64);
    let _ = rt.heap.record_set_record(record, 0, source);
    let _ = rt.heap.record_set_record(record, 1, schema);
    let _ = rt.heap.record_set_record(record, 2, limits);
    let _ = rt.heap.record_set_record(record, 3, mark);
    let encodings = plan
        .encodings
        .iter()
        .map(|encoding| jet_rt::JetVal::RecordRef(pack_encoding(rt, encoding)))
        .collect();
    let transforms = plan
        .transforms
        .iter()
        .map(|transform| jet_rt::JetVal::RecordRef(pack_transform(rt, transform)))
        .collect();
    let scales = plan
        .scales
        .iter()
        .map(|scale| jet_rt::JetVal::RecordRef(pack_scale(rt, scale)))
        .collect();
    let axes = plan
        .axes
        .iter()
        .map(|axis| jet_rt::JetVal::RecordRef(pack_axis(rt, axis)))
        .collect();
    let legends = plan
        .legends
        .iter()
        .map(|legend| jet_rt::JetVal::RecordRef(pack_legend(rt, legend)))
        .collect();
    let facets = plan
        .facets
        .iter()
        .map(|facet| jet_rt::JetVal::RecordRef(pack_facet(rt, facet)))
        .collect();
    let layers = plan
        .layers
        .iter()
        .map(|layer| jet_rt::JetVal::RecordRef(pack_layer(rt, layer)))
        .collect();
    let interactions = plan
        .interactions
        .iter()
        .map(|interaction| jet_rt::JetVal::RecordRef(pack_enum(rt, *interaction as i64)))
        .collect();
    for (index, values) in [
        encodings,
        transforms,
        scales,
        axes,
        legends,
        facets,
        layers,
        interactions,
    ]
    .into_iter()
    .enumerate()
    {
        let list = rt.heap.alloc_list_values(values);
        let _ = rt.heap.record_set_int(record, (index + 4) as i64, list);
    }
    let accessibility = pack_accessibility(rt, &plan.accessibility);
    let layout = pack_layout(rt, &plan.layout);
    let _ = rt.heap.record_set_record(record, 12, accessibility);
    let _ = rt.heap.record_set_record(record, 13, layout);
    record
}

fn pack_inspection(
    rt: &mut crate::JitRuntime,
    inspection: &data_plot_rt::JetDataPlotInspection,
) -> i64 {
    let record = rt.heap.alloc_record(4);
    let plan = pack_plot_plan(rt, &inspection.plan);
    let indices = rt.heap.alloc_int_list(inspection.selected_indices.clone());
    let columns = inspection
        .selected_columns
        .iter()
        .map(|field| jet_rt::JetVal::RecordRef(pack_plot_field(rt, field)))
        .collect();
    let data = inspection
        .selected_data
        .iter()
        .map(|row| {
            let selected = rt.heap.alloc_record(2);
            let values = row
                .values
                .iter()
                .map(|(field, value)| {
                    let tuple = rt.heap.alloc_record(2);
                    let field = pack_plot_field(rt, field);
                    let value = pack_plot_value(rt, value);
                    let _ = rt.heap.record_set_record(tuple, 0, field);
                    let _ = rt.heap.record_set_record(tuple, 1, value);
                    jet_rt::JetVal::RecordRef(tuple)
                })
                .collect();
            let values = rt.heap.alloc_list_values(values);
            let _ = rt.heap.record_set_int(selected, 0, row.index);
            let _ = rt.heap.record_set_int(selected, 1, values);
            jet_rt::JetVal::RecordRef(selected)
        })
        .collect();
    let columns = rt.heap.alloc_list_values(columns);
    let data = rt.heap.alloc_list_values(data);
    let _ = rt.heap.record_set_record(record, 0, plan);
    let _ = rt.heap.record_set_int(record, 1, indices);
    let _ = rt.heap.record_set_int(record, 2, columns);
    let _ = rt.heap.record_set_int(record, 3, data);
    record
}

fn pack_capability(
    rt: &mut crate::JitRuntime,
    capability: &data_plot_rt::JetDataPlotCapability,
) -> i64 {
    let record = rt.heap.alloc_record(5);
    let backend = pack_enum(rt, capability.backend as i64);
    let support = pack_enum(rt, capability.support as i64);
    let feature = rt.heap.alloc_string(capability.feature.clone());
    let reason = rt.heap.alloc_string(capability.reason.clone());
    let replacement = rt.heap.alloc_string(capability.replacement.clone());
    let _ = rt.heap.record_set_record(record, 0, backend);
    let _ = rt.heap.record_set_string(record, 1, feature);
    let _ = rt.heap.record_set_record(record, 2, support);
    let _ = rt.heap.record_set_string(record, 3, reason);
    let _ = rt.heap.record_set_string(record, 4, replacement);
    record
}

fn pack_render(rt: &mut crate::JitRuntime, render: &data_plot_rt::JetDataPlotRender) -> i64 {
    let record = rt.heap.alloc_record(5);
    let backend = pack_enum(rt, render.backend as i64);
    let format = pack_enum(rt, render.format as i64);
    let body = rt.heap.alloc_string(render.body.clone());
    let source = pack_source(rt, &render.source);
    let capabilities = render
        .capabilities
        .iter()
        .map(|capability| jet_rt::JetVal::RecordRef(pack_capability(rt, capability)))
        .collect();
    let capabilities = rt.heap.alloc_list_values(capabilities);
    let _ = rt.heap.record_set_record(record, 0, backend);
    let _ = rt.heap.record_set_record(record, 1, format);
    let _ = rt.heap.record_set_string(record, 2, body);
    let _ = rt.heap.record_set_record(record, 3, source);
    let _ = rt.heap.record_set_int(record, 4, capabilities);
    record
}


fn apply_plot_column<F>(
    rt: &mut crate::JitRuntime,
    plot: i64,
    column: i64,
    operation: &str,
    apply: F,
) -> i64
where
    F: FnOnce(
        &data_plot_rt::JetDataPlot<DataPlotRow>,
        data_plot_rt::JetDataPlotColumn<DataPlotRow>,
    ) -> Result<data_plot_rt::JetDataPlot<DataPlotRow>, data_kernel::jet_std::DataError>,
{
    let result = (|| {
        let mut slot = plot_slot(rt, plot, operation)?;
        let descriptor = decode_plot_column(rt, column, operation)?;
        let descriptor = ensure_plot_column(&mut slot, descriptor, operation)?;
        let column = materialize_plot_column(&descriptor);
        let plot = apply(&slot.plot, column)?;
        Ok(DataPlotSlot {
            plot,
            columns: slot.columns,
        })
    })();
    match result {
        Ok(slot) => match store_plot(rt, slot) {
            Ok(handle) => crate::runtime_host::alloc_jit_result(rt, true, handle as u64),
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    }
}

fn apply_plot_config<F>(
    rt: &mut crate::JitRuntime,
    plot: i64,
    operation: &str,
    apply: F,
) -> i64
where
    F: FnOnce(
        &data_plot_rt::JetDataPlot<DataPlotRow>,
    ) -> Result<data_plot_rt::JetDataPlot<DataPlotRow>, data_kernel::jet_std::DataError>,
{
    let result = (|| {
        let slot = plot_slot(rt, plot, operation)?;
        let plot = apply(&slot.plot)?;
        Ok(DataPlotSlot {
            plot,
            columns: slot.columns,
        })
    })();
    match result {
        Ok(slot) => match store_plot(rt, slot) {
            Ok(handle) => crate::runtime_host::alloc_jit_result(rt, true, handle as u64),
            Err(error) => result_data_err(error),
        },
        Err(error) => result_data_err(error),
    }
}

fn jet_jit_data_plot_x(plot: i64, column: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        apply_plot_column(rt, plot, column, "data.plot.x", |plot, column| {
            data_plot_rt::jet_data_plot_x(plot, column)
        })
    })
}

fn jet_jit_data_plot_y(plot: i64, column: i64, aggregate: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| {
            let aggregate = decode_aggregate(rt, aggregate, "data.plot.y")?;
            Ok(aggregate)
        })();
        match result {
            Ok(aggregate) => apply_plot_column(
                rt,
                plot,
                column,
                "data.plot.y",
                move |plot, column| data_plot_rt::jet_data_plot_y(plot, column, aggregate),
            ),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_color(plot: i64, column: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        apply_plot_column(rt, plot, column, "data.plot.color", |plot, column| {
            data_plot_rt::jet_data_plot_color(plot, column)
        })
    })
}

fn jet_jit_data_plot_size(plot: i64, column: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        apply_plot_column(rt, plot, column, "data.plot.size", |plot, column| {
            data_plot_rt::jet_data_plot_size(plot, column)
        })
    })
}

fn jet_jit_data_plot_text_channel(plot: i64, column: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        apply_plot_column(rt, plot, column, "data.plot.text_channel", |plot, column| {
            data_plot_rt::jet_data_plot_text_channel(plot, column)
        })
    })
}

fn jet_jit_data_plot_detail(plot: i64, column: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        apply_plot_column(rt, plot, column, "data.plot.detail", |plot, column| {
            data_plot_rt::jet_data_plot_detail(plot, column)
        })
    })
}

fn jet_jit_data_plot_facet(plot: i64, column: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| {
            let mut slot = plot_slot(rt, plot, "data.plot.facet")?;
            let descriptor = decode_plot_column(rt, column, "data.plot.facet")?;
            let descriptor = ensure_plot_column(&mut slot, descriptor, "data.plot.facet")?;
            let column = materialize_plot_column(&descriptor);
            let plot = slot
                .plot
                .clone()
                .facet(column)
                .map_err(data_plot_rt::JetDataPlotError::into_data_error)?;
            Ok(DataPlotSlot {
                plot,
                columns: slot.columns,
            })
        })();
        match result {
            Ok(slot) => match store_plot(rt, slot) {
                Ok(handle) => crate::runtime_host::alloc_jit_result(rt, true, handle as u64),
                Err(error) => result_data_err(error),
            },
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_with_transform(plot: i64, transform: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_transform(rt, transform, "data.plot.with_transform") {
            Ok(transform) => apply_plot_config(rt, plot, "data.plot.with_transform", move |plot| {
                data_plot_rt::jet_data_plot_with_transform(plot, transform)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_with_scale(plot: i64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_scale(rt, scale, "data.plot.with_scale") {
            Ok(scale) => apply_plot_config(rt, plot, "data.plot.with_scale", move |plot| {
                data_plot_rt::jet_data_plot_with_scale(plot, scale)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_with_axis(plot: i64, axis: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_axis(rt, axis, "data.plot.with_axis") {
            Ok(axis) => apply_plot_config(rt, plot, "data.plot.with_axis", move |plot| {
                data_plot_rt::jet_data_plot_with_axis(plot, axis)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_with_legend(plot: i64, legend: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_legend(rt, legend, "data.plot.with_legend") {
            Ok(legend) => apply_plot_config(rt, plot, "data.plot.with_legend", move |plot| {
                data_plot_rt::jet_data_plot_with_legend(plot, legend)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_with_layer(plot: i64, layer: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_layer(rt, layer, "data.plot.with_layer") {
            Ok(layer) => apply_plot_config(rt, plot, "data.plot.with_layer", move |plot| {
                data_plot_rt::jet_data_plot_with_layer(plot, layer)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_with_interaction(plot: i64, interaction: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_interaction(rt, interaction, "data.plot.with_interaction") {
            Ok(interaction) => {
                apply_plot_config(rt, plot, "data.plot.with_interaction", move |plot| {
                    data_plot_rt::jet_data_plot_with_interaction(plot, interaction)
                })
            }
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_accessibility(plot: i64, accessibility: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_accessibility(rt, accessibility, "data.plot.accessibility") {
            Ok(accessibility) => apply_plot_config(rt, plot, "data.plot.accessibility", move |plot| {
                data_plot_rt::jet_data_plot_accessibility(plot, accessibility)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_layout(plot: i64, layout: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match decode_layout(rt, layout, "data.plot.layout") {
            Ok(layout) => apply_plot_config(rt, plot, "data.plot.layout", move |plot| {
                data_plot_rt::jet_data_plot_layout(plot, layout)
            }),
            Err(error) => result_data_err(error),
        }
    })
}

fn jet_jit_data_plot_select_indices(plot: i64, indices: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let indices = match rt.heap.clone_int_list(indices) {
            Some(indices) => indices,
            None => return result_data_err(data_decode_error(
                "data.plot.select_indices",
                "selection is not an integer list",
            )),
        };
        apply_plot_config(rt, plot, "data.plot.select_indices", move |plot| {
            data_plot_rt::jet_data_plot_select_indices(plot, indices)
        })
    })
}
// ── Deferred typed Query carriers ─────────────────────────────────────────
//
// Query is a normal heap value in the resident tier.  Keeping the source,
// operation list, logical plan, and one-shot state in the carrier avoids a
// process-global side table and gives derived queries the same clone/share
// behaviour as the Prelude `DataQuery`.
const QUERY_CARRIER_TAG: i64 = -2963;
const GROUP_CARRIER_TAG: i64 = -2964;
const QUERY_KIND_ROWS: i64 = 0;
const QUERY_KIND_STREAM: i64 = 1;
const QUERY_KIND_COMPUTED_GROUP: i64 = 2;
const QUERY_REDUCER_COUNT: i64 = 1;
const QUERY_REDUCER_SUM: i64 = 2;
const QUERY_REDUCER_MEAN: i64 = 3;
const QUERY_OPERATION_FILTER: i64 = 1;
const QUERY_OPERATION_SORT: i64 = 2;
const QUERY_FIELD_TAG: i64 = 0;
const QUERY_FIELD_KIND: i64 = 1;
const QUERY_FIELD_SOURCE: i64 = 2;
const QUERY_FIELD_OPERATIONS: i64 = 3;
const QUERY_FIELD_PLAN: i64 = 4;
const QUERY_FIELD_STATE: i64 = 5;
const QUERY_FIELD_GROUP: i64 = 6;
const QUERY_FIELD_REDUCER: i64 = 7;
const QUERY_FIELD_VALUE_CALLBACK: i64 = 8;
const QUERY_FIELD_VALUE_TYPE: i64 = 9;
const QUERY_CARRIER_FIELDS: usize = 10;
const GROUP_FIELD_TAG: i64 = 0;
const GROUP_FIELD_QUERY: i64 = 1;
const GROUP_FIELD_KEY_CALLBACK: i64 = 2;
const GROUP_FIELD_KEY_TYPE: i64 = 3;

#[derive(Clone)]
struct QueryRow {
    value: jet_rt::JetVal,
    word: i64,
}

#[derive(Clone, PartialEq)]
enum QueryKey {
    Int(String),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
    Other(jet_rt::JetVal),
}

#[derive(Clone)]
struct QueryGroup {
    key: QueryKey,
    key_word: i64,
    count: i64,
    int_sum: Option<i64>,
    float_sum: Option<f64>,
}

#[derive(Clone, Copy)]
struct QueryParts {
    kind: i64,
    source: i64,
    operations: i64,
    /// The retained logical plan (`QUERY_FIELD_PLAN`), a list of step names
    /// each query operation appends to; `data.query.plan` reads it back.
    plan: i64,
    state: i64,
    group: i64,
    reducer: i64,
    value_callback: i64,
    value_type: i64,
}

fn query_carrier(rt: &crate::JitRuntime, query: i64) -> Option<QueryParts> {
    if rt.heap.record_get_int(query, QUERY_FIELD_TAG)? != QUERY_CARRIER_TAG {
        return None;
    }
    Some(QueryParts {
        kind: rt.heap.record_get_int(query, QUERY_FIELD_KIND)?,
        source: rt.heap.record_get_int(query, QUERY_FIELD_SOURCE)?,
        operations: rt.heap.record_get_int(query, QUERY_FIELD_OPERATIONS)?,
        plan: rt.heap.record_get_int(query, QUERY_FIELD_PLAN)?,
        state: rt.heap.record_get_int(query, QUERY_FIELD_STATE)?,
        group: rt.heap.record_get_int(query, QUERY_FIELD_GROUP)?,
        reducer: rt.heap.record_get_int(query, QUERY_FIELD_REDUCER)?,
        value_callback: rt.heap.record_get_int(query, QUERY_FIELD_VALUE_CALLBACK)?,
        value_type: rt.heap.record_get_int(query, QUERY_FIELD_VALUE_TYPE)?,
    })
}

fn query_group_parts(
    rt: &crate::JitRuntime,
    group: i64,
    operation: &str,
) -> Result<(i64, i64, i64), DataError> {
    if rt.heap.record_get_int(group, GROUP_FIELD_TAG) != Some(GROUP_CARRIER_TAG) {
        return Err(data_decode_error(operation, "grouped query is not a valid carrier"));
    }
    let query = rt
        .heap
        .record_get_int(group, GROUP_FIELD_QUERY)
        .ok_or_else(|| data_decode_error(operation, "grouped query has no source query"))?;
    let key_callback = rt
        .heap
        .record_get_int(group, GROUP_FIELD_KEY_CALLBACK)
        .ok_or_else(|| data_decode_error(operation, "grouped query has no key callback"))?;
    let key_type = rt
        .heap
        .record_get_int(group, GROUP_FIELD_KEY_TYPE)
        .ok_or_else(|| data_decode_error(operation, "grouped query has no key type"))?;
    Ok((query, key_callback, key_type))
}

fn alloc_query_carrier(
    rt: &mut crate::JitRuntime,
    kind: i64,
    source: i64,
    operations: i64,
    plan: i64,
    state: i64,
    group: i64,
    reducer: i64,
    value_callback: i64,
    value_type: i64,
) -> i64 {
    let query = rt.heap.alloc_record(QUERY_CARRIER_FIELDS);
    for (index, value) in [
        QUERY_CARRIER_TAG,
        kind,
        source,
        operations,
        plan,
        state,
        group,
        reducer,
        value_callback,
        value_type,
    ]
    .into_iter()
    .enumerate()
    {
        let _ = rt.heap.record_set_int(query, index as i64, value);
    }
    query
}

fn alloc_query_state(rt: &mut crate::JitRuntime) -> i64 {
    let state = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_bool(state, 0, false);
    state
}

fn query_plan_with_step(
    rt: &mut crate::JitRuntime,
    plan: i64,
    step: &str,
    operation: &str,
) -> Result<i64, DataError> {
    let plan = rt
        .heap
        .clone_list(plan)
        .ok_or_else(|| data_decode_error(operation, "query plan is not a list"))?;
    let step = rt.heap.alloc_string(step);
    rt.heap
        .list_push_int(plan, step)
        .ok_or_else(|| data_decode_error(operation, "query plan cannot append a step"))?;
    Ok(plan)
}

fn query_operation_list_with(
    rt: &mut crate::JitRuntime,
    query: QueryParts,
    kind: i64,
    callback: i64,
    operation: &str,
) -> Result<i64, DataError> {
    if callback >= 0
        || crate::runtime_host::jit_callable_parts(rt, callback)
            .is_none_or(|slot| slot.raw_unary.is_none() || slot.raw_pair.is_some())
    {
        return Err(data_decode_error(operation, "query callback is not a unary callable"));
    }
    let operations = rt
        .heap
        .clone_list(query.operations)
        .ok_or_else(|| data_decode_error(operation, "query operation list is not a list"))?;
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, kind);
    let _ = rt.heap.record_set_int(record, 1, callback);
    rt.heap
        .list_push_int(operations, record)
        .ok_or_else(|| data_decode_error(operation, "query operation list cannot append"))?;
    Ok(operations)
}

fn query_rows(
    rt: &mut crate::JitRuntime,
    rows: i64,
    operation: &str,
) -> Result<Vec<QueryRow>, DataError> {
    let values = rt
        .heap
        .clone_list_values(rows)
        .ok_or_else(|| data_decode_error(operation, "query source is not a list"))?;
    values
        .into_iter()
        .map(|value| {
            let word = row_word(rt, value.clone(), operation)?;
            Ok(QueryRow { value, word })
        })
        .collect()
}
fn query_stream_rows(
    rt: &mut crate::JitRuntime,
    stream: i64,
    operation: &str,
) -> Result<Vec<QueryRow>, DataError> {
    let index = usize::try_from(stream)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .ok_or_else(|| data_decode_error(operation, "query source is not a valid DataStream"))?;
    let stream = rt
        .data_streams
        .get_mut(index)
        .ok_or_else(|| data_decode_error(operation, "query source is not a valid DataStream"))?;
    let rows = stream.rows[stream.index..].to_vec();
    stream.index = stream.rows.len();
    Ok(rows
        .into_iter()
        .map(|word| QueryRow {
            value: jet_rt::JetVal::Int(word),
            word,
        })
        .collect())
}

fn alloc_query_rows(rt: &mut crate::JitRuntime, rows: Vec<QueryRow>) -> i64 {
    if rows
        .iter()
        .all(|row| matches!(row.value, jet_rt::JetVal::Int(_)))
    {
        return rt.heap.alloc_int_list(
            rows.into_iter()
                .map(|row| match row.value {
                    jet_rt::JetVal::Int(value) => value,
                    _ => 0,
                })
                .collect(),
        );
    }
    rt.heap.alloc_list_values(
        rows.into_iter()
            .map(|row| match row.value {
                jet_rt::JetVal::RecordRef(value) => jet_rt::JetVal::Int(value),
                value => value,
            })
            .collect(),
    )
}

fn query_callback(
    callback: i64,
    row: i64,
    operation: &str,
) -> Result<i64, DataError> {
    let slot = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback)
    })
    .ok_or_else(|| data_decode_error(operation, "query callback handle is unknown"))?;
    if slot.raw_unary.is_none() || slot.raw_pair.is_some() {
        return Err(data_decode_error(operation, "query callback is not unary"));
    }
    let value = crate::runtime_host::invoke_universal_unary(slot, row)
        .ok_or_else(|| data_decode_error(operation, "query callback has no universal thunk"))?;
    if let Some(reason) = Concurrency::with_runtime_mut(|rt| rt.trapped.clone()) {
        return Err(data_decode_error(operation, reason));
    }
    Ok(value)
}

fn query_callback_string(value: i64, operation: &str) -> Result<String, DataError> {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(value))
        .ok_or_else(|| data_decode_error(operation, "query sort callback did not return String"))
}

fn descriptor_for_key(
    rt: &crate::JitRuntime,
    key: i64,
    operation: &str,
) -> Result<crate::runtime_host::RuntimeTypeDescriptor, DataError> {
    let text = rt
        .heap
        .clone_string(key)
        .ok_or_else(|| data_decode_error(operation, "query type metadata is not a string"))?;
    let id = text
        .strip_prefix("id:")
        .and_then(|value| value.parse::<u64>().ok());
    id.and_then(|id| rt.runtime_type_descriptor(id).cloned())
        .or_else(|| rt.runtime_type_descriptor_by_name(&text).cloned())
        .ok_or_else(|| data_decode_error(operation, format!("query has no checked type `{text}`")))
}

fn query_key(
    raw: i64,
    descriptor: &crate::runtime_host::RuntimeTypeDescriptor,
    operation: &str,
) -> Result<QueryKey, DataError> {
    let result = Concurrency::with_runtime_mut(|rt| {
        Some(match descriptor.kind {
            crate::runtime_host::RuntimeValueKind::Int => {
                let text = if descriptor.integer_width.is_some() || descriptor.integer_range.is_some() {
                    if descriptor
                        .integer_width
                        .is_some_and(|width| !width.signed && width.bits == 64)
                    {
                        (raw as u64).to_string()
                    } else {
                        raw.to_string()
                    }
                } else {
                    rt.heap.int_to_string(raw)
                };
                Ok(QueryKey::Int(text))
            }
            crate::runtime_host::RuntimeValueKind::Float => {
                let value = match descriptor.abi {
                    crate::runtime_host::RuntimeValueAbi::Float32 => {
                        f32::from_bits(raw as u32) as f64
                    }
                    _ => f64::from_bits(raw as u64),
                };
                Ok(QueryKey::Float(value))
            }
            crate::runtime_host::RuntimeValueKind::Bool => Ok(QueryKey::Bool(raw != 0)),
            crate::runtime_host::RuntimeValueKind::Char => char::from_u32(raw as u32)
                .map(QueryKey::Char)
                .ok_or_else(|| data_decode_error(operation, "group key callback returned invalid Char")),
            crate::runtime_host::RuntimeValueKind::String => rt
                .heap
                .clone_string(raw)
                .map(QueryKey::String)
                .ok_or_else(|| data_decode_error(operation, "group key callback returned invalid String")),
            _ => rt
                .heap
                .clone_value(raw)
                .map(QueryKey::Other)
                .ok_or_else(|| data_decode_error(operation, "group key callback returned invalid key")),
        })
    });
    result.ok_or_else(|| data_decode_error(operation, "runtime is unavailable"))?
}

fn query_store_field(
    rt: &mut crate::JitRuntime,
    record: i64,
    index: i64,
    raw: i64,
    descriptor: &crate::runtime_host::RuntimeTypeDescriptor,
) -> Result<(), DataError> {
    let written = match descriptor.kind {
        crate::runtime_host::RuntimeValueKind::Float => rt.heap.record_set_float(
            record,
            index,
            if descriptor.abi == crate::runtime_host::RuntimeValueAbi::Float32 {
                f32::from_bits(raw as u32) as f64
            } else {
                f64::from_bits(raw as u64)
            },
        ),
        crate::runtime_host::RuntimeValueKind::Bool => rt.heap.record_set_bool(record, index, raw != 0),
        crate::runtime_host::RuntimeValueKind::Char => char::from_u32(raw as u32)
            .and_then(|value| rt.heap.record_set_char(record, index, value)),
        crate::runtime_host::RuntimeValueKind::String => rt.heap.record_set_string(record, index, raw),
        crate::runtime_host::RuntimeValueKind::Record => rt.heap.record_set_record(record, index, raw),
        crate::runtime_host::RuntimeValueKind::Enum
            if rt.heap.clone_record_values(raw).is_some() =>
        {
            rt.heap.record_set_record(record, index, raw)
        }
        _ => rt.heap.record_set_int(record, index, raw),
    };
    written.ok_or_else(|| data_decode_error("query.collect", "group result field could not be stored"))
}

fn query_apply_operations(
    mut rows: Vec<QueryRow>,
    operations: i64,
) -> Result<Vec<QueryRow>, DataError> {
    let operations = Concurrency::with_runtime_mut(|rt| rt.heap.clone_int_list(operations))
        .ok_or_else(|| data_decode_error("query.collect", "query operation list is invalid"))?;
    for operation_handle in operations {
        let (kind, callback) = Concurrency::with_runtime_mut(|rt| {
            rt.heap
                .record_get_int(operation_handle, 0)
                .and_then(|kind| {
                    rt.heap
                        .record_get_int(operation_handle, 1)
                        .map(|callback| (kind, callback))
                })
        })
        .ok_or_else(|| data_decode_error("query.collect", "query operation is invalid"))?;
        match kind {
            QUERY_OPERATION_FILTER => {
                let mut filtered = Vec::with_capacity(rows.len());
                for row in rows {
                    let keep = query_callback(callback, row.word, "query.filter")?;
                    if keep != 0 {
                        filtered.push(row);
                    }
                }
                rows = filtered;
            }
            QUERY_OPERATION_SORT => {
                let max_sort_rows = data_kernel::jet_std::DataLimits::safe().max_sort_rows;
                if rows.len() as i64 > max_sort_rows {
                    return Err(err(
                        DataErrorKind::Limit,
                        "query.sort_by",
                        format!("max_sort_rows {max_sort_rows} exceeded"),
                    ));
                }
                let mut keyed = Vec::with_capacity(rows.len());
                for row in rows {
                    let key = query_callback(callback, row.word, "query.sort_by")?;
                    keyed.push((query_callback_string(key, "query.sort_by")?, row));
                }
                keyed.sort_by_key(|(key, _)| key.clone());
                rows = keyed.into_iter().map(|(_, row)| row).collect();
            }
            _ => {
                return Err(data_decode_error(
                    "query.collect",
                    "query operation has an unknown kind",
                ));
            }
        }
    }
    let max_output_rows = data_kernel::jet_std::DataLimits::safe().max_output_rows;
    if rows.len() as i64 > max_output_rows {
        return Err(err(
            DataErrorKind::Limit,
            "query.collect",
            format!("max_output_rows {max_output_rows} exceeded"),
        ));
    }
    Ok(rows)
}

fn query_group_rows(
    rows: Vec<QueryRow>,
    key_callback: i64,
    key_descriptor: &crate::runtime_host::RuntimeTypeDescriptor,
    reducer: i64,
    value_callback: i64,
    value_descriptor: Option<&crate::runtime_host::RuntimeTypeDescriptor>,
) -> Result<Vec<QueryRow>, DataError> {
    let operation = match reducer {
        QUERY_REDUCER_COUNT => "query.group_by.count",
        QUERY_REDUCER_SUM => "query.group_by.sum",
        QUERY_REDUCER_MEAN => "query.group_by.mean",
        _ => "query.group_by",
    };
    let value_kind = value_descriptor.map(|descriptor| descriptor.kind);
    let mut groups = Vec::new();
    let limits = data_kernel::jet_std::DataLimits::safe();
    for row in rows {
        let key_word = query_callback(key_callback, row.word, operation)?;
        let key = query_key(key_word, key_descriptor, operation)?;
        let position = groups.iter().position(|group: &QueryGroup| group.key == key);
        if let Some(position) = position {
            let group = &mut groups[position];
            group.count = group.count.saturating_add(1);
            match reducer {
                QUERY_REDUCER_SUM => match value_kind {
                    Some(crate::runtime_host::RuntimeValueKind::Int) => {
                        let value = query_callback(value_callback, row.word, operation)?;
                        let next = if value_descriptor.is_some_and(|descriptor| {
                            descriptor.integer_width.is_some() || descriptor.integer_range.is_some()
                        }) {
                            group
                                .int_sum
                                .unwrap_or(0)
                                .checked_add(value)
                                .ok_or_else(|| {
                                    err(DataErrorKind::Overflow, operation, "integer group sum overflowed")
                                })
                        } else {
                            Ok(Concurrency::with_runtime_mut(|rt| {
                                rt.heap.int_add(group.int_sum.unwrap_or(0), value)
                            }))
                        };
                        group.int_sum = Some(next?);
                    }
                    Some(crate::runtime_host::RuntimeValueKind::Float) => {
                        let value = query_callback(value_callback, row.word, operation)?;
                        let value = match value_descriptor.map(|descriptor| descriptor.abi) {
                            Some(crate::runtime_host::RuntimeValueAbi::Float32) => {
                                f32::from_bits(value as u32) as f64
                            }
                            _ => f64::from_bits(value as u64),
                        };
                        group.float_sum = Some(group.float_sum.unwrap_or(0.0) + value);
                    }
                    _ => {
                        return Err(data_decode_error(
                            operation,
                            "query sum callback return type is not Add-compatible",
                        ));
                    }
                },
                QUERY_REDUCER_MEAN => {
                    let value = query_callback(value_callback, row.word, operation)?;
                    let value = f64::from_bits(value as u64);
                    if !value.is_finite() {
                        return Err(err(
                            DataErrorKind::NonFinite,
                            operation,
                            "group values must be finite",
                        ));
                    }
                    group.float_sum = Some(group.float_sum.unwrap_or(0.0) + value);
                }
                _ => {}
            }
        } else {
            if groups.len() as i64 >= limits.max_groups {
                return Err(err(
                    DataErrorKind::Limit,
                    operation,
                    format!("max_groups {} exceeded", limits.max_groups),
                ));
            }
            let (int_sum, float_sum) = match reducer {
                QUERY_REDUCER_SUM => match value_kind {
                    Some(crate::runtime_host::RuntimeValueKind::Int) => (
                        Some(query_callback(value_callback, row.word, operation)?),
                        None,
                    ),
                    Some(crate::runtime_host::RuntimeValueKind::Float) => {
                        let value = query_callback(value_callback, row.word, operation)?;
                        let value = match value_descriptor.map(|descriptor| descriptor.abi) {
                            Some(crate::runtime_host::RuntimeValueAbi::Float32) => {
                                f32::from_bits(value as u32) as f64
                            }
                            _ => f64::from_bits(value as u64),
                        };
                        (None, Some(value))
                    }
                    _ => {
                        return Err(data_decode_error(
                            operation,
                            "query sum callback return type is not Add-compatible",
                        ));
                    }
                },
                QUERY_REDUCER_MEAN => {
                    let value = query_callback(value_callback, row.word, operation)?;
                    let value = f64::from_bits(value as u64);
                    if !value.is_finite() {
                        return Err(err(
                            DataErrorKind::NonFinite,
                            operation,
                            "group values must be finite",
                        ));
                    }
                    (None, Some(value))
                }
                _ => (None, None),
            };
            groups.push(QueryGroup {
                key,
                key_word,
                count: 1,
                int_sum,
                float_sum,
            });
        }
    }
    let result = Concurrency::with_runtime_mut(|rt| {
        Some(
            groups
                .into_iter()
                .map(|group| {
                    let record = rt.heap.alloc_record(2);
                    query_store_field(rt, record, 0, group.key_word, key_descriptor)?;
                    let value = match reducer {
                        QUERY_REDUCER_COUNT => rt.heap.int_from_i64(group.count),
                        QUERY_REDUCER_SUM => {
                            if let Some(value) = group.int_sum {
                                value
                            } else {
                                group.float_sum.unwrap_or(0.0).to_bits() as i64
                            }
                        }
                        QUERY_REDUCER_MEAN => {
                            (group.float_sum.unwrap_or(0.0) / group.count as f64).to_bits() as i64
                        }
                        _ => 0,
                    };
                    let value_descriptor = match reducer {
                        QUERY_REDUCER_COUNT => None,
                        QUERY_REDUCER_SUM | QUERY_REDUCER_MEAN => value_descriptor,
                        _ => None,
                    };
                    if let Some(descriptor) = value_descriptor {
                        query_store_field(rt, record, 1, value, descriptor)?;
                    } else {
                        let _ = rt.heap.record_set_int(record, 1, value);
                    }
                    Ok(QueryRow {
                        value: jet_rt::JetVal::Int(record),
                        word: record,
                    })
                })
                .collect::<Result<Vec<_>, DataError>>(),
        )
    });
    result.ok_or_else(|| data_decode_error(operation, "runtime is unavailable"))?
}

fn query_collect_rows(query: i64) -> Result<Vec<QueryRow>, DataError> {
    let parts = Concurrency::with_runtime_mut(|rt| {
        Some(
            query_carrier(rt, query)
                .ok_or_else(|| data_decode_error("query.collect", "query is not a valid carrier")),
        )
    })
    .ok_or_else(|| data_decode_error("query.collect", "runtime is unavailable"))??;
    let rows = if parts.kind == QUERY_KIND_ROWS {
        Concurrency::with_runtime_mut(|rt| Some(query_rows(rt, parts.source, "query.collect")))
            .ok_or_else(|| data_decode_error("query.collect", "runtime is unavailable"))??
    } else if parts.kind == QUERY_KIND_STREAM {
        let already_collected = Concurrency::with_runtime_mut(|rt| {
            let already = rt.heap.record_get_bool(parts.state, 0).unwrap_or(true);
            if !already {
                let _ = rt.heap.record_set_bool(parts.state, 0, true);
            }
            already
        });
        if already_collected {
            return Err(err(
                DataErrorKind::State,
                "query.collect",
                "query has already been collected",
            ));
        }
        Concurrency::with_runtime_mut(|rt| Some(query_stream_rows(rt, parts.source, "query.collect")))
            .ok_or_else(|| data_decode_error("query.collect", "runtime is unavailable"))??
    } else if parts.kind == QUERY_KIND_COMPUTED_GROUP {
        let already_collected = Concurrency::with_runtime_mut(|rt| {
            let already = rt.heap.record_get_bool(parts.state, 0).unwrap_or(true);
            if !already {
                let _ = rt.heap.record_set_bool(parts.state, 0, true);
            }
            already
        });
        if already_collected {
            return Err(err(
                DataErrorKind::State,
                "query.collect",
                "query has already been collected",
            ));
        }
        let base_rows = query_collect_rows(parts.source)?;
        let (_, key_callback, key_type) = Concurrency::with_runtime_mut(|rt| {
            Some(query_group_parts(rt, parts.group, "query.collect"))
        })
        .ok_or_else(|| data_decode_error("query.collect", "runtime is unavailable"))??;
        let value_descriptor = if parts.reducer == QUERY_REDUCER_SUM {
            Some(
                Concurrency::with_runtime_mut(|rt| {
                    Some(descriptor_for_key(rt, parts.value_type, "query.group_by.sum"))
                })
                .ok_or_else(|| data_decode_error("query.group_by.sum", "runtime is unavailable"))??,
            )
        } else {
            None
        };
        let key_descriptor = Concurrency::with_runtime_mut(|rt| {
            Some(descriptor_for_key(rt, key_type, "query.collect"))
        })
        .ok_or_else(|| data_decode_error("query.collect", "runtime is unavailable"))??;
        query_group_rows(
            base_rows,
            key_callback,
            &key_descriptor,
            parts.reducer,
            parts.value_callback,
            value_descriptor.as_ref(),
        )?
    } else {
        return Err(data_decode_error("query.collect", "query carrier has unknown source kind"));
    };
    query_apply_operations(rows, parts.operations)
}

fn jet_jit_data_query(rows: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (kind, source) = if let Some(source) = rt.heap.clone_list(rows) {
            (QUERY_KIND_ROWS, source)
        } else {
            let stream_index = usize::try_from(rows).ok().and_then(|index| index.checked_sub(1));
            if stream_index.is_none_or(|index| rt.data_streams.get(index).is_none()) {
                rt.set_host_fault("data.query: source is not a list or DataStream");
                return 0;
            }
            (QUERY_KIND_STREAM, rows)
        };
        let operations = rt.heap.alloc_empty_list();
        let plan = rt.heap.alloc_empty_list();
        let step = rt.heap.alloc_string(if kind == QUERY_KIND_STREAM {
            "stream"
        } else {
            "scan"
        });
        let _ = rt.heap.list_push_int(plan, step);
        let state = alloc_query_state(rt);
        alloc_query_carrier(
            rt,
            kind,
            source,
            operations,
            plan,
            state,
            0,
            0,
            0,
            0,
        )
    })
}

fn jet_jit_data_query_sql(rows: i64, sql: i64, type_key: i64) -> i64 {
    let source_rows = match Concurrency::with_runtime_mut(|rt| Some(query_rows(rt, rows, "data.query"))) {
        Some(Ok(rows)) => rows,
        Some(Err(error)) => {
            return crate::Encoding::result_err_fields(
                crate::Encoding::json_rt::FieldError::one(error.reason),
            )
        }
        None => {
            return crate::Encoding::result_err_fields(
                crate::Encoding::json_rt::FieldError::one("runtime is unavailable"),
            )
        }
    };
    let sql = match Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(sql)) {
        Some(sql) => sql,
        None => {
            return crate::Encoding::result_err_fields(
                crate::Encoding::json_rt::FieldError::one("query SQL is not a String"),
            )
        }
    };
    let descriptor = Concurrency::with_runtime_mut(|rt| {
        if type_key == 0 {
            None
        } else {
            descriptor_for_key(rt, type_key, "data.query")
                .ok()
        }
    });
    let trees = if let Some(descriptor) = descriptor {
        match Concurrency::with_runtime_mut(|rt| {
            Some(
                source_rows
                    .iter()
                    .map(|row| crate::Receipt::encode_jit_value(rt, row.word, &descriptor))
                    .collect::<Result<Vec<_>, _>>(),
            )
        }) {
            Some(Ok(trees)) => trees,
            Some(Err(reason)) => {
                return crate::Encoding::result_err_fields(
                    crate::Encoding::json_rt::FieldError::one(reason),
                )
            }
            None => {
                return crate::Encoding::result_err_fields(
                    crate::Encoding::json_rt::FieldError::one("runtime is unavailable"),
                )
            }
        }
    } else {
        let Some(trees) = source_rows
            .iter()
            .map(|row| crate::Encoding::read_datatree(row.word))
            .collect::<Option<Vec<_>>>()
        else {
            return crate::Encoding::result_err_fields(
                crate::Encoding::json_rt::FieldError::one(
                    "query SQL has no checked row type metadata",
                ),
            );
        };
        trees
    };
    let selected = match crate::Encoding::data_query_rt::jet_data_query_indices(&trees, &sql) {
        Ok(selected) => selected,
        Err(errors) => return crate::Encoding::result_err_fields(errors),
    };
    let selected_rows = selected
        .into_iter()
        .filter_map(|index| source_rows.get(index).cloned())
        .collect::<Vec<_>>();
    let query = Concurrency::with_runtime_mut(|rt| {
        let source = alloc_query_rows(rt, selected_rows);
        let operations = rt.heap.alloc_empty_list();
        let plan = rt.heap.alloc_empty_list();
        let scan = rt.heap.alloc_string("scan");
        let sql_step = rt.heap.alloc_string("sql");
        let _ = rt.heap.list_push_int(plan, scan);
        let _ = rt.heap.list_push_int(plan, sql_step);
        let state = alloc_query_state(rt);
        alloc_query_carrier(
            rt,
            QUERY_KIND_ROWS,
            source,
            operations,
            plan,
            state,
            0,
            0,
            0,
            0,
        )
    });
    Concurrency::with_runtime_mut(|rt| crate::runtime_host::alloc_jit_result(rt, true, query as u64))
}

fn jet_jit_data_query_filter(query: i64, callback: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(parts) = query_carrier(rt, query) else {
            rt.set_host_fault("data.query.filter: query is not a valid carrier");
            return 0;
        };
        let operations = match query_operation_list_with(
            rt,
            parts,
            QUERY_OPERATION_FILTER,
            callback,
            "data.query.filter",
        ) {
            Ok(operations) => operations,
            Err(error) => {
                rt.set_host_fault(format!("data.query.filter: {error}"));
                return 0;
            }
        };
        let plan = match query_plan_with_step(rt, parts.plan, "filter", "data.query.filter") {
            Ok(plan) => plan,
            Err(error) => {
                rt.set_host_fault(format!("data.query.filter: {error}"));
                return 0;
            }
        };
        alloc_query_carrier(
            rt,
            parts.kind,
            parts.source,
            operations,
            plan,
            parts.state,
            parts.group,
            parts.reducer,
            parts.value_callback,
            parts.value_type,
        )
    })
}

fn jet_jit_data_query_sort_by(query: i64, callback: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(parts) = query_carrier(rt, query) else {
            rt.set_host_fault("data.query.sort_by: query is not a valid carrier");
            return 0;
        };
        let operations = match query_operation_list_with(
            rt,
            parts,
            QUERY_OPERATION_SORT,
            callback,
            "data.query.sort_by",
        ) {
            Ok(operations) => operations,
            Err(error) => {
                rt.set_host_fault(format!("data.query.sort_by: {error}"));
                return 0;
            }
        };
        let plan = match query_plan_with_step(rt, parts.plan, "sort_by", "data.query.sort_by") {
            Ok(plan) => plan,
            Err(error) => {
                rt.set_host_fault(format!("data.query.sort_by: {error}"));
                return 0;
            }
        };
        alloc_query_carrier(
            rt,
            parts.kind,
            parts.source,
            operations,
            plan,
            parts.state,
            parts.group,
            parts.reducer,
            parts.value_callback,
            parts.value_type,
        )
    })
}

fn jet_jit_data_query_collect(query: i64) -> i64 {
    match query_collect_rows(query) {
        Ok(rows) => Concurrency::with_runtime_mut(|rt| {
            let rows = alloc_query_rows(rt, rows);
            crate::runtime_host::alloc_jit_result(rt, true, rows as u64)
        }),
        Err(error) => result_data_err(error),
    }
}

fn jet_jit_data_query_plan(query: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(parts) = query_carrier(rt, query) else {
            rt.set_host_fault("data.query.plan: query is not a valid carrier");
            return 0;
        };
        rt.heap.clone_list(parts.plan).unwrap_or_else(|| {
            rt.set_host_fault("data.query.plan: query plan is not a list");
            0
        })
    })
}

fn jet_jit_data_query_group_by(query: i64, callback: i64, key_type: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if query_carrier(rt, query).is_none() {
            rt.set_host_fault("data.query.group_by: query is not a valid carrier");
            return 0;
        }
        if callback >= 0
            || crate::runtime_host::jit_callable_parts(rt, callback)
                .is_none_or(|slot| slot.raw_unary.is_none() || slot.raw_pair.is_some())
        {
            rt.set_host_fault("data.query.group_by: key callback is not unary");
            return 0;
        }
        let group = rt.heap.alloc_record(4);
        let _ = rt.heap.record_set_int(group, GROUP_FIELD_TAG, GROUP_CARRIER_TAG);
        let _ = rt.heap.record_set_int(group, GROUP_FIELD_QUERY, query);
        let _ = rt.heap.record_set_int(group, GROUP_FIELD_KEY_CALLBACK, callback);
        let _ = rt.heap.record_set_int(group, GROUP_FIELD_KEY_TYPE, key_type);
        group
    })
}

fn query_reducer_plan(
    rt: &mut crate::JitRuntime,
    grouped: i64,
    reducer: &str,
    operation: &str,
) -> Result<(QueryParts, i64, i64, i64), DataError> {
    let (query, _, _) = query_group_parts(rt, grouped, operation)?;
    let parts = query_carrier(rt, query)
        .ok_or_else(|| data_decode_error(operation, "grouped query source is invalid"))?;
    let plan = query_plan_with_step(rt, parts.plan, "group_by", operation)?;
    let plan = {
        let reducer = rt.heap.alloc_string(reducer);
        rt.heap
            .list_push_int(plan, reducer)
            .ok_or_else(|| data_decode_error(operation, "query plan cannot append reducer"))?;
        plan
    };
    Ok((parts, query, grouped, plan))
}

fn jet_jit_data_group_count_query(grouped: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (_parts, query, group, plan) =
            match query_reducer_plan(rt, grouped, "count", "query.group_by.count") {
                Ok(value) => value,
                Err(error) => {
                    rt.set_host_fault(format!("query.group_by.count: {error}"));
                    return Some(0);
                }
            };
        let operations = rt.heap.alloc_empty_list();
        let state = alloc_query_state(rt);
        Some(alloc_query_carrier(
            rt,
            QUERY_KIND_COMPUTED_GROUP,
            query,
            operations,
            plan,
            state,
            group,
            QUERY_REDUCER_COUNT,
            0,
            0,
        ))
    })
    .unwrap_or(0)
}

fn jet_jit_data_group_sum_query(grouped: i64, callback: i64, value_type: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (_parts, query, group, plan) =
            match query_reducer_plan(rt, grouped, "sum", "query.group_by.sum") {
                Ok(value) => value,
                Err(error) => {
                    rt.set_host_fault(format!("query.group_by.sum: {error}"));
                    return Some(0);
                }
            };
        if callback >= 0
            || crate::runtime_host::jit_callable_parts(rt, callback)
                .is_none_or(|slot| slot.raw_unary.is_none() || slot.raw_pair.is_some())
        {
            rt.set_host_fault("query.group_by.sum: value callback is not unary");
            return Some(0);
        }
        let operations = rt.heap.alloc_empty_list();
        let state = alloc_query_state(rt);
        Some(alloc_query_carrier(
            rt,
            QUERY_KIND_COMPUTED_GROUP,
            query,
            operations,
            plan,
            state,
            group,
            QUERY_REDUCER_SUM,
            callback,
            value_type,
        ))
    })
    .unwrap_or(0)
}

fn jet_jit_data_group_mean_query(grouped: i64, callback: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (_parts, query, group, plan) =
            match query_reducer_plan(rt, grouped, "mean", "query.group_by.mean") {
                Ok(value) => value,
                Err(error) => {
                    rt.set_host_fault(format!("query.group_by.mean: {error}"));
                    return Some(0);
                }
            };
        if callback >= 0
            || crate::runtime_host::jit_callable_parts(rt, callback)
                .is_none_or(|slot| slot.raw_unary.is_none() || slot.raw_pair.is_some())
        {
            rt.set_host_fault("query.group_by.mean: value callback is not unary");
            return Some(0);
        }
        let operations = rt.heap.alloc_empty_list();
        let state = alloc_query_state(rt);
        Some(alloc_query_carrier(
            rt,
            QUERY_KIND_COMPUTED_GROUP,
            query,
            operations,
            plan,
            state,
            group,
            QUERY_REDUCER_MEAN,
            callback,
            0,
        ))
    })
    .unwrap_or(0)
}


fn err_at(
    kind: DataErrorKind,
    op: &str,
    index: Option<i64>,
    reason: impl Into<String>,
) -> DataError {
    let mut e = err(kind, op, reason);
    e.index = index.ok_or(JetAbsent);
    e
}

host_fns! {
    struct DataHostFns;
    register: register_symbols;
    declare: declare(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig_void = Signature::new(cc);
        sig_void.returns.push(AbiParam::new(types::I64));
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = sig_unary.clone();
        sig_binary.params.push(AbiParam::new(types::I64));
        let mut sig_ternary = sig_binary.clone();
        sig_ternary.params.push(AbiParam::new(types::I64));
        let mut sig_quaternary = sig_ternary.clone();
        sig_quaternary.params.push(AbiParam::new(types::I64));
        let mut sig_quinary = sig_quaternary.clone();
        sig_quinary.params.push(AbiParam::new(types::I64));


    }
    status: "jet_jit_data_status" => jet_jit_data_status: sig_void;
    require_bridge: "jet_jit_data_require_bridge" => jet_jit_data_require_bridge: sig_unary;
    stat: "jet_jit_data_stat" => jet_jit_data_stat: sig_binary;
    quantile: "jet_jit_data_quantile" => jet_jit_data_quantile: sig_binary;
    describe: "jet_jit_data_describe" => jet_jit_data_describe: sig_unary;
    bar_text: "jet_jit_data_bar_text" => jet_jit_data_bar_text: sig_unary;
    bar_svg: "jet_jit_data_bar_svg" => jet_jit_data_bar_svg: sig_unary;
    line_text: "jet_jit_data_line_text" => jet_jit_data_line_text: sig_binary;
    line_svg: "jet_jit_data_line_svg" => jet_jit_data_line_svg: sig_binary;
    error_show: "jet_jit_data_error_show" => jet_jit_data_error_show: sig_unary;
    inner_join: "jet_jit_data_inner_join" => jet_jit_data_inner_join: sig_quaternary;
    left_join: "jet_jit_data_left_join" => jet_jit_data_left_join: sig_quaternary;
    pivot_sum: "jet_jit_data_pivot_sum" => jet_jit_data_pivot_sum: sig_quaternary;
    rolling_mean: "jet_jit_data_rolling_mean" => jet_jit_data_rolling_mean: sig_binary;
    data_loader_load: "jet_data_loader_load" => jet_data_loader_load: sig_ternary;
    data_loader_load_default: "jet_data_loader_load_default" => jet_data_loader_load_default: sig_binary;
    data_loader_file: "jet_data_loader_file" => jet_data_loader_file: sig_quaternary;
    data_loader_file_member: "jet_data_loader_file_member" => jet_data_loader_file_member: sig_quinary;
    data_loader_url: "jet_data_loader_url" => jet_data_loader_url: sig_quinary;
    data_loader_database: "jet_data_loader_database" => jet_data_loader_database: sig_quinary;
    data_loader_value: "jet_data_loader_value" => jet_data_loader_value: sig_ternary;
    data_loader_snapshot: "jet_data_loader_snapshot" => jet_data_loader_snapshot: sig_binary;
    data_loader_stream: "jet_data_loader_stream" => jet_data_loader_stream: sig_binary;
    data_stream_collect: "jet_data_stream_collect" => jet_data_stream_collect: sig_binary;
    data_stream_next: "jet_data_stream_next" => jet_data_stream_next: sig_binary;
    data_loader_bind: "jet_data_loader_bind" => jet_data_loader_bind: sig_binary;
    data_loader_bind_text: "jet_data_loader_bind_text" => jet_data_loader_bind_text: sig_binary;
    data_loader_cancel: "jet_data_loader_cancel" => jet_data_loader_cancel: sig_unary;
    data_loader_offline: "jet_data_loader_offline" => jet_data_loader_offline: sig_binary;
    data_loader_invalidate: "jet_data_loader_invalidate" => jet_data_loader_invalidate: sig_binary;
    data_loader_needs_refresh: "jet_data_loader_needs_refresh" => jet_data_loader_needs_refresh: sig_unary;
    data_loader_ready: "jet_data_loader_ready" => jet_data_loader_ready: sig_unary;
    data_loader_status: "jet_data_loader_status" => jet_data_loader_status: sig_unary;
    data_loader_source_identity: "jet_data_loader_source_identity" => jet_data_loader_source_identity: sig_unary;
    data_loader_authority_of: "jet_data_loader_authority_of" => jet_data_loader_authority_of: sig_unary;
    data_snapshot_reusable: "jet_data_snapshot_reusable" => jet_data_snapshot_reusable: sig_binary;
    data_stream_cancel: "jet_data_stream_cancel" => jet_data_stream_cancel: sig_unary;
    csv_reader: "jet_jit_data_csv_reader" => jet_jit_data_csv_reader: sig_ternary;
    stream_next: "jet_jit_data_stream_next" => jet_jit_data_stream_next: sig_unary;
    data_count: "jet_jit_data_count" => jet_jit_data_count: sig_unary;
    query: "jet_jit_data_query" => jet_jit_data_query: sig_unary;
    query_sql: "jet_jit_data_query_sql" => jet_jit_data_query_sql: sig_ternary;
    query_filter: "jet_jit_data_query_filter" => jet_jit_data_query_filter: sig_binary;
    query_sort_by: "jet_jit_data_query_sort_by" => jet_jit_data_query_sort_by: sig_binary;
    query_collect: "jet_jit_data_query_collect" => jet_jit_data_query_collect: sig_unary;
    query_plan: "jet_jit_data_query_plan" => jet_jit_data_query_plan: sig_unary;
    query_group_by: "jet_jit_data_query_group_by" => jet_jit_data_query_group_by: sig_ternary;
    group_count_query: "jet_jit_data_group_count_query" => jet_jit_data_group_count_query: sig_unary;
    group_sum_query: "jet_jit_data_group_sum_query" => jet_jit_data_group_sum_query: sig_ternary;
    group_mean_query: "jet_jit_data_group_mean_query" => jet_jit_data_group_mean_query: sig_binary;
    plot_column: "jet_jit_data_plot_column" => jet_jit_data_plot_column: sig_binary;
    data_plot: "jet_jit_data_plot" => jet_jit_data_plot: sig_unary;
    plot_inspect: "jet_jit_data_plot_inspect" => jet_jit_data_plot_inspect: sig_unary;
    plot_inspect_json: "jet_jit_data_plot_inspect_json" => jet_jit_data_plot_inspect_json: sig_unary;
    plot_text: "jet_jit_data_plot_text" => jet_jit_data_plot_text: sig_unary;
    plot_svg: "jet_jit_data_plot_svg" => jet_jit_data_plot_svg: sig_unary;
    plot_show: "jet_jit_data_plot_show" => jet_jit_data_plot_show: sig_unary;
    plot_render: "jet_jit_data_plot_render" => jet_jit_data_plot_render: sig_binary;
    plot_line: "jet_jit_data_plot_line" => jet_jit_data_plot_line: sig_unary;
    plot_bar: "jet_jit_data_plot_bar" => jet_jit_data_plot_bar: sig_unary;
    plot_point: "jet_jit_data_plot_point" => jet_jit_data_plot_point: sig_unary;
    plot_x: "jet_jit_data_plot_x" => jet_jit_data_plot_x: sig_binary;
    plot_y: "jet_jit_data_plot_y" => jet_jit_data_plot_y: sig_ternary;
    plot_color: "jet_jit_data_plot_color" => jet_jit_data_plot_color: sig_binary;
    plot_size: "jet_jit_data_plot_size" => jet_jit_data_plot_size: sig_binary;
    plot_text_channel: "jet_jit_data_plot_text_channel" => jet_jit_data_plot_text_channel: sig_binary;
    plot_detail: "jet_jit_data_plot_detail" => jet_jit_data_plot_detail: sig_binary;
    plot_with_transform: "jet_jit_data_plot_with_transform" => jet_jit_data_plot_with_transform: sig_binary;
    plot_with_scale: "jet_jit_data_plot_with_scale" => jet_jit_data_plot_with_scale: sig_binary;
    plot_with_axis: "jet_jit_data_plot_with_axis" => jet_jit_data_plot_with_axis: sig_binary;
    plot_with_legend: "jet_jit_data_plot_with_legend" => jet_jit_data_plot_with_legend: sig_binary;
    plot_facet: "jet_jit_data_plot_facet" => jet_jit_data_plot_facet: sig_binary;
    plot_with_layer: "jet_jit_data_plot_with_layer" => jet_jit_data_plot_with_layer: sig_binary;
    plot_with_interaction: "jet_jit_data_plot_with_interaction" => jet_jit_data_plot_with_interaction: sig_binary;
    plot_accessibility: "jet_jit_data_plot_accessibility" => jet_jit_data_plot_accessibility: sig_binary;
    plot_layout: "jet_jit_data_plot_layout" => jet_jit_data_plot_layout: sig_binary;
    plot_select_indices: "jet_jit_data_plot_select_indices" => jet_jit_data_plot_select_indices: sig_binary;
}
