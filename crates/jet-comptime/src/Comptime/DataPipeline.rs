//! D-QUERY-RETAIN1=A: one typed `Query<T>` surface over ordinary lists,
//! checked readers, and checked SQL. Query plans and grouped reductions stay
//! in this per-interpreter state; materialized results are ordinary lists.
//! `DataStream<T>` remains one-shot and fallible, while typed loaders preserve
//! ownership, authority, and snapshot state.
//!
//! `csv<T>`/`json<T>` use the call-site type argument at runtime; query
//! callbacks use the same closure path as `list.map`/`.filter`/`.sort_by`.
//! The shared query kernel is also used by the AOT and JIT tiers.
//! parity: guard tests/dataflow_stream.rs::rolling_mean_nonfinite_matches_aot_and_default_dev
//!
//! Legacy carrier wrappers are intentionally absent.
//! 
//! 

use std::collections::{BTreeMap, HashMap};

use super::CollectionEval::collection_semantics::{JetTableColumn, JetTableTypeId};
mod sql_query {
    include!("../../../jet-codegen/src/Prelude/Core/SqlQuery.rs");
}
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, CtKey, Type};

use super::Diagnostics::unsupported;
use super::Interpreter::Interp;
use super::Methods::as_string;
use super::Methods::data_plot_rt;
use crate::AST::{CtReport, CtValue};

fn ct_struct(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect(),
    }
}
fn struct_field<'a>(v: &'a CtValue, type_name: &str, field: &str) -> Option<&'a CtValue> {
    match v {
        CtValue::Struct {
            type_name: t,
            fields,
        } if t == type_name => fields.iter().find(|(n, _)| n == field).map(|(_, v)| v),
        _ => None,
    }
}
fn expect_list<'a>(v: &'a CtValue, what: &str, span: Span) -> Result<&'a Vec<CtValue>, Diagnostic> {
    match v {
        CtValue::List(xs) => Ok(xs),
        _ => Err(unsupported(
            &format!("`data.{}` needs a list-backed value", what),
            span,
        )),
    }
}

pub(super) fn query_row_field<'a>(row: &'a CtValue, field: &str) -> Option<&'a CtValue> {
    match row {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, value)| value),
        CtValue::Enum { type_name, .. } if matches!(
            type_name.as_str(),
            "DataTree" | "TOML" | "YAML" | "CSV"
        ) => {
            match super::JSONInterp::json_payload(row, "Object") {
                Some(CtValue::Map(entries)) => entries.iter().find_map(|(key, value)| match key {
                    CtKey::Str(name) if name == field => Some(value),
                    _ => None,
                }),
                Some(CtValue::Struct { fields, .. }) => fields
                    .iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, value)| value),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn query_cell_text(value: &CtValue) -> String {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(type_name.as_str(), "DataTree" | "TOML" | "YAML" | "CSV") => {
            match variant.as_str() {
                "Null" => String::new(),
                _ => args
                    .first()
                    .map(|(_, payload)| query_cell_text(payload))
                    .unwrap_or_default(),
            }
        }
        CtValue::Str(text) => text.clone(),
        CtValue::Int(value) => value.to_string(),
        CtValue::BigInt(value) => value.to_string_rep(),
        CtValue::Float(value) => value.to_json(),
        CtValue::Bool(value) => value.to_string(),
        _ => super::JSONInterp::render_json_pretty(value, false, 0),
    }
}
fn query_cell_sql_value(value: &CtValue) -> Option<sql_query::JetSqlValue> {
    match value {
        CtValue::Unit => Some(sql_query::JetSqlValue::Null),
        CtValue::Bool(value) => Some(sql_query::JetSqlValue::Bool(*value)),
        CtValue::Int(value) => Some(sql_query::JetSqlValue::Int(*value)),
        CtValue::BigInt(value) => value
            .to_string_rep()
            .parse::<i64>()
            .map(sql_query::JetSqlValue::Int)
            .ok()
            .or_else(|| Some(sql_query::JetSqlValue::Text(value.to_string_rep()))),
        CtValue::Float(value) => Some(sql_query::JetSqlValue::Float(value.as_f64())),
        CtValue::Str(value) => Some(sql_query::JetSqlValue::Text(value.clone())),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if matches!(type_name.as_str(), "DataTree" | "TOML" | "YAML" | "CSV") => {
            match variant.as_str() {
                "Null" => Some(sql_query::JetSqlValue::Null),
                "Bool" | "Int" | "Number" | "Float" | "TypedText" | "Text" => args
                    .first()
                    .and_then(|(_, value)| query_cell_sql_value(value)),
                _ => Some(sql_query::JetSqlValue::Text(query_cell_text(value))),
            }
        }
        _ => Some(sql_query::JetSqlValue::Text(query_cell_text(value))),
    }
}

fn query_sql_value_ct(value: sql_query::JetSqlValue) -> CtValue {
    match value {
        sql_query::JetSqlValue::Null => CtValue::Unit,
        sql_query::JetSqlValue::Bool(value) => CtValue::Bool(value),
        sql_query::JetSqlValue::Int(value) => CtValue::Int(value),
        sql_query::JetSqlValue::Float(value) => CtValue::Float(CtFloat::f64(value)),
        sql_query::JetSqlValue::Text(value) => CtValue::Str(value),
    }
}

fn query_runtime_fields(rows: &[CtValue]) -> Vec<String> {
    rows.iter()
        .find_map(|row| match row {
            CtValue::Struct { fields, .. } => Some(fields.iter().map(|(name, _)| name.clone()).collect()),
            CtValue::Enum { .. } => super::JSONInterp::json_payload(row, "Object").and_then(|value| {
                match value {
                    CtValue::Struct { fields, .. } => {
                        Some(fields.iter().map(|(name, _)| name.clone()).collect())
                    }
                    CtValue::Map(entries) => Some(
                        entries
                            .iter()
                            .filter_map(|(key, _)| match key {
                                CtKey::Str(name) => Some(name.clone()),
                                _ => None,
                            })
                            .collect(),
                    ),
                    _ => None,
                }
            }),
            _ => None,
        })
        .unwrap_or_default()
}

fn data_schema_elem_ty(arg_ty: &Type) -> Option<&Type> {
    match arg_ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. } => Some(inner.as_ref()),
        _ => None,
    }
}
fn data_join_right_type(call_ret: Option<&Type>) -> Option<Type> {
    let joined = match call_ret? {
        Type::Result { ok, .. } => ok.as_ref(),
        ty => ty,
    };
    let Type::List(join) = joined else {
        return None;
    };
    let Type::Apply { name, args } = join.as_ref() else {
        return None;
    };
    if name != "DataJoin" || args.len() != 2 {
        return None;
    }
    match &args[1] {
        Type::Option(inner) => Some((**inner).clone()),
        right => Some(right.clone()),
    }
}
fn query_join_right_type(call_ret: Option<&Type>) -> Option<Type> {
    let Type::Apply { name, args } = call_ret? else {
        return None;
    };
    if name != "Query" || args.len() != 1 {
        return None;
    }
    let Type::Apply {
        name: join_name,
        args: join_args,
    } = &args[0]
    else {
        return None;
    };
    if join_name != "DataJoin" || join_args.len() != 2 {
        return None;
    }
    match &join_args[1] {
        Type::Option(inner) => Some((**inner).clone()),
        right => Some(right.clone()),
    }
}
fn query_sql_rows(
    rows: Vec<CtValue>,
    sql: &str,
    declared_fields: Option<Vec<String>>,
) -> Result<Vec<CtValue>, CtValue> {
    let query = sql_query::parse_sql_query(sql)
        .map_err(|error| super::TypedDecode::decode_error(format!("`data.query()`: {error}")))?;
    sql_query::validate_sql_query_schema(
        &query,
        |field| match declared_fields.as_ref() {
            Some(fields) => fields.iter().any(|known| known == field),
            None if rows.is_empty() => true,
            None => rows
                .iter()
                .any(|row| query_row_field(row, field).is_some()),
        },
        "`data.query()`",
    )
    .map_err(super::TypedDecode::decode_error)?;
    if query.needs_projection_execution() {
        let fields = declared_fields.unwrap_or_else(|| query_runtime_fields(&rows));
        let result = sql_query::execute_sql_rows(
            &fields,
            rows.len(),
            &query,
            |index, field| query_row_field(&rows[index], field).and_then(query_cell_sql_value),
        )
        .map_err(|error| {
            super::TypedDecode::decode_error(format!("`data.query()`: {error}"))
        })?;
        let type_name = rows
            .iter()
            .find_map(|row| match row {
                CtValue::Struct { type_name, .. } => Some(type_name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "SqlRow".to_string());
        Ok(result
            .into_iter()
            .map(|row| CtValue::Struct {
                type_name: type_name.clone(),
                fields: row
                    .fields
                    .into_iter()
                    .map(|(name, value)| (name, query_sql_value_ct(value)))
                    .collect(),
            })
            .collect())
    } else {
        let selected = sql_query::select_sql_indices(rows.len(), &query, |index, field| {
            query_row_field(&rows[index], field).map(query_cell_text)
        });
        Ok(selected
            .into_iter()
            .map(|index| rows[index].clone())
            .collect())
    }
}
fn query_declared_fields(
    arg0_ty: Option<&Type>,
    call_ret: Option<&Type>,
    structs: &HashMap<String, &crate::AST::StructDef>,
) -> Option<Vec<String>> {
    let elem_ty = arg0_ty.and_then(data_schema_elem_ty).or_else(|| {
        let Type::Result { ok, .. } = call_ret? else {
            return None;
        };
        data_schema_elem_ty(ok)
    })?;
    let struct_name = elem_ty.base_name()?;
    let definition = structs.get(struct_name)?;
    Some(
        definition
            .fields
            .iter()
            .map(|field| field.name.clone())
            .collect(),
    )
}



fn data_column(name: &str, type_name: &str) -> CtValue {
    let id = JetTableColumn::new(
        name.to_string(),
        JetTableTypeId::new(type_name.to_string()),
    )
    .id
    .as_str()
    .to_string();
    ct_struct(
        "DataColumn",
        vec![
            ("id", CtValue::Str(id)),
            ("name", CtValue::Str(name.to_string())),
            ("type_name", CtValue::Str(type_name.to_string())),
            ("nullable", CtValue::Bool(type_name.ends_with('?'))),
        ],
    )
}
fn data_loader_enum(type_name: &str, variant: &str) -> CtValue {
    CtValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn data_loader_error_value(kind: &str, operation: &str, reason: impl Into<String>) -> CtValue {
    ct_struct(
        "DataError",
        vec![
            ("kind", data_loader_enum("DataErrorKind", kind)),
            ("operation", CtValue::Str(operation.to_string())),
            ("row", CtValue::absent(Type::Int)),
            ("column", CtValue::absent(Type::Int)),
            ("index", CtValue::absent(Type::Int)),
            (
                "reason",
                CtValue::Str(jet_foundation::PreludeDataFlow::status_text(&reason.into())),
            ),
            (
                "cause",
                CtValue::absent(Type::Named("EncodingError".to_string())),
            ),
        ],
    )
}

fn data_loader_failure(kind: &str, operation: &str, reason: impl Into<String>) -> CtValue {
    CtValue::failed(Box::new(data_loader_error_value(kind, operation, reason)))
}

fn data_loader_kernel_failure(
    error: jet_foundation::PreludeDataFlow::KernelError,
) -> CtValue {
    let kind = match error.kind {
        jet_foundation::PreludeDataFlow::ErrorKind::InvalidArgument => "InvalidArgument",
        jet_foundation::PreludeDataFlow::ErrorKind::Limit => "Limit",
        jet_foundation::PreludeDataFlow::ErrorKind::State => "State",
        jet_foundation::PreludeDataFlow::ErrorKind::Bridge => "Bridge",
    };
    data_loader_failure(kind, &error.operation, error.reason)
}

fn data_loader_bridge(operation: &str) -> CtValue {
    data_loader_failure(
        "Bridge",
        operation,
        "provider payload is missing; bind a canonical provider response",
    )
}

fn data_loader_field<'a>(loader: &'a CtValue, field: &str) -> Option<&'a CtValue> {
    struct_field(loader, "DataLoader", field)
}

fn data_snapshot_reusable_value(left: &CtValue, right: &CtValue) -> Option<bool> {
    if let (Some(left), Some(right)) = (
        matches!(left, CtValue::Struct { type_name, .. } if type_name == "DataSnapshotIdentity")
            .then_some(left),
        matches!(right, CtValue::Struct { type_name, .. } if type_name == "DataSnapshotIdentity")
            .then_some(right),
    ) {
        return Some(
            ["id", "source", "content", "schema", "format"]
                .iter()
                .all(|field| {
                    struct_field(left, "DataSnapshotIdentity", field)
                        == struct_field(right, "DataSnapshotIdentity", field)
                }),
        );
    }
    let (CtValue::Struct { type_name: left_type, .. }, CtValue::Struct {
        type_name: right_type,
        ..
    }) = (left, right) else {
        return None;
    };
    if left_type != "DataLoader" || right_type != "DataSnapshot" {
        return None;
    }
    let provenance = struct_field(right, "DataSnapshot", "provenance")?;
    Some(
        struct_field(provenance, "DataProvenance", "source")
            == struct_field(left, "DataLoader", "source")
            && struct_field(provenance, "DataProvenance", "authority")
                == struct_field(left, "DataLoader", "authority")
            && struct_field(provenance, "DataProvenance", "format")
                == struct_field(left, "DataLoader", "format"),
    )
}

type DataKernel = jet_foundation::PreludeDataFlow::LoaderState;
type DataKernelLimits = jet_foundation::PreludeDataFlow::Limits;
type DataKernelStatus = jet_foundation::PreludeDataFlow::Status;

#[derive(Clone)]
struct ComptimeLoader {
    state: DataKernel,
    seed: Option<CtValue>,
    decode_ty: Option<Type>,
}

struct ComptimeStream {
    rows: Vec<CtValue>,
    cursor: usize,
    cancelled: bool,
    row_ty: Option<Type>,
}

#[derive(Clone)]
enum ComptimeQueryOperation {
    Filter(CtValue),
    SortBy(CtValue),
    Map(CtValue),
    Extreme {
        key: CtValue,
        maximum: bool,
    },
    Join {
        right: CtValue,
        left_key: CtValue,
        right_key: CtValue,
        left: bool,
        right_type: Type,
    },
}

#[derive(Clone)]
struct ComptimeQuery {
    rows: Vec<CtValue>,
    tracked: Option<usize>,
    operations: Vec<ComptimeQueryOperation>,
    plan: Vec<String>,
}

#[derive(Clone)]
struct ComptimeTrackedChange {
    revision: u64,
}

#[derive(Clone)]
struct ComptimeTracked {
    rows: Vec<(u64, CtValue)>,
    key: CtValue,
    next_order: u64,
    revision: u64,
    changes: Vec<ComptimeTrackedChange>,
}

#[derive(Clone)]
struct ComptimeWatch {
    query: ComptimeQuery,
    lifecycle: jet_foundation::LiveLifecycle::JetLiveLifecycle,
    source_revision: u64,
    retained_rows: usize,
    entries: Vec<CtValue>,
    mode: String,
    recomputations: u64,
}

/// All mutable data-pipeline handles belong to one interpreter invocation.
/// CtValue carriers contain only slot identities; no process-global registry
/// can make a stale carrier accidentally resolve in another invocation.
pub struct DataPipelineState {
    loaders: Vec<Option<ComptimeLoader>>,
    streams: Vec<Option<ComptimeStream>>,
    queries: Vec<Option<ComptimeQuery>>,
    tracked: Vec<Option<ComptimeTracked>>,
    watches: Vec<Option<ComptimeWatch>>,
    plots: Vec<Option<data_plot_rt::JetDataPlot<CtValue>>>,
}

impl Default for DataPipelineState {
    fn default() -> Self {
        Self {
            loaders: Vec::new(),
            streams: Vec::new(),
            queries: Vec::new(),
            tracked: Vec::new(),
            watches: Vec::new(),
            plots: Vec::new(),
        }
    }
}
fn store_query(
    state: &mut DataPipelineState,
    rows: Vec<CtValue>,
    plan: Vec<String>,
) -> CtValue {
    store_query_source(state, rows, None, plan)
}

fn store_query_source(
    state: &mut DataPipelineState,
    rows: Vec<CtValue>,
    tracked: Option<usize>,
    plan: Vec<String>,
) -> CtValue {
    state.queries.push(Some(ComptimeQuery {
        rows,
        tracked,
        operations: Vec::new(),
        plan,
    }));
    ct_struct(
        "Query",
        vec![(
            "slot",
            CtValue::Int(i64::try_from(state.queries.len()).unwrap_or(i64::MAX)),
        )],
    )
}

fn store_tracked(
    state: &mut DataPipelineState,
    rows: Vec<(u64, CtValue)>,
    key: CtValue,
    revision: u64,
) -> CtValue {
    let next_order = rows
        .last()
        .map(|(order, _)| order.saturating_add(1))
        .unwrap_or(0);
    state.tracked.push(Some(ComptimeTracked {
        rows,
        key,
        next_order,
        revision,
        changes: Vec::new(),
    }));
    ct_struct(
        "DataTracked",
        vec![(
            "slot",
            CtValue::Int(i64::try_from(state.tracked.len()).unwrap_or(i64::MAX)),
        )],
    )
}

fn tracked_id(
    state: &DataPipelineState,
    tracked: &CtValue,
    span: Span,
) -> Result<usize, Diagnostic> {
    let Some(CtValue::Int(id)) = struct_field(tracked, "DataTracked", "slot") else {
        return Err(unsupported("`DataTracked` carrier has no state slot", span));
    };
    if *id <= 0 {
        return Err(unsupported("`DataTracked` state slot is invalid", span));
    }
    let id = usize::try_from(*id)
        .map_err(|_| unsupported("`DataTracked` state slot is invalid", span))?;
    if state.tracked.get(id - 1).and_then(Option::as_ref).is_none() {
        return Err(unsupported("`DataTracked` state slot is stale", span));
    }
    Ok(id)
}

fn store_watch(
    state: &mut DataPipelineState,
    query: ComptimeQuery,
    entries: Vec<CtValue>,
    source_revision: u64,
    retained_rows: usize,
    mode: String,
) -> CtValue {
    state.watches.push(Some(ComptimeWatch {
        query,
        lifecycle: jet_foundation::LiveLifecycle::JetLiveLifecycle::active_now(),
        source_revision,
        retained_rows,
        entries,
        mode,
        recomputations: 0,
    }));
    ct_struct(
        "DataWatch",
        vec![(
            "slot",
            CtValue::Int(i64::try_from(state.watches.len()).unwrap_or(i64::MAX)),
        )],
    )
}

fn watch_id(
    state: &DataPipelineState,
    watch: &CtValue,
    span: Span,
) -> Result<usize, Diagnostic> {
    let Some(CtValue::Int(id)) = struct_field(watch, "DataWatch", "slot") else {
        return Err(unsupported("`DataWatch` carrier has no state slot", span));
    };
    if *id <= 0 {
        return Err(unsupported("`DataWatch` state slot is invalid", span));
    }
    let id = usize::try_from(*id)
        .map_err(|_| unsupported("`DataWatch` state slot is invalid", span))?;
    if state.watches.get(id - 1).and_then(Option::as_ref).is_none() {
        return Err(unsupported("`DataWatch` state slot is stale", span));
    }
    Ok(id)
}

fn query_id(
    state: &DataPipelineState,
    query: &CtValue,
    span: Span,
) -> Result<usize, Diagnostic> {
    let Some(CtValue::Int(id)) = struct_field(query, "Query", "slot") else {
        return Err(unsupported("`Query` carrier has no state slot", span));
    };
    if *id <= 0 {
        return Err(unsupported("`Query` state slot is invalid", span));
    }
    let id = usize::try_from(*id)
        .map_err(|_| unsupported("`Query` state slot is invalid", span))?;
    if state.queries.get(id - 1).and_then(Option::as_ref).is_none() {
        return Err(unsupported("`Query` state slot is stale", span));
    }
    Ok(id)
}

fn clone_query(
    state: &DataPipelineState,
    query: &CtValue,
    span: Span,
) -> Result<ComptimeQuery, Diagnostic> {
    let id = query_id(state, query, span)?;
    state.queries[id - 1]
        .as_ref()
        .cloned()
        .ok_or_else(|| unsupported("`Query` state slot is stale", span))
}





fn data_loader_format(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        "csv" => Some("csv"),
        "json" => Some("json"),
        "jsonl" | "ndjson" => Some("jsonl"),
        "parquet" => Some("parquet"),
        "arrow" | "arrow-ipc" | "feather" => Some("arrow"),
        _ => None,
    }
}

fn data_loader_format_for_locator(locator: &str) -> Result<String, CtValue> {
    let path = locator
        .split(|character| character == '?' || character == '#')
        .next()
        .unwrap_or(locator);
    let path = path.rsplit_once('/').map_or(path, |(_, path)| path);
    let Some(extension) = path.rsplit_once('.').map(|(_, extension)| extension) else {
        return Err(data_loader_failure(
            "Bridge",
            "data.load",
            "source has no core format; import a provider package",
        ));
    };
    data_loader_format(extension)
        .map(str::to_string)
        .ok_or_else(|| {
            data_loader_failure(
                "Bridge",
                "data.load",
                format!("format `{extension}` is provider-owned; import its package"),
            )
        })
}

fn data_kernel_limits_safe() -> DataKernelLimits {
    DataKernelLimits {
        buffer_bytes: 65_536,
        max_depth: 256,
        max_item_bytes: 16_777_216,
        max_total_bytes: None,
        max_expansion_depth: 32,
        max_expansion_bytes: 8_388_608,
        max_groups: 100_000,
        max_sort_rows: 1_000_000,
        max_join_rows: 1_000_000,
        max_output_rows: 1_000_000,
    }
}

fn data_int_field(value: Option<&CtValue>, fallback: i64) -> i64 {
    match value {
        Some(CtValue::Int(value)) => *value,
        _ => fallback,
    }
}

