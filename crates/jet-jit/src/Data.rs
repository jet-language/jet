//! `core.data` host shims — same checked analytics rules as DataFlow.rs.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::BTreeMap;

mod data_plot_rt {
    pub(crate) mod jet_std {
        #[derive(Clone, Debug)]
        pub(crate) struct DataGroup {
            pub(crate) key: String,
            pub(crate) count: i64,
            pub(crate) sum: f64,
            pub(crate) mean: f64,
        }

        #[derive(Clone, Debug, Default)]
        pub(crate) struct DataLineOptions {
            pub(crate) title: String,
            pub(crate) x_label: String,
            pub(crate) y_label: String,
            pub(crate) markers: bool,
            pub(crate) reference: Option<f64>,
            pub(crate) style: String,
            pub(crate) color: String,
            pub(crate) legend: String,
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/DataPlot.rs");
}

use data_plot_rt::jet_std::{DataGroup, DataLineOptions};

#[derive(Clone, Debug)]
struct DataError {
    kind: &'static str,
    operation: String,
    reason: String,
    index: Option<i64>,
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = format!("{} {}", self.kind, self.operation);
        if let Some(index) = self.index {
            out.push_str(&format!(", index {index}"));
        }
        out.push_str(&format!(": {}", self.reason));
        f.write_str(&out)
    }
}

fn err(kind: &'static str, operation: &str, reason: impl Into<String>) -> DataError {
    DataError {
        kind,
        operation: operation.into(),
        reason: reason.into(),
        index: None,
    }
}

fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn reject_nonfinite(operation: &str, values: &[f64]) -> Result<(), DataError> {
    for (i, v) in values.iter().enumerate() {
        if !v.is_finite() {
            return Err(DataError {
                kind: "NonFinite",
                operation: operation.into(),
                reason: "numeric input must be finite".into(),
                index: Some(i as i64),
            });
        }
    }
    Ok(())
}

/// Neumaier compensated sum — matches AOT `jet_data_neumaier_sum` (reports as `sum`).
fn neumaier_sum(values: &[f64]) -> Result<f64, DataError> {
    reject_nonfinite("sum", values)?;
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for value in values.iter().copied() {
        let t = sum + value;
        if sum.abs() >= value.abs() {
            compensation += (sum - t) + value;
        } else {
            compensation += (value - t) + sum;
        }
        sum = t;
        if !sum.is_finite() || !compensation.is_finite() {
            return Err(err(
                "Overflow",
                "sum",
                "finite overflow while summing",
            ));
        }
    }
    let out = sum + compensation;
    if !out.is_finite() {
        return Err(err(
            "Overflow",
            "sum",
            "finite overflow while summing",
        ));
    }
    Ok(normalize_zero(out))
}

fn mean_checked(values: &[f64]) -> Result<f64, DataError> {
    if values.is_empty() {
        return Err(err("Empty", "mean", "mean of empty data is undefined"));
    }
    let sum = neumaier_sum(values)?;
    Ok(normalize_zero(sum / values.len() as f64))
}

fn sum_checked(values: &[f64]) -> Result<f64, DataError> {
    if values.is_empty() {
        return Ok(0.0);
    }
    neumaier_sum(values)
}

fn min_checked(values: &[f64]) -> Result<f64, DataError> {
    if values.is_empty() {
        return Err(err("Empty", "min", "min of empty data is undefined"));
    }
    reject_nonfinite("min", values)?;
    Ok(normalize_zero(
        values.iter().copied().fold(f64::INFINITY, f64::min),
    ))
}

fn max_checked(values: &[f64]) -> Result<f64, DataError> {
    if values.is_empty() {
        return Err(err("Empty", "max", "max of empty data is undefined"));
    }
    reject_nonfinite("max", values)?;
    Ok(normalize_zero(
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ))
}

fn quantile_checked(values: &[f64], q: f64) -> Result<f64, DataError> {
    if !q.is_finite() || !(0.0..=1.0).contains(&q) {
        return Err(err(
            "InvalidArgument",
            "quantile",
            "quantile q must be a finite value in 0.0 through 1.0",
        ));
    }
    if values.is_empty() {
        return Err(err(
            "Empty",
            "quantile",
            "quantile of empty data is undefined",
        ));
    }
    reject_nonfinite("quantile", values)?;
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let value = if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    };
    Ok(normalize_zero(value))
}

fn median_checked(values: &[f64]) -> Result<f64, DataError> {
    quantile_checked(values, 0.5).map_err(|mut e| {
        e.operation = "median".into();
        e
    })
}

fn variance_checked(values: &[f64]) -> Result<f64, DataError> {
    if values.is_empty() {
        return Err(err(
            "Empty",
            "variance",
            "variance of empty data is undefined",
        ));
    }
    reject_nonfinite("variance", values)?;
    // Deterministic Welford population variance in input order (matches AOT).
    let mut count = 0.0f64;
    let mut mean = 0.0f64;
    let mut m2 = 0.0f64;
    for value in values.iter().copied() {
        count += 1.0;
        let delta = value - mean;
        mean += delta / count;
        let delta2 = value - mean;
        m2 += delta * delta2;
        if !mean.is_finite() || !m2.is_finite() {
            return Err(err(
                "Overflow",
                "variance",
                "finite overflow while computing variance",
            ));
        }
    }
    Ok(normalize_zero(m2 / count))
}

