//! Backend-neutral facts for typed lazy table plans.
//!
//! This module deliberately contains no rows, closures, host handles, or
//! lowering decisions.  Sema records the checked logical graph here; later
//! stages may carry the same facts into TIR and tooling without re-inference.

use crate::Diagnostics::Span;

use super::Type;

/// Version of the serialized table-plan fact shape.
pub const DATA_PLAN_FACT_SCHEMA_VERSION: u32 = 1;

/// Stable identifier for a node in a plan's topologically ordered graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DataPlanNodeId(pub usize);

impl DataPlanNodeId {
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Typed operations that may occur in a lazy table plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataPlanOperationKind {
    Scan,
    Filter,
    SortBy,
    InnerJoin,
    LeftJoin,
    Group,
    Window,
    Project,
    Collect,
}

impl DataPlanOperationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Filter => "filter",
            Self::SortBy => "sort_by",
            Self::InnerJoin => "inner_join",
            Self::LeftJoin => "left_join",
            Self::Group => "group",
            Self::Window => "window",
            Self::Project => "project",
            Self::Collect => "collect",
        }
    }

    /// The backend-neutral physical operator used by the identity plan.
    pub const fn default_physical_operator(self) -> DataPlanPhysicalOperatorKind {
        match self {
            Self::Scan => DataPlanPhysicalOperatorKind::Scan,
            Self::Filter => DataPlanPhysicalOperatorKind::Filter,
            Self::SortBy => DataPlanPhysicalOperatorKind::SortBy,
            Self::InnerJoin => DataPlanPhysicalOperatorKind::InnerJoin,
            Self::LeftJoin => DataPlanPhysicalOperatorKind::LeftJoin,
            Self::Group => DataPlanPhysicalOperatorKind::Group,
            Self::Window => DataPlanPhysicalOperatorKind::Window,
            Self::Project => DataPlanPhysicalOperatorKind::Project,
            Self::Collect => DataPlanPhysicalOperatorKind::Materialize,
        }
    }
}

/// The source shape recorded by a typed scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataPlanSourceKind {
    Table,
    Series,
    Csv,
    Json,
    Jsonl,
    Parquet,
    Arrow,
    Loader,
    CsvReader,
    JsonReader,
}

impl DataPlanSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Series => "series",
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Parquet => "parquet",
            Self::Arrow => "arrow",
            Self::Loader => "loader",
            Self::CsvReader => "csv_reader",
            Self::JsonReader => "json_reader",
        }
    }
}

/// One named field in a table row schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlanColumn {
    pub name: String,
    pub ty: Type,
    pub nullable: bool,
}

impl DataPlanColumn {
    pub fn new(name: impl Into<String>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: false,
        }
    }

    pub fn with_nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }
}

/// The checked row type and ordered columns carried by every plan node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlanSchema {
    pub row_type: Type,
    pub columns: Vec<DataPlanColumn>,
}


impl DataPlanSchema {
    pub fn new(row_type: Type, columns: Vec<DataPlanColumn>) -> Self {
        Self { row_type, columns }
    }

    pub fn column(&self, name: &str) -> Option<&DataPlanColumn> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// Reject duplicate names before a source can enter the plan graph.
    pub fn validate(&self) -> Result<(), DataPlanSchemaError> {
        for (index, column) in self.columns.iter().enumerate() {
            if self
                .columns
                .iter()
                .take(index)
                .any(|previous| previous.name == column.name)
            {
                return Err(DataPlanSchemaError::DuplicateColumn(column.name.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlanSchemaError {
    DuplicateColumn(String),
}

impl std::fmt::Display for DataPlanSchemaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateColumn(name) => write!(formatter, "duplicate table column `{name}`"),
        }
    }
}

impl std::error::Error for DataPlanSchemaError {}

/// A resolved callable fact attached to a filter, sort, or future transform.
///
/// `label` is explanatory metadata only.  It is never a dynamic closure and
/// therefore cannot make the compiler or inspector depend on a backend value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlanCallable {
    pub label: String,
    pub parameter: Type,
    pub result: Type,
    pub span: Span,
}

impl DataPlanCallable {
    pub fn new(
        label: impl Into<String>,
        parameter: Type,
        result: Type,
        span: Span,
    ) -> Self {
        Self {
            label: label.into(),
            parameter,
            result,
            span,
        }
    }
}

/// Whether a node can stay in bounded streaming mode or must materialize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlanStreamMode {
    /// The operation may be consumed in bounded batches.  `None` means that
    /// the batch size is selected by the execution host, not by semantics.
    Bounded { batch_rows: Option<usize> },
    /// An explicit materialization boundary.  The reason is part of the fact;
    /// consumers must not silently turn a stream into an in-memory table.
    Materialized { reason: String },
}

impl DataPlanStreamMode {
    pub fn bounded(batch_rows: Option<usize>) -> Result<Self, DataPlanStreamError> {
        if batch_rows == Some(0) {
            return Err(DataPlanStreamError::ZeroBatchSize);
        }
        Ok(Self::Bounded { batch_rows })
    }

