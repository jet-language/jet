// D-DATA-SURFACE1=A / D-DATA-PLOT1=A / D-DATA-STATUS1=A: the one `core.data`
// statistics kernel (I9). AOT embeds this file, and the Cranelift JIT host
// (`crates/jet-jit/src/Data.rs`) and the comptime/interpreter tier
// (`crates/jet-comptime/src/Comptime/Methods/core_calls.rs`) `include!` this
// exact source. Every tier therefore runs the same compensated arithmetic and
// reports the same `DataError`. A second copy of this math on any tier is an
// I9 violation; `tests/data_one_kernel.rs` fails the build on one.

pub(crate) fn jet_data_error(
    kind: jet_std::DataErrorKind,
    operation: &str,
    reason: impl Into<String>,
) -> jet_std::DataError {
    jet_std::DataError {
        kind,
        operation: operation.to_string(),
        row: Err(JetAbsent),
        column: Err(JetAbsent),
        index: Err(JetAbsent),
        reason: reason.into(),
        cause: Err(JetAbsent),
    }
}

pub(crate) fn jet_data_error_at(
    kind: jet_std::DataErrorKind,
    operation: &str,
    index: JetOutcome<i64, JetAbsent>,
    reason: impl Into<String>,
) -> jet_std::DataError {
    let mut error = jet_data_error(kind, operation, reason);
    error.index = index;
    error
}

pub(crate) fn jet_data_normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

pub(crate) fn jet_data_reject_nonfinite(operation: &str, values: &[f64]) -> Result<(), jet_std::DataError> {
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                operation,
                Ok(index as i64),
                "numeric input must be finite",
            ));
        }
    }
    Ok(())
}

pub(crate) fn jet_data_neumaier_sum(values: &[f64]) -> Result<f64, jet_std::DataError> {
    jet_data_reject_nonfinite("sum", values)?;
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
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "sum",
                "finite overflow while summing",
            ));
        }
    }
    let out = sum + compensation;
    if !out.is_finite() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Overflow,
            "sum",
            "finite overflow while summing",
        ));
    }
    Ok(jet_data_normalize_zero(out))
}

pub(crate) fn jet_data_sum_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Ok(0.0);
    }
    jet_data_neumaier_sum(values)
}

pub(crate) fn jet_data_mean_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "mean",
            "mean of empty data is undefined",
        ));
    }
    let sum = jet_data_neumaier_sum(values)?;
    Ok(jet_data_normalize_zero(sum / values.len() as f64))
}

pub(crate) fn jet_data_min_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "min",
            "min of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("min", values)?;
    Ok(jet_data_normalize_zero(
        values.iter().copied().fold(f64::INFINITY, f64::min),
    ))
}

pub(crate) fn jet_data_max_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "max",
            "max of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("max", values)?;
    Ok(jet_data_normalize_zero(
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ))
}

pub(crate) fn jet_data_quantile_checked(values: &Vec<f64>, q: f64) -> Result<f64, jet_std::DataError> {
    if !q.is_finite() || !(0.0..=1.0).contains(&q) {
        return Err(jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            "quantile",
            "quantile q must be a finite value in 0.0 through 1.0",
        ));
    }
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "quantile",
            "quantile of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("quantile", values)?;
    let mut sorted = values.clone();
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
    Ok(jet_data_normalize_zero(value))
}

pub(crate) fn jet_data_median_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    jet_data_quantile_checked(values, 0.5).map_err(|mut error| {
        error.operation = "median".to_string();
        error
    })
}

pub(crate) fn jet_data_variance_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "variance",
            "variance of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("variance", values)?;
    // Deterministic Welford population variance in input order.
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
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "variance",
                "finite overflow while computing variance",
            ));
        }
    }
    Ok(jet_data_normalize_zero(m2 / count))
}

pub(crate) fn jet_data_stddev_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    Ok(jet_data_normalize_zero(jet_data_variance_checked(values)?.sqrt()))
}

pub(crate) fn jet_data_describe_checked(values: &Vec<f64>) -> Result<jet_std::DataSummary, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "describe",
            "describe of empty data is undefined",
        ));
    }
    Ok(jet_std::DataSummary {
        count: values.len() as i64,
        sum: jet_data_sum_checked(values)?,
        mean: jet_data_mean_checked(values)?,
        min: jet_data_min_checked(values)?,
        max: jet_data_max_checked(values)?,
        median: jet_data_median_checked(values)?,
        variance: jet_data_variance_checked(values)?,
        stddev: jet_data_stddev_checked(values)?,
    })
}

