// D-SQL-SURFACE1=C: one small, std-only query kernel shared by the CSV file
// door and the in-memory list door. The row stays `DataTree` here; typed
// wrappers preserve source row identity for wildcard/filter-only queries and
// materialize explicit projections through the shared SQL result kernel.
//
// Supported surface: SELECT * or explicit fields, COUNT/SUM/AVG/MIN/MAX,
// optional WHERE, GROUP BY (including ordinals), ORDER BY, and LIMIT. The plan
// shape leaves room for predicate pushdown, memory budgets, and spill policy
// without putting those policies in a tier adapter.
mod jet_sql_query {
    include!("../../Core/SqlQuery.rs");
}

type JetAnalyticsQuery = jet_sql_query::JetSqlQuery;

fn jet_analytics_query_error(message: impl Into<String>) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::one(message.into())
}

/// Shared CSV query source adapter. Tier hosts marshal this Result carrier;
/// file-read policy and its error text remain in the Prelude.
const JET_DATA_QUERY_MAX_BYTES: u64 = 512 * 1024 * 1024;

pub fn jet_data_query_read(path: &String) -> Result<String, Vec<jet_std::FieldError>> {
    use std::io::Read;

    let file = std::fs::File::open(path)
        .map_err(|error| jet_analytics_query_error(format!("could not read `{path}`: {error}")))?;
    let size = file
        .metadata()
        .map_err(|error| jet_analytics_query_error(format!("could not read `{path}`: {error}")))?
        .len();
    if size > JET_DATA_QUERY_MAX_BYTES {
        return Err(jet_analytics_query_error(format!(
            "could not read `{path}`: input exceeds the {}-byte query bound",
            JET_DATA_QUERY_MAX_BYTES
        )));
    }
    let mut bytes = Vec::new();
    file.take(JET_DATA_QUERY_MAX_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| jet_analytics_query_error(format!("could not read `{path}`: {error}")))?;
    if bytes.len() as u64 > JET_DATA_QUERY_MAX_BYTES {
        return Err(jet_analytics_query_error(format!(
            "could not read `{path}`: input grew beyond the {}-byte query bound",
            JET_DATA_QUERY_MAX_BYTES
        )));
    }
    String::from_utf8(bytes)
        .map_err(|error| jet_analytics_query_error(format!("could not read `{path}`: {error}")))
}

/// Validate a CSV header against the canonical SQL row-field traversal.
pub fn jet_data_query_validate_fields(
    fields: &[String],
    sql: &String,
) -> Result<(), Vec<jet_std::FieldError>> {
    let query = jet_analytics_query_spec(sql).map_err(jet_analytics_query_error)?;
    jet_sql_query::validate_sql_query_schema(
        &query,
        |field| fields.iter().any(|known| known == field),
        "analytics query",
    )
    .map_err(jet_analytics_query_error)
}


fn jet_analytics_query_spec(sql: &str) -> Result<JetAnalyticsQuery, String> {
    jet_sql_query::parse_sql_query(sql)
}
fn jet_analytics_type_id(type_name: &str) -> JetTableTypeId {
    match type_name {
        "bool" => JetTableTypeId::boolean(),
        "int" => JetTableTypeId::integer(),
        "float" => JetTableTypeId::float(),
        "text" => JetTableTypeId::string(),
        "Any" => JetTableTypeId::any(),
        other => JetTableTypeId::new(other),
    }
}

/// Query planning uses the same all-row schema inference as loader snapshots.
fn jet_analytics_schema(rows: &[jet_std::DataTree]) -> JetTableSchema {
    let tree = jet_std::DataTree::Array(rows.to_vec());
    let inferred = jet_std::DataSchema::infer(&tree, jet_std::DataFormat::JSON);
    let columns = inferred
        .columns
        .into_iter()
        .map(|column| JetTableColumn::new(column.name, jet_analytics_type_id(&column.type_name)))
        .collect();
    JetTableSchema::new(JetTableTypeId::new("DataTree"), columns)
}