    pub fn materialized(reason: impl Into<String>) -> Result<Self, DataPlanStreamError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(DataPlanStreamError::MissingMaterializationReason);
        }
        Ok(Self::Materialized { reason })
    }

    pub const fn is_streaming(&self) -> bool {
        matches!(self, Self::Bounded { .. })
    }

    pub const fn mode_name(&self) -> &'static str {
        match self {
            Self::Bounded { .. } => "streaming",
            Self::Materialized { .. } => "materialized",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Bounded { batch_rows: Some(rows) } => format!("batch_rows={rows}"),
            Self::Bounded { batch_rows: None } => "batch_rows=host".to_string(),
            Self::Materialized { reason } => format!("reason={reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlanStreamError {
    ZeroBatchSize,
    MissingMaterializationReason,
}

impl std::fmt::Display for DataPlanStreamError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroBatchSize => formatter.write_str("table plan batch size must be greater than zero"),
            Self::MissingMaterializationReason => {
                formatter.write_str("table plan materialization needs a reason")
            }
        }
    }
}

impl std::error::Error for DataPlanStreamError {}

/// One node in the checked logical plan graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlanLogicalNode {
    pub id: DataPlanNodeId,
    pub operation: DataPlanOperationKind,
    pub inputs: Vec<DataPlanNodeId>,
    pub source: Option<DataPlanSourceKind>,
    pub schema: DataPlanSchema,
    pub callable: Option<DataPlanCallable>,
    pub stream: DataPlanStreamMode,
    pub span: Span,
}

/// Backend-neutral physical operators.  These names describe plan facts, not
/// a Rust, JIT, interpreter, or browser implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataPlanPhysicalOperatorKind {
    Scan,
    Filter,
    SortBy,
    InnerJoin,
    LeftJoin,
    Group,
    Window,
    Project,
    Batch,
    Materialize,
}

impl DataPlanPhysicalOperatorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Filter => "filter",
            Self::SortBy => "sort_by",
            Self::InnerJoin => "inner_join",
            Self::LeftJoin => "left_join",
            Self::Group => "group",
            Self::Window => "window",
            Self::Project => "project",
            Self::Batch => "batch",
            Self::Materialize => "materialize",
        }
    }
}

/// One optimizer choice associated with a logical node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlanPhysicalNode {
    pub logical: DataPlanNodeId,
    pub operator: DataPlanPhysicalOperatorKind,
    pub stream: DataPlanStreamMode,
    pub reason: String,
}

/// Complete typed plan fact.  `logical` and `physical` are ordered by node
/// creation, making projection deterministic without a map or a second pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlanFact {
    pub source: DataPlanSourceKind,
    pub logical: Vec<DataPlanLogicalNode>,
    pub physical: Vec<DataPlanPhysicalNode>,
    pub output: DataPlanNodeId,
    pub source_span: Span,
}

impl DataPlanFact {
    pub fn scan(source: DataPlanSourceKind, schema: DataPlanSchema, span: Span) -> Self {
        let logical = vec![DataPlanLogicalNode {
            id: DataPlanNodeId(0),
            operation: DataPlanOperationKind::Scan,
            inputs: Vec::new(),
            source: Some(source),
            schema,
            callable: None,
            stream: DataPlanStreamMode::Bounded { batch_rows: None },
            span,
        }];
        let physical = vec![DataPlanPhysicalNode {
            logical: DataPlanNodeId(0),
            operator: DataPlanPhysicalOperatorKind::Scan,
            stream: DataPlanStreamMode::Bounded { batch_rows: None },
            reason: "source scan".to_string(),
        }];
        Self {
            source,
            logical,
            physical,
            output: DataPlanNodeId(0),
            source_span: span,
        }
    }

