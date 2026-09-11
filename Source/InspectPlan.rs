//! Read-only projection of typed table-plan facts for inspect/devtools.
//!
//! This module does not check source files and does not dispatch a CLI command.
//! It keeps the projection separate so `CmdInspect` can later wire it to the
//! existing command surface without creating a second plan analyzer.

use std::fmt::Write as _;

use jet_foundation::AST::{
    DataPlanFact, DataPlanLogicalNode, DataPlanPhysicalNode, DataPlanStreamMode,
    DataPlanValidationError,
};
use jet_foundation::JSON::json_escape;

/// Stable machine-readable schema name for `inspect plan` output.
pub const INSPECT_PLAN_SCHEMA: &str = "jet.inspect.plan/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanColumn {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanCallable {
    pub label: String,
    pub parameter: String,
    pub result: String,
    pub span: InspectPlanSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanStream {
    pub mode: String,
    pub batch_rows: Option<usize>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanNode {
    pub id: usize,
    pub operation: String,
    pub inputs: Vec<usize>,
    pub source: Option<String>,
    pub row_type: String,
    pub schema: Vec<InspectPlanColumn>,
    pub callable: Option<InspectPlanCallable>,
    pub stream: InspectPlanStream,
    pub span: InspectPlanSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanPhysicalNode {
    pub logical: usize,
    pub operator: String,
    pub stream: InspectPlanStream,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectPlanSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanStreaming {
    pub mode: String,
    pub batch_rows: Option<usize>,
    pub fallbacks: Vec<String>,
}

/// Complete text/JSON projection of one checked plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPlanProjection {
    pub schema: &'static str,
    pub source: String,
    pub output: usize,
    pub row_type: String,
    pub columns: Vec<InspectPlanColumn>,
    pub logical: Vec<InspectPlanNode>,
    pub physical: Vec<InspectPlanPhysicalNode>,
    pub streaming: InspectPlanStreaming,
    pub canonical_key: String,
}

impl InspectPlanProjection {
    pub fn from_fact(
        source: impl Into<String>,
        fact: &DataPlanFact,
    ) -> Result<Self, InspectPlanError> {
        fact.validate().map_err(InspectPlanError::Validation)?;
        let output = fact
            .output_node()
            .ok_or(InspectPlanError::MissingOutput)?;
        let logical = fact.logical.iter().map(project_logical).collect::<Vec<_>>();
        let physical = fact
            .physical
            .iter()
            .map(project_physical)
            .collect::<Vec<_>>();
        let streaming = project_streaming(fact);
        Ok(Self {
            schema: INSPECT_PLAN_SCHEMA,
            source: source.into(),
            output: fact.output.index(),
            row_type: output.schema.row_type.name(),
            columns: output
                .schema
                .columns
                .iter()
                .map(|column| InspectPlanColumn {
                    name: column.name.clone(),
                    ty: column.ty.name(),
                })
                .collect(),
            logical,
            physical,
            streaming,
            canonical_key: fact.canonical_key(),
        })
    }

    /// Stable human output.  Node order is the checked topological order, and
    /// no target implementation detail is printed.
    pub fn render_text(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "plan {}", self.source);
        let _ = writeln!(output, "schema: {}", self.row_type);
        if self.columns.is_empty() {
            output.push_str("  columns: (none)\n");
        } else {
            output.push_str("  columns:\n");
            for column in &self.columns {
                let _ = writeln!(output, "    {}: {}", column.name, column.ty);
            }
        }
        let _ = writeln!(output, "logical (output={}):", self.output);
        for node in &self.logical {
            let inputs = if node.inputs.is_empty() {
                "-".to_string()
            } else {
                node.inputs
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let source = node
                .source
                .as_deref()
                .map_or_else(String::new, |source| format!(" source={source}"));
            let _ = writeln!(
                output,
                "  #{} {} <- [{}]{}  type={}  stream={} {}  span={}..{}",
                node.id,
                node.operation,
                inputs,
                source,
                node.row_type,
                node.stream.mode,
                stream_detail(&node.stream),
                node.span.start,
                node.span.end
            );
            if let Some(callable) = &node.callable {
                let _ = writeln!(
                    output,
                    "    callable {}: {} -> {} ({}..{})",
                    callable.label,
                    callable.parameter,
                    callable.result,
                    callable.span.start,
                    callable.span.end
                );
            }
        }
        output.push_str("physical:\n");
        for node in &self.physical {
            let _ = writeln!(
                output,
                "  logical=#{} {}  stream={} {}  reason={}",
                node.logical,
                node.operator,
                node.stream.mode,
                stream_detail(&node.stream),
                node.reason
            );
        }
        let batch_rows = self
            .streaming
            .batch_rows
            .map_or_else(|| "host".to_string(), |rows| rows.to_string());
        let fallbacks = if self.streaming.fallbacks.is_empty() {
            "none".to_string()
        } else {
            self.streaming.fallbacks.join(" | ")
        };
        let _ = writeln!(
            output,
            "streaming: mode={} batch_rows={} fallbacks={}",
            self.streaming.mode, batch_rows, fallbacks
        );
        output
    }

    /// Stable machine output with fixed field ordering and escaped strings.
    pub fn render_json(&self) -> String {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                    json_escape(&column.name),
                    json_escape(&column.ty)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let logical = self
            .logical
            .iter()
            .map(render_logical_json)
            .collect::<Vec<_>>()
            .join(",");
        let physical = self
            .physical
            .iter()
            .map(render_physical_json)
            .collect::<Vec<_>>()
            .join(",");
        let fallbacks = self
            .streaming
            .fallbacks
            .iter()
            .map(|fallback| format!("\"{}\"", json_escape(fallback)))
            .collect::<Vec<_>>()
            .join(",");
        let batch_rows = self
            .streaming
            .batch_rows
            .map_or_else(|| "null".to_string(), |rows| rows.to_string());
        format!(
            "{{\"schema\":\"{}\",\"source\":\"{}\",\"output\":{},\"row_type\":\"{}\",\"columns\":[{}],\"logical\":[{}],\"physical\":[{}],\"streaming\":{{\"mode\":\"{}\",\"batch_rows\":{},\"fallbacks\":[{}]}},\"canonical_key\":\"{}\"}}",
            self.schema,
            json_escape(&self.source),
            self.output,
            json_escape(&self.row_type),
            columns,
            logical,
            physical,
            json_escape(&self.streaming.mode),
            batch_rows,
            fallbacks,
            json_escape(&self.canonical_key),
        )
    }
}

/// Pure entry point used by command and devtools wiring.
pub fn project_plan(
    source: impl Into<String>,
    fact: &DataPlanFact,
) -> Result<InspectPlanProjection, InspectPlanError> {
    InspectPlanProjection::from_fact(source, fact)
}

/// Machine-readable error projection for a plan that failed fact validation.
pub fn render_plan_error_json(error: &InspectPlanError) -> String {
    format!(
        "{{\"schema\":\"{}\",\"status\":\"error\",\"error\":\"{}\"}}",
        INSPECT_PLAN_SCHEMA,
        json_escape(&error.to_string())
    )
}

fn project_logical(node: &DataPlanLogicalNode) -> InspectPlanNode {
    InspectPlanNode {
        id: node.id.index(),
        operation: node.operation.as_str().to_string(),
        inputs: node.inputs.iter().map(|input| input.index()).collect(),
        source: node.source.map(|source| source.as_str().to_string()),
        row_type: node.schema.row_type.name(),
        schema: node
            .schema
            .columns
            .iter()
            .map(|column| InspectPlanColumn {
                name: column.name.clone(),
                ty: column.ty.name(),
            })
            .collect(),
        callable: node.callable.as_ref().map(|callable| InspectPlanCallable {
            label: callable.label.clone(),
            parameter: callable.parameter.name(),
            result: callable.result.name(),
            span: InspectPlanSpan {
                start: callable.span.start,
                end: callable.span.end,
            },
        }),
        stream: project_stream(&node.stream),
        span: InspectPlanSpan {
            start: node.span.start,
            end: node.span.end,
        },
    }
}

fn project_physical(node: &DataPlanPhysicalNode) -> InspectPlanPhysicalNode {
    InspectPlanPhysicalNode {
        logical: node.logical.index(),
        operator: node.operator.as_str().to_string(),
        stream: project_stream(&node.stream),
        reason: node.reason.clone(),
    }
}

fn project_stream(stream: &DataPlanStreamMode) -> InspectPlanStream {
    match stream {
        DataPlanStreamMode::Bounded { batch_rows } => InspectPlanStream {
            mode: "streaming".to_string(),
            batch_rows: *batch_rows,
            reason: None,
        },
        DataPlanStreamMode::Materialized { reason } => InspectPlanStream {
            mode: "materialized".to_string(),
            batch_rows: None,
            reason: Some(reason.clone()),
        },
    }
}

fn project_streaming(fact: &DataPlanFact) -> InspectPlanStreaming {
    let output = fact.output_node();
    let stream = output
        .map(|node| project_stream(&node.stream))
        .unwrap_or_else(|| InspectPlanStream {
            mode: "unknown".to_string(),
            batch_rows: None,
            reason: None,
        });
    let mut fallbacks = Vec::new();
    for node in &fact.logical {
        if let DataPlanStreamMode::Materialized { reason } = &node.stream {
            fallbacks.push(format!("logical#{}: {}", node.id.index(), reason));
        }
    }
    for node in &fact.physical {
        if let DataPlanStreamMode::Materialized { reason } = &node.stream {
            fallbacks.push(format!("physical#{}: {}", node.logical.index(), reason));
        }
    }
    InspectPlanStreaming {
        mode: stream.mode,
        batch_rows: stream.batch_rows,
        fallbacks,
    }
}

fn stream_detail(stream: &InspectPlanStream) -> String {
    match (&stream.batch_rows, &stream.reason) {
        (Some(rows), _) => format!("batch_rows={rows}"),
        (None, Some(reason)) => format!("reason={reason}"),
        (None, None) => "batch_rows=host".to_string(),
    }
}

fn render_logical_json(node: &InspectPlanNode) -> String {
    let inputs = node
        .inputs
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let source = node
        .source
        .as_deref()
        .map_or_else(|| "null".to_string(), |source| json_string(source));
    let columns = node
        .schema
        .iter()
        .map(|column| {
            format!(
                "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                json_escape(&column.name),
                json_escape(&column.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let callable = node
        .callable
        .as_ref()
        .map(render_callable_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"id\":{},\"operation\":\"{}\",\"inputs\":[{}],\"source\":{},\"row_type\":\"{}\",\"schema\":[{}],\"callable\":{},\"stream\":{},\"span\":{{\"start\":{},\"end\":{}}}}}",
        node.id,
        json_escape(&node.operation),
        inputs,
        source,
        json_escape(&node.row_type),
        columns,
        callable,
        render_stream_json(&node.stream),
        node.span.start,
        node.span.end,
    )
}

fn render_callable_json(callable: &InspectPlanCallable) -> String {
    format!(
        "{{\"label\":\"{}\",\"parameter\":\"{}\",\"result\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}}}}",
        json_escape(&callable.label),
        json_escape(&callable.parameter),
        json_escape(&callable.result),
        callable.span.start,
        callable.span.end,
    )
}

fn render_stream_json(stream: &InspectPlanStream) -> String {
    let batch_rows = stream
        .batch_rows
        .map_or_else(|| "null".to_string(), |rows| rows.to_string());
    let reason = stream
        .reason
        .as_deref()
        .map_or_else(|| "null".to_string(), json_string);
    format!(
        "{{\"mode\":\"{}\",\"batch_rows\":{},\"reason\":{}}}",
        json_escape(&stream.mode),
        batch_rows,
        reason
    )
}

fn render_physical_json(node: &InspectPlanPhysicalNode) -> String {
    format!(
        "{{\"logical\":{},\"operator\":\"{}\",\"stream\":{},\"reason\":\"{}\"}}",
        node.logical,
        json_escape(&node.operator),
        render_stream_json(&node.stream),
        json_escape(&node.reason),
    )
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectPlanError {
    Validation(DataPlanValidationError),
    MissingOutput,
}

impl std::fmt::Display for InspectPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::MissingOutput => formatter.write_str("table plan has no output node"),
        }
    }
}

impl std::error::Error for InspectPlanError {}