fn data_optional_int(value: Option<&CtValue>) -> Option<i64> {
    match value {
        Some(CtValue::Int(value)) => Some(*value),
        Some(CtValue::Present(value)) => match value.as_ref() {
            CtValue::Int(value) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}
fn build_tracked(
    state: &mut DataPipelineState,
    rows: Vec<CtValue>,
    key: CtValue,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let max_rows = usize::try_from(data_kernel_limits_safe().max_output_rows)
        .unwrap_or(usize::MAX);
    if rows.len() > max_rows {
        return Err(unsupported(
            "`data.track()` rows exceed max_output_rows",
            span,
        ));
    }
    let mut keyed = Vec::with_capacity(rows.len());
    for (order, row) in rows.into_iter().enumerate() {
        let key_value = call(&key, vec![row.clone()], span)?;
        if keyed.iter().any(|(_, existing): &(u64, CtValue)| *existing == key_value) {
            return Err(unsupported(
                "`data.track()` key callback returned a duplicate key",
                span,
            ));
        }
        keyed.push((u64::try_from(order).unwrap_or(u64::MAX), row));
    }
    Ok(store_tracked(state, keyed, key, 0))
}

fn invalidate_tracked_watches(state: &mut DataPipelineState, tracked_id: usize, cause: &str) {
    let affected: Vec<usize> = state
        .watches
        .iter()
        .enumerate()
        .filter_map(|(index, watch)| {
            let watch = watch.as_ref()?;
            query_depends_on_tracked(state, &watch.query, tracked_id).then_some(index)
        })
        .collect();
    for index in affected {
        if let Some(watch) = state.watches[index].as_mut() {
            let _ = watch.lifecycle.invalidate(cause);
        }
    }
}

fn query_depends_on_tracked(
    state: &DataPipelineState,
    query: &ComptimeQuery,
    tracked_id: usize,
) -> bool {
    if query.tracked == Some(tracked_id) {
        return true;
    }
    query.operations.iter().any(|operation| {
        let ComptimeQueryOperation::Join { right, .. } = operation else {
            return false;
        };
        let Some(CtValue::Int(id)) = struct_field(right, "Query", "slot") else {
            return false;
        };
        let Ok(id) = usize::try_from(*id) else {
            return false;
        };
        state
            .queries
            .get(id.saturating_sub(1))
            .and_then(Option::as_ref)
            .is_some_and(|right_query| query_depends_on_tracked(state, right_query, tracked_id))
    })
}
fn query_is_maintained(state: &DataPipelineState, query: &ComptimeQuery) -> bool {
    query.tracked.is_some()
        || query.operations.iter().any(|operation| {
            let ComptimeQueryOperation::Join { right, .. } = operation else {
                return false;
            };
            let Some(CtValue::Int(id)) = struct_field(right, "Query", "slot") else {
                return false;
            };
            let Ok(id) = usize::try_from(*id) else {
                return false;
            };
            state
                .queries
                .get(id.saturating_sub(1))
                .and_then(Option::as_ref)
                .is_some_and(|right_query| query_is_maintained(state, right_query))
        })
}

fn bump_tracked_revision(
    tracked: &mut ComptimeTracked,
    span: Span,
) -> Result<(), Diagnostic> {
    tracked.revision = tracked
        .revision
        .checked_add(1)
        .ok_or_else(|| unsupported("`DataTracked` revision overflowed", span))?;
    tracked.changes.push(ComptimeTrackedChange {
        revision: tracked.revision,
    });
    let max_history = usize::try_from(data_kernel_limits_safe().max_output_rows)
        .unwrap_or(usize::MAX);
    if tracked.changes.len() > max_history {
        let overflow = tracked.changes.len() - max_history;
        tracked.changes.drain(..overflow);
    }
    Ok(())
}

fn tracked_insert(
    state: &mut DataPipelineState,
    tracked: &CtValue,
    row: CtValue,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<(), Diagnostic> {
    let id = tracked_id(state, tracked, span)?;
    let key_callback = state.tracked[id - 1]
        .as_ref()
        .map(|slot| slot.key.clone())
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let key_value = call(&key_callback, vec![row.clone()], span)?;
    let existing_rows = state.tracked[id - 1]
        .as_ref()
        .map(|slot| slot.rows.iter().map(|(_, row)| row.clone()).collect::<Vec<_>>())
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    for existing in existing_rows.iter().cloned() {
        if call(&key_callback, vec![existing], span)? == key_value {
            return Err(unsupported("`DataTracked.insert()` found a duplicate key", span));
        }
    }
    let max_rows = usize::try_from(data_kernel_limits_safe().max_output_rows)
        .unwrap_or(usize::MAX);
    if existing_rows.len() >= max_rows {
        return Err(unsupported(
            "`DataTracked.insert()` rows exceed max_output_rows",
            span,
        ));
    }
    let slot = state.tracked[id - 1]
        .as_mut()
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let order = slot.next_order;
    slot.next_order = slot
        .next_order
        .checked_add(1)
        .ok_or_else(|| unsupported("`DataTracked` source order overflowed", span))?;
    slot.rows.push((order, row.clone()));
    bump_tracked_revision(slot, span)?;
    invalidate_tracked_watches(state, id - 1, "insert");
    Ok(())
}

fn tracked_replace(
    state: &mut DataPipelineState,
    tracked: &CtValue,
    key_value: CtValue,
    row: CtValue,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<(), Diagnostic> {
    let id = tracked_id(state, tracked, span)?;
    let key_callback = state.tracked[id - 1]
        .as_ref()
        .map(|slot| slot.key.clone())
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let existing_rows = state.tracked[id - 1]
        .as_ref()
        .map(|slot| slot.rows.clone())
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let mut found = None;
    for (index, (order, existing)) in existing_rows.iter().enumerate() {
        if call(&key_callback, vec![existing.clone()], span)? == key_value {
            found = Some((index, *order));
            break;
        }
    }
    let Some((index, order)) = found else {
        return Err(unsupported("`DataTracked.replace()` key is missing", span));
    };
    let row_key = call(&key_callback, vec![row.clone()], span)?;
    if row_key != key_value {
        return Err(unsupported(
            "`DataTracked.replace()` row key does not match the requested key",
            span,
        ));
    }
    let slot = state.tracked[id - 1]
        .as_mut()
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    slot.rows[index] = (order, row.clone());
    bump_tracked_revision(slot, span)?;
    invalidate_tracked_watches(state, id - 1, "replace");
    Ok(())
}

fn tracked_remove(
    state: &mut DataPipelineState,
    tracked: &CtValue,
    key_value: CtValue,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<(), Diagnostic> {
    let id = tracked_id(state, tracked, span)?;
    let key_callback = state.tracked[id - 1]
        .as_ref()
        .map(|slot| slot.key.clone())
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let existing_rows = state.tracked[id - 1]
        .as_ref()
        .map(|slot| slot.rows.clone())
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let mut found = None;
    for (index, (order, existing)) in existing_rows.iter().enumerate() {
        if call(&key_callback, vec![existing.clone()], span)? == key_value {
            found = Some((index, *order));
            break;
        }
    }
    let Some((index, order)) = found else {
        return Err(unsupported("`DataTracked.remove()` key is missing", span));
    };
    let slot = state.tracked[id - 1]
        .as_mut()
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?;
    let removed = slot.rows.remove(index);
    debug_assert_eq!(removed.0, order);
    bump_tracked_revision(slot, span)?;
    invalidate_tracked_watches(state, id - 1, "remove");
    Ok(())
}

fn tracked_revision(
    state: &DataPipelineState,
    tracked: usize,
    span: Span,
) -> Result<u64, Diagnostic> {
    state
        .tracked
        .get(tracked)
        .and_then(Option::as_ref)
        .map(|slot| {
            slot
                .changes
                .last()
                .map(|change| change.revision)
                .unwrap_or(slot.revision)
                .max(slot.revision)
        })
        .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))
}

fn watch_refresh(
    state: &mut DataPipelineState,
    watch: &CtValue,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let id = watch_id(state, watch, span)?;
    let (query, source_revision, entries, generation, active, cancelled) = {
        let slot = state.watches[id - 1]
            .as_ref()
            .ok_or_else(|| unsupported("`DataWatch` state slot is stale", span))?;
        let source_revision = query_source_revision(state, &slot.query, span)?;
        (
            slot.query.clone(),
            source_revision,
            slot.entries.clone(),
            slot.lifecycle.generation,
            slot.lifecycle.active,
            slot.lifecycle.cancelled,
        )
    };
    if !active || cancelled {
        return Err(unsupported("`DataWatch` is inactive", span));
    }
    if source_revision == state.watches[id - 1].as_ref().unwrap().source_revision {
        return Ok(entries);
    }
    if !state.watches[id - 1]
        .as_mut()
        .unwrap()
        .lifecycle
        .begin_refresh(generation)
    {
        return Err(unsupported("`DataWatch` refresh was superseded", span));
    }
    let refreshed = match collect_query_rows(state, &query, call, span) {
        Ok(rows) if rows.len() <= data_kernel_limits_safe().max_output_rows as usize => rows,
        Ok(_) => {
            let error = unsupported("`DataWatch` output exceeds max_output_rows", span);
            let _ = state.watches[id - 1]
                .as_mut()
                .unwrap()
                .lifecycle
                .fail(generation, error.what.clone());
            return Err(error);
        }
        Err(error) => {
            let _ = state.watches[id - 1]
                .as_mut()
                .unwrap()
                .lifecycle
                .fail(generation, error.what.clone());
            return Err(error);
        }
    };
    let fresh_at_ms =
        jet_foundation::LiveLifecycle::JetLiveLifecycle::active_now().fresh_at_ms;
    let retained_rows = query_retained_rows(state, &query, span)?;
    let slot = state.watches[id - 1]
        .as_mut()
        .ok_or_else(|| unsupported("`DataWatch` state slot is stale", span))?;
    if !slot.lifecycle.publish(generation, fresh_at_ms) {
        return Err(unsupported("`DataWatch` refresh was superseded", span));
    }
    slot.entries = refreshed.clone();
    slot.source_revision = source_revision;
    slot.retained_rows = retained_rows;
    slot.recomputations = slot.recomputations.saturating_add(1);
    Ok(refreshed)
}

fn query_source_revision(
    state: &DataPipelineState,
    query: &ComptimeQuery,
    span: Span,
) -> Result<u64, Diagnostic> {
    let mut revision = query
        .tracked
        .map(|tracked| tracked_revision(state, tracked, span))
        .transpose()?
        .unwrap_or(0);
    for operation in &query.operations {
        let ComptimeQueryOperation::Join { right, .. } = operation else {
            continue;
        };
        let right_query = clone_query(state, right, span)?;
        let right_revision = query_source_revision(state, &right_query, span)?;
        revision = revision
            .wrapping_mul(1_000_003)
            .wrapping_add(right_revision.saturating_add(1));
    }
    Ok(revision)
}
fn query_retained_rows(
    state: &DataPipelineState,
    query: &ComptimeQuery,
    span: Span,
) -> Result<usize, Diagnostic> {
    let mut retained = query
        .tracked
        .map(|tracked| {
            state
                .tracked
                .get(tracked)
                .and_then(Option::as_ref)
                .map(|slot| slot.rows.len())
                .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))
        })
        .transpose()?
        .unwrap_or(query.rows.len());
    for operation in &query.operations {
        let ComptimeQueryOperation::Join { right, .. } = operation else {
            continue;
        };
        let right_query = clone_query(state, right, span)?;
        retained = retained
            .checked_add(query_retained_rows(state, &right_query, span)?)
            .ok_or_else(|| unsupported("query retained row count overflowed", span))?;
    }
    Ok(retained)
}

fn data_limits_from_value(value: Option<&CtValue>) -> DataKernelLimits {
    let defaults = data_kernel_limits_safe();
    let Some(value) = value else {
        return defaults;
    };
    let encoding = struct_field(value, "DataLimits", "encoding").unwrap_or(value);
    DataKernelLimits {
        buffer_bytes: data_int_field(
            struct_field(encoding, "EncodingLimits", "buffer_bytes"),
            defaults.buffer_bytes,
        ),
        max_depth: data_int_field(
            struct_field(encoding, "EncodingLimits", "max_depth"),
            defaults.max_depth,
        ),
        max_item_bytes: data_int_field(
            struct_field(encoding, "EncodingLimits", "max_item_bytes"),
            defaults.max_item_bytes,
        ),
        max_total_bytes: data_optional_int(struct_field(
            encoding,
            "EncodingLimits",
            "max_total_bytes",
        )),
        max_expansion_depth: data_int_field(
            struct_field(encoding, "EncodingLimits", "max_expansion_depth"),
            defaults.max_expansion_depth,
        ),
        max_expansion_bytes: data_int_field(
            struct_field(encoding, "EncodingLimits", "max_expansion_bytes"),
            defaults.max_expansion_bytes,
        ),
        max_groups: data_int_field(
            struct_field(value, "DataLimits", "max_groups"),
            defaults.max_groups,
        ),
        max_sort_rows: data_int_field(
            struct_field(value, "DataLimits", "max_sort_rows"),
            defaults.max_sort_rows,
        ),
        max_join_rows: data_int_field(
            struct_field(value, "DataLimits", "max_join_rows"),
            defaults.max_join_rows,
        ),
        max_output_rows: data_int_field(
            struct_field(value, "DataLimits", "max_output_rows"),
            defaults.max_output_rows,
        ),
    }
}

fn data_kernel_kind_value(kind: jet_foundation::PreludeDataFlow::LoaderKind) -> CtValue {
    data_loader_enum(
        "DataLoaderKind",
        match kind {
            jet_foundation::PreludeDataFlow::LoaderKind::File => "File",
            jet_foundation::PreludeDataFlow::LoaderKind::Url => "Url",
            jet_foundation::PreludeDataFlow::LoaderKind::Database => "Database",
            jet_foundation::PreludeDataFlow::LoaderKind::Value => "Value",
        },
    )
}

fn data_kernel_format_value(format: &str) -> CtValue {
    data_loader_enum(
        "DataFormat",
        match format {
            "csv" => "CSV",
            "json" => "JSON",
            "jsonl" => "JSONL",
            "parquet" => "Parquet",
            "arrow" => "Arrow",
            _ => "JSON",
        },
    )
}

fn data_kernel_authority_value(
    authority: &jet_foundation::PreludeDataFlow::Authority,
) -> CtValue {
    ct_struct(
        "DataAuthority",
        vec![
            ("scope", CtValue::Str(authority.scope.clone())),
            ("revision", CtValue::Str(authority.revision.clone())),
        ],
    )
}

fn data_kernel_source_value(
    source: &jet_foundation::PreludeDataFlow::SourceIdentity,
) -> CtValue {
    ct_struct(
        "DataSourceIdentity",
        vec![
            ("kind", data_kernel_kind_value(source.kind)),
            ("locator", CtValue::Str(source.locator.clone())),
            ("member", CtValue::Str(source.member.clone())),
            (
                "parameters",
                CtValue::List(
                    source
                        .parameters
                        .iter()
                        .cloned()
                        .map(CtValue::Str)
                        .collect(),
                ),
            ),
        ],
    )
}

fn data_kernel_limits_value(limits: &DataKernelLimits) -> CtValue {
    let max_total_bytes = limits
        .max_total_bytes
        .map(|value| CtValue::Present(Box::new(CtValue::Int(value))))
        .unwrap_or_else(|| CtValue::absent(Type::Int));
    ct_struct(
        "DataLimits",
        vec![
            (
                "encoding",
                ct_struct(
                    "EncodingLimits",
                    vec![
                        ("buffer_bytes", CtValue::Int(limits.buffer_bytes)),
                        ("max_depth", CtValue::Int(limits.max_depth)),
                        ("max_item_bytes", CtValue::Int(limits.max_item_bytes)),
                        ("max_total_bytes", max_total_bytes),
                        (
                            "max_expansion_depth",
                            CtValue::Int(limits.max_expansion_depth),
                        ),
                        (
                            "max_expansion_bytes",
                            CtValue::Int(limits.max_expansion_bytes),
                        ),
                    ],
                ),
            ),
            ("max_groups", CtValue::Int(limits.max_groups)),
            ("max_sort_rows", CtValue::Int(limits.max_sort_rows)),
            ("max_join_rows", CtValue::Int(limits.max_join_rows)),
            ("max_output_rows", CtValue::Int(limits.max_output_rows)),
        ],
    )
}

fn data_kernel_status_value(status: &DataKernelStatus) -> CtValue {
    ct_struct(
        "DataLoaderStatus",
        vec![
            ("identity", CtValue::Str(status.identity.clone())),
            (
                "freshness",
                data_loader_enum(
                    "DataFreshness",
                    match status.freshness {
                        jet_foundation::PreludeDataFlow::Freshness::Pending => "Pending",
                        jet_foundation::PreludeDataFlow::Freshness::Fresh => "Fresh",
                        jet_foundation::PreludeDataFlow::Freshness::Stale => "Stale",
                        jet_foundation::PreludeDataFlow::Freshness::Error => "Error",
                        jet_foundation::PreludeDataFlow::Freshness::Offline => "Offline",
                        jet_foundation::PreludeDataFlow::Freshness::Cancelled => "Cancelled",
                    },
                ),
            ),
            (
                "invalidated_by",
                data_loader_enum(
                    "DataInvalidationCause",
                    match status.invalidated_by {
                        jet_foundation::PreludeDataFlow::InvalidationCause::None => "None",
                        jet_foundation::PreludeDataFlow::InvalidationCause::Loader => "Loader",
                        jet_foundation::PreludeDataFlow::InvalidationCause::Input => "Input",
                        jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember => {
                            "ArchiveMember"
                        }
                        jet_foundation::PreludeDataFlow::InvalidationCause::Parameters => {
                            "Parameters"
                        }
                        jet_foundation::PreludeDataFlow::InvalidationCause::Credential => {
                            "Credential"
                        }
                        jet_foundation::PreludeDataFlow::InvalidationCause::Capability => {
                            "Capability"
                        }
                        jet_foundation::PreludeDataFlow::InvalidationCause::Manual => "Manual",
                    },
                ),
            ),
            ("error", CtValue::Str(status.error.clone())),
            ("cleanup", CtValue::Str(status.cleanup.clone())),
            ("buffered_bytes", CtValue::Int(status.buffered_bytes)),
            ("backpressure", CtValue::Bool(status.backpressure)),
            ("last_good", CtValue::Bool(status.last_good)),
        ],
    )
}

fn data_bytes_value(bytes: Option<&Vec<u8>>) -> CtValue {
    bytes
        .map(|bytes| CtValue::Present(Box::new(CtValue::Bytes(bytes.clone()))))
        .unwrap_or_else(|| CtValue::absent(Type::List(Box::new(Type::Named("U8".to_string())))))
}

fn data_loader_state_field() -> String {
    jet_foundation::Names::mangle_generated("data_loader_state")
}

fn data_stream_state_field() -> String {
    jet_foundation::Names::mangle_generated("data_stream_state")
}

fn data_loader_carrier(id: usize, state: &DataKernel) -> CtValue {
    ct_struct(
        "DataLoader",
        vec![
            ("source", data_kernel_source_value(&state.source)),
            ("format", data_kernel_format_value(&state.format)),
            ("authority", data_kernel_authority_value(&state.authority)),
            ("limits", data_kernel_limits_value(&state.limits)),
            ("payload", data_bytes_value(state.payload.as_ref())),
            ("last_good", data_bytes_value(state.last_good.as_ref())),
            ("cancelled", CtValue::Bool(state.cancelled)),
            ("offline", CtValue::Bool(state.offline)),
            (
                data_loader_state_field().as_str(),
                CtValue::Int(
                    i64::try_from(id).expect("data loader state slot exceeds Jet Int range"),
                ),
            ),
        ],
    )
}

fn data_stream_carrier(id: usize) -> CtValue {
    ct_struct(
        "DataStream",
        vec![(
            data_stream_state_field().as_str(),
            CtValue::Int(
                i64::try_from(id).expect("data stream state slot exceeds Jet Int range"),
            ),
        )],
    )
}

fn store_data_loader(state: &mut DataPipelineState, loader: ComptimeLoader) -> CtValue {
    state.loaders.push(Some(loader));
    let id = state.loaders.len();
    let loader_state = state.loaders[id - 1]
        .as_ref()
        .map(|loader| loader.state.clone())
        .expect("loader inserted");
    CtValue::Present(Box::new(data_loader_carrier(id, &loader_state)))
}

fn store_data_stream(state: &mut DataPipelineState, stream: ComptimeStream) -> CtValue {
    state.streams.push(Some(stream));
    data_stream_carrier(state.streams.len())
}

fn data_loader_state_id(
    state: &DataPipelineState,
    value: &CtValue,
    span: Span,
) -> Result<usize, Diagnostic> {
    let field = data_loader_state_field();
    let Some(CtValue::Int(id)) = struct_field(value, "DataLoader", &field) else {
        return Err(unsupported(
            "`DataLoader` carrier has no canonical state identity",
            span,
        ));
    };
    if *id <= 0 {
        return Err(unsupported("`DataLoader` state identity is invalid", span));
    }
    let id = usize::try_from(*id)
        .map_err(|_| unsupported("`DataLoader` state identity is invalid", span))?;
    state
        .loaders
        .get(id - 1)
        .and_then(Option::as_ref)
        .map(|_| id)
        .ok_or_else(|| unsupported("`DataLoader` state identity is stale", span))
}

fn data_stream_state_id(
    state: &DataPipelineState,
    value: &CtValue,
    span: Span,
) -> Result<usize, Diagnostic> {
    let field = data_stream_state_field();
    let Some(CtValue::Int(id)) = struct_field(value, "DataStream", &field) else {
        return Err(unsupported(
            "`DataStream` carrier has no canonical state identity",
            span,
        ));
    };
    if *id <= 0 {
        return Err(unsupported("`DataStream` state identity is invalid", span));
    }
    let id = usize::try_from(*id)
        .map_err(|_| unsupported("`DataStream` state identity is invalid", span))?;
    state
        .streams
        .get(id - 1)
        .and_then(Option::as_ref)
        .map(|_| id)
        .ok_or_else(|| unsupported("`DataStream` state identity is stale", span))
}

fn data_loader_string_arg(
    args: &[CtValue],
    index: usize,
    name: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    args.get(index)
        .ok_or_else(|| unsupported(&format!("`data.{name}()` is missing argument {}", index + 1), span))
        .and_then(|value| as_string(value, span).map(str::to_string))
}

fn data_loader_parameters(
    value: &CtValue,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    expect_list(value, "database", span)?.iter().map(|item| match item {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported("`data.database()` parameters need String values", span)),
    }).collect()
}

fn data_loader_authority(
    scope: &str,
    revision: &str,
) -> Result<jet_foundation::PreludeDataFlow::Authority, CtValue> {
    jet_foundation::PreludeDataFlow::authority(scope, revision)
        .map_err(data_loader_kernel_failure)
}

fn data_loader_new(
    registry: &mut DataPipelineState,
    kind: jet_foundation::PreludeDataFlow::LoaderKind,
    locator: String,
    member: String,
    parameters: Vec<String>,
    format: String,
    limits: DataKernelLimits,
    authority: jet_foundation::PreludeDataFlow::Authority,
    payload: Option<Vec<u8>>,
    decode_ty: Option<Type>,
    seed: Option<CtValue>,
) -> Result<CtValue, CtValue> {
    let mut state = match jet_foundation::PreludeDataFlow::new_loader(
        kind,
        locator,
        member,
        parameters,
        format,
        limits,
        authority,
    ) {
        Ok(state) => state,
        Err(error) => return Err(data_loader_kernel_failure(error)),
    };
    if let Some(payload) = payload {
        if let Err(error) = jet_foundation::PreludeDataFlow::check_payload(
            &state.limits,
            payload.len(),
            "data.value",
        ) {
            return Err(data_loader_kernel_failure(error));
        }
        state.payload = Some(payload);
    }
    Ok(store_data_loader(
        registry,
        ComptimeLoader {
            state,
            seed,
            decode_ty,
        },
    ))
}
fn loader_type_from_resolved(value: Option<&Type>) -> Option<Type> {
    match value? {
        Type::Result { ok, .. } | Type::Option(ok) => loader_type_from_resolved(Some(ok)),
        Type::Apply { name, args }
            if matches!(name.as_str(), "DataLoader" | "DataSnapshot" | "DataStream")
                && args.len() == 1 =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

fn typed_decode_failure_text(value: &CtValue) -> String {
    let CtValue::List(errors) = value else {
        return value.jet_show();
    };
    let reasons = errors
        .iter()
        .filter_map(|error| {
            let path = struct_field(error, "FieldError", "path").map(query_cell_text)?;
            let reason = struct_field(error, "FieldError", "reason").map(query_cell_text)?;
            Some(if path.is_empty() {
                reason
            } else {
                format!("{path}: {reason}")
            })
        })
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        value.jet_show()
    } else {
        reasons.join("; ")
    }
}
fn data_typed_tree(value: &CtValue) -> CtValue {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DataTree" && variant == "TypedText" => {
            super::JSONInterp::json_variant(
                "Text",
                args.first().map(|(_, value)| data_typed_tree(value)),
            )
        }
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DataTree" => CtValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args
                .iter()
                .map(|(name, value)| (name.clone(), data_typed_tree(value)))
                .collect(),
        },
        _ => value.clone(),
    }
}


fn data_dynamic_object(fields: Vec<(String, CtValue)>) -> CtValue {
    super::JSONInterp::json_variant(
        "Object",
        Some(CtValue::Struct {
            type_name: "JSONObject".to_string(),
            fields,
        }),
    )
}

fn loader_type_for_call(type_args: &[Type], resolved_ret: Option<&Type>) -> Option<Type> {
    type_args
        .first()
        .cloned()
        .or_else(|| loader_type_from_resolved(resolved_ret))
}

fn data_dynamic_array(values: Vec<CtValue>) -> CtValue {
    super::JSONInterp::json_variant("Array", Some(CtValue::List(values)))
}