fn jet_analytics_literal_type(value: &str) -> JetTableTypeId {
    let value = value.trim_matches('\'').trim_matches('"');
    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
        JetTableTypeId::boolean()
    } else if value.parse::<i64>().is_ok() {
        JetTableTypeId::integer()
    } else if value.parse::<f64>().is_ok() {
        JetTableTypeId::float()
    } else {
        JetTableTypeId::string()
    }
}
fn jet_analytics_result_schema(
    input: &JetTableSchema,
    spec: &JetAnalyticsQuery,
) -> JetTableSchema {
    let row_type = JetTableTypeId::new(format!("{}::sql", input.row_type.as_str()));
    let columns = spec
        .projection
        .iter()
        .flat_map(|item| match item {
            jet_sql_query::JetSqlSelectItem::Wildcard => input.columns.clone(),
            jet_sql_query::JetSqlSelectItem::Field { field, alias } => {
                let name = alias.clone().unwrap_or_else(|| field.clone());
                let ty = input
                    .column_named(field)
                    .map(|column| column.ty.clone())
                    .unwrap_or_else(JetTableTypeId::any);
                vec![JetTableColumn::new(name, ty)]
            }
            jet_sql_query::JetSqlSelectItem::Aggregate {
                function,
                field,
                alias,
            } => {
                let name = alias.clone().unwrap_or_else(|| match field {
                    Some(field) => format!("{}_{}", function.as_str(), field),
                    None => function.as_str().to_string(),
                });
                let ty = match function {
                    jet_sql_query::JetSqlAggregate::Count => JetTableTypeId::integer(),
                    jet_sql_query::JetSqlAggregate::Avg => JetTableTypeId::float(),
                    jet_sql_query::JetSqlAggregate::Sum => field
                        .as_ref()
                        .and_then(|field| input.column_named(field))
                        .map(|column| column.ty.clone())
                        .unwrap_or_else(JetTableTypeId::float),
                    jet_sql_query::JetSqlAggregate::Min
                    | jet_sql_query::JetSqlAggregate::Max => field
                        .as_ref()
                        .and_then(|field| input.column_named(field))
                        .map(|column| column.ty.clone())
                        .unwrap_or_else(JetTableTypeId::any),
                };
                vec![JetTableColumn::new(name, ty)]
            }
        })
        .collect();
    JetTableSchema::new(row_type, columns)
}

fn jet_analytics_select_columns(
    input: &JetTableSchema,
    spec: &JetAnalyticsQuery,
) -> Option<Vec<JetTableColumnId>> {
    if spec.has_aggregate() || !spec.group_by.is_empty() {
        return None;
    }
    spec.projection
        .iter()
        .map(|item| match item {
            jet_sql_query::JetSqlSelectItem::Field { field, alias: None } => {
                input.column_named(field).map(|column| column.id.clone())
            }
            jet_sql_query::JetSqlSelectItem::Wildcard => None,
            jet_sql_query::JetSqlSelectItem::Field {
                alias: Some(_), ..
            }
            | jet_sql_query::JetSqlSelectItem::Aggregate { .. } => None,
        })
        .collect()
}


