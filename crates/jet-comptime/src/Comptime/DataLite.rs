//! Card #392 pass 3: `core.data`'s fixed-signature stats + plot surface
//! (D-DATA-SURFACE1/PLOT1/STATUS1 — `sum`/`mean`/`min`/`max`/`median`/
//! `variance`/`stddev`/`quantile`/`rolling_mean`/`describe`/`status`/
//! `bar_text`/`bar_svg`), ported verbatim from AOT's `jet_data_*` /
//! `jet_data_svg_escape` (`crates/jet-codegen/src/Prelude/CoreLib/Top/
//! EncodingTraits.rs` + `DataFmt.rs`) so comptime/REPL tier-0 matches AOT
//! byte-for-byte (R12 parity).
//!
//! Not here: none of the call-site-typed table/lazy pipeline names above —
//! those live in `DataPipeline.rs` (including `schema`).

pub(super) fn sum(values: &[f64]) -> f64 {
    values.iter().copied().sum()
}

pub(super) fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        sum(values) / values.len() as f64
    }
}

pub(super) fn min(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::min).unwrap_or(0.0)
}

pub(super) fn max(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::max).unwrap_or(0.0)
}

pub(super) fn median(values: &[f64]) -> f64 {
    quantile(values, 0.5)
}

pub(super) fn quantile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = q.clamp(0.0, 1.0);
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    }
}

pub(super) fn variance(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let m = mean(values);
    values
        .iter()
        .map(|v| {
            let d = *v - m;
            d * d
        })
        .sum::<f64>()
        / values.len() as f64
}

pub(super) fn stddev(values: &[f64]) -> f64 {
    variance(values).sqrt()
}

pub(super) fn rolling_mean(values: &[f64], width: i64) -> Vec<f64> {
    let width = width.max(1) as usize;
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_add(1).saturating_sub(width);
        let window = &values[start..=i];
        out.push(window.iter().sum::<f64>() / window.len() as f64);
    }
    out
}

/// `(step, path, replacement)` rows — mirrors `jet_data_status`'s
/// `Vec<DataStatus>` literal exactly.
pub(super) fn status_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("core.data.csv", "native", "native"),
        ("core.data.stats", "native", "native"),
        ("core.data.table", "native", "native"),
        ("core.data.lazy", "native", "native"),
        ("core.data.missing", "native", "native"),
        ("core.data.schema", "native", "native"),
        ("core.data.json", "native", "native"),
        (
            "py.* / r.* / gpu.*",
            "bridge-ready",
            "report via data.status() and jet inspect dossier data",
        ),
    ]
}

/// `(key, count)` pairs — the `bar_text`/`bar_svg` renderers only read
/// `DataGroup.key`/`.count` (never `.sum`/`.mean`), matching AOT.
pub(super) fn bar_text(groups: &[(String, i64)]) -> String {
    let mut lines = Vec::new();
    for (key, count) in groups {
        let n = if *count < 0 { 0 } else { (*count).min(40) } as usize;
        lines.push(format!("{} | {} {}", key, "#".repeat(n), count));
    }
    lines.join("\n")
}

pub(super) fn bar_svg(groups: &[(String, i64)]) -> String {
    let width = 320.0f64;
    let row_h = 24.0f64;
    let height = 24.0 + row_h * groups.len() as f64;
    let max = groups
        .iter()
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"{}\" viewBox=\"0 0 320 {}\">",
        height as i64, height as i64
    );
    out.push_str("<rect width=\"320\" height=\"100%\" fill=\"white\"/>");
    for (i, (key, count)) in groups.iter().enumerate() {
        let y = 18.0 + i as f64 * row_h;
        let bar_w = ((*count as f64 / max) * (width - 120.0)).round();
        out.push_str(&format!(
            "<text x=\"8\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            y as i64,
            svg_escape(key)
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
            count
        ));
    }
    out.push_str("</svg>");
    out
}

fn svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// `bar_text`/`bar_svg` take `[DataGroup]`, but `DataGroup` isn't a
// user-constructible type at comptime/REPL yet (only `group_count`/
// `group_sum`/`group_mean` — the still-open generic pipeline gap — produce
// one), so there's no Jet-source transcript to exercise them with; these
// unit tests check the ported function directly against AOT's exact
// literal output (`jet_data_bar_text`/`_svg` in `DataFmt.rs`) instead.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_text_matches_aot_shape() {
        let groups = vec![("a".to_string(), 3), ("b".to_string(), 1)];
        assert_eq!(bar_text(&groups), "a | ### 3\nb | # 1");
    }

    #[test]
    fn bar_text_clamps_negative_and_over_40() {
        let groups = vec![("neg".to_string(), -5), ("big".to_string(), 100)];
        assert_eq!(
            bar_text(&groups),
            format!("neg |  -5\nbig | {} 100", "#".repeat(40))
        );
    }

    #[test]
    fn bar_svg_matches_aot_shape() {
        let groups = vec![("a".to_string(), 2)];
        let svg = bar_svg(&groups);
        assert!(svg.starts_with(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"48\""
        ));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains(">a</text>"));
        assert!(svg.contains(">2</text>"));
    }

    #[test]
    fn svg_escape_matches_aot() {
        assert_eq!(
            svg_escape("<a & \"b\">"),
            "&lt;a &amp; &quot;b&quot;&gt;"
        );
    }

    #[test]
    fn stats_match_aot_formulas() {
        let xs = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(sum(&xs), 10.0);
        assert_eq!(mean(&xs), 2.5);
        assert_eq!(min(&xs), 1.0);
        assert_eq!(max(&xs), 4.0);
        assert_eq!(median(&xs), 2.5);
        assert_eq!(variance(&xs), 1.25);
        assert_eq!(stddev(&xs), 1.25f64.sqrt());
        assert_eq!(quantile(&xs, 0.25), 1.75);
        assert_eq!(rolling_mean(&xs, 2), vec![1.0, 1.5, 2.5, 3.5]);
    }

    #[test]
    fn empty_stats_are_zero_not_panics() {
        let xs: [f64; 0] = [];
        assert_eq!(sum(&xs), 0.0);
        assert_eq!(mean(&xs), 0.0);
        assert_eq!(min(&xs), 0.0);
        assert_eq!(max(&xs), 0.0);
        assert_eq!(median(&xs), 0.0);
        assert_eq!(variance(&xs), 0.0);
    }
}