fn data_dynamic_rows(tree: &CtValue) -> Vec<CtValue> {
    super::JSONInterp::json_payload(tree, "Array")
        .and_then(|value| match value {
            CtValue::List(values) => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_else(|| vec![tree.clone()])
}

fn data_arrow_limits(
    limits: &DataKernelLimits,
) -> Result<jet_foundation::ArrowData::ArrowLimits, CtValue> {
    let max_buffer_bytes = usize::try_from(limits.max_expansion_bytes).map_err(|_| {
        data_loader_failure(
            "Limit",
            "data.loader.parquet",
            "max_expansion_bytes cannot be represented by this target",
        )
    })?;
    let max_total_i64 = limits.max_total_bytes.unwrap_or_else(|| {
        limits
            .max_item_bytes
            .max(limits.max_expansion_bytes)
    });
    let max_total_bytes = usize::try_from(max_total_i64).map_err(|_| {
        data_loader_failure(
            "Limit",
            "data.loader.parquet",
            "max_total_bytes cannot be represented by this target",
        )
    })?;
    Ok(jet_foundation::ArrowData::ArrowLimits {
        max_rows: limits.max_output_rows,
        max_columns: 1024,
        max_children: limits.max_depth,
        max_dictionary_values: limits.max_output_rows,
        max_buffer_bytes,
        max_total_bytes,
    })
}

fn data_arrow_tree_value(value: jet_foundation::DataTree::DataTree) -> CtValue {
    match value {
        jet_foundation::DataTree::DataTree::Null => super::JSONInterp::json_variant("Null", None),
        jet_foundation::DataTree::DataTree::Bool(value) => {
            super::JSONInterp::json_variant("Bool", Some(CtValue::Bool(value)))
        }
        jet_foundation::DataTree::DataTree::Int(value) => {
            super::JSONInterp::json_variant("Int", Some(CtValue::Int(value)))
        }
        jet_foundation::DataTree::DataTree::Float(value) => super::JSONInterp::json_variant(
            "Float",
            Some(CtValue::Float(CtFloat::f64(value))),
        ),
        jet_foundation::DataTree::DataTree::Number(value) => {
            super::JSONInterp::json_variant("Number", Some(CtValue::Str(value)))
        }
        jet_foundation::DataTree::DataTree::TypedText(value)
        | jet_foundation::DataTree::DataTree::Text(value) => {
            super::JSONInterp::json_variant("Text", Some(CtValue::Str(value)))
        }
        jet_foundation::DataTree::DataTree::Bytes(value) => {
            super::JSONInterp::json_variant("Bytes", Some(CtValue::Bytes(value)))
        }
        jet_foundation::DataTree::DataTree::Array(values) => data_dynamic_array(
            values
                .into_iter()
                .map(data_arrow_tree_value)
                .collect(),
        ),
        jet_foundation::DataTree::DataTree::Object(fields) => data_dynamic_object(
            fields
                .into_iter()
                .map(|(key, value)| (key, data_arrow_tree_value(value)))
                .collect(),
        ),
    }
}

fn data_arrow_tree_failure(error: jet_foundation::ArrowData::ArrowError) -> CtValue {
    let kind = match error.kind {
        jet_foundation::ArrowData::ArrowErrorCode::Limit => "Limit",
        jet_foundation::ArrowData::ArrowErrorCode::ProducerFailure => "Bridge",
        _ => "Decode",
    };
    data_loader_failure(kind, "data.loader.parquet", error.to_string())
}

fn data_arrow_tree(
    payload: &[u8],
    limits: &DataKernelLimits,
) -> Result<CtValue, CtValue> {
    let arrow_limits = data_arrow_limits(limits)?;
    jet_foundation::ArrowFileReader::read_tree(
        jet_foundation::ArrowFileReader::FORMAT_PARQUET,
        payload,
        &arrow_limits,
    )
    .map(data_arrow_tree_value)
    .map_err(data_arrow_tree_failure)
}

fn data_dynamic_tree(
    payload: &[u8],
    format: &str,
    limits: &DataKernelLimits,
) -> Result<CtValue, CtValue> {
    if format == "parquet" {
        return data_arrow_tree(payload, limits);
    }
    if format == "arrow" {
        return Err(data_loader_failure(
            "Bridge",
            "data.loader",
            "Arrow IPC has a separate provider boundary; Parquet is the registered reader",
        ));
    }
    let text = String::from_utf8(payload.to_vec()).map_err(|error| {
        data_loader_failure(
            "Decode",
            "data.loader",
            format!("payload is not UTF-8: {error}"),
        )
    })?;
    let tree = match format {
        "json" => super::JSONInterp::parse_json_typed_ordered(&text).map_err(|error| {
            data_loader_failure(
                "Decode",
                "data.loader.json",
                format!("invalid JSON (line {}): {}", error.line, error.message),
            )
        })?,
        "jsonl" => {
            let mut values = Vec::new();
            for (line_index, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if values.len() as i64 >= limits.max_output_rows {
                    return Err(data_loader_failure(
                        "Limit",
                        "data.loader.jsonl",
                        format!("max_output_rows {} exceeded", limits.max_output_rows),
                    ));
                }
                let value = super::JSONInterp::parse_json_typed_ordered(line).map_err(|error| {
                    data_loader_failure(
                        "Decode",
                        "data.loader.jsonl",
                        format!(
                            "invalid JSONL record at line {} (line {}): {}",
                            line_index + 1,
                            error.line,
                            error.message
                        ),
                    )
                })?;
                values.push(value);
            }
            data_dynamic_array(values)
        }
        "csv" => {
            let records = super::EncodingLite::csv_parse(&text, ",", false, false)
                .map_err(|reason| data_loader_failure("Decode", "data.loader.csv", reason))?;
            let Some(header) = records.first() else {
                return Ok(data_dynamic_array(Vec::new()));
            };
            let mut header_names = std::collections::BTreeSet::new();
            for name in &header.fields {
                if let Err(error) =
                    jet_foundation::PreludeDataFlow::validate_public_text("CSV header", name, false)
                {
                    return Err(data_loader_kernel_failure(error));
                }
                if !header_names.insert(name.clone()) {
                    return Err(data_loader_failure(
                        "Decode",
                        "data.loader.csv",
                        format!("duplicate CSV header `{name}`"),
                    ));
                }
            }
            let mut values = Vec::new();
            for record in records.iter().skip(1) {
                if values.len() as i64 >= limits.max_output_rows {
                    return Err(data_loader_failure(
                        "Limit",
                        "data.loader.csv",
                        format!("max_output_rows {} exceeded", limits.max_output_rows),
                    ));
                }
                if record.fields.len() > header.fields.len() {
                    return Err(data_loader_failure(
                        "Decode",
                        "data.loader.csv",
                        format!(
                            "CSV row has {} fields; header has {}",
                            record.fields.len(),
                            header.fields.len()
                        ),
                    ));
                }
                let fields = header
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        (
                            name.clone(),
                            record
                                .fields
                                .get(index)
                                .map(|value| {
                                    super::JSONInterp::json_variant(
                                        "TypedText",
                                        Some(CtValue::Str(value.clone())),
                                    )
                                })
                                .unwrap_or_else(|| super::JSONInterp::json_variant("Null", None)),
                        )
                    })
                    .collect();
                values.push(data_dynamic_object(fields));
            }
            data_dynamic_array(values)
        }
        "parquet" | "arrow" => {
            return Err(data_loader_bridge("data.loader"));
        }
        _ => {
            return Err(data_loader_failure(
                "Bridge",
                "data.loader",
                format!("format `{format}` is provider-owned; import its package"),
            ));
        }
    };
    if data_dynamic_rows(&tree).len() as i64 > limits.max_output_rows {
        return Err(data_loader_failure(
            "Limit",
            "data.loader",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(tree)
}

fn data_loader_payload(
    state: &DataKernel,
) -> Result<(Vec<u8>, bool), CtValue> {
    if let Some(payload) = jet_foundation::PreludeDataFlow::payload(state)
        .map_err(data_loader_kernel_failure)?
    {
        return Ok(payload);
    }
    if state.source.kind == jet_foundation::PreludeDataFlow::LoaderKind::File
        && state.source.member.is_empty()
    {
        let payload = std::fs::read(&state.raw_locator).map_err(|error| {
            data_loader_failure(
                "IO",
                "data.loader.file",
                format!("could not read `{}`: {error}", state.source.locator),
            )
        })?;
        jet_foundation::PreludeDataFlow::check_payload(
            &state.limits,
            payload.len(),
            "data.loader.file",
        )
        .map_err(data_loader_kernel_failure)?;
        return Ok((payload, state.offline));
    }
    let operation = match state.source.kind {
        jet_foundation::PreludeDataFlow::LoaderKind::Url => "data.loader.url",
        jet_foundation::PreludeDataFlow::LoaderKind::Database => "data.loader.database",
        jet_foundation::PreludeDataFlow::LoaderKind::File => "data.loader.archive",
        jet_foundation::PreludeDataFlow::LoaderKind::Value => "data.loader.value",
    };
    Err(data_loader_bridge(operation))
}

fn data_dynamic_scalar_type(value: &CtValue) -> (&'static str, bool) {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            ..
        } if type_name == "DataTree" => match variant.as_str() {
            "Null" => ("null", true),
            "Bool" => ("bool", false),
            "Number" => ("float", false),
            "TypedText" | "Text" => ("text", false),
            "Array" => ("array", false),
            "Object" => ("object", false),
            _ => ("Any", false),
        },
        CtValue::Bool(_) => ("bool", false),
        CtValue::Int(_) | CtValue::BigInt(_) => ("int", false),
        CtValue::Float(_) => ("float", false),
        CtValue::Str(_) => ("text", false),
        CtValue::Bytes(_) => ("bytes", false),
        CtValue::List(_) => ("array", false),
        CtValue::Map(_) => ("object", false),
        CtValue::Struct { .. } => ("object", false),
        _ => ("Any", false),
    }
}

fn data_dynamic_merge_type(left: &str, right: &str) -> (&'static str, bool) {
    if left == right {
        return (
            match left {
                "null" => "null",
                "bool" => "bool",
                "int" => "int",
                "float" => "float",
                "text" => "text",
                "bytes" => "bytes",
                "array" => "array",
                "object" => "object",
                _ => "Any",
            },
            left == "null",
        );
    }
    if left == "null" {
        return (match right {
            "bool" => "bool",
            "int" => "int",
            "float" => "float",
            "text" => "text",
            "bytes" => "bytes",
            "array" => "array",
            "object" => "object",
            _ => "Any",
        }, true);
    }
    if right == "null" {
        return (match left {
            "bool" => "bool",
            "int" => "int",
            "float" => "float",
            "text" => "text",
            "bytes" => "bytes",
            "array" => "array",
            "object" => "object",
            _ => "Any",
        }, true);
    }
    if (left == "int" && right == "float") || (left == "float" && right == "int") {
        return ("float", false);
    }
    ("Any", false)
}

fn data_dynamic_object_fields(value: &CtValue) -> Option<Vec<(String, CtValue)>> {
    if let CtValue::Struct { type_name, fields } = value {
        if type_name == "JSONObject" {
            return Some(fields.clone());
        }
    }
    super::JSONInterp::json_payload(value, "Object")
        .and_then(|value| data_dynamic_object_fields(value))
}

fn data_schema_value(tree: &CtValue, format: &str) -> CtValue {
    let rows = data_dynamic_rows(tree);
    let mut fields = BTreeMap::<String, (&'static str, bool)>::new();
    let mut rows_seen = 0usize;
    for row in &rows {
        if let Some(entries) = data_dynamic_object_fields(row) {
            let present = entries
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<std::collections::BTreeSet<_>>();
            for (name, value) in entries {
                let (current, nullable) = data_dynamic_scalar_type(&value);
                let (merged, merged_nullable) = fields
                    .get(&name)
                    .map(|(previous, previous_nullable)| {
                        let (merged, type_nullable) = data_dynamic_merge_type(previous, current);
                        (merged, *previous_nullable || nullable || type_nullable)
                    })
                    .unwrap_or((current, nullable));
                fields.insert(name, (merged, merged_nullable || rows_seen > 0));
            }
            for name in fields
                .keys()
                .filter(|name| !present.contains(*name))
                .cloned()
                .collect::<Vec<_>>()
            {
                if let Some((type_name, _)) = fields.get(&name).copied() {
                    fields.insert(name, (type_name, true));
                }
            }
        } else {
            let (current, nullable) = data_dynamic_scalar_type(row);
            let (merged, merged_nullable) = fields
                .get("$")
                .map(|(previous, previous_nullable)| {
                    let (merged, type_nullable) = data_dynamic_merge_type(previous, current);
                    (merged, *previous_nullable || nullable || type_nullable)
                })
                .unwrap_or((current, nullable || rows_seen > 0));
            fields.insert("$".to_string(), (merged, merged_nullable));
        }
        rows_seen = rows_seen.saturating_add(1);
    }
    let columns = fields
        .into_iter()
        .enumerate()
        .map(|(order, (name, (type_name, nullable)))| {
            data_column_with_id(order, &name, type_name, nullable)
        })
        .collect::<Vec<_>>();
    let mut identity = String::new();
    jet_foundation::PreludeDataFlow::identity_part(&mut identity, format);
    for column in &columns {
        for name in ["id", "name", "type_name", "nullable"] {
            if let Some(value) = struct_field(column, "DataColumn", name) {
                let text = match value {
                    CtValue::Bool(value) => value.to_string(),
                    CtValue::Str(value) => value.clone(),
                    _ => String::new(),
                };
                jet_foundation::PreludeDataFlow::identity_part(&mut identity, &text);
            }
        }
    }
    ct_struct(
        "DataSchema",
        vec![
            (
                "identity",
                CtValue::Str(jet_foundation::PreludeDataFlow::digest(identity.as_bytes())),
            ),
            ("format", data_kernel_format_value(format)),
            ("columns", CtValue::List(columns)),
            (
                "projection",
                CtValue::absent(Type::Named("ShapeProjection".to_string())),
            ),
        ],
    )
}

fn data_column_with_id(
    order: usize,
    name: &str,
    type_name: &str,
    nullable: bool,
) -> CtValue {
    ct_struct(
        "DataColumn",
        vec![
            (
                "id",
                CtValue::Str(format!("column-{order}-{name}")),
            ),
            ("name", CtValue::Str(name.to_string())),
            ("type_name", CtValue::Str(type_name.to_string())),
            ("nullable", CtValue::Bool(nullable)),
        ],
    )
}

fn data_loader_fail_value(
    loader: &mut ComptimeLoader,
    value: &CtValue,
    operation: &str,
) {
    let kind = match struct_field(value, "DataError", "kind") {
        Some(CtValue::Enum { variant, .. }) if variant == "Limit" => {
            jet_foundation::PreludeDataFlow::ErrorKind::Limit
        }
        Some(CtValue::Enum { variant, .. }) if variant == "State" => {
            jet_foundation::PreludeDataFlow::ErrorKind::State
        }
        Some(CtValue::Enum { variant, .. }) if variant == "Bridge" => {
            jet_foundation::PreludeDataFlow::ErrorKind::Bridge
        }
        _ => jet_foundation::PreludeDataFlow::ErrorKind::InvalidArgument,
    };
    let reason = struct_field(value, "DataError", "reason")
        .map(query_cell_text)
        .unwrap_or_else(|| value.jet_show());
    let error = jet_foundation::PreludeDataFlow::KernelError {
        kind,
        operation: operation.to_string(),
        reason,
    };
    jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
}

fn data_dynamic_canonical(value: &CtValue) -> CtValue {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DataTree" && variant == "Object" => {
            let Some(CtValue::Struct {
                type_name: object_type,
                fields,
            }) = args.first().map(|(_, value)| value)
            else {
                return value.clone();
            };
            if object_type != "JSONObject" {
                return value.clone();
            }
            let mut fields = fields
                .iter()
                .map(|(name, value)| (name.clone(), data_dynamic_canonical(value)))
                .collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            data_dynamic_object(fields)
        }
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DataTree" && variant == "Array" => {
            let Some(CtValue::List(values)) = args.first().map(|(_, value)| value) else {
                return value.clone();
            };
            data_dynamic_array(
                values
                    .iter()
                    .map(data_dynamic_canonical)
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

fn data_snapshot_dynamic(
    loader: &mut ComptimeLoader,
    interp: &mut super::Interpreter::Interp<'_>,
    span: Span,
) -> Result<CtValue, CtValue> {
    if loader.state.cancelled {
        let error = jet_foundation::PreludeDataFlow::KernelError {
            kind: jet_foundation::PreludeDataFlow::ErrorKind::State,
            operation: "data.loader".to_string(),
            reason: "loader was cancelled before snapshot".to_string(),
        };
        jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
        return Err(data_loader_kernel_failure(error));
    }
    if let Err(error) = jet_foundation::PreludeDataFlow::validate_loader(&loader.state) {
        jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
        return Err(data_loader_kernel_failure(error));
    }
    if let Err(error) = jet_foundation::PreludeDataFlow::validate_limits(&loader.state.limits) {
        jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
        return Err(data_loader_kernel_failure(error));
    }
    let (payload, offline) = match data_loader_payload(&loader.state) {
        Ok(payload) => payload,
        Err(error) => {
            data_loader_fail_value(loader, &error, "data.loader");
            return Err(error);
        }
    };
    let tree = match data_dynamic_tree(&payload, &loader.state.format, &loader.state.limits) {
        Ok(tree) => tree,
        Err(error) => {
            data_loader_fail_value(loader, &error, "data.loader");
            return Err(error);
        }
    };
    let value = if let Some(ty) = loader.decode_ty.clone() {
        let decode_tree = if loader.state.format == "csv" {
            data_typed_tree(&tree)
        } else {
            tree.clone()
        };
        match interp.typed_decode_top(&ty, &decode_tree, span) {
            Ok(value) => value,
            Err(error) => {
                let failure = data_loader_failure(
                    "Decode",
                    "data.loader",
                    format!("typed decode failed: {}", typed_decode_failure_text(&error)),
                );
                data_loader_fail_value(loader, &failure, "data.loader");
                return Err(failure);
            }
        }
    } else {
        loader.seed.clone().unwrap_or_else(|| tree.clone())
    };
    let canonical_value = if loader.decode_ty.is_some() {
        match interp.encode_value(&value, span) {
            Ok(value) => value,
            Err(error) => {
                let failure = data_loader_failure(
                    "Decode",
                    "data.loader",
                    format!("typed encode failed: {error:?}"),
                );
                data_loader_fail_value(loader, &failure, "data.loader");
                return Err(failure);
            }
        }
    } else {
        data_dynamic_canonical(&value)
    };
    let canonical =
        super::JSONInterp::render_ordered_datatree(&canonical_value, false, 0).into_bytes();
    let schema = data_schema_value(&tree, &loader.state.format);
    let schema_id = match struct_field(&schema, "DataSchema", "identity") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => String::new(),
    };
    let mut state = loader.state.clone();
    let facts = match jet_foundation::PreludeDataFlow::commit_snapshot(
        &mut state,
        payload,
        &canonical,
        &schema_id,
        offline,
    ) {
        Ok(facts) => facts,
        Err(error) => {
            jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
            return Err(data_loader_kernel_failure(error));
        }
    };
    loader.state = state;
    let identity = ct_struct(
        "DataSnapshotIdentity",
        vec![
            ("id", CtValue::Str(format!("snapshot-{}", facts.snapshot_id))),
            ("source", CtValue::Str(format!("source-{}", facts.source_id))),
            ("content", CtValue::Str(facts.content_id)),
            ("schema", CtValue::Str(facts.schema_id)),
            ("format", data_kernel_format_value(&loader.state.format)),
        ],
    );
    let provenance = ct_struct(
        "DataProvenance",
        vec![
            ("source", data_kernel_source_value(&loader.state.source)),
            ("format", data_kernel_format_value(&loader.state.format)),
            ("authority", data_kernel_authority_value(&loader.state.authority)),
        ],
    );
    Ok(ct_struct(
        "DataSnapshot",
        vec![
            ("value", value),
            ("identity", identity),
            ("provenance", provenance),
            ("schema", schema),
            (
                "status",
                data_kernel_status_value(&jet_foundation::PreludeDataFlow::status(
                    &loader.state,
                )),
            ),
            ("content", CtValue::Bytes(canonical)),
        ],
    ))
}

fn data_stream_rows(
    loader: &ComptimeLoader,
    interp: &mut super::Interpreter::Interp<'_>,
    span: Span,
) -> Result<Vec<CtValue>, CtValue> {
    let (payload, _) = data_loader_payload(&loader.state)?;
    let tree = data_dynamic_tree(&payload, &loader.state.format, &loader.state.limits)?;
    let rows = data_dynamic_rows(&tree);
    let Some(ty) = loader.decode_ty.clone() else {
        return Ok(rows);
    };
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let row = if loader.state.format == "csv" {
                data_typed_tree(&row)
            } else {
                row
            };
            interp.typed_decode_top(&ty, &row, span).map_err(|error| {
                data_loader_failure(
                    "Decode",
                    "data.loader.stream",
                    format!(
                        "typed decode failed at row {}: {}",
                        index + 1,
                        typed_decode_failure_text(&error)
                    ),
                )
            })
        })
        .collect()
}
fn data_stream_next(stream: &mut ComptimeStream) -> CtValue {
    if stream.cancelled {
        return data_loader_failure("State", "data.stream", "stream was cancelled");
    }
    if let Some(row) = stream.rows.get(stream.cursor).cloned() {
        stream.cursor += 1;
        return CtValue::Present(Box::new(CtValue::Present(Box::new(row))));
    }
    let row_ty = stream
        .row_ty
        .clone()
        .unwrap_or_else(|| Type::Named("Unknown".to_string()));
    CtValue::Present(Box::new(CtValue::absent(row_ty)))
}
fn data_state_plain_tree(value: &CtValue) -> CtValue {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DataTree" => match variant.as_str() {
            "Null" => CtValue::Unit,
            "Bool" | "Int" | "Float" | "Text" | "TypedText" | "Bytes" => args
                .first()
                .map(|(_, value)| data_state_plain_tree(value))
                .unwrap_or(CtValue::Unit),
            "Number" => {
                let Some(CtValue::Str(value)) = args.first().map(|(_, value)| value) else {
                    return CtValue::Unit;
                };
                value
                    .parse::<i64>()
                    .map(CtValue::Int)
                    .or_else(|_| value.parse::<f64>().map(|value| CtValue::Float(CtFloat::f64(value))))
                    .unwrap_or_else(|_| CtValue::Str(value.clone()))
            }
            "Array" => CtValue::List(
                args.first()
                    .and_then(|(_, value)| match value {
                        CtValue::List(values) => Some(
                            values
                                .iter()
                                .map(data_state_plain_tree)
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    })
                    .unwrap_or_default(),
            ),
            "Object" => CtValue::Struct {
                type_name: "JSONObject".to_string(),
                fields: args
                    .first()
                    .and_then(|(_, value)| data_dynamic_object_fields(value))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(name, value)| (name, data_state_plain_tree(&value)))
                    .collect(),
            },
            _ => value.clone(),
        },
        CtValue::List(values) => {
            CtValue::List(values.iter().map(data_state_plain_tree).collect())
        }
        CtValue::Map(values) => CtValue::Map(
            values
                .iter()
                .map(|(key, value)| (key.clone(), data_state_plain_tree(value)))
                .collect(),
        ),
        CtValue::Struct { type_name, fields } => CtValue::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.clone(), data_state_plain_tree(value)))
                .collect(),
        },
        CtValue::Present(value) => data_state_plain_tree(value),
        _ => value.clone(),
    }
}

fn data_state_decode_tree(ty: &Type, tree: &CtValue) -> Result<CtValue, CtValue> {
    let plain = data_state_plain_tree(tree);
    match ty {
        Type::Int | Type::IntN { .. } | Type::InlineRange { .. } => match plain {
            CtValue::Int(value) => Ok(CtValue::Int(value)),
            CtValue::Str(value) => value.parse::<i64>().map(CtValue::Int).map_err(|_| {
                data_loader_failure(
                    "Decode",
                    "data.loader",
                    format!("expected Int for `{}`", ty.name()),
                )
            }),
            _ => Err(data_loader_failure(
                "Decode",
                "data.loader",
                format!("expected Int for `{}`", ty.name()),
            )),
        },
        Type::Float | Type::Float32 => match plain {
            CtValue::Float(value) => Ok(CtValue::Float(value)),
            CtValue::Int(value) => Ok(CtValue::Float(CtFloat::f64(value as f64))),
            CtValue::Str(value) => value
                .parse::<f64>()
                .map(|value| CtValue::Float(CtFloat::f64(value)))
                .map_err(|_| {
                    data_loader_failure(
                        "Decode",
                        "data.loader",
                        format!("expected Float for `{}`", ty.name()),
                    )
                }),
            _ => Err(data_loader_failure(
                "Decode",
                "data.loader",
                format!("expected Float for `{}`", ty.name()),
            )),
        },
        Type::Bool => match plain {
            CtValue::Bool(value) => Ok(CtValue::Bool(value)),
            _ => Err(data_loader_failure(
                "Decode",
                "data.loader",
                format!("expected Bool for `{}`", ty.name()),
            )),
        },
        Type::String => match plain {
            CtValue::Str(value) => Ok(CtValue::Str(value)),
            _ => Err(data_loader_failure(
                "Decode",
                "data.loader",
                format!("expected String for `{}`", ty.name()),
            )),
        },
        Type::Char => match plain {
            CtValue::Str(value) => value
                .chars()
                .next()
                .filter(|_| value.chars().count() == 1)
                .map(CtValue::Char)
                .ok_or_else(|| {
                    data_loader_failure(
                        "Decode",
                        "data.loader",
                        format!("expected one character for `{}`", ty.name()),
                    )
                }),
            _ => Err(data_loader_failure(
                "Decode",
                "data.loader",
                format!("expected one character for `{}`", ty.name()),
            )),
        },
        Type::Option(inner) => {
            if matches!(plain, CtValue::Unit) {
                Ok(CtValue::absent((**inner).clone()))
            } else {
                data_state_decode_tree(inner, tree)
                    .map(|value| CtValue::Present(Box::new(value)))
            }
        }
        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
            let CtValue::List(values) = plain else {
                return Err(data_loader_failure(
                    "Decode",
                    "data.loader",
                    format!("expected a list for `{}`", ty.name()),
                ));
            };
            let mut decoded = Vec::with_capacity(values.len());
            for value in values {
                decoded.push(data_state_decode_tree(inner, &value)?);
            }
            Ok(CtValue::List(decoded))
        }
        Type::Map {
            key,
            value: value_ty,
            ..
        } if matches!(key.as_ref(), Type::String) => {
            let CtValue::Struct { fields, .. } = plain else {
                return Err(data_loader_failure(
                    "Decode",
                    "data.loader",
                    format!("expected an object for `{}`", ty.name()),
                ));
            };
            let mut decoded = BTreeMap::new();
            for (name, cell) in fields {
                decoded.insert(
                    CtKey::Str(name),
                    data_state_decode_tree(value_ty, &cell)?,
                );
            }
            Ok(CtValue::Map(decoded))
        }
        Type::Map { .. } => Err(data_loader_failure(
            "Decode",
            "data.loader",
            "loader maps require String keys",
        )),
        Type::Result { ok, .. } => data_state_decode_tree(ok, tree)
            .map(|value| CtValue::Present(Box::new(value))),
        Type::Shared(inner)
        | Type::Tagged { inner, .. }
        | Type::Quantity { base: inner, .. } => data_state_decode_tree(inner, tree),
        Type::Named(name) => match plain {
            CtValue::Struct { fields, .. } => Ok(CtValue::Struct {
                type_name: name.clone(),
                fields,
            }),
            value => Ok(value),
        },
        Type::Apply { name, .. } => match plain {
            CtValue::Struct { fields, .. } => Ok(CtValue::Struct {
                type_name: name.clone(),
                fields,
            }),
            value => Ok(value),
        },
        Type::Union(_) => Ok(plain),
        _ => Err(data_loader_failure(
            "Decode",
            "data.loader",
            format!("MIR loader cannot decode `{}`", ty.name()),
        )),
    }
}