fn stddev_checked(values: &[f64]) -> Result<f64, DataError> {
    Ok(normalize_zero(variance_checked(values)?.sqrt()))
}

#[derive(Clone)]
struct DataSummary {
    count: i64,
    sum: f64,
    mean: f64,
    min: f64,
    max: f64,
    median: f64,
    variance: f64,
    stddev: f64,
}

fn describe_checked(values: &[f64]) -> Result<DataSummary, DataError> {
    if values.is_empty() {
        return Err(err(
            "Empty",
            "describe",
            "describe of empty data is undefined",
        ));
    }
    let variance = variance_checked(values)?;
    Ok(DataSummary {
        count: values.len() as i64,
        sum: sum_checked(values)?,
        mean: mean_checked(values)?,
        min: min_checked(values)?,
        max: max_checked(values)?,
        median: median_checked(values)?,
        variance,
        stddev: normalize_zero(variance.sqrt()),
    })
}

fn bar_text_checked(groups: &[DataGroup]) -> Result<String, DataError> {
    for (index, g) in groups.iter().enumerate() {
        if g.count < 0 {
            return Err(DataError {
                kind: "InvalidArgument",
                operation: "bar_text".into(),
                reason: "plot counts must be non-negative".into(),
                index: Some(index as i64),
            });
        }
        if !g.sum.is_finite() || !g.mean.is_finite() {
            return Err(DataError {
                kind: "NonFinite",
                operation: "bar_text".into(),
                reason: "plot values must be finite".into(),
                index: Some(index as i64),
            });
        }
    }
    let mut lines = Vec::new();
    for g in groups {
        let n = if g.count < 0 {
            0
        } else {
            g.count.min(40) as usize
        };
        lines.push(format!("{} | {} {}", g.key, "#".repeat(n), g.count));
    }
    Ok(lines.join("\n"))
}

fn svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn bar_svg_checked(groups: &[DataGroup]) -> Result<String, DataError> {
    for (index, g) in groups.iter().enumerate() {
        if g.count < 0 {
            return Err(DataError {
                kind: "InvalidArgument",
                operation: "bar_svg".into(),
                reason: "plot counts must be non-negative".into(),
                index: Some(index as i64),
            });
        }
        if !g.sum.is_finite() || !g.mean.is_finite() {
            return Err(DataError {
                kind: "NonFinite",
                operation: "bar_svg".into(),
                reason: "plot values must be finite".into(),
                index: Some(index as i64),
            });
        }
    }
    let width = 320.0f64;
    let row_h = 24.0f64;
    let height = 24.0 + row_h * groups.len() as f64;
    let max = groups.iter().map(|g| g.count).max().unwrap_or(1).max(1) as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"{}\" viewBox=\"0 0 320 {}\">",
        height as i64, height as i64
    );
    out.push_str("<rect width=\"320\" height=\"100%\" fill=\"white\"/>");
    for (i, g) in groups.iter().enumerate() {
        let y = 18.0 + i as f64 * row_h;
        let bar_w = ((g.count as f64 / max) * (width - 120.0)).round();
        out.push_str(&format!(
            "<text x=\"8\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            y as i64,
            svg_escape(&g.key)
        ));
        out.push_str(&format!(
            "<rect x=\"96\" y=\"{}\" width=\"{}\" height=\"14\" fill=\"#2f6f73\"/>",
            (y - 12.0) as i64,
            bar_w as i64
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            (104.0 + bar_w) as i64,
            y as i64,
            g.count
        ));
    }
    out.push_str("</svg>");
    Ok(out)
}

struct DataStatus {
    step: String,
    path: String,
    copy: String,
    ownership: String,
    trust: String,
    fallback: String,
    replacement: String,
}

fn status_native(step: &str) -> DataStatus {
    DataStatus {
        step: step.into(),
        path: "native".into(),
        copy: "none".into(),
        ownership: "jet".into(),
        trust: "native".into(),
        fallback: "none".into(),
        replacement: "native".into(),
    }
}

fn bridge_status(step: &str) -> DataStatus {
    match step {
        "py.*" => DataStatus {
            step: "py.*".into(),
            path: "unavailable".into(),
            copy: "owned-copy".into(),
            ownership: "python-sidecar".into(),
            trust: "untrusted-foreign".into(),
            fallback: "none".into(),
            replacement: "core.data native table/series/stats".into(),
        },
        "r.*" => DataStatus {
            step: "r.*".into(),
            path: "unavailable".into(),
            copy: "owned-copy".into(),
            ownership: "r-sidecar".into(),
            trust: "untrusted-foreign".into(),
            fallback: "none".into(),
            replacement: "core.data.Table typed round-trip".into(),
        },
        _ => DataStatus {
            step: "gpu.*".into(),
            path: "unavailable".into(),
            copy: "device-transfer".into(),
            ownership: "device-buffer".into(),
            trust: "untrusted-accelerator".into(),
            fallback: "none".into(),
            replacement: "core.data / Tensor native CPU path".into(),
        },
    }
}

fn data_status() -> Vec<DataStatus> {
    vec![
        status_native("core.data.csv"),
        status_native("core.data.stats"),
        status_native("core.data.table"),
        status_native("core.data.lazy"),
        status_native("core.data.missing"),
        status_native("core.data.schema"),
        status_native("core.data.json"),
        bridge_status("py.*"),
        bridge_status("r.*"),
        bridge_status("gpu.*"),
    ]
}

