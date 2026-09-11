//! TIR carrier for checked table-plan facts.
//!
//! This is a total, backend-neutral copy of the sema plan.  It intentionally
//! has no evaluator, closure, row buffer, or target-specific operator.  A
//! future MIR boundary can attach this value to a typed expression and every
//! tier can consume the same decisions.

use crate::AST::{
    DataPlanCallable, DataPlanFact, DataPlanLogicalNode, DataPlanNodeId, DataPlanOperationKind,
    DataPlanPhysicalNode, DataPlanPhysicalOperatorKind, DataPlanSchema, DataPlanSourceKind,
    DataPlanStreamMode, DataPlanValidationError, Type,
};
use crate::Diagnostics::Span;
use super::{TExpr, TExprKind};

/// Version of the TIR table-plan carrier.
pub const TDATA_PLAN_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TDataPlanNode {
    pub id: DataPlanNodeId,
    pub operation: DataPlanOperationKind,
    pub inputs: Vec<DataPlanNodeId>,
    pub source: Option<DataPlanSourceKind>,
    pub schema: DataPlanSchema,
    pub callable: Option<DataPlanCallable>,
    pub stream: DataPlanStreamMode,
    pub span: Span,
}

impl From<&DataPlanLogicalNode> for TDataPlanNode {
    fn from(node: &DataPlanLogicalNode) -> Self {
        Self {
            id: node.id,
            operation: node.operation,
            inputs: node.inputs.clone(),
            source: node.source,
            schema: node.schema.clone(),
            callable: node.callable.clone(),
            stream: node.stream.clone(),
            span: node.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TDataPlanPhysicalNode {
    pub logical: DataPlanNodeId,
    pub operator: DataPlanPhysicalOperatorKind,
    pub stream: DataPlanStreamMode,
    pub reason: String,
}

impl From<&DataPlanPhysicalNode> for TDataPlanPhysicalNode {
    fn from(node: &DataPlanPhysicalNode) -> Self {
        Self {
            logical: node.logical,
            operator: node.operator,
            stream: node.stream.clone(),
            reason: node.reason.clone(),
        }
    }
}

/// Complete table-plan facts carried across the TIR seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TDataPlan {
    pub source: DataPlanSourceKind,
    pub logical: Vec<TDataPlanNode>,
    pub physical: Vec<TDataPlanPhysicalNode>,
    pub output: DataPlanNodeId,
    pub source_span: Span,
}

impl TDataPlan {

    /// Copy checked sema facts into the total TIR carrier.  Validation occurs
    /// before copying so no later tier needs an optional or fallback path.
    pub fn from_fact(fact: &DataPlanFact) -> Result<Self, DataPlanError> {
        fact.validate().map_err(DataPlanError::Validation)?;
        let plan = Self {
            source: fact.source,
            logical: fact.logical.iter().map(TDataPlanNode::from).collect(),
            physical: fact
                .physical
                .iter()
                .map(TDataPlanPhysicalNode::from)
                .collect(),
            output: fact.output,
            source_span: fact.source_span,
        };
        plan.validate()?
            .then_some(plan)
            .ok_or(DataPlanError::InvalidProjection)
    }

    pub fn output_node(&self) -> Option<&TDataPlanNode> {
        self.logical.get(self.output.index())
    }

    pub fn output_schema(&self) -> Option<&DataPlanSchema> {
        self.output_node().map(|node| &node.schema)
    }

    pub fn output_type(&self) -> Option<&Type> {
        self.output_schema().map(|schema| &schema.row_type)
    }

    pub fn validate(&self) -> Result<bool, DataPlanError> {
        if self.logical.is_empty() || self.output.index() >= self.logical.len() {
            return Ok(false);
        }
        if self.logical[0].operation != DataPlanOperationKind::Scan
            || self.logical[0].source != Some(self.source)
        {
            return Ok(false);
        }
        for (index, node) in self.logical.iter().enumerate() {
            if node.id.index() != index || node.schema.validate().is_err() {
                return Ok(false);
            }
            if node.inputs.iter().any(|input| input.index() >= index) {
                return Ok(false);
            }
        }
        if self
            .physical
            .iter()
            .any(|node| node.logical.index() >= self.logical.len() || node.reason.trim().is_empty())
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Preserve every checked fact in the TIR identity.  A graph-only key
    /// aliases plans with different schemas or callbacks and can make an
    /// inspector/cache report the wrong physical plan.
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
    /// Reconstitute the foundation fact for a checked chained operation.
    /// The fields are a lossless projection, so extending a plan never
    /// re-infers a source, schema, or callback.
    pub(crate) fn to_fact(&self) -> DataPlanFact {
        DataPlanFact {
            source: self.source,
            logical: self
                .logical
                .iter()
                .map(|node| DataPlanLogicalNode {
                    id: node.id,
                    operation: node.operation,
                    inputs: node.inputs.clone(),
                    source: node.source,
                    schema: node.schema.clone(),
                    callable: node.callable.clone(),
                    stream: node.stream.clone(),
                    span: node.span,
                })
                .collect(),
            physical: self
                .physical
                .iter()
                .map(|node| DataPlanPhysicalNode {
                    logical: node.logical,
                    operator: node.operator,
                    stream: node.stream.clone(),
                    reason: node.reason.clone(),
                })
                .collect(),
            output: self.output,
            source_span: self.source_span,
        }
    }
}


fn result_inner(ty: &Type) -> &Type {
    match ty {
        Type::Result { ok, .. } => result_inner(ok),
        _ => ty,
    }
}

fn row_type(ty: &Type) -> Option<Type> {
    match result_inner(ty) {
        Type::List(inner) | Type::FixedList { elem: inner, .. } => Some((**inner).clone()),
        Type::Apply { name, args }
            if matches!(name.as_str(), "Query" | "DataStream" | "DataLoader")
                && args.len() == 1 =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

fn chained_fact(args: &[TExpr]) -> Option<DataPlanFact> {
    args.first().and_then(|arg| match &arg.kind {
        TExprKind::CoreCall {
            data_plan: Some(plan),
            ..
        } => Some(plan.to_fact()),
        _ => None,
    })
}

fn callable(
    arg: Option<&TExpr>,
    label: &str,
    span: Span,
) -> Result<DataPlanCallable, String> {
    let Some(TExpr {
        ty:
            Type::Fn {
                params,
                ret: Some(ret),
                ..
            },
        ..
    }) = arg
    else {
        return Err(format!("checked data call `{label}` has no typed callable"));
    };
    let [parameter] = params.as_slice() else {
        return Err(format!(
            "checked data call `{label}` callable must take one row parameter"
        ));
    };
    Ok(crate::Sema::callable_fact(
        label.to_string(),
        parameter.clone(),
        (**ret).clone(),
        span,
    ))
}

/// Build the one checked data-plan fact attached to a recognized Core call.
/// This is a projection of sema's builder, never an execution decision.
pub(crate) fn data_plan_for_core_call(
    record: &'static crate::Syntax::CoreCallRecord,
    args: &[TExpr],
    result_ty: &Type,
    span: Span,
) -> Result<Option<TDataPlan>, String> {
    let Some(kind) = crate::Sema::classify_data_call(record.module, record.member)
        .or_else(|| {
            crate::Sema::classify_data_receiver_call(record.receiver_types, record.member)
        })
    else {
        return Ok(None);
    };
    let fact = chained_fact(args);
    let mut builder = match fact {
        Some(fact) => crate::Sema::DataPlanBuilder::from_fact(fact)
            .map_err(|error| error.to_string())?,
        None => {
            let source = match kind {
                crate::Sema::DataPlanCallKind::Scan(source) => source,
                _ => DataPlanSourceKind::Table,
            };
            let row_type = row_type(
                if matches!(kind, crate::Sema::DataPlanCallKind::Scan(_)) {
                    result_ty
                } else {
                    args.first().map(|arg| &arg.ty).unwrap_or(result_ty)
                },
            )
            .ok_or_else(|| {
                format!(
                    "checked data call `{}.{}` has no typed row schema",
                    record.module, record.member
                )
            })?;
            crate::Sema::DataPlanBuilder::new(
                source,
                crate::Sema::scalar_schema(row_type),
                span,
            )
        }
        .map_err(|error| error.to_string())?,
    };
    match kind {
        crate::Sema::DataPlanCallKind::Scan(_) => {}
        crate::Sema::DataPlanCallKind::Filter | crate::Sema::DataPlanCallKind::SortBy => {
            let label = format!("{}.{}", record.module, record.member);
            let callback = callable(args.get(1), &label, span)?;
            if matches!(kind, crate::Sema::DataPlanCallKind::Filter) {
                builder
                    .filter(callback, span)
                    .map_err(|error| error.to_string())?;
            } else {
                builder
                    .sort_by(callback, span)
                    .map_err(|error| error.to_string())?;
            }
        }
        crate::Sema::DataPlanCallKind::Collect => {
            builder.collect(span).map_err(|error| error.to_string())?;
        }
        crate::Sema::DataPlanCallKind::InspectPlan => {}
    }
    let fact = builder.finish().map_err(|error| error.to_string())?;
    TDataPlan::from_fact(&fact)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataPlanError {
    Validation(DataPlanValidationError),
    InvalidProjection,
}

impl std::fmt::Display for DataPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::InvalidProjection => formatter.write_str("TIR table-plan projection is invalid"),
        }
    }
}

impl std::error::Error for DataPlanError {}

/// Explicitly named handoff for callers that already use the `project_*`
/// vocabulary.  It performs the same checked conversion as `TDataPlan::from_fact`.
pub fn project_data_plan(fact: &DataPlanFact) -> Result<TDataPlan, DataPlanError> {
    TDataPlan::from_fact(fact)
}