pub(crate) fn jet_data_rolling_mean_checked(
    values: &Vec<f64>,
    width: i64,
) -> Result<Vec<f64>, jet_std::DataError> {
    if width < 1 {
        return Err(jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            "rolling_mean",
            "rolling width must be positive",
        ));
    }
    jet_data_reject_nonfinite("rolling_mean", values)?;
    let width = width as usize;
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_add(1).saturating_sub(width);
        let window = &values[start..=i];
        let mean = jet_data_neumaier_sum(window)? / window.len() as f64;
        out.push(jet_data_normalize_zero(mean));
    }
    Ok(out)
}

pub(crate) fn jet_data_bar_text_checked(
    groups: &Vec<jet_std::DataGroup>,
) -> Result<String, jet_std::DataError> {
    for (index, group) in groups.iter().enumerate() {
        if group.count < 0 {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::InvalidArgument,
                "bar_text",
                Ok(index as i64),
                "plot counts must be non-negative",
            ));
        }
        if !group.sum.is_finite() || !group.mean.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                "bar_text",
                Ok(index as i64),
                "plot values must be finite",
            ));
        }
    }
    Ok(jet_data_bar_text(groups))
}

pub(crate) fn jet_data_bar_svg_checked(groups: &Vec<jet_std::DataGroup>) -> Result<String, jet_std::DataError> {
    for (index, group) in groups.iter().enumerate() {
        if group.count < 0 {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::InvalidArgument,
                "bar_svg",
                Ok(index as i64),
                "plot counts must be non-negative",
            ));
        }
        if !group.sum.is_finite() || !group.mean.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                "bar_svg",
                Ok(index as i64),
                "plot values must be finite",
            ));
        }
    }
    Ok(jet_data_bar_svg(groups))
}

