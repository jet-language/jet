#[derive(Clone, Debug)]
pub(crate) struct DataPlotError {
    pub(crate) kind: &'static str,
    pub(crate) operation: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) index: Option<i64>,
}

impl std::fmt::Display for DataPlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.kind, self.operation)?;
        if let Some(index) = self.index {
            write!(f, ", index {index}")?;
        }
        write!(f, ": {}", self.reason)
    }
}

fn jet_data_line_values(groups: &Vec<jet_std::DataGroup>) -> Vec<f64> {
    groups.iter().map(|group| group.mean).collect()
}

fn jet_data_line_options_validate(
    options: &jet_std::DataLineOptions,
    operation: &'static str,
) -> Result<(), DataPlotError> {
    if let Ok(reference) = options.reference {
        if !reference.is_finite() {
            return Err(DataPlotError {
                kind: "NonFinite",
                operation,
                reason: "reference line must be finite",
                index: None,
            });
        }
    }
    if options.style != "solid" && options.style != "dashed" && options.style != "dotted" {
        return Err(DataPlotError {
            kind: "InvalidArgument",
            operation,
            reason: "line style must be solid, dashed, or dotted",
            index: None,
        });
    }
    if options.color.is_empty() {
        return Err(DataPlotError {
            kind: "InvalidArgument",
            operation,
            reason: "line color must not be empty",
            index: None,
        });
    }
    Ok(())
}

fn jet_data_line_validate(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
    operation: &'static str,
) -> Result<(), DataPlotError> {
    for (index, group) in groups.iter().enumerate() {
        if group.count < 0 {
            return Err(DataPlotError {
                kind: "InvalidArgument",
                operation,
                reason: "plot counts must be non-negative",
                index: Some(index as i64),
            });
        }
        if !group.mean.is_finite() {
            return Err(DataPlotError {
                kind: "NonFinite",
                operation,
                reason: "plot values must be finite",
                index: Some(index as i64),
            });
        }
    }
    jet_data_line_options_validate(options, operation)
}

pub(crate) fn jet_data_line_text_plot_checked(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
) -> Result<String, DataPlotError> {
    jet_data_line_validate(groups, options, "line_text")?;
    Ok(jet_data_line_text(groups, options))
}

pub(crate) fn jet_data_line_svg_plot_checked(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
) -> Result<String, DataPlotError> {
    jet_data_line_validate(groups, options, "line_svg")?;
    Ok(jet_data_line_svg(groups, options))
}

pub(crate) fn jet_data_line_text(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
) -> String {
    let values = jet_data_line_values(groups);
    let labels = groups
        .iter()
        .map(|group| group.key.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    let points = values
        .iter()
        .map(|value| format!("{value:.6}"))
        .collect::<Vec<_>>()
        .join(" -> ");
    let mut lines = Vec::new();
    if !options.title.is_empty() {
        lines.push(options.title.clone());
    }
    lines.push(format!("{}: {}", options.x_label, labels));
    lines.push(format!("{}: {}", options.y_label, points));
    lines.push(format!(
        "line: style={} color={} markers={}",
        options.style,
        options.color,
        if options.markers { "on" } else { "off" }
    ));
    if let Ok(reference) = options.reference {
        lines.push(format!("reference: {reference:.6}"));
    }
    if !options.legend.is_empty() {
        lines.push(format!("legend: {}", options.legend));
    }
    lines.join("\n")
}

pub(crate) fn jet_data_line_svg(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
) -> String {
    let width = 640.0f64;
    let height = 360.0f64;
    let left = 64.0f64;
    let right = 24.0f64;
    let top = 44.0f64;
    let bottom = 52.0f64;
    let plot_width = width - left - right;
    let plot_height = height - top - bottom;
    let values = jet_data_line_values(groups);
    let mut min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let mut max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if let Ok(reference) = options.reference {
        min = min.min(reference);
        max = max.max(reference);
    }
    if !min.is_finite() || !max.is_finite() {
        min = 0.0;
        max = 1.0;
    }
    if (max - min).abs() < f64::EPSILON {
        min -= 1.0;
        max += 1.0;
    }
    let point = |index: usize, value: f64| {
        let x = if groups.len() <= 1 {
            left + plot_width / 2.0
        } else {
            left + plot_width * index as f64 / (groups.len() - 1) as f64
        };
        let y = top + (max - value) / (max - min) * plot_height;
        (x, y)
    };
    let points = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let (x, y) = point(index, *value);
            format!("{x:.3},{y:.3}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let dash = match options.style.as_str() {
        "dashed" => " stroke-dasharray=\"8 5\"",
        "dotted" => " stroke-dasharray=\"2 5\"",
        _ => "",
    };
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"360\" viewBox=\"0 0 640 360\"><title>{}</title>",
        jet_data_plot_svg_escape(&options.title)
    );
    out.push_str("<rect width=\"640\" height=\"360\" fill=\"white\"/>");
    out.push_str(&format!(
        "<line x1=\"{left}\" y1=\"{top}\" x2=\"{left}\" y2=\"{}\" stroke=\"#333\"/><line x1=\"{left}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333\"/>",
        height - bottom,
        height - bottom,
        width - right,
        height - bottom
    ));
    if let Ok(reference) = options.reference {
        let (_, y) = point(0, reference);
        out.push_str(&format!(
            "<line x1=\"{left}\" y1=\"{y:.3}\" x2=\"{}\" y2=\"{y:.3}\" stroke=\"#888\" stroke-dasharray=\"4 4\"/>",
            width - right
        ));
    }
    out.push_str(&format!(
        "<polyline fill=\"none\" stroke=\"{}\" stroke-width=\"2\"{} points=\"{}\"/>",
        jet_data_plot_svg_escape(&options.color),
        dash,
        points
    ));
    if options.markers {
        for (index, value) in values.iter().enumerate() {
            let (x, y) = point(index, *value);
            out.push_str(&format!(
                "<circle cx=\"{x:.3}\" cy=\"{y:.3}\" r=\"4\" fill=\"{}\"/>",
                jet_data_plot_svg_escape(&options.color)
            ));
        }
    }
    for (index, group) in groups.iter().enumerate() {
        let (x, _) = point(index, values[index]);
        out.push_str(&format!(
            "<text x=\"{x:.3}\" y=\"{}\" text-anchor=\"middle\" font-family=\"monospace\" font-size=\"11\">{}</text>",
            height - bottom + 18.0,
            jet_data_plot_svg_escape(&group.key)
        ));
    }
    out.push_str(&format!(
        "<text x=\"{}\" y=\"24\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"16\">{}</text>",
        width / 2.0,
        jet_data_plot_svg_escape(&options.title)
    ));
    out.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"12\">{}</text>",
        width / 2.0,
        height - 10.0,
        jet_data_plot_svg_escape(&options.x_label)
    ));
    out.push_str(&format!(
        "<text x=\"16\" y=\"{}\" text-anchor=\"middle\" transform=\"rotate(-90 16 {})\" font-family=\"sans-serif\" font-size=\"12\">{}</text>",
        height / 2.0,
        height / 2.0,
        jet_data_plot_svg_escape(&options.y_label)
    ));
    if !options.legend.is_empty() {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"12\" fill=\"{}\">{}</text>",
            width - right - 120.0,
            top - 12.0,
            jet_data_plot_svg_escape(&options.color),
            jet_data_plot_svg_escape(&options.legend)
        ));
    }
    out.push_str("</svg>");
    out
}

fn jet_data_plot_svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