    /// Append one already-typed logical node and return its stable id.
    pub fn append_logical(
        &mut self,
        operation: DataPlanOperationKind,
        inputs: Vec<DataPlanNodeId>,
        schema: DataPlanSchema,
        callable: Option<DataPlanCallable>,
        stream: DataPlanStreamMode,
        span: Span,
    ) -> DataPlanNodeId {
        let id = DataPlanNodeId(self.logical.len());
        self.logical.push(DataPlanLogicalNode {
            id,
            operation,
            inputs,
            source: None,
            schema,
            callable,
            stream,
            span,
        });
        self.output = id;
        id
    }

    /// Record an optimizer choice without introducing execution machinery.
    pub fn append_physical(
        &mut self,
        logical: DataPlanNodeId,
        operator: DataPlanPhysicalOperatorKind,
        stream: DataPlanStreamMode,
        reason: impl Into<String>,
    ) {
        self.physical.push(DataPlanPhysicalNode {
            logical,
            operator,
            stream,
            reason: reason.into(),
        });
    }

    pub fn output_node(&self) -> Option<&DataPlanLogicalNode> {
        self.logical.get(self.output.index())
    }

    pub fn output_schema(&self) -> Option<&DataPlanSchema> {
        self.output_node().map(|node| &node.schema)
    }