// Editions before 2027 type these calls as plain `Float`/`[Float]` instead of
// `Float ? DataError`. They run the kernel above and report an undefined
// result as `0.0` (an empty list, for `rolling_mean`) instead of an error —
// the same arithmetic, a weaker report.
pub(crate) fn jet_data_sum(values: &Vec<f64>) -> f64 {
    jet_data_sum_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_mean(values: &Vec<f64>) -> f64 {
    jet_data_mean_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_min(values: &Vec<f64>) -> f64 {
    jet_data_min_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_max(values: &Vec<f64>) -> f64 {
    jet_data_max_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_median(values: &Vec<f64>) -> f64 {
    jet_data_median_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_quantile(values: &Vec<f64>, q: f64) -> f64 {
    jet_data_quantile_checked(values, q.clamp(0.0, 1.0)).unwrap_or(0.0)
}

pub(crate) fn jet_data_variance(values: &Vec<f64>) -> f64 {
    jet_data_variance_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_stddev(values: &Vec<f64>) -> f64 {
    jet_data_stddev_checked(values).unwrap_or(0.0)
}

pub(crate) fn jet_data_rolling_mean(values: &Vec<f64>, width: i64) -> Vec<f64> {
    jet_data_rolling_mean_checked(values, width.max(1)).unwrap_or_default()
}

pub(crate) fn jet_data_describe(values: &Vec<f64>) -> jet_std::DataSummary {
    jet_data_describe_checked(values).unwrap_or(jet_std::DataSummary {
        count: values.len() as i64,
        sum: 0.0,
        mean: 0.0,
        min: 0.0,
        max: 0.0,
        median: 0.0,
        variance: 0.0,
        stddev: 0.0,
    })
}

pub(crate) fn jet_data_status_native(step: &str) -> jet_std::DataStatus {
    jet_std::DataStatus {
        step: step.to_string(),
        path: "native".to_string(),
        copy: "none".to_string(),
        ownership: "jet".to_string(),
        trust: "native".to_string(),
        fallback: "none".to_string(),
        replacement: "native".to_string(),
    }
}

pub(crate) fn jet_data_bridge_tool_on_path(tool: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(tool).is_file())
}

/// Live availability. Python binder is Planned and gpu.* is unshipped — both
/// stay unavailable even if host tools exist. R becomes `available` only when
/// Rscript is on PATH *and* the expert opt-in `JET_DATA_R_BRIDGE=1` is set
/// (keeps default goldens deterministic; tests prove the success path).
pub(crate) fn jet_data_bridge_path(step: &str) -> &'static str {
    match step {
        "r.*"
            if jet_data_bridge_tool_on_path("Rscript")
                && std::env::var("JET_DATA_R_BRIDGE").as_deref() == Ok("1") =>
        {
            "available"
        }
        _ => "unavailable",
    }
}

/// D-DATA-BRIDGE1 / D-DATA-STATUS1: one honest row per provider root.
/// Unavailable bridges keep path=`unavailable` and never claim readiness.
pub(crate) fn jet_data_bridge_status(step: &str) -> jet_std::DataStatus {
    let path = jet_data_bridge_path(step);
    match step {
        "py.*" => jet_std::DataStatus {
            step: "py.*".to_string(),
            path: path.to_string(),
            copy: "owned-copy".to_string(),
            ownership: "python-sidecar".to_string(),
            trust: "untrusted-foreign".to_string(),
            fallback: "none".to_string(),
            replacement: "core.data native table/series/stats".to_string(),
        },
        "r.*" => jet_std::DataStatus {
            step: "r.*".to_string(),
            path: path.to_string(),
            copy: "owned-copy".to_string(),
            ownership: "r-sidecar".to_string(),
            trust: "untrusted-foreign".to_string(),
            fallback: "none".to_string(),
            replacement: "core.data.Table typed round-trip".to_string(),
        },
        "gpu.*" => jet_std::DataStatus {
            step: "gpu.*".to_string(),
            path: path.to_string(),
            copy: "device-transfer".to_string(),
            ownership: "device-buffer".to_string(),
            trust: "untrusted-accelerator".to_string(),
            fallback: "none".to_string(),
            replacement: "core.data / Tensor native CPU path".to_string(),
        },
        _ => jet_std::DataStatus {
            step: step.to_string(),
            path: "unavailable".to_string(),
            copy: "unknown".to_string(),
            ownership: "unknown".to_string(),
            trust: "untrusted-foreign".to_string(),
            fallback: "none".to_string(),
            replacement: "core.data native".to_string(),
        },
    }
}

pub(crate) fn jet_data_status() -> Vec<jet_std::DataStatus> {
    vec![
        jet_data_status_native("core.data.csv"),
        jet_data_status_native("core.data.stats"),
        jet_data_status_native("core.data.table"),
        jet_data_status_native("core.data.lazy"),
        jet_data_status_native("core.data.missing"),
        jet_data_status_native("core.data.schema"),
        jet_data_status_native("core.data.json"),
        jet_data_bridge_status("py.*"),
        jet_data_bridge_status("r.*"),
        jet_data_bridge_status("gpu.*"),
    ]
}

pub(crate) fn jet_data_normalize_bridge_provider(provider: &str) -> Option<&'static str> {
    let lower = provider.trim().trim_end_matches('.').to_ascii_lowercase();
    let p = lower.strip_suffix(".*").unwrap_or(lower.as_str());
    match p {
        "py" | "python" => Some("py.*"),
        "r" => Some("r.*"),
        "gpu" | "cuda" | "metal" | "vulkan" | "webgpu" => Some("gpu.*"),
        _ => None,
    }
}

/// Fail closed when a Python/R/GPU bridge is not available. Never invent results.
pub(crate) fn jet_data_require_bridge(provider: &String) -> Result<(), jet_std::DataError> {
    let Some(step) = jet_data_normalize_bridge_provider(provider) else {
        return Err(jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            "require_bridge",
            format!("unknown data bridge provider `{provider}`; expected py, r, or gpu"),
        ));
    };
    let status = jet_data_bridge_status(step);
    if status.path == "available" {
        return Ok(());
    }
    Err(jet_data_error(
        jet_std::DataErrorKind::Bridge,
        "require_bridge",
        format!(
            "{step} unavailable (copy={}, ownership={}, trust={}, fallback={}, replacement={})",
            status.copy, status.ownership, status.trust, status.fallback, status.replacement
        ),
    ))
}

pub(crate) fn jet_data_bar_text(groups: &Vec<jet_std::DataGroup>) -> String {
    let mut lines = Vec::new();
    for g in groups {
        let n = if g.count < 0 { 0 } else { g.count.min(40) } as usize;
        lines.push(format!("{} | {} {}", g.key, "#".repeat(n), g.count));
    }
    lines.join("\n")
}

pub(crate) fn jet_data_bar_svg(groups: &Vec<jet_std::DataGroup>) -> String {
    let width = 320.0f64;
    let row_h = 24.0f64;
    let height = 24.0 + row_h * groups.len() as f64;
    let max = groups.iter().map(|g| g.count).max().unwrap_or(1).max(1) as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"{}\" viewBox=\"0 0 320 {}\">",
        height as i64,
        height as i64
    );
    out.push_str("<rect width=\"320\" height=\"100%\" fill=\"white\"/>");
    for (i, g) in groups.iter().enumerate() {
        let y = 18.0 + i as f64 * row_h;
        let bar_w = ((g.count as f64 / max) * (width - 120.0)).round();
        out.push_str(&format!(
            "<text x=\"8\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            y as i64,
            jet_data_svg_escape(&g.key)
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
    out
}

pub(crate) fn jet_data_svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