fn data_state_encode_tree(value: &CtValue) -> Result<CtValue, CtValue> {
    match value {
        CtValue::Int(value) => Ok(super::JSONInterp::json_variant(
            "Int",
            Some(CtValue::Int(*value)),
        )),
        CtValue::BigInt(value) => Ok(super::JSONInterp::json_variant(
            "Int",
            Some(CtValue::BigInt(value.clone())),
        )),
        CtValue::Float(value) => Ok(super::JSONInterp::json_variant(
            "Float",
            Some(CtValue::Float(*value)),
        )),
        CtValue::Bool(value) => Ok(super::JSONInterp::json_variant(
            "Bool",
            Some(CtValue::Bool(*value)),
        )),
        CtValue::Str(value) => Ok(super::JSONInterp::json_variant(
            "Text",
            Some(CtValue::Str(value.clone())),
        )),
        CtValue::Char(value) => Ok(super::JSONInterp::json_variant(
            "Text",
            Some(CtValue::Str(value.to_string())),
        )),
        CtValue::Bytes(value) => Ok(super::JSONInterp::json_variant(
            "Bytes",
            Some(CtValue::Bytes(value.clone())),
        )),
        CtValue::List(values) => values
            .iter()
            .map(data_state_encode_tree)
            .collect::<Result<Vec<_>, _>>()
            .map(|values| super::JSONInterp::json_variant("Array", Some(CtValue::List(values)))),
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                let CtKey::Str(key) = key else {
                    return Err(data_loader_failure(
                        "Decode",
                        "data.loader",
                        "loader maps require String keys",
                    ));
                };
                Ok((key.clone(), data_state_encode_tree(value)?))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|fields| {
                super::JSONInterp::json_variant(
                    "Object",
                    Some(CtValue::Struct {
                        type_name: "JSONObject".to_string(),
                        fields,
                    }),
                )
            }),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .map(|(name, value)| Ok((name.clone(), data_state_encode_tree(value)?)))
            .collect::<Result<Vec<_>, CtValue>>()
            .map(|fields| {
                super::JSONInterp::json_variant(
                    "Object",
                    Some(CtValue::Struct {
                        type_name: "JSONObject".to_string(),
                        fields,
                    }),
                )
            }),
        CtValue::Present(value) => data_state_encode_tree(value),
        CtValue::Failed(CtReport::Clean(_)) | CtValue::Unit => {
            Ok(super::JSONInterp::json_variant("Null", None))
        }
        CtValue::Enum { type_name, .. } if type_name == "DataTree" => Ok(value.clone()),
        CtValue::Enum { .. } | CtValue::Failed(CtReport::Told(_)) => Err(data_loader_failure(
            "Decode",
            "data.loader",
            "typed loader value has no canonical Encode representation",
        )),
        CtValue::Closure(_) => Err(data_loader_failure(
            "Decode",
            "data.loader",
            "closures cannot cross the typed loader boundary",
        )),
    }
}

fn data_snapshot_dynamic_in_state(
    loader: &mut ComptimeLoader,
) -> Result<CtValue, CtValue> {
    if loader.state.cancelled {
        let error = jet_foundation::PreludeDataFlow::KernelError {
            kind: jet_foundation::PreludeDataFlow::ErrorKind::State,
            operation: "data.loader".to_string(),
            reason: "loader was cancelled before snapshot".to_string(),
        };
        jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
        return Err(data_loader_kernel_failure(error));
    }
    if let Err(error) = jet_foundation::PreludeDataFlow::validate_loader(&loader.state) {
        jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
        return Err(data_loader_kernel_failure(error));
    }
    if let Err(error) = jet_foundation::PreludeDataFlow::validate_limits(&loader.state.limits) {
        jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
        return Err(data_loader_kernel_failure(error));
    }
    let (payload, offline) = match data_loader_payload(&loader.state) {
        Ok(payload) => payload,
        Err(error) => {
            data_loader_fail_value(loader, &error, "data.loader");
            return Err(error);
        }
    };
    let tree = match data_dynamic_tree(&payload, &loader.state.format, &loader.state.limits) {
        Ok(tree) => tree,
        Err(error) => {
            data_loader_fail_value(loader, &error, "data.loader");
            return Err(error);
        }
    };
    let value = if let Some(ty) = loader.decode_ty.clone() {
        let decode_tree = if loader.state.format == "csv" {
            data_typed_tree(&tree)
        } else {
            tree.clone()
        };
        match data_state_decode_tree(&ty, &decode_tree) {
            Ok(value) => value,
            Err(error) => {
                data_loader_fail_value(loader, &error, "data.loader");
                return Err(error);
            }
        }
    } else {
        loader.seed.clone().unwrap_or_else(|| tree.clone())
    };
    let canonical_value = if loader.decode_ty.is_some() {
        match data_state_encode_tree(&value) {
            Ok(value) => value,
            Err(error) => {
                data_loader_fail_value(loader, &error, "data.loader");
                return Err(error);
            }
        }
    } else {
        data_dynamic_canonical(&value)
    };
    let canonical =
        super::JSONInterp::render_ordered_datatree(&canonical_value, false, 0).into_bytes();
    let schema = data_schema_value(&tree, &loader.state.format);
    let schema_id = match struct_field(&schema, "DataSchema", "identity") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => String::new(),
    };
    let mut state = loader.state.clone();
    let facts = match jet_foundation::PreludeDataFlow::commit_snapshot(
        &mut state,
        payload,
        &canonical,
        &schema_id,
        offline,
    ) {
        Ok(facts) => facts,
        Err(error) => {
            jet_foundation::PreludeDataFlow::fail(&mut loader.state, &error);
            return Err(data_loader_kernel_failure(error));
        }
    };
    loader.state = state;
    let identity = ct_struct(
        "DataSnapshotIdentity",
        vec![
            ("id", CtValue::Str(format!("snapshot-{}", facts.snapshot_id))),
            ("source", CtValue::Str(format!("source-{}", facts.source_id))),
            ("content", CtValue::Str(facts.content_id)),
            ("schema", CtValue::Str(facts.schema_id)),
            ("format", data_kernel_format_value(&loader.state.format)),
        ],
    );
    let provenance = ct_struct(
        "DataProvenance",
        vec![
            ("source", data_kernel_source_value(&loader.state.source)),
            ("format", data_kernel_format_value(&loader.state.format)),
            ("authority", data_kernel_authority_value(&loader.state.authority)),
        ],
    );
    Ok(ct_struct(
        "DataSnapshot",
        vec![
            ("value", value),
            ("identity", identity),
            ("provenance", provenance),
            ("schema", schema),
            (
                "status",
                data_kernel_status_value(&jet_foundation::PreludeDataFlow::status(
                    &loader.state,
                )),
            ),
            ("content", CtValue::Bytes(canonical)),
        ],
    ))
}

fn data_stream_rows_in_state(loader: &ComptimeLoader) -> Result<Vec<CtValue>, CtValue> {
    let (payload, _) = data_loader_payload(&loader.state)?;
    let tree = data_dynamic_tree(&payload, &loader.state.format, &loader.state.limits)?;
    let rows = data_dynamic_rows(&tree);
    let Some(ty) = loader.decode_ty.clone() else {
        return Ok(rows);
    };
    rows.into_iter()
        .map(|row| {
            let row = if loader.state.format == "csv" {
                data_typed_tree(&row)
            } else {
                row
            };
            data_state_decode_tree(&ty, &row)
        })
        .collect()
}

/// State-only data route used by the canonical MIR evaluator. Checked MIR
/// carries the row schema and descriptor facts when the erased CtValue call
/// cannot carry them itself.
pub fn eval_data_call_in_state(
    state: &mut DataPipelineState,
    module: &str,
    method: &str,
    args: &[CtValue],
    type_args: &[Type],
    resolved_ret: Option<&Type>,
    data_plan: Option<&jet_foundation::MIR::MirDataPlan>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    // D-QUERY-RETAIN1=A: receiver Core rows carry an empty module. The
    // `Query`/`DataGroupedQuery` carrier is argument 0; callbacks are MIR
    // standalone closures invoked through the shared closure host. The
    // checked-SQL list door is the same row family with the list first.
    if module.is_empty() {
        if let Some(recv) = args.first() {
            if matches!(recv, CtValue::Struct { type_name, .. } if matches!(
                type_name.as_str(),
                "Query" | "DataGroupedQuery" | "DataTracked" | "DataWatch"
            ))
            {
                return eval_data_query_method_with(
                    state,
                    recv,
                    method,
                    &args[1..],
                    resolved_ret,
                    &mut |callback, callback_args, span| {
                        super::Methods::invoke_standalone_closure(callback, callback_args, span)
                    },
                    span,
                );
            }
            if method == "query" && matches!(recv, CtValue::List(_)) {
                return eval_data_call_in_state(
                    state,
                    "core.data",
                    "query",
                    args,
                    type_args,
                    resolved_ret,
                    data_plan,
                    span,
                );
            }
        }
    }
    if matches!(module, "core.data" | "core.data.plot")
        && matches!(
            method,
            "plot" | "inspect" | "inspect_json" | "text" | "svg" | "show" | "render"
        )
    {
        return Some(plot_static_call(
            state,
            method,
            args,
            None,
            &HashMap::new(),
            span,
        ));
    }
    // MIR may carry a checked `Result<DataSummary, DataError>` even when the
    // package-edition thread still reflects the legacy plain-value surface.
    // Keep this state route on the shared statistics kernel and honor that
    // declaration instead of erasing the outcome carrier.
    if module == "core.data" && method == "describe" {
        return Some(super::Methods::eval_data_describe(args, resolved_ret, span));
    }
    if module == "core.data"
        && matches!(
            method,
            "load" | "load_default" | "file" | "file_member" | "url" | "database" | "value"
        )
    {
        return Some(data_loader_constructor_in_state(
            state,
            method,
            args,
            type_args,
            resolved_ret,
            span,
        ));
    }
    if module == "core.data" && method == "snapshot" {
        let result = args
            .first()
            .ok_or_else(|| unsupported("`data.snapshot()` is missing loader", span))
            .and_then(|loader| {
                eval_data_loader_method_in_state(state, loader, "snapshot", &[], span)
                    .unwrap_or_else(|| Err(unsupported("data.snapshot", span)))
            });
        return Some(result);
    }
    if module == "core.data.loader" && method == "snapshot_reusable" {
        return Some(match (args.first(), args.get(1)) {
            (Some(left), Some(right)) => data_snapshot_reusable_value(left, right)
                .map(CtValue::Bool)
                .ok_or_else(|| {
                    unsupported(
                        "`data.loader.snapshot_reusable()` needs snapshot identities",
                        span,
                    )
                }),
            _ => Err(unsupported(
                "`data.loader.snapshot_reusable()`: missing argument",
                span,
            )),
        });
    }
    if module == "core.data.loader" {
        let result = args
            .first()
            .ok_or_else(|| {
                unsupported(&format!("`data.loader.{method}()`: missing loader"), span)
            })
            .and_then(|loader| {
                eval_data_loader_method_in_state(state, loader, method, &args[1..], span)
                    .unwrap_or_else(|| Err(unsupported(&format!("data.loader.{method}"), span)))
            });
        return Some(result);
    }
    if module == "core.data.stream" {
        let result = args
            .first()
            .ok_or_else(|| {
                unsupported(&format!("`data.stream.{method}()`: missing stream"), span)
            })
            .and_then(|stream| {
                eval_data_stream_method_in_state(state, stream, method, &args[1..], span)
                    .unwrap_or_else(|| Err(unsupported(&format!("data.stream.{method}"), span)))
            });
        return Some(result);
    }

    if module == "core.data" && matches!(method, "inner_join" | "left_join") {
        let [left, right, left_key, right_key] = args else {
            return Some(Err(unsupported(
                &format!("`data.{method}()` expects left, right, and two key callbacks"),
                span,
            )));
        };
        let left = match left {
            CtValue::List(rows) => rows.clone(),
            _ => {
                return Some(Err(unsupported(
                    &format!("`data.{method}()` needs a left list"),
                    span,
                )))
            }
        };
        let right = match right {
            CtValue::List(rows) => rows.clone(),
            _ => {
                return Some(Err(unsupported(
                    &format!("`data.{method}()` needs a right list"),
                    span,
                )))
            }
        };
        let right_type = data_join_right_type(resolved_ret);
        let mut call = |callback: &CtValue, args: Vec<CtValue>, span: Span| {
            super::Methods::invoke_standalone_closure(callback, args, span)
        };
        let joined = collect_join_rows(
            &left,
            &right,
            left_key,
            right_key,
            method == "left_join",
            right_type,
            &mut call,
            span,
        )
        .map(CtValue::List)
        .map(|joined| CtValue::Present(Box::new(joined)));
        return Some(joined);
    }

    if module == "core.data" && method == "pivot_sum" {
        return Some(super::Methods::eval_data_pivot_sum(
            args,
            span,
            |callback, callback_args, span| {
                super::Methods::invoke_standalone_closure(callback, callback_args, span)
            },
        ));
    }

    if module == "core.data" && method == "track" {
        if args.len() != 2 {
            return Some(Err(unsupported(
                "`data.track()` expects rows and one key callback",
                span,
            )));
        }
        let rows = match args.first() {
            Some(CtValue::List(rows)) => rows.clone(),
            Some(_) => return Some(Err(unsupported("`data.track()` needs a list", span))),
            None => return Some(Err(unsupported("`data.track()` is missing rows", span))),
        };
        let key = args[1].clone();
        return Some(build_tracked(
            state,
            rows,
            key,
            &mut |callback, callback_args, span| {
                super::Methods::invoke_standalone_closure(callback, callback_args, span)
            },
            span,
        ).map(|tracked| CtValue::Present(Box::new(tracked))));
    }
    if module == "core.data" && method == "query" {
        let rows = match args.first() {
            Some(CtValue::List(rows)) => rows.clone(),
            Some(_) => return Some(Err(unsupported("`data.query()` needs a list", span))),
            None => return Some(Err(unsupported("`data.query()` is missing rows", span))),
        };
        let declared_fields = data_plan
            .and_then(|plan| plan.logical.iter().find(|node| node.id == plan.output))
            .map(|node| node.columns.iter().map(|column| column.name.clone()).collect());
        let selected = if args.len() == 1 {
            Ok(rows)
        } else {
            let Some(CtValue::Str(sql)) = args.get(1) else {
                return Some(Err(unsupported(
                    "`data.query()` expects SQL text as argument 1",
                    span,
                )));
            };
            query_sql_rows(rows, sql, declared_fields)
        };
        return Some(match selected {
            Ok(rows) => {
                let query = store_query(
                    state,
                    rows,
                    if args.len() == 1 {
                        vec!["scan".to_string()]
                    } else {
                        vec!["scan".to_string(), "sql".to_string()]
                    },
                );
                if args.len() == 1 {
                    Ok(query)
                } else {
                    Ok(CtValue::Present(Box::new(query)))
                }
            }
            Err(error) => Ok(CtValue::failed(Box::new(error))),
        });
    }
    None
}

fn data_loader_constructor_in_state(
    state: &mut DataPipelineState,
    method: &str,
    args: &[CtValue],
    type_args: &[Type],
    resolved_ret: Option<&Type>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let operation = format!("core.data.{method}");
    let (kind, locator, member, parameters, format, authority, limits, payload, seed) =
        match method {
            "load" => {
                let locator = data_loader_string_arg(args, 0, method, span)?;
                let limits = data_limits_from_value(args.get(1));
                let format = match data_loader_format_for_locator(&locator) {
                    Ok(format) => format,
                    Err(error) => return Ok(error),
                };
                let kind = if locator.starts_with("http://")
                    || locator.starts_with("https://")
                {
                    jet_foundation::PreludeDataFlow::LoaderKind::Url
                } else {
                    jet_foundation::PreludeDataFlow::LoaderKind::File
                };
                let scope = if kind == jet_foundation::PreludeDataFlow::LoaderKind::Url {
                    "network"
                } else {
                    "local"
                };
                let authority = match data_loader_authority(scope, "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    kind,
                    locator,
                    String::new(),
                    Vec::new(),
                    format,
                    authority,
                    limits,
                    None,
                    None,
                )
            }
            "load_default" => {
                let locator = data_loader_string_arg(args, 0, method, span)?;
                let format = match data_loader_format_for_locator(&locator) {
                    Ok(format) => format,
                    Err(error) => return Ok(error),
                };
                let kind = if locator.starts_with("http://")
                    || locator.starts_with("https://")
                {
                    jet_foundation::PreludeDataFlow::LoaderKind::Url
                } else {
                    jet_foundation::PreludeDataFlow::LoaderKind::File
                };
                let scope = if kind == jet_foundation::PreludeDataFlow::LoaderKind::Url {
                    "network"
                } else {
                    "local"
                };
                let authority = match data_loader_authority(scope, "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    kind,
                    locator,
                    String::new(),
                    Vec::new(),
                    format,
                    authority,
                    data_kernel_limits_safe(),
                    None,
                    None,
                )
            }
            "file" => {
                let path = data_loader_string_arg(args, 0, method, span)?;
                let format = data_loader_string_arg(args, 1, method, span)?;
                let Some(format) = data_loader_format(&format) else {
                    return Ok(data_loader_failure(
                        "Bridge",
                        &operation,
                        "format is provider-owned; import its package",
                    ));
                };
                let authority = match data_loader_authority("local", "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    jet_foundation::PreludeDataFlow::LoaderKind::File,
                    path,
                    String::new(),
                    Vec::new(),
                    format.to_string(),
                    authority,
                    data_limits_from_value(args.get(2)),
                    None,
                    None,
                )
            }
            "file_member" => {
                let path = data_loader_string_arg(args, 0, method, span)?;
                let member = data_loader_string_arg(args, 1, method, span)?;
                let format = data_loader_string_arg(args, 2, method, span)?;
                let Some(format) = data_loader_format(&format) else {
                    return Ok(data_loader_failure(
                        "Bridge",
                        &operation,
                        "format is provider-owned; import its package",
                    ));
                };
                let authority = match data_loader_authority("local", "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    jet_foundation::PreludeDataFlow::LoaderKind::File,
                    path,
                    member,
                    Vec::new(),
                    format.to_string(),
                    authority,
                    data_limits_from_value(args.get(3)),
                    None,
                    None,
                )
            }
            "url" => {
                let url = data_loader_string_arg(args, 0, method, span)?;
                let format = data_loader_string_arg(args, 1, method, span)?;
                let authority_name = data_loader_string_arg(args, 2, method, span)?;
                let Some(format) = data_loader_format(&format) else {
                    return Ok(data_loader_failure(
                        "Bridge",
                        &operation,
                        "format is provider-owned; import its package",
                    ));
                };
                let authority = match data_loader_authority(&authority_name, "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    jet_foundation::PreludeDataFlow::LoaderKind::Url,
                    url,
                    String::new(),
                    Vec::new(),
                    format.to_string(),
                    authority,
                    data_limits_from_value(args.get(3)),
                    None,
                    None,
                )
            }
            "database" => {
                let query = data_loader_string_arg(args, 0, method, span)?;
                let parameters = args
                    .get(1)
                    .ok_or_else(|| unsupported("`data.database()` is missing parameters", span))
                    .and_then(|value| data_loader_parameters(value, span))?;
                let authority_name = data_loader_string_arg(args, 2, method, span)?;
                let authority = match data_loader_authority(&authority_name, "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    jet_foundation::PreludeDataFlow::LoaderKind::Database,
                    query,
                    String::new(),
                    parameters,
                    "json".to_string(),
                    authority,
                    data_limits_from_value(args.get(3)),
                    None,
                    None,
                )
            }
            "value" => {
                let value = args
                    .first()
                    .cloned()
                    .ok_or_else(|| unsupported("`data.value()` is missing value", span))?;
                let limits = data_limits_from_value(args.get(1));
                let text = super::JSONInterp::render_ordered_datatree(&value, false, 0);
                let authority = match data_loader_authority("local", "") {
                    Ok(authority) => authority,
                    Err(error) => return Ok(error),
                };
                (
                    jet_foundation::PreludeDataFlow::LoaderKind::Value,
                    "value".to_string(),
                    String::new(),
                    Vec::new(),
                    "json".to_string(),
                    authority,
                    limits,
                    Some(text.into_bytes()),
                    Some(value),
                )
            }
            _ => unreachable!(),
        };
    match data_loader_new(
        state,
        kind,
        locator,
        member,
        parameters,
        format,
        limits,
        authority,
        payload,
        loader_type_for_call(type_args, resolved_ret),
        seed,
    ) {
        Ok(value) => Ok(value),
        Err(error) => Ok(error),
    }
}

