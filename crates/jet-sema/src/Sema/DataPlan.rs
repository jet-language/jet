//! Sema-owned construction of typed query-plan facts.
//!
//! The builder accepts only checked types and callable facts. It creates no
//! rows and executes no predicate: its sole job is to reject an invalid query
//! transition before recording a backend-neutral graph for TIR and tooling.

use crate::AST::{
    DataPlanCallable, DataPlanFact, DataPlanNodeId, DataPlanOperationKind, DataPlanPhysicalNode,
    DataPlanPhysicalOperatorKind, DataPlanSchema, DataPlanSchemaError, DataPlanSourceKind,
    DataPlanStreamError, DataPlanStreamMode, DataPlanValidationError, Type,
};
use crate::Diagnostics::Span;

/// Recognized `core.data` source calls and canonical `Query<T>` transitions.
/// `InspectPlan` is a query over an existing query value and therefore does
/// not add a graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataPlanCallKind {
    Scan(DataPlanSourceKind),
    Filter,
    SortBy,
    Collect,
    InspectPlan,
}

/// Classify a checked core-data call without deciding how any tier executes
/// it. Generated/imported calls retain the qualified `core.data` module name.
pub fn classify_data_call(module: &str, method: &str) -> Option<DataPlanCallKind> {
    if module != "core.data" {
        return None;
    }
    match method {
        "query" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::Table)),
        "csv" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::Csv)),
        "json" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::Json)),
        "jsonl" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::Jsonl)),
        "parquet" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::Parquet)),
        "arrow" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::Arrow)),
        "load" | "load_default" | "file" | "file_member" | "url" | "database" | "value" => {
            Some(DataPlanCallKind::Scan(DataPlanSourceKind::Loader))
        }
        "csv_reader" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::CsvReader)),
        "json_reader" => Some(DataPlanCallKind::Scan(DataPlanSourceKind::JsonReader)),
        _ => None,
    }
}

/// Classify a receiver operation on the canonical deferred `Query<T>`.
/// Receiver rows intentionally have an empty module in the Core registry, so
/// their operation must be resolved from the retained receiver type.
pub fn classify_data_receiver_call(
    receiver_types: &[&str],
    method: &str,
) -> Option<DataPlanCallKind> {
    if !receiver_types.iter().any(|name| *name == "Query") {
        return None;
    }
    match method {
        "filter" => Some(DataPlanCallKind::Filter),
        "sort_by" => Some(DataPlanCallKind::SortBy),
        "collect" => Some(DataPlanCallKind::Collect),
        "plan" => Some(DataPlanCallKind::InspectPlan),
        _ => None,
    }

}

/// A mutable sema-side plan accumulator.  Every successful mutation appends
/// one logical node and one identity physical choice, so a consumer always
/// has a complete deterministic projection even before optimization.
#[derive(Debug, Clone)]
pub struct DataPlanBuilder {
    fact: DataPlanFact,
}

impl DataPlanBuilder {
    pub fn new(
        source: DataPlanSourceKind,
        schema: DataPlanSchema,
        span: Span,
    ) -> Result<Self, DataPlanBuildError> {
        schema
            .validate()
            .map_err(DataPlanBuildError::Schema)?;
        Ok(Self {
            fact: DataPlanFact::scan(source, schema, span),
        })
    }


    /// Re-enter a previously checked fact when a chained expression carries
    /// its plan across another TIR node.  The graph remains sema-owned: this
    /// constructor only re-validates the copied fact before a later append.
    pub fn from_fact(fact: DataPlanFact) -> Result<Self, DataPlanBuildError> {
        fact.validate()
            .map_err(DataPlanBuildError::Validation)?;
        Ok(Self { fact })
    }

    pub fn fact(&self) -> &DataPlanFact {
        &self.fact
    }

    pub fn current_node(&self) -> Option<&crate::AST::DataPlanLogicalNode> {
        self.fact.output_node()
    }

    pub fn current_node_id(&self) -> DataPlanNodeId {
        self.fact.output
    }


    pub fn filter(
        &mut self,
        callable: DataPlanCallable,
        span: Span,
    ) -> Result<DataPlanNodeId, DataPlanBuildError> {
        self.append_unary(DataPlanOperationKind::Filter, callable, span)
    }

    pub fn sort_by(
        &mut self,
        callable: DataPlanCallable,
        span: Span,
    ) -> Result<DataPlanNodeId, DataPlanBuildError> {
        self.append_unary(DataPlanOperationKind::SortBy, callable, span)
    }

    /// An explicit collect is the only standard operation that creates a
    /// materialization boundary.  The reason is retained in both logical and
    /// physical facts for the inspector.
    pub fn collect(&mut self, span: Span) -> Result<DataPlanNodeId, DataPlanBuildError> {
        let stream = DataPlanStreamMode::materialized("explicit collect")
            .map_err(DataPlanBuildError::Stream)?;
        let schema = self
            .current_node()
            .ok_or(DataPlanBuildError::EmptyInput)?
            .schema
            .clone();
        self.append(
            DataPlanOperationKind::Collect,
            vec![self.current_node_id()],
            schema,
            None,
            stream,
            span,
        )
    }

    /// Append a checked unary operation whose output schema may differ from
    /// its input (for example, a future projection or aggregate).
    pub fn append_unary(
        &mut self,
        operation: DataPlanOperationKind,
        callable: DataPlanCallable,
        span: Span,
    ) -> Result<DataPlanNodeId, DataPlanBuildError> {
        let schema = self
            .current_node()
            .ok_or(DataPlanBuildError::EmptyInput)?
            .schema
            .clone();
        let stream = default_stream_for(operation)?;
        self.append(
            operation,
            vec![self.current_node_id()],
            schema,
            Some(callable),
            stream,
            span,
        )
    }