    /// A stable, human-readable identity for cache keys and inspector tests.
    ///
    /// The identity includes the complete typed shape.  Operation names and
    /// row types alone are not enough: two plans can have the same graph while
    /// selecting different columns or carrying different checked callables.
    pub fn canonical_key(&self) -> String {
        let logical = self
            .logical
            .iter()
            .map(|node| {
                let inputs = node
                    .inputs
                    .iter()
                    .map(|input| input.index().to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let source = node
                    .source
                    .map(DataPlanSourceKind::as_str)
                    .unwrap_or("none");
                let columns = node
                    .schema
                    .columns
                    .iter()
                    .map(|column| format!("{:?}:{}:nullable={}", column.name, column.ty.name(), column.nullable))
                    .collect::<Vec<_>>()
                    .join(",");
                let callable = node
                    .callable
                    .as_ref()
                    .map(|callable| {
                        format!(
                            "{:?}:{}:{}:{}-{}",
                            callable.label,
                            callable.parameter.name(),
                            callable.result.name(),
                            callable.span.start,
                            callable.span.end
                        )
                    })
                    .unwrap_or_else(|| "none".to_string());
                format!(
                    "{}:{}:{}:source={source}:row={}:columns=[{columns}]:callable={callable}:stream={:?}:span={}-{}",
                    node.id.index(),
                    node.operation.as_str(),
                    inputs,
                    node.schema.row_type.name(),
                    node.stream,
                    node.span.start,
                    node.span.end
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        let physical = self
            .physical
            .iter()
            .map(|node| {
                format!(
                    "{}:{}:stream={:?}:reason={:?}",
                    node.logical.index(),
                    node.operator.as_str(),
                    node.stream,
                    node.reason
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        format!(
            "source={};output={};logical={logical};physical={physical}",
            self.source.as_str(),
            self.output.index()
        )
    }

    /// Check graph shape and all typed callback/schema facts before a later
    /// stage accepts this plan.
    pub fn validate(&self) -> Result<(), DataPlanValidationError> {
        if self.logical.is_empty() {
            return Err(DataPlanValidationError::EmptyPlan);
        }
        if self.logical[0].operation != DataPlanOperationKind::Scan
            || self.logical[0].source != Some(self.source)
        {
            return Err(DataPlanValidationError::MissingScan);
        }
        if self.output.index() >= self.logical.len() {
            return Err(DataPlanValidationError::InvalidOutput(self.output));
        }
        for (index, node) in self.logical.iter().enumerate() {
            if node.id.index() != index {
                return Err(DataPlanValidationError::NodeIdMismatch {
                    expected: index,
                    actual: node.id,
                });
            }
            node.schema
                .validate()
                .map_err(DataPlanValidationError::Schema)?;
            for input in &node.inputs {
                if input.index() >= index {
                    return Err(DataPlanValidationError::InputNotEarlier {
                        node: node.id,
                        input: *input,
                    });
                }
            }
            if matches!(node.operation, DataPlanOperationKind::Filter | DataPlanOperationKind::SortBy)
                && node.callable.is_none()
            {
                return Err(DataPlanValidationError::MissingCallable(node.id));
            }
            if let Some(callable) = &node.callable {
                if let Some(input) = node.inputs.first().and_then(|id| self.logical.get(id.index())) {
                    if callable.parameter != input.schema.row_type {
                        return Err(DataPlanValidationError::CallbackParameter {
                            node: node.id,
                            expected: input.schema.row_type.name(),
                            actual: callable.parameter.name(),
                        });
                    }
                }
                let expected = match node.operation {
                    DataPlanOperationKind::Filter => Some(Type::Bool),
                    DataPlanOperationKind::SortBy => Some(Type::String),
                    _ => None,
                };
                if let Some(expected) = expected {
                    if callable.result != expected {
                        return Err(DataPlanValidationError::CallbackResult {
                            node: node.id,
                            expected: expected.name(),
                            actual: callable.result.name(),
                        });
                    }
                }
            }
            if let DataPlanStreamMode::Materialized { reason } = &node.stream {
                if reason.trim().is_empty() {
                    return Err(DataPlanValidationError::MissingMaterializationReason(node.id));
                }
            }
        }
        for physical in &self.physical {
            if physical.logical.index() >= self.logical.len() {
                return Err(DataPlanValidationError::InvalidPhysicalNode(physical.logical));
            }
            if let DataPlanStreamMode::Materialized { reason } = &physical.stream {
                if reason.trim().is_empty() {
                    return Err(DataPlanValidationError::MissingPhysicalReason(physical.logical));
                }
            }
            if physical.reason.trim().is_empty() {
                return Err(DataPlanValidationError::MissingPhysicalReason(physical.logical));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlanValidationError {
    EmptyPlan,
    MissingScan,
    InvalidOutput(DataPlanNodeId),
    NodeIdMismatch {
        expected: usize,
        actual: DataPlanNodeId,
    },
    InputNotEarlier {
        node: DataPlanNodeId,
        input: DataPlanNodeId,
    },
    Schema(DataPlanSchemaError),
    MissingCallable(DataPlanNodeId),
    CallbackParameter {
        node: DataPlanNodeId,
        expected: String,
        actual: String,
    },
    CallbackResult {
        node: DataPlanNodeId,
        expected: String,
        actual: String,
    },
    MissingMaterializationReason(DataPlanNodeId),
    InvalidPhysicalNode(DataPlanNodeId),
    MissingPhysicalReason(DataPlanNodeId),
}

impl std::fmt::Display for DataPlanValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPlan => formatter.write_str("table plan has no nodes"),
            Self::MissingScan => formatter.write_str("table plan must start with a typed scan"),
            Self::InvalidOutput(id) => write!(formatter, "table plan output node {} is missing", id.index()),
            Self::NodeIdMismatch { expected, actual } => write!(
                formatter,
                "table plan node id {} does not match position {expected}",
                actual.index()
            ),
            Self::InputNotEarlier { node, input } => write!(
                formatter,
                "table plan node {} references non-previous input {}",
                node.index(),
                input.index()
            ),
            Self::Schema(error) => error.fmt(formatter),
            Self::MissingCallable(node) => {
                write!(formatter, "table plan node {} needs a checked callable", node.index())
            }
            Self::CallbackParameter {
                node,
                expected,
                actual,
            } => write!(
                formatter,
                "table plan node {} callback takes {actual}, expected {expected}",
                node.index()
            ),
            Self::CallbackResult {
                node,
                expected,
                actual,
            } => write!(
                formatter,
                "table plan node {} callback returns {actual}, expected {expected}",
                node.index()
            ),
            Self::MissingMaterializationReason(node) => write!(
                formatter,
                "table plan node {} materialization has no reason",
                node.index()
            ),
            Self::InvalidPhysicalNode(node) => write!(
                formatter,
                "table plan physical node {} references a missing logical node",
                node.index()
            ),
            Self::MissingPhysicalReason(node) => write!(
                formatter,
                "table plan physical node {} has no decision reason",
                node.index()
            ),
        }
    }
}

impl std::error::Error for DataPlanValidationError {}