fn eval_data_loader_method_in_state(
    state: &mut DataPipelineState,
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(recv, CtValue::Struct { type_name, .. } if type_name == "DataLoader") {
        return None;
    }
    let id = match data_loader_state_id(state, recv, span) {
        Ok(id) => id,
        Err(error) => return Some(Err(error)),
    };
    let mut slot = state
        .loaders
        .get_mut(id - 1)
        .and_then(Option::take)
        .expect("loader identity checked");
    let result = (|| -> Result<CtValue, Diagnostic> {
        match method {
            "status" => Ok(data_kernel_status_value(
                &jet_foundation::PreludeDataFlow::status(&slot.state),
            )),
            "source_identity" => Ok(data_kernel_source_value(&slot.state.source)),
            "authority" | "authority_of" => Ok(data_kernel_authority_value(&slot.state.authority)),
            "ready" => Ok(CtValue::Bool(jet_foundation::PreludeDataFlow::ready(
                &slot.state,
            ))),
            "needs_refresh" => Ok(CtValue::Bool(
                jet_foundation::PreludeDataFlow::needs_refresh(&slot.state),
            )),
            "snapshot_reusable" => {
                let snapshot = args.first().ok_or_else(|| {
                    unsupported("`data.loader.snapshot_reusable()` needs a snapshot", span)
                })?;
                data_snapshot_reusable_value(recv, snapshot)
                    .map(CtValue::Bool)
                    .ok_or_else(|| unsupported("`snapshot_reusable()` needs a DataSnapshot", span))
            }
            "bind" => {
                let payload = match args.first() {
                    Some(CtValue::Bytes(bytes)) => bytes.clone(),
                    Some(CtValue::List(values)) => values
                        .iter()
                        .map(|value| match value {
                            CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                            _ => Err(unsupported("`DataLoader.bind()` needs byte values", span)),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(unsupported(
                            "`DataLoader.bind()` needs byte values",
                            span,
                        ))
                    }
                };
                match jet_foundation::PreludeDataFlow::bind(&mut slot.state, payload) {
                    Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                    Err(error) => {
                        jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                        Ok(data_loader_kernel_failure(error))
                    }
                }
            }
            "bind_text" => {
                let payload = match args.first() {
                    Some(CtValue::Str(value)) => value.as_bytes().to_vec(),
                    _ => return Err(unsupported("`DataLoader.bind_text()` needs String", span)),
                };
                match jet_foundation::PreludeDataFlow::bind(&mut slot.state, payload) {
                    Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                    Err(error) => {
                        jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                        Ok(data_loader_kernel_failure(error))
                    }
                }
            }
            "cancel" => {
                jet_foundation::PreludeDataFlow::cancel(&mut slot.state);
                Ok(CtValue::Unit)
            }
            "offline" => {
                let enabled = match args.first() {
                    Some(CtValue::Bool(value)) => *value,
                    _ => return Err(unsupported("`DataLoader.offline()` needs Bool", span)),
                };
                jet_foundation::PreludeDataFlow::set_offline(&mut slot.state, enabled);
                Ok(CtValue::Unit)
            }
            "invalidate" => {
                let cause = match args.first() {
                    Some(CtValue::Enum {
                        type_name,
                        variant,
                        ..
                    }) if type_name == "DataInvalidationCause" => match variant.as_str() {
                        "None" => jet_foundation::PreludeDataFlow::InvalidationCause::None,
                        "Loader" => jet_foundation::PreludeDataFlow::InvalidationCause::Loader,
                        "Input" => jet_foundation::PreludeDataFlow::InvalidationCause::Input,
                        "ArchiveMember" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember
                        }
                        "Parameters" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::Parameters
                        }
                        "Credential" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::Credential
                        }
                        "Capability" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::Capability
                        }
                        "Manual" => jet_foundation::PreludeDataFlow::InvalidationCause::Manual,
                        _ => {
                            return Err(unsupported(
                                "`DataLoader.invalidate()` needs DataInvalidationCause",
                                span,
                            ))
                        }
                    },
                    _ => {
                        return Err(unsupported(
                            "`DataLoader.invalidate()` needs DataInvalidationCause",
                            span,
                        ))
                    }
                };
                jet_foundation::PreludeDataFlow::invalidate(&mut slot.state, cause);
                Ok(CtValue::Unit)
            }
            "stream" => {
                if slot.state.cancelled {
                    let error = jet_foundation::PreludeDataFlow::KernelError {
                        kind: jet_foundation::PreludeDataFlow::ErrorKind::State,
                        operation: "data.loader.stream".to_string(),
                        reason: "loader was cancelled".to_string(),
                    };
                    jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                    return Ok(data_loader_kernel_failure(error));
                }
                if let Err(error) =
                    jet_foundation::PreludeDataFlow::validate_loader(&slot.state)
                {
                    jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                    return Ok(data_loader_kernel_failure(error));
                }
                if let Err(error) =
                    jet_foundation::PreludeDataFlow::validate_limits(&slot.state.limits)
                {
                    jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                    return Ok(data_loader_kernel_failure(error));
                }
                match data_stream_rows_in_state(&slot) {
                    Ok(rows) => {
                        let size = slot
                            .state
                            .payload
                            .as_ref()
                            .map_or(0, Vec::len);
                        let bytes = i64::try_from(size).unwrap_or(i64::MAX);
                        slot.state.status.buffered_bytes = bytes;
                        slot.state.status.backpressure =
                            bytes > slot.state.limits.buffer_bytes;
                        Ok(CtValue::Present(Box::new(store_data_stream(
                            state,
                            ComptimeStream {
                                rows,
                                cursor: 0,
                                cancelled: false,
                                row_ty: slot.decode_ty.clone(),
                            },
                        ))))
                    }
                    Err(error) => {
                        data_loader_fail_value(&mut slot, &error, "data.loader.stream");
                        Ok(error)
                    }
                }
            }
            "snapshot" => match data_snapshot_dynamic_in_state(&mut slot) {
                Ok(value) => Ok(value),
                Err(error) => Ok(error),
            },
            _ => Err(unsupported(
                &format!("`DataLoader.{method}()` is not a registered operation"),
                span,
            )),
        }
    })();
    state.loaders[id - 1] = Some(slot);
    Some(result)
}

fn eval_data_stream_method_in_state(
    state: &mut DataPipelineState,
    recv: &CtValue,
    method: &str,
    _args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(recv, CtValue::Struct { type_name, .. } if type_name == "DataStream") {
        return None;
    }
    let id = match data_stream_state_id(state, recv, span) {
        Ok(id) => id,
        Err(error) => return Some(Err(error)),
    };
    let mut stream = state
        .streams
        .get_mut(id - 1)
        .and_then(Option::take)
        .expect("stream identity checked");
    let result = match method {
        "next" => Ok(data_stream_next(&mut stream)),
        "collect" => {
            if stream.cancelled {
                Ok(data_loader_failure(
                    "State",
                    "data.stream.collect",
                    "stream was cancelled",
                ))
            } else {
                let rows = stream.rows[stream.cursor..].to_vec();
                stream.cursor = stream.rows.len();
                Ok(CtValue::Present(Box::new(CtValue::List(rows))))
            }
        }
        "cancel" => {
            stream.cancelled = true;
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(
            &format!("`DataStream.{method}()` is not a registered operation"),
            span,
        )),
    };
    state.streams[id - 1] = Some(stream);
    Some(result)
}

/// Static Core data-loader/stream projection. Local CSV, JSON, and JSONL use
/// the Foundation carrier; provider-owned sources retain structured Bridge.
pub(super) fn eval_data_loader_call(
    interp: &mut super::Interpreter::Interp<'_>,
    module: &str,
    method: &str,
    args: &[CtValue],
    type_args: &[Type],
    resolved_ret: Option<&Type>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let operation = format!("{module}.{method}");
    match (module, method) {
        ("core.data", "load" | "load_default" | "file" | "file_member" | "url" | "database" | "value") => {
            let result = (|| -> Result<CtValue, Diagnostic> {
                let (kind, locator, member, parameters, format, authority, limits, payload, seed) =
                    match method {
                        "load" => {
                            let locator = data_loader_string_arg(args, 0, method, span)?;
                            let limits = data_limits_from_value(args.get(1));
                            let format = match data_loader_format_for_locator(&locator) {
                                Ok(format) => format,
                                Err(error) => return Ok(error),
                            };
                            let kind = if locator.starts_with("http://")
                                || locator.starts_with("https://")
                            {
                                jet_foundation::PreludeDataFlow::LoaderKind::Url
                            } else {
                                jet_foundation::PreludeDataFlow::LoaderKind::File
                            };
                            let scope = if kind
                                == jet_foundation::PreludeDataFlow::LoaderKind::Url
                            {
                                "network"
                            } else {
                                "local"
                            };
                            let authority = match data_loader_authority(scope, "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (
                                kind,
                                locator,
                                String::new(),
                                Vec::new(),
                                format,
                                authority,
                                limits,
                                None,
                                None,
                            )
                        }
                        "load_default" => {
                            let locator = data_loader_string_arg(args, 0, method, span)?;
                            let format = match data_loader_format_for_locator(&locator) {
                                Ok(format) => format,
                                Err(error) => return Ok(error),
                            };
                            let kind = if locator.starts_with("http://")
                                || locator.starts_with("https://")
                            {
                                jet_foundation::PreludeDataFlow::LoaderKind::Url
                            } else {
                                jet_foundation::PreludeDataFlow::LoaderKind::File
                            };
                            let scope = if kind
                                == jet_foundation::PreludeDataFlow::LoaderKind::Url
                            {
                                "network"
                            } else {
                                "local"
                            };
                            let authority = match data_loader_authority(scope, "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (kind, locator, String::new(), Vec::new(), format, authority, data_kernel_limits_safe(), None, None)
                        }
                        "file" => {
                            let path = data_loader_string_arg(args, 0, method, span)?;
                            let format = data_loader_string_arg(args, 1, method, span)?;
                            let Some(format) = data_loader_format(&format) else {
                                return Ok(data_loader_failure(
                                    "Bridge",
                                    &operation,
                                    "format is provider-owned; import its package",
                                ));
                            };
                            let authority = match data_loader_authority("local", "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (jet_foundation::PreludeDataFlow::LoaderKind::File, path, String::new(), Vec::new(), format.to_string(), authority, data_limits_from_value(args.get(2)), None, None)
                        }
                        "file_member" => {
                            let path = data_loader_string_arg(args, 0, method, span)?;
                            let member = data_loader_string_arg(args, 1, method, span)?;
                            let format = data_loader_string_arg(args, 2, method, span)?;
                            let Some(format) = data_loader_format(&format) else {
                                return Ok(data_loader_failure(
                                    "Bridge",
                                    &operation,
                                    "format is provider-owned; import its package",
                                ));
                            };
                            let authority = match data_loader_authority("local", "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (jet_foundation::PreludeDataFlow::LoaderKind::File, path, member, Vec::new(), format.to_string(), authority, data_limits_from_value(args.get(3)), None, None)
                        }
                        "url" => {
                            let url = data_loader_string_arg(args, 0, method, span)?;
                            let format = data_loader_string_arg(args, 1, method, span)?;
                            let authority_name = data_loader_string_arg(args, 2, method, span)?;
                            let Some(format) = data_loader_format(&format) else {
                                return Ok(data_loader_failure(
                                    "Bridge",
                                    &operation,
                                    "format is provider-owned; import its package",
                                ));
                            };
                            let authority = match data_loader_authority(&authority_name, "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (jet_foundation::PreludeDataFlow::LoaderKind::Url, url, String::new(), Vec::new(), format.to_string(), authority, data_limits_from_value(args.get(3)), None, None)
                        }
                        "database" => {
                            let query = data_loader_string_arg(args, 0, method, span)?;
                            let parameters = args
                                .get(1)
                                .ok_or_else(|| unsupported("`data.database()` is missing parameters", span))
                                .and_then(|value| data_loader_parameters(value, span))?;
                            let authority_name = data_loader_string_arg(args, 2, method, span)?;
                            let authority = match data_loader_authority(&authority_name, "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (jet_foundation::PreludeDataFlow::LoaderKind::Database, query, String::new(), parameters, "json".to_string(), authority, data_limits_from_value(args.get(3)), None, None)
                        }
                        "value" => {
                            let value = args
                                .first()
                                .cloned()
                                .ok_or_else(|| unsupported("`data.value()` is missing value", span))?;
                            let limits = data_limits_from_value(args.get(1));
                            let text = super::JSONInterp::render_ordered_datatree(&value, false, 0);
                            let authority = match data_loader_authority("local", "") {
                                Ok(authority) => authority,
                                Err(error) => return Ok(error),
                            };
                            (
                                jet_foundation::PreludeDataFlow::LoaderKind::Value,
                                "value".to_string(),
                                String::new(),
                                Vec::new(),
                                "json".to_string(),
                                authority,
                                limits,
                                Some(text.into_bytes()),
                                Some(value),
                            )
                        }
                        _ => unreachable!(),
                    };
                match data_loader_new(
                    &mut interp.data_pipeline,
                    kind,
                    locator,
                    member,
                    parameters,
                    format,
                    limits,
                    authority,
                    payload,
                    loader_type_for_call(type_args, resolved_ret),
                    seed,
                ) {
                    Ok(value) => Ok(value),
                    Err(error) => Ok(error),
                }
            })();
            Some(result)
        }
        ("core.data", "snapshot") => {
            let result = args
                .first()
                .ok_or_else(|| unsupported("`data.snapshot()` is missing loader", span))
                .and_then(|loader| {
                    eval_data_loader_method(interp, loader, "snapshot", &[], span)
                        .unwrap_or_else(|| Err(unsupported("data.snapshot", span)))
                });
            Some(result)
        }
        ("core.data.loader", "snapshot_reusable") => {
            let result = match (args.first(), args.get(1)) {
                (Some(left), Some(right)) => data_snapshot_reusable_value(left, right)
                    .map(CtValue::Bool)
                    .ok_or_else(|| {
                        unsupported(
                            "`data.loader.snapshot_reusable()` needs snapshot identities",
                            span,
                        )
                    }),
                _ => Err(unsupported(
                    "`data.loader.snapshot_reusable()`: missing argument",
                    span,
                )),
            };
            Some(result)
        }
        ("core.data.loader", _) => {
            let result = args
                .first()
                .ok_or_else(|| {
                    unsupported(&format!("`data.loader.{method}()`: missing loader"), span)
                })
                .and_then(|loader| {
                    eval_data_loader_method(interp, loader, method, &args[1..], span)
                        .unwrap_or_else(|| Err(unsupported(&operation, span)))
                });
            Some(result)
        }
        ("core.data.stream", _) => {
            let result = args
                .first()
                .ok_or_else(|| unsupported(&format!("`data.stream.{method}()`: missing stream"), span))
                .and_then(|stream| {
                    eval_data_stream_method(interp, stream, method, &args[1..], span)
                        .unwrap_or_else(|| Err(unsupported(&operation, span)))
                });
            Some(result)
        }
        _ => None,
    }
}

pub(super) fn eval_data_loader_method(
    interp: &mut super::Interpreter::Interp<'_>,
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(recv, CtValue::Struct { type_name, .. } if type_name == "DataLoader") {
        return None;
    }
    let id = match data_loader_state_id(&interp.data_pipeline, recv, span) {
        Ok(id) => id,
        Err(error) => return Some(Err(error)),
    };
    let mut slot = interp
        .data_pipeline
        .loaders
        .get_mut(id - 1)
        .and_then(Option::take)
        .expect("loader identity checked");
    let result = (|| -> Result<CtValue, Diagnostic> {
        match method {
            "status" => Ok(data_kernel_status_value(
                &jet_foundation::PreludeDataFlow::status(&slot.state),
            )),
            "source_identity" => Ok(data_kernel_source_value(&slot.state.source)),
            "authority" | "authority_of" => Ok(data_kernel_authority_value(&slot.state.authority)),
            "ready" => Ok(CtValue::Bool(jet_foundation::PreludeDataFlow::ready(
                &slot.state,
            ))),
            "needs_refresh" => Ok(CtValue::Bool(
                jet_foundation::PreludeDataFlow::needs_refresh(&slot.state),
            )),
            "snapshot_reusable" => {
                let snapshot = args.first().ok_or_else(|| {
                    unsupported("`data.loader.snapshot_reusable()` needs a snapshot", span)
                })?;
                data_snapshot_reusable_value(recv, snapshot)
                    .map(CtValue::Bool)
                    .ok_or_else(|| unsupported("`snapshot_reusable()` needs a DataSnapshot", span))
            }
            "bind" => {
                let payload = match args.first() {
                    Some(CtValue::Bytes(bytes)) => bytes.clone(),
                    Some(CtValue::List(values)) => values
                        .iter()
                        .map(|value| match value {
                            CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                            _ => Err(unsupported("`DataLoader.bind()` needs byte values", span)),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(unsupported(
                            "`DataLoader.bind()` needs byte values",
                            span,
                        ))
                    }
                };
                match jet_foundation::PreludeDataFlow::bind(&mut slot.state, payload) {
                    Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                    Err(error) => {
                        jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                        Ok(data_loader_kernel_failure(error))
                    }
                }
            }
            "bind_text" => {
                let payload = match args.first() {
                    Some(CtValue::Str(value)) => value.as_bytes().to_vec(),
                    _ => return Err(unsupported("`DataLoader.bind_text()` needs String", span)),
                };
                match jet_foundation::PreludeDataFlow::bind(&mut slot.state, payload) {
                    Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                    Err(error) => {
                        jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                        Ok(data_loader_kernel_failure(error))
                    }
                }
            }
            "cancel" => {
                jet_foundation::PreludeDataFlow::cancel(&mut slot.state);
                Ok(CtValue::Unit)
            }
            "offline" => {
                let enabled = match args.first() {
                    Some(CtValue::Bool(value)) => *value,
                    _ => return Err(unsupported("`DataLoader.offline()` needs Bool", span)),
                };
                jet_foundation::PreludeDataFlow::set_offline(&mut slot.state, enabled);
                Ok(CtValue::Unit)
            }
            "invalidate" => {
                let cause = match args.first() {
                    Some(CtValue::Enum {
                        type_name,
                        variant,
                        ..
                    }) if type_name == "DataInvalidationCause" => match variant.as_str() {
                        "None" => jet_foundation::PreludeDataFlow::InvalidationCause::None,
                        "Loader" => jet_foundation::PreludeDataFlow::InvalidationCause::Loader,
                        "Input" => jet_foundation::PreludeDataFlow::InvalidationCause::Input,
                        "ArchiveMember" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember
                        }
                        "Parameters" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::Parameters
                        }
                        "Credential" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::Credential
                        }
                        "Capability" => {
                            jet_foundation::PreludeDataFlow::InvalidationCause::Capability
                        }
                        "Manual" => jet_foundation::PreludeDataFlow::InvalidationCause::Manual,
                        _ => {
                            return Err(unsupported(
                                "`DataLoader.invalidate()` needs DataInvalidationCause",
                                span,
                            ))
                        }
                    },
                    _ => {
                        return Err(unsupported(
                            "`DataLoader.invalidate()` needs DataInvalidationCause",
                            span,
                        ))
                    }
                };
                jet_foundation::PreludeDataFlow::invalidate(&mut slot.state, cause);
                Ok(CtValue::Unit)
            }
            "stream" => {
                if slot.state.cancelled {
                    let error = jet_foundation::PreludeDataFlow::KernelError {
                        kind: jet_foundation::PreludeDataFlow::ErrorKind::State,
                        operation: "data.loader.stream".to_string(),
                        reason: "loader was cancelled".to_string(),
                    };
                    jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                    return Ok(data_loader_kernel_failure(error));
                }
                if let Err(error) =
                    jet_foundation::PreludeDataFlow::validate_loader(&slot.state)
                {
                    jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                    return Ok(data_loader_kernel_failure(error));
                }
                if let Err(error) =
                    jet_foundation::PreludeDataFlow::validate_limits(&slot.state.limits)
                {
                    jet_foundation::PreludeDataFlow::fail(&mut slot.state, &error);
                    return Ok(data_loader_kernel_failure(error));
                }
                match data_stream_rows(&slot, interp, span) {
                    Ok(rows) => {
                        let size = slot
                            .state
                            .payload
                            .as_ref()
                            .map_or(0, Vec::len);
                        let bytes = i64::try_from(size).unwrap_or(i64::MAX);
                        slot.state.status.buffered_bytes = bytes;
                        slot.state.status.backpressure =
                            bytes > slot.state.limits.buffer_bytes;
                        Ok(CtValue::Present(Box::new(store_data_stream(
                            &mut interp.data_pipeline,
                            ComptimeStream {
                                rows,
                                cursor: 0,
                                cancelled: false,
                                row_ty: slot.decode_ty.clone(),
                            },
                        ))))
                    }
                    Err(error) => {
                        data_loader_fail_value(&mut slot, &error, "data.loader.stream");
                        Ok(error)
                    }
                }
            }
            "snapshot" => match data_snapshot_dynamic(&mut slot, interp, span) {
                Ok(value) => Ok(value),
                Err(error) => Ok(error),
            },
            _ => Err(unsupported(
                &format!("`DataLoader.{method}()` is not a registered operation"),
                span,
            )),
        }
    })();
    interp.data_pipeline.loaders[id - 1] = Some(slot);
    Some(result)
}

pub(super) fn eval_data_stream_method(
    interp: &mut super::Interpreter::Interp<'_>,
    recv: &CtValue,
    method: &str,
    _args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(recv, CtValue::Struct { type_name, .. } if type_name == "DataStream") {
        return None;
    }
    let id = match data_stream_state_id(&interp.data_pipeline, recv, span) {
        Ok(id) => id,
        Err(error) => return Some(Err(error)),
    };
    let mut stream = interp
        .data_pipeline
        .streams
        .get_mut(id - 1)
        .and_then(Option::take)
        .expect("stream identity checked");
    let result = match method {
        "next" => Ok(data_stream_next(&mut stream)),
        "collect" => {
            if stream.cancelled {
                Ok(data_loader_failure(
                    "State",
                    "data.stream.collect",
                    "stream was cancelled",
                ))
            } else {
                let rows = stream.rows[stream.cursor..].to_vec();
                stream.cursor = stream.rows.len();
                Ok(CtValue::Present(Box::new(CtValue::List(rows))))
            }
        }
        "cancel" => {
            stream.cancelled = true;
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(
            &format!("`DataStream.{method}()` is not a registered operation"),
            span,
        )),
    };
    interp.data_pipeline.streams[id - 1] = Some(stream);
    Some(result)
}

fn ct_value_type_name(v: &CtValue) -> String {
    match v {
        CtValue::Int(_) => "Int".to_string(),
        CtValue::Float(value) => value.jet_type().name(),
        CtValue::Bool(_) => "Bool".to_string(),
        CtValue::Char(_) => "Char".to_string(),
        CtValue::Str(_) => "String".to_string(),
        CtValue::BigInt(_) => "Int".to_string(),
        CtValue::Bytes(_) => "[U8]".to_string(),
        CtValue::List(_) => "List".to_string(),
        CtValue::Map(_) => "Map".to_string(),
        CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => type_name.clone(),
        CtValue::Present(inner) => format!("{}?", ct_value_type_name(inner)),
        CtValue::Failed(CtReport::Clean(ty)) => format!("{}?", ty.name()),
        CtValue::Failed(CtReport::Told(_)) => "Result".to_string(),
        CtValue::Unit => "()".to_string(),
        CtValue::Closure(_) => "Fn".to_string(),
    }
}
// The typed plot handle is intentionally opaque to CtValue. Its state remains
// one shared `JetDataPlot` plan, while the interpreter only carries a stable
// slot identity through chained receiver calls.

fn plot_handle(id: usize) -> CtValue {
    ct_struct(
        "JetDataPlot",
        vec![("id", CtValue::Int(i64::try_from(id).unwrap_or(i64::MAX)))],
    )
}

fn plot_handle_id(
    state: &DataPipelineState,
    value: &CtValue,
    span: Span,
) -> Result<usize, Diagnostic> {
    let Some(CtValue::Int(id)) = struct_field(value, "JetDataPlot", "id") else {
        return Err(unsupported("`JetDataPlot` handle is malformed", span));
    };
    if *id <= 0 {
        return Err(unsupported("`JetDataPlot` handle is invalid", span));
    }
    let id = usize::try_from(*id)
        .map_err(|_| unsupported("`JetDataPlot` handle is invalid", span))?;
    state
        .plots
        .get(id - 1)
        .and_then(Option::as_ref)
        .map(|_| id)
        .ok_or_else(|| unsupported("`JetDataPlot` handle is stale", span))
}

fn store_plot(
    state: &mut DataPipelineState,
    plot: data_plot_rt::JetDataPlot<CtValue>,
) -> CtValue {
    state.plots.push(Some(plot));
    plot_handle(state.plots.len())
}

fn replace_plot(
    state: &mut DataPipelineState,
    handle: &CtValue,
    plot: data_plot_rt::JetDataPlot<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let id = plot_handle_id(state, handle, span)?;
    let slot = state
        .plots
        .get_mut(id - 1)
        .and_then(Option::as_mut)
        .ok_or_else(|| unsupported("`JetDataPlot` handle is stale", span))?;
    *slot = plot;
    Ok(handle.clone())
}

fn clone_plot(
    state: &DataPipelineState,
    handle: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlot<CtValue>, Diagnostic> {
    let id = plot_handle_id(state, handle, span)?;
    state
        .plots
        .get(id - 1)
        .and_then(Option::as_ref)
        .cloned()
        .ok_or_else(|| unsupported("`JetDataPlot` handle is stale", span))
}

fn plot_value(value: &CtValue) -> data_plot_rt::JetDataPlotValue {
    match value {
        CtValue::Int(value) => data_plot_rt::JetDataPlotValue::Integer(*value),
        CtValue::BigInt(value) => value
            .to_string_rep()
            .parse::<i64>()
            .map(data_plot_rt::JetDataPlotValue::Integer)
            .unwrap_or_else(|_| data_plot_rt::JetDataPlotValue::Text(value.to_string_rep())),
        CtValue::Float(value) => data_plot_rt::JetDataPlotValue::Number(value.as_f64()),
        CtValue::Bool(value) => data_plot_rt::JetDataPlotValue::Boolean(*value),
        CtValue::Char(value) => data_plot_rt::JetDataPlotValue::Text(value.to_string()),
        CtValue::Str(value) => data_plot_rt::JetDataPlotValue::Text(value.clone()),
        CtValue::Present(value) => plot_value(value),
        CtValue::Failed(_) => data_plot_rt::JetDataPlotValue::Null,
        CtValue::Unit => data_plot_rt::JetDataPlotValue::Null,
        _ => data_plot_rt::JetDataPlotValue::Text(query_cell_text(value)),
    }
}

fn plot_variant<'a>(
    value: &'a CtValue,
    expected: &str,
    span: Span,
) -> Result<&'a str, Diagnostic> {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            ..
        } if type_name.is_empty() || type_name == expected || type_name.ends_with(expected) => {
            Ok(variant
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(variant))
        }
        _ => Err(unsupported(
            &format!("expected `{expected}` option"),
            span,
        )),
    }
}

