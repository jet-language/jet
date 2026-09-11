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

fn jet_data_line_values<K>(groups: &Vec<jet_std::GroupValue<K, f64>>) -> Vec<f64> {
    groups.iter().map(|group| group.value).collect()
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

fn jet_data_line_validate<K: crate::JetShow>(
    groups: &Vec<jet_std::GroupValue<K, f64>>,
    options: &jet_std::DataLineOptions,
    operation: &'static str,
) -> Result<(), DataPlotError> {
    for (index, group) in groups.iter().enumerate() {
        if !group.value.is_finite() {
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

pub(crate) fn jet_data_line_text_plot_checked<K: crate::JetShow>(
    groups: &Vec<jet_std::GroupValue<K, f64>>,
    options: &jet_std::DataLineOptions,
) -> Result<String, DataPlotError> {
    jet_data_line_validate(groups, options, "line_text")?;
    Ok(jet_data_line_text(groups, options))
}

pub(crate) fn jet_data_line_svg_plot_checked<K: crate::JetShow>(
    groups: &Vec<jet_std::GroupValue<K, f64>>,
    options: &jet_std::DataLineOptions,
) -> Result<String, DataPlotError> {
    jet_data_line_validate(groups, options, "line_svg")?;
    Ok(jet_data_line_svg(groups, options))
}

pub(crate) fn jet_data_line_text<K: crate::JetShow>(
    groups: &Vec<jet_std::GroupValue<K, f64>>,
    options: &jet_std::DataLineOptions,
) -> String {
    let values = jet_data_line_values(groups);
    let labels = groups
        .iter()
        .map(|group| group.key.jet_show())
        .collect::<Vec<_>>()
        .join(" | ");
    let points = values
        .iter()
        .map(|value| {
            let value = jet_data_plot_normalize_zero(*value);
            format!("{value:.6}")
        })
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
        let reference = jet_data_plot_normalize_zero(reference);
        lines.push(format!("reference: {reference:.6}"));
    }
    if !options.legend.is_empty() {
        lines.push(format!("legend: {}", options.legend));
    }
    lines.join("\n")
}

pub(crate) fn jet_data_line_svg<K: crate::JetShow>(
    groups: &Vec<jet_std::GroupValue<K, f64>>,
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
            jet_data_plot_svg_escape(&group.key.jet_show())
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
// D-DX-PLOT1=A: one typed, headless grammar shared by every plot adapter.
//
// The plan owns only schema identities and declarative facts.  Row accessors
// stay on the typed `JetDataPlot<T>` handle and are evaluated only while an
// adapter renders a projection.  This keeps inspection source-backed and
// avoids a second untyped carrier or a renderer-specific semantic sidecar.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc as JetDataPlotArc;

const JET_DATA_PLOT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotMark {
    Line,
    Bar,
    Point,
}

impl JetDataPlotMark {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Bar => "bar",
            Self::Point => "point",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotChannel {
    X,
    Y,
    Color,
    Size,
    Text,
    Detail,
}

impl JetDataPlotChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Color => "color",
            Self::Size => "size",
            Self::Text => "text",
            Self::Detail => "detail",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotAggregate {
    None,
    Count,
    Sum,
    Mean,
    Min,
    Max,
}

impl JetDataPlotAggregate {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Mean => "mean",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    fn needs_numeric(self) -> bool {
        matches!(self, Self::Sum | Self::Mean | Self::Min | Self::Max)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotFilterOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl JetDataPlotFilterOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "eq",
            Self::NotEqual => "ne",
            Self::Less => "lt",
            Self::LessEqual => "lte",
            Self::Greater => "gt",
            Self::GreaterEqual => "gte",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetDataPlotValue {
    Text(String),
    Integer(i64),
    Number(f64),
    Boolean(bool),
    Null,
}

impl JetDataPlotValue {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "String",
            Self::Integer(_) => "Int",
            Self::Number(_) => "Float",
            Self::Boolean(_) => "Bool",
            Self::Null => "Null",
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    fn canonical_key(&self) -> String {
        match self {
            Self::Text(value) => format!("text:{}", jet_data_plot_escape(value)),
            Self::Integer(value) => format!("int:{value}"),
            Self::Number(value) => format!("float:{}", jet_data_plot_number(*value)),
            Self::Boolean(value) => format!("bool:{value}"),
            Self::Null => "null".to_string(),
        }
    }

    fn display_text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Number(value) => jet_data_plot_number(*value),
            Self::Boolean(value) => value.to_string(),
            Self::Null => "null".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotField {
    pub id: String,
    pub name: String,
    pub type_name: String,
}

impl JetDataPlotField {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        let name = name.into();
        let type_name = type_name.into();
        let id = jet_foundation::PreludeDataFlow::column_identity(&name, &type_name);
        Self {
            id,
            name,
            type_name,
        }
    }

    pub fn with_id(
        id: impl Into<String>,
        name: impl Into<String>,
        type_name: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            type_name: type_name.into(),
        }
    }

    pub fn validate(&self) -> Result<(), JetDataPlotError> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(JetDataPlotError::invalid(
                "plot.schema",
                "plot column identity and name must not be empty",
            )
            .with_field(self.name.clone()));
        }
        if self
            .id
            .chars()
            .chain(self.name.chars())
            .chain(self.type_name.chars())
            .any(char::is_control)
        {
            return Err(JetDataPlotError::invalid(
                "plot.schema",
                "plot column metadata must not contain control characters",
            )
            .with_field(self.name.clone()));
        }
        if self.type_name.trim().is_empty() {
            return Err(JetDataPlotError::invalid(
                "plot.schema",
                "plot column type must not be empty",
            )
            .with_field(self.name.clone()));
        }
        Ok(())
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}",
            jet_data_plot_escape(&self.id),
            jet_data_plot_escape(&self.name),
            jet_data_plot_escape(&self.type_name)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotSchema {
    pub identity: String,
    pub row_type: String,
    pub columns: Vec<JetDataPlotField>,
}

impl JetDataPlotSchema {
    pub fn new(
        identity: impl Into<String>,
        row_type: impl Into<String>,
        columns: Vec<JetDataPlotField>,
    ) -> Self {
        Self {
            identity: identity.into(),
            row_type: row_type.into(),
            columns,
        }
    }

    pub fn validate(&self) -> Result<(), JetDataPlotError> {
        if self.identity.trim().is_empty() || self.row_type.trim().is_empty() {
            return Err(JetDataPlotError::invalid(
                "plot.schema",
                "plot schema identity and row type must not be empty",
            ));
        }
        if self
            .identity
            .chars()
            .chain(self.row_type.chars())
            .any(char::is_control)
        {
            return Err(JetDataPlotError::invalid(
                "plot.schema",
                "plot schema metadata must not contain control characters",
            ));
        }
        let mut names = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for column in &self.columns {
            column.validate()?;
            if !names.insert(column.name.clone()) {
                return Err(JetDataPlotError::invalid(
                    "plot.schema",
                    "plot schema repeats a column name",
                )
                .with_field(column.name.clone()));
            }
            if !ids.insert(column.id.clone()) {
                return Err(JetDataPlotError::invalid(
                    "plot.schema",
                    "plot schema repeats a column identity",
                )
                .with_field(column.name.clone()));
            }
        }
        Ok(())
    }

    pub fn column_named(&self, name: &str) -> Option<&JetDataPlotField> {
        self.columns.iter().find(|column| column.name == name)
    }

    pub fn column_id(&self, id: &str) -> Option<&JetDataPlotField> {
        self.columns.iter().find(|column| column.id == id)
    }

    fn resolve(&self, field: &JetDataPlotField) -> Result<JetDataPlotField, JetDataPlotError> {
        let found = self.column_id(&field.id).ok_or_else(|| {
            JetDataPlotError::invalid(
                "plot.schema",
                "plot encoding references an unknown table column identity",
            )
            .with_field(field.name.clone())
            .with_expected(field.id.clone())
            .with_actual("missing")
        })?;
        if found.name != field.name || found.type_name != field.type_name {
            return Err(JetDataPlotError::invalid(
                "plot.schema",
                "plot encoding column type does not match the table schema",
            )
            .with_field(field.name.clone())
            .with_expected(found.type_name.clone())
            .with_actual(field.type_name.clone()));
        }
        Ok(found.clone())
    }

    fn canonical_key(&self) -> String {
        format!(
            "schema={}:row={}:columns=[{}]",
            jet_data_plot_escape(&self.identity),
            jet_data_plot_escape(&self.row_type),
            self.columns
                .iter()
                .map(JetDataPlotField::canonical_key)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotSourceFacts {
    pub table_plan_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub row_type: String,
    pub rows: i64,
    pub data_identity: String,
    pub provenance: String,
}

impl JetDataPlotSourceFacts {
    pub fn new(
        table_plan_identity: impl Into<String>,
        source_identity: impl Into<String>,
        schema_identity: impl Into<String>,
        row_type: impl Into<String>,
        rows: i64,
        data_identity: impl Into<String>,
        provenance: impl Into<String>,
    ) -> Self {
        Self {
            table_plan_identity: table_plan_identity.into(),
            source_identity: source_identity.into(),
            schema_identity: schema_identity.into(),
            row_type: row_type.into(),
            rows,
            data_identity: data_identity.into(),
            provenance: provenance.into(),
        }
    }

    pub fn validate(&self) -> Result<(), JetDataPlotError> {
        if self.rows < 0 {
            return Err(JetDataPlotError::invalid(
                "plot.source",
                "plot source row count must not be negative",
            ));
        }
        for value in [
            &self.table_plan_identity,
            &self.source_identity,
            &self.schema_identity,
            &self.row_type,
            &self.data_identity,
            &self.provenance,
        ] {
            if value.trim().is_empty() {
                return Err(JetDataPlotError::invalid(
                    "plot.source",
                    "plot source facts must not be empty",
                ));
            }
            if value.chars().any(char::is_control) {
                return Err(JetDataPlotError::invalid(
                    "plot.source",
                    "plot source facts must not contain control characters",
                ));
            }
        }
        Ok(())
    }

    fn canonical_key(&self) -> String {
        format!(
            "plan={};source={};schema={};row={};rows={};data={};provenance={}",
            jet_data_plot_escape(&self.table_plan_identity),
            jet_data_plot_escape(&self.source_identity),
            jet_data_plot_escape(&self.schema_identity),
            jet_data_plot_escape(&self.row_type),
            self.rows,
            jet_data_plot_escape(&self.data_identity),
            jet_data_plot_escape(&self.provenance)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotEncoding {
    pub channel: JetDataPlotChannel,
    pub field: JetDataPlotField,
    pub aggregate: JetDataPlotAggregate,
}

impl JetDataPlotEncoding {
    pub fn new(
        channel: JetDataPlotChannel,
        field: JetDataPlotField,
        aggregate: JetDataPlotAggregate,
    ) -> Self {
        Self {
            channel,
            field,
            aggregate,
        }
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.channel.as_str(),
            self.field.canonical_key(),
            self.aggregate.as_str()
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetDataPlotTransform {
    Filter {
        field: JetDataPlotField,
        op: JetDataPlotFilterOp,
        value: JetDataPlotValue,
    },
    Sort {
        field: JetDataPlotField,
        descending: bool,
    },
    Bin {
        field: JetDataPlotField,
        step: f64,
    },
    Aggregate {
        group_by: Vec<JetDataPlotField>,
        field: JetDataPlotField,
        aggregate: JetDataPlotAggregate,
    },
}

impl JetDataPlotTransform {
    fn canonical_key(&self) -> String {
        match self {
            Self::Filter { field, op, value } => {
                format!("filter:{}:{}:{}", field.canonical_key(), op.as_str(), value.canonical_key())
            }
            Self::Sort {
                field,
                descending,
            } => format!(
                "sort:{}:{}",
                field.canonical_key(),
                if *descending { "desc" } else { "asc" }
            ),
            Self::Bin { field, step } => {
                format!("bin:{}:{}", field.canonical_key(), jet_data_plot_number(*step))
            }
            Self::Aggregate {
                group_by,
                field,
                aggregate,
            } => format!(
                "aggregate:[{}]:{}:{}",
                group_by
                    .iter()
                    .map(JetDataPlotField::canonical_key)
                    .collect::<Vec<_>>()
                    .join(","),
                field.canonical_key(),
                aggregate.as_str()
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotScaleKind {
    Linear,
    Log,
    Band,
    Point,
}

impl JetDataPlotScaleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Log => "log",
            Self::Band => "band",
            Self::Point => "point",
        }
    }

    fn needs_numeric(self) -> bool {
        matches!(self, Self::Linear | Self::Log)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetDataPlotDomain {
    Auto,
    Numeric { min: f64, max: f64 },
    Categories(Vec<String>),
}

impl JetDataPlotDomain {
    fn canonical_key(&self) -> String {
        match self {
            Self::Auto => "auto".to_string(),
            Self::Numeric { min, max } => {
                format!("numeric:{}:{}", jet_data_plot_number(*min), jet_data_plot_number(*max))
            }
            Self::Categories(values) => format!(
                "categories:[{}]",
                values
                    .iter()
                    .map(|value| jet_data_plot_escape(value))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotScale {
    pub channel: JetDataPlotChannel,
    pub kind: JetDataPlotScaleKind,
    pub domain: JetDataPlotDomain,
    pub clamp: bool,
    pub reverse: bool,
}

impl JetDataPlotScale {
    pub fn new(
        channel: JetDataPlotChannel,
        kind: JetDataPlotScaleKind,
        domain: JetDataPlotDomain,
    ) -> Self {
        Self {
            channel,
            kind,
            domain,
            clamp: false,
            reverse: false,
        }
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}:clamp={}:reverse={}",
            self.channel.as_str(),
            self.kind.as_str(),
            self.domain.canonical_key(),
            self.clamp,
            self.reverse
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotAxis {
    pub channel: JetDataPlotChannel,
    pub title: String,
    pub visible: bool,
    pub grid: bool,
    pub ticks: i64,
}

impl JetDataPlotAxis {
    pub fn new(channel: JetDataPlotChannel, title: impl Into<String>) -> Self {
        Self {
            channel,
            title: title.into(),
            visible: true,
            grid: true,
            ticks: 5,
        }
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:visible={}:grid={}:ticks={}",
            self.channel.as_str(),
            jet_data_plot_escape(&self.title),
            self.visible,
            self.grid,
            self.ticks
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotLegendPosition {
    Top,
    Right,
    Bottom,
    Left,
}

impl JetDataPlotLegendPosition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Left => "left",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotLegend {
    pub channel: JetDataPlotChannel,
    pub title: String,
    pub position: JetDataPlotLegendPosition,
    pub visible: bool,
}

impl JetDataPlotLegend {
    pub fn new(channel: JetDataPlotChannel, title: impl Into<String>) -> Self {
        Self {
            channel,
            title: title.into(),
            position: JetDataPlotLegendPosition::Right,
            visible: true,
        }
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}:visible={}",
            self.channel.as_str(),
            jet_data_plot_escape(&self.title),
            self.position.as_str(),
            self.visible
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotFacetKind {
    Row,
    Column,
}

impl JetDataPlotFacetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Column => "column",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotFacet {
    pub field: JetDataPlotField,
    pub kind: JetDataPlotFacetKind,
    pub title: String,
    pub columns: i64,
    pub rows: i64,
}

impl JetDataPlotFacet {
    pub fn new(field: JetDataPlotField, kind: JetDataPlotFacetKind) -> Self {
        Self {
            field,
            kind,
            title: String::new(),
            columns: 0,
            rows: 0,
        }
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}:columns={}:rows={}",
            self.kind.as_str(),
            self.field.canonical_key(),
            jet_data_plot_escape(&self.title),
            self.columns,
            self.rows
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotLayer {
    pub name: String,
    pub mark: JetDataPlotMark,
    pub encodings: Vec<JetDataPlotEncoding>,
    pub transforms: Vec<JetDataPlotTransform>,
    pub opacity: f64,
}

impl JetDataPlotLayer {
    pub fn new(name: impl Into<String>, mark: JetDataPlotMark) -> Self {
        Self {
            name: name.into(),
            mark,
            encodings: Vec::new(),
            transforms: Vec::new(),
            opacity: 1.0,
        }
    }

    pub fn encode(
        mut self,
        channel: JetDataPlotChannel,
        field: JetDataPlotField,
        aggregate: JetDataPlotAggregate,
    ) -> Self {
        let encoding = JetDataPlotEncoding::new(channel, field, aggregate);
        if let Some(existing) = self
            .encodings
            .iter_mut()
            .find(|existing| existing.channel == channel)
        {
            *existing = encoding;
        } else {
            self.encodings.push(encoding);
        }
        self
    }

    pub fn transform(mut self, transform: JetDataPlotTransform) -> Self {
        self.transforms.push(transform);
        self
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:opacity={}:encodings=[{}]:transforms=[{}]",
            jet_data_plot_escape(&self.name),
            self.mark.as_str(),
            jet_data_plot_number(self.opacity),
            self.encodings
                .iter()
                .map(JetDataPlotEncoding::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.transforms
                .iter()
                .map(JetDataPlotTransform::canonical_key)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotInteraction {
    Hover,
    Select,
    Zoom,
    Pan,
    Brush,
}

impl JetDataPlotInteraction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Select => "select",
            Self::Zoom => "zoom",
            Self::Pan => "pan",
            Self::Brush => "brush",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotAccessibility {
    pub title: String,
    pub description: String,
    pub summary: String,
    pub keyboard: bool,
    pub announce_selection: bool,
}

impl Default for JetDataPlotAccessibility {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            summary: String::new(),
            keyboard: true,
            announce_selection: true,
        }
    }
}

impl JetDataPlotAccessibility {
    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}:keyboard={}:announce_selection={}",
            jet_data_plot_escape(&self.title),
            jet_data_plot_escape(&self.description),
            jet_data_plot_escape(&self.summary),
            self.keyboard,
            self.announce_selection
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotLayout {
    pub width: f64,
    pub height: f64,
    pub margin_top: f64,
    pub margin_right: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
}

impl Default for JetDataPlotLayout {
    fn default() -> Self {
        Self {
            width: 640.0,
            height: 360.0,
            margin_top: 44.0,
            margin_right: 24.0,
            margin_bottom: 52.0,
            margin_left: 64.0,
        }
    }
}

impl JetDataPlotLayout {
    fn validate(&self) -> Result<(), JetDataPlotError> {
        if ![
            self.width,
            self.height,
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
        ]
        .into_iter()
        .all(f64::is_finite)
            || self.width <= 0.0
            || self.height <= 0.0
            || self.margin_top < 0.0
            || self.margin_right < 0.0
            || self.margin_bottom < 0.0
            || self.margin_left < 0.0
            || self.width <= self.margin_left + self.margin_right
            || self.height <= self.margin_top + self.margin_bottom
        {
            return Err(JetDataPlotError::invalid(
                "plot.layout",
                "plot layout dimensions and margins are invalid",
            ));
        }
        Ok(())
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            jet_data_plot_number(self.width),
            jet_data_plot_number(self.height),
            jet_data_plot_number(self.margin_top),
            jet_data_plot_number(self.margin_right),
            jet_data_plot_number(self.margin_bottom),
            jet_data_plot_number(self.margin_left)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotBackend {
    Terminal,
    Browser,
    Native,
    Export,
}

impl JetDataPlotBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Browser => "browser",
            Self::Native => "native",
            Self::Export => "export",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotSupport {
    Supported,
    Degraded,
    Unsupported,
}

impl JetDataPlotSupport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Degraded => "degraded",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDataPlotCapability {
    pub backend: JetDataPlotBackend,
    pub feature: String,
    pub support: JetDataPlotSupport,
    pub reason: String,
    pub replacement: String,
}

impl JetDataPlotCapability {
    fn supported(backend: JetDataPlotBackend, feature: impl Into<String>) -> Self {
        Self {
            backend,
            feature: feature.into(),
            support: JetDataPlotSupport::Supported,
            reason: String::new(),
            replacement: String::new(),
        }
    }

    fn degraded(
        backend: JetDataPlotBackend,
        feature: impl Into<String>,
        reason: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            backend,
            feature: feature.into(),
            support: JetDataPlotSupport::Degraded,
            reason: reason.into(),
            replacement: replacement.into(),
        }
    }

    fn unsupported(
        backend: JetDataPlotBackend,
        feature: impl Into<String>,
        reason: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            backend,
            feature: feature.into(),
            support: JetDataPlotSupport::Unsupported,
            reason: reason.into(),
            replacement: replacement.into(),
        }
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.backend.as_str(),
            jet_data_plot_escape(&self.feature),
            self.support.as_str(),
            jet_data_plot_escape(&self.reason),
            jet_data_plot_escape(&self.replacement)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDataPlotError {
    pub kind: JetDataPlotErrorKind,
    pub operation: String,
    pub field: Option<String>,
    pub channel: Option<String>,
    pub mark: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub index: Option<i64>,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDataPlotErrorKind {
    InvalidArgument,
    NonFinite,
    Unsupported,
    Empty,
    Limit,
}

impl JetDataPlotError {
    fn invalid(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: JetDataPlotErrorKind::InvalidArgument,
            operation: operation.into(),
            field: None,
            channel: None,
            mark: None,
            expected: None,
            actual: None,
            index: None,
            reason: reason.into(),
        }
    }

    fn non_finite(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: JetDataPlotErrorKind::NonFinite,
            ..Self::invalid(operation, reason)
        }
    }

    fn unsupported(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: JetDataPlotErrorKind::Unsupported,
            ..Self::invalid(operation, reason)
        }
    }

    fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    fn with_channel(mut self, channel: JetDataPlotChannel) -> Self {
        self.channel = Some(channel.as_str().to_string());
        self
    }

    fn with_mark(mut self, mark: JetDataPlotMark) -> Self {
        self.mark = Some(mark.as_str().to_string());
        self
    }

    fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    fn with_index(mut self, index: usize) -> Self {
        self.index = Some(i64::try_from(index).unwrap_or(i64::MAX));
        self
    }

    fn message(&self) -> String {
        let mut out = self.reason.clone();
        if let Some(field) = &self.field {
            out.push_str(&format!("; field={field}"));
        }
        if let Some(channel) = &self.channel {
            out.push_str(&format!("; channel={channel}"));
        }
        if let Some(mark) = &self.mark {
            out.push_str(&format!("; mark={mark}"));
        }
        if let Some(expected) = &self.expected {
            out.push_str(&format!("; expected={expected}"));
        }
        if let Some(actual) = &self.actual {
            out.push_str(&format!("; actual={actual}"));
        }
        out
    }

    pub(crate) fn into_data_error(self) -> jet_std::DataError {
        let kind = match self.kind {
            JetDataPlotErrorKind::NonFinite => jet_std::DataErrorKind::NonFinite,
            JetDataPlotErrorKind::Unsupported => jet_std::DataErrorKind::Bridge,
            JetDataPlotErrorKind::Empty => jet_std::DataErrorKind::Empty,
            JetDataPlotErrorKind::Limit => jet_std::DataErrorKind::Limit,
            JetDataPlotErrorKind::InvalidArgument => jet_std::DataErrorKind::InvalidArgument,
        };
        let reason = self.message();
        jet_std::DataError {
            kind,
            operation: self.operation,
            row: Err(JetAbsent),
            column: Err(JetAbsent),
            index: self.index.map(Ok).unwrap_or(Err(JetAbsent)),
            reason,
            cause: Err(JetAbsent),
        }
    }
}

impl std::fmt::Display for JetDataPlotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?} {}: {}", self.kind, self.operation, self.message())?;
        if let Some(index) = self.index {
            write!(formatter, ", index {index}")?;
        }
        Ok(())
    }
}

impl std::error::Error for JetDataPlotError {}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotPlan {
    pub source: JetDataPlotSourceFacts,
    pub schema: JetDataPlotSchema,
    pub limits: jet_std::DataLimits,
    pub mark: JetDataPlotMark,
    pub encodings: Vec<JetDataPlotEncoding>,
    pub transforms: Vec<JetDataPlotTransform>,
    pub scales: Vec<JetDataPlotScale>,
    pub axes: Vec<JetDataPlotAxis>,
    pub legends: Vec<JetDataPlotLegend>,
    pub facets: Vec<JetDataPlotFacet>,
    pub layers: Vec<JetDataPlotLayer>,
    pub interactions: Vec<JetDataPlotInteraction>,
    pub accessibility: JetDataPlotAccessibility,
    pub layout: JetDataPlotLayout,
}

impl JetDataPlotPlan {
    pub fn new(
        source: JetDataPlotSourceFacts,
        schema: JetDataPlotSchema,
    ) -> Result<Self, JetDataPlotError> {
        Self::with_limits(source, schema, jet_std::DataLimits::safe())
    }

    pub fn with_limits(
        source: JetDataPlotSourceFacts,
        schema: JetDataPlotSchema,
        limits: jet_std::DataLimits,
    ) -> Result<Self, JetDataPlotError> {
        source.validate()?;
        schema.validate()?;
        for (name, value) in [
            ("max_groups", limits.max_groups),
            ("max_sort_rows", limits.max_sort_rows),
            ("max_join_rows", limits.max_join_rows),
            ("max_output_rows", limits.max_output_rows),
        ] {
            if value < 1 {
                return Err(JetDataPlotError::invalid(
                    "plot.limits",
                    format!("{name} must be positive"),
                )
                .with_actual(value.to_string()));
            }
        }
        if source.schema_identity != schema.identity {
            return Err(JetDataPlotError::invalid(
                "plot.plan",
                "plot source schema identity does not match the plot schema",
            )
            .with_expected(schema.identity.clone())
            .with_actual(source.schema_identity.clone()));
        }
        if source.row_type != schema.row_type {
            return Err(JetDataPlotError::invalid(
                "plot.plan",
                "plot source row type does not match the plot schema",
            )
            .with_expected(schema.row_type.clone())
            .with_actual(source.row_type.clone()));
        }
        Ok(Self {
            source,
            schema,
            limits,
            mark: JetDataPlotMark::Line,
            encodings: Vec::new(),
            transforms: Vec::new(),
            scales: Vec::new(),
            axes: Vec::new(),
            legends: Vec::new(),
            facets: Vec::new(),
            layers: Vec::new(),
            interactions: Vec::new(),
            accessibility: JetDataPlotAccessibility::default(),
            layout: JetDataPlotLayout::default(),
        })
    }

    pub fn set_encoding(
        &mut self,
        channel: JetDataPlotChannel,
        field: JetDataPlotField,
        aggregate: JetDataPlotAggregate,
    ) -> Result<(), JetDataPlotError> {
        let field = self.schema.resolve(&field)?;
        if aggregate.needs_numeric() && !jet_data_plot_numeric_type(&field.type_name) {
            return Err(JetDataPlotError::invalid(
                "plot.encoding",
                "numeric aggregation needs an integer or floating column",
            )
            .with_field(field.name)
            .with_channel(channel)
            .with_expected("numeric")
            .with_actual(field.type_name));
        }
        if channel == JetDataPlotChannel::Size && !jet_data_plot_numeric_type(&field.type_name) {
            return Err(JetDataPlotError::invalid(
                "plot.encoding",
                "size encoding needs an integer or floating column",
            )
            .with_field(field.name)
            .with_channel(channel)
            .with_expected("numeric")
            .with_actual(field.type_name));
        }
        let encoding = JetDataPlotEncoding::new(channel, field, aggregate);
        if let Some(existing) = self
            .encodings
            .iter_mut()
            .find(|existing| existing.channel == channel)
        {
            *existing = encoding;
        } else {
            self.encodings.push(encoding);
        }
        Ok(())
    }

    pub fn add_transform(
        &mut self,
        transform: JetDataPlotTransform,
    ) -> Result<(), JetDataPlotError> {
        self.validate_transform(&transform)?;
        self.transforms.push(transform);
        Ok(())
    }

    pub fn add_scale(&mut self, scale: JetDataPlotScale) -> Result<(), JetDataPlotError> {
        self.validate_scale(&scale)?;
        if let Some(existing) = self
            .scales
            .iter_mut()
            .find(|existing| existing.channel == scale.channel)
        {
            *existing = scale;
        } else {
            self.scales.push(scale);
        }
        Ok(())
    }

    pub fn add_axis(&mut self, axis: JetDataPlotAxis) -> Result<(), JetDataPlotError> {
        if !matches!(axis.channel, JetDataPlotChannel::X | JetDataPlotChannel::Y) {
            return Err(JetDataPlotError::invalid(
                "plot.axis",
                "axes can only describe x or y channels",
            )
            .with_channel(axis.channel));
        }
        if axis.ticks < 0 {
            return Err(JetDataPlotError::invalid(
                "plot.axis",
                "axis tick count must not be negative",
            )
            .with_channel(axis.channel));
        }
        if axis.title.chars().any(char::is_control) {
            return Err(JetDataPlotError::invalid(
                "plot.axis",
                "axis title must not contain control characters",
            )
            .with_channel(axis.channel));
        }
        if let Some(existing) = self
            .axes
            .iter_mut()
            .find(|existing| existing.channel == axis.channel)
        {
            *existing = axis;
        } else {
            self.axes.push(axis);
        }
        Ok(())
    }

    pub fn add_legend(&mut self, legend: JetDataPlotLegend) -> Result<(), JetDataPlotError> {
        if !matches!(
            legend.channel,
            JetDataPlotChannel::Color | JetDataPlotChannel::Detail | JetDataPlotChannel::Text
        ) {
            return Err(JetDataPlotError::invalid(
                "plot.legend",
                "legends need color, detail, or text channels",
            )
            .with_channel(legend.channel));
        }
        if legend.title.chars().any(char::is_control) {
            return Err(JetDataPlotError::invalid(
                "plot.legend",
                "legend title must not contain control characters",
            )
            .with_channel(legend.channel));
        }
        self.legends.push(legend);
        Ok(())
    }

    pub fn add_facet(&mut self, facet: JetDataPlotFacet) -> Result<(), JetDataPlotError> {
        self.schema.resolve(&facet.field)?;
        if facet.columns < 0 || facet.rows < 0 {
            return Err(JetDataPlotError::invalid(
                "plot.facet",
                "facet row and column limits must not be negative",
            )
            .with_field(facet.field.name));
        }
        if facet.title.chars().any(char::is_control) {
            return Err(JetDataPlotError::invalid(
                "plot.facet",
                "facet title must not contain control characters",
            )
            .with_field(facet.field.name));
        }
        self.facets.push(facet);
        Ok(())
    }

    pub fn add_layer(&mut self, layer: JetDataPlotLayer) -> Result<(), JetDataPlotError> {
        self.validate_layer(&layer)?;
        self.layers.push(layer);
        Ok(())
    }

    pub fn add_interaction(
        &mut self,
        interaction: JetDataPlotInteraction,
    ) -> Result<(), JetDataPlotError> {
        if !self.interactions.contains(&interaction) {
            self.interactions.push(interaction);
            self.interactions.sort();
        }
        Ok(())
    }

    pub fn set_accessibility(
        &mut self,
        accessibility: JetDataPlotAccessibility,
    ) -> Result<(), JetDataPlotError> {
        for value in [
            &accessibility.title,
            &accessibility.description,
            &accessibility.summary,
        ] {
            if value.chars().any(char::is_control) {
                return Err(JetDataPlotError::invalid(
                    "plot.accessibility",
                    "accessibility text must not contain control characters",
                ));
            }
        }
        self.accessibility = accessibility;
        Ok(())
    }

    pub fn set_layout(&mut self, layout: JetDataPlotLayout) -> Result<(), JetDataPlotError> {
        layout.validate()?;
        self.layout = layout;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), JetDataPlotError> {
        self.source.validate()?;
        self.schema.validate()?;
        self.layout.validate()?;
        let mut channels = BTreeSet::new();
        let mut scale_channels = BTreeSet::new();
        let mut axis_channels = BTreeSet::new();
        for encoding in &self.encodings {
            if !channels.insert(encoding.channel) {
                return Err(JetDataPlotError::invalid(
                    "plot.encoding",
                    "plot plan repeats an encoding channel",
                )
                .with_channel(encoding.channel));
            }
            let field = self.schema.resolve(&encoding.field)?;
            if encoding.aggregate.needs_numeric() && !jet_data_plot_numeric_type(&field.type_name) {
                return Err(JetDataPlotError::invalid(
                    "plot.encoding",
                    "numeric aggregation needs an integer or floating column",
                )
                .with_field(field.name)
                .with_channel(encoding.channel));
            }
            if encoding.channel == JetDataPlotChannel::Size
                && !jet_data_plot_numeric_type(&field.type_name)
            {
                return Err(JetDataPlotError::invalid(
                    "plot.encoding",
                    "size encoding needs an integer or floating column",
                )
                .with_field(field.name)
                .with_channel(encoding.channel));
            }
        }
        for transform in &self.transforms {
            self.validate_transform(transform)?;
        }
        for scale in &self.scales {
            if !scale_channels.insert(scale.channel) {
                return Err(JetDataPlotError::invalid(
                    "plot.scale",
                    "plot plan repeats a scale channel",
                )
                .with_channel(scale.channel));
            }
            self.validate_scale(scale)?;
        }
        for axis in &self.axes {
            if !axis_channels.insert(axis.channel) {
                return Err(JetDataPlotError::invalid(
                    "plot.axis",
                    "plot plan repeats an axis channel",
                )
                .with_channel(axis.channel));
            }
            if !matches!(axis.channel, JetDataPlotChannel::X | JetDataPlotChannel::Y) {
                return Err(JetDataPlotError::invalid(
                    "plot.axis",
                    "axes can only describe x or y channels",
                )
                .with_channel(axis.channel));
            }
            if !channels.contains(&axis.channel) {
                return Err(JetDataPlotError::invalid(
                    "plot.axis",
                    "axis references a channel that is not encoded",
                )
                .with_channel(axis.channel));
            }
            if axis.ticks < 0 || axis.title.chars().any(char::is_control) {
                return Err(JetDataPlotError::invalid(
                    "plot.axis",
                    "axis labels and tick counts are invalid",
                )
                .with_channel(axis.channel));
            }
        }
        for legend in &self.legends {
            if !matches!(
                legend.channel,
                JetDataPlotChannel::Color
                    | JetDataPlotChannel::Detail
                    | JetDataPlotChannel::Text
            ) {
                return Err(JetDataPlotError::invalid(
                    "plot.legend",
                    "legends need color, detail, or text channels",
                )
                .with_channel(legend.channel));
            }
            if !channels.contains(&legend.channel) {
                return Err(JetDataPlotError::invalid(
                    "plot.legend",
                    "legend references a channel that is not encoded",
                )
                .with_channel(legend.channel));
            }
            if legend.title.chars().any(char::is_control) {
                return Err(JetDataPlotError::invalid(
                    "plot.legend",
                    "legend title must not contain control characters",
                )
                .with_channel(legend.channel));
            }
        }
        for facet in &self.facets {
            self.schema.resolve(&facet.field)?;
            if facet.columns < 0 || facet.rows < 0 {
                return Err(JetDataPlotError::invalid(
                    "plot.facet",
                    "facet row and column limits must not be negative",
                )
                .with_field(facet.field.name.clone()));
            }
            if facet.title.chars().any(char::is_control) {
                return Err(JetDataPlotError::invalid(
                    "plot.facet",
                    "facet title must not contain control characters",
                )
                .with_field(facet.field.name.clone()));
            }
        }
        for layer in &self.layers {
            self.validate_layer(layer)?;
        }
        for value in [
            &self.accessibility.title,
            &self.accessibility.description,
            &self.accessibility.summary,
        ] {
            if value.chars().any(char::is_control) {
                return Err(JetDataPlotError::invalid(
                    "plot.accessibility",
                    "accessibility text must not contain control characters",
                ));
            }
        }
        Ok(())
    }
    pub fn validate_for_render(&self) -> Result<(), JetDataPlotError> {
        self.validate()?;
        self.validate_mark_encodings(self.mark, &self.encodings)?;
        for layer in &self.layers {
            let encodings = jet_data_plot_effective_encodings(&self.encodings, &layer.encodings);
            self.validate_mark_encodings(layer.mark, &encodings)?;
        }
        Ok(())
    }

    pub fn canonical_key(&self) -> String {
        format!(
            "version={};source={};{};limits={};mark={};encodings=[{}];transforms=[{}];scales=[{}];axes=[{}];legends=[{}];facets=[{}];layers=[{}];interactions=[{}];accessibility={};layout={}",
            JET_DATA_PLOT_SCHEMA_VERSION,
            self.source.canonical_key(),
            self.schema.canonical_key(),
            jet_data_plot_limits_key(&self.limits),
            self.mark.as_str(),
            self.encodings
                .iter()
                .map(JetDataPlotEncoding::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.transforms
                .iter()
                .map(JetDataPlotTransform::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.scales
                .iter()
                .map(JetDataPlotScale::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.axes
                .iter()
                .map(JetDataPlotAxis::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.legends
                .iter()
                .map(JetDataPlotLegend::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.facets
                .iter()
                .map(JetDataPlotFacet::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.layers
                .iter()
                .map(JetDataPlotLayer::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.interactions
                .iter()
                .map(|interaction| interaction.as_str())
                .collect::<Vec<_>>()
                .join(","),
            self.accessibility.canonical_key(),
            self.layout.canonical_key()
        )
    }

    pub fn inspect_text(&self) -> Result<String, JetDataPlotError> {
        self.validate()?;
        Ok(format!(
            "plot version={} source={} schema={} limits={} mark={} encodings=[{}] transforms=[{}] scales=[{}] axes=[{}] legends=[{}] facets=[{}] layers=[{}] interactions=[{}] layout={} accessibility={}",
            JET_DATA_PLOT_SCHEMA_VERSION,
            self.source.canonical_key(),
            self.schema.canonical_key(),
            jet_data_plot_limits_key(&self.limits),
            self.mark.as_str(),
            self.encodings
                .iter()
                .map(JetDataPlotEncoding::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.transforms
                .iter()
                .map(JetDataPlotTransform::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.scales
                .iter()
                .map(JetDataPlotScale::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.axes
                .iter()
                .map(JetDataPlotAxis::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.legends
                .iter()
                .map(JetDataPlotLegend::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.facets
                .iter()
                .map(JetDataPlotFacet::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.layers
                .iter()
                .map(JetDataPlotLayer::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.interactions
                .iter()
                .map(|interaction| interaction.as_str())
                .collect::<Vec<_>>()
                .join(","),
            self.layout.canonical_key(),
            self.accessibility.canonical_key()
        ))
    }

    pub fn inspect_json(&self) -> Result<String, JetDataPlotError> {
        self.validate()?;
        Ok(jet_data_plot_plan_json(self))
    }

    pub fn capabilities(&self, backend: JetDataPlotBackend) -> Vec<JetDataPlotCapability> {
        let mut capabilities = vec![
            JetDataPlotCapability::supported(backend, "line"),
            JetDataPlotCapability::supported(backend, "bar"),
            JetDataPlotCapability::supported(backend, "point"),
            JetDataPlotCapability::supported(backend, "layers"),
            JetDataPlotCapability::supported(backend, "facets"),
            JetDataPlotCapability::supported(backend, "accessibility"),
        ];
        if backend == JetDataPlotBackend::Terminal {
            capabilities.push(JetDataPlotCapability::degraded(
                backend,
                "svg",
                "terminal adapters emit text rather than SVG",
                "use the deterministic text projection",
            ));
        } else {
            capabilities.push(JetDataPlotCapability::supported(backend, "svg"));
        }
        for interaction in &self.interactions {
            let feature = format!("interaction.{}", interaction.as_str());
            match backend {
                JetDataPlotBackend::Browser => capabilities.push(JetDataPlotCapability::degraded(
                    backend,
                    feature,
                    "a live client owns pointer and keyboard events",
                    "attach the browser interaction adapter to this checked plan",
                )),
                JetDataPlotBackend::Terminal => capabilities.push(JetDataPlotCapability::unsupported(
                    backend,
                    feature,
                    "terminal output has no live pointer or keyboard session",
                    "inspect the source-backed selection or use browser.show",
                )),
                JetDataPlotBackend::Native | JetDataPlotBackend::Export => {
                    capabilities.push(JetDataPlotCapability::degraded(
                        backend,
                        feature,
                        "this projection is static and does not own a live client",
                        "use the browser adapter for live interaction",
                    ))
                }
            }
        }
        capabilities.sort_by(|left, right| left.feature.cmp(&right.feature));
        capabilities
    }

    fn validate_transform(
        &self,
        transform: &JetDataPlotTransform,
    ) -> Result<(), JetDataPlotError> {
        match transform {
            JetDataPlotTransform::Filter { field, value, .. } => {
                let field = self.schema.resolve(field)?;
                if let JetDataPlotValue::Number(number) = value {
                    if !number.is_finite() {
                        return Err(JetDataPlotError::non_finite(
                            "plot.transform",
                            "filter literal must be finite",
                        )
                        .with_field(field.name.clone()));
                    }
                }
                if let JetDataPlotValue::Text(text) = value {
                    if text.chars().any(char::is_control) {
                        return Err(JetDataPlotError::invalid(
                            "plot.transform",
                            "filter text must not contain control characters",
                        )
                        .with_field(field.name.clone()));
                    }
                }
                if !jet_data_plot_filter_value_matches_type(value, &field.type_name) {
                    return Err(JetDataPlotError::invalid(
                        "plot.transform",
                        "filter literal type does not match the table column",
                    )
                    .with_field(field.name.clone())
                    .with_expected(field.type_name)
                    .with_actual(value.kind()));
                }
            }
            JetDataPlotTransform::Sort { field, .. } => {
                self.schema.resolve(field)?;
            }
            JetDataPlotTransform::Bin { field, step } => {
                let field = self.schema.resolve(field)?;
                if !jet_data_plot_numeric_type(&field.type_name) {
                    return Err(JetDataPlotError::invalid(
                        "plot.transform",
                        "bin transform needs an integer or floating column",
                    )
                    .with_field(field.name));
                }
                if !step.is_finite() || *step <= 0.0 {
                    return Err(JetDataPlotError::invalid(
                        "plot.transform",
                        "bin step must be finite and positive",
                    )
                    .with_field(field.name));
                }
            }
            JetDataPlotTransform::Aggregate {
                group_by,
                field,
                aggregate,
            } => {
                for field in group_by {
                    self.schema.resolve(field)?;
                }
                let field = self.schema.resolve(field)?;
                if aggregate.needs_numeric() && !jet_data_plot_numeric_type(&field.type_name) {
                    return Err(JetDataPlotError::invalid(
                        "plot.transform",
                        "numeric aggregation needs an integer or floating column",
                    )
                    .with_field(field.name)
                    .with_expected("numeric")
                    .with_actual(field.type_name));
                }
                if *aggregate == JetDataPlotAggregate::None {
                    return Err(JetDataPlotError::invalid(
                        "plot.transform",
                        "aggregate transform needs a concrete aggregate",
                    )
                    .with_field(field.name));
                }
            }
        }
        Ok(())
    }

    fn validate_scale(&self, scale: &JetDataPlotScale) -> Result<(), JetDataPlotError> {
        let Some(encoding) = self
            .encodings
            .iter()
            .find(|encoding| encoding.channel == scale.channel)
        else {
            return Err(JetDataPlotError::invalid(
                "plot.scale",
                "scale references a channel that is not encoded",
            )
            .with_channel(scale.channel));
        };
        let field = self.schema.resolve(&encoding.field)?;
        if scale.kind.needs_numeric() && !jet_data_plot_numeric_type(&field.type_name) {
            return Err(JetDataPlotError::invalid(
                "plot.scale",
                "linear and log scales need an integer or floating column",
            )
            .with_channel(scale.channel)
            .with_field(field.name)
            .with_expected("numeric")
            .with_actual(field.type_name));
        }
        match &scale.domain {
            JetDataPlotDomain::Numeric { min, max } => {
                if !min.is_finite() || !max.is_finite() || min >= max {
                    return Err(JetDataPlotError::invalid(
                        "plot.scale",
                        "numeric scale domain must be finite and increasing",
                    )
                    .with_channel(scale.channel));
                }
                if scale.kind == JetDataPlotScaleKind::Log && *min <= 0.0 {
                    return Err(JetDataPlotError::invalid(
                        "plot.scale",
                        "log scale domain must be positive",
                    )
                    .with_channel(scale.channel));
                }
            }
            JetDataPlotDomain::Categories(values) => {
                let mut seen = BTreeSet::new();
                if values.is_empty()
                    || values.iter().any(|value| {
                        value.trim().is_empty()
                            || value.chars().any(char::is_control)
                            || !seen.insert(value.clone())
                    })
                {
                    return Err(JetDataPlotError::invalid(
                        "plot.scale",
                        "categorical scale domain must contain unique non-empty labels",
                    )
                    .with_channel(scale.channel));
                }
                if scale.kind == JetDataPlotScaleKind::Linear
                    || scale.kind == JetDataPlotScaleKind::Log
                {
                    return Err(JetDataPlotError::invalid(
                        "plot.scale",
                        "numeric scales cannot use categorical domains",
                    )
                    .with_channel(scale.channel));
                }
            }
            JetDataPlotDomain::Auto => {}
        }
        Ok(())
    }

    fn validate_layer(&self, layer: &JetDataPlotLayer) -> Result<(), JetDataPlotError> {
        if layer.name.trim().is_empty() || layer.name.chars().any(char::is_control) {
            return Err(JetDataPlotError::invalid(
                "plot.layer",
                "plot layer name must be non-empty and control-free",
            ));
        }
        if !layer.opacity.is_finite() || !(0.0..=1.0).contains(&layer.opacity) {
            return Err(JetDataPlotError::invalid(
                "plot.layer",
                "plot layer opacity must be finite and between zero and one",
            )
            .with_field(layer.name.clone()));
        }
        let mut channels = BTreeSet::new();
        for encoding in &layer.encodings {
            if !channels.insert(encoding.channel) {
                return Err(JetDataPlotError::invalid(
                    "plot.layer",
                    "plot layer repeats an encoding channel",
                )
                .with_channel(encoding.channel)
                .with_field(layer.name.clone()));
            }
            let field = self.schema.resolve(&encoding.field)?;
            if encoding.aggregate.needs_numeric() && !jet_data_plot_numeric_type(&field.type_name) {
                return Err(JetDataPlotError::invalid(
                    "plot.layer",
                    "numeric aggregation needs an integer or floating column",
                )
                .with_field(field.name)
                .with_channel(encoding.channel));
            }
            if encoding.channel == JetDataPlotChannel::Size
                && !jet_data_plot_numeric_type(&field.type_name)
            {
                return Err(JetDataPlotError::invalid(
                    "plot.layer",
                    "size encoding needs an integer or floating column",
                )
                .with_field(field.name)
                .with_channel(encoding.channel));
            }
        }
        for transform in &layer.transforms {
            self.validate_transform(transform)?;
        }
        Ok(())
    }

    fn validate_mark_encodings(
        &self,
        mark: JetDataPlotMark,
        encodings: &[JetDataPlotEncoding],
    ) -> Result<(), JetDataPlotError> {
        let x = encodings
            .iter()
            .find(|encoding| encoding.channel == JetDataPlotChannel::X);
        let y = encodings
            .iter()
            .find(|encoding| encoding.channel == JetDataPlotChannel::Y);
        let Some(x) = x else {
            return Err(JetDataPlotError::invalid(
                "plot.mark",
                "plot mark needs an x encoding",
            )
            .with_mark(mark));
        };
        let Some(y) = y else {
            return Err(JetDataPlotError::invalid(
                "plot.mark",
                "plot mark needs a y encoding",
            )
            .with_mark(mark));
        };
        let y_field = self.schema.resolve(&y.field)?;
        if !jet_data_plot_numeric_type(&y_field.type_name) {
            return Err(JetDataPlotError::invalid(
                "plot.mark",
                "line, bar, and point marks need a numeric y encoding",
            )
            .with_mark(mark)
            .with_field(y_field.name)
            .with_channel(JetDataPlotChannel::Y)
            .with_expected("numeric")
            .with_actual(y_field.type_name));
        }
        if x.aggregate.needs_numeric() {
            let x_field = self.schema.resolve(&x.field)?;
            if !jet_data_plot_numeric_type(&x_field.type_name) {
                return Err(JetDataPlotError::invalid(
                    "plot.mark",
                    "numeric x aggregation needs a numeric column",
                )
                .with_mark(mark)
                .with_field(x_field.name)
                .with_channel(JetDataPlotChannel::X));
            }
        }
        Ok(())
    }
}
pub struct JetDataPlotColumn<T> {
    pub field: JetDataPlotField,
    accessor: JetDataPlotArc<dyn Fn(&T) -> JetDataPlotValue + 'static>,
}

impl<T> Clone for JetDataPlotColumn<T> {
    fn clone(&self) -> Self {
        Self {
            field: self.field.clone(),
            accessor: self.accessor.clone(),
        }
    }
}

impl<T> std::fmt::Debug for JetDataPlotColumn<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetDataPlotColumn")
            .field("field", &self.field)
            .finish_non_exhaustive()
    }
}

impl<T> JetDataPlotColumn<T> {
    pub fn new<F>(field: JetDataPlotField, accessor: F) -> Self
    where
        F: Fn(&T) -> JetDataPlotValue + 'static,
    {
        Self {
            field,
            accessor: JetDataPlotArc::new(accessor),
        }
    }

    pub fn value(&self, row: &T) -> JetDataPlotValue {
        (self.accessor)(row)
    }
}

pub fn jet_data_plot_column<T, F>(
    field: JetDataPlotField,
    accessor: F,
) -> JetDataPlotColumn<T>
where
    F: Fn(&T) -> JetDataPlotValue + 'static,
{
    JetDataPlotColumn::new(field, accessor)
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotSelectedRow {
    pub index: i64,
    pub values: Vec<(JetDataPlotField, JetDataPlotValue)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotInspection {
    pub plan: JetDataPlotPlan,
    pub selected_indices: Vec<i64>,
    pub selected_columns: Vec<JetDataPlotField>,
    pub selected_data: Vec<JetDataPlotSelectedRow>,
}

impl JetDataPlotInspection {
    pub fn text(&self) -> String {
        let mut out = format!(
            "{} selected_indices=[{}] selected_columns=[{}] selected_data=[",
            self.plan.inspect_text().unwrap_or_else(|error| error.to_string()),
            self.selected_indices
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
            self.selected_columns
                .iter()
                .map(JetDataPlotField::canonical_key)
                .collect::<Vec<_>>()
                .join(",")
        );
        for (position, row) in self.selected_data.iter().enumerate() {
            if position > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{}:[{}]",
                row.index,
                row.values
                    .iter()
                    .map(|(field, value)| {
                        format!(
                            "{}={}",
                            jet_data_plot_escape(&field.name),
                            jet_data_plot_escape(&value.display_text())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        out.push(']');
        out
    }

    pub fn json(&self) -> String {
        let mut out = String::from("{\"plan\":");
        out.push_str(&jet_data_plot_plan_json(&self.plan));
        out.push_str(",\"selected_indices\":[");
        out.push_str(
            &self
                .selected_indices
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("],\"selected_columns\":[");
        out.push_str(
            &self
                .selected_columns
                .iter()
                .map(|field| {
                    format!(
                        "{{\"id\":\"{}\",\"name\":\"{}\",\"type\":\"{}\"}}",
                        jet_data_plot_json_escape(&field.id),
                        jet_data_plot_json_escape(&field.name),
                        jet_data_plot_json_escape(&field.type_name)
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("],\"selected_data\":[");
        out.push_str(
            &self
                .selected_data
                .iter()
                .map(|row| {
                    format!(
                        "{{\"index\":{},\"values\":{}}}",
                        row.index,
                        jet_data_plot_values_json(&row.values)
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("]}");
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDataPlotRenderFormat {
    Text,
    Svg,
}

impl JetDataPlotRenderFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Svg => "svg",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotRender {
    pub backend: JetDataPlotBackend,
    pub format: JetDataPlotRenderFormat,
    pub body: String,
    pub source: JetDataPlotSourceFacts,
    pub capabilities: Vec<JetDataPlotCapability>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct JetDataPlotProjection {
    pub backend: JetDataPlotBackend,
    pub plan: JetDataPlotPlan,
    pub source: JetDataPlotSourceFacts,
    pub capabilities: Vec<JetDataPlotCapability>,
}

/// A tier-local typed ordinary-list adapter. Each execution tier transfers
/// rows, checked source/schema facts, and canonical execution limits into the
/// shared plot grammar without a materialized table carrier.
pub trait JetDataPlotRows {
    type Row: 'static;

    fn jet_data_plot_parts(self) -> (
        Vec<Self::Row>,
        JetDataPlotSourceFacts,
        JetDataPlotSchema,
        jet_std::DataLimits,
    );
}

/// Build one checked plot builder from ordinary typed list rows.
pub fn jet_data_plot<Rows>(
    rows: Rows,
) -> Result<JetDataPlot<Rows::Row>, jet_std::DataError>
where
    Rows: JetDataPlotRows,
{
    let (rows, source, schema, limits) = rows.jet_data_plot_parts();
    JetDataPlot::from_rows_with_limits(rows, source, schema, limits)
        .map_err(JetDataPlotError::into_data_error)
}

fn jet_data_plot_result<T>(
    result: Result<T, JetDataPlotError>,
) -> Result<T, jet_std::DataError> {
    result.map_err(JetDataPlotError::into_data_error)
}

pub fn jet_data_plot_inspect<T: 'static>(
    plot: &JetDataPlot<T>,
) -> Result<JetDataPlotInspection, jet_std::DataError> {
    jet_data_plot_result(plot.inspect())
}

pub fn jet_data_plot_inspect_json<T: 'static>(
    plot: &JetDataPlot<T>,
) -> Result<String, jet_std::DataError> {
    jet_data_plot_result(plot.inspect_json())
}

pub fn jet_data_plot_text<T: 'static>(
    plot: &JetDataPlot<T>,
) -> Result<String, jet_std::DataError> {
    jet_data_plot_result(plot.text())
}

pub fn jet_data_plot_svg<T: 'static>(
    plot: &JetDataPlot<T>,
) -> Result<String, jet_std::DataError> {
    jet_data_plot_result(plot.svg())
}

pub fn jet_data_plot_show<T: 'static>(
    plot: &JetDataPlot<T>,
) -> Result<JetDataPlotRender, jet_std::DataError> {
    jet_data_plot_result(plot.show())
}

pub fn jet_data_plot_render<T: 'static>(
    plot: &JetDataPlot<T>,
    backend: JetDataPlotBackend,
) -> Result<JetDataPlotRender, jet_std::DataError> {
    jet_data_plot_result(plot.render(backend))
}

pub fn jet_data_plot_line<T: 'static>(plot: &JetDataPlot<T>) -> JetDataPlot<T> {
    (*plot).clone().line()
}

pub fn jet_data_plot_bar<T: 'static>(plot: &JetDataPlot<T>) -> JetDataPlot<T> {
    (*plot).clone().bar()
}

pub fn jet_data_plot_point<T: 'static>(plot: &JetDataPlot<T>) -> JetDataPlot<T> {
    (*plot).clone().point()
}

pub fn jet_data_plot_mark<T: 'static>(
    plot: &JetDataPlot<T>,
    mark: JetDataPlotMark,
) -> JetDataPlot<T> {
    (*plot).clone().mark(mark)
}

pub fn jet_data_plot_x<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().x(column))
}

pub fn jet_data_plot_y<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
    aggregate: JetDataPlotAggregate,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().y(column, aggregate))
}

pub fn jet_data_plot_color<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().color(column))
}

pub fn jet_data_plot_size<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().size(column))
}

pub fn jet_data_plot_text_channel<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().text_channel(column))
}

pub fn jet_data_plot_detail<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().detail(column))
}

pub fn jet_data_plot_encode<T: 'static>(
    plot: &JetDataPlot<T>,
    channel: JetDataPlotChannel,
    column: JetDataPlotColumn<T>,
    aggregate: JetDataPlotAggregate,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().encode(channel, column, aggregate))
}

pub fn jet_data_plot_with_transform<T: 'static>(
    plot: &JetDataPlot<T>,
    transform: JetDataPlotTransform,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().with_transform(transform))
}

pub fn jet_data_plot_with_scale<T: 'static>(
    plot: &JetDataPlot<T>,
    scale: JetDataPlotScale,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().with_scale(scale))
}

pub fn jet_data_plot_with_axis<T: 'static>(
    plot: &JetDataPlot<T>,
    axis: JetDataPlotAxis,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().with_axis(axis))
}

pub fn jet_data_plot_with_legend<T: 'static>(
    plot: &JetDataPlot<T>,
    legend: JetDataPlotLegend,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().with_legend(legend))
}

pub fn jet_data_plot_facet<T: 'static>(
    plot: &JetDataPlot<T>,
    column: JetDataPlotColumn<T>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().facet(column))
}


pub fn jet_data_plot_with_layer<T: 'static>(
    plot: &JetDataPlot<T>,
    layer: JetDataPlotLayer,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().with_layer(layer))
}

pub fn jet_data_plot_with_interaction<T: 'static>(
    plot: &JetDataPlot<T>,
    interaction: JetDataPlotInteraction,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().with_interaction(interaction))
}

pub fn jet_data_plot_accessibility<T: 'static>(
    plot: &JetDataPlot<T>,
    accessibility: JetDataPlotAccessibility,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().accessibility(accessibility))
}

pub fn jet_data_plot_layout<T: 'static>(
    plot: &JetDataPlot<T>,
    layout: JetDataPlotLayout,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().layout(layout))
}

pub fn jet_data_plot_select_indices<T: 'static>(
    plot: &JetDataPlot<T>,
    indices: Vec<i64>,
) -> Result<JetDataPlot<T>, jet_std::DataError> {
    jet_data_plot_result((*plot).clone().select_indices(indices))
}
pub struct JetDataPlot<T> {
    rows: JetDataPlotArc<Vec<T>>,
    plan: JetDataPlotPlan,
    bindings: Vec<JetDataPlotColumn<T>>,
    selection: Vec<usize>,
}

impl<T> Clone for JetDataPlot<T> {
    fn clone(&self) -> Self {
        Self {
            rows: JetDataPlotArc::clone(&self.rows),
            plan: self.plan.clone(),
            bindings: self.bindings.clone(),
            selection: self.selection.clone(),
        }
    }
}

impl<T> std::fmt::Debug for JetDataPlot<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetDataPlot")
            .field("rows", &self.rows.len())
            .field("plan", &self.plan)
            .field("bindings", &self.bindings)
            .field("selection", &self.selection)
            .finish()
    }
}

impl<T: 'static> JetDataPlot<T> {
    pub fn new(
        rows: Vec<T>,
        source: JetDataPlotSourceFacts,
        schema: JetDataPlotSchema,
    ) -> Result<Self, JetDataPlotError> {
        Self::from_rows(rows, source, schema)
    }

    pub fn new_with_limits(
        rows: Vec<T>,
        source: JetDataPlotSourceFacts,
        schema: JetDataPlotSchema,
        limits: jet_std::DataLimits,
    ) -> Result<Self, JetDataPlotError> {
        Self::from_rows_with_limits(rows, source, schema, limits)
    }

    pub fn from_rows(
        rows: Vec<T>,
        source: JetDataPlotSourceFacts,
        schema: JetDataPlotSchema,
    ) -> Result<Self, JetDataPlotError> {
        Self::from_rows_with_limits(rows, source, schema, jet_std::DataLimits::safe())
    }

    pub fn from_rows_with_limits(
        rows: Vec<T>,
        source: JetDataPlotSourceFacts,
        schema: JetDataPlotSchema,
        limits: jet_std::DataLimits,
    ) -> Result<Self, JetDataPlotError> {
        // `source.row_type` is the canonical Jet plan identity. It must not
        // be compared with the host carrier's Rust type name: comptime rows
        // are erased `CtValue`s while retaining their checked Jet schema.
        let actual_rows = i64::try_from(rows.len()).unwrap_or(i64::MAX);
        if source.rows != actual_rows {
            return Err(JetDataPlotError::invalid(
                "plot.source",
                "plot source row count does not match the typed row list",
            )
            .with_expected(actual_rows.to_string())
            .with_actual(source.rows.to_string()));
        }
        Ok(Self {
            rows: JetDataPlotArc::new(rows),
            plan: JetDataPlotPlan::with_limits(source, schema, limits)?,
            bindings: Vec::new(),
            selection: Vec::new(),
        })
    }

    pub fn plan(&self) -> &JetDataPlotPlan {
        &self.plan
    }

    pub fn source(&self) -> &JetDataPlotSourceFacts {
        &self.plan.source
    }

    pub fn schema(&self) -> &JetDataPlotSchema {
        &self.plan.schema
    }

    pub fn rows(&self) -> &[T] {
        self.rows.as_slice()
    }

    pub fn mark(mut self, mark: JetDataPlotMark) -> Self {
        self.plan.mark = mark;
        self
    }

    pub fn line(self) -> Self {
        self.mark(JetDataPlotMark::Line)
    }

    pub fn bar(self) -> Self {
        self.mark(JetDataPlotMark::Bar)
    }

    pub fn point(self) -> Self {
        self.mark(JetDataPlotMark::Point)
    }

    pub fn bind_column(
        mut self,
        column: JetDataPlotColumn<T>,
    ) -> Result<Self, JetDataPlotError> {
        self.bind_column_mut(column)?;
        Ok(self)
    }

    pub fn encode(
        mut self,
        channel: JetDataPlotChannel,
        column: JetDataPlotColumn<T>,
        aggregate: JetDataPlotAggregate,
    ) -> Result<Self, JetDataPlotError> {
        self.bind_column_mut(column.clone())?;
        self.plan
            .set_encoding(channel, column.field, aggregate)?;
        Ok(self)
    }

    pub fn x(self, column: JetDataPlotColumn<T>) -> Result<Self, JetDataPlotError> {
        self.encode(JetDataPlotChannel::X, column, JetDataPlotAggregate::None)
    }

    pub fn y(
        self,
        column: JetDataPlotColumn<T>,
        aggregate: JetDataPlotAggregate,
    ) -> Result<Self, JetDataPlotError> {
        self.encode(JetDataPlotChannel::Y, column, aggregate)
    }

    pub fn color(self, column: JetDataPlotColumn<T>) -> Result<Self, JetDataPlotError> {
        self.encode(
            JetDataPlotChannel::Color,
            column,
            JetDataPlotAggregate::None,
        )
    }

    pub fn size(self, column: JetDataPlotColumn<T>) -> Result<Self, JetDataPlotError> {
        self.encode(
            JetDataPlotChannel::Size,
            column,
            JetDataPlotAggregate::None,
        )
    }

    pub fn text_channel(self, column: JetDataPlotColumn<T>) -> Result<Self, JetDataPlotError> {
        self.encode(
            JetDataPlotChannel::Text,
            column,
            JetDataPlotAggregate::None,
        )
    }

    pub fn detail(self, column: JetDataPlotColumn<T>) -> Result<Self, JetDataPlotError> {
        self.encode(
            JetDataPlotChannel::Detail,
            column,
            JetDataPlotAggregate::None,
        )
    }

    pub fn with_transform(
        mut self,
        transform: JetDataPlotTransform,
    ) -> Result<Self, JetDataPlotError> {
        self.plan.add_transform(transform)?;
        Ok(self)
    }

    pub fn with_scale(mut self, scale: JetDataPlotScale) -> Result<Self, JetDataPlotError> {
        self.plan.add_scale(scale)?;
        Ok(self)
    }

    pub fn with_axis(mut self, axis: JetDataPlotAxis) -> Result<Self, JetDataPlotError> {
        self.plan.add_axis(axis)?;
        Ok(self)
    }

    pub fn with_legend(mut self, legend: JetDataPlotLegend) -> Result<Self, JetDataPlotError> {
        self.plan.add_legend(legend)?;
        Ok(self)
    }

    pub fn facet(
        mut self,
        column: JetDataPlotColumn<T>,
    ) -> Result<Self, JetDataPlotError> {
        self.bind_column_mut(column.clone())?;
        self.plan.add_facet(JetDataPlotFacet::new(
            column.field,
            JetDataPlotFacetKind::Column,
        ))?;
        Ok(self)
    }

    pub fn with_facet(
        mut self,
        facet: JetDataPlotFacet,
        column: JetDataPlotColumn<T>,
    ) -> Result<Self, JetDataPlotError> {
        self.bind_column_mut(column)?;
        self.plan.add_facet(facet)?;
        Ok(self)
    }

    pub fn with_layer(mut self, layer: JetDataPlotLayer) -> Result<Self, JetDataPlotError> {
        self.plan.add_layer(layer)?;
        Ok(self)
    }

    pub fn with_interaction(
        mut self,
        interaction: JetDataPlotInteraction,
    ) -> Result<Self, JetDataPlotError> {
        self.plan.add_interaction(interaction)?;
        Ok(self)
    }

    pub fn accessibility(
        mut self,
        accessibility: JetDataPlotAccessibility,
    ) -> Result<Self, JetDataPlotError> {
        self.plan.set_accessibility(accessibility)?;
        Ok(self)
    }

    pub fn layout(mut self, layout: JetDataPlotLayout) -> Result<Self, JetDataPlotError> {
        self.plan.set_layout(layout)?;
        Ok(self)
    }

    pub fn select_indices(mut self, indices: Vec<i64>) -> Result<Self, JetDataPlotError> {
        let mut selection = Vec::with_capacity(indices.len());
        for index in indices {
            if index < 0 || usize::try_from(index).map_or(true, |index| index >= self.rows.len())
            {
                return Err(JetDataPlotError::invalid(
                    "plot.select",
                    "selected row index is outside the source table",
                )
                .with_index(usize::try_from(index.max(0)).unwrap_or(usize::MAX)));
            }
            selection.push(index as usize);
        }
        selection.sort_unstable();
        selection.dedup();
        self.selection = selection;
        Ok(self)
    }

    pub fn selected_indices(&self) -> Vec<i64> {
        self.selection
            .iter()
            .map(|index| i64::try_from(*index).unwrap_or(i64::MAX))
            .collect()
    }

    pub fn capabilities(&self, backend: JetDataPlotBackend) -> Vec<JetDataPlotCapability> {
        self.plan.capabilities(backend)
    }

    pub fn projection(
        &self,
        backend: JetDataPlotBackend,
    ) -> Result<JetDataPlotProjection, JetDataPlotError> {
        self.plan.validate_for_render()?;
        Ok(JetDataPlotProjection {
            backend,
            plan: self.plan.clone(),
            source: self.plan.source.clone(),
            capabilities: self.plan.capabilities(backend),
        })
    }

    pub fn inspect(&self) -> Result<JetDataPlotInspection, JetDataPlotError> {
        self.plan.validate_for_render()?;
        let rows = self.materialize_rows()?;
        let fields = jet_data_plot_required_fields(&self.plan);
        let index_value = |index| {
            i64::try_from(index)
                .map_err(|_| JetDataPlotError::invalid("plot.inspect", "row index is outside Int's range"))
        };
        let selected_indices = if self.selection.is_empty() {
            rows.iter()
                .map(|row| index_value(row.ordinal))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            self.selection.iter().copied().map(index_value).collect::<Result<Vec<_>, _>>()?
        };
        let selected = selected_indices
            .iter()
            .filter_map(|index| {
                rows.iter()
                    .find(|row| row.ordinal == *index as usize)
                    .map(|row| JetDataPlotSelectedRow {
                        index: *index,
                        values: fields
                            .iter()
                            .filter_map(|field| {
                                jet_data_plot_datum_value(row, field)
                                    .map(|value| (field.clone(), value.clone()))
                            })
                            .collect(),
                    })
            })
            .collect();
        Ok(JetDataPlotInspection {
            plan: self.plan.clone(),
            selected_indices,
            selected_columns: fields,
            selected_data: selected,
        })
    }

    pub fn inspect_text(&self) -> Result<String, JetDataPlotError> {
        Ok(self.inspect()?.text())
    }

    pub fn inspect_json(&self) -> Result<String, JetDataPlotError> {
        Ok(self.inspect()?.json())
    }

    pub fn text(&self) -> Result<String, JetDataPlotError> {
        Ok(self.render(JetDataPlotBackend::Terminal)?.body)
    }

    pub fn svg(&self) -> Result<String, JetDataPlotError> {
        Ok(self.render(JetDataPlotBackend::Export)?.body)
    }

    pub fn show(&self) -> Result<JetDataPlotRender, JetDataPlotError> {
        self.render(JetDataPlotBackend::Browser)
    }

    pub fn render(
        &self,
        backend: JetDataPlotBackend,
    ) -> Result<JetDataPlotRender, JetDataPlotError> {
        self.plan.validate_for_render()?;
        let rows = self.materialize_rows()?;
        let capabilities = self.plan.capabilities(backend);
        let (format, body) = if backend == JetDataPlotBackend::Terminal {
            (
                JetDataPlotRenderFormat::Text,
                jet_data_plot_render_text(&self.plan, &rows, &capabilities)?,
            )
        } else {
            (
                JetDataPlotRenderFormat::Svg,
                jet_data_plot_render_svg(&self.plan, &rows, &capabilities, &self.selection)?,
            )
        };
        Ok(JetDataPlotRender {
            backend,
            format,
            body,
            source: self.plan.source.clone(),
            capabilities,
        })
    }

    fn bind_column_mut(
        &mut self,
        column: JetDataPlotColumn<T>,
    ) -> Result<(), JetDataPlotError> {
        let field = self.plan.schema.resolve(&column.field)?;
        if let Some(existing) = self
            .bindings
            .iter_mut()
            .find(|existing| existing.field.id == field.id)
        {
            *existing = JetDataPlotColumn {
                field,
                accessor: column.accessor,
            };
        } else {
            self.bindings.push(JetDataPlotColumn {
                field,
                accessor: column.accessor,
            });
        }
        Ok(())
    }

    fn materialize_rows(&self) -> Result<Vec<JetDataPlotDatum>, JetDataPlotError> {
        let fields = jet_data_plot_required_fields(&self.plan);
        let output_limit = usize::try_from(self.plan.limits.max_output_rows).unwrap_or(usize::MAX);
        let mut rows = Vec::with_capacity(self.rows.len().min(output_limit));
        for (ordinal, row) in self.rows.iter().enumerate() {
            let mut values = Vec::with_capacity(fields.len());
            for field in &fields {
                let Some(binding) = self
                    .bindings
                    .iter()
                    .find(|binding| binding.field.id == field.id)
                else {
                    return Err(JetDataPlotError::invalid(
                        "plot.data",
                        "plot plan has no typed accessor for a selected column",
                    )
                    .with_field(field.name.clone()));
                };
                let value = binding.value(row);
                if let JetDataPlotValue::Number(number) = &value {
                    if !number.is_finite() {
                        return Err(JetDataPlotError::non_finite(
                            "plot.data",
                            "plot column contains a non-finite number",
                        )
                        .with_field(field.name.clone())
                        .with_index(ordinal));
                    }
                }
                if let JetDataPlotValue::Text(text) = &value {
                    if text.chars().any(char::is_control) {
                        return Err(JetDataPlotError::invalid(
                            "plot.data",
                            "plot text value must not contain control characters",
                        )
                        .with_field(field.name.clone())
                        .with_index(ordinal));
                    }
                }
                if !jet_data_plot_value_matches_type(&value, &field.type_name) {
                    return Err(JetDataPlotError::invalid(
                        "plot.data",
                        "typed accessor returned a value that disagrees with the schema",
                    )
                    .with_field(field.name.clone())
                    .with_expected(field.type_name.clone())
                    .with_actual(value.kind())
                    .with_index(ordinal));
                }
                values.push((field.id.clone(), value));
            }
            rows.push(JetDataPlotDatum { ordinal, values });
        }
        jet_data_plot_check_output_limit(&rows, &self.plan.limits, "plot.data")?;
        jet_data_plot_apply_transforms(&mut rows, &self.plan.transforms, &self.plan.limits)?;
        jet_data_plot_apply_encoding_aggregates(
            &mut rows,
            &self.plan,
            &self.plan.encodings,
            &self.plan.limits,
        )?;
        jet_data_plot_check_output_limit(&rows, &self.plan.limits, "plot.data")?;
        Ok(rows)
    }
}
#[derive(Clone, Debug)]
struct JetDataPlotDatum {
    ordinal: usize,
    values: Vec<(String, JetDataPlotValue)>,
}

fn jet_data_plot_required_fields(plan: &JetDataPlotPlan) -> Vec<JetDataPlotField> {
    let mut fields = Vec::new();
    for encoding in &plan.encodings {
        jet_data_plot_push_field(&mut fields, &encoding.field);
    }
    for transform in &plan.transforms {
        jet_data_plot_push_transform_fields(&mut fields, transform);
    }
    for facet in &plan.facets {
        jet_data_plot_push_field(&mut fields, &facet.field);
    }
    for layer in &plan.layers {
        for encoding in &layer.encodings {
            jet_data_plot_push_field(&mut fields, &encoding.field);
        }
        for transform in &layer.transforms {
            jet_data_plot_push_transform_fields(&mut fields, transform);
        }
    }
    fields
}

fn jet_data_plot_push_transform_fields(
    fields: &mut Vec<JetDataPlotField>,
    transform: &JetDataPlotTransform,
) {
    match transform {
        JetDataPlotTransform::Filter { field, .. }
        | JetDataPlotTransform::Sort { field, .. }
        | JetDataPlotTransform::Bin { field, .. } => jet_data_plot_push_field(fields, field),
        JetDataPlotTransform::Aggregate {
            group_by, field, ..
        } => {
            for group_field in group_by {
                jet_data_plot_push_field(fields, group_field);
            }
            jet_data_plot_push_field(fields, field);
        }
    }
}

fn jet_data_plot_push_field(fields: &mut Vec<JetDataPlotField>, field: &JetDataPlotField) {
    if !fields.iter().any(|known| known.id == field.id) {
        fields.push(field.clone());
    }
}

fn jet_data_plot_datum_value<'a>(
    datum: &'a JetDataPlotDatum,
    field: &JetDataPlotField,
) -> Option<&'a JetDataPlotValue> {
    datum
        .values
        .iter()
        .find(|(id, _)| id == &field.id)
        .map(|(_, value)| value)
}

fn jet_data_plot_datum_value_mut<'a>(
    datum: &'a mut JetDataPlotDatum,
    field: &JetDataPlotField,
) -> Option<&'a mut JetDataPlotValue> {
    datum
        .values
        .iter_mut()
        .find(|(id, _)| id == &field.id)
        .map(|(_, value)| value)
}

fn jet_data_plot_numeric_type(type_name: &str) -> bool {
    let lower = type_name.trim().to_ascii_lowercase();
    let lower = lower
        .strip_prefix("option<")
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(&lower);
    lower.contains("int")
        || lower.contains("float")
        || lower.contains("number")
        || lower.contains("decimal")
        || lower == "f64"
        || lower == "f32"
}

fn jet_data_plot_value_matches_type(value: &JetDataPlotValue, type_name: &str) -> bool {
    if matches!(value, JetDataPlotValue::Null) {
        return true;
    }
    let lower = type_name.trim().to_ascii_lowercase();
    let lower = lower
        .strip_prefix("option<")
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(&lower);
    match value {
        JetDataPlotValue::Text(_) => {
            lower.contains("str") || lower.contains("string") || lower.contains("text")
        }
        JetDataPlotValue::Integer(_) => {
            lower.contains("int")
                || lower.contains("uint")
                || lower == "i64"
                || lower == "u64"
                || lower == "usize"
        }
        JetDataPlotValue::Number(_) => jet_data_plot_numeric_type(&lower),
        JetDataPlotValue::Boolean(_) => lower.contains("bool"),
        JetDataPlotValue::Null => true,
    }
}
fn jet_data_plot_filter_value_matches_type(
    value: &JetDataPlotValue,
    type_name: &str,
) -> bool {
    match value {
        JetDataPlotValue::Integer(_) | JetDataPlotValue::Number(_)
            if jet_data_plot_numeric_type(type_name) =>
        {
            true
        }
        _ => jet_data_plot_value_matches_type(value, type_name),
    }
}

fn jet_data_plot_compare_values(
    left: &JetDataPlotValue,
    right: &JetDataPlotValue,
) -> std::cmp::Ordering {
    match (left.number(), right.number()) {
        (Some(left), Some(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => left.canonical_key().cmp(&right.canonical_key()),
    }
}

fn jet_data_plot_filter_matches(
    value: Option<&JetDataPlotValue>,
    op: JetDataPlotFilterOp,
    expected: &JetDataPlotValue,
) -> bool {
    let Some(value) = value else {
        return false;
    };
    let ordering = jet_data_plot_compare_values(value, expected);
    match op {
        JetDataPlotFilterOp::Equal => ordering == std::cmp::Ordering::Equal,
        JetDataPlotFilterOp::NotEqual => ordering != std::cmp::Ordering::Equal,
        JetDataPlotFilterOp::Less => ordering == std::cmp::Ordering::Less,
        JetDataPlotFilterOp::LessEqual => ordering != std::cmp::Ordering::Greater,
        JetDataPlotFilterOp::Greater => ordering == std::cmp::Ordering::Greater,
        JetDataPlotFilterOp::GreaterEqual => ordering != std::cmp::Ordering::Less,
    }
}

fn jet_data_plot_check_output_limit(
    rows: &[JetDataPlotDatum],
    limits: &jet_std::DataLimits,
    operation: &str,
) -> Result<(), JetDataPlotError> {
    let actual = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    if actual > limits.max_output_rows {
        return Err(JetDataPlotError {
            kind: JetDataPlotErrorKind::Limit,
            ..JetDataPlotError::invalid(
                operation,
                format!("plot output row limit {} exceeded", limits.max_output_rows),
            )
            .with_expected(limits.max_output_rows.to_string())
            .with_actual(actual.to_string())
        });
    }
    Ok(())
}

fn jet_data_plot_apply_transforms(
    rows: &mut Vec<JetDataPlotDatum>,
    transforms: &[JetDataPlotTransform],
    limits: &jet_std::DataLimits,
) -> Result<(), JetDataPlotError> {
    for transform in transforms {
        match transform {
            JetDataPlotTransform::Filter { field, op, value } => {
                rows.retain(|row| {
                    jet_data_plot_filter_matches(jet_data_plot_datum_value(row, field), *op, value)
                });
            }
            JetDataPlotTransform::Sort {
                field,
                descending,
            } => {
                let actual = i64::try_from(rows.len()).unwrap_or(i64::MAX);
                if actual > limits.max_sort_rows {
                    return Err(JetDataPlotError {
                        kind: JetDataPlotErrorKind::Limit,
                        ..JetDataPlotError::invalid(
                            "plot.sort",
                            format!("plot sort row limit {} exceeded", limits.max_sort_rows),
                        )
                        .with_expected(limits.max_sort_rows.to_string())
                        .with_actual(actual.to_string())
                    });
                }
                rows.sort_by(|left, right| {
                    let left_value = jet_data_plot_datum_value(left, field)
                        .cloned()
                        .unwrap_or(JetDataPlotValue::Null);
                    let right_value = jet_data_plot_datum_value(right, field)
                        .cloned()
                        .unwrap_or(JetDataPlotValue::Null);
                    let ordering = jet_data_plot_compare_values(&left_value, &right_value)
                        .then_with(|| left.ordinal.cmp(&right.ordinal));
                    if *descending {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
            }
            JetDataPlotTransform::Bin { field, step } => {
                for row in rows.iter_mut() {
                    let ordinal = row.ordinal;
                    let Some(value) = jet_data_plot_datum_value_mut(row, field) else {
                        continue;
                    };
                    let Some(number) = value.number() else {
                        continue;
                    };
                    if !number.is_finite() {
                        return Err(JetDataPlotError::non_finite(
                            "plot.transform",
                            "bin transform received a non-finite value",
                        )
                        .with_field(field.name.clone())
                        .with_index(ordinal));
                    }
                    let bucket = (number / step).floor() * step;
                    if !bucket.is_finite() {
                        return Err(JetDataPlotError::non_finite(
                            "plot.transform",
                            "bin transform produced a non-finite bucket",
                        )
                        .with_field(field.name.clone())
                        .with_index(ordinal));
                    }
                    *value = JetDataPlotValue::Number(jet_data_plot_normalize_zero(bucket));
                }
            }
            JetDataPlotTransform::Aggregate {
                group_by,
                field,
                aggregate,
            } => jet_data_plot_aggregate_rows(rows, group_by, field, *aggregate, limits)?,
        }
        jet_data_plot_check_output_limit(rows, limits, "plot.transform")?;
    }
    Ok(())
}

fn jet_data_plot_aggregate_rows(
    rows: &mut Vec<JetDataPlotDatum>,
    group_by: &[JetDataPlotField],
    field: &JetDataPlotField,
    aggregate: JetDataPlotAggregate,
    limits: &jet_std::DataLimits,
) -> Result<(), JetDataPlotError> {
    if aggregate == JetDataPlotAggregate::None {
        return Err(JetDataPlotError::invalid(
            "plot.aggregate",
            "aggregate operation must be concrete",
        )
        .with_field(field.name.clone()));
    }
    let mut groups: BTreeMap<String, (JetDataPlotDatum, i64, f64, Option<f64>, Option<f64>)> =
        BTreeMap::new();
    for row in rows.drain(..) {
        let key = group_by
            .iter()
            .map(|group_field| {
                jet_data_plot_datum_value(&row, group_field)
                    .map(JetDataPlotValue::canonical_key)
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect::<Vec<_>>()
            .join("|");
        let value = jet_data_plot_datum_value(&row, field).and_then(JetDataPlotValue::number);
        let is_new_group = !groups.contains_key(&key);
        if is_new_group
            && i64::try_from(groups.len()).unwrap_or(i64::MAX) >= limits.max_groups
        {
            return Err(JetDataPlotError {
                kind: JetDataPlotErrorKind::Limit,
                ..JetDataPlotError::invalid(
                    "plot.aggregate",
                    format!("plot group limit {} exceeded", limits.max_groups),
                )
                .with_expected(limits.max_groups.to_string())
                .with_actual(groups.len().to_string())
            });
        }
        let entry = groups
            .entry(key)
            .or_insert_with(|| (row.clone(), 0, 0.0, None, None));
        entry.1 = entry.1.saturating_add(1);
        if let Some(value) = value {
            if !value.is_finite() {
                return Err(JetDataPlotError::non_finite(
                    "plot.aggregate",
                    "aggregate received a non-finite value",
                )
                .with_field(field.name.clone())
                .with_index(entry.0.ordinal));
            }
            entry.2 += value;
            entry.3 = Some(entry.3.map_or(value, |current| current.min(value)));
            entry.4 = Some(entry.4.map_or(value, |current| current.max(value)));
        }
    }
    if i64::try_from(groups.len()).unwrap_or(i64::MAX) > limits.max_output_rows {
        return Err(JetDataPlotError {
            kind: JetDataPlotErrorKind::Limit,
            ..JetDataPlotError::invalid(
                "plot.aggregate",
                format!(
                    "plot output row limit {} exceeded",
                    limits.max_output_rows
                ),
            )
            .with_expected(limits.max_output_rows.to_string())
            .with_actual(groups.len().to_string())
        });
    }
    let mut aggregated = Vec::with_capacity(groups.len());
    for (_, (mut row, count, sum, minimum, maximum)) in groups {
        let value = match aggregate {
            JetDataPlotAggregate::Count => JetDataPlotValue::Integer(count),
            JetDataPlotAggregate::Sum => minimum
                .map(|_| JetDataPlotValue::Number(jet_data_plot_normalize_zero(sum)))
                .unwrap_or(JetDataPlotValue::Null),
            JetDataPlotAggregate::Mean => minimum
                .map(|_| {
                    JetDataPlotValue::Number(jet_data_plot_normalize_zero(sum / count as f64))
                })
                .unwrap_or(JetDataPlotValue::Null),
            JetDataPlotAggregate::Min => minimum
                .map(|value| JetDataPlotValue::Number(jet_data_plot_normalize_zero(value)))
                .unwrap_or(JetDataPlotValue::Null),
            JetDataPlotAggregate::Max => maximum
                .map(|value| JetDataPlotValue::Number(jet_data_plot_normalize_zero(value)))
                .unwrap_or(JetDataPlotValue::Null),
            JetDataPlotAggregate::None => unreachable!("validated above"),
        };
        if let Some(existing) = jet_data_plot_datum_value_mut(&mut row, field) {
            *existing = value;
        } else {
            row.values.push((field.id.clone(), value));
        }
        aggregated.push(row);
    }
    *rows = aggregated;
    Ok(())
}

fn jet_data_plot_apply_encoding_aggregates(
    rows: &mut Vec<JetDataPlotDatum>,
    plan: &JetDataPlotPlan,
    encodings: &[JetDataPlotEncoding],
    limits: &jet_std::DataLimits,
) -> Result<(), JetDataPlotError> {
    for encoding in encodings {
        if encoding.aggregate == JetDataPlotAggregate::None {
            continue;
        }
        let group_by = encodings
            .iter()
            .filter(|candidate| {
                candidate.channel != encoding.channel
                    && candidate.aggregate == JetDataPlotAggregate::None
            })
            .map(|candidate| candidate.field.clone())
            .collect::<Vec<_>>();
        jet_data_plot_aggregate_rows(
            rows,
            &group_by,
            &encoding.field,
            encoding.aggregate,
            limits,
        )?;
        jet_data_plot_check_output_limit(rows, limits, "plot.aggregate")?;
        if rows.is_empty() && plan.source.rows > 0 {
            break;
        }
    }
    Ok(())
}

fn jet_data_plot_effective_encodings(
    root: &[JetDataPlotEncoding],
    layer: &[JetDataPlotEncoding],
) -> Vec<JetDataPlotEncoding> {
    let mut effective = root.to_vec();
    for encoding in layer {
        if let Some(existing) = effective
            .iter_mut()
            .find(|existing| existing.channel == encoding.channel)
        {
            *existing = encoding.clone();
        } else {
            effective.push(encoding.clone());
        }
    }
    effective
}

fn jet_data_plot_layer_rows(
    source_rows: &[JetDataPlotDatum],
    plan: &JetDataPlotPlan,
    layer: Option<&JetDataPlotLayer>,
) -> Result<Vec<JetDataPlotDatum>, JetDataPlotError> {
    let Some(layer) = layer else {
        return Ok(source_rows.to_vec());
    };
    let mut rows = source_rows.to_vec();
    jet_data_plot_apply_transforms(&mut rows, &layer.transforms, &plan.limits)?;
    let effective = jet_data_plot_effective_encodings(&plan.encodings, &layer.encodings);
    jet_data_plot_apply_encoding_aggregates(&mut rows, plan, &effective, &plan.limits)?;
    jet_data_plot_check_output_limit(&rows, &plan.limits, "plot.layer")?;
    Ok(rows)
}

fn jet_data_plot_facet_buckets(
    rows: &[JetDataPlotDatum],
    facets: &[JetDataPlotFacet],
) -> Vec<(String, String, Vec<JetDataPlotDatum>)> {
    if facets.is_empty() {
        return vec![("".to_string(), "".to_string(), rows.to_vec())];
    }
    let row_facets = facets
        .iter()
        .filter(|facet| facet.kind == JetDataPlotFacetKind::Row)
        .collect::<Vec<_>>();
    let column_facets = facets
        .iter()
        .filter(|facet| facet.kind == JetDataPlotFacetKind::Column)
        .collect::<Vec<_>>();
    let mut buckets: BTreeMap<(String, String), Vec<JetDataPlotDatum>> = BTreeMap::new();
    for row in rows {
        let row_key = row_facets
            .iter()
            .map(|facet| {
                jet_data_plot_datum_value(row, &facet.field)
                    .map(JetDataPlotValue::display_text)
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect::<Vec<_>>()
            .join(" / ");
        let column_key = column_facets
            .iter()
            .map(|facet| {
                jet_data_plot_datum_value(row, &facet.field)
                    .map(JetDataPlotValue::display_text)
                    .unwrap_or_else(|| "null".to_string())
            })
            .collect::<Vec<_>>()
            .join(" / ");
        buckets
            .entry((row_key, column_key))
            .or_default()
            .push(row.clone());
    }
    buckets
        .into_iter()
        .map(|((row_key, column_key), rows)| (row_key, column_key, rows))
        .collect()
}

fn jet_data_plot_render_text(
    plan: &JetDataPlotPlan,
    rows: &[JetDataPlotDatum],
    capabilities: &[JetDataPlotCapability],
) -> Result<String, JetDataPlotError> {
    let mut out = plan.inspect_text()?;
    out.push_str("\ncapabilities=[");
    out.push_str(
        &capabilities
            .iter()
            .map(JetDataPlotCapability::canonical_key)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push(']');
    let layers = if plan.layers.is_empty() {
        vec![None]
    } else {
        let mut layers = Vec::with_capacity(plan.layers.len() + 1);
        layers.push(None);
        layers.extend(plan.layers.iter().map(Some));
        layers
    };
    let fields = jet_data_plot_required_fields(plan);
    for (layer_index, layer) in layers.into_iter().enumerate() {
        let layer_rows = jet_data_plot_layer_rows(rows, plan, layer)?;
        let encodings = layer
            .map(|layer| jet_data_plot_effective_encodings(&plan.encodings, &layer.encodings))
            .unwrap_or_else(|| plan.encodings.clone());
        let mark = layer.map_or(plan.mark, |layer| layer.mark);
        out.push_str(&format!(
            "\nlayer={} name={} mark={} opacity={}",
            layer_index,
            layer.map_or("base", |layer| layer.name.as_str()),
            mark.as_str(),
            jet_data_plot_number(layer.map_or(1.0, |layer| layer.opacity))
        ));
        out.push_str(" encodings=[");
        out.push_str(
            &encodings
                .iter()
                .map(JetDataPlotEncoding::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push(']');
        for (row_key, column_key, bucket) in jet_data_plot_facet_buckets(&layer_rows, &plan.facets) {
            out.push_str(&format!(
                "\nfacet[row={},column={}] rows=[",
                jet_data_plot_escape(&row_key),
                jet_data_plot_escape(&column_key)
            ));
            for (position, row) in bucket.iter().enumerate() {
                if position > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{}:{{{}}}",
                    row.ordinal,
                    fields
                        .iter()
                        .filter_map(|field| {
                            jet_data_plot_datum_value(row, field)
                                .map(|value| {
                                    format!(
                                        "{}={}",
                                        jet_data_plot_escape(&field.name),
                                        jet_data_plot_escape(&value.display_text())
                                    )
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            out.push(']');
        }
    }
    Ok(out)
}

fn jet_data_plot_plan_json(plan: &JetDataPlotPlan) -> String {
    let mut out = String::from("{");
    out.push_str(&format!(
        "\"version\":{},\"source\":{},\"schema\":{},\"limits\":{{\"max_groups\":{},\"max_sort_rows\":{},\"max_join_rows\":{},\"max_output_rows\":{}}},\"mark\":\"{}\",\"encodings\":[",
        JET_DATA_PLOT_SCHEMA_VERSION,
        jet_data_plot_source_json(&plan.source),
        jet_data_plot_schema_json(&plan.schema),
        plan.limits.max_groups,
        plan.limits.max_sort_rows,
        plan.limits.max_join_rows,
        plan.limits.max_output_rows,
        plan.mark.as_str()
    ));
    out.push_str(
        &plan
            .encodings
            .iter()
            .map(jet_data_plot_encoding_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"transforms\":[");
    out.push_str(
        &plan
            .transforms
            .iter()
            .map(jet_data_plot_transform_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"scales\":[");
    out.push_str(
        &plan
            .scales
            .iter()
            .map(jet_data_plot_scale_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"axes\":[");
    out.push_str(
        &plan
            .axes
            .iter()
            .map(jet_data_plot_axis_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"legends\":[");
    out.push_str(
        &plan
            .legends
            .iter()
            .map(jet_data_plot_legend_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"facets\":[");
    out.push_str(
        &plan
            .facets
            .iter()
            .map(jet_data_plot_facet_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"layers\":[");
    out.push_str(
        &plan
            .layers
            .iter()
            .map(jet_data_plot_layer_json)
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"interactions\":[");
    out.push_str(
        &plan
            .interactions
            .iter()
            .map(|interaction| format!("\"{}\"", interaction.as_str()))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\"accessibility\":");
    out.push_str(&format!(
        "{{\"title\":\"{}\",\"description\":\"{}\",\"summary\":\"{}\",\"keyboard\":{},\"announce_selection\":{}}}",
        jet_data_plot_json_escape(&plan.accessibility.title),
        jet_data_plot_json_escape(&plan.accessibility.description),
        jet_data_plot_json_escape(&plan.accessibility.summary),
        plan.accessibility.keyboard,
        plan.accessibility.announce_selection
    ));
    out.push_str(&format!(
        ",\"layout\":{{\"width\":{},\"height\":{},\"margin_top\":{},\"margin_right\":{},\"margin_bottom\":{},\"margin_left\":{}}}}}",
        jet_data_plot_number(plan.layout.width),
        jet_data_plot_number(plan.layout.height),
        jet_data_plot_number(plan.layout.margin_top),
        jet_data_plot_number(plan.layout.margin_right),
        jet_data_plot_number(plan.layout.margin_bottom),
        jet_data_plot_number(plan.layout.margin_left)
    ));
    out
}
fn jet_data_plot_json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => out.push(character),
        }
    }
    out
}

fn jet_data_plot_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace('|', "\\|")
        .replace(',', "\\,")
        .replace('=', "\\=")
}

fn jet_data_plot_number(value: f64) -> String {
    let value = jet_data_plot_normalize_zero(value);
    if value.is_finite() {
        value.to_string()
    } else if value.is_nan() {
        "NaN".to_string()
    } else if value.is_sign_positive() {
        "inf".to_string()
    } else {
        "-inf".to_string()
    }
}

fn jet_data_plot_normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn jet_data_plot_source_json(source: &JetDataPlotSourceFacts) -> String {
    format!(
        "{{\"table_plan\":\"{}\",\"source\":\"{}\",\"schema\":\"{}\",\"row_type\":\"{}\",\"rows\":{},\"data_identity\":\"{}\",\"provenance\":\"{}\"}}",
        jet_data_plot_json_escape(&source.table_plan_identity),
        jet_data_plot_json_escape(&source.source_identity),
        jet_data_plot_json_escape(&source.schema_identity),
        jet_data_plot_json_escape(&source.row_type),
        source.rows,
        jet_data_plot_json_escape(&source.data_identity),
        jet_data_plot_json_escape(&source.provenance)
    )
}
fn jet_data_plot_limits_key(limits: &jet_std::DataLimits) -> String {
    format!(
        "groups={},sort={},join={},output={}",
        limits.max_groups, limits.max_sort_rows, limits.max_join_rows, limits.max_output_rows
    )
}


fn jet_data_plot_schema_json(schema: &JetDataPlotSchema) -> String {
    format!(
        "{{\"identity\":\"{}\",\"row_type\":\"{}\",\"columns\":[{}]}}",
        jet_data_plot_json_escape(&schema.identity),
        jet_data_plot_json_escape(&schema.row_type),
        schema
            .columns
            .iter()
            .map(|field| {
                format!(
                    "{{\"id\":\"{}\",\"name\":\"{}\",\"type\":\"{}\"}}",
                    jet_data_plot_json_escape(&field.id),
                    jet_data_plot_json_escape(&field.name),
                    jet_data_plot_json_escape(&field.type_name)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn jet_data_plot_encoding_json(encoding: &JetDataPlotEncoding) -> String {
    format!(
        "{{\"channel\":\"{}\",\"field\":{},\"aggregate\":\"{}\"}}",
        encoding.channel.as_str(),
        jet_data_plot_field_json(&encoding.field),
        encoding.aggregate.as_str()
    )
}

fn jet_data_plot_field_json(field: &JetDataPlotField) -> String {
    format!(
        "{{\"id\":\"{}\",\"name\":\"{}\",\"type\":\"{}\"}}",
        jet_data_plot_json_escape(&field.id),
        jet_data_plot_json_escape(&field.name),
        jet_data_plot_json_escape(&field.type_name)
    )
}

fn jet_data_plot_value_json(value: &JetDataPlotValue) -> String {
    match value {
        JetDataPlotValue::Text(value) => {
            format!("{{\"kind\":\"text\",\"value\":\"{}\"}}", jet_data_plot_json_escape(value))
        }
        JetDataPlotValue::Integer(value) => {
            format!("{{\"kind\":\"integer\",\"value\":{value}}}")
        }
        JetDataPlotValue::Number(value) => format!(
            "{{\"kind\":\"number\",\"value\":{}}}",
            jet_data_plot_number(*value)
        ),
        JetDataPlotValue::Boolean(value) => {
            format!("{{\"kind\":\"boolean\",\"value\":{value}}}")
        }
        JetDataPlotValue::Null => "{\"kind\":\"null\",\"value\":null}".to_string(),
    }
}

fn jet_data_plot_values_json(values: &[(JetDataPlotField, JetDataPlotValue)]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|(field, value)| {
                format!(
                    "{{\"field\":{},\"value\":{}}}",
                    jet_data_plot_field_json(field),
                    jet_data_plot_value_json(value)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn jet_data_plot_transform_json(transform: &JetDataPlotTransform) -> String {
    match transform {
        JetDataPlotTransform::Filter { field, op, value } => format!(
            "{{\"kind\":\"filter\",\"field\":{},\"op\":\"{}\",\"value\":{}}}",
            jet_data_plot_field_json(field),
            op.as_str(),
            jet_data_plot_value_json(value)
        ),
        JetDataPlotTransform::Sort {
            field,
            descending,
        } => format!(
            "{{\"kind\":\"sort\",\"field\":{},\"descending\":{descending}}}",
            jet_data_plot_field_json(field)
        ),
        JetDataPlotTransform::Bin { field, step } => format!(
            "{{\"kind\":\"bin\",\"field\":{},\"step\":{}}}",
            jet_data_plot_field_json(field),
            jet_data_plot_number(*step)
        ),
        JetDataPlotTransform::Aggregate {
            group_by,
            field,
            aggregate,
        } => format!(
            "{{\"kind\":\"aggregate\",\"group_by\":[{}],\"field\":{},\"aggregate\":\"{}\"}}",
            group_by
                .iter()
                .map(jet_data_plot_field_json)
                .collect::<Vec<_>>()
                .join(","),
            jet_data_plot_field_json(field),
            aggregate.as_str()
        ),
    }
}

fn jet_data_plot_domain_json(domain: &JetDataPlotDomain) -> String {
    match domain {
        JetDataPlotDomain::Auto => "\"auto\"".to_string(),
        JetDataPlotDomain::Numeric { min, max } => format!(
            "{{\"kind\":\"numeric\",\"min\":{},\"max\":{}}}",
            jet_data_plot_number(*min),
            jet_data_plot_number(*max)
        ),
        JetDataPlotDomain::Categories(values) => format!(
            "{{\"kind\":\"categories\",\"values\":[{}]}}",
            values
                .iter()
                .map(|value| format!("\"{}\"", jet_data_plot_json_escape(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn jet_data_plot_scale_json(scale: &JetDataPlotScale) -> String {
    format!(
        "{{\"channel\":\"{}\",\"kind\":\"{}\",\"domain\":{},\"clamp\":{},\"reverse\":{}}}",
        scale.channel.as_str(),
        scale.kind.as_str(),
        jet_data_plot_domain_json(&scale.domain),
        scale.clamp,
        scale.reverse
    )
}

fn jet_data_plot_axis_json(axis: &JetDataPlotAxis) -> String {
    format!(
        "{{\"channel\":\"{}\",\"title\":\"{}\",\"visible\":{},\"grid\":{},\"ticks\":{}}}",
        axis.channel.as_str(),
        jet_data_plot_json_escape(&axis.title),
        axis.visible,
        axis.grid,
        axis.ticks
    )
}

fn jet_data_plot_legend_json(legend: &JetDataPlotLegend) -> String {
    format!(
        "{{\"channel\":\"{}\",\"title\":\"{}\",\"position\":\"{}\",\"visible\":{}}}",
        legend.channel.as_str(),
        jet_data_plot_json_escape(&legend.title),
        legend.position.as_str(),
        legend.visible
    )
}

fn jet_data_plot_facet_json(facet: &JetDataPlotFacet) -> String {
    format!(
        "{{\"field\":{},\"kind\":\"{}\",\"title\":\"{}\",\"columns\":{},\"rows\":{}}}",
        jet_data_plot_field_json(&facet.field),
        facet.kind.as_str(),
        jet_data_plot_json_escape(&facet.title),
        facet.columns,
        facet.rows
    )
}

fn jet_data_plot_layer_json(layer: &JetDataPlotLayer) -> String {
    format!(
        "{{\"name\":\"{}\",\"mark\":\"{}\",\"opacity\":{},\"encodings\":[{}],\"transforms\":[{}]}}",
        jet_data_plot_json_escape(&layer.name),
        layer.mark.as_str(),
        jet_data_plot_number(layer.opacity),
        layer
            .encodings
            .iter()
            .map(jet_data_plot_encoding_json)
            .collect::<Vec<_>>()
            .join(","),
        layer
            .transforms
            .iter()
            .map(jet_data_plot_transform_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn jet_data_plot_render_svg(
    plan: &JetDataPlotPlan,
    rows: &[JetDataPlotDatum],
    capabilities: &[JetDataPlotCapability],
    selection: &[usize],
) -> Result<String, JetDataPlotError> {
    let layout = &plan.layout;
    let buckets = jet_data_plot_facet_buckets(rows, &plan.facets);
    let bucket_count = buckets.len().max(1);
    let configured_columns = plan
        .facets
        .iter()
        .map(|facet| facet.columns)
        .find(|columns| *columns > 0)
        .map(|columns| columns as usize);
    let configured_rows = plan
        .facets
        .iter()
        .map(|facet| facet.rows)
        .find(|rows| *rows > 0)
        .map(|rows| rows as usize);
    let requested_columns = configured_columns.unwrap_or_else(|| {
        (bucket_count as f64).sqrt().ceil() as usize
    });
    let rows_count = configured_rows
        .unwrap_or_else(|| bucket_count.div_ceil(requested_columns.max(1)))
        .clamp(1, bucket_count);
    let columns = requested_columns
        .max(bucket_count.div_ceil(rows_count))
        .clamp(1, bucket_count);
    let chart_width = layout.width - layout.margin_left - layout.margin_right;
    let chart_height = layout.height - layout.margin_top - layout.margin_bottom;
    let cell_width = chart_width / columns as f64;
    let cell_height = chart_height / rows_count as f64;
    let label = if plan.accessibility.title.is_empty() {
        "Jet data plot"
    } else {
        plan.accessibility.title.as_str()
    };
    let interaction_contract = plan
        .interactions
        .iter()
        .map(|interaction| interaction.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let capability_contract = capabilities
        .iter()
        .map(JetDataPlotCapability::canonical_key)
        .collect::<Vec<_>>()
        .join(",");
    let selected_contract = selection
        .iter()
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"{}\">",
        jet_data_plot_number(layout.width),
        jet_data_plot_number(layout.height),
        jet_data_plot_number(layout.width),
        jet_data_plot_number(layout.height),
        jet_data_plot_svg_escape(label)
    );
    if !plan.accessibility.title.is_empty() {
        out.push_str(&format!(
            "<title>{}</title>",
            jet_data_plot_svg_escape(&plan.accessibility.title)
        ));
    }
    if !plan.accessibility.description.is_empty() {
        out.push_str(&format!(
            "<desc>{}</desc>",
            jet_data_plot_svg_escape(&plan.accessibility.description)
        ));
    }
    out.push_str("<metadata>");
    out.push_str(&jet_data_plot_svg_escape(&jet_data_plot_plan_json(plan)));
    out.push_str("</metadata>");
    out.push_str(&format!(
        "<g data-source=\"{}\" data-table-plan=\"{}\" data-data-identity=\"{}\" data-interactions=\"{}\" data-capabilities=\"{}\" data-selection=\"{}\">",
        jet_data_plot_svg_escape(&plan.source.source_identity),
        jet_data_plot_svg_escape(&plan.source.table_plan_identity),
        jet_data_plot_svg_escape(&plan.source.data_identity),
        jet_data_plot_svg_escape(&interaction_contract),
        jet_data_plot_svg_escape(&capability_contract),
        jet_data_plot_svg_escape(&selected_contract)
    ));
    if !plan.accessibility.summary.is_empty() {
        out.push_str(&format!(
            "<text class=\"plot-summary\" x=\"{}\" y=\"{}\">{}</text>",
            jet_data_plot_number(layout.margin_left),
            jet_data_plot_number(layout.margin_top - 20.0),
            jet_data_plot_svg_escape(&plan.accessibility.summary)
        ));
    }

    let layer_count = plan.layers.len() + 1;
    let mut facet_index = 0;
    while facet_index < bucket_count {
        let (facet_row, facet_column, _facet_rows) = buckets
            .get(facet_index)
            .cloned()
            .unwrap_or_else(|| ("".to_string(), "".to_string(), Vec::new()));
        let column = facet_index % columns;
        let row = facet_index / columns;
        let left = layout.margin_left + column as f64 * cell_width;
        let top = layout.margin_top + row as f64 * cell_height;
        let area_left = left + 8.0;
        let area_top = top + 8.0;
        let area_width = (cell_width - 16.0).max(1.0);
        let area_height = (cell_height - 16.0).max(1.0);
        if !facet_row.is_empty() || !facet_column.is_empty() {
            out.push_str(&format!(
                "<text class=\"facet\" x=\"{}\" y=\"{}\">{}</text>",
                jet_data_plot_number(area_left),
                jet_data_plot_number(area_top + 2.0),
                jet_data_plot_svg_escape(&format!("{facet_row} {facet_column}"))
            ));
        }
        let mut layer_index = 0;
        while layer_index < layer_count {
            let layer = if layer_index == 0 {
                None
            } else {
                plan.layers.get(layer_index - 1)
            };
            let effective = layer
                .map(|layer| jet_data_plot_effective_encodings(&plan.encodings, &layer.encodings))
                .unwrap_or_else(|| plan.encodings.clone());
            let layer_rows = if let Some(layer) = layer {
                jet_data_plot_layer_rows(rows, plan, Some(layer))?
            } else {
                rows.to_vec()
            };
            let layer_rows = if plan.facets.is_empty() {
                layer_rows
            } else {
                jet_data_plot_facet_buckets(&layer_rows, &plan.facets)
                    .into_iter()
                    .find(|(row_key, column_key, _)| {
                        *row_key == facet_row && *column_key == facet_column
                    })
                    .map(|(_, _, rows)| rows)
                    .unwrap_or_default()
            };
            let mark = layer.map_or(plan.mark, |layer| layer.mark);
            let opacity = layer.map_or(1.0, |layer| layer.opacity);
            out.push_str(&format!(
                "<g class=\"plot-layer\" data-layer=\"{}\" data-mark=\"{}\" opacity=\"{}\">",
                layer_index,
                mark.as_str(),
                jet_data_plot_number(opacity)
            ));
            jet_data_plot_svg_draw_axes(
                &mut out,
                plan,
                &effective,
                &layer_rows,
                area_left,
                area_top,
                area_width,
                area_height,
            )?;
            jet_data_plot_svg_draw_marks(
                &mut out,
                plan,
                &effective,
                &layer_rows,
                mark,
                area_left,
                area_top,
                area_width,
                area_height,
            )?;
            out.push_str("</g>");
            layer_index += 1;
        }
        facet_index += 1;
    }
    jet_data_plot_svg_draw_legends(&mut out, plan, rows)?;
    out.push_str("</g></svg>");
    let _ = capabilities;
    Ok(out)
}
fn jet_data_plot_svg_draw_legends(
    out: &mut String,
    plan: &JetDataPlotPlan,
    rows: &[JetDataPlotDatum],
) -> Result<(), JetDataPlotError> {
    let mut top_x = plan.layout.margin_left;
    let mut bottom_x = plan.layout.margin_left;
    let mut right_y = plan.layout.margin_top;
    let mut left_y = plan.layout.margin_top;
    for legend in &plan.legends {
        if !legend.visible {
            continue;
        }
        let Some(encoding) = jet_data_plot_channel_encoding(&plan.encodings, legend.channel) else {
            continue;
        };
        let categories = jet_data_plot_categories(rows, &encoding.field, None);
        let (x, y, vertical) = match legend.position {
            JetDataPlotLegendPosition::Top => {
                let position = (top_x, (plan.layout.margin_top - 6.0).max(12.0), false);
                top_x += (legend.title.len() as f64 * 7.0).max(24.0) + 10.0;
                position
            }
            JetDataPlotLegendPosition::Bottom => {
                let position = (
                    bottom_x,
                    (plan.layout.height - plan.layout.margin_bottom + 20.0)
                        .min(plan.layout.height - 4.0),
                    false,
                );
                bottom_x += (legend.title.len() as f64 * 7.0).max(24.0) + 10.0;
                position
            }
            JetDataPlotLegendPosition::Right => {
                let position = (
                    (plan.layout.width - plan.layout.margin_right + 8.0)
                        .min(plan.layout.width - 8.0),
                    right_y,
                    true,
                );
                right_y += 20.0;
                position
            }
            JetDataPlotLegendPosition::Left => {
                let position = (
                    (plan.layout.margin_left - 120.0).max(8.0),
                    left_y,
                    true,
                );
                left_y += 20.0;
                position
            }
        };
        out.push_str(&format!(
            "<g class=\"legend\" data-channel=\"{}\" data-position=\"{}\"><text x=\"{}\" y=\"{}\">{}</text>",
            legend.channel.as_str(),
            legend.position.as_str(),
            jet_data_plot_number(x),
            jet_data_plot_number(y),
            jet_data_plot_svg_escape(&legend.title)
        ));
        if vertical {
            let mut category_y = y + 10.0;
            for category in categories {
                let key = JetDataPlotValue::Text(category.clone()).canonical_key();
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"8\" height=\"8\" fill=\"{}\"/><text x=\"{}\" y=\"{}\">{}</text>",
                    jet_data_plot_number(x),
                    jet_data_plot_number(category_y),
                    jet_data_plot_palette_color(&key),
                    jet_data_plot_number(x + 12.0),
                    jet_data_plot_number(category_y + 8.0),
                    jet_data_plot_svg_escape(&category)
                ));
                category_y += 20.0;
            }
        } else {
            let mut category_x = x + (legend.title.len() as f64 * 7.0).max(24.0) + 10.0;
            for category in categories {
                let key = JetDataPlotValue::Text(category.clone()).canonical_key();
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"8\" height=\"8\" fill=\"{}\"/><text x=\"{}\" y=\"{}\">{}</text>",
                    jet_data_plot_number(category_x),
                    jet_data_plot_number(y - 8.0),
                    jet_data_plot_palette_color(&key),
                    jet_data_plot_number(category_x + 12.0),
                    jet_data_plot_number(y),
                    jet_data_plot_svg_escape(&category)
                ));
                category_x += (category.len() as f64 * 7.0).max(24.0) + 22.0;
            }
        }
        out.push_str("</g>");
    }
    Ok(())
}


fn jet_data_plot_channel_encoding<'a>(
    encodings: &'a [JetDataPlotEncoding],
    channel: JetDataPlotChannel,
) -> Option<&'a JetDataPlotEncoding> {
    encodings.iter().find(|encoding| encoding.channel == channel)
}

fn jet_data_plot_channel_scale<'a>(
    plan: &'a JetDataPlotPlan,
    channel: JetDataPlotChannel,
) -> Option<&'a JetDataPlotScale> {
    plan.scales
        .iter()
        .find(|scale| scale.channel == channel)
}

fn jet_data_plot_channel_axis<'a>(
    plan: &'a JetDataPlotPlan,
    channel: JetDataPlotChannel,
) -> Option<&'a JetDataPlotAxis> {
    plan.axes
        .iter()
        .find(|axis| axis.channel == channel)
}

fn jet_data_plot_numeric_bounds(
    rows: &[JetDataPlotDatum],
    field: &JetDataPlotField,
    scale: Option<&JetDataPlotScale>,
) -> Result<(f64, f64), JetDataPlotError> {
    if let Some(JetDataPlotScale {
        domain: JetDataPlotDomain::Numeric { min, max },
        kind,
        ..
    }) = scale
    {
        if *kind == JetDataPlotScaleKind::Log {
            for row in rows {
                let Some(value) =
                    jet_data_plot_datum_value(row, field).and_then(JetDataPlotValue::number)
                else {
                    continue;
                };
                if !value.is_finite() || value <= 0.0 {
                    return Err(JetDataPlotError::invalid(
                        "plot.scale",
                        "log plot scale needs strictly positive finite data",
                    )
                    .with_field(field.name.clone())
                    .with_index(row.ordinal));
                }
            }
        }
        return Ok((*min, *max));
    }
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for row in rows {
        let Some(value) = jet_data_plot_datum_value(row, field).and_then(JetDataPlotValue::number)
        else {
            continue;
        };
        if !value.is_finite() {
            return Err(JetDataPlotError::non_finite(
                "plot.scale",
                "plot scale received a non-finite value",
            )
            .with_field(field.name.clone())
            .with_index(row.ordinal));
        }
        if scale.is_some_and(|scale| scale.kind == JetDataPlotScaleKind::Log) && value <= 0.0 {
            return Err(JetDataPlotError::invalid(
                "plot.scale",
                "log plot scale needs strictly positive data",
            )
            .with_field(field.name.clone())
            .with_index(row.ordinal));
        }
        minimum = minimum.min(value);
        maximum = maximum.max(value);
    }
    if !minimum.is_finite() || !maximum.is_finite() {
        return Ok((0.0, 1.0));
    }
    if minimum == maximum {
        let padding = minimum.abs().max(1.0) * 0.5;
        minimum -= padding;
        maximum += padding;
    }
    Ok((minimum, maximum))
}

fn jet_data_plot_categories(
    rows: &[JetDataPlotDatum],
    field: &JetDataPlotField,
    scale: Option<&JetDataPlotScale>,
) -> Vec<String> {
    if let Some(JetDataPlotScale {
        domain: JetDataPlotDomain::Categories(values),
        ..
    }) = scale
    {
        return values.clone();
    }
    let mut categories = BTreeSet::new();
    for row in rows {
        if let Some(value) = jet_data_plot_datum_value(row, field) {
            categories.insert(value.display_text());
        }
    }
    categories.into_iter().collect()
}

fn jet_data_plot_svg_x(
    value: &JetDataPlotValue,
    scale: Option<&JetDataPlotScale>,
    categories: &[String],
    minimum: f64,
    maximum: f64,
    left: f64,
    width: f64,
) -> f64 {
    let numeric = scale
        .map(|scale| scale.kind.needs_numeric())
        .unwrap_or_else(|| value.number().is_some());
    let reverse = scale.map(|scale| scale.reverse).unwrap_or(false);
    if numeric {
        let mut ratio = value.number().unwrap_or(minimum);
        if scale.is_some_and(|scale| scale.kind == JetDataPlotScaleKind::Log) {
            ratio = ratio.max(f64::MIN_POSITIVE).ln();
            let min_log = minimum.max(f64::MIN_POSITIVE).ln();
            let max_log = maximum.max(f64::MIN_POSITIVE).ln();
            ratio = (ratio - min_log) / (max_log - min_log).max(f64::EPSILON);
        } else {
            ratio = (ratio - minimum) / (maximum - minimum).max(f64::EPSILON);
        }
        let ratio = if scale.map(|scale| scale.clamp).unwrap_or(true) {
            ratio.clamp(0.0, 1.0)
        } else {
            ratio
        };
        let ratio = if reverse { 1.0 - ratio } else { ratio };
        left + ratio * width
    } else {
        let key = value.display_text();
        let index = categories.iter().position(|category| category == &key).unwrap_or(0);
        let step = width / categories.len().max(1) as f64;
        let position = index as f64 * step + step * 0.5;
        if reverse {
            left + width - position
        } else {
            left + position
        }
    }
}

fn jet_data_plot_svg_y(
    value: &JetDataPlotValue,
    scale: Option<&JetDataPlotScale>,
    minimum: f64,
    maximum: f64,
    top: f64,
    height: f64,
) -> f64 {
    let mut ratio = value.number().unwrap_or(minimum);
    if scale.is_some_and(|scale| scale.kind == JetDataPlotScaleKind::Log) {
        ratio = ratio.max(f64::MIN_POSITIVE).ln();
        let min_log = minimum.max(f64::MIN_POSITIVE).ln();
        let max_log = maximum.max(f64::MIN_POSITIVE).ln();
        ratio = (ratio - min_log) / (max_log - min_log).max(f64::EPSILON);
    } else {
        ratio = (ratio - minimum) / (maximum - minimum).max(f64::EPSILON);
    }
    let ratio = if scale.map(|scale| scale.clamp).unwrap_or(true) {
        ratio.clamp(0.0, 1.0)
    } else {
        ratio
    };
    if scale.is_some_and(|scale| scale.reverse) {
        top + ratio * height
    } else {
        top + (1.0 - ratio) * height
    }
}
fn jet_data_plot_svg_draw_axes(
    out: &mut String,
    plan: &JetDataPlotPlan,
    encodings: &[JetDataPlotEncoding],
    rows: &[JetDataPlotDatum],
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<(), JetDataPlotError> {
    let x_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::X)
        .ok_or_else(|| JetDataPlotError::invalid("plot.svg", "plot mark has no x encoding"))?;
    let y_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::Y)
        .ok_or_else(|| JetDataPlotError::invalid("plot.svg", "plot mark has no y encoding"))?;
    let x_scale = jet_data_plot_channel_scale(plan, JetDataPlotChannel::X);
    let y_scale = jet_data_plot_channel_scale(plan, JetDataPlotChannel::Y);
    let x_categories = jet_data_plot_categories(rows, &x_encoding.field, x_scale);
    let x_numeric = x_scale
        .map(|scale| scale.kind.needs_numeric())
        .unwrap_or_else(|| {
            rows.iter().any(|row| {
                jet_data_plot_datum_value(row, &x_encoding.field)
                    .is_some_and(|value| value.number().is_some())
            })
        });
    let (x_minimum, x_maximum) = if x_numeric {
        jet_data_plot_numeric_bounds(rows, &x_encoding.field, x_scale)?
    } else {
        (0.0, 1.0)
    };
    let (y_minimum, y_maximum) =
        jet_data_plot_numeric_bounds(rows, &y_encoding.field, y_scale)?;
    let x_axis = jet_data_plot_channel_axis(plan, JetDataPlotChannel::X);
    let y_axis = jet_data_plot_channel_axis(plan, JetDataPlotChannel::Y);
    if y_axis.is_none_or(|axis| axis.visible) {
        out.push_str(&format!(
            "<line class=\"axis y\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
            jet_data_plot_number(left),
            jet_data_plot_number(top),
            jet_data_plot_number(left),
            jet_data_plot_number(top + height)
        ));
    }
    if x_axis.is_none_or(|axis| axis.visible) {
        out.push_str(&format!(
            "<line class=\"axis x\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
            jet_data_plot_number(left),
            jet_data_plot_number(top + height),
            jet_data_plot_number(left + width),
            jet_data_plot_number(top + height)
        ));
    }
    let y_ticks = y_axis.map_or(5, |axis| axis.ticks).clamp(0, 32) as usize;
    for tick in 0..=y_ticks {
        let ratio = if y_ticks == 0 {
            0.0
        } else {
            tick as f64 / y_ticks as f64
        };
        let value = y_minimum + (y_maximum - y_minimum) * ratio;
        let position = jet_data_plot_svg_y(
            &JetDataPlotValue::Number(value),
            y_scale,
            y_minimum,
            y_maximum,
            top,
            height,
        );
        if y_axis.is_none_or(|axis| axis.grid) {
            out.push_str(&format!(
                "<line class=\"grid\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                jet_data_plot_number(left),
                jet_data_plot_number(position),
                jet_data_plot_number(left + width),
                jet_data_plot_number(position)
            ));
        }
        out.push_str(&format!(
            "<text class=\"tick y\" x=\"{}\" y=\"{}\">{}</text>",
            jet_data_plot_number(left - 6.0),
            jet_data_plot_number(position + 3.0),
            jet_data_plot_svg_escape(&jet_data_plot_number(value))
        ));
    }
    if x_numeric {
        let x_ticks = x_axis.map_or(5, |axis| axis.ticks).clamp(0, 32) as usize;
        for tick in 0..=x_ticks {
            let ratio = if x_ticks == 0 {
                0.0
            } else {
                tick as f64 / x_ticks as f64
            };
            let value = x_minimum + (x_maximum - x_minimum) * ratio;
            let position = jet_data_plot_svg_x(
                &JetDataPlotValue::Number(value),
                x_scale,
                &x_categories,
                x_minimum,
                x_maximum,
                left,
                width,
            );
            out.push_str(&format!(
                "<text class=\"tick x\" x=\"{}\" y=\"{}\">{}</text>",
                jet_data_plot_number(position),
                jet_data_plot_number(top + height + 18.0),
                jet_data_plot_svg_escape(&jet_data_plot_number(value))
            ));
        }
    } else if !x_categories.is_empty() {
        for category in &x_categories {
            let position = jet_data_plot_svg_x(
                &JetDataPlotValue::Text(category.clone()),
                x_scale,
                &x_categories,
                x_minimum,
                x_maximum,
                left,
                width,
            );
            out.push_str(&format!(
                "<text class=\"tick x\" x=\"{}\" y=\"{}\">{}</text>",
                jet_data_plot_number(position),
                jet_data_plot_number(top + height + 18.0),
                jet_data_plot_svg_escape(category)
            ));
        }
    }
    if let Some(axis) = x_axis {
        if !axis.title.is_empty() {
            out.push_str(&format!(
                "<text class=\"axis-title x\" x=\"{}\" y=\"{}\">{}</text>",
                jet_data_plot_number(left + width * 0.5),
                jet_data_plot_number(top + height + 38.0),
                jet_data_plot_svg_escape(&axis.title)
            ));
        }
    }
    if let Some(axis) = y_axis {
        if !axis.title.is_empty() {
            out.push_str(&format!(
                "<text class=\"axis-title y\" x=\"{}\" y=\"{}\" transform=\"rotate(-90 {} {})\">{}</text>",
                jet_data_plot_number(left - 46.0),
                jet_data_plot_number(top + height * 0.5),
                jet_data_plot_number(left - 46.0),
                jet_data_plot_number(top + height * 0.5),
                jet_data_plot_svg_escape(&axis.title)
            ));
        }
    }
    Ok(())
}

fn jet_data_plot_svg_draw_marks(
    out: &mut String,
    plan: &JetDataPlotPlan,
    encodings: &[JetDataPlotEncoding],
    rows: &[JetDataPlotDatum],
    mark: JetDataPlotMark,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Result<(), JetDataPlotError> {
    let x_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::X)
        .ok_or_else(|| JetDataPlotError::invalid("plot.svg", "plot mark has no x encoding"))?;
    let y_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::Y)
        .ok_or_else(|| JetDataPlotError::invalid("plot.svg", "plot mark has no y encoding"))?;
    let x_scale = jet_data_plot_channel_scale(plan, JetDataPlotChannel::X);
    let y_scale = jet_data_plot_channel_scale(plan, JetDataPlotChannel::Y);
    let categories = jet_data_plot_categories(rows, &x_encoding.field, x_scale);
    let x_numeric = x_scale
        .map(|scale| scale.kind.needs_numeric())
        .unwrap_or_else(|| {
            rows.iter().any(|row| {
                jet_data_plot_datum_value(row, &x_encoding.field)
                    .is_some_and(|value| value.number().is_some())
            })
        });
    let (x_minimum, x_maximum) = if x_numeric {
        jet_data_plot_numeric_bounds(rows, &x_encoding.field, x_scale)?
    } else {
        (0.0, 1.0)
    };
    let (y_minimum, y_maximum) =
        jet_data_plot_numeric_bounds(rows, &y_encoding.field, y_scale)?;
    let color_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::Color);
    let size_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::Size);
    let text_encoding = jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::Text);
    match mark {
        JetDataPlotMark::Line => {
            let mut paths: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
            for row in rows {
                let Some(x_value) = jet_data_plot_datum_value(row, &x_encoding.field) else {
                    continue;
                };
                let Some(y_value) = jet_data_plot_datum_value(row, &y_encoding.field) else {
                    continue;
                };
                let Some(y_number) = y_value.number() else {
                    continue;
                };
                let group = color_encoding
                    .and_then(|encoding| jet_data_plot_datum_value(row, &encoding.field))
                    .map(JetDataPlotValue::canonical_key)
                    .unwrap_or_else(|| "default".to_string());
                let group = if let Some(encoding) =
                    jet_data_plot_channel_encoding(encodings, JetDataPlotChannel::Detail)
                {
                    format!(
                        "{}|{}",
                        group,
                        jet_data_plot_datum_value(row, &encoding.field)
                            .map(JetDataPlotValue::canonical_key)
                            .unwrap_or_else(|| "null".to_string())
                    )
                } else {
                    group
                };
                let x = jet_data_plot_svg_x(
                    x_value,
                    x_scale,
                    &categories,
                    x_minimum,
                    x_maximum,
                    left,
                    width,
                );
                let y = jet_data_plot_svg_y(
                    y_value,
                    y_scale,
                    y_minimum,
                    y_maximum,
                    top,
                    height,
                );
                if y_number.is_finite() {
                    paths.entry(group).or_default().push((x, y));
                }
            }
            for (group, points) in paths {
                if points.is_empty() {
                    continue;
                }
                let points = points
                    .iter()
                    .map(|(x, y)| {
                        format!("{} {}", jet_data_plot_number(*x), jet_data_plot_number(*y))
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(&format!(
                    "<polyline class=\"mark line\" data-series=\"{}\" points=\"{}\" fill=\"none\" stroke=\"{}\"/>",
                    jet_data_plot_svg_escape(&group),
                    points,
                    jet_data_plot_palette_color(&group)
                ));
            }
            if let Some(encoding) = text_encoding {
                for row in rows {
                    let Some(x_value) = jet_data_plot_datum_value(row, &x_encoding.field) else {
                        continue;
                    };
                    let Some(y_value) = jet_data_plot_datum_value(row, &y_encoding.field) else {
                        continue;
                    };
                    if y_value.number().is_none() {
                        continue;
                    }
                    let x = jet_data_plot_svg_x(
                        x_value,
                        x_scale,
                        &categories,
                        x_minimum,
                        x_maximum,
                        left,
                        width,
                    );
                    let y = jet_data_plot_svg_y(
                        y_value,
                        y_scale,
                        y_minimum,
                        y_maximum,
                        top,
                        height,
                    );
                    if let Some(value) = jet_data_plot_datum_value(row, &encoding.field) {
                        out.push_str(&format!(
                            "<text class=\"mark-label\" x=\"{}\" y=\"{}\">{}</text>",
                            jet_data_plot_number(x + 2.0),
                            jet_data_plot_number(y - 4.0),
                            jet_data_plot_svg_escape(&value.display_text())
                        ));
                    }
                }
            }
        }
        JetDataPlotMark::Bar => {
            let count = rows.len().max(1) as f64;
            let band_width = width / count * 0.78;
            let baseline = jet_data_plot_svg_y(
                &JetDataPlotValue::Number(0.0),
                y_scale,
                y_minimum,
                y_maximum,
                top,
                height,
            );
            for (position, row) in rows.iter().enumerate() {
                let Some(x_value) = jet_data_plot_datum_value(row, &x_encoding.field) else {
                    continue;
                };
                let Some(y_value) = jet_data_plot_datum_value(row, &y_encoding.field) else {
                    continue;
                };
                if y_value.number().is_none() {
                    continue;
                }
                let x = if x_numeric {
                    jet_data_plot_svg_x(
                        x_value,
                        x_scale,
                        &categories,
                        x_minimum,
                        x_maximum,
                        left,
                        width,
                    ) - band_width * 0.5
                } else {
                    left + (position as f64 + 0.5) * width / count - band_width * 0.5
                };
                let y = jet_data_plot_svg_y(
                    y_value,
                    y_scale,
                    y_minimum,
                    y_maximum,
                    top,
                    height,
                );
                let top_edge = y.min(baseline);
                let bar_height = (y - baseline).abs().max(1.0);
                let color_key = color_encoding
                    .and_then(|encoding| jet_data_plot_datum_value(row, &encoding.field))
                    .map(JetDataPlotValue::canonical_key)
                    .unwrap_or_else(|| "default".to_string());
                out.push_str(&format!(
                    "<rect class=\"mark bar\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    jet_data_plot_number(x),
                    jet_data_plot_number(top_edge),
                    jet_data_plot_number(band_width),
                    jet_data_plot_number(bar_height),
                    jet_data_plot_palette_color(&color_key)
                ));
                if let Some(encoding) = text_encoding {
                    if let Some(value) = jet_data_plot_datum_value(row, &encoding.field) {
                        out.push_str(&format!(
                            "<text class=\"mark-label\" x=\"{}\" y=\"{}\">{}</text>",
                            jet_data_plot_number(x + band_width * 0.5),
                            jet_data_plot_number(top_edge - 4.0),
                            jet_data_plot_svg_escape(&value.display_text())
                        ));
                    }
                }
            }
        }
        JetDataPlotMark::Point => {
            for row in rows {
                let Some(x_value) = jet_data_plot_datum_value(row, &x_encoding.field) else {
                    continue;
                };
                let Some(y_value) = jet_data_plot_datum_value(row, &y_encoding.field) else {
                    continue;
                };
                if y_value.number().is_none() {
                    continue;
                }
                let x = jet_data_plot_svg_x(
                    x_value,
                    x_scale,
                    &categories,
                    x_minimum,
                    x_maximum,
                    left,
                    width,
                );
                let y = jet_data_plot_svg_y(
                    y_value,
                    y_scale,
                    y_minimum,
                    y_maximum,
                    top,
                    height,
                );
                let radius = size_encoding
                    .and_then(|encoding| jet_data_plot_datum_value(row, &encoding.field))
                    .and_then(JetDataPlotValue::number)
                    .map(|value| 2.0 + value.abs().sqrt().min(7.0))
                    .unwrap_or(4.0);
                let color_key = color_encoding
                    .and_then(|encoding| jet_data_plot_datum_value(row, &encoding.field))
                    .map(JetDataPlotValue::canonical_key)
                    .unwrap_or_else(|| "default".to_string());
                out.push_str(&format!(
                    "<circle class=\"mark point\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>",
                    jet_data_plot_number(x),
                    jet_data_plot_number(y),
                    jet_data_plot_number(radius),
                    jet_data_plot_palette_color(&color_key)
                ));
                if let Some(encoding) = text_encoding {
                    if let Some(value) = jet_data_plot_datum_value(row, &encoding.field) {
                        out.push_str(&format!(
                            "<text class=\"mark-label\" x=\"{}\" y=\"{}\">{}</text>",
                            jet_data_plot_number(x + radius + 2.0),
                            jet_data_plot_number(y - radius - 2.0),
                            jet_data_plot_svg_escape(&value.display_text())
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn jet_data_plot_palette_color(key: &str) -> &'static str {
    const COLORS: [&str; 8] = [
        "#2563eb", "#dc2626", "#059669", "#d97706", "#7c3aed", "#0891b2", "#db2777",
        "#4f46e5",
    ];
    let mut hash = 0_u64;
    for byte in key.bytes() {
        hash = hash.wrapping_mul(131).wrapping_add(u64::from(byte));
    }
    COLORS[(hash as usize) % COLORS.len()]
}