    /// Append a typed node with explicit inputs.  Callers use this for joins
    /// and for future operations after sema has checked the result schema.
    pub fn append(
        &mut self,
        operation: DataPlanOperationKind,
        inputs: Vec<DataPlanNodeId>,
        schema: DataPlanSchema,
        callable: Option<DataPlanCallable>,
        stream: DataPlanStreamMode,
        span: Span,
    ) -> Result<DataPlanNodeId, DataPlanBuildError> {
        schema
            .validate()
            .map_err(DataPlanBuildError::Schema)?;
        if inputs.is_empty() && operation != DataPlanOperationKind::Scan {
            return Err(DataPlanBuildError::MissingInputs(operation));
        }
        let first_input = inputs
            .first()
            .and_then(|id| self.fact.logical.get(id.index()))
            .ok_or(DataPlanBuildError::UnknownInput)?;
        if let Some(callable) = &callable {
            if callable.parameter != first_input.schema.row_type {
                return Err(DataPlanBuildError::CallableParameter {
                    operation,
                    expected: first_input.schema.row_type.name(),
                    actual: callable.parameter.name(),
                });
            }
            let expected = match operation {
                DataPlanOperationKind::Filter => Some(Type::Bool),
                DataPlanOperationKind::SortBy => Some(Type::String),
                _ => None,
            };
            if let Some(expected) = expected {
                if callable.result != expected {
                    return Err(DataPlanBuildError::CallableResult {
                        operation,
                        expected: expected.name(),
                        actual: callable.result.name(),
                    });
                }
            }
        }
        let id = self.fact.append_logical(
            operation,
            inputs,
            schema,
            callable,
            stream.clone(),
            span,
        );
        self.fact.append_physical(
            id,
            operation.default_physical_operator(),
            stream,
            format!("identity {}", operation.as_str()),
        );
        Ok(id)
    }

    /// Replace the default identity choice with an optimizer fact.  This is
    /// still only metadata; no tier-specific operator or execution callback is
    /// accepted here.
    pub fn add_physical_choice(
        &mut self,
        logical: DataPlanNodeId,
        operator: DataPlanPhysicalOperatorKind,
        stream: DataPlanStreamMode,
        reason: impl Into<String>,
    ) -> Result<(), DataPlanBuildError> {
        if logical.index() >= self.fact.logical.len() {
            return Err(DataPlanBuildError::UnknownInput);
        }
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(DataPlanBuildError::MissingPhysicalReason);
        }
        self.fact.append_physical(logical, operator, stream, reason);
        Ok(())
    }

    pub fn finish(self) -> Result<DataPlanFact, DataPlanBuildError> {
        self.fact
            .validate()
            .map_err(DataPlanBuildError::Validation)?;
        Ok(self.fact)
    }
}

fn default_stream_for(
    operation: DataPlanOperationKind,
) -> Result<DataPlanStreamMode, DataPlanBuildError> {
    match operation {
        DataPlanOperationKind::SortBy => DataPlanStreamMode::materialized("sort_by requires ordering")
            .map_err(DataPlanBuildError::Stream),
        _ => DataPlanStreamMode::bounded(None).map_err(DataPlanBuildError::Stream),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlanBuildError {
    Schema(DataPlanSchemaError),
    Stream(DataPlanStreamError),
    Validation(DataPlanValidationError),
    EmptyInput,
    MissingInputs(DataPlanOperationKind),
    UnknownInput,
    CallableParameter {
        operation: DataPlanOperationKind,
        expected: String,
        actual: String,
    },
    CallableResult {
        operation: DataPlanOperationKind,
        expected: String,
        actual: String,
    },
    MissingPhysicalReason,
}

impl std::fmt::Display for DataPlanBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(error) => error.fmt(formatter),
            Self::Stream(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
            Self::EmptyInput => formatter.write_str("table plan has no input frame"),
            Self::MissingInputs(operation) => {
                write!(formatter, "table plan `{}` needs an input", operation.as_str())
            }
            Self::UnknownInput => formatter.write_str("table plan input node is unknown"),
            Self::CallableParameter {
                operation,
                expected,
                actual,
            } => write!(
                formatter,
                "table plan `{}` callback takes {actual}, expected {expected}",
                operation.as_str()
            ),
            Self::CallableResult {
                operation,
                expected,
                actual,
            } => write!(
                formatter,
                "table plan `{}` callback returns {actual}, expected {expected}",
                operation.as_str()
            ),
            Self::MissingPhysicalReason => {
                formatter.write_str("table plan physical choice needs a reason")
            }
        }
    }
}

/// Construct a source schema from a checked row type when the source has no
/// named fields (for example a scalar-list input).
pub fn scalar_schema(row_type: Type) -> DataPlanSchema {
    let nullable = matches!(&row_type, Type::Option(_));
    DataPlanSchema::new(
        row_type.clone(),
        vec![crate::AST::DataPlanColumn::new("value", row_type).with_nullable(nullable)],
    )
}

/// Small helper used by sema call sites that need to preserve the callable
/// label while filling the type/span fields from the checked expression.
pub fn callable_fact(
    label: impl Into<String>,
    parameter: Type,
    result: Type,
    span: Span,
) -> DataPlanCallable {
    DataPlanCallable::new(label, parameter, result, span)
}

/// Convert an already-built plan into the physical fact shape without
/// permitting a caller to smuggle an execution-only node into the graph.
pub fn physical_facts(fact: &DataPlanFact) -> &[DataPlanPhysicalNode] {
    &fact.physical
}