fn plot_enum_arg<'a>(
    value: &'a CtValue,
    name: &str,
    index: usize,
) -> Option<&'a CtValue> {
    let CtValue::Enum { args, .. } = value else {
        return None;
    };
    args.iter()
        .find_map(|(label, value)| (label.as_deref() == Some(name)).then_some(value))
        .or_else(|| args.get(index).map(|(_, value)| value))
}

fn plot_string(value: &CtValue, what: &str, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported(&format!("{what} must be String"), span)),
    }
}

fn plot_bool(value: &CtValue, what: &str, span: Span) -> Result<bool, Diagnostic> {
    match value {
        CtValue::Bool(value) => Ok(*value),
        _ => Err(unsupported(&format!("{what} must be Bool"), span)),
    }
}

fn plot_int(value: &CtValue, what: &str, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(*value),
        _ => Err(unsupported(&format!("{what} must be Int"), span)),
    }
}

fn plot_float(value: &CtValue, what: &str, span: Span) -> Result<f64, Diagnostic> {
    match value {
        CtValue::Float(value) => Ok(value.as_f64()),
        CtValue::Int(value) => Ok(*value as f64),
        _ => Err(unsupported(&format!("{what} must be Float"), span)),
    }
}

fn plot_field(value: &CtValue, span: Span) -> Result<data_plot_rt::JetDataPlotField, Diagnostic> {
    if let Some(field) = struct_field(value, "JetDataPlotColumn", "field") {
        return plot_field(field, span);
    }
    let id = struct_field(value, "JetDataPlotField", "id")
        .and_then(|value| matches!(value, CtValue::Str(_)).then(|| query_cell_text(value)));
    let name = struct_field(value, "JetDataPlotField", "name")
        .ok_or_else(|| unsupported("plot column is missing field.name", span))
        .and_then(|value| plot_string(value, "plot field name", span))?;
    let type_name = struct_field(value, "JetDataPlotField", "type_name")
        .ok_or_else(|| unsupported("plot column is missing field.type_name", span))
        .and_then(|value| plot_string(value, "plot field type_name", span))?;
    Ok(match id {
        Some(id) => data_plot_rt::JetDataPlotField::with_id(id, name, type_name),
        None => data_plot_rt::JetDataPlotField::new(name, type_name),
    })
}

fn plot_column(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotColumn<CtValue>, Diagnostic> {
    let field = plot_field(value, span)?;
    let name = field.name.clone();
    Ok(data_plot_rt::jet_data_plot_column(field, move |row| {
        query_row_field(row, &name)
            .map(plot_value)
            .unwrap_or(data_plot_rt::JetDataPlotValue::Null)
    }))
}

fn plot_struct_field<'a>(value: &'a CtValue, name: &str) -> Option<&'a CtValue> {
    match value {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value)),
        _ => None,
    }
}

fn plot_optional_bool(
    value: &CtValue,
    name: &str,
    default: bool,
    span: Span,
) -> Result<bool, Diagnostic> {
    plot_struct_field(value, name)
        .map(|value| plot_bool(value, &format!("plot {name}"), span))
        .unwrap_or(Ok(default))
}

fn plot_optional_int(
    value: &CtValue,
    name: &str,
    default: i64,
    span: Span,
) -> Result<i64, Diagnostic> {
    plot_struct_field(value, name)
        .map(|value| plot_int(value, &format!("plot {name}"), span))
        .unwrap_or(Ok(default))
}

fn plot_optional_float(
    value: &CtValue,
    name: &str,
    default: f64,
    span: Span,
) -> Result<f64, Diagnostic> {
    plot_struct_field(value, name)
        .map(|value| plot_float(value, &format!("plot {name}"), span))
        .unwrap_or(Ok(default))
}