fn normalize_bridge(provider: &str) -> Option<&'static str> {
    let lower = provider.trim().trim_end_matches('.').to_ascii_lowercase();
    let p = lower.strip_suffix(".*").unwrap_or(lower.as_str());
    match p {
        "py" | "python" => Some("py.*"),
        "r" => Some("r.*"),
        "gpu" | "cuda" | "metal" | "vulkan" | "webgpu" => Some("gpu.*"),
        _ => None,
    }
}

fn require_bridge(provider: &str) -> Result<(), DataError> {
    let Some(step) = normalize_bridge(provider) else {
        return Err(err(
            "InvalidArgument",
            "require_bridge",
            format!("unknown data bridge provider `{provider}`; expected py, r, or gpu"),
        ));
    };
    let status = bridge_status(step);
    Err(err(
        "Bridge",
        "require_bridge",
        format!(
            "{step} unavailable (copy={}, ownership={}, trust={}, fallback={}, replacement={})",
            status.copy, status.ownership, status.trust, status.fallback, status.replacement
        ),
    ))
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

fn result_data_err(e: DataError) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(7);
        let kind = rt.heap.alloc_string(e.kind);
        let _ = rt.heap.record_set_string(h, 0, kind);
        let op = rt.heap.alloc_string(e.operation.clone());
        let _ = rt.heap.record_set_string(h, 1, op);
        let _ = rt.heap.record_set_int(h, 2, 0);
        let _ = rt.heap.record_set_int(h, 3, 0);
        // Store index+1 so 0 means None (matches Display omitting absent index).
        let _ = rt
            .heap
            .record_set_int(h, 4, e.index.map(|i| i + 1).unwrap_or(0));
        let reason = rt.heap.alloc_string(e.reason.clone());
        let _ = rt.heap.record_set_string(h, 5, reason);
        let _ = rt.heap.record_set_int(h, 6, 0);
        crate::runtime_host::alloc_jit_result(rt, false, h as u64)
    })
}

extern "C" fn jet_jit_data_error_show(handle: i64) -> i64 {
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
        Ok(()) => Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, 0)
        }),
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

extern "C" fn jet_jit_data_status() -> i64 {
    pack_status(data_status())
}

extern "C" fn jet_jit_data_require_bridge(provider: i64) -> i64 {
    let name =
        Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(provider).unwrap_or_default());
    result_unit(require_bridge(&name))
}

extern "C" fn jet_jit_data_stat(values: i64, op: i64) -> i64 {
    let vals = float_list(values);
    let r = match op {
        0 => mean_checked(&vals),
        1 => sum_checked(&vals),
        2 => min_checked(&vals),
        3 => max_checked(&vals),
        4 => median_checked(&vals),
        5 => variance_checked(&vals),
        _ => stddev_checked(&vals),
    };
    result_f64(r)
}

extern "C" fn jet_jit_data_quantile(values: i64, q_bits: i64) -> i64 {
    let vals = float_list(values);
    let q = f64::from_bits(q_bits as u64);
    result_f64(quantile_checked(&vals, q))
}

extern "C" fn jet_jit_data_describe(values: i64) -> i64 {
    let vals = float_list(values);
    Concurrency::with_runtime_mut(|rt| match describe_checked(&vals) {
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

fn load_groups(groups: i64) -> Vec<DataGroup> {
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
            out.push(DataGroup {
                key,
                count: rt.heap.record_get_int(h, 1).unwrap_or(0),
                sum: rt.heap.record_get_float(h, 2).unwrap_or(0.0),
                mean: rt.heap.record_get_float(h, 3).unwrap_or(0.0),
            });
        }
        out
    })
}

