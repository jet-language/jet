//! Shared deterministic line-plot semantics for Core, the interpreter, and JIT.

#[derive(Clone, Debug, PartialEq)]
pub struct LinePoint {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineConfig {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub markers: bool,
    pub reference: f64,
    pub style: String,
    pub color: String,
    pub legend: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlotError {
    pub kind: &'static str,
    pub reason: String,
    pub index: Option<i64>,
}

fn error(kind: &'static str, reason: impl Into<String>, index: Option<i64>) -> PlotError {
    PlotError {
        kind,
        reason: reason.into(),
        index,
    }
}

fn valid_color(color: &str) -> bool {
    if color.is_empty() {
        return false;
    }
    if let Some(hex) = color.strip_prefix('#') {
        return (hex.len() == 3 || hex.len() == 6)
            && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    color
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '-')
}

fn dash_for_style(style: &str) -> Option<&'static str> {
    match style {
        "solid" => Some(""),
        "dashed" => Some("6,4"),
        "dotted" => Some("2,3"),
        _ => None,
    }
}

pub fn validate_line(points: &[LinePoint], config: &LineConfig) -> Result<(), PlotError> {
    if points.is_empty() {
        return Err(error(
            "Empty",
            "line chart needs at least one point",
            None,
        ));
    }
    if !config.reference.is_finite() {
        return Err(error(
            "NonFinite",
            "line reference must be finite",
            None,
        ));
    }
    for (index, point) in points.iter().enumerate() {
        if !point.value.is_finite() {
            return Err(error(
                "NonFinite",
                "line values must be finite",
                Some(index as i64),
            ));
        }
    }
    if dash_for_style(&config.style).is_none() {
        return Err(error(
            "InvalidArgument",
            "line style must be solid, dashed, or dotted",
            None,
        ));
    }
    if !valid_color(&config.color) {
        return Err(error(
            "InvalidArgument",
            "line color must be a named color or a 3/6-digit hex color",
            None,
        ));
    }
    Ok(())
}

fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

pub fn format_number(value: f64) -> String {
    let mut shown = format!("{:.3}", normalize_zero(value));
    while shown.contains('.') && shown.ends_with('0') {
        shown.pop();
    }
    if shown.ends_with('.') {
        shown.pop();
    }
    shown
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn render_line_text(points: &[LinePoint], config: &LineConfig) -> String {
    let mut lines = Vec::new();
    if !config.title.is_empty() {
        lines.push(format!("title: {}", config.title));
    }
    lines.push(format!("x: {}", config.x_label));
    lines.push(format!("y: {}", config.y_label));
    lines.push(format!(
        "series: style={} color={} markers={}",
        config.style, config.color, config.markers
    ));
    for point in points {
        lines.push(format!("{} | {}", point.label, format_number(point.value)));
    }
    lines.push(format!("reference: {}", format_number(config.reference)));
    if !config.legend.is_empty() {
        lines.push(format!("legend: {}", config.legend));
    }
    lines.join("\n")
}

pub fn render_line_svg(points: &[LinePoint], config: &LineConfig) -> String {
    const WIDTH: f64 = 640.0;
    const HEIGHT: f64 = 360.0;
    const LEFT: f64 = 64.0;
    const RIGHT: f64 = 24.0;
    const TOP: f64 = 42.0;
    const BOTTOM: f64 = 54.0;
    let plot_width = WIDTH - LEFT - RIGHT;
    let plot_height = HEIGHT - TOP - BOTTOM;

    let mut min = config.reference;
    let mut max = config.reference;
    for point in points {
        min = min.min(point.value);
        max = max.max(point.value);
    }
    if min == max {
        min -= 1.0;
        max += 1.0;
    }
    let range = max - min;
    let x_at = |index: usize| {
        if points.len() == 1 {
            LEFT + plot_width / 2.0
        } else {
            LEFT + plot_width * index as f64 / (points.len() - 1) as f64
        }
    };
    let y_at = |value: f64| TOP + plot_height * (max - value) / range;
    let color = if valid_color(&config.color) {
        config.color.as_str()
    } else {
        "#2f6f73"
    };
    let dash = dash_for_style(&config.style).unwrap_or("");
    let mut out = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"360\" viewBox=\"0 0 640 360\">",
    );
    out.push_str("<rect width=\"640\" height=\"360\" fill=\"white\"/>");
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#444\"/>\n",
        LEFT as i64,
        (TOP + plot_height) as i64,
        (WIDTH - RIGHT) as i64,
        (TOP + plot_height) as i64
    ));
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#444\"/>\n",
        LEFT as i64,
        TOP as i64,
        LEFT as i64,
        (TOP + plot_height) as i64
    ));
    let path = points
        .iter()
        .enumerate()
        .map(|(index, point)| format!("{:.2},{:.2}", x_at(index), y_at(point.value)))
        .collect::<Vec<_>>()
        .join(" ");
    let dash_attr = if dash.is_empty() {
        String::new()
    } else {
        format!(" stroke-dasharray=\"{dash}\"")
    };
    out.push_str(&format!(
        "<polyline points=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"{dash_attr}/>"
    ));
    if config.markers {
        for (index, point) in points.iter().enumerate() {
            out.push_str(&format!(
                "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"4\" fill=\"{color}\"/>",
                x_at(index),
                y_at(point.value)
            ));
        }
    }
    let reference_y = y_at(config.reference);
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{:.2}\" x2=\"{}\" y2=\"{:.2}\" stroke=\"#777\" stroke-dasharray=\"4,3\"/>",
        LEFT as i64,
        reference_y,
        (WIDTH - RIGHT) as i64,
        reference_y
    ));
    if !config.title.is_empty() {
        out.push_str(&format!(
            "<text x=\"320\" y=\"22\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"16\">{}</text>",
            escape_xml(&config.title)
        ));
    }
    out.push_str(&format!(
        "<text x=\"320\" y=\"350\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"12\">{}</text>",
        escape_xml(&config.x_label)
    ));
    out.push_str(&format!(
        "<text x=\"14\" y=\"200\" text-anchor=\"middle\" transform=\"rotate(-90 14 200)\" font-family=\"sans-serif\" font-size=\"12\">{}</text>",
        escape_xml(&config.y_label)
    ));
    for (index, point) in points.iter().enumerate() {
        out.push_str(&format!(
            "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"10\">{}</text>",
            x_at(index),
            HEIGHT - 36.0,
            escape_xml(&point.label)
        ));
    }
    if !config.legend.is_empty() {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"12\">{}</text>",
            (WIDTH - RIGHT - 120.0) as i64,
            (TOP + 16.0) as i64,
            escape_xml(&config.legend)
        ));
    }
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LineConfig {
        LineConfig {
            title: "Sales".to_string(),
            x_label: "Month".to_string(),
            y_label: "Value".to_string(),
            markers: true,
            reference: 2.0,
            style: "dashed".to_string(),
            color: "#2f6f73".to_string(),
            legend: "Revenue".to_string(),
        }
    }

    #[test]
    fn line_renderers_are_deterministic_and_escape_labels() {
        let points = vec![
            LinePoint {
                label: "Jan & <Q1>".to_string(),
                value: 1.0,
            },
            LinePoint {
                label: "Feb".to_string(),
                value: 3.0,
            },
        ];
        let cfg = config();
        validate_line(&points, &cfg).unwrap();
        assert_eq!(render_line_text(&points, &cfg), render_line_text(&points, &cfg));
        let svg = render_line_svg(&points, &cfg);
        assert!(svg.contains("stroke-dasharray=\"6,4\""));
        assert!(svg.contains("Jan &amp; &lt;Q1&gt;"));
        assert!(svg.contains("<circle"));
        assert!(svg.contains("stroke=\"#777\""));
    }
}