fn plot_mark(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotMark, Diagnostic> {
    match plot_variant(value, "JetDataPlotMark", span)? {
        "Line" => Ok(data_plot_rt::JetDataPlotMark::Line),
        "Bar" => Ok(data_plot_rt::JetDataPlotMark::Bar),
        "Point" => Ok(data_plot_rt::JetDataPlotMark::Point),
        _ => Err(unsupported("unknown JetDataPlotMark variant", span)),
    }
}

fn plot_channel(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotChannel, Diagnostic> {
    match plot_variant(value, "JetDataPlotChannel", span)? {
        "X" => Ok(data_plot_rt::JetDataPlotChannel::X),
        "Y" => Ok(data_plot_rt::JetDataPlotChannel::Y),
        "Color" => Ok(data_plot_rt::JetDataPlotChannel::Color),
        "Size" => Ok(data_plot_rt::JetDataPlotChannel::Size),
        "Text" => Ok(data_plot_rt::JetDataPlotChannel::Text),
        "Detail" => Ok(data_plot_rt::JetDataPlotChannel::Detail),
        _ => Err(unsupported("unknown JetDataPlotChannel variant", span)),
    }
}

fn plot_aggregate(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotAggregate, Diagnostic> {
    match plot_variant(value, "JetDataPlotAggregate", span)? {
        "None" => Ok(data_plot_rt::JetDataPlotAggregate::None),
        "Count" => Ok(data_plot_rt::JetDataPlotAggregate::Count),
        "Sum" => Ok(data_plot_rt::JetDataPlotAggregate::Sum),
        "Mean" => Ok(data_plot_rt::JetDataPlotAggregate::Mean),
        "Min" => Ok(data_plot_rt::JetDataPlotAggregate::Min),
        "Max" => Ok(data_plot_rt::JetDataPlotAggregate::Max),
        _ => Err(unsupported("unknown JetDataPlotAggregate variant", span)),
    }
}

fn plot_filter_op(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotFilterOp, Diagnostic> {
    match plot_variant(value, "JetDataPlotFilterOp", span)? {
        "Equal" => Ok(data_plot_rt::JetDataPlotFilterOp::Equal),
        "NotEqual" => Ok(data_plot_rt::JetDataPlotFilterOp::NotEqual),
        "Less" => Ok(data_plot_rt::JetDataPlotFilterOp::Less),
        "LessEqual" => Ok(data_plot_rt::JetDataPlotFilterOp::LessEqual),
        "Greater" => Ok(data_plot_rt::JetDataPlotFilterOp::Greater),
        "GreaterEqual" => Ok(data_plot_rt::JetDataPlotFilterOp::GreaterEqual),
        _ => Err(unsupported("unknown JetDataPlotFilterOp variant", span)),
    }
}

fn plot_scale_kind(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotScaleKind, Diagnostic> {
    match plot_variant(value, "JetDataPlotScaleKind", span)? {
        "Linear" => Ok(data_plot_rt::JetDataPlotScaleKind::Linear),
        "Log" => Ok(data_plot_rt::JetDataPlotScaleKind::Log),
        "Band" => Ok(data_plot_rt::JetDataPlotScaleKind::Band),
        "Point" => Ok(data_plot_rt::JetDataPlotScaleKind::Point),
        _ => Err(unsupported("unknown JetDataPlotScaleKind variant", span)),
    }
}

fn plot_domain(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotDomain, Diagnostic> {
    match plot_variant(value, "JetDataPlotDomain", span)? {
        "Auto" => Ok(data_plot_rt::JetDataPlotDomain::Auto),
        "Numeric" => {
            let min = plot_enum_arg(value, "min", 0)
                .ok_or_else(|| unsupported("numeric plot domain is missing min", span))
                .and_then(|value| plot_float(value, "plot domain min", span))?;
            let max = plot_enum_arg(value, "max", 1)
                .ok_or_else(|| unsupported("numeric plot domain is missing max", span))
                .and_then(|value| plot_float(value, "plot domain max", span))?;
            Ok(data_plot_rt::JetDataPlotDomain::Numeric { min, max })
        }
        "Categories" => {
            let values = plot_enum_arg(value, "values", 0)
                .ok_or_else(|| unsupported("category plot domain is missing values", span))?;
            let CtValue::List(values) = values else {
                return Err(unsupported("plot domain categories must be a list", span));
            };
            Ok(data_plot_rt::JetDataPlotDomain::Categories(
                values
                    .iter()
                    .map(|value| plot_string(value, "plot domain category", span))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        _ => Err(unsupported("unknown JetDataPlotDomain variant", span)),
    }
}

fn plot_encoding(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotEncoding, Diagnostic> {
    let channel = plot_struct_field(value, "channel")
        .ok_or_else(|| unsupported("plot encoding is missing channel", span))
        .and_then(|value| plot_channel(value, span))?;
    let field = plot_struct_field(value, "field")
        .ok_or_else(|| unsupported("plot encoding is missing field", span))
        .and_then(|value| plot_field(value, span))?;
    let aggregate = plot_struct_field(value, "aggregate")
        .ok_or_else(|| unsupported("plot encoding is missing aggregate", span))
        .and_then(|value| plot_aggregate(value, span))?;
    Ok(data_plot_rt::JetDataPlotEncoding::new(
        channel, field, aggregate,
    ))
}

fn plot_transform(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotTransform, Diagnostic> {
    match plot_variant(value, "JetDataPlotTransform", span)? {
        "Filter" => {
            let field = plot_enum_arg(value, "field", 0)
                .ok_or_else(|| unsupported("plot filter is missing field", span))
                .and_then(|value| plot_field(value, span))?;
            let op = plot_enum_arg(value, "op", 1)
                .ok_or_else(|| unsupported("plot filter is missing op", span))
                .and_then(|value| plot_filter_op(value, span))?;
            let filter_value = plot_enum_arg(value, "value", 2)
                .ok_or_else(|| unsupported("plot filter is missing value", span))
                .map(plot_value)?;
            Ok(data_plot_rt::JetDataPlotTransform::Filter {
                field,
                op,
                value: filter_value,
            })
        }
        "Sort" => {
            let field = plot_enum_arg(value, "field", 0)
                .ok_or_else(|| unsupported("plot sort is missing field", span))
                .and_then(|value| plot_field(value, span))?;
            let descending = plot_enum_arg(value, "descending", 1)
                .map(|value| plot_bool(value, "plot sort descending", span))
                .transpose()?
                .unwrap_or(false);
            Ok(data_plot_rt::JetDataPlotTransform::Sort {
                field,
                descending,
            })
        }
        "Bin" => {
            let field = plot_enum_arg(value, "field", 0)
                .ok_or_else(|| unsupported("plot bin is missing field", span))
                .and_then(|value| plot_field(value, span))?;
            let step = plot_enum_arg(value, "step", 1)
                .ok_or_else(|| unsupported("plot bin is missing step", span))
                .and_then(|value| plot_float(value, "plot bin step", span))?;
            Ok(data_plot_rt::JetDataPlotTransform::Bin { field, step })
        }
        "Aggregate" => {
            let group_by = plot_enum_arg(value, "group_by", 0)
                .ok_or_else(|| unsupported("plot aggregate is missing group_by", span))?;
            let CtValue::List(group_by) = group_by else {
                return Err(unsupported("plot aggregate group_by must be a list", span));
            };
            let group_by = group_by
                .iter()
                .map(|value| plot_field(value, span))
                .collect::<Result<Vec<_>, _>>()?;
            let field = plot_enum_arg(value, "field", 1)
                .ok_or_else(|| unsupported("plot aggregate is missing field", span))
                .and_then(|value| plot_field(value, span))?;
            let aggregate = plot_enum_arg(value, "aggregate", 2)
                .ok_or_else(|| unsupported("plot aggregate is missing aggregate", span))
                .and_then(|value| plot_aggregate(value, span))?;
            Ok(data_plot_rt::JetDataPlotTransform::Aggregate {
                group_by,
                field,
                aggregate,
            })
        }
        _ => Err(unsupported("unknown JetDataPlotTransform variant", span)),
    }
}

fn plot_scale(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotScale, Diagnostic> {
    let channel = plot_struct_field(value, "channel")
        .ok_or_else(|| unsupported("plot scale is missing channel", span))
        .and_then(|value| plot_channel(value, span))?;
    let kind = plot_struct_field(value, "kind")
        .ok_or_else(|| unsupported("plot scale is missing kind", span))
        .and_then(|value| plot_scale_kind(value, span))?;
    let domain = plot_struct_field(value, "domain")
        .ok_or_else(|| unsupported("plot scale is missing domain", span))
        .and_then(|value| plot_domain(value, span))?;
    Ok(data_plot_rt::JetDataPlotScale {
        channel,
        kind,
        domain,
        clamp: plot_optional_bool(value, "clamp", false, span)?,
        reverse: plot_optional_bool(value, "reverse", false, span)?,
    })
}

fn plot_axis(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotAxis, Diagnostic> {
    let channel = plot_struct_field(value, "channel")
        .ok_or_else(|| unsupported("plot axis is missing channel", span))
        .and_then(|value| plot_channel(value, span))?;
    let title = plot_struct_field(value, "title")
        .ok_or_else(|| unsupported("plot axis is missing title", span))
        .and_then(|value| plot_string(value, "plot axis title", span))?;
    Ok(data_plot_rt::JetDataPlotAxis {
        channel,
        title,
        visible: plot_optional_bool(value, "visible", true, span)?,
        grid: plot_optional_bool(value, "grid", true, span)?,
        ticks: plot_optional_int(value, "ticks", 5, span)?,
    })
}

fn plot_legend(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotLegend, Diagnostic> {
    let channel = plot_struct_field(value, "channel")
        .ok_or_else(|| unsupported("plot legend is missing channel", span))
        .and_then(|value| plot_channel(value, span))?;
    let title = plot_struct_field(value, "title")
        .ok_or_else(|| unsupported("plot legend is missing title", span))
        .and_then(|value| plot_string(value, "plot legend title", span))?;
    let position = match plot_variant(
        plot_struct_field(value, "position")
            .ok_or_else(|| unsupported("plot legend is missing position", span))?,
        "JetDataPlotLegendPosition",
        span,
    )? {
        "Top" => data_plot_rt::JetDataPlotLegendPosition::Top,
        "Right" => data_plot_rt::JetDataPlotLegendPosition::Right,
        "Bottom" => data_plot_rt::JetDataPlotLegendPosition::Bottom,
        "Left" => data_plot_rt::JetDataPlotLegendPosition::Left,
        _ => return Err(unsupported("unknown JetDataPlotLegendPosition", span)),
    };
    Ok(data_plot_rt::JetDataPlotLegend {
        channel,
        title,
        position,
        visible: plot_optional_bool(value, "visible", true, span)?,
    })
}

fn plot_facet(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotFacet, Diagnostic> {
    let field = plot_struct_field(value, "field")
        .ok_or_else(|| unsupported("plot facet is missing field", span))
        .and_then(|value| plot_field(value, span))?;
    let kind = match plot_variant(
        plot_struct_field(value, "kind")
            .ok_or_else(|| unsupported("plot facet is missing kind", span))?,
        "JetDataPlotFacetKind",
        span,
    )? {
        "Row" => data_plot_rt::JetDataPlotFacetKind::Row,
        "Column" => data_plot_rt::JetDataPlotFacetKind::Column,
        _ => return Err(unsupported("unknown JetDataPlotFacetKind", span)),
    };
    Ok(data_plot_rt::JetDataPlotFacet {
        field,
        kind,
        title: plot_struct_field(value, "title")
            .map(|value| plot_string(value, "plot facet title", span))
            .transpose()?
            .unwrap_or_default(),
        columns: plot_optional_int(value, "columns", 0, span)?,
        rows: plot_optional_int(value, "rows", 0, span)?,
    })
}

fn plot_layer(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotLayer, Diagnostic> {
    let name = plot_struct_field(value, "name")
        .ok_or_else(|| unsupported("plot layer is missing name", span))
        .and_then(|value| plot_string(value, "plot layer name", span))?;
    let mark = plot_struct_field(value, "mark")
        .ok_or_else(|| unsupported("plot layer is missing mark", span))
        .and_then(|value| plot_mark(value, span))?;
    let encodings = match plot_struct_field(value, "encodings") {
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| plot_encoding(value, span))
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(unsupported("plot layer encodings must be a list", span)),
        None => Vec::new(),
    };
    let transforms = match plot_struct_field(value, "transforms") {
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| plot_transform(value, span))
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(unsupported("plot layer transforms must be a list", span)),
        None => Vec::new(),
    };
    Ok(data_plot_rt::JetDataPlotLayer {
        name,
        mark,
        encodings,
        transforms,
        opacity: plot_optional_float(value, "opacity", 1.0, span)?,
    })
}

fn plot_interaction(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotInteraction, Diagnostic> {
    match plot_variant(value, "JetDataPlotInteraction", span)? {
        "Hover" => Ok(data_plot_rt::JetDataPlotInteraction::Hover),
        "Select" => Ok(data_plot_rt::JetDataPlotInteraction::Select),
        "Zoom" => Ok(data_plot_rt::JetDataPlotInteraction::Zoom),
        "Pan" => Ok(data_plot_rt::JetDataPlotInteraction::Pan),
        "Brush" => Ok(data_plot_rt::JetDataPlotInteraction::Brush),
        _ => Err(unsupported("unknown JetDataPlotInteraction variant", span)),
    }
}

fn plot_accessibility(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotAccessibility, Diagnostic> {
    Ok(data_plot_rt::JetDataPlotAccessibility {
        title: plot_struct_field(value, "title")
            .map(|value| plot_string(value, "plot accessibility title", span))
            .transpose()?
            .unwrap_or_default(),
        description: plot_struct_field(value, "description")
            .map(|value| plot_string(value, "plot accessibility description", span))
            .transpose()?
            .unwrap_or_default(),
        summary: plot_struct_field(value, "summary")
            .map(|value| plot_string(value, "plot accessibility summary", span))
            .transpose()?
            .unwrap_or_default(),
        keyboard: plot_optional_bool(value, "keyboard", true, span)?,
        announce_selection: plot_optional_bool(value, "announce_selection", true, span)?,
    })
}

fn plot_layout(
    value: &CtValue,
    span: Span,
) -> Result<data_plot_rt::JetDataPlotLayout, Diagnostic> {
    let defaults = data_plot_rt::JetDataPlotLayout::default();
    Ok(data_plot_rt::JetDataPlotLayout {
        width: plot_optional_float(value, "width", defaults.width, span)?,
        height: plot_optional_float(value, "height", defaults.height, span)?,
        margin_top: plot_optional_float(value, "margin_top", defaults.margin_top, span)?,
        margin_right: plot_optional_float(value, "margin_right", defaults.margin_right, span)?,
        margin_bottom: plot_optional_float(value, "margin_bottom", defaults.margin_bottom, span)?,
        margin_left: plot_optional_float(value, "margin_left", defaults.margin_left, span)?,
    })
}
fn plot_field_value(field: &data_plot_rt::JetDataPlotField) -> CtValue {
    ct_struct(
        "JetDataPlotField",
        vec![
            ("id", CtValue::Str(field.id.clone())),
            ("name", CtValue::Str(field.name.clone())),
            ("type_name", CtValue::Str(field.type_name.clone())),
        ],
    )
}

fn plot_enum_value(type_name: &str, variant: &str) -> CtValue {
    CtValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn plot_mark_value(mark: data_plot_rt::JetDataPlotMark) -> CtValue {
    plot_enum_value("JetDataPlotMark", mark.as_str())
}

fn plot_channel_value(channel: data_plot_rt::JetDataPlotChannel) -> CtValue {
    plot_enum_value("JetDataPlotChannel", channel.as_str())
}

fn plot_aggregate_value(aggregate: data_plot_rt::JetDataPlotAggregate) -> CtValue {
    plot_enum_value("JetDataPlotAggregate", aggregate.as_str())
}

fn plot_filter_op_value(op: data_plot_rt::JetDataPlotFilterOp) -> CtValue {
    plot_enum_value("JetDataPlotFilterOp", op.as_str())
}

fn plot_scale_kind_value(kind: data_plot_rt::JetDataPlotScaleKind) -> CtValue {
    plot_enum_value("JetDataPlotScaleKind", kind.as_str())
}

fn plot_domain_value(domain: &data_plot_rt::JetDataPlotDomain) -> CtValue {
    match domain {
        data_plot_rt::JetDataPlotDomain::Auto => {
            plot_enum_value("JetDataPlotDomain", "Auto")
        }
        data_plot_rt::JetDataPlotDomain::Numeric { min, max } => CtValue::Enum {
            type_name: "JetDataPlotDomain".to_string(),
            variant: "Numeric".to_string(),
            args: vec![
                (Some("min".to_string()), CtValue::Float(CtFloat::f64(*min))),
                (Some("max".to_string()), CtValue::Float(CtFloat::f64(*max))),
            ],
        },
        data_plot_rt::JetDataPlotDomain::Categories(values) => CtValue::Enum {
            type_name: "JetDataPlotDomain".to_string(),
            variant: "Categories".to_string(),
            args: vec![(
                Some("values".to_string()),
                CtValue::List(values.iter().cloned().map(CtValue::Str).collect()),
            )],
        },
    }
}

fn plot_value_value(value: &data_plot_rt::JetDataPlotValue) -> CtValue {
    let (variant, payload) = match value {
        data_plot_rt::JetDataPlotValue::Text(value) => ("Text", CtValue::Str(value.clone())),
        data_plot_rt::JetDataPlotValue::Integer(value) => ("Integer", CtValue::Int(*value)),
        data_plot_rt::JetDataPlotValue::Number(value) => {
            ("Number", CtValue::Float(CtFloat::f64(*value)))
        }
        data_plot_rt::JetDataPlotValue::Boolean(value) => ("Boolean", CtValue::Bool(*value)),
        data_plot_rt::JetDataPlotValue::Null => ("Null", CtValue::Unit),
    };
    CtValue::Enum {
        type_name: "JetDataPlotValue".to_string(),
        variant: variant.to_string(),
        args: if matches!(value, data_plot_rt::JetDataPlotValue::Null) {
            Vec::new()
        } else {
            vec![(None, payload)]
        },
    }
}

fn plot_encoding_value(encoding: &data_plot_rt::JetDataPlotEncoding) -> CtValue {
    ct_struct(
        "JetDataPlotEncoding",
        vec![
            ("channel", plot_channel_value(encoding.channel)),
            ("field", plot_field_value(&encoding.field)),
            ("aggregate", plot_aggregate_value(encoding.aggregate)),
        ],
    )
}

fn plot_transform_value(transform: &data_plot_rt::JetDataPlotTransform) -> CtValue {
    match transform {
        data_plot_rt::JetDataPlotTransform::Filter { field, op, value } => CtValue::Enum {
            type_name: "JetDataPlotTransform".to_string(),
            variant: "Filter".to_string(),
            args: vec![
                (Some("field".to_string()), plot_field_value(field)),
                (Some("op".to_string()), plot_filter_op_value(*op)),
                (Some("value".to_string()), plot_value_value(value)),
            ],
        },
        data_plot_rt::JetDataPlotTransform::Sort { field, descending } => CtValue::Enum {
            type_name: "JetDataPlotTransform".to_string(),
            variant: "Sort".to_string(),
            args: vec![
                (Some("field".to_string()), plot_field_value(field)),
                (Some("descending".to_string()), CtValue::Bool(*descending)),
            ],
        },
        data_plot_rt::JetDataPlotTransform::Bin { field, step } => CtValue::Enum {
            type_name: "JetDataPlotTransform".to_string(),
            variant: "Bin".to_string(),
            args: vec![
                (Some("field".to_string()), plot_field_value(field)),
                (
                    Some("step".to_string()),
                    CtValue::Float(CtFloat::f64(*step)),
                ),
            ],
        },
        data_plot_rt::JetDataPlotTransform::Aggregate {
            group_by,
            field,
            aggregate,
        } => CtValue::Enum {
            type_name: "JetDataPlotTransform".to_string(),
            variant: "Aggregate".to_string(),
            args: vec![
                (
                    Some("group_by".to_string()),
                    CtValue::List(group_by.iter().map(plot_field_value).collect()),
                ),
                (Some("field".to_string()), plot_field_value(field)),
                (Some("aggregate".to_string()), plot_aggregate_value(*aggregate)),
            ],
        },
    }
}

fn plot_scale_value(scale: &data_plot_rt::JetDataPlotScale) -> CtValue {
    ct_struct(
        "JetDataPlotScale",
        vec![
            ("channel", plot_channel_value(scale.channel)),
            ("kind", plot_scale_kind_value(scale.kind)),
            ("domain", plot_domain_value(&scale.domain)),
            ("clamp", CtValue::Bool(scale.clamp)),
            ("reverse", CtValue::Bool(scale.reverse)),
        ],
    )
}

fn plot_axis_value(axis: &data_plot_rt::JetDataPlotAxis) -> CtValue {
    ct_struct(
        "JetDataPlotAxis",
        vec![
            ("channel", plot_channel_value(axis.channel)),
            ("title", CtValue::Str(axis.title.clone())),
            ("visible", CtValue::Bool(axis.visible)),
            ("grid", CtValue::Bool(axis.grid)),
            ("ticks", CtValue::Int(axis.ticks)),
        ],
    )
}

fn plot_legend_position_value(position: data_plot_rt::JetDataPlotLegendPosition) -> CtValue {
    plot_enum_value("JetDataPlotLegendPosition", position.as_str())
}

fn plot_legend_value(legend: &data_plot_rt::JetDataPlotLegend) -> CtValue {
    ct_struct(
        "JetDataPlotLegend",
        vec![
            ("channel", plot_channel_value(legend.channel)),
            ("title", CtValue::Str(legend.title.clone())),
            ("position", plot_legend_position_value(legend.position)),
            ("visible", CtValue::Bool(legend.visible)),
        ],
    )
}

fn plot_facet_kind_value(kind: data_plot_rt::JetDataPlotFacetKind) -> CtValue {
    plot_enum_value("JetDataPlotFacetKind", kind.as_str())
}

fn plot_facet_value(facet: &data_plot_rt::JetDataPlotFacet) -> CtValue {
    ct_struct(
        "JetDataPlotFacet",
        vec![
            ("field", plot_field_value(&facet.field)),
            ("kind", plot_facet_kind_value(facet.kind)),
            ("title", CtValue::Str(facet.title.clone())),
            ("columns", CtValue::Int(facet.columns)),
            ("rows", CtValue::Int(facet.rows)),
        ],
    )
}

fn plot_layer_value(layer: &data_plot_rt::JetDataPlotLayer) -> CtValue {
    ct_struct(
        "JetDataPlotLayer",
        vec![
            ("name", CtValue::Str(layer.name.clone())),
            ("mark", plot_mark_value(layer.mark)),
            (
                "encodings",
                CtValue::List(layer.encodings.iter().map(plot_encoding_value).collect()),
            ),
            (
                "transforms",
                CtValue::List(layer.transforms.iter().map(plot_transform_value).collect()),
            ),
            ("opacity", CtValue::Float(CtFloat::f64(layer.opacity))),
        ],
    )
}

fn plot_interaction_value(interaction: data_plot_rt::JetDataPlotInteraction) -> CtValue {
    plot_enum_value("JetDataPlotInteraction", interaction.as_str())
}

fn plot_accessibility_value(
    accessibility: &data_plot_rt::JetDataPlotAccessibility,
) -> CtValue {
    ct_struct(
        "JetDataPlotAccessibility",
        vec![
            ("title", CtValue::Str(accessibility.title.clone())),
            ("description", CtValue::Str(accessibility.description.clone())),
            ("summary", CtValue::Str(accessibility.summary.clone())),
            ("keyboard", CtValue::Bool(accessibility.keyboard)),
            (
                "announce_selection",
                CtValue::Bool(accessibility.announce_selection),
            ),
        ],
    )
}

fn plot_layout_value(layout: &data_plot_rt::JetDataPlotLayout) -> CtValue {
    ct_struct(
        "JetDataPlotLayout",
        vec![
            ("width", CtValue::Float(CtFloat::f64(layout.width))),
            ("height", CtValue::Float(CtFloat::f64(layout.height))),
            (
                "margin_top",
                CtValue::Float(CtFloat::f64(layout.margin_top)),
            ),
            (
                "margin_right",
                CtValue::Float(CtFloat::f64(layout.margin_right)),
            ),
            (
                "margin_bottom",
                CtValue::Float(CtFloat::f64(layout.margin_bottom)),
            ),
            (
                "margin_left",
                CtValue::Float(CtFloat::f64(layout.margin_left)),
            ),
        ],
    )
}

fn plot_backend_value(backend: data_plot_rt::JetDataPlotBackend) -> CtValue {
    plot_enum_value("JetDataPlotBackend", backend.as_str())
}

fn plot_support_value(support: data_plot_rt::JetDataPlotSupport) -> CtValue {
    plot_enum_value("JetDataPlotSupport", support.as_str())
}

fn plot_source_value(source: &data_plot_rt::JetDataPlotSourceFacts) -> CtValue {
    ct_struct(
        "JetDataPlotSourceFacts",
        vec![
            (
                "table_plan_identity",
                CtValue::Str(source.table_plan_identity.clone()),
            ),
            (
                "source_identity",
                CtValue::Str(source.source_identity.clone()),
            ),
            (
                "schema_identity",
                CtValue::Str(source.schema_identity.clone()),
            ),
            ("row_type", CtValue::Str(source.row_type.clone())),
            ("rows", CtValue::Int(source.rows)),
            ("data_identity", CtValue::Str(source.data_identity.clone())),
            ("provenance", CtValue::Str(source.provenance.clone())),
        ],
    )
}

fn plot_schema_value(schema: &data_plot_rt::JetDataPlotSchema) -> CtValue {
    ct_struct(
        "JetDataPlotSchema",
        vec![
            ("identity", CtValue::Str(schema.identity.clone())),
            ("row_type", CtValue::Str(schema.row_type.clone())),
            (
                "columns",
                CtValue::List(schema.columns.iter().map(plot_field_value).collect()),
            ),
        ],
    )
}

fn plot_limits_value(limits: &data_plot_rt::jet_std::DataLimits) -> CtValue {
    ct_struct(
        "DataLimits",
        vec![
            ("max_groups", CtValue::Int(limits.max_groups)),
            ("max_sort_rows", CtValue::Int(limits.max_sort_rows)),
            ("max_join_rows", CtValue::Int(limits.max_join_rows)),
            ("max_output_rows", CtValue::Int(limits.max_output_rows)),
        ],
    )
}

fn plot_plan_value(plan: &data_plot_rt::JetDataPlotPlan) -> CtValue {
    ct_struct(
        "JetDataPlotPlan",
        vec![
            ("source", plot_source_value(&plan.source)),
            ("schema", plot_schema_value(&plan.schema)),
            ("limits", plot_limits_value(&plan.limits)),
            ("mark", plot_mark_value(plan.mark)),
            (
                "encodings",
                CtValue::List(plan.encodings.iter().map(plot_encoding_value).collect()),
            ),
            (
                "transforms",
                CtValue::List(plan.transforms.iter().map(plot_transform_value).collect()),
            ),
            (
                "scales",
                CtValue::List(plan.scales.iter().map(plot_scale_value).collect()),
            ),
            (
                "axes",
                CtValue::List(plan.axes.iter().map(plot_axis_value).collect()),
            ),
            (
                "legends",
                CtValue::List(plan.legends.iter().map(plot_legend_value).collect()),
            ),
            (
                "facets",
                CtValue::List(plan.facets.iter().map(plot_facet_value).collect()),
            ),
            (
                "layers",
                CtValue::List(plan.layers.iter().map(plot_layer_value).collect()),
            ),
            (
                "interactions",
                CtValue::List(
                    plan.interactions
                        .iter()
                        .copied()
                        .map(plot_interaction_value)
                        .collect(),
                ),
            ),
            (
                "accessibility",
                plot_accessibility_value(&plan.accessibility),
            ),
            ("layout", plot_layout_value(&plan.layout)),
        ],
    )
}

fn plot_capability_value(capability: &data_plot_rt::JetDataPlotCapability) -> CtValue {
    ct_struct(
        "JetDataPlotCapability",
        vec![
            ("backend", plot_backend_value(capability.backend)),
            ("feature", CtValue::Str(capability.feature.clone())),
            ("support", plot_support_value(capability.support)),
            ("reason", CtValue::Str(capability.reason.clone())),
            ("replacement", CtValue::Str(capability.replacement.clone())),
        ],
    )
}

fn plot_selected_row_value(row: &data_plot_rt::JetDataPlotSelectedRow) -> CtValue {
    ct_struct(
        "JetDataPlotSelectedRow",
        vec![
            ("index", CtValue::Int(row.index)),
            (
                "values",
                CtValue::List(
                    row.values
                        .iter()
                        .map(|(field, value)| CtValue::Struct {
                            type_name: "tuple".to_string(),
                            fields: vec![
                                ("field".to_string(), plot_field_value(field)),
                                ("value".to_string(), plot_value_value(value)),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    )
}

fn plot_inspection_value(inspection: &data_plot_rt::JetDataPlotInspection) -> CtValue {
    ct_struct(
        "JetDataPlotInspection",
        vec![
            ("plan", plot_plan_value(&inspection.plan)),
            (
                "selected_indices",
                CtValue::List(
                    inspection
                        .selected_indices
                        .iter()
                        .copied()
                        .map(CtValue::Int)
                        .collect(),
                ),
            ),
            (
                "selected_columns",
                CtValue::List(
                    inspection
                        .selected_columns
                        .iter()
                        .map(plot_field_value)
                        .collect(),
                ),
            ),
            (
                "selected_data",
                CtValue::List(
                    inspection
                        .selected_data
                        .iter()
                        .map(plot_selected_row_value)
                        .collect(),
                ),
            ),
        ],
    )
}

fn plot_render_format_value(format: data_plot_rt::JetDataPlotRenderFormat) -> CtValue {
    plot_enum_value("JetDataPlotRenderFormat", format.as_str())
}

fn plot_render_value(render: &data_plot_rt::JetDataPlotRender) -> CtValue {
    ct_struct(
        "JetDataPlotRender",
        vec![
            ("backend", plot_backend_value(render.backend)),
            ("format", plot_render_format_value(render.format)),
            ("body", CtValue::Str(render.body.clone())),
            ("source", plot_source_value(&render.source)),
            (
                "capabilities",
                CtValue::List(
                    render
                        .capabilities
                        .iter()
                        .map(plot_capability_value)
                        .collect(),
                ),
            ),
        ],
    )
}

fn plot_rows_fields(
    arg0_ty: Option<&Type>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    rows: &[CtValue],
) -> Vec<data_plot_rt::JetDataPlotField> {
    if let Some(CtValue::Struct { fields, .. }) = rows.first() {
        return fields
            .iter()
            .map(|(name, value)| data_plot_rt::JetDataPlotField::new(name, ct_value_type_name(value)))
            .collect();
    }
    if let Some(elem_ty) = arg0_ty.and_then(data_schema_elem_ty) {
        if let Some(definition) = elem_ty.base_name().and_then(|name| structs.get(name)) {
            return definition
                .fields
                .iter()
                .map(|field| {
                    data_plot_rt::JetDataPlotField::new(field.name.clone(), field.ty.name())
                })
                .collect();
        }
        return vec![data_plot_rt::JetDataPlotField::new("value", elem_ty.name())];
    }
    rows.first()
        .map(|value| {
            vec![data_plot_rt::JetDataPlotField::new(
                "value",
                ct_value_type_name(value),
            )]
        })
        .unwrap_or_default()
}


fn plot_error_value(error: &data_plot_rt::JetDataPlotError) -> CtValue {
    let optional_int = |value: Option<i64>| {
        value
            .map(|value| CtValue::Present(Box::new(CtValue::Int(value))))
            .unwrap_or_else(|| CtValue::absent(Type::Int))
    };
    ct_struct(
        "DataError",
        vec![
            (
                "kind",
                plot_enum_value("DataErrorKind", &format!("{:?}", error.kind)),
            ),
            ("operation", CtValue::Str(error.operation.clone())),
            ("row", CtValue::absent(Type::Int)),
            ("column", CtValue::absent(Type::Int)),
            ("index", optional_int(error.index)),
            ("reason", CtValue::Str(error.to_string())),
            ("cause", CtValue::absent(Type::Named("EncodingError".to_string()))),
        ],
    )
}

fn plot_result<T>(
    result: Result<T, data_plot_rt::JetDataPlotError>,
    to_value: impl FnOnce(T) -> CtValue,
) -> CtValue {
    match result {
        Ok(value) => CtValue::Present(Box::new(to_value(value))),
        Err(error) => CtValue::failed(Box::new(plot_error_value(&error))),
    }
}

fn plot_from_rows(
    rows: &CtValue,
    arg0_ty: Option<&Type>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    span: Span,
) -> Result<data_plot_rt::JetDataPlot<CtValue>, Diagnostic> {
    let rows = expect_list(rows, "plot", span)?.clone();
    let fields = plot_rows_fields(arg0_ty, structs, &rows);
    let row_type = arg0_ty
        .and_then(data_schema_elem_ty)
        .map(Type::name)
        .or_else(|| rows.first().map(ct_value_type_name))
        .unwrap_or_else(|| "Value".to_string());
    let identity = format!("data.list:{row_type}");
    let schema = data_plot_rt::JetDataPlotSchema::new(
        identity.clone(),
        row_type.clone(),
        fields.clone(),
    );
    let source = data_plot_rt::JetDataPlotSourceFacts::new(
        identity.clone(),
        identity.clone(),
        identity.clone(),
        row_type,
        i64::try_from(rows.len()).unwrap_or(i64::MAX),
        identity,
        "comptime:data.list",
    );
    let mut plot = data_plot_rt::JetDataPlot::from_rows_with_limits(
        rows,
        source,
        schema,
        data_plot_rt::jet_std::DataLimits::safe(),
    )
    .map_err(|error| unsupported(&error.to_string(), span))?;
    for field in fields {
        let name = field.name.clone();
        plot = plot
            .bind_column(data_plot_rt::jet_data_plot_column(field, move |row| {
                let value = if name == "value" {
                    Some(row)
                } else {
                    query_row_field(row, &name)
                };
                value
                    .map(plot_value)
                    .unwrap_or(data_plot_rt::JetDataPlotValue::Null)
            }))
            .map_err(|error| unsupported(&error.to_string(), span))?;
    }
    Ok(plot)
}

/// One declared row shape, for schema derivation: a struct's generic parameter
/// names in source order, then its `(field, declared type)` list in source
/// order.
///
/// The AST comptime interpreter reads `StructDef`s; the canonical TIR
/// evaluator reads sema's registered field/type tables. Both hand the same
/// facts to the one kernel below, so neither tier owns a schema of its own.
pub type SchemaRow<'f> = &'f dyn Fn(&str) -> Option<(Vec<String>, Vec<(String, Type)>)>;

/// Build the `core.data.plot` value from ordinary typed list rows.
pub(crate) fn plot_static_call(
    state: &mut DataPipelineState,
    method: &str,
    argv: &[CtValue],
    arg0_ty: Option<&Type>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let one = |what: &str| {
        argv.first()
            .ok_or_else(|| unsupported(&format!("`data.{method}()`: missing {what}"), span))
    };
    match method {
        "plot" => {
            let plot = plot_from_rows(one("rows")?, arg0_ty, structs, span)?;
            Ok(CtValue::Present(Box::new(store_plot(state, plot))))
        }
        "inspect" => {
            let plot = clone_plot(state, one("plot")?, span)?;
            Ok(plot_result(plot.inspect(), |value| plot_inspection_value(&value)))
        }
        "inspect_json" => {
            let plot = clone_plot(state, one("plot")?, span)?;
            Ok(plot_result(plot.inspect_json(), CtValue::Str))
        }
        "text" => {
            let plot = clone_plot(state, one("plot")?, span)?;
            Ok(plot_result(plot.text(), CtValue::Str))
        }
        "svg" => {
            let plot = clone_plot(state, one("plot")?, span)?;
            Ok(plot_result(plot.svg(), CtValue::Str))
        }
        "show" => {
            let plot = clone_plot(state, one("plot")?, span)?;
            Ok(plot_result(plot.show(), |value| plot_render_value(&value)))
        }
        "render" => {
            let plot = clone_plot(state, one("plot")?, span)?;
            let backend = argv
                .get(1)
                .ok_or_else(|| unsupported("`data.render()`: missing backend", span))
                .and_then(|value| match plot_variant(value, "JetDataPlotBackend", span)? {
                    "Terminal" => Ok(data_plot_rt::JetDataPlotBackend::Terminal),
                    "Browser" => Ok(data_plot_rt::JetDataPlotBackend::Browser),
                    "Native" => Ok(data_plot_rt::JetDataPlotBackend::Native),
                    "Export" => Ok(data_plot_rt::JetDataPlotBackend::Export),
                    _ => Err(unsupported("unknown JetDataPlotBackend variant", span)),
                })?;
            Ok(plot_result(plot.render(backend), |value| plot_render_value(&value)))
        }
        _ => Err(unsupported(&format!("`data.{method}()`"), span)),
    }
}

/// Receiver-shaped plot calls share the same opaque plan slot as the static
/// render/inspection calls. The caller keeps the returned handle, while the
/// slot replacement makes aliases observe one logical builder plan.
pub(crate) fn eval_data_plot_method(
    state: &mut DataPipelineState,
    recv: &CtValue,
    method: &str,
    argv: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(
        recv,
        CtValue::Struct { type_name, .. } if type_name == "JetDataPlot"
    ) {
        return None;
    }
    let result = (|| {
        let plot = clone_plot(state, recv, span)?;
        match method {
            "line" => replace_plot(state, recv, plot.line(), span),
            "bar" => replace_plot(state, recv, plot.bar(), span),
            "point" => replace_plot(state, recv, plot.point(), span),
            "mark" => {
                let mark = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.mark() needs a mark", span))
                    .and_then(|value| plot_mark(value, span))?;
                replace_plot(state, recv, plot.mark(mark), span)
            }
            "x" => {
                let column = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.x() needs a column", span))
                    .and_then(|value| plot_column(value, span))?;
                match plot.x(column) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "y" => {
                let column = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.y() needs a column", span))
                    .and_then(|value| plot_column(value, span))?;
                let aggregate = argv
                    .get(1)
                    .ok_or_else(|| unsupported("plot.y() needs an aggregate", span))
                    .and_then(|value| plot_aggregate(value, span))?;
                match plot.y(column, aggregate) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "color" | "size" | "text" | "text_channel" | "detail" => {
                let column = argv
                    .first()
                    .ok_or_else(|| unsupported("plot channel method needs a column", span))
                    .and_then(|value| plot_column(value, span))?;
                let result = match method {
                    "color" => plot.color(column),
                    "size" => plot.size(column),
                    "text" | "text_channel" => plot.text_channel(column),
                    "detail" => plot.detail(column),
                    _ => unreachable!("plot channel method set is closed"),
                };
                match result {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "encode" => {
                let channel = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.encode() needs a channel", span))
                    .and_then(|value| plot_channel(value, span))?;
                let column = argv
                    .get(1)
                    .ok_or_else(|| unsupported("plot.encode() needs a column", span))
                    .and_then(|value| plot_column(value, span))?;
                let aggregate = argv
                    .get(2)
                    .ok_or_else(|| unsupported("plot.encode() needs an aggregate", span))
                    .and_then(|value| plot_aggregate(value, span))?;
                match plot.encode(channel, column, aggregate) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_transform" => {
                let transform = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_transform() needs a transform", span))
                    .and_then(|value| plot_transform(value, span))?;
                match plot.with_transform(transform) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_scale" => {
                let scale = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_scale() needs a scale", span))
                    .and_then(|value| plot_scale(value, span))?;
                match plot.with_scale(scale) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_axis" => {
                let axis = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_axis() needs an axis", span))
                    .and_then(|value| plot_axis(value, span))?;
                match plot.with_axis(axis) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_legend" => {
                let legend = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_legend() needs a legend", span))
                    .and_then(|value| plot_legend(value, span))?;
                match plot.with_legend(legend) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_facet" => {
                let facet = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_facet() needs a facet", span))
                    .and_then(|value| plot_facet(value, span))?;
                let column = argv
                    .get(1)
                    .ok_or_else(|| unsupported("plot.with_facet() needs a column", span))
                    .and_then(|value| plot_column(value, span))?;
                match plot.with_facet(facet, column) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_layer" => {
                let layer = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_layer() needs a layer", span))
                    .and_then(|value| plot_layer(value, span))?;
                match plot.with_layer(layer) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "with_interaction" => {
                let interaction = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.with_interaction() needs an interaction", span))
                    .and_then(|value| plot_interaction(value, span))?;
                match plot.with_interaction(interaction) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "accessibility" => {
                let accessibility = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.accessibility() needs accessibility", span))
                    .and_then(|value| plot_accessibility(value, span))?;
                match plot.accessibility(accessibility) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "layout" => {
                let layout = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.layout() needs a layout", span))
                    .and_then(|value| plot_layout(value, span))?;
                match plot.layout(layout) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            "select_indices" => {
                let indices = argv
                    .first()
                    .ok_or_else(|| unsupported("plot.select_indices() needs indices", span))?;
                let CtValue::List(indices) = indices else {
                    return Err(unsupported("plot.select_indices() needs a list", span));
                };
                let indices = indices
                    .iter()
                    .map(|value| plot_int(value, "plot selection index", span))
                    .collect::<Result<Vec<_>, _>>()?;
                match plot.select_indices(indices) {
                    Ok(next) => replace_plot(state, recv, next, span),
                    Err(error) => return Ok(CtValue::failed(Box::new(plot_error_value(&error)))),
                }
            }
            _ => {
                return Err(unsupported(
                    &format!("unsupported JetDataPlot receiver method `{method}`"),
                    span,
                ))
            }
        }
    })();
    Some(result)
}

/// `CtValue` carrier needs where the Rust helper carries `T` on the type.


/// Type-driven schema columns — the derivation AOT emits from
/// `emit_data_schema_columns`. A scalar list is one `value` column; a named
/// or applied row struct expands into its declared fields with the call-site
/// type arguments substituted in.
pub fn schema_columns_for_type(
    elem_ty: &Type,
    expand_struct: bool,
    row: SchemaRow<'_>,
) -> Option<Vec<CtValue>> {
    if !expand_struct {
        return Some(vec![data_column("value", &elem_ty.name())]);
    }
    let Some(struct_name) = elem_ty.base_name() else {
        return Some(vec![data_column("value", &elem_ty.name())]);
    };
    let Some((type_params, fields)) = row(struct_name) else {
        return Some(vec![data_column("value", &elem_ty.name())]);
    };
    let args = match elem_ty {
        Type::Apply { args, .. } => args.as_slice(),
        Type::Named(_) => &[],
        _ => return None,
    };
    if type_params.len() != args.len() {
        return None;
    }
    let subst: HashMap<String, Type> = type_params.into_iter().zip(args.iter().cloned()).collect();
    Some(
        fields
            .iter()
            .map(|(name, field_ty)| {
                let field_ty = crate::Generics::substitute_type(field_ty, &subst);
                data_column(name, &field_ty.name())
            })
            .collect(),
    )
}

/// Derive `core.data.schema` from an ordinary list's checked row type.
pub fn schema_value(
    recv: &CtValue,
    arg0_ty: Option<&Type>,
    row: SchemaRow<'_>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let sample = match recv {
        CtValue::List(xs) => xs.first(),
        _ => {
            return Err(unsupported(
                "`data.schema()` needs an ordinary typed list",
                span,
            ))
        }
    };
    let columns = match arg0_ty
        .and_then(data_schema_elem_ty)
        .and_then(|elem| schema_columns_for_type(elem, true, row))
    {
        Some(columns) => columns,
        None => match sample {
            Some(CtValue::Struct { fields, .. }) => fields
                .iter()
                .map(|(name, value)| data_column(name, &ct_value_type_name(value)))
                .collect(),
            Some(value) => vec![data_column("value", &ct_value_type_name(value))],
            None => Vec::new(),
        },
    };
    Ok(CtValue::List(columns))
}

fn add_query_values(
    left: &mut CtValue,
    right: CtValue,
    span: Span,
) -> Result<(), Diagnostic> {
    match (left, right) {
        (CtValue::Int(left), CtValue::Int(right)) => {
            *left = left
                .checked_add(right)
                .ok_or_else(|| unsupported("`Query.sum()` integer overflow", span))?;
            Ok(())
        }
        (CtValue::Float(left), CtValue::Float(right)) => {
            *left = CtFloat::f64(left.as_f64() + right.as_f64());
            Ok(())
        }
        (CtValue::Float(left), CtValue::Int(right)) => {
            *left = CtFloat::f64(left.as_f64() + right as f64);
            Ok(())
        }
        (CtValue::Int(_), CtValue::Float(_)) => Err(unsupported(
            "`Query.sum()` cannot mix Int and Float values",
            span,
        )),
        _ => Err(unsupported(
            "`Query.sum()` callback must return numeric values",
            span,
        )),
    }
}

/// One callback invocation strategy: the AST interpreter calls through its own
/// `Interp`, the MIR interpreter through the standalone closure host. The
/// query semantics below are written once against this seam.
type QueryCallback<'i> =
    &'i mut dyn FnMut(&CtValue, Vec<CtValue>, Span) -> Result<CtValue, Diagnostic>;

fn collect_query_rows(
    state: &DataPipelineState,
    query: &ComptimeQuery,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let limits = data_kernel_limits_safe();
    let max_sort_rows = usize::try_from(limits.max_sort_rows).unwrap_or(usize::MAX);
    let max_join_rows = usize::try_from(limits.max_join_rows).unwrap_or(usize::MAX);
    let max_output_rows = usize::try_from(limits.max_output_rows).unwrap_or(usize::MAX);
    let mut rows = match query.tracked {
        Some(id) => state
            .tracked
            .get(id)
            .and_then(Option::as_ref)
            .ok_or_else(|| unsupported("`DataTracked` state slot is stale", span))?
            .rows
            .iter()
            .map(|(_, row)| row.clone())
            .collect(),
        None => query.rows.clone(),
    };
    for operation in &query.operations {
        match operation {
            ComptimeQueryOperation::Filter(predicate) => {
                let mut filtered = Vec::with_capacity(rows.len());
                for row in rows {
                    if super::Builtins::as_bool(&call(predicate, vec![row.clone()], span)?, span)? {
                        filtered.push(row);
                    }
                }
                rows = filtered;
            }
            ComptimeQueryOperation::SortBy(key) => {
                if rows.len() > max_sort_rows {
                    return Err(unsupported(
                        "`Query.sort_by()` input exceeds max_sort_rows",
                        span,
                    ));
                }
                let mut keyed = Vec::with_capacity(rows.len());
                for row in rows {
                    let value = as_string(&call(key, vec![row.clone()], span)?, span)?.to_string();
                    keyed.push((value, row));
                }
                keyed.sort_by(|left, right| left.0.cmp(&right.0));
                rows = keyed.into_iter().map(|(_, row)| row).collect();
            }
            ComptimeQueryOperation::Map(mapper) => {
                rows = rows
                    .into_iter()
                    .map(|row| call(mapper, vec![row], span))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            ComptimeQueryOperation::Extreme { key, maximum } => {
                let mut selected: Option<(CtValue, CtValue)> = None;
                for row in rows {
                    let value = call(key, vec![row], span)?;
                    if let CtValue::Float(number) = &value {
                        if !number.as_f64().is_finite() {
                            return Err(unsupported(
                                "`Query.min()`/`Query.max()` key must be finite",
                                span,
                            ));
                        }
                    }
                    let replace = match selected.as_ref() {
                        None => true,
                        Some((best, _)) => {
                            let order =
                                super::Builtins::cmp(value.clone(), best.clone(), span)?;
                            if *maximum {
                                order == std::cmp::Ordering::Greater
                            } else {
                                order == std::cmp::Ordering::Less
                            }
                        }
                    };
                    if replace {
                        selected = Some((value.clone(), value));
                    }
                }
                rows = match selected {
                    Some((_, value)) => vec![value],
                    None => {
                        return Err(unsupported(
                            if *maximum {
                                "`Query.max()` cannot reduce an empty query"
                            } else {
                                "`Query.min()` cannot reduce an empty query"
                            },
                            span,
                        ))
                    }
                };
            }
            ComptimeQueryOperation::Join {
                right,
                left_key,
                right_key,
                left,
                right_type,
            } => {
                let right_query = clone_query(state, right, span)?;
                let right_rows = collect_query_rows(state, &right_query, &mut *call, span)?;
                if right_rows.len() > max_join_rows {
                    return Err(unsupported(
                        "`Query.join()` input exceeds max_join_rows",
                        span,
                    ));
                }
                rows = collect_join_rows(
                    &rows,
                    &right_rows,
                    left_key,
                    right_key,
                    *left,
                    Some(right_type.clone()),
                    &mut *call,
                    span,
                )?;
            }
        }
    }
    if rows.len() > max_output_rows {
        return Err(unsupported(
            "`Query.collect()` output exceeds max_output_rows",
            span,
        ));
    }
    Ok(rows)
}

fn collect_join_rows(
    left_rows: &[CtValue],
    right_rows: &[CtValue],
    left_key: &CtValue,
    right_key: &CtValue,
    left_join: bool,
    right_type: Option<Type>,
    call: QueryCallback<'_>,
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let mut right_by_key = BTreeMap::<String, Vec<CtValue>>::new();
    for row in right_rows {
        let key = as_string(&call(right_key, vec![row.clone()], span)?, span)?.to_string();
        right_by_key.entry(key).or_default().push(row.clone());
    }
    let mut joined = Vec::new();
    for left_row in left_rows {
        let key = as_string(
            &call(left_key, vec![left_row.clone()], span)?,
            span,
        )?
        .to_string();
        match right_by_key.get(&key) {
            Some(matches) => {
                for right_row in matches {
                    joined.push(ct_struct(
                        "DataJoin",
                        vec![
                            ("left", left_row.clone()),
                            (
                                "right",
                                if left_join {
                                    CtValue::Present(Box::new(right_row.clone()))
                                } else {
                                    right_row.clone()
                                },
                            ),
                        ],
                    ));
                }
            }
            None if left_join => {
                let right_type = right_type.clone().ok_or_else(|| {
                    unsupported(
                        "`data.left_join()` needs a resolved right row type",
                        span,
                    )
                })?;
                joined.push(ct_struct(
                    "DataJoin",
                    vec![
                        ("left", left_row.clone()),
                        ("right", CtValue::absent(right_type)),
                    ],
                ));
            }
            None => {}
        }
    }
    Ok(joined)
}

fn store_query_with_operations(
    state: &mut DataPipelineState,
    query: ComptimeQuery,
) -> CtValue {
    state.queries.push(Some(query));
    ct_struct(
        "Query",
        vec![(
            "slot",
            CtValue::Int(i64::try_from(state.queries.len()).unwrap_or(i64::MAX)),
        )],
    )
}

/// `Query` / `DataGroupedQuery` receiver methods for both interpreters.
pub(crate) fn eval_data_query_method_with(
    state: &mut DataPipelineState,
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    resolved_ret: Option<&Type>,
    call: QueryCallback<'_>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let type_name = match recv {
        CtValue::Struct { type_name, .. } => type_name.as_str(),
        _ => return None,
    };
    if type_name == "DataTracked" {
        let tracked = match tracked_id(state, recv, span) {
            Ok(id) => id,
            Err(error) => return Some(Err(error)),
        };
        match method {
            "query" => {
                if !args.is_empty() {
                    return Some(Err(unsupported(
                        "`DataTracked.query()` takes no arguments",
                        span,
                    )));
                }
                let slot = match state.tracked[tracked - 1].as_ref() {
                    Some(slot) => slot,
                    None => return Some(Err(unsupported("`DataTracked` state slot is stale", span))),
                };
                let query = ComptimeQuery {
                    rows: slot.rows.iter().map(|(_, row)| row.clone()).collect(),
                    tracked: Some(tracked - 1),
                    operations: Vec::new(),
                    plan: vec!["track".to_string()],
                };
                return Some(Ok(store_query_with_operations(state, query)));
            }
            "insert" => {
                if args.len() != 1 {
                    return Some(Err(unsupported(
                        "`DataTracked.insert()` needs one row",
                        span,
                    )));
                }
                return Some(
                    tracked_insert(state, recv, args[0].clone(), call, span)
                        .map(|()| CtValue::Present(Box::new(CtValue::Unit))),
                );
            }
            "replace" => {
                if args.len() != 2 {
                    return Some(Err(unsupported(
                        "`DataTracked.replace()` needs a key and row",
                        span,
                    )));
                }
                return Some(
                    tracked_replace(
                        state,
                        recv,
                        args[0].clone(),
                        args[1].clone(),
                        call,
                        span,
                    )
                    .map(|()| CtValue::Present(Box::new(CtValue::Unit))),
                );
            }
            "remove" => {
                if args.len() != 1 {
                    return Some(Err(unsupported(
                        "`DataTracked.remove()` needs one key",
                        span,
                    )));
                }
                return Some(
                    tracked_remove(state, recv, args[0].clone(), call, span)
                        .map(|()| CtValue::Present(Box::new(CtValue::Unit))),
                );
            }
            _ => return None,
        }
    }
    if type_name == "DataWatch" {
        let watch = match watch_id(state, recv, span) {
            Ok(id) => id,
            Err(error) => return Some(Err(error)),
        };
        match method {
            "get" => {
                if !args.is_empty() {
                    return Some(Err(unsupported("`DataWatch.get()` takes no arguments", span)));
                }
                return Some(
                    watch_refresh(state, recv, call, span)
                        .map(|rows| CtValue::Present(Box::new(CtValue::List(rows)))),
                );
            }
            "status" => {
                if !args.is_empty() {
                    return Some(Err(unsupported(
                        "`DataWatch.status()` takes no arguments",
                        span,
                    )));
                }
                let slot = match state.watches[watch - 1].as_ref() {
                    Some(slot) => slot,
                    None => return Some(Err(unsupported("`DataWatch` state slot is stale", span))),
                };
                let revision = match query_source_revision(state, &slot.query, span) {
                    Ok(revision) => revision,
                    Err(error) => return Some(Err(error)),
                };
                return Some(Ok(ct_struct(
                    "DataWatchStatus",
                    vec![
                        ("mode", CtValue::Str(slot.mode.clone())),
                        (
                            "revision",
                            CtValue::Int(i64::try_from(revision).unwrap_or(i64::MAX)),
                        ),
                        (
                            "retained_rows",
                            CtValue::Int(
                                i64::try_from(slot.retained_rows).unwrap_or(i64::MAX),
                            ),
                        ),
                        (
                            "recomputations",
                            CtValue::Int(
                                i64::try_from(slot.recomputations).unwrap_or(i64::MAX),
                            ),
                        ),
                        ("active", CtValue::Bool(slot.lifecycle.active)),
                        (
                            "generation",
                            CtValue::Int(
                                i64::try_from(slot.lifecycle.generation).unwrap_or(i64::MAX),
                            ),
                        ),
                        ("dirty", CtValue::Bool(slot.lifecycle.dirty)),
                        ("error", CtValue::Str(slot.lifecycle.error.clone())),
                        (
                            "freshness_ms",
                            CtValue::Int(
                                i64::try_from(slot.lifecycle.freshness_ms())
                                    .unwrap_or(i64::MAX),
                            ),
                        ),
                        (
                            "invalidation_cause",
                            CtValue::Str(slot.lifecycle.invalidation_cause.clone()),
                        ),
                        ("refreshing", CtValue::Bool(slot.lifecycle.refreshing)),
                        ("cancelled", CtValue::Bool(slot.lifecycle.cancelled)),
                    ],
                )));
            }
            "cancel" => {
                if !args.is_empty() {
                    return Some(Err(unsupported(
                        "`DataWatch.cancel()` takes no arguments",
                        span,
                    )));
                }
                let slot = match state.watches[watch - 1].as_mut() {
                    Some(slot) => slot,
                    None => return Some(Err(unsupported("`DataWatch` state slot is stale", span))),
                };
                slot.lifecycle.cancel();
                return Some(Ok(CtValue::Unit));
            }
            _ => return None,
        }
    }
    if type_name == "Query" {
        let query = match clone_query(state, recv, span) {
            Ok(query) => query,
            Err(error) => return Some(Err(error)),
        };
        match method {
            "filter" | "sort_by" | "map" => {
                if args.len() != 1 {
                    return Some(Err(unsupported(
                        &format!("`Query.{method}()` needs one callback"),
                        span,
                    )));
                }
                let mut next = query;
                next.operations.push(match method {
                    "filter" => ComptimeQueryOperation::Filter(args[0].clone()),
                    "sort_by" => ComptimeQueryOperation::SortBy(args[0].clone()),
                    _ => ComptimeQueryOperation::Map(args[0].clone()),
                });
                next.plan.push(method.to_string());
                return Some(Ok(store_query_with_operations(state, next)));
            }
            "min" | "max" => {
                if args.len() != 1 {
                    return Some(Err(unsupported(
                        &format!("`Query.{method}()` needs one callback"),
                        span,
                    )));
                }
                let mut next = query;
                next.operations.push(ComptimeQueryOperation::Extreme {
                    key: args[0].clone(),
                    maximum: method == "max",
                });
                next.plan.push(method.to_string());
                return Some(Ok(store_query_with_operations(state, next)));
            }
            "inner_join" | "left_join" => {
                if args.len() != 3 {
                    return Some(Err(unsupported(
                        &format!("`Query.{method}()` needs a query and two callbacks"),
                        span,
                    )));
                }
                let right_query = match clone_query(state, &args[0], span) {
                    Ok(query) => query,
                    Err(error) => return Some(Err(error)),
                };
                let right_type = query_join_right_type(resolved_ret)
                    .or_else(|| right_query.rows.first().map(CtValue::jet_type))
                    .unwrap_or(Type::Int);
                let mut next = query;
                next.operations.push(ComptimeQueryOperation::Join {
                    right: args[0].clone(),
                    left_key: args[1].clone(),
                    right_key: args[2].clone(),
                    left: method == "left_join",
                    right_type,
                });
                next.plan.push(method.to_string());
                return Some(Ok(store_query_with_operations(state, next)));
            }
            "watch" => {
                if !args.is_empty() {
                    return Some(Err(unsupported("`Query.watch()` takes no arguments", span)));
                }
                if !query_is_maintained(state, &query) {
                    return Some(Err(unsupported(
                        "E2476: `Query.watch()` requires a maintained `data.track()` source",
                        span,
                    )));
                }
                let entries = match collect_query_rows(state, &query, call, span) {
                    Ok(rows) if rows.len() <= data_kernel_limits_safe().max_output_rows as usize => {
                        rows
                    }
                    Ok(_) => {
                        return Some(Err(unsupported(
                            "`DataWatch` output exceeds max_output_rows",
                            span,
                        )))
                    }
                    Err(error) => return Some(Err(error)),
                };
                let source_revision = match query_source_revision(state, &query, span) {
                    Ok(revision) => revision,
                    Err(error) => return Some(Err(error)),
                };
                let retained_rows = match query_retained_rows(state, &query, span) {
                    Ok(rows) => rows,
                    Err(error) => return Some(Err(error)),
                };
                let mode = query
                    .plan
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "tracked".to_string());
                return Some(Ok(CtValue::Present(Box::new(store_watch(
                    state,
                    query,
                    entries,
                    source_revision,
                    retained_rows,
                    mode,
                )))));
            }
            "collect" => {
                if !args.is_empty() {
                    return Some(Err(unsupported("`Query.collect()` takes no arguments", span)));
                }
                return Some(
                    collect_query_rows(state, &query, call, span)
                        .map(|rows| CtValue::Present(Box::new(CtValue::List(rows)))),
                );
            }
            "plan" => {
                if !args.is_empty() {
                    return Some(Err(unsupported("`Query.plan()` takes no arguments", span)));
                }
                return Some(Ok(CtValue::List(
                    query.plan.into_iter().map(CtValue::Str).collect(),
                )));
            }
            "group_by" => {
                if args.len() != 1 {
                    return Some(Err(unsupported("`Query.group_by()` needs one callback", span)));
                }
                return Some(Ok(ct_struct(
                    "DataGroupedQuery",
                    vec![("query", recv.clone()), ("key", args[0].clone())],
                )));
            }
            _ => return None,
        }
    }
    if type_name != "DataGroupedQuery" {
        return None;
    }
    let grouped_query = match struct_field(recv, "DataGroupedQuery", "query") {
        Some(query) => match clone_query(state, query, span) {
            Ok(query) => query,
            Err(error) => return Some(Err(error)),
        },
        None => return Some(Err(unsupported("`DataGroupedQuery` has no query source", span))),
    };
    let key = match struct_field(recv, "DataGroupedQuery", "key") {
        Some(key) => key.clone(),
        None => return Some(Err(unsupported("`DataGroupedQuery` has no key callback", span))),
    };
    let mut plan = grouped_query.plan.clone();
    plan.push("group_by".to_string());
    plan.push(method.to_string());
    if method == "count" {
        if !args.is_empty() {
            return Some(Err(unsupported("`DataGroupedQuery.count()` takes no arguments", span)));
        }
        let rows = match collect_query_rows(state, &grouped_query, call, span) {
            Ok(rows) => rows,
            Err(error) => return Some(Err(error)),
        };
        let mut groups: Vec<(CtValue, i64)> = Vec::new();
        for row in rows {
            let group_key = match call(&key, vec![row], span) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
            if let Some((_, count)) = groups.iter_mut().find(|(existing, _)| *existing == group_key) {
                *count += 1;
            } else {
                groups.push((group_key, 1));
            }
        }
        let rows = groups
            .into_iter()
            .map(|(key, value)| ct_struct("Group", vec![("key", key), ("value", CtValue::Int(value))]))
            .collect();
        return Some(Ok(store_query(state, rows, plan)));
    }
    if !matches!(method, "sum" | "mean") || args.len() != 1 {
        return None;
    }
    let value = args[0].clone();
    let rows = match collect_query_rows(state, &grouped_query, call, span) {
        Ok(rows) => rows,
        Err(error) => return Some(Err(error)),
    };
    let mut groups: Vec<(CtValue, CtValue, i64)> = Vec::new();
    for row in rows {
        let group_key = match call(&key, vec![row.clone()], span) {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        let group_value = match call(&value, vec![row], span) {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        if let Some((_, total, count)) = groups.iter_mut().find(|(existing, _, _)| *existing == group_key) {
            if method == "mean" {
                let (CtValue::Float(total_value), CtValue::Float(next_value)) = (&*total, &group_value) else {
                    return Some(Err(unsupported("`Query.mean()` callback must return Float", span)));
                };
                *total = CtValue::Float(CtFloat::f64(total_value.as_f64() + next_value.as_f64()));
            } else if let Err(error) = add_query_values(total, group_value, span) {
                return Some(Err(error));
            }
            *count += 1;
        } else {
            if method == "mean" && !matches!(group_value, CtValue::Float(_)) {
                return Some(Err(unsupported("`Query.mean()` callback must return Float", span)));
            }
            groups.push((group_key, group_value, 1));
        }
    }
    let rows = groups
        .into_iter()
        .map(|(key, mut value, count)| {
            if method == "mean" {
                if let CtValue::Float(total) = value {
                    value = CtValue::Float(CtFloat::f64(total.as_f64() / count as f64));
                }
            }
            ct_struct("Group", vec![("key", key), ("value", value)])
        })
        .collect();
    Some(Ok(store_query(state, rows, plan)))
}

impl<'a> Interp<'a> {
    pub(crate) fn eval_data_query_method(
        &mut self,
        recv: &CtValue,
        method: &str,
        args: &[CtValue],
        resolved_ret: Option<&Type>,
        span: Span,
    ) -> Option<Result<CtValue, Diagnostic>> {
        // The query state is lent to the shared kernel while callbacks run
        // through this evaluator; it is restored before returning so every
        // slot allocated here stays visible to later calls.
        let mut state = std::mem::take(&mut self.data_pipeline);
        let result = eval_data_query_method_with(
            &mut state,
            recv,
            method,
            args,
            resolved_ret,
            &mut |callback, callback_args, span| self.call_closure(callback, callback_args, span),
            span,
        );
        self.data_pipeline = state;
        result
    }


    pub(super) fn eval_data_call(
        &mut self,
        method: &str,
        mut argv: Vec<CtValue>,
        type_args: &[Type],
        arg0_ty: Option<&Type>,
        call_ret: Option<&Type>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        match method {
            "plot" | "inspect" | "inspect_json" | "text" | "svg" | "show" | "render" => {
                plot_static_call(&mut self.data_pipeline, method, &argv, arg0_ty, self.structs, span)
            }
            "track" => {
                if argv.len() != 2 {
                    return Err(unsupported(
                        "`data.track()` expects rows and one key callback",
                        span,
                    ));
                }
                let rows = expect_list(&argv[0], "track", span)?.clone();
                let key = argv[1].clone();
                let mut state = std::mem::take(&mut self.data_pipeline);
                let result = build_tracked(
                    &mut state,
                    rows,
                    key,
                    &mut |callback, callback_args, span| {
                        self.call_closure(callback, callback_args, span)
                    },
                    span,
                )
                .map(|tracked| CtValue::Present(Box::new(tracked)));
                self.data_pipeline = state;
                return result;
            }
            "query" => {
                let rows = expect_list(
                    argv.first()
                        .ok_or_else(|| unsupported("`data.query()`: missing rows", span))?,
                    "query",
                    span,
                )?
                .clone();
                if argv.len() == 1 {
                    return Ok(store_query(
                        &mut self.data_pipeline,
                        rows,
                        vec!["scan".to_string()],
                    ));
                }
                let sql = match argv.get(1) {
                    Some(CtValue::Str(value)) => value,
                    _ => {
                        return Err(unsupported(
                            "`data.query()`: expected SQL text as argument 1",
                            span,
                        ))
                    }
                };
                let selected = match query_sql_rows(
                    rows,
                    sql,
                    query_declared_fields(arg0_ty, call_ret, self.structs),
                ) {
                    Ok(rows) => rows,
                    Err(error) => return Ok(CtValue::failed(Box::new(error))),
                };
                Ok(CtValue::Present(Box::new(store_query(
                    &mut self.data_pipeline,
                    selected,
                    vec!["scan".to_string(), "sql".to_string()],
                ))))
            }
            "csv" => {
                let Some(ty) = type_args.first() else {
                    return Err(unsupported("`data.csv<T>()` needs a type argument", span));
                };
                let text = match argv.first() {
                    Some(CtValue::Str(s)) => s.clone(),
                    _ => {
                        return Err(unsupported(
                            "`data.csv()`: expected a string argument",
                            span,
                        ))
                    }
                };
                self.eval_typed_csv_decode(&text, ty, span)
            }
            "json" => {
                let Some(ty) = type_args.first() else {
                    return Err(unsupported("`data.json<T>()` needs a type argument", span));
                };
                let text = match argv.first() {
                    Some(CtValue::Str(s)) => s.clone(),
                    _ => {
                        return Err(unsupported(
                            "`data.json()`: expected a string argument",
                            span,
                        ))
                    }
                };
                // Array-of-objects → `[T]`, same Decode model as `encoding.json.decode<[T]>`.
                self.eval_typed_decode(
                    "core.encoding.json",
                    &text,
                    &Type::List(Box::new(ty.clone())),
                    span,
                )
            }
            "count" => {
                let recv = argv
                    .first()
                    .ok_or_else(|| unsupported("`data.count()`: missing argument", span))?;
                Ok(CtValue::Int(
                    expect_list(recv, "count", span)?.len() as i64,
                ))
            }
            "schema" => {
                let recv = argv
                    .first()
                    .ok_or_else(|| unsupported("`data.schema()`: missing argument", span))?;
                let structs = self.structs;
                let row: SchemaRow<'_> = &|name: &str| {
                    let def = structs.get(name)?;
                    Some((
                        def.type_params
                            .iter()
                            .map(|param| param.name.clone())
                            .collect::<Vec<String>>(),
                        def.fields
                            .iter()
                            .map(|field| (field.name.clone(), field.ty.clone()))
                            .collect::<Vec<(String, Type)>>(),
                    ))
                };
                schema_value(recv, arg0_ty, row, span)
            }
            "pivot_sum" => {
                super::Methods::eval_data_pivot_sum(&argv, span, |callback, args, span| {
                    self.call_closure(callback, args, span)
                })
            }

            "inner_join" | "left_join" => {
                let right_key = argv.pop().unwrap();
                let left_key = argv.pop().unwrap();
                let right = expect_list(&argv[1], method, span)?.clone();
                let left = expect_list(&argv[0], method, span)?.clone();
                let right_type = data_join_right_type(call_ret);
                let mut call = |callback: &CtValue, args: Vec<CtValue>, span: Span| {
                    self.call_closure(callback, args, span)
                };
                Ok(CtValue::List(collect_join_rows(
                    &left,
                    &right,
                    &left_key,
                    &right_key,
                    method == "left_join",
                    right_type,
                    &mut call,
                    span,
                )?))
            }
            // I4: the pipeline runs on the runtime evaluator too; name the
            // call, not a phase.
            _ => Err(unsupported(&format!("`data.{}()`", method), span)),
        }
    }
}