extern "C" fn jet_jit_data_bar_text(groups: i64) -> i64 {
    let groups = load_groups(groups);
    Concurrency::with_runtime_mut(|rt| match bar_text_checked(&groups) {
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

extern "C" fn jet_jit_data_bar_svg(groups: i64) -> i64 {
    let groups = load_groups(groups);
    Concurrency::with_runtime_mut(|rt| match bar_svg_checked(&groups) {
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
        let reference = rt.heap.record_get_int(options, 4).and_then(|raw| {
            (raw != 0).then(|| f64::from_bits(raw.wrapping_sub(1) as u64))
        });
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
    result_data_err(DataError {
        kind: error.kind,
        operation: error.operation.to_string(),
        reason: error.reason.to_string(),
        index: error.index,
    })
}

extern "C" fn jet_jit_data_line_text(groups: i64, options: i64) -> i64 {
    let groups = load_groups(groups);
    let options = load_line_options(options);
    Concurrency::with_runtime_mut(|rt| match data_plot_rt::jet_data_line_text_plot_checked(
        &groups, &options,
    ) {
        Ok(s) => {
            let sid = rt.heap.alloc_string(s);
            crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
        }
        Err(e) => {
            result_data_plot_err(e)
        }
    })
}

extern "C" fn jet_jit_data_line_svg(groups: i64, options: i64) -> i64 {
    let groups = load_groups(groups);
    let options = load_line_options(options);
    Concurrency::with_runtime_mut(|rt| match data_plot_rt::jet_data_line_svg_plot_checked(
        &groups, &options,
    ) {
        Ok(s) => {
            let sid = rt.heap.alloc_string(s);
            crate::runtime_host::alloc_jit_result(rt, true, sid as u64)
        }
        Err(e) => {
            result_data_plot_err(e)
        }
    })
}

extern "C" fn jet_jit_data_group_reduce(keys: i64, values: i64, mode: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let n = rt.heap.list_len(keys).unwrap_or(0);
        let mut map: BTreeMap<String, (i64, f64)> = BTreeMap::new();
        for i in 0..n {
            let kid = rt.heap.list_get_int(keys, i).unwrap_or(0);
            let key = rt.heap.clone_string(kid).unwrap_or_default();
            let val = if mode == 0 {
                0.0
            } else {
                rt.heap.list_get_float(values, i).unwrap_or(0.0)
            };
            let e = map.entry(key).or_insert((0, 0.0));
            e.0 += 1;
            if mode != 0 {
                e.1 += val;
            }
        }
        let mut out = Vec::new();
        for (key, (count, sum)) in map {
            let mean = if count == 0 {
                0.0
            } else {
                sum / count as f64
            };
            let h = rt.heap.alloc_record(4);
            let ks = rt.heap.alloc_string(key);
            let _ = rt.heap.record_set_string(h, 0, ks);
            let _ = rt.heap.record_set_int(h, 1, count);
            let _ = rt.heap.record_set_float(
                h,
                2,
                if mode == 0 { count as f64 } else { sum },
            );
            let _ = rt.heap.record_set_float(
                h,
                3,
                if mode == 0 { count as f64 } else { mean },
            );
            out.push(h);
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

extern "C" fn jet_jit_data_inner_join(
    left: i64,
    right: i64,
    left_keys: i64,
    right_keys: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let ln = rt.heap.list_len(left).unwrap_or(0);
        let rn = rt.heap.list_len(right).unwrap_or(0);
        let mut right_map: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for i in 0..rn {
            let kid = rt.heap.list_get_int(right_keys, i).unwrap_or(0);
            let key = rt.heap.clone_string(kid).unwrap_or_default();
            let row = rt.heap.list_get_int(right, i).unwrap_or(0);
            right_map.entry(key).or_default().push(row);
        }
        let mut out = Vec::new();
        for i in 0..ln {
            let kid = rt.heap.list_get_int(left_keys, i).unwrap_or(0);
            let key = rt.heap.clone_string(kid).unwrap_or_default();
            let left_row = rt.heap.list_get_int(left, i).unwrap_or(0);
            if let Some(matches) = right_map.get(&key) {
                for &right_row in matches {
                    let h = rt.heap.alloc_record(2);
                    let _ = rt.heap.record_set_int(h, 0, left_row);
                    let _ = rt.heap.record_set_int(h, 1, right_row);
                    out.push(h);
                }
            }
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

extern "C" fn jet_jit_data_left_join(
    left: i64,
    right: i64,
    left_keys: i64,
    right_keys: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let ln = rt.heap.list_len(left).unwrap_or(0);
        let rn = rt.heap.list_len(right).unwrap_or(0);
        let mut right_map: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for i in 0..rn {
            let kid = rt.heap.list_get_int(right_keys, i).unwrap_or(0);
            let key = rt.heap.clone_string(kid).unwrap_or_default();
            let row = rt.heap.list_get_int(right, i).unwrap_or(0);
            right_map.entry(key).or_default().push(row);
        }
        let mut out = Vec::new();
        for i in 0..ln {
            let kid = rt.heap.list_get_int(left_keys, i).unwrap_or(0);
            let key = rt.heap.clone_string(kid).unwrap_or_default();
            let left_row = rt.heap.list_get_int(left, i).unwrap_or(0);
            match right_map.get(&key) {
                Some(matches) => {
                    for &right_row in matches {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, left_row);
                        // Option<Named>: Some(x) = x + 1
                        let _ = rt.heap.record_set_int(h, 1, right_row + 1);
                        out.push(h);
                    }
                }
                None => {
                    let h = rt.heap.alloc_record(2);
                    let _ = rt.heap.record_set_int(h, 0, left_row);
                    let _ = rt.heap.record_set_int(h, 1, 0); // None
                    out.push(h);
                }
            }
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

extern "C" fn jet_jit_data_pivot_sum(row_keys: i64, col_keys: i64, values: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let n = rt.heap.list_len(row_keys).unwrap_or(0);
        let mut map: BTreeMap<(String, String), (i64, f64)> = BTreeMap::new();
        for i in 0..n {
            let rk = rt
                .heap
                .list_get_int(row_keys, i)
                .and_then(|id| rt.heap.clone_string(id))
                .unwrap_or_default();
            let ck = rt
                .heap
                .list_get_int(col_keys, i)
                .and_then(|id| rt.heap.clone_string(id))
                .unwrap_or_default();
            let v = rt.heap.list_get_float(values, i).unwrap_or(0.0);
            if !v.is_finite() {
                let e = err("NonFinite", "pivot_sum", "pivot values must be finite");
                let sid = rt.heap.alloc_string(e.to_string());
                return crate::runtime_host::alloc_jit_result(rt, false, sid as u64);
            }
            let entry = map.entry((rk, ck)).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += v;
        }
        let mut out = Vec::new();
        for ((row_key, column_key), (count, sum)) in map {
            let mean = if count == 0 {
                0.0
            } else {
                sum / count as f64
            };
            let h = rt.heap.alloc_record(5);
            let rks = rt.heap.alloc_string(row_key);
            let cks = rt.heap.alloc_string(column_key);
            let _ = rt.heap.record_set_string(h, 0, rks);
            let _ = rt.heap.record_set_string(h, 1, cks);
            let _ = rt.heap.record_set_int(h, 2, count);
            let _ = rt.heap.record_set_float(h, 3, sum);
            let _ = rt.heap.record_set_float(h, 4, mean);
            out.push(h);
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

extern "C" fn jet_jit_data_rolling_mean(values: i64, width: i64) -> i64 {
    let vals = float_list(values);
    if width < 1 {
        return result_data_err(err(
            "InvalidArgument",
            "rolling_mean",
            "rolling width must be positive",
        ));
    }
    for (index, v) in vals.iter().enumerate() {
        if !v.is_finite() {
            return result_data_err(err_at(
                "NonFinite",
                "rolling_mean",
                Some(index as i64),
                "numeric input must be finite",
            ));
        }
    }
    let width = width as usize;
    let mut out = Vec::with_capacity(vals.len());
    for i in 0..vals.len() {
        let start = i.saturating_add(1).saturating_sub(width);
        let window = &vals[start..=i];
        match neumaier_sum(window) {
            Ok(sum) => out.push(normalize_zero(sum / window.len() as f64)),
            Err(e) => {
                let mut e = e;
                e.operation = "rolling_mean".into();
                return result_data_err(e);
            }
        }
    }
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for v in out {
            let _ = rt.heap.list_push_float(list, v);
        }
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

extern "C" fn jet_jit_data_missing_count(series: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let values = rt.heap.record_get_int(series, 0).unwrap_or(0);
        let stored = rt.heap.record_get_int(series, 1).unwrap_or(0);
        let n = rt.heap.list_len(values).unwrap_or(0);
        let mut missing = stored;
        for i in 0..n {
            // Option payload: None == 0
            let v = rt.heap.list_get_int(values, i).unwrap_or(0);
            if v == 0 {
                missing += 1;
            }
        }
        missing
    })
}

// ---- LazyFrame deferred ops (FuncId resolved after finalize) ----

use std::sync::Mutex;

static LAZY_PENDING: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static LAZY_RESOLVED: Mutex<BTreeMap<u32, usize>> = Mutex::new(BTreeMap::new());
static LAZY_FN_TABLE: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static LAZY_FRAME_OPS: Mutex<BTreeMap<i64, Vec<(u8, u32)>>> = Mutex::new(BTreeMap::new());

pub(crate) fn note_lazy_callable(func_id: FuncId) {
    let id = func_id.as_u32();
    if let Ok(mut pending) = LAZY_PENDING.lock() {
        pending.push(id);
    }
}

pub(crate) fn bind_lazy_callables(module: &JITModule) {
    use cranelift_module::Module;
    let pending = LAZY_PENDING
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default();
    let mut table = LAZY_FN_TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let mut resolved = LAZY_RESOLVED.lock().unwrap_or_else(|e| e.into_inner());
    for fid in pending {
        if resolved.contains_key(&fid) {
            continue;
        }
        let ptr = module.get_finalized_function(FuncId::from_u32(fid));
        let idx = table.len();
        table.push(ptr as usize);
        resolved.insert(fid, idx);
    }
}

pub(crate) fn clear_lazy_state() {
    let _ = LAZY_PENDING.lock().map(|mut p| p.clear());
    let _ = LAZY_RESOLVED.lock().map(|mut p| p.clear());
    let _ = LAZY_FN_TABLE.lock().map(|mut p| p.clear());
    let _ = LAZY_FRAME_OPS.lock().map(|mut p| p.clear());
}

fn clone_frame_ops(src: i64, dst: i64) {
    if let Ok(mut ops) = LAZY_FRAME_OPS.lock() {
        if let Some(v) = ops.get(&src).cloned() {
            ops.insert(dst, v);
        }
    }
}

extern "C" fn jet_jit_data_lazy_push_op(frame: i64, kind: i64, func_id: i64) -> i64 {
    note_lazy_callable(FuncId::from_u32(func_id as u32));
    if let Ok(mut ops) = LAZY_FRAME_OPS.lock() {
        ops.entry(frame).or_default().push((kind as u8, func_id as u32));
    }
    frame
}

extern "C" fn jet_jit_data_lazy_clone_ops(src: i64, dst: i64) -> i64 {
    clone_frame_ops(src, dst);
    dst
}

fn materialize_lazy_rows(rt: &mut crate::JitRuntime, frame: i64) -> Result<i64, DataError> {
    let rows = rt.heap.record_get_int(frame, 0).unwrap_or(0);
    let ops = LAZY_FRAME_OPS
        .lock()
        .ok()
        .and_then(|m| m.get(&frame).cloned())
        .unwrap_or_default();
    if ops.is_empty() {
        return Ok(rows);
    }
    // Apply deferred ops via finalized callables.
    let resolved = match LAZY_RESOLVED.lock() {
        Ok(g) => g,
        Err(_) => {
            return Err(err("State", "collect", "lazy resolve lock poisoned"));
        }
    };
    let table = match LAZY_FN_TABLE.lock() {
        Ok(g) => g,
        Err(_) => {
            return Err(err("State", "collect", "lazy fn table lock poisoned"));
        }
    };
    let mut cur = rt
        .heap
        .clone_int_list(rows)
        .unwrap_or_default();
    for (kind, fid) in ops {
        let Some(&idx) = resolved.get(&fid) else {
            return Err(err(
                "State",
                "collect",
                format!("lazy callable {fid} was not finalized"),
            ));
        };
        if idx >= table.len() {
            return Err(err(
                "State",
                "collect",
                format!("lazy callable index {idx} out of range"),
            ));
        }
        let ptr = table[idx] as *const u8;
        if ptr.is_null() {
            return Err(err("State", "collect", "lazy callable null"));
        }
        if kind == 0 {
            type Pred = unsafe extern "C" fn(i64) -> i8;
            let pred: Pred = unsafe { std::mem::transmute(ptr) };
            cur.retain(|&row| unsafe { pred(row) } != 0);
        } else {
            type KeyFn = unsafe extern "C" fn(i64) -> i64;
            let key_fn: KeyFn = unsafe { std::mem::transmute(ptr) };
            let mut keyed: Vec<(String, i64)> = cur
                .iter()
                .map(|&row| {
                    let kid = unsafe { key_fn(row) };
                    let s = rt.heap.clone_string(kid).unwrap_or_default();
                    (s, row)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            cur = keyed.into_iter().map(|(_, r)| r).collect();
        }
    }
    Ok(rt.heap.alloc_int_list(cur))
}

extern "C" fn jet_jit_data_lazy_count(frame: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rows = rt.heap.record_get_int(frame, 0).unwrap_or(0);
        rt.heap.list_len(rows).unwrap_or(0)
    })
}

extern "C" fn jet_jit_data_collect(frame: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rows = rt.heap.record_get_int(frame, 0).unwrap_or(0);
        let missing = rt.heap.record_get_int(frame, 1).unwrap_or(0);
        let plan = rt.heap.record_get_int(frame, 2).unwrap_or(0);
        let plan = rt.heap.clone_int_list(plan).unwrap_or_default();
        let mut plan = plan;
        let collect_s = rt.heap.alloc_string("collect");
        plan.push(collect_s);
        let plan_h = rt.heap.alloc_int_list(plan);
        // Clone rows so collect owns a stable snapshot.
        let rows = rt.heap.clone_int_list(rows).unwrap_or_default();
        let rows = rt.heap.alloc_int_list(rows);
        let rec = rt.heap.alloc_record(3);
        let _ = rt.heap.record_set_int(rec, 0, rows);
        let _ = rt.heap.record_set_int(rec, 1, missing);
        let _ = rt.heap.record_set_int(rec, 2, plan_h);
        crate::runtime_host::alloc_jit_result(rt, true, rec as u64)
    })
}

/// Typed pull stream for `core.data.csv_reader` — rows are already-decoded
/// record handles (Event: service:String, latency_ms:Float for current stems).
pub(crate) struct DataStreamSlot {
    rows: Vec<i64>,
    index: usize,
    max_groups: i64,
}

fn option_bits(opt: Option<i64>) -> u64 {
    match opt {
        None => 0,
        Some(v) => (v as u64).wrapping_add(1),
    }
}

/// Decode CSV `service,latency_ms` rows into Event records.
extern "C" fn jet_jit_data_csv_reader(file: i64, encoding: i64, max_groups: i64) -> i64 {
    let csv = crate::enc_stream::jet_jit_csv_reader(file, encoding);
    let (ok, handle) = Concurrency::with_runtime_mut(|rt| {
        let ok = crate::runtime_host::jit_result_is_ok(rt, csv).unwrap_or(false);
        let h = crate::runtime_host::jit_result_i64(rt, csv).unwrap_or(0);
        (ok, h)
    });
    if !ok {
        return csv;
    }
    let mut decoded = Vec::new();
    let mut saw_header = false;
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
            let list = (bits as u64).wrapping_sub(1) as i64;
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
        if !saw_header {
            saw_header = true;
            continue;
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
        });
        let h = rt.data_streams.len() as i64;
        crate::runtime_host::alloc_jit_result(rt, true, h as u64)
    })
}

extern "C" fn jet_jit_data_stream_next(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = match (handle as usize).checked_sub(1) {
            Some(i) => i,
            None => {
                let e = err("InvalidArgument", "next", "bad DataStream");
                let h = rt.heap.alloc_record(7);
                let kind = rt.heap.alloc_string(e.kind);
                let _ = rt.heap.record_set_string(h, 0, kind);
                let op = rt.heap.alloc_string(e.operation);
                let _ = rt.heap.record_set_string(h, 1, op);
                let _ = rt.heap.record_set_int(h, 2, 0);
                let _ = rt.heap.record_set_int(h, 3, 0);
                let _ = rt.heap.record_set_int(h, 4, 0);
                let reason = rt.heap.alloc_string(e.reason);
                let _ = rt.heap.record_set_string(h, 5, reason);
                let _ = rt.heap.record_set_int(h, 6, 0);
                return crate::runtime_host::alloc_jit_result(rt, false, h as u64);
            }
        };
        let Some(stream) = rt.data_streams.get_mut(idx) else {
            let e = err("InvalidArgument", "next", "bad DataStream");
            let h = rt.heap.alloc_record(7);
            let kind = rt.heap.alloc_string(e.kind);
            let _ = rt.heap.record_set_string(h, 0, kind);
            let op = rt.heap.alloc_string(e.operation);
            let _ = rt.heap.record_set_string(h, 1, op);
            let _ = rt.heap.record_set_int(h, 2, 0);
            let _ = rt.heap.record_set_int(h, 3, 0);
            let _ = rt.heap.record_set_int(h, 4, 0);
            let reason = rt.heap.alloc_string(e.reason);
            let _ = rt.heap.record_set_string(h, 5, reason);
            let _ = rt.heap.record_set_int(h, 6, 0);
            return crate::runtime_host::alloc_jit_result(rt, false, h as u64);
        };
        if stream.index >= stream.rows.len() {
            return crate::runtime_host::alloc_jit_result(rt, true, 0);
        }
        let row = stream.rows[stream.index];
        stream.index += 1;
        crate::runtime_host::alloc_jit_result(rt, true, option_bits(Some(row)))
    })
}

extern "C" fn jet_jit_data_stream_rest(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = match (handle as usize).checked_sub(1) {
            Some(i) => i,
            None => {
                let e = err("InvalidArgument", "group_mean", "bad DataStream");
                let h = rt.heap.alloc_record(7);
                let kind = rt.heap.alloc_string(e.kind);
                let _ = rt.heap.record_set_string(h, 0, kind);
                let op = rt.heap.alloc_string(e.operation);
                let _ = rt.heap.record_set_string(h, 1, op);
                let _ = rt.heap.record_set_int(h, 2, 0);
                let _ = rt.heap.record_set_int(h, 3, 0);
                let _ = rt.heap.record_set_int(h, 4, 0);
                let reason = rt.heap.alloc_string(e.reason);
                let _ = rt.heap.record_set_string(h, 5, reason);
                let _ = rt.heap.record_set_int(h, 6, 0);
                return crate::runtime_host::alloc_jit_result(rt, false, h as u64);
            }
        };
        let Some(stream) = rt.data_streams.get_mut(idx) else {
            let e = err("InvalidArgument", "group_mean", "bad DataStream");
            let h = rt.heap.alloc_record(7);
            let kind = rt.heap.alloc_string(e.kind);
            let _ = rt.heap.record_set_string(h, 0, kind);
            let op = rt.heap.alloc_string(e.operation);
            let _ = rt.heap.record_set_string(h, 1, op);
            let _ = rt.heap.record_set_int(h, 2, 0);
            let _ = rt.heap.record_set_int(h, 3, 0);
            let _ = rt.heap.record_set_int(h, 4, 0);
            let reason = rt.heap.alloc_string(e.reason);
            let _ = rt.heap.record_set_string(h, 5, reason);
            let _ = rt.heap.record_set_int(h, 6, 0);
            return crate::runtime_host::alloc_jit_result(rt, false, h as u64);
        };
        let rest = stream.rows[stream.index..].to_vec();
        stream.index = stream.rows.len();
        let list = rt.heap.alloc_int_list(rest);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

extern "C" fn jet_jit_data_stream_max_groups(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = match (handle as usize).checked_sub(1) {
            Some(i) => i,
            None => return 0,
        };
        rt.data_streams
            .get(idx)
            .map(|s| s.max_groups)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_data_group_reduce_limited(keys: i64, values: i64, max_groups: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let n = rt.heap.list_len(keys).unwrap_or(0);
        let mut map: BTreeMap<String, (i64, f64)> = BTreeMap::new();
        for i in 0..n {
            let kid = rt.heap.list_get_int(keys, i).unwrap_or(0);
            let key = rt.heap.clone_string(kid).unwrap_or_default();
            let val = rt.heap.list_get_float(values, i).unwrap_or(0.0);
            if !val.is_finite() {
                let e = err_at(
                    "NonFinite",
                    "group_mean",
                    Some(i),
                    "group values must be finite",
                );
                return result_data_err(e);
            }
            if !map.contains_key(&key) && map.len() as i64 >= max_groups {
                return result_data_err(err(
                    "Limit",
                    "group_mean",
                    format!("max_groups {max_groups} exceeded"),
                ));
            }
            let e = map.entry(key).or_insert((0, 0.0));
            e.0 += 1;
            e.1 += val;
            if !e.1.is_finite() {
                return result_data_err(err(
                    "Overflow",
                    "group_mean",
                    "finite overflow while grouping",
                ));
            }
        }
        let mut out = Vec::new();
        for (key, (count, sum)) in map {
            let mean = if count == 0 {
                0.0
            } else {
                sum / count as f64
            };
            let h = rt.heap.alloc_record(4);
            let ks = rt.heap.alloc_string(key);
            let _ = rt.heap.record_set_string(h, 0, ks);
            let _ = rt.heap.record_set_int(h, 1, count);
            let _ = rt.heap.record_set_float(h, 2, sum);
            let _ = rt.heap.record_set_float(h, 3, mean);
            out.push(h);
        }
        let list = rt.heap.alloc_int_list(out);
        crate::runtime_host::alloc_jit_result(rt, true, list as u64)
    })
}

fn err_at(kind: &'static str, op: &str, index: Option<i64>, reason: impl Into<String>) -> DataError {
    let mut e = err(kind, op, reason);
    e.index = index;
    e
}

pub(crate) struct DataHostFns {
    pub status: FuncId,
    pub require_bridge: FuncId,
    pub stat: FuncId,
    pub quantile: FuncId,
    pub describe: FuncId,
    pub bar_text: FuncId,
    pub bar_svg: FuncId,
    pub line_text: FuncId,
    pub line_svg: FuncId,
    pub group_reduce: FuncId,
    pub group_reduce_limited: FuncId,
    pub error_show: FuncId,
    pub inner_join: FuncId,
    pub left_join: FuncId,
    pub pivot_sum: FuncId,
    pub rolling_mean: FuncId,
    pub missing_count: FuncId,
    pub lazy_push_op: FuncId,
    pub lazy_clone_ops: FuncId,
    pub lazy_count: FuncId,
    pub collect: FuncId,
    pub csv_reader: FuncId,
    pub stream_next: FuncId,
    pub stream_rest: FuncId,
    pub stream_max_groups: FuncId,
}

pub(crate) fn register_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_data_status", jet_jit_data_status as *const u8);
    builder.symbol(
        "jet_jit_data_require_bridge",
        jet_jit_data_require_bridge as *const u8,
    );
    builder.symbol("jet_jit_data_stat", jet_jit_data_stat as *const u8);
    builder.symbol("jet_jit_data_quantile", jet_jit_data_quantile as *const u8);
    builder.symbol("jet_jit_data_describe", jet_jit_data_describe as *const u8);
    builder.symbol("jet_jit_data_bar_text", jet_jit_data_bar_text as *const u8);
    builder.symbol("jet_jit_data_bar_svg", jet_jit_data_bar_svg as *const u8);
    builder.symbol("jet_jit_data_line_text", jet_jit_data_line_text as *const u8);
    builder.symbol("jet_jit_data_line_svg", jet_jit_data_line_svg as *const u8);
    builder.symbol(
        "jet_jit_data_group_reduce",
        jet_jit_data_group_reduce as *const u8,
    );
    builder.symbol(
        "jet_jit_data_group_reduce_limited",
        jet_jit_data_group_reduce_limited as *const u8,
    );
    builder.symbol(
        "jet_jit_data_error_show",
        jet_jit_data_error_show as *const u8,
    );
    builder.symbol(
        "jet_jit_data_inner_join",
        jet_jit_data_inner_join as *const u8,
    );
    builder.symbol("jet_jit_data_left_join", jet_jit_data_left_join as *const u8);
    builder.symbol("jet_jit_data_pivot_sum", jet_jit_data_pivot_sum as *const u8);
    builder.symbol(
        "jet_jit_data_rolling_mean",
        jet_jit_data_rolling_mean as *const u8,
    );
    builder.symbol(
        "jet_jit_data_missing_count",
        jet_jit_data_missing_count as *const u8,
    );
    builder.symbol(
        "jet_jit_data_lazy_push_op",
        jet_jit_data_lazy_push_op as *const u8,
    );
    builder.symbol(
        "jet_jit_data_lazy_clone_ops",
        jet_jit_data_lazy_clone_ops as *const u8,
    );
    builder.symbol(
        "jet_jit_data_lazy_count",
        jet_jit_data_lazy_count as *const u8,
    );
    builder.symbol("jet_jit_data_collect", jet_jit_data_collect as *const u8);
    builder.symbol(
        "jet_jit_data_csv_reader",
        jet_jit_data_csv_reader as *const u8,
    );
    builder.symbol(
        "jet_jit_data_stream_next",
        jet_jit_data_stream_next as *const u8,
    );
    builder.symbol(
        "jet_jit_data_stream_rest",
        jet_jit_data_stream_rest as *const u8,
    );
    builder.symbol(
        "jet_jit_data_stream_max_groups",
        jet_jit_data_stream_max_groups as *const u8,
    );
}

pub(crate) fn declare(module: &mut JITModule) -> Result<DataHostFns, String> {
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
    let mut import = |name: &str, sig: &Signature| -> Result<FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(DataHostFns {
        status: import("jet_jit_data_status", &sig_void)?,
        require_bridge: import("jet_jit_data_require_bridge", &sig_unary)?,
        stat: import("jet_jit_data_stat", &sig_binary)?,
        quantile: import("jet_jit_data_quantile", &sig_binary)?,
        describe: import("jet_jit_data_describe", &sig_unary)?,
        bar_text: import("jet_jit_data_bar_text", &sig_unary)?,
        bar_svg: import("jet_jit_data_bar_svg", &sig_unary)?,
        line_text: import("jet_jit_data_line_text", &sig_binary)?,
        line_svg: import("jet_jit_data_line_svg", &sig_binary)?,
        group_reduce: import("jet_jit_data_group_reduce", &sig_ternary)?,
        group_reduce_limited: import("jet_jit_data_group_reduce_limited", &sig_ternary)?,
        error_show: import("jet_jit_data_error_show", &sig_unary)?,
        inner_join: import("jet_jit_data_inner_join", &sig_quaternary)?,
        left_join: import("jet_jit_data_left_join", &sig_quaternary)?,
        pivot_sum: import("jet_jit_data_pivot_sum", &sig_ternary)?,
        rolling_mean: import("jet_jit_data_rolling_mean", &sig_binary)?,
        missing_count: import("jet_jit_data_missing_count", &sig_unary)?,
        lazy_push_op: import("jet_jit_data_lazy_push_op", &sig_ternary)?,
        lazy_clone_ops: import("jet_jit_data_lazy_clone_ops", &sig_binary)?,
        lazy_count: import("jet_jit_data_lazy_count", &sig_unary)?,
        collect: import("jet_jit_data_collect", &sig_unary)?,
        csv_reader: import("jet_jit_data_csv_reader", &sig_ternary)?,
        stream_next: import("jet_jit_data_stream_next", &sig_unary)?,
        stream_rest: import("jet_jit_data_stream_rest", &sig_unary)?,
        stream_max_groups: import("jet_jit_data_stream_max_groups", &sig_unary)?,
    })
}