fn jet_analytics_plan(
    rows: &[jet_std::DataTree],
    spec: &JetAnalyticsQuery,
) -> Result<JetTablePlan, String> {
    let schema = jet_analytics_schema(rows);
    jet_sql_query::validate_sql_query_schema(
        spec,
        |field| schema.column_named(field).is_some(),
        "analytics query",
    )?;
    let source = JetTableSource::new(spec.source.clone(), schema.clone()).with_rows(rows.len() as u128);
    let mut plan = JetTablePlan::from_source("data.query", source);
    let mut current_schema = schema.clone();
    if let Some((field, operator, value)) = &spec.filter {
        let literal_type = jet_analytics_literal_type(value);
        let callable = JetTableCallable::new(
            format!("query.where:{field}:{operator}"),
            current_schema.row_type.clone(),
            JetTableTypeId::boolean(),
        );
        let node = plan
            .append_filter(current_schema.clone(), callable)
            .map_err(|error| format!("analytics query plan rejected WHERE: {error}"))?;
        plan.nodes[node.index()].note =
            format!("field={field};operator={operator};value_type={}", literal_type.as_str());
    }
    if spec.has_aggregate() || !spec.group_by.is_empty() {
        let result_schema = jet_analytics_result_schema(&current_schema, spec);
        let input = plan.output;
        let callable = JetTableCallable::new(
            format!("query.group:{}", spec.group_by.len()),
            current_schema.row_type.clone(),
            result_schema.row_type.clone(),
        );
        let node = plan
            .append(
                JetTableOperation::Group,
                vec![input],
                result_schema.clone(),
                Some(callable),
            )
            .map_err(|error| format!("analytics query plan rejected GROUP BY: {error}"))?;
        plan.nodes[node.index()].note = format!(
            "keys={};projection={}",
            spec.group_by.len(),
            spec.projection
                .iter()
                .map(jet_sql_query::JetSqlSelectItem::output_name)
                .collect::<Vec<_>>()
                .join(",")
        );
        current_schema = result_schema;
    } else if !spec.is_identity_projection() {
        let result_schema = jet_analytics_result_schema(&current_schema, spec);
        if let Some(columns) = jet_analytics_select_columns(&current_schema, spec) {
            let node = plan
                .append_select(result_schema.clone(), columns)
                .map_err(|error| format!("analytics query plan rejected SELECT: {error}"))?;
            plan.nodes[node.index()].note = "explicit source-column projection".to_string();
        } else {
            let callable = JetTableCallable::new(
                format!(
                    "query.project:{}",
                    spec.projection
                        .iter()
                        .map(jet_sql_query::JetSqlSelectItem::output_name)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                current_schema.row_type.clone(),
                result_schema.row_type.clone(),
            );
            let node = plan
                .append_map(result_schema.clone(), callable)
                .map_err(|error| format!("analytics query plan rejected SELECT: {error}"))?;
            plan.nodes[node.index()].note = "aliased source-column projection".to_string();
        }
        current_schema = result_schema;
    }
    if let Some((field, descending)) = &spec.order {
        let callable = JetTableCallable::new(
            format!("query.order:{field}:{}", if *descending { "desc" } else { "asc" }),
            current_schema.row_type.clone(),
            JetTableTypeId::string(),
        );
        let node = plan
            .append_sort(current_schema.clone(), callable)
            .map_err(|error| format!("analytics query plan rejected ORDER BY: {error}"))?;
        plan.nodes[node.index()].note = format!(
            "field={field};direction={}",
            if *descending { "desc" } else { "asc" }
        );
    }
    if let Some(limit) = spec.limit {
        let node = plan
            .append_limit(current_schema, limit as u128)
            .map_err(|error| format!("analytics query plan rejected LIMIT: {error}"))?;
        plan.nodes[node.index()].note = format!("rows={limit}");
    }
    plan.validate()
        .map_err(|error| format!("analytics query plan rejected: {error}"))?;
    Ok(plan)
}


fn jet_analytics_cell_text(cell: &jet_std::DataTree) -> String {
    match cell {
        jet_std::DataTree::Text(value) => value.clone(),
        jet_std::DataTree::Int(value) => value.to_string(),
        jet_std::DataTree::Float(value) => value.to_string(),
        jet_std::DataTree::Bool(value) => value.to_string(),
        jet_std::DataTree::Null => String::new(),
        other => jet_std::render_datatree_json(other, false, 0),
    }
}
fn jet_analytics_sql_value(value: &jet_std::DataTree) -> jet_sql_query::JetSqlValue {
    match value {
        jet_std::DataTree::Null => jet_sql_query::JetSqlValue::Null,
        jet_std::DataTree::Bool(value) => jet_sql_query::JetSqlValue::Bool(*value),
        jet_std::DataTree::Int(value) => jet_sql_query::JetSqlValue::Int(*value),
        jet_std::DataTree::Float(value) => jet_sql_query::JetSqlValue::Float(*value),
        jet_std::DataTree::Number(value) => jet_sql_query::JetSqlValue::literal(value),
        jet_std::DataTree::TypedText(value) | jet_std::DataTree::Text(value) => {
            jet_sql_query::JetSqlValue::Text(value.clone())
        }
        other => jet_sql_query::JetSqlValue::Text(jet_std::render_datatree_json(other, false, 0)),
    }
}

fn jet_analytics_sql_tree(value: jet_sql_query::JetSqlValue) -> jet_std::DataTree {
    match value {
        jet_sql_query::JetSqlValue::Null => jet_std::DataTree::Null,
        jet_sql_query::JetSqlValue::Bool(value) => jet_std::DataTree::Bool(value),
        jet_sql_query::JetSqlValue::Int(value) => jet_std::DataTree::Int(value),
        jet_sql_query::JetSqlValue::Float(value) => jet_std::DataTree::Float(value),
        jet_sql_query::JetSqlValue::Text(value) => jet_std::DataTree::Text(value),
    }
}

fn jet_data_query_execute_trees(
    rows: &[jet_std::DataTree],
    spec: &JetAnalyticsQuery,
) -> Result<Vec<jet_std::DataTree>, String> {
    let fields = jet_analytics_schema(rows)
        .columns
        .into_iter()
        .map(|column| column.name)
        .collect::<Vec<_>>();
    let result = jet_sql_query::execute_sql_rows(&fields, rows.len(), spec, |index, field| {
        jet_std::datatree_get(&rows[index], field).map(jet_analytics_sql_value)
    })?;
    Ok(result
        .into_iter()
        .map(|row| {
            jet_std::DataTree::Object(
                row.fields
                    .into_iter()
                    .map(|(name, value)| (name, jet_analytics_sql_tree(value)))
                    .collect(),
            )
        })
        .collect())
}



/// Return selected input positions. Keeping positions lets typed wrappers
/// apply exactly the same filter/order/limit without a descriptor ABI.
///
/// A projection or aggregate changes the row shape and therefore cannot be
/// represented as positions in a typed `T`; those callers use
/// `jet_data_query_trees`, which materializes the same shared result rows.
pub fn jet_data_query_indices(
    rows: &[jet_std::DataTree],
    sql: &String,
) -> Result<Vec<usize>, Vec<jet_std::FieldError>> {
    let spec = jet_analytics_query_spec(sql).map_err(jet_analytics_query_error)?;
    let plan = jet_analytics_plan(rows, &spec).map_err(jet_analytics_query_error)?;
    plan
        .inspect()
        .map_err(|error| jet_analytics_query_error(format!(
            "analytics query plan inspection rejected: {error}"
        )))?;
    if spec.needs_projection_execution() {
        return Err(jet_analytics_query_error(
            "SQL projection/grouping changes the row shape; use the DataTree query result",
        ));
    }
    let selected = jet_sql_query::select_sql_indices(rows.len(), &spec, |index, field| {
        jet_std::datatree_get(&rows[index], field).map(jet_analytics_cell_text)
    });
    Ok(selected)
}

/// Shared non-generic Prelude entry used by resident adapters. Explicit
/// projections and grouped aggregates become ordered DataTree objects; the
/// filter/order/limit-only path preserves source rows and source identity.
pub fn jet_data_query_trees(
    rows: &Vec<jet_std::DataTree>,
    sql: &String,
) -> Result<Vec<jet_std::DataTree>, Vec<jet_std::FieldError>> {
    let spec = jet_analytics_query_spec(sql).map_err(jet_analytics_query_error)?;
    if spec.needs_projection_execution() {
        let plan = jet_analytics_plan(rows, &spec).map_err(jet_analytics_query_error)?;
        plan
            .inspect()
            .map_err(|error| jet_analytics_query_error(format!(
                "analytics query plan inspection rejected: {error}"
            )))?;
        return jet_data_query_execute_trees(rows, &spec).map_err(jet_analytics_query_error);
    }
    let selected = jet_data_query_indices(rows, sql)?;
    Ok(selected.into_iter().map(|index| rows[index].clone()).collect())
}
