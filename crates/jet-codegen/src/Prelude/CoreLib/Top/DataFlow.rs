// D-DATAFLOW1=A: typed pull streams, DataLimits, DataError analytics (edition 2027).

/// D-COLUMNAR-BOUNDARY1=A: the checked Arrow owner is connected to the safe
/// `JetStd` carrier only in the demand-emitted Arrow flow. The always-present
/// type surface stores an operation object; this module supplies the checked
/// Foundation owner and its row adapter without exposing C pointers.
impl<T> jet_std::DataArrowBatch<T>
where
    T: crate::jet_arrow_data::ArrowRow + Clone + 'static,
{
    pub fn new(owner: crate::jet_arrow_data::ColumnBatch<T>) -> Self {
        Self::from_ops(Box::new(owner))
    }
}

impl<T> jet_std::DataArrowOps<T> for crate::jet_arrow_data::ColumnBatch<T>
where
    T: crate::jet_arrow_data::ArrowRow + Clone + 'static,
{
    fn row_count(&self) -> usize {
        self.report.rows
    }

    fn row(&self, index: usize) -> Result<T, String> {
        crate::jet_arrow_data::ArrowImported::<T>::row(self, index)
            .map_err(|error| error.to_string())
    }
}

/// Plotting accepts ordinary typed lists.  The list is the canonical materialized
/// value; schema/source facts are derived from its checked row type.
impl<T: 'static> JetDataPlotRows for Vec<T> {
    type Row = T;

    fn jet_data_plot_parts(
        self,
    ) -> (
        Vec<Self::Row>,
        JetDataPlotSourceFacts,
        JetDataPlotSchema,
        jet_std::DataLimits,
    ) {
        let row_type = std::any::type_name::<T>();
        let identity = format!("data.list:{row_type}");
        let schema = JetDataPlotSchema::new(identity.clone(), row_type, Vec::new());
        let source = JetDataPlotSourceFacts::new(
            identity.clone(),
            identity.clone(),
            identity.clone(),
            row_type,
            self.len() as i64,
            identity,
            "data.list",
        );
        (self, source, schema, jet_std::DataLimits::safe())
    }
}

fn jet_data_from_encoding(
    operation: &str,
    row: JetOutcome<i64, JetAbsent>,
    enc: jet_std::EncodingError,
) -> jet_std::DataError {
    let kind = match enc.kind {
        jet_std::EncodingErrorKind::Limit => jet_std::DataErrorKind::Limit,
        jet_std::EncodingErrorKind::IO => jet_std::DataErrorKind::IO,
        jet_std::EncodingErrorKind::State => jet_std::DataErrorKind::State,
        _ => jet_std::DataErrorKind::Decode,
    };
    jet_std::DataError {
        kind,
        operation: operation.to_string(),
        row,
        column: enc.column,
        index: Err(JetAbsent),
        reason: enc.reason.clone(),
        cause: Ok(enc),
    }
}

fn jet_data_limits_validate(limits: &jet_std::DataLimits) -> Result<(), jet_std::DataError> {
    jet_encoding_validate_limits(&limits.encoding).map_err(|enc| {
        jet_data_from_encoding("DataLimits", Err(JetAbsent), enc)
    })?;
    jet_data_kernel_validate_limits(&jet_data_kernel_limits(
        limits.max_groups,
        limits.max_sort_rows,
        limits.max_join_rows,
        limits.max_output_rows,
    ))
}

fn jet_data_plot_error(error: DataPlotError) -> jet_std::DataError {
    let kind = match error.kind {
        "NonFinite" => jet_std::DataErrorKind::NonFinite,
        _ => jet_std::DataErrorKind::InvalidArgument,
    };
    jet_data_error_at(kind, error.operation, jet_outcome_of(error.index), error.reason)
}

fn jet_data_typed_plot_error(error: JetDataPlotError) -> jet_std::DataError {
    error.into_data_error()
}
fn jet_data_canonical_unary_plan<T>(
    operation: JetTableOperation,
    operation_name: &str,
    rows: usize,
    result_type: JetTableTypeId,
    output_columns: Vec<JetTableColumn>,
) -> Result<JetTablePlan, jet_std::DataError> {
    let row_type = JetTableTypeId::new(std::any::type_name::<T>());
    let input_schema = JetTableSchema::new(row_type.clone(), Vec::new());
    let source = JetTableSource::new(
        format!("data.{operation_name}.source"),
        input_schema.clone(),
    )
    .with_rows(rows as u128);
    let mut plan = JetTablePlan::from_source(format!("data.{operation_name}"), source);
    let output_schema = if operation == JetTableOperation::Sort {
        input_schema.clone()
    } else {
        JetTableSchema::new(result_type.clone(), output_columns)
    };
    let callable_result = if operation == JetTableOperation::Sort {
        JetTableTypeId::string()
    } else {
        result_type
    };
    let callable = JetTableCallable::new(
        format!("data.{operation_name}"),
        row_type,
        callable_result,
    );
    let node = match operation {
        JetTableOperation::Sort => plan
            .append_sort(output_schema, callable)
            .map_err(|error| jet_data_error(
                jet_std::DataErrorKind::InvalidArgument,
                operation_name,
                format!("canonical table plan rejected: {error}"),
            ))?,
        JetTableOperation::Group => {
            let input = plan.output;
            plan.append(
                JetTableOperation::Group,
                vec![input],
                output_schema,
                Some(callable),
            )
            .map_err(|error| jet_data_error(
                jet_std::DataErrorKind::InvalidArgument,
                operation_name,
                format!("canonical table plan rejected: {error}"),
            ))?
        }
        _ => {
            return Err(jet_data_error(
                jet_std::DataErrorKind::InvalidArgument,
                operation_name,
                "unsupported canonical unary operation",
            ));
        }
    };
    plan.nodes[node.index()].note = format!("rows={rows}");
    plan.validate().map_err(|error| {
        jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            operation_name,
            format!("canonical table plan rejected: {error}"),
        )
    })?;
    Ok(plan)
}

fn jet_data_canonical_join_plan<T, U>(
    operation_name: &str,
    kind: JetTableJoinKind,
    left_rows: usize,
    right_rows: usize,
) -> Result<JetTablePlan, jet_std::DataError> {
    let row_type = JetTableTypeId::new(std::any::type_name::<T>());
    let input_schema = JetTableSchema::new(row_type, Vec::new());
    let source = JetTableSource::new(
        format!("data.{operation_name}.left"),
        input_schema,
    )
    .with_rows(left_rows as u128);
    let mut plan = JetTablePlan::from_source(format!("data.{operation_name}"), source);
    let left_type = JetTableTypeId::new(std::any::type_name::<T>());
    let right_type = JetTableTypeId::new(std::any::type_name::<U>());
    let right_schema = JetTableSchema::new(right_type.clone(), Vec::new());
    let right_source = JetTableSource::new(
        format!("data.{operation_name}.right"),
        right_schema,
    )
    .with_rows(right_rows as u128);
    let right = plan
        .append_source(right_source)
        .map_err(|error| {
            jet_data_error(
                jet_std::DataErrorKind::InvalidArgument,
                operation_name,
                format!("canonical table plan rejected: {error}"),
            )
        })?;
    let output_schema = JetTableSchema::new(
        JetTableTypeId::new(format!(
            "DataJoin<{},{}>",
            left_type.as_str(),
            right_type.as_str()
        )),
        vec![
            JetTableColumn::new("left", left_type),
            JetTableColumn::new(
                "right",
                if kind == JetTableJoinKind::Left {
                    JetTableTypeId::new(format!("JetOutcome<{},JetAbsent>", right_type.as_str()))
                } else {
                    right_type
                },
            ),
        ],
    );
    let node = plan
        .append_join(right, output_schema, kind, Vec::new())
        .map_err(|error| {
            jet_data_error(
                jet_std::DataErrorKind::InvalidArgument,
                operation_name,
                format!("canonical table plan rejected: {error}"),
            )
        })?;
    // Join output cardinality depends on adapter-owned key values; keep that
    // result explicitly unavailable even though both scan bounds are known.
    plan.nodes[node.index()].facts = JetTableFacts::unknown(2);
    plan.nodes[node.index()].note =
        format!("left_rows={left_rows};right_rows={right_rows};right_type={}", std::any::type_name::<U>());
    plan.validate().map_err(|error| {
        jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            operation_name,
            format!("canonical table plan rejected: {error}"),
        )
    })?;
    Ok(plan)
}
fn jet_data_plan_inspect(
    plan: &JetTablePlan,
    operation: &str,
) -> Result<(), jet_std::DataError> {
    plan.inspect().map(|_| ()).map_err(|error| {
        jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            operation,
            format!("canonical table inspection rejected: {error}"),
        )
    })
}



fn jet_data_pivot_sum_checked<T, FR, FC, FV>(
    rows: &Vec<T>,
    row_key: FR,
    col_key: FC,
    value: FV,
    limits: &jet_std::DataLimits,
) -> Result<Vec<jet_std::DataPivotCell>, jet_std::DataError>
where
    T: Clone,
    FR: Fn(T) -> String,
    FC: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_limits_validate(limits)?;
    let plan = jet_data_canonical_unary_plan::<T>(
        JetTableOperation::Group,
        "pivot_sum",
        rows.len(),
        JetTableTypeId::new("DataPivotCell"),
        vec![
            JetTableColumn::new("row_key", JetTableTypeId::string()),
            JetTableColumn::new("column_key", JetTableTypeId::string()),
            JetTableColumn::new("count", JetTableTypeId::integer()),
            JetTableColumn::new("sum", JetTableTypeId::float()),
            JetTableColumn::new("mean", JetTableTypeId::float()),
        ],
    )?;
    jet_data_plan_inspect(&plan, "pivot_sum")?;
    let keyed_rows = rows
        .iter()
        .cloned()
        .map(|row| (row_key(row.clone()), col_key(row.clone()), value(row)))
        .collect::<Vec<_>>();
    let cells = jet_data_pivot_sum_values(
        &keyed_rows,
        &jet_data_kernel_limits(
            limits.max_groups,
            limits.max_sort_rows,
            limits.max_join_rows,
            limits.max_output_rows,
        ),
    )?;
    Ok(cells
        .into_iter()
        .map(|cell| jet_std::DataPivotCell {
            row_key: cell.row_key,
            column_key: cell.column_key,
            count: cell.count,
            sum: cell.sum,
            mean: cell.mean,
        })
        .collect())
}
fn jet_data_pivot_sum_checked_default<T, FR, FC, FV>(
    rows: &Vec<T>,
    row_key: FR,
    col_key: FC,
    value: FV,
) -> Result<Vec<jet_std::DataPivotCell>, jet_std::DataError>
where
    T: Clone,
    FR: Fn(T) -> String,
    FC: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_pivot_sum_checked(
        rows,
        row_key,
        col_key,
        value,
        &jet_std::DataLimits::safe(),
    )
}



fn jet_data_inner_join_checked<T, U, FL, FR>(
    left: &Vec<T>,
    right: &Vec<U>,
    left_key: FL,
    right_key: FR,
    limits: &jet_std::DataLimits,
) -> Result<Vec<jet_std::DataJoin<T, U>>, jet_std::DataError>
where
    T: Clone,
    U: Clone,
    FL: Fn(T) -> String,
    FR: Fn(U) -> String,
{
    jet_data_limits_validate(limits)?;
    let plan = jet_data_canonical_join_plan::<T, U>(
        "inner_join",
        JetTableJoinKind::Inner,
        left.len(),
        right.len(),
    )?;
    jet_data_plan_inspect(&plan, "inner_join")?;
    let left_keys = left
        .iter()
        .cloned()
        .map(|row| left_key(row))
        .collect::<Vec<_>>();
    let right_keys = right
        .iter()
        .cloned()
        .map(|row| right_key(row))
        .collect::<Vec<_>>();
    let indices = jet_data_join_indices(
        &left_keys,
        &right_keys,
        false,
        &jet_data_kernel_limits(
            limits.max_groups,
            limits.max_sort_rows,
            limits.max_join_rows,
            limits.max_output_rows,
        ),
    )?;
    let mut joined = Vec::with_capacity(indices.len());
    for index in indices {
        let Some(right_index) = index.right else {
            return Err(jet_data_error(
                jet_std::DataErrorKind::State,
                "inner_join",
                "inner join kernel returned an absent right row",
            ));
        };
        joined.push(jet_std::DataJoin {
            left: left[index.left].clone(),
            right: right[right_index].clone(),
        });
    }
    Ok(joined)
}

fn jet_data_inner_join_checked_default<T, U, FL, FR>(
    left: &Vec<T>,
    right: &Vec<U>,
    left_key: FL,
    right_key: FR,
) -> Result<Vec<jet_std::DataJoin<T, U>>, jet_std::DataError>
where
    T: Clone,
    U: Clone,
    FL: Fn(T) -> String,
    FR: Fn(U) -> String,
{
    jet_data_inner_join_checked(left, right, left_key, right_key, &jet_std::DataLimits::safe())
}

fn jet_data_left_join_checked<T, U, FL, FR>(
    left: &Vec<T>,
    right: &Vec<U>,
    left_key: FL,
    right_key: FR,
    limits: &jet_std::DataLimits,
) -> Result<Vec<jet_std::DataJoin<T, JetOutcome<U, JetAbsent>>>, jet_std::DataError>
where
    T: Clone,
    U: Clone,
    FL: Fn(T) -> String,
    FR: Fn(U) -> String,
{
    jet_data_limits_validate(limits)?;
    let plan = jet_data_canonical_join_plan::<T, U>(
        "left_join",
        JetTableJoinKind::Left,
        left.len(),
        right.len(),
    )?;
    jet_data_plan_inspect(&plan, "left_join")?;
    let left_keys = left
        .iter()
        .cloned()
        .map(|row| left_key(row))
        .collect::<Vec<_>>();
    let right_keys = right
        .iter()
        .cloned()
        .map(|row| right_key(row))
        .collect::<Vec<_>>();
    let indices = jet_data_join_indices(
        &left_keys,
        &right_keys,
        true,
        &jet_data_kernel_limits(
            limits.max_groups,
            limits.max_sort_rows,
            limits.max_join_rows,
            limits.max_output_rows,
        ),
    )?;
    Ok(indices
        .into_iter()
        .map(|index| jet_std::DataJoin {
            left: left[index.left].clone(),
            right: index
                .right
                .map(|right_index| right[right_index].clone())
                .ok_or(JetAbsent),
        })
        .collect())
}

fn jet_data_left_join_checked_default<T, U, FL, FR>(
    left: &Vec<T>,
    right: &Vec<U>,
    left_key: FL,
    right_key: FR,
) -> Result<Vec<jet_std::DataJoin<T, JetOutcome<U, JetAbsent>>>, jet_std::DataError>
where
    T: Clone,
    U: Clone,
    FL: Fn(T) -> String,
    FR: Fn(U) -> String,
{
    jet_data_left_join_checked(left, right, left_key, right_key, &jet_std::DataLimits::safe())
}

fn jet_data_collect_rows_checked<T: Clone>(
    rows_source: &jet_std::DataQueryRows<T>,
    limits: &jet_std::DataLimits,
) -> Result<Vec<T>, jet_std::DataError> {
    jet_data_limits_validate(limits)?;
    let max_output_rows = usize::try_from(limits.max_output_rows).map_err(|_| {
        jet_data_error(
            jet_std::DataErrorKind::Limit,
            "query.collect",
            "max_output_rows cannot be represented by this target",
        )
    })?;
    let mut cursor = rows_source.cursor();
    let mut rows = Vec::with_capacity(max_output_rows.min(256));
    loop {
        let remaining = max_output_rows.saturating_sub(rows.len());
        let request = remaining.saturating_add(1).min(256);
        let batch = cursor
            .next_batch_with_sort_limit(request, Some(limits.max_sort_rows))
            .map_err(|limit| {
                jet_data_error(
                    jet_std::DataErrorKind::Limit,
                    limit.operation,
                    format!("max_sort_rows {} exceeded", limit.limit),
                )
            })?;
        if batch.len() > remaining {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                "query.collect",
                format!("max_output_rows {} exceeded", limits.max_output_rows),
            ));
        }
        let exhausted = batch.len() < request;
        rows.try_reserve_exact(batch.len()).map_err(|_| {
            jet_data_error(
                jet_std::DataErrorKind::Limit,
                "query.collect",
                "output allocation exceeded the configured bound",
            )
        })?;
        rows.extend(batch);
        if exhausted {
            break;
        }
    }
    Ok(rows)
}

/// D-QUERY-RETAIN1=A: construct the single deferred query carrier from an
/// ordinary list.  The list remains owned by the query and is not reopened.
fn jet_data_query<T: Clone + 'static>(
    rows: &Vec<T>,
) -> jet_std::DataQuery<T> {
    let plan = jet_table_plan_from_descriptors(
        std::any::type_name::<T>(),
        &[],
        rows.len() as u128,
    )
    .unwrap_or_else(|error| panic!("invalid compiler query metadata: {error}"));
    jet_std::DataQuery::from_rows(rows.clone(), 0, plan)
}
fn jet_data_track<T, K, F>(
    rows: &Vec<T>,
    key: F,
) -> Result<jet_std::DataTracked<T, K>, jet_std::DataError>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    F: Fn(T) -> K + 'static,
{
    jet_std::DataTracked::new(rows.clone(), key)
}

fn jet_data_query_tracked<T, K>(
    tracked: &jet_std::DataTracked<T, K>,
) -> jet_std::DataQuery<T>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
{
    jet_std::DataQuery::from_tracked(tracked)
}
fn jet_data_track_insert<T, K>(
    tracked: &jet_std::DataTracked<T, K>,
    row: T,
) -> Result<(), jet_std::DataError>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
{
    tracked.insert(row)
}

fn jet_data_track_replace<T, K>(
    tracked: &jet_std::DataTracked<T, K>,
    key: K,
    row: T,
) -> Result<(), jet_std::DataError>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
{
    tracked.replace(key, row)
}

fn jet_data_track_remove<T, K>(
    tracked: &jet_std::DataTracked<T, K>,
    key: K,
) -> Result<(), jet_std::DataError>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
{
    tracked.remove(key)
}


/// D-QUERY-RETAIN1=A / D-DATAFLOW1=A: a one-shot `DataStream` becomes a
/// single-use query. The reader is consumed by the first `collect`; a second
/// collect reports a `State` error instead of reopening the source.
fn jet_data_query_stream<T: __jet_Decode + Clone + 'static>(
    stream: jet_std::DataStream,
) -> jet_std::DataQuery<T> {
    let mut stream = stream;
    jet_std::DataQuery::from_computation(vec!["stream".to_string()], move || {
        jet_data_stream_collect::<T>(&mut stream)
    })
}

fn jet_data_query_plan<T>(query: &jet_std::DataQuery<T>) -> Vec<String> {
    query.plan()
}

fn jet_data_grouped_steps<T, K>(
    grouped: &jet_std::DataGroupedQuery<T, K>,
    reducer: &str,
) -> Vec<String> {
    let mut steps = grouped.query.plan();
    steps.push("group_by".to_string());
    steps.push(reducer.to_string());
    steps
}

fn jet_data_query_filter<T: Clone + 'static, F>(
    query: &jet_std::DataQuery<T>,
    predicate: F,
) -> jet_std::DataQuery<T>
where
    F: Fn(T) -> bool + 'static,
{
    query.filter(predicate)
}

fn jet_data_query_sort_by<T: Clone + 'static, F>(
    query: &jet_std::DataQuery<T>,
    key: F,
) -> jet_std::DataQuery<T>
where
    F: Fn(T) -> String + 'static,
{
    query.sort_by(key)
}

fn jet_data_query_apply_operations<T: Clone>(
    rows: &mut Vec<T>,
    operations: &[jet_std::DataQueryOperation<T>],
    limits: &jet_std::DataLimits,
) -> Result<(), jet_std::DataError> {
    for operation in operations {
        match operation {
            jet_std::DataQueryOperation::Filter(predicate) => {
                rows.retain(|row| predicate(row.clone()));
            }
            jet_std::DataQueryOperation::SortBy(key) => {
                if rows.len() as i64 > limits.max_sort_rows {
                    return Err(jet_data_error(
                        jet_std::DataErrorKind::Limit,
                        "query.sort_by",
                        format!("max_sort_rows {} exceeded", limits.max_sort_rows),
                    ));
                }
                rows.sort_by_key(|row| key(row.clone()));
            }
        }
    }
    if rows.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "query.collect",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(())
}

fn jet_data_query_collect<T: Clone + 'static>(
    query: &jet_std::DataQuery<T>,
) -> Result<Vec<T>, jet_std::DataError> {
    let limits = jet_std::DataLimits::safe();
    if let Some(rows_source) = query.rows_frame() {
        return jet_data_collect_rows_checked(&rows_source, &limits);
    }
    if query.is_recomputed() {
        let compute = query.recomputation().ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::State,
                "query.collect",
                "recomputable query has no producer",
            )
        })?;
        let mut rows = compute?;
        jet_data_query_apply_operations(&mut rows, &query.operations, &limits)?;
        return Ok(rows);
    }
    if query.is_computed() {
        let compute = query.take_computation().ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::State,
                "query.collect",
                "query has already been collected",
            )
        })?;
        let mut rows = compute()?;
        jet_data_query_apply_operations(&mut rows, &query.operations, &limits)?;
        return Ok(rows);
    }
    Err(jet_data_error(
        jet_std::DataErrorKind::State,
        "query.collect",
        "query source is unavailable",
    ))
}

fn jet_data_query_watch<T: Clone + 'static>(
    query: &jet_std::DataQuery<T>,
) -> Result<jet_std::DataWatch<T>, jet_std::DataError> {
    jet_std::DataWatch::from_query(query.clone())
}

fn jet_data_watch_get<T: Clone + 'static>(
    watch: &jet_std::DataWatch<T>,
) -> Result<Vec<T>, jet_std::DataError> {
    watch.get()
}

fn jet_data_watch_status<T: Clone + 'static>(
    watch: &jet_std::DataWatch<T>,
) -> jet_std::DataWatchStatus {
    watch.status()
}

fn jet_data_watch_cancel<T: Clone + 'static>(watch: &jet_std::DataWatch<T>) {
    watch.cancel();
}

fn jet_data_query_group_by<T: Clone + 'static, K, F>(
    query: &jet_std::DataQuery<T>,
    key: F,
) -> jet_std::DataGroupedQuery<T, K>
where
    K: 'static,
    F: Fn(T) -> K + 'static,
{
    jet_std::DataGroupedQuery {
        query: query.clone(),
        key: std::sync::Arc::new(key),
    }
}

fn jet_data_group_sum_materialized<T, K, V, FK, FV>(
    query: &jet_std::DataQuery<T>,
    key: &FK,
    value: &FV,
) -> Result<Vec<jet_std::GroupValue<K, V>>, jet_std::DataError>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    V: Clone + std::ops::Add<Output = V> + 'static,
    FK: ?Sized + Fn(T) -> K,
    FV: ?Sized + Fn(T) -> Result<V, jet_std::DataError>,
{
    let rows = jet_data_query_collect(query)?;
    let limits = jet_std::DataLimits::safe();
    let mut groups: Vec<(K, V)> = Vec::new();
    for row in rows {
        let group_key = key(row.clone());
        let group_value = value(row)?;
        if let Some((_, total)) = groups.iter_mut().find(|(existing, _)| existing == &group_key)
        {
            *total = total.clone() + group_value;
        } else {
            if groups.len() as i64 >= limits.max_groups {
                return Err(jet_data_error(
                    jet_std::DataErrorKind::Limit,
                    "query.group_by.sum",
                    format!("max_groups {} exceeded", limits.max_groups),
                ));
            }
            groups.push((group_key, group_value));
        }
    }
    if groups.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "query.group_by.sum",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(groups
        .into_iter()
        .map(|(key, value)| jet_std::GroupValue { key, value })
        .collect())
}

#[derive(Clone)]
struct JetDataGroupBucket<K, V> {
    key: K,
    rank: (Option<String>, u64),
    members: Vec<(u64, V, Option<String>)>,
}
#[derive(Clone)]
struct JetDataGroupWatchState<K, V> {
    buckets: Vec<JetDataGroupBucket<K, V>>,
    revision: u64,
    values: Vec<jet_std::GroupValue<K, V>>,
}

struct JetDataGroupWatchSource<T, K, V> {
    source: std::rc::Rc<dyn jet_std::DataTrackedReader<T>>,
    operations: Vec<jet_std::DataQueryOperation<T>>,
    key: std::sync::Arc<dyn Fn(T) -> K>,
    value: std::sync::Arc<dyn Fn(T) -> Result<V, jet_std::DataError>>,
    aggregate: std::sync::Arc<
        dyn Fn(&[V]) -> Result<V, jet_std::DataError>,
    >,
    operation: &'static str,
    mode: &'static str,
    recomputed: bool,
    state: std::cell::RefCell<Option<JetDataGroupWatchState<K, V>>>,
}

impl<T: Clone + 'static, K: Clone + PartialEq + 'static, V: Clone + 'static>
    JetDataGroupWatchSource<T, K, V>
{
    fn accepts(&self, row: &T) -> (bool, Option<String>) {
        let mut sort_key = None;
        for operation in &self.operations {
            match operation {
                jet_std::DataQueryOperation::Filter(predicate) => {
                    if !(predicate)(row.clone()) {
                        return (false, None);
                    }
                }
                jet_std::DataQueryOperation::SortBy(key) => {
                    // Group membership is independent of order, but preserve
                    // the checked callback's evaluation contract and final key.
                    sort_key = Some(key(row.clone()));
                }
            }
        }
        (true, sort_key)
    }
    fn check_sort_limit(&self) -> Result<(), jet_std::DataError> {
        if self
            .operations
            .iter()
            .any(|operation| matches!(operation, jet_std::DataQueryOperation::SortBy(_)))
        {
            let limit = jet_std::DataLimits::safe().max_sort_rows;
            if self.source.retained_rows() as i64 > limit {
                return Err(jet_data_error(
                    jet_std::DataErrorKind::Limit,
                    self.operation,
                    format!("max_sort_rows {limit} exceeded"),
                ));
            }
        }
        Ok(())
    }

    fn add(
        &self,
        state: &mut JetDataGroupWatchState<K, V>,
        order: u64,
        row: T,
    ) -> Result<(), jet_std::DataError> {
        let (accepted, sort_key) = self.accepts(&row);
        if !accepted {
            return Ok(());
        }
        let group_key = (self.key)(row.clone());
        let group_value = (self.value)(row)?;
        let rank = (sort_key.clone(), order);
        if let Some(bucket) = state
            .buckets
            .iter_mut()
            .find(|bucket| bucket.key == group_key)
        {
            let position = bucket
                .members
                .iter()
                .position(|(id, _, _)| *id > order)
                .unwrap_or(bucket.members.len());
            bucket.members.insert(position, (order, group_value, sort_key));
            if rank < bucket.rank {
                bucket.rank = rank;
            }
            return Ok(());
        }
        let limits = jet_std::DataLimits::safe();
        if state.buckets.len() as i64 >= limits.max_groups {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                self.operation,
                format!("max_groups {} exceeded", limits.max_groups),
            ));
        }
        state.buckets.push(JetDataGroupBucket {
            key: group_key,
            rank,
            members: vec![(order, group_value, sort_key)],
        });
        Ok(())
    }

    fn remove(&self, state: &mut JetDataGroupWatchState<K, V>, order: u64) {
        let mut empty = None;
        for (index, bucket) in state.buckets.iter_mut().enumerate() {
            if let Some(member) = bucket
                .members
                .iter()
                .position(|(id, _, _)| *id == order)
            {
                bucket.members.remove(member);
                if bucket.members.is_empty() {
                    empty = Some(index);
                } else {
                    bucket.rank = bucket
                        .members
                        .iter()
                        .map(|(id, _, key)| (key.clone(), *id))
                        .min()
                        .expect("non-empty group has a rank");
                }
                break;
            }
        }
        if let Some(index) = empty {
            state.buckets.remove(index);
        }
    }

    fn materialize(
        &self,
        state: &JetDataGroupWatchState<K, V>,
    ) -> Result<Vec<jet_std::GroupValue<K, V>>, jet_std::DataError> {
        let limits = jet_std::DataLimits::safe();
        if state.buckets.len() as i64 > limits.max_output_rows {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                self.operation,
                format!("max_output_rows {} exceeded", limits.max_output_rows),
            ));
        }
        let mut buckets = state.buckets.iter().collect::<Vec<_>>();
        buckets.sort_by(|left, right| left.rank.cmp(&right.rank));
        buckets
            .into_iter()
            .map(|bucket| {
                Ok(jet_std::GroupValue {
                    key: bucket.key.clone(),
                    value: (self.aggregate)(
                        &bucket
                            .members
                            .iter()
                            .map(|(_, value, _)| value.clone())
                            .collect::<Vec<_>>(),
                    )?,
                })
            })
            .collect()
    }

    fn entries(
        &self,
        values: Vec<jet_std::GroupValue<K, V>>,
    ) -> Vec<jet_std::DataWatchEntry<jet_std::GroupValue<K, V>>> {
        values
            .into_iter()
            .enumerate()
            .map(|(order, value)| jet_std::DataWatchEntry {
                order: order as u64,
                key: None,
                value,
            })
            .collect()
    }
}

impl<T: Clone + 'static, K: Clone + PartialEq + 'static, V: Clone + 'static>
    jet_std::DataWatchSource<jet_std::GroupValue<K, V>>
    for JetDataGroupWatchSource<T, K, V>
{
    fn initial(
        &self,
    ) -> Result<
        jet_std::DataWatchMaterialized<jet_std::GroupValue<K, V>>,
        jet_std::DataError,
    > {
        self.check_sort_limit()?;
        let (source_revision, entries) = self.source.snapshot_entries_at_revision();
        let mut state = JetDataGroupWatchState {
            buckets: Vec::new(),
            revision: source_revision,
            values: Vec::new(),
        };
        for (order, row) in entries {
            self.add(&mut state, order, row)?;
        }
        state.values = self.materialize(&state)?;
        let revision = state.revision;
        let entries = self.entries(state.values.clone());
        *self.state.borrow_mut() = Some(state);
        Ok(jet_std::DataWatchMaterialized {
            entries,
            revision,
            mode: self.mode,
            recomputed: false,
            changed: true,
            retained_rows: self.source.retained_rows(),
        })
    }

    fn refresh(
        &self,
        revision: u64,
        prior: &[jet_std::DataWatchEntry<jet_std::GroupValue<K, V>>],
    ) -> Result<
        jet_std::DataWatchMaterialized<jet_std::GroupValue<K, V>>,
        jet_std::DataError,
    > {
        self.check_sort_limit()?;
        let mut state = self.state.borrow_mut();
        let state = state.as_mut().ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::State,
                self.operation,
                "group watch was not initialized",
            )
        })?;
        if revision > state.revision {
            return Err(jet_data_error(
                jet_std::DataErrorKind::StaleRevision,
                self.operation,
                "watch revision is newer than its maintained group state",
            ));
        }
        if self.source.revision() == state.revision {
            return Ok(jet_std::DataWatchMaterialized {
                entries: self.entries(state.values.clone()),
                revision: state.revision,
                mode: self.mode,
                recomputed: false,
                changed: false,
                retained_rows: self.source.retained_rows(),
            });
        }
        let changes = self.source.changes_since(state.revision)?;
        let mut next = state.clone();
        for change in changes {
            match change.kind {
                jet_std::DataTrackedChangeKind::Insert { row } => {
                    self.add(&mut next, change.order, row)?;
                }
                jet_std::DataTrackedChangeKind::Replace { after, .. } => {
                    self.remove(&mut next, change.order);
                    self.add(&mut next, change.order, after)?;
                }
                jet_std::DataTrackedChangeKind::Remove { .. } => {
                    self.remove(&mut next, change.order);
                }
            }
        }
        next.revision = self.source.revision();
        let values = self.materialize(&next)?;
        next.values = values;
        let entries = self.entries(next.values.clone());
        let next_revision = next.revision;
        *state = next;
        Ok(jet_std::DataWatchMaterialized {
            entries,
            revision: next_revision,
            mode: self.mode,
            recomputed: self.recomputed,
            changed: true,
            retained_rows: self.source.retained_rows(),
        })
    }
}

/// A stable snapshot adapter used when one side of a maintained join is
/// ordinary query data. It materializes that side once, then lets the keyed
/// join source react only to the maintained side's revisions.
fn jet_data_static_query_entries<T: Clone + 'static>(
    query: &jet_std::DataQuery<T>,
    cache: &std::cell::RefCell<Option<Vec<jet_std::DataWatchEntry<T>>>>,
) -> Result<Vec<jet_std::DataWatchEntry<T>>, jet_std::DataError> {
    if let Some(entries) = cache.borrow().as_ref() {
        return Ok(entries.clone());
    }
    let rows = jet_data_query_collect(query)?;
    let entries = rows
        .into_iter()
        .enumerate()
        .map(|(order, value)| jet_std::DataWatchEntry {
            order: order as u64,
            key: None,
            value,
        })
        .collect::<Vec<_>>();
    *cache.borrow_mut() = Some(entries.clone());
    Ok(entries)
}

struct JetDataStaticWatchSource<T> {
    query: jet_std::DataQuery<T>,
    entries: std::rc::Rc<std::cell::RefCell<Option<Vec<jet_std::DataWatchEntry<T>>>>>,
}

impl<T: Clone + 'static> jet_std::DataWatchSource<T> for JetDataStaticWatchSource<T> {
    fn initial(&self) -> Result<jet_std::DataWatchMaterialized<T>, jet_std::DataError> {
        let entries = jet_data_static_query_entries(&self.query, &self.entries)?;
        let retained_rows = entries.len();
        Ok(jet_std::DataWatchMaterialized {
            entries,
            revision: 0,
            mode: "incremental",
            recomputed: false,
            changed: true,
            retained_rows,
        })
    }

    fn refresh(
        &self,
        _revision: u64,
        prior: &[jet_std::DataWatchEntry<T>],
    ) -> Result<jet_std::DataWatchMaterialized<T>, jet_std::DataError> {
        let entries = self.entries.borrow().clone().ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::State,
                "data.query.watch",
                "static join input was not initialized",
            )
        })?;
        Ok(jet_std::DataWatchMaterialized {
            entries: prior.to_vec(),
            revision: 0,
            mode: "incremental",
            recomputed: false,
            changed: false,
            retained_rows: entries.len(),
        })
    }
}

#[derive(Clone, Copy)]
enum JetDataJoinKind {
    Inner,
    Left,
}

#[derive(Clone)]
struct JetDataJoinWatchState<T, U, O> {
    left: Vec<jet_std::DataWatchEntry<T>>,
    right: Vec<jet_std::DataWatchEntry<U>>,
    output: Vec<jet_std::DataWatchEntry<O>>,
    left_revision: u64,
    right_revision: u64,
    publication_revision: u64,
}

struct JetDataJoinWatchSource<T, U, O> {
    left_source: std::rc::Rc<dyn jet_std::DataWatchSource<T>>,
    right_source: std::rc::Rc<dyn jet_std::DataWatchSource<U>>,
    left_key: std::sync::Arc<dyn Fn(T) -> String>,
    right_key: std::sync::Arc<dyn Fn(U) -> String>,
    join: std::sync::Arc<dyn Fn(T, Option<U>) -> O>,
    kind: JetDataJoinKind,
    operation: &'static str,
    state: std::cell::RefCell<Option<JetDataJoinWatchState<T, U, O>>>,
}

impl<T: Clone + 'static, U: Clone + 'static, O: Clone + 'static>
    JetDataJoinWatchSource<T, U, O>
{
    fn output_entries(
        &self,
        left: &[jet_std::DataWatchEntry<T>],
        right: &[jet_std::DataWatchEntry<U>],
    ) -> Result<Vec<jet_std::DataWatchEntry<O>>, jet_std::DataError> {
        let limits = jet_std::DataLimits::safe();
        if left.len() as i64 > limits.max_join_rows {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                self.operation,
                format!("max_join_rows {} exceeded while reading left input", limits.max_join_rows),
            ));
        }
        if right.len() as i64 > limits.max_join_rows {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                self.operation,
                format!("max_join_rows {} exceeded while indexing right input", limits.max_join_rows),
            ));
        }
        let mut index: std::collections::BTreeMap<
            String,
            Vec<&jet_std::DataWatchEntry<U>>,
        > = std::collections::BTreeMap::new();
        for entry in right {
            index
                .entry((self.right_key)(entry.value.clone()))
                .or_default()
                .push(entry);
        }
        let mut output = Vec::new();
        for left_entry in left {
            let key = (self.left_key)(left_entry.value.clone());
            let matches = index.get(&key).cloned().unwrap_or_default();
            if matches.is_empty() {
                if matches!(self.kind, JetDataJoinKind::Left) {
                    if output.len() as i64 >= limits.max_output_rows {
                        return Err(jet_data_error(
                            jet_std::DataErrorKind::Limit,
                            self.operation,
                            format!("max_output_rows {} exceeded", limits.max_output_rows),
                        ));
                    }
                    output.push(jet_std::DataWatchEntry {
                        order: output.len() as u64,
                        key: None,
                        value: (self.join)(left_entry.value.clone(), None),
                    });
                }
                continue;
            }
            for right_entry in matches {
                if output.len() as i64 >= limits.max_output_rows {
                    return Err(jet_data_error(
                        jet_std::DataErrorKind::Limit,
                        self.operation,
                        format!("max_output_rows {} exceeded", limits.max_output_rows),
                    ));
                }
                output.push(jet_std::DataWatchEntry {
                    order: output.len() as u64,
                    key: None,
                    value: (self.join)(
                        left_entry.value.clone(),
                        Some(right_entry.value.clone()),
                    ),
                });
            }
        }
        Ok(output)
    }

    fn retained_rows(
        &self,
        left: usize,
        right: usize,
    ) -> Result<usize, jet_std::DataError> {
        let retained = left.checked_add(right).ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::Limit,
                self.operation,
                "join retained row count overflow",
            )
        })?;
        Ok(retained)
    }
}

impl<T: Clone + 'static, U: Clone + 'static, O: Clone + 'static>
    jet_std::DataWatchSource<O> for JetDataJoinWatchSource<T, U, O>
{
    fn initial(&self) -> Result<jet_std::DataWatchMaterialized<O>, jet_std::DataError> {
        let left = self.left_source.initial()?;
        let right = self.right_source.initial()?;
        let output = self.output_entries(&left.entries, &right.entries)?;
        let publication_revision = left.revision.max(right.revision);
        let state = JetDataJoinWatchState {
            left: left.entries.clone(),
            right: right.entries.clone(),
            output: output.clone(),
            left_revision: left.revision,
            right_revision: right.revision,
            publication_revision,
        };
        let retained_rows = self.retained_rows(left.retained_rows, right.retained_rows)?;
        *self.state.borrow_mut() = Some(state);
        Ok(jet_std::DataWatchMaterialized {
            entries: output,
            revision: publication_revision,
            mode: if left.mode == "recomputed" || right.mode == "recomputed" {
                "recomputed"
            } else {
                "incremental"
            },
            recomputed: left.recomputed || right.recomputed,
            changed: true,
            retained_rows,
        })
    }

    fn refresh(
        &self,
        _revision: u64,
        _prior: &[jet_std::DataWatchEntry<O>],
    ) -> Result<jet_std::DataWatchMaterialized<O>, jet_std::DataError> {
        let mut holder = self.state.borrow_mut();
        let state = holder.as_mut().ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::State,
                self.operation,
                "join watch was not initialized",
            )
        })?;
        let left = self.left_source.refresh(state.left_revision, &state.left)?;
        let right = self.right_source.refresh(state.right_revision, &state.right)?;
        let changed = left.changed || right.changed;
        let mut next = state.clone();
        if changed {
            next.left = left.entries.clone();
            next.right = right.entries.clone();
            next.left_revision = left.revision;
            next.right_revision = right.revision;
            next.publication_revision =
                next.publication_revision.checked_add(1).ok_or_else(|| {
                    jet_data_error(
                        jet_std::DataErrorKind::Overflow,
                        self.operation,
                        "join publication revision exhausted",
                    )
                })?;
            next.output = self.output_entries(&next.left, &next.right)?;
        }
        let retained_rows = self.retained_rows(left.retained_rows, right.retained_rows)?;
        let entries = next.output.clone();
        let publication_revision = next.publication_revision;
        *state = next;
        Ok(jet_std::DataWatchMaterialized {
            entries,
            revision: publication_revision,
            mode: if left.mode == "recomputed" || right.mode == "recomputed" {
                "recomputed"
            } else {
                "incremental"
            },
            recomputed: left.recomputed || right.recomputed,
            changed,
            retained_rows,
        })
    }
}

fn jet_data_join_query<T, U, O, FL, FR, J>(
    left: &jet_std::DataQuery<T>,
    right: &jet_std::DataQuery<U>,
    left_key: FL,
    right_key: FR,
    kind: JetDataJoinKind,
    join: J,
    operation: &'static str,
) -> jet_std::DataQuery<O>
where
    T: Clone + 'static,
    U: Clone + 'static,
    O: Clone + 'static,
    FL: Fn(T) -> String + 'static,
    FR: Fn(U) -> String + 'static,
    J: Fn(T, Option<U>) -> O + 'static,
{
    let left_source = left.maintained_watch_source();
    let right_source = right.maintained_watch_source();
    let left_static_entries = if left_source.is_none() {
        Some(std::rc::Rc::new(std::cell::RefCell::new(None)))
    } else {
        None
    };
    let right_static_entries = if right_source.is_none() {
        Some(std::rc::Rc::new(std::cell::RefCell::new(None)))
    } else {
        None
    };
    let tracking = left.tracking.clone().or_else(|| right.tracking.clone());
    let mut steps = left.plan();
    steps.push(operation.strip_prefix("query.").unwrap_or(operation).to_string());
    let left_input = left.clone();
    let right_input = right.clone();
    let left_key = std::sync::Arc::new(left_key);
    let right_key = std::sync::Arc::new(right_key);
    let join = std::sync::Arc::new(join);
    let compute_left_key = left_key.clone();
    let compute_right_key = right_key.clone();
    let compute_join = join.clone();
    let compute_left_static_entries = left_static_entries.clone();
    let compute_right_static_entries = right_static_entries.clone();
    let result = jet_data_reducer_query(tracking, steps, move || {
        let join = compute_join.clone();
        let left_rows = if let Some(cache) = &compute_left_static_entries {
            jet_data_static_query_entries(&left_input, cache)?
                .into_iter()
                .map(|entry| entry.value)
                .collect()
        } else {
            jet_data_query_collect(&left_input)?
        };
        let right_rows = if let Some(cache) = &compute_right_static_entries {
            jet_data_static_query_entries(&right_input, cache)?
                .into_iter()
                .map(|entry| entry.value)
                .collect()
        } else {
            jet_data_query_collect(&right_input)?
        };
        let limits = jet_std::DataLimits::safe();
        match kind {
            JetDataJoinKind::Inner => {
                let left_key = compute_left_key.clone();
                let right_key = compute_right_key.clone();
                let join = join.clone();
                let rows = jet_data_inner_join_checked(
                    &left_rows,
                    &right_rows,
                    move |row| left_key(row),
                    move |row| right_key(row),
                    &limits,
                )?;
                Ok(rows
                    .into_iter()
                    .map(|row| join(row.left, Some(row.right)))
                    .collect())
            }
            JetDataJoinKind::Left => {
                let left_key = compute_left_key.clone();
                let right_key = compute_right_key.clone();
                let join = join.clone();
                let rows = jet_data_left_join_checked(
                    &left_rows,
                    &right_rows,
                    move |row| left_key(row),
                    move |row| right_key(row),
                    &limits,
                )?;
                Ok(rows
                    .into_iter()
                    .map(|row| join(row.left, row.right.ok()))
                    .collect())
            }
        }
    });
    if left_source.is_some() || right_source.is_some() {
        let left_source = left_source.unwrap_or_else(|| {
            let entries = match left_static_entries {
                Some(entries) => entries,
                None => unreachable!("missing static left join cache"),
            };
            std::rc::Rc::new(JetDataStaticWatchSource {
                query: left.clone(),
                entries,
            }) as std::rc::Rc<dyn jet_std::DataWatchSource<T>>
        });
        let right_source = right_source.unwrap_or_else(|| {
            let entries = match right_static_entries {
                Some(entries) => entries,
                None => unreachable!("missing static right join cache"),
            };
            std::rc::Rc::new(JetDataStaticWatchSource {
                query: right.clone(),
                entries,
            }) as std::rc::Rc<dyn jet_std::DataWatchSource<U>>
        });
        result.with_watch_source(std::rc::Rc::new(JetDataJoinWatchSource {
            left_source,
            right_source,
            left_key,
            right_key,
            join,
            kind,
            operation,
            state: std::cell::RefCell::new(None),
        }))
    } else {
        result
    }
}

fn jet_data_query_inner_join<T, U, FL, FR>(
    left: &jet_std::DataQuery<T>,
    right: &jet_std::DataQuery<U>,
    left_key: FL,
    right_key: FR,
) -> jet_std::DataQuery<jet_std::DataJoin<T, U>>
where
    T: Clone + 'static,
    U: Clone + 'static,
    FL: Fn(T) -> String + 'static,
    FR: Fn(U) -> String + 'static,
{
    jet_data_join_query(
        left,
        right,
        left_key,
        right_key,
        JetDataJoinKind::Inner,
        |left, right| jet_std::DataJoin {
            left,
            right: right.expect("inner join invariant"),
        },
        "query.inner_join",
    )
}

fn jet_data_query_left_join<T, U, FL, FR>(
    left: &jet_std::DataQuery<T>,
    right: &jet_std::DataQuery<U>,
    left_key: FL,
    right_key: FR,
) -> jet_std::DataQuery<jet_std::DataJoin<T, JetOutcome<U, JetAbsent>>>
where
    T: Clone + 'static,
    U: Clone + 'static,
    FL: Fn(T) -> String + 'static,
    FR: Fn(U) -> String + 'static,
{
    jet_data_join_query(
        left,
        right,
        left_key,
        right_key,
        JetDataJoinKind::Left,
        |left, right| jet_std::DataJoin {
            left,
            right: right.ok_or(JetAbsent),
        },
        "query.left_join",
    )
}

struct JetDataMapWatchSource<T, U> {
    source: std::rc::Rc<dyn jet_std::DataWatchSource<T>>,
    base: std::cell::RefCell<Option<Vec<jet_std::DataWatchEntry<T>>>>,
    map: std::sync::Arc<dyn Fn(T) -> Result<U, jet_std::DataError>>,
}

impl<T: Clone + 'static, U: Clone + 'static> JetDataMapWatchSource<T, U> {
    fn map_entries(
        &self,
        entries: Vec<jet_std::DataWatchEntry<T>>,
    ) -> Result<Vec<jet_std::DataWatchEntry<U>>, jet_std::DataError> {
        entries
            .into_iter()
            .map(|entry| {
                Ok(jet_std::DataWatchEntry {
                    order: entry.order,
                    key: None,
                    value: (self.map)(entry.value)?,
                })
            })
            .collect()
    }
}

impl<T: Clone + 'static, U: Clone + 'static> jet_std::DataWatchSource<U>
    for JetDataMapWatchSource<T, U>
{
    fn initial(&self) -> Result<jet_std::DataWatchMaterialized<U>, jet_std::DataError> {
        let materialized = self.source.initial()?;
        let entries = self.map_entries(materialized.entries.clone())?;
        *self.base.borrow_mut() = Some(materialized.entries);
        Ok(jet_std::DataWatchMaterialized {
            entries,
            revision: materialized.revision,
            mode: materialized.mode,
            recomputed: materialized.recomputed,
            changed: materialized.changed,
            retained_rows: materialized.retained_rows,
        })
    }

    fn refresh(
        &self,
        revision: u64,
        prior: &[jet_std::DataWatchEntry<U>],
    ) -> Result<jet_std::DataWatchMaterialized<U>, jet_std::DataError> {
        let base = self.base.borrow().clone().ok_or_else(|| {
            jet_std::DataError::simple(
                jet_std::DataErrorKind::State,
                "data.query.watch",
                "mapped watch was not initialized",
            )
        })?;
        let materialized = self.source.refresh(revision, &base)?;
        if !materialized.changed && materialized.revision == revision {
            return Ok(jet_std::DataWatchMaterialized {
                entries: prior.to_vec(),
                revision: materialized.revision,
                mode: materialized.mode,
                recomputed: false,
                changed: false,
                retained_rows: materialized.retained_rows,
            });
        }
        let next_base = materialized.entries.clone();
        let entries = self.map_entries(next_base.clone())?;
        *self.base.borrow_mut() = Some(next_base);
        Ok(jet_std::DataWatchMaterialized {
            entries,
            revision: materialized.revision,
            mode: materialized.mode,
            recomputed: materialized.recomputed,
            changed: materialized.changed,
            retained_rows: materialized.retained_rows,
        })
    }
}
fn jet_data_extreme_key_finite<K: 'static>(key: &K) -> bool {
    let key = key as &dyn std::any::Any;
    key.downcast_ref::<f64>()
        .map(|value| value.is_finite())
        .or_else(|| key.downcast_ref::<f32>().map(|value| value.is_finite()))
        .unwrap_or(true)
}

#[derive(Clone, Copy)]
enum JetDataExtreme {
    Min,
    Max,
}

struct JetDataExtremeWatchSource<T, K> {
    source: std::rc::Rc<dyn jet_std::DataWatchSource<T>>,
    base: std::cell::RefCell<Option<Vec<jet_std::DataWatchEntry<T>>>>,
    key: std::sync::Arc<dyn Fn(T) -> K>,
    extreme: JetDataExtreme,
    operation: &'static str,
}

impl<T: Clone + 'static, K: Clone + PartialOrd + 'static>
    JetDataExtremeWatchSource<T, K>
{
    fn select(
        &self,
        entries: &[jet_std::DataWatchEntry<T>],
    ) -> Result<(K, u64), jet_std::DataError> {
        let mut selected: Option<(K, u64)> = None;
        for entry in entries {
            let candidate = (self.key)(entry.value.clone());
            if !jet_data_extreme_key_finite(&candidate) {
                return Err(jet_data_error(
                    jet_std::DataErrorKind::NonFinite,
                    self.operation,
                    "extreme key must be finite",
                ));
            }
            let replace = match &selected {
                None => true,
                Some((current, current_order)) => {
                    let ordering = candidate.partial_cmp(current).ok_or_else(|| {
                        jet_data_error(
                            jet_std::DataErrorKind::NonFinite,
                            self.operation,
                            "extreme key is not ordered",
                        )
                    })?;
                    match self.extreme {
                        JetDataExtreme::Min => {
                            ordering == std::cmp::Ordering::Less
                                || (ordering == std::cmp::Ordering::Equal
                                    && entry.order < *current_order)
                        }
                        JetDataExtreme::Max => {
                            ordering == std::cmp::Ordering::Greater
                                || (ordering == std::cmp::Ordering::Equal
                                    && entry.order < *current_order)
                        }
                    }
                }
            };
            if replace {
                selected = Some((candidate, entry.order));
            }
        }
        selected.ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::Empty,
                self.operation,
                "empty query has no extreme",
            )
        })
    }

    fn materialize(
        &self,
        materialized: jet_std::DataWatchMaterialized<T>,
    ) -> Result<jet_std::DataWatchMaterialized<K>, jet_std::DataError> {
        let (value, order) = self.select(&materialized.entries)?;
        Ok(jet_std::DataWatchMaterialized {
            entries: vec![jet_std::DataWatchEntry {
                order,
                key: None,
                value,
            }],
            revision: materialized.revision,
            mode: materialized.mode,
            recomputed: materialized.recomputed,
            changed: materialized.changed,
            retained_rows: materialized.retained_rows,
        })
    }
}

impl<T: Clone + 'static, K: Clone + PartialOrd + 'static> jet_std::DataWatchSource<K>
    for JetDataExtremeWatchSource<T, K>
{
    fn initial(&self) -> Result<jet_std::DataWatchMaterialized<K>, jet_std::DataError> {
        let materialized = self.source.initial()?;
        *self.base.borrow_mut() = Some(materialized.entries.clone());
        self.materialize(materialized)
    }

    fn refresh(
        &self,
        revision: u64,
        prior: &[jet_std::DataWatchEntry<K>],
    ) -> Result<jet_std::DataWatchMaterialized<K>, jet_std::DataError> {
        let base = self.base.borrow().clone().ok_or_else(|| {
            jet_std::DataError::simple(
                jet_std::DataErrorKind::State,
                "data.query.watch",
                "extreme watch was not initialized",
            )
        })?;
        let materialized = self.source.refresh(revision, &base)?;
        if !materialized.changed && materialized.revision == revision {
            return Ok(jet_std::DataWatchMaterialized {
                entries: prior.to_vec(),
                revision: materialized.revision,
                mode: materialized.mode,
                recomputed: false,
                changed: false,
                retained_rows: materialized.retained_rows,
            });
        }
        let next_base = materialized.entries.clone();
        let output = self.materialize(materialized)?;
        *self.base.borrow_mut() = Some(next_base);
        Ok(output)
    }
}

fn jet_data_extreme_materialized<T, K, F>(
    rows: Vec<T>,
    key: &F,
    extreme: JetDataExtreme,
    operation: &'static str,
) -> Result<Vec<K>, jet_std::DataError>
where
    T: Clone,
    K: Clone + PartialOrd + 'static,
    F: ?Sized + Fn(T) -> K,
{
    let mut selected: Option<(K, u64)> = None;
    for (order, row) in rows.into_iter().enumerate() {
        let candidate = key(row);
        if !jet_data_extreme_key_finite(&candidate) {
            return Err(jet_data_error(
                jet_std::DataErrorKind::NonFinite,
                operation,
                "extreme key must be finite",
            ));
        }
        let replace = match &selected {
            None => true,
            Some((current, current_order)) => {
                let ordering = candidate.partial_cmp(current).ok_or_else(|| {
                    jet_data_error(
                        jet_std::DataErrorKind::NonFinite,
                        operation,
                        "extreme key is not ordered",
                    )
                })?;
                match extreme {
                    JetDataExtreme::Min => {
                        ordering == std::cmp::Ordering::Less
                            || (ordering == std::cmp::Ordering::Equal
                                && (order as u64) < *current_order)
                    }
                    JetDataExtreme::Max => {
                        ordering == std::cmp::Ordering::Greater
                            || (ordering == std::cmp::Ordering::Equal
                                && (order as u64) < *current_order)
                    }
                }
            }
        };
        if replace {
            selected = Some((candidate, order as u64));
        }
    }
    selected
        .map(|(value, _)| vec![value])
        .ok_or_else(|| {
            jet_data_error(
                jet_std::DataErrorKind::Empty,
                operation,
                "empty query has no extreme",
            )
        })
}

fn jet_data_query_extreme_by<T, K, F>(
    query: &jet_std::DataQuery<T>,
    key: F,
    extreme: JetDataExtreme,
    operation: &'static str,
) -> jet_std::DataQuery<K>
where
    T: Clone + 'static,
    K: Clone + PartialOrd + 'static,
    F: Fn(T) -> K + 'static,
{
    let source = query.maintained_watch_source();
    let tracking = query.tracking.clone();
    let mut steps = query.plan();
    steps.push(operation.rsplit('.').next().unwrap_or(operation).to_string());
    let key = std::sync::Arc::new(key);
    let input = query.clone();
    let compute_key = key.clone();
    let mut result = jet_data_reducer_query(tracking, steps, move || {
        let rows = jet_data_query_collect(&input)?;
        jet_data_extreme_materialized(rows, compute_key.as_ref(), extreme, operation)
    });
    if let Some(source) = source {
        result = result.with_watch_source(std::rc::Rc::new(
            JetDataExtremeWatchSource {
                source,
                base: std::cell::RefCell::new(None),
                key,
                extreme,
                operation,
            },
        ));
    }
    result
}

fn jet_data_query_min<T, V, F>(
    query: &jet_std::DataQuery<T>,
    key: F,
) -> jet_std::DataQuery<V>
where
    T: Clone + 'static,
    V: Clone + PartialOrd + 'static,
    F: Fn(T) -> V + 'static,
{
    jet_data_query_extreme_by(query, key, JetDataExtreme::Min, "query.min")
}

fn jet_data_query_max<T, V, F>(
    query: &jet_std::DataQuery<T>,
    key: F,
) -> jet_std::DataQuery<V>
where
    T: Clone + 'static,
    V: Clone + PartialOrd + 'static,
    F: Fn(T) -> V + 'static,
{
    jet_data_query_extreme_by(query, key, JetDataExtreme::Max, "query.max")
}



fn jet_data_query_map_checked<T, U, F>(
    query: &jet_std::DataQuery<T>,
    mapper: F,
) -> jet_std::DataQuery<U>
where
    T: Clone + 'static,
    U: Clone + 'static,
    F: Fn(T) -> Result<U, jet_std::DataError> + 'static,
{
    let source = query.maintained_watch_source();
    let tracking = query.tracking.clone();
    let mut steps = query.plan();
    steps.push("map".to_string());
    let mapper = std::sync::Arc::new(mapper);
    let input = query.clone();
    let compute_mapper = mapper.clone();
    let mut mapped = jet_data_reducer_query(tracking, steps, move || {
        let rows = jet_data_query_collect(&input)?;
        rows.into_iter().map(|row| compute_mapper(row)).collect()
    });
    if let Some(source) = source {
        let watch_source = JetDataMapWatchSource {
            source,
            base: std::cell::RefCell::new(None),
            map: mapper,
        };
        mapped = mapped.with_watch_source(std::rc::Rc::new(watch_source));
    }
    mapped
}

fn jet_data_query_map<T, U, F>(
    query: &jet_std::DataQuery<T>,
    mapper: F,
) -> jet_std::DataQuery<U>
where
    T: Clone + 'static,
    U: Clone + 'static,
    F: Fn(T) -> U + 'static,
{
    jet_data_query_map_checked(query, move |row| Ok(mapper(row)))
}



fn jet_data_reducer_query<O, F>(
    tracking: Option<std::rc::Rc<dyn jet_std::DataTrackedMeta>>,
    steps: Vec<String>,
    compute: F,
) -> jet_std::DataQuery<O>
where
    F: Fn() -> Result<Vec<O>, jet_std::DataError> + 'static,
{
    match tracking {
        Some(tracking) => jet_std::DataQuery::from_recomputation(steps, tracking, compute),
        None => jet_std::DataQuery::from_computation(steps, compute),
    }
}


fn jet_data_group_sum_query_checked<T, K, V, F>(
    grouped: &jet_std::DataGroupedQuery<T, K>,
    value: F,
) -> jet_std::DataQuery<jet_std::GroupValue<K, V>>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    V: Clone + std::ops::Add<Output = V> + 'static,
    F: Fn(T) -> Result<V, jet_std::DataError> + 'static,
{
    let query = grouped.query.clone();
    let key = grouped.key.clone();
    let value = std::sync::Arc::new(value);
    let steps = jet_data_grouped_steps(grouped, "sum");
    let tracking = query.tracking.clone();
    let source = query.tracked_reader();
    let sum_recomputed = matches!(std::any::type_name::<V>(), "f32" | "f64");
    let aggregate = std::sync::Arc::new(|values: &[V]| {
        let mut values = values.iter().cloned();
        let Some(first) = values.next() else {
            return Err(jet_data_error(
                jet_std::DataErrorKind::State,
                "query.group_by.sum",
                "empty group has no sum",
            ));
        };
        Ok(values.fold(first, |total, value| total + value))
    });
    let result = jet_data_reducer_query(tracking, steps, {
        let query = query.clone();
        let key = key.clone();
        let value = value.clone();
        move || jet_data_group_sum_materialized(&query, key.as_ref(), value.as_ref())
    });
    if let Some(source) = source {
        result.with_watch_source(std::rc::Rc::new(JetDataGroupWatchSource {
            source,
            operations: query.operations.clone(),
            key,
            value,
            aggregate,
            operation: "query.group_by.sum",
            mode: if sum_recomputed {
                "recomputed"
            } else {
                "incremental"
            },
            recomputed: sum_recomputed,
            state: std::cell::RefCell::new(None),
        }))
    } else {
        result
    }
}

fn jet_data_group_sum_query<T, K, V, F>(
    grouped: &jet_std::DataGroupedQuery<T, K>,
    value: F,
) -> jet_std::DataQuery<jet_std::GroupValue<K, V>>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    V: Clone + std::ops::Add<Output = V> + 'static,
    F: Fn(T) -> V + 'static,
{
    jet_data_group_sum_query_checked(grouped, move |row| -> Result<V, jet_std::DataError> {
        Ok(value(row))
    })
}

fn jet_data_group_count_query<T, K, V>(
    grouped: &jet_std::DataGroupedQuery<T, K>,
) -> jet_std::DataQuery<jet_std::GroupValue<K, V>>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    V: Clone + From<i64> + 'static,
{
    let query = grouped.query.clone();
    let key = grouped.key.clone();
    let steps = jet_data_grouped_steps(grouped, "count");
    let tracking = query.tracking.clone();
    let source = query.tracked_reader();
    let value = std::sync::Arc::new(|_: T| -> Result<V, jet_std::DataError> { Ok(V::from(1)) });
    let aggregate = std::sync::Arc::new(|values: &[V]| Ok(V::from(values.len() as i64)));
    let result = jet_data_reducer_query(tracking, steps, {
        let query = query.clone();
        let key = key.clone();
        move || {
            let rows = jet_data_query_collect(&query)?;
            let limits = jet_std::DataLimits::safe();
            let mut groups: Vec<(K, i64)> = Vec::new();
            for row in rows {
                let group_key = key(row);
                if let Some((_, count)) = groups
                    .iter_mut()
                    .find(|(existing, _)| existing == &group_key)
                {
                    *count = count.saturating_add(1);
                } else {
                    if groups.len() as i64 >= limits.max_groups {
                        return Err(jet_data_error(
                            jet_std::DataErrorKind::Limit,
                            "query.group_by.count",
                            format!("max_groups {} exceeded", limits.max_groups),
                        ));
                    }
                    groups.push((group_key, 1));
                }
            }
            Ok(groups
                .into_iter()
                .map(|(key, value)| jet_std::GroupValue { key, value: V::from(value) })
                .collect())
        }
    });
    if let Some(source) = source {
        result.with_watch_source(std::rc::Rc::new(JetDataGroupWatchSource {
            source,
            operations: query.operations.clone(),
            key,
            value,
            aggregate,
            operation: "query.group_by.count",
            mode: "incremental",
            recomputed: false,
            state: std::cell::RefCell::new(None),
        }))
    } else {
        result
    }
}

fn jet_data_group_mean_query_checked<T, K, F>(
    grouped: &jet_std::DataGroupedQuery<T, K>,
    value: F,
) -> jet_std::DataQuery<jet_std::GroupValue<K, f64>>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    F: Fn(T) -> Result<f64, jet_std::DataError> + 'static,
{
    let query = grouped.query.clone();
    let key = grouped.key.clone();
    let source = query.tracked_reader();
    let steps = jet_data_grouped_steps(grouped, "mean");
    let tracking = query.tracking.clone();
    let aggregate = std::sync::Arc::new(|values: &[f64]| {
        if values.iter().any(|value| !value.is_finite()) {
            return Err(jet_data_error(
                jet_std::DataErrorKind::NonFinite,
                "query.group_by.mean",
                "group values must be finite",
            ));
        }
        if values.is_empty() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::State,
                "query.group_by.mean",
                "empty group has no mean",
            ));
        }
        Ok(values.iter().copied().sum::<f64>() / values.len() as f64)
    });
    let value = std::sync::Arc::new(value);
    let result = jet_data_reducer_query(tracking, steps, {
        let query = query.clone();
        let key = key.clone();
        let value = value.clone();
        move || {
            let rows = jet_data_query_collect(&query)?;
            let limits = jet_std::DataLimits::safe();
            let mut groups: Vec<(K, f64, i64)> = Vec::new();
            for row in rows {
                let group_key = key(row.clone());
                let group_value = value(row)?;
                if !group_value.is_finite() {
                    return Err(jet_data_error(
                        jet_std::DataErrorKind::NonFinite,
                        "query.group_by.mean",
                        "group values must be finite",
                    ));
                }
                if let Some((_, total, count)) = groups
                    .iter_mut()
                    .find(|(existing, _, _)| existing == &group_key)
                {
                    *total += group_value;
                    *count = count.saturating_add(1);
                } else {
                    if groups.len() as i64 >= limits.max_groups {
                        return Err(jet_data_error(
                            jet_std::DataErrorKind::Limit,
                            "query.group_by.mean",
                            format!("max_groups {} exceeded", limits.max_groups),
                        ));
                    }
                    groups.push((group_key, group_value, 1));
                }
            }
            Ok(groups
                .into_iter()
                .map(|(key, total, count)| jet_std::GroupValue {
                    key,
                    value: total / count as f64,
                })
                .collect())
        }
    });
    if let Some(source) = source {
        result.with_watch_source(std::rc::Rc::new(JetDataGroupWatchSource {
            source,
            operations: query.operations.clone(),
            key,
            value,
            aggregate,
            operation: "query.group_by.mean",
            mode: "recomputed",
            recomputed: true,
            state: std::cell::RefCell::new(None),
        }))
    } else {
        result
    }
}

fn jet_data_group_mean_query<T, K, F>(
    grouped: &jet_std::DataGroupedQuery<T, K>,
    value: F,
) -> jet_std::DataQuery<jet_std::GroupValue<K, f64>>
where
    T: Clone + 'static,
    K: Clone + PartialEq + 'static,
    F: Fn(T) -> f64 + 'static,
{
    jet_data_group_mean_query_checked(grouped, move |row| -> Result<f64, jet_std::DataError> {
        Ok(value(row))
    })
}

fn jet_data_csv_reader(
    input: JetFileReader,
    limits: jet_std::DataLimits,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    jet_data_limits_validate(&limits)?;
    let reader = jet_enc_csv_reader(
        input,
        limits.encoding.clone(),
        ",".to_string(),
        false,
        false,
    )
        .map_err(|enc| jet_data_from_encoding("csv_reader", Err(JetAbsent), enc))?;
    Ok(jet_std::DataStream {
        inner: jet_std::DataStreamInner::CSV {
            reader,
            headers: None,
        },
        limits,
        terminal: None,
        eof: false,
        row_index: 0,
        cancelled: false,
    })
}

fn jet_data_json_reader(
    input: JetFileReader,
    limits: jet_std::DataLimits,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    jet_data_limits_validate(&limits)?;
    let mut reader = jet_enc_json_reader(input, limits.encoding.clone())
        .map_err(|enc| jet_data_from_encoding("json_reader", Err(JetAbsent), enc))?;
    reader.typed_numbers = true;
    Ok(jet_std::DataStream {
        inner: jet_std::DataStreamInner::JSON {
            reader,
            array_started: false,
            array_done: false,
        },
        limits,
        terminal: None,
        eof: false,
        row_index: 0,
        cancelled: false,
    })
}

fn jet_data_jsonl_reader(
    input: JetFileReader,
    limits: jet_std::DataLimits,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    jet_data_limits_validate(&limits)?;
    let reader = jet_enc_jsonl_reader(input, limits.encoding.clone())
        .map_err(|enc| jet_data_from_encoding("jsonl_reader", Err(JetAbsent), enc))?;
    Ok(jet_std::DataStream {
        inner: jet_std::DataStreamInner::JSONL { reader },
        limits,
        terminal: None,
        eof: false,
        row_index: 0,
        cancelled: false,
    })
}

fn jet_data_stream_fail<T>(
    stream: &mut jet_std::DataStream,
    error: jet_std::DataError,
) -> Result<T, jet_std::DataError> {
    stream.terminal = Some(error.clone());
    Err(error)
}

fn jet_data_field_errors_reason(errors: Vec<jet_std::FieldError>) -> String {
    errors
        .into_iter()
        .map(|error| {
            if error.path.is_empty() {
                error.reason
            } else {
                format!("{}: {}", error.path, error.reason)
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn jet_data_stream_decode_csv_row<T: __jet_Decode>(
    headers: &[String],
    row: Vec<String>,
    row_index: i64,
) -> Result<T, jet_std::DataError> {
    if row.len() != headers.len() {
        let mut error = jet_data_error(
            jet_std::DataErrorKind::Decode,
            "csv_reader",
            format!(
                "CSV row has {} fields; expected {}",
                row.len(),
                headers.len()
            ),
        );
        error.row = Ok(row_index);
        return Err(error);
    }
    let obj: Vec<(String, jet_std::DataTree)> = headers
        .iter()
        .cloned()
        .zip(row.into_iter().map(jet_std::DataTree::Text))
        .collect();
    T::jet_decode(&jet_std::DataTree::Object(obj)).map_err(|errors| {
        let mut error = jet_data_error(
            jet_std::DataErrorKind::Decode,
            "csv_reader",
            jet_data_field_errors_reason(errors),
        );
        error.row = Ok(row_index);
        error
    })
}


// D-FAIL-CARRIER1=A: the row is `?T !DataError` — end of stream is a clean
// absence, a broken row is a report.
fn jet_data_stream_next<T: __jet_Decode>(
    stream: &mut jet_std::DataStream,
) -> Result<JetOutcome<T, JetAbsent>, jet_std::DataError> {
    jet_data_stream_scan(stream).map(jet_outcome_of)
}


fn jet_data_stream_scan<T: __jet_Decode>(
    stream: &mut jet_std::DataStream,
) -> Result<Option<T>, jet_std::DataError> {
    if let Some(error) = &stream.terminal {
        return Err(error.clone());
    }
    if stream.cancelled {
        return jet_data_stream_fail(
            stream,
            jet_data_error(
                jet_std::DataErrorKind::State,
                "data.stream",
                "stream was cancelled before the next row",
            ),
        );
    }
    if stream.eof {
        return Ok(None);
    }
    match &mut stream.inner {
        jet_std::DataStreamInner::CSV { reader, headers } => {
            if headers.is_none() {
                match reader.next_record() {
                    Ok(Some(header)) => *headers = Some(header.fields),
                    Ok(None) => {
                        stream.eof = true;
                        return Ok(None);
                    }
                    Err(enc) => {
                        return jet_data_stream_fail(
                            stream,
                            jet_data_from_encoding("csv_reader", Err(JetAbsent), enc),
                        );
                    }
                }
            }
            let header = headers.as_ref().unwrap().clone();
            match reader.next_record() {
                Ok(None) => {
                    stream.eof = true;
                    Ok(None)
                }
                Ok(Some(row)) => {
                    stream.row_index += 1;
                    match jet_data_stream_decode_csv_row::<T>(
                        &header,
                        row.fields,
                        stream.row_index,
                    ) {
                        Ok(value) => Ok(Some(value)),
                        Err(error) => jet_data_stream_fail(stream, error),
                    }
                }
                Err(enc) => jet_data_stream_fail(
                    stream,
                    jet_data_from_encoding("csv_reader", Ok(stream.row_index + 1), enc),
                ),
            }
        }
        jet_std::DataStreamInner::JSON {
            reader,
            array_started,
            array_done,
        } => {
            if *array_done {
                stream.eof = true;
                return Ok(None);
            }
            if !*array_started {
                match reader.next_event() {
                    Ok(Some(jet_std::DataEvent::ArrayStart)) => *array_started = true,
                    Ok(Some(_)) => {
                        return jet_data_stream_fail(
                            stream,
                            jet_data_error(
                                jet_std::DataErrorKind::Decode,
                                "json_reader",
                                "typed JSON stream requires a top-level array of objects",
                            ),
                        );
                    }
                    Ok(None) => {
                        stream.eof = true;
                        return Ok(None);
                    }
                    Err(enc) => {
                        return jet_data_stream_fail(
                            stream,
                            jet_data_from_encoding("json_reader", Err(JetAbsent), enc),
                        );
                    }
                }
            }
            let first = match reader.next_event() {
                Ok(None) => {
                    *array_done = true;
                    stream.eof = true;
                    return Ok(None);
                }
                Ok(Some(jet_std::DataEvent::ArrayEnd)) => {
                    *array_done = true;
                    stream.eof = true;
                    return Ok(None);
                }
                Ok(Some(event)) => event,
                Err(enc) => {
                    return jet_data_stream_fail(
                        stream,
                        jet_data_from_encoding("json_reader", Ok(stream.row_index + 1), enc),
                    );
                }
            };
            let mut heap = match jet_json_fold_budget(reader) {
                Ok(heap) => heap,
                Err(enc) => {
                    return jet_data_stream_fail(
                        stream,
                        jet_data_from_encoding("json_reader", Ok(stream.row_index + 1), enc),
                    );
                }
            };
            match jet_json_fold_from_event(reader, first, &mut heap) {
                Ok(tree) => {
                    stream.row_index += 1;
                    match T::jet_decode(&tree) {
                        Ok(value) => Ok(Some(value)),
                        Err(errors) => {
                            let mut error = jet_data_error(
                                jet_std::DataErrorKind::Decode,
                                "json_reader",
                                jet_data_field_errors_reason(errors),
                            );
                            error.row = Ok(stream.row_index);
                            jet_data_stream_fail(stream, error)
                        }
                    }
                }
                Err(enc) => jet_data_stream_fail(
                    stream,
                    jet_data_from_encoding("json_reader", Ok(stream.row_index + 1), enc),
                ),
            }
        }
        jet_std::DataStreamInner::JSONL { reader } => match reader.next_record() {
            Ok(None) => {
                stream.eof = true;
                Ok(None)
            }
            Ok(Some(tree)) => {
                stream.row_index += 1;
                match T::jet_decode(&tree) {
                    Ok(value) => Ok(Some(value)),
                    Err(errors) => {
                        let mut error = jet_data_error(
                            jet_std::DataErrorKind::Decode,
                            "jsonl_reader",
                            jet_data_field_errors_reason(errors),
                        );
                        error.row = Ok(stream.row_index);
                        jet_data_stream_fail(stream, error)
                    }
                }
            }
            Err(enc) => jet_data_stream_fail(
                stream,
                jet_data_from_encoding("jsonl_reader", Ok(stream.row_index + 1), enc),
            ),
        },
        jet_std::DataStreamInner::Provider {
            format: _format,
            rows,
            cursor,
        } => {
            if *cursor >= rows.len() {
                stream.eof = true;
                return Ok(None);
            }
            let tree = rows[*cursor].clone();
            *cursor += 1;
            stream.row_index += 1;
            match T::jet_decode(&tree) {
                Ok(value) => Ok(Some(value)),
                Err(errors) => {
                    let mut error = jet_data_error(
                        jet_std::DataErrorKind::Decode,
                        "data.provider",
                        jet_data_field_errors_reason(errors),
                    );
                    error.row = Ok(stream.row_index);
                    jet_data_stream_fail(stream, error)
                }
            }
        }
    }
}

fn jet_data_stream_collect<T: __jet_Decode + Clone>(
    stream: &mut jet_std::DataStream,
) -> Result<Vec<T>, jet_std::DataError> {
    jet_data_limits_validate(&stream.limits)?;
    let max_output_rows = usize::try_from(stream.limits.max_output_rows).map_err(|_| {
        jet_data_error(
            jet_std::DataErrorKind::Limit,
            "collect",
            "max_output_rows cannot be represented by this target",
        )
    })?;
    let mut out = Vec::with_capacity(max_output_rows.min(256));
    loop {
        match jet_data_stream_next::<T>(stream)? {
            Err(JetAbsent) => return Ok(out),
            Ok(row) => {
                if out.len() >= max_output_rows {
                    // Crossing fails before retaining the item that crosses.
                    return jet_data_stream_fail(
                        stream,
                        jet_data_error(
                            jet_std::DataErrorKind::Limit,
                            "collect",
                            format!(
                                "max_output_rows {} exceeded",
                                stream.limits.max_output_rows
                            ),
                        ),
                    );
                }
                out.try_reserve_exact(1).map_err(|_| {
                    jet_data_error(
                        jet_std::DataErrorKind::Limit,
                        "collect",
                        "output allocation exceeded the configured bound",
                    )
                })?;
                out.push(row);
            }
        }
    }
}


// D-DX-LOADERS1=A: core owns only dependency-free text loaders and the
// deterministic snapshot contract. URL, database, archive, and provider
// packages bind canonical payloads through this same carrier; no second cache
// or credential store is introduced.
const JET_DATA_LOADER_MAX_PUBLIC_TEXT: usize = 4096;
const JET_DATA_LOADER_MAX_STATUS_TEXT: usize = 1024;

fn jet_data_loader_kernel_kind(
    kind: jet_std::DataLoaderKind,
) -> jet_foundation::PreludeDataFlow::LoaderKind {
    match kind {
        jet_std::DataLoaderKind::File => jet_foundation::PreludeDataFlow::LoaderKind::File,
        jet_std::DataLoaderKind::Url => jet_foundation::PreludeDataFlow::LoaderKind::Url,
        jet_std::DataLoaderKind::Database => {
            jet_foundation::PreludeDataFlow::LoaderKind::Database
        }
        jet_std::DataLoaderKind::Value => jet_foundation::PreludeDataFlow::LoaderKind::Value,
    }
}

fn jet_data_loader_kernel_limits(
    limits: &jet_std::DataLimits,
) -> jet_foundation::PreludeDataFlow::Limits {
    jet_foundation::PreludeDataFlow::Limits {
        buffer_bytes: limits.encoding.buffer_bytes,
        max_depth: limits.encoding.max_depth,
        max_item_bytes: limits.encoding.max_item_bytes,
        max_total_bytes: limits.encoding.max_total_bytes.ok(),
        max_expansion_depth: limits.encoding.max_expansion_depth,
        max_expansion_bytes: limits.encoding.max_expansion_bytes,
        max_groups: limits.max_groups,
        max_sort_rows: limits.max_sort_rows,
        max_join_rows: limits.max_join_rows,
        max_output_rows: limits.max_output_rows,
    }
}

fn jet_data_loader_kernel_authority(
    authority: &jet_std::DataAuthority,
) -> jet_foundation::PreludeDataFlow::Authority {
    jet_foundation::PreludeDataFlow::Authority {
        scope: authority.scope.clone(),
        revision: authority.revision.clone(),
    }
}

fn jet_data_loader_kernel_status(
    status: &jet_std::DataLoaderStatus,
) -> jet_foundation::PreludeDataFlow::Status {
    jet_foundation::PreludeDataFlow::Status {
        identity: status.identity.clone(),
        freshness: match status.freshness {
            jet_std::DataFreshness::Pending => {
                jet_foundation::PreludeDataFlow::Freshness::Pending
            }
            jet_std::DataFreshness::Fresh => jet_foundation::PreludeDataFlow::Freshness::Fresh,
            jet_std::DataFreshness::Stale => jet_foundation::PreludeDataFlow::Freshness::Stale,
            jet_std::DataFreshness::Error => jet_foundation::PreludeDataFlow::Freshness::Error,
            jet_std::DataFreshness::Offline => {
                jet_foundation::PreludeDataFlow::Freshness::Offline
            }
            jet_std::DataFreshness::Cancelled => {
                jet_foundation::PreludeDataFlow::Freshness::Cancelled
            }
        },
        invalidated_by: match status.invalidated_by {
            jet_std::DataInvalidationCause::None => {
                jet_foundation::PreludeDataFlow::InvalidationCause::None
            }
            jet_std::DataInvalidationCause::Loader => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Loader
            }
            jet_std::DataInvalidationCause::Input => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Input
            }
            jet_std::DataInvalidationCause::ArchiveMember => {
                jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember
            }
            jet_std::DataInvalidationCause::Parameters => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Parameters
            }
            jet_std::DataInvalidationCause::Credential => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Credential
            }
            jet_std::DataInvalidationCause::Capability => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Capability
            }
            jet_std::DataInvalidationCause::Manual => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Manual
            }
        },
        error: status.error.clone(),
        cleanup: status.cleanup.clone(),
        buffered_bytes: status.buffered_bytes,
        backpressure: status.backpressure,
        last_good: status.last_good,
    }
}

fn jet_data_loader_kernel_state<T>(
    loader: &jet_std::DataLoader<T>,
) -> jet_foundation::PreludeDataFlow::LoaderState {
    jet_foundation::PreludeDataFlow::LoaderState {
        source: jet_foundation::PreludeDataFlow::SourceIdentity {
            kind: jet_data_loader_kernel_kind(loader.source.kind),
            locator: loader.source.locator.clone(),
            member: loader.source.member.clone(),
            parameters: loader.source.parameters.clone(),
        },
        format: loader.format.as_str().to_string(),
        authority: jet_data_loader_kernel_authority(&loader.authority),
        limits: jet_data_loader_kernel_limits(&loader.limits),
        payload: loader.payload.clone(),
        last_good: loader.last_good.clone(),
        cancelled: loader.cancelled,
        offline: loader.offline,
        status: jet_data_loader_kernel_status(&loader.status),
        raw_locator: loader.raw_locator.clone(),
    }
}

fn jet_data_loader_apply_kernel<T>(
    loader: &mut jet_std::DataLoader<T>,
    state: jet_foundation::PreludeDataFlow::LoaderState,
) {
    loader.source.locator = state.source.locator;
    loader.source.member = state.source.member;
    loader.source.parameters = state.source.parameters;
    loader.authority.scope = state.authority.scope;
    loader.authority.revision = state.authority.revision;
    loader.payload = state.payload;
    loader.last_good = state.last_good;
    loader.cancelled = state.cancelled;
    loader.offline = state.offline;
    loader.status.identity = state.status.identity;
    loader.status.freshness = match state.status.freshness {
        jet_foundation::PreludeDataFlow::Freshness::Pending => jet_std::DataFreshness::Pending,
        jet_foundation::PreludeDataFlow::Freshness::Fresh => jet_std::DataFreshness::Fresh,
        jet_foundation::PreludeDataFlow::Freshness::Stale => jet_std::DataFreshness::Stale,
        jet_foundation::PreludeDataFlow::Freshness::Error => jet_std::DataFreshness::Error,
        jet_foundation::PreludeDataFlow::Freshness::Offline => jet_std::DataFreshness::Offline,
        jet_foundation::PreludeDataFlow::Freshness::Cancelled => {
            jet_std::DataFreshness::Cancelled
        }
    };
    loader.status.invalidated_by = match state.status.invalidated_by {
        jet_foundation::PreludeDataFlow::InvalidationCause::None => {
            jet_std::DataInvalidationCause::None
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::Loader => {
            jet_std::DataInvalidationCause::Loader
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::Input => {
            jet_std::DataInvalidationCause::Input
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember => {
            jet_std::DataInvalidationCause::ArchiveMember
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::Parameters => {
            jet_std::DataInvalidationCause::Parameters
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::Credential => {
            jet_std::DataInvalidationCause::Credential
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::Capability => {
            jet_std::DataInvalidationCause::Capability
        }
        jet_foundation::PreludeDataFlow::InvalidationCause::Manual => {
            jet_std::DataInvalidationCause::Manual
        }
    };
    loader.status.error = state.status.error;
    loader.status.cleanup = state.status.cleanup;
    loader.status.buffered_bytes = state.status.buffered_bytes;
    loader.status.backpressure = state.status.backpressure;
    loader.status.last_good = state.status.last_good;
    loader.raw_locator = state.raw_locator;
}

fn jet_data_loader_status_from_kernel(
    status: jet_foundation::PreludeDataFlow::Status,
) -> jet_std::DataLoaderStatus {
    jet_std::DataLoaderStatus {
        identity: status.identity,
        freshness: match status.freshness {
            jet_foundation::PreludeDataFlow::Freshness::Pending => {
                jet_std::DataFreshness::Pending
            }
            jet_foundation::PreludeDataFlow::Freshness::Fresh => jet_std::DataFreshness::Fresh,
            jet_foundation::PreludeDataFlow::Freshness::Stale => jet_std::DataFreshness::Stale,
            jet_foundation::PreludeDataFlow::Freshness::Error => jet_std::DataFreshness::Error,
            jet_foundation::PreludeDataFlow::Freshness::Offline => {
                jet_std::DataFreshness::Offline
            }
            jet_foundation::PreludeDataFlow::Freshness::Cancelled => {
                jet_std::DataFreshness::Cancelled
            }
        },
        invalidated_by: match status.invalidated_by {
            jet_foundation::PreludeDataFlow::InvalidationCause::None => {
                jet_std::DataInvalidationCause::None
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::Loader => {
                jet_std::DataInvalidationCause::Loader
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::Input => {
                jet_std::DataInvalidationCause::Input
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember => {
                jet_std::DataInvalidationCause::ArchiveMember
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::Parameters => {
                jet_std::DataInvalidationCause::Parameters
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::Credential => {
                jet_std::DataInvalidationCause::Credential
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::Capability => {
                jet_std::DataInvalidationCause::Capability
            }
            jet_foundation::PreludeDataFlow::InvalidationCause::Manual => {
                jet_std::DataInvalidationCause::Manual
            }
        },
        error: status.error,
        cleanup: status.cleanup,
        buffered_bytes: status.buffered_bytes,
        backpressure: status.backpressure,
        last_good: status.last_good,
    }
}

fn jet_data_loader_kernel_error(
    error: jet_foundation::PreludeDataFlow::KernelError,
) -> jet_std::DataError {
    let kind = match error.kind {
        jet_foundation::PreludeDataFlow::ErrorKind::InvalidArgument => {
            jet_std::DataErrorKind::InvalidArgument
        }
        jet_foundation::PreludeDataFlow::ErrorKind::Limit => jet_std::DataErrorKind::Limit,
        jet_foundation::PreludeDataFlow::ErrorKind::State => jet_std::DataErrorKind::State,
        jet_foundation::PreludeDataFlow::ErrorKind::Bridge => jet_std::DataErrorKind::Bridge,
    };
    jet_data_loader_error(kind, &error.operation, error.reason)
}

fn jet_data_loader_bounded_text(value: &str, max_bytes: usize) -> String {
    jet_foundation::PreludeDataFlow::bounded_text(value, max_bytes)
}

fn jet_data_loader_sensitive_key(key: &str) -> bool {
    jet_foundation::PreludeDataFlow::sensitive_key(key)
}

fn jet_data_loader_sensitive_value(value: &str) -> bool {
    jet_foundation::PreludeDataFlow::sensitive_value(value)
}

fn jet_data_loader_status_text(value: &str) -> String {
    jet_foundation::PreludeDataFlow::status_text(value)
}

fn jet_data_loader_error(
    kind: jet_std::DataErrorKind,
    operation: &str,
    reason: impl Into<String>,
) -> jet_std::DataError {
    let reason = reason.into();
    jet_data_error(
        kind,
        operation,
        jet_data_loader_bounded_text(&reason, JET_DATA_LOADER_MAX_STATUS_TEXT),
    )
}

fn jet_data_loader_public_parameter(parameter: &str) -> String {
    jet_foundation::PreludeDataFlow::public_parameter(parameter)
}

fn jet_data_loader_public_member(member: &str) -> String {
    jet_foundation::PreludeDataFlow::public_member(member)
}

fn jet_data_loader_public_locator(locator: &str) -> String {
    jet_foundation::PreludeDataFlow::public_locator(locator)
}

fn jet_data_loader_validate_public_text(
    label: &str,
    value: &str,
    allow_empty: bool,
) -> Result<(), jet_std::DataError> {
    jet_foundation::PreludeDataFlow::validate_public_text(label, value, allow_empty)
        .map_err(jet_data_loader_kernel_error)
}

fn jet_data_loader_authority(
    scope: &str,
    revision: &str,
) -> Result<jet_std::DataAuthority, jet_std::DataError> {
    jet_foundation::PreludeDataFlow::authority(scope, revision)
        .map(|authority| jet_std::DataAuthority {
            scope: authority.scope,
            revision: authority.revision,
        })
        .map_err(jet_data_loader_kernel_error)
}

fn jet_data_loader_format(format: &str) -> Result<jet_std::DataFormat, jet_std::DataError> {
    jet_std::DataFormat::from_str(format).ok_or_else(|| {
        jet_data_loader_error(
            jet_std::DataErrorKind::Bridge,
            "data.loader",
            format!("format `{format}` is provider-owned; import its package"),
        )
    })
}

fn jet_data_loader_source(
    kind: jet_std::DataLoaderKind,
    locator: &str,
    member: &str,
    parameters: &[String],
) -> jet_std::DataSourceIdentity {
    let source = jet_foundation::PreludeDataFlow::source(
        jet_data_loader_kernel_kind(kind),
        locator,
        member,
        parameters,
    );
    jet_std::DataSourceIdentity {
        kind,
        locator: source.locator,
        member: source.member,
        parameters: source.parameters,
    }
}

fn jet_data_loader_new<T>(
    kind: jet_std::DataLoaderKind,
    locator: String,
    member: String,
    parameters: Vec<String>,
    format: jet_std::DataFormat,
    limits: jet_std::DataLimits,
    authority: jet_std::DataAuthority,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    let state = jet_foundation::PreludeDataFlow::new_loader(
        jet_data_loader_kernel_kind(kind),
        locator,
        member,
        parameters,
        format.as_str().to_string(),
        jet_data_loader_kernel_limits(&limits),
        jet_data_loader_kernel_authority(&authority),
    )
    .map_err(jet_data_loader_kernel_error)?;
    Ok(jet_std::DataLoader {
        source: jet_std::DataSourceIdentity {
            kind,
            locator: state.source.locator,
            member: state.source.member,
            parameters: state.source.parameters,
        },
        format,
        authority: jet_std::DataAuthority {
            scope: state.authority.scope,
            revision: state.authority.revision,
        },
        limits,
        payload: state.payload,
        last_good: state.last_good,
        cancelled: state.cancelled,
        offline: state.offline,
        status: jet_std::DataLoaderStatus {
            identity: state.status.identity,
            freshness: jet_std::DataFreshness::Pending,
            invalidated_by: jet_std::DataInvalidationCause::None,
            error: state.status.error,
            cleanup: state.status.cleanup,
            buffered_bytes: state.status.buffered_bytes,
            backpressure: state.status.backpressure,
            last_good: state.status.last_good,
        },
        raw_locator: state.raw_locator,
        marker: std::marker::PhantomData,
    })
}

fn jet_data_loader_validate_runtime<T>(
    loader: &jet_std::DataLoader<T>,
) -> Result<(), jet_std::DataError> {
    jet_foundation::PreludeDataFlow::validate_loader(&jet_data_loader_kernel_state(loader))
        .map_err(jet_data_loader_kernel_error)
}

fn jet_data_loader_format_for_locator(
    locator: &str,
) -> Result<jet_std::DataFormat, jet_std::DataError> {
    let path = locator
        .split(|character| character == '?' || character == '#')
        .next()
        .unwrap_or(locator);
    let path = path.rsplit_once('/').map_or(path, |(_, path)| path);
    let Some(extension) = path.rsplit_once('.').map(|(_, extension)| extension) else {
        return Err(jet_data_loader_error(
            jet_std::DataErrorKind::Bridge,
            "data.load",
            "source has no core format; import a provider package",
        ));
    };
    jet_data_loader_format(extension)
}

fn jet_data_loader_load<T>(
    locator: &String,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    let format = jet_data_loader_format_for_locator(locator)?;
    if locator.starts_with("http://") || locator.starts_with("https://") {
        return jet_data_loader_url(
            locator,
            &format.as_str().to_string(),
            &"network".to_string(),
            limits,
        );
    }
    jet_data_loader_file(locator, &format.as_str().to_string(), limits)
}

fn jet_data_loader_load_default<T>(
    locator: &String,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    jet_data_loader_load(locator, &jet_std::DataLimits::safe())
}

fn jet_data_loader_file<T>(
    path: &String,
    format: &String,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    jet_data_loader_new(
        jet_std::DataLoaderKind::File,
        path.clone(),
        String::new(),
        Vec::new(),
        jet_data_loader_format(format)?,
        limits.clone(),
        jet_data_loader_authority("local", "")?,
    )
}

fn jet_data_loader_file_member<T>(
    path: &String,
    member: &String,
    format: &String,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    jet_data_loader_new(
        jet_std::DataLoaderKind::File,
        path.clone(),
        member.clone(),
        Vec::new(),
        jet_data_loader_format(format)?,
        limits.clone(),
        jet_data_loader_authority("local", "")?,
    )
}

fn jet_data_loader_url<T>(
    url: &String,
    format: &String,
    authority: &String,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    jet_data_loader_new(
        jet_std::DataLoaderKind::Url,
        url.clone(),
        String::new(),
        Vec::new(),
        jet_data_loader_format(format)?,
        limits.clone(),
        jet_data_loader_authority(authority, "")?,
    )
}

fn jet_data_loader_database<T>(
    query: &String,
    parameters: &Vec<String>,
    authority: &String,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    jet_data_loader_new(
        jet_std::DataLoaderKind::Database,
        query.clone(),
        String::new(),
        parameters.clone(),
        jet_std::DataFormat::JSON,
        limits.clone(),
        jet_data_loader_authority(authority, "")?,
    )
}

fn jet_data_loader_value<T: __jet_Encode>(
    value: &T,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataLoader<T>, jet_std::DataError> {
    let content = jet_data_loader_canonical_tree(&value.jet_encode());
    let bytes = content.into_bytes();
    jet_data_loader_check_payload(limits, bytes.len(), "data.value")?;
    let mut loader = jet_data_loader_new(
        jet_std::DataLoaderKind::Value,
        "value".to_string(),
        String::new(),
        Vec::new(),
        jet_std::DataFormat::JSON,
        limits.clone(),
        jet_data_loader_authority("local", "")?,
    )?;
    loader.payload = Some(bytes);
    Ok(loader)
}

fn jet_data_loader_with_authority<T>(
    loader: &mut jet_std::DataLoader<T>,
    scope: &String,
    revision: &String,
) -> Result<(), jet_std::DataError> {
    let mut state = jet_data_loader_kernel_state(loader);
    jet_foundation::PreludeDataFlow::set_authority(&mut state, scope, revision)
        .map_err(jet_data_loader_kernel_error)?;
    jet_data_loader_apply_kernel(loader, state);
    Ok(())
}

fn jet_data_loader_check_payload(
    limits: &jet_std::DataLimits,
    bytes: usize,
    operation: &str,
) -> Result<(), jet_std::DataError> {
    jet_foundation::PreludeDataFlow::check_payload(
        &jet_data_loader_kernel_limits(limits),
        bytes,
        operation,
    )
    .map_err(jet_data_loader_kernel_error)
}

fn jet_data_loader_quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn jet_data_loader_canonical_tree(tree: &jet_std::DataTree) -> String {
    match tree {
        jet_std::DataTree::Null => "null".to_string(),
        jet_std::DataTree::Bool(value) => value.to_string(),
        jet_std::DataTree::Int(value) => jet_std::jet_int_to_string(*value),
        jet_std::DataTree::Float(value) => format!("{value:?}"),
        jet_std::DataTree::Number(value) => value.clone(),
        jet_std::DataTree::TypedText(value) | jet_std::DataTree::Text(value) => {
            jet_data_loader_quote(value)
        }
        jet_std::DataTree::Bytes(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        jet_std::DataTree::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(jet_data_loader_canonical_tree)
                .collect::<Vec<_>>()
                .join(",")
        ),
        jet_std::DataTree::Object(entries) => {
            let mut entries = entries.clone();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            format!(
                "{{{}}}",
                entries
                    .iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        jet_data_loader_quote(key),
                        jet_data_loader_canonical_tree(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn jet_data_loader_schema(
    tree: &jet_std::DataTree,
    format: jet_std::DataFormat,
) -> jet_std::DataSchema {
    let mut schema = jet_std::DataSchema::infer(tree, format);
    schema.identity = jet_data_loader_digest(schema.identity.as_bytes());
    schema
}

fn jet_data_loader_digest(bytes: &[u8]) -> String {
    jet_foundation::PreludeDataFlow::digest(bytes)
}

fn jet_data_loader_identity_part(output: &mut String, value: &str) {
    jet_foundation::PreludeDataFlow::identity_part(output, value);
}

fn jet_data_loader_csv_tree(
    text: &str,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataTree, jet_std::DataError> {
    let text = text.to_string();
    let rows = jet_ring_csv_parse(&text, &",".to_string(), false, false).map_err(|reason| {
        jet_data_loader_error(
            jet_std::DataErrorKind::Decode,
            "data.loader.csv",
            reason,
        )
    })?;
    let mut rows = rows.into_iter();
    let Some(header) = rows.next() else {
        return Ok(jet_std::DataTree::Array(Vec::new()));
    };
    let mut header_names = std::collections::BTreeSet::new();
    for name in &header {
        jet_data_loader_validate_public_text("CSV header", name, false)?;
        if !header_names.insert(name.clone()) {
            return Err(jet_data_loader_error(
                jet_std::DataErrorKind::Decode,
                "data.loader.csv",
                format!("duplicate CSV header `{name}`"),
            ));
        }
    }
    let mut values = Vec::new();
    for row in rows {
        if values.len() as i64 >= limits.max_output_rows {
            return Err(jet_data_loader_error(
                jet_std::DataErrorKind::Limit,
                "data.loader.csv",
                format!("max_output_rows {} exceeded", limits.max_output_rows),
            ));
        }
        if row.len() > header.len() {
            return Err(jet_data_loader_error(
                jet_std::DataErrorKind::Decode,
                "data.loader.csv",
                format!(
                    "CSV row has {} fields; header has {}",
                    row.len(),
                    header.len()
                ),
            ));
        }
        let fields = header
            .iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    name.clone(),
                    row.get(index)
                        .map(|value| jet_std::DataTree::Text(value.clone()))
                        .unwrap_or(jet_std::DataTree::Null),
                )
            })
            .collect();
        values.push(jet_std::DataTree::Object(fields));
    }
    Ok(jet_std::DataTree::Array(values))
}

fn jet_data_loader_jsonl_tree(
    text: &str,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataTree, jet_std::DataError> {
    let mut values = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if values.len() as i64 >= limits.max_output_rows {
            return Err(jet_data_loader_error(
                jet_std::DataErrorKind::Limit,
                "data.loader.jsonl",
                format!("max_output_rows {} exceeded", limits.max_output_rows),
            ));
        }
        let value = jet_std::parse_json_typed_datatree(line).map_err(|error| {
            jet_data_loader_error(
                jet_std::DataErrorKind::Decode,
                "data.loader.jsonl",
                match error.line {
                    Ok(line) => format!(
                        "invalid JSONL record at line {} (line {line}): {}",
                        line_index + 1,
                        error.reason
                    ),
                    Err(_) => format!(
                        "invalid JSONL record at line {}: {}",
                        line_index + 1,
                        error.reason
                    ),
                },
            )
        })?;
        values.push(value);
    }
    Ok(jet_std::DataTree::Array(values))
}


fn jet_data_loader_check_tree_rows(
    tree: &jet_std::DataTree,
    limits: &jet_std::DataLimits,
    operation: &str,
) -> Result<(), jet_std::DataError> {
    if let jet_std::DataTree::Array(rows) = tree {
        if rows.len() as i64 > limits.max_output_rows {
            return Err(jet_data_loader_error(
                jet_std::DataErrorKind::Limit,
                operation,
                format!("max_output_rows {} exceeded", limits.max_output_rows),
            ));
        }
    }
    Ok(())
}

// D-DATA-READER1=A: the hidden bridge exports the official Parquet provider
// function; this generated adapter registers that function with the one
// Foundation slot before asking Foundation to validate and project its Arrow
// C output. Arrow IPC stays a separate, unimplemented format boundary.
unsafe extern "C" {
    fn jet_data_read(
        format: u8,
        input: *const u8,
        input_len: usize,
        limits: *const jet_foundation::ArrowFileReader::ProviderLimits,
        out_schema: *mut *mut jet_foundation::ArrowData::ArrowSchema,
        out_array: *mut *mut jet_foundation::ArrowData::ArrowArray,
    ) -> i32;
}

fn jet_data_arrow_limits(
    limits: &jet_std::DataLimits,
) -> Result<jet_foundation::ArrowData::ArrowLimits, jet_std::DataError> {
    let max_buffer_bytes = usize::try_from(limits.encoding.max_expansion_bytes).map_err(|_| {
        jet_data_loader_error(
            jet_std::DataErrorKind::Limit,
            "data.loader.parquet",
            "max_expansion_bytes cannot be represented by this target",
        )
    })?;
    let max_total_i64 = limits
        .encoding
        .max_total_bytes
        .ok()
        .unwrap_or_else(|| {
            limits
                .encoding
                .max_item_bytes
                .max(limits.encoding.max_expansion_bytes)
        });
    let max_total_bytes = usize::try_from(max_total_i64).map_err(|_| {
        jet_data_loader_error(
            jet_std::DataErrorKind::Limit,
            "data.loader.parquet",
            "max_total_bytes cannot be represented by this target",
        )
    })?;
    Ok(jet_foundation::ArrowData::ArrowLimits {
        max_rows: limits.max_output_rows,
        max_columns: 1024,
        max_children: limits.encoding.max_depth,
        max_dictionary_values: limits.max_output_rows,
        max_buffer_bytes,
        max_total_bytes,
    })
}

fn jet_data_arrow_error(error: jet_foundation::ArrowData::ArrowError) -> jet_std::DataError {
    let kind = match error.kind {
        jet_foundation::ArrowData::ArrowErrorCode::Limit => jet_std::DataErrorKind::Limit,
        jet_foundation::ArrowData::ArrowErrorCode::ProducerFailure => {
            jet_std::DataErrorKind::Bridge
        }
        _ => jet_std::DataErrorKind::Decode,
    };
    jet_data_loader_error(kind, "data.loader.parquet", error.to_string())
}

fn jet_data_loader_parquet_tree(
    payload: &[u8],
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataTree, jet_std::DataError> {
    let arrow_limits = jet_data_arrow_limits(limits)?;
    // SAFETY: `jet_data_read` is the exact C ABI exported by the hidden
    // official bridge. Foundation owns registration and validates every
    // pointer, callback, offset, and buffer before returning this call.
    jet_foundation::ArrowFileReader::register(jet_data_read).map_err(jet_data_arrow_error)?;
    jet_foundation::ArrowFileReader::read_tree(
        jet_foundation::ArrowFileReader::FORMAT_PARQUET,
        payload,
        &arrow_limits,
    )
    .map_err(jet_data_arrow_error)
}

fn jet_data_loader_tree(
    payload: &[u8],
    format: jet_std::DataFormat,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataTree, jet_std::DataError> {
    if format == jet_std::DataFormat::Parquet {
        return jet_data_loader_parquet_tree(payload, limits);
    }
    if format == jet_std::DataFormat::Arrow {
        return Err(jet_data_loader_error(
            jet_std::DataErrorKind::Bridge,
            "data.loader",
            "Arrow IPC has a separate provider boundary; Parquet is the registered reader",
        ));
    }
    let text = String::from_utf8(payload.to_vec()).map_err(|error| {
        jet_data_loader_error(
            jet_std::DataErrorKind::Decode,
            "data.loader",
            format!("payload is not UTF-8: {error}"),
        )
    })?;
    let tree = match format {
        jet_std::DataFormat::JSON => {
            jet_std::parse_json_typed_datatree(&text).map_err(|error| {
                jet_data_loader_error(
                    jet_std::DataErrorKind::Decode,
                    "data.loader.json",
                    match error.line {
                        Ok(line) => format!("invalid JSON (line {line}): {}", error.reason),
                        Err(_) => format!("invalid JSON: {}", error.reason),
                    }
                )
            })?
        }
        jet_std::DataFormat::CSV => jet_data_loader_csv_tree(&text, limits)?,
        jet_std::DataFormat::JSONL => jet_data_loader_jsonl_tree(&text, limits)?,
        jet_std::DataFormat::Parquet | jet_std::DataFormat::Arrow => unreachable!(),
    };
    jet_data_loader_check_tree_rows(&tree, limits, "data.loader")?;
    Ok(tree)
}

fn jet_data_loader_payload<T>(
    loader: &jet_std::DataLoader<T>,
) -> Result<(Vec<u8>, bool), jet_std::DataError> {
    let state = jet_data_loader_kernel_state(loader);
    if let Some(payload) = jet_foundation::PreludeDataFlow::payload(&state)
        .map_err(jet_data_loader_kernel_error)?
    {
        return Ok(payload);
    }
    if loader.source.kind == jet_std::DataLoaderKind::File
        && loader.source.member.is_empty()
    {
        let payload = std::fs::read(&loader.raw_locator).map_err(|error| {
            jet_data_loader_error(
                jet_std::DataErrorKind::IO,
                "data.loader.file",
                format!("could not read `{}`: {}", loader.source.locator, error),
            )
        })?;
        jet_data_loader_check_payload(&loader.limits, payload.len(), "data.loader.file")?;
        return Ok((payload, loader.offline));
    }
    let operation = match loader.source.kind {
        jet_std::DataLoaderKind::Url => "data.loader.url",
        jet_std::DataLoaderKind::Database => "data.loader.database",
        jet_std::DataLoaderKind::File => "data.loader.archive",
        jet_std::DataLoaderKind::Value => "data.loader.value",
    };
    Err(jet_data_loader_error(
        jet_std::DataErrorKind::Bridge,
        operation,
        "provider payload is missing; core does not access remote authority",
    ))
}

fn jet_data_loader_fail<T>(
    loader: &mut jet_std::DataLoader<T>,
    error: jet_std::DataError,
) -> jet_std::DataError {
    let kind = match error.kind {
        jet_std::DataErrorKind::Limit => {
            jet_foundation::PreludeDataFlow::ErrorKind::Limit
        }
        jet_std::DataErrorKind::State => {
            jet_foundation::PreludeDataFlow::ErrorKind::State
        }
        jet_std::DataErrorKind::Bridge => {
            jet_foundation::PreludeDataFlow::ErrorKind::Bridge
        }
        _ => jet_foundation::PreludeDataFlow::ErrorKind::InvalidArgument,
    };
    let mut state = jet_data_loader_kernel_state(loader);
    jet_foundation::PreludeDataFlow::fail(
        &mut state,
        &jet_foundation::PreludeDataFlow::KernelError {
            kind,
            operation: error.operation.clone(),
            reason: error.reason.clone(),
        },
    );
    jet_data_loader_apply_kernel(loader, state);
    error
}


fn jet_data_loader_snapshot<T: __jet_Decode + __jet_Encode>(
    loader: &mut jet_std::DataLoader<T>,
) -> Result<jet_std::DataSnapshot<T>, jet_std::DataError> {
    if loader.cancelled {
        return Err(jet_data_loader_fail(
            loader,
            jet_data_loader_error(
                jet_std::DataErrorKind::State,
                "data.loader",
                "loader was cancelled before snapshot",
            ),
        ));
    }
    if let Err(error) = jet_data_loader_validate_runtime(loader) {
        return Err(jet_data_loader_fail(loader, error));
    }
    jet_data_limits_validate(&loader.limits).map_err(|error| jet_data_loader_fail(loader, error))?;
    let (payload, offline) = match jet_data_loader_payload(loader) {
        Ok(payload) => payload,
        Err(error) => return Err(jet_data_loader_fail(loader, error)),
    };
    let tree = match jet_data_loader_tree(&payload, loader.format, &loader.limits) {
        Ok(tree) => tree,
        Err(error) => return Err(jet_data_loader_fail(loader, error)),
    };
    let value = match T::jet_decode(&tree) {
        Ok(value) => value,
        Err(errors) => {
            return Err(jet_data_loader_fail(
                loader,
                jet_data_loader_error(
                    jet_std::DataErrorKind::Decode,
                    "data.loader",
                    jet_data_field_errors_reason(errors),
                ),
            ));
        }
    };
    let canonical = jet_data_loader_canonical_tree(&value.jet_encode()).into_bytes();
    let schema = jet_data_loader_schema(&tree, loader.format);
    let schema_id = schema.identity.clone();
    let provenance = jet_std::DataProvenance {
        source: loader.source.clone(),
        format: loader.format,
        authority: loader.authority.clone(),
    };
    let mut state = jet_data_loader_kernel_state(loader);
    let facts = match jet_foundation::PreludeDataFlow::commit_snapshot(
        &mut state,
        payload,
        &canonical,
        &schema_id,
        offline,
    ) {
        Ok(facts) => facts,
        Err(error) => {
            return Err(jet_data_loader_fail(
                loader,
                jet_data_loader_kernel_error(error),
            ));
        }
    };
    jet_data_loader_apply_kernel(loader, state);
    let status = loader.status.clone();
    let identity = jet_std::DataSnapshotIdentity {
        id: format!("snapshot-{}", facts.snapshot_id),
        source: format!("source-{}", facts.source_id),
        content: facts.content_id,
        schema: facts.schema_id,
        format: loader.format,
    };
    Ok(jet_std::DataSnapshot {
        value,
        identity,
        provenance,
        schema,
        status,
        content: canonical,
    })
}

fn jet_data_loader_bind<T>(
    loader: &mut jet_std::DataLoader<T>,
    payload: &Vec<u8>,
) -> Result<(), jet_std::DataError> {
    let mut state = jet_data_loader_kernel_state(loader);
    if let Err(error) =
        jet_foundation::PreludeDataFlow::bind(&mut state, payload.clone())
    {
        return Err(jet_data_loader_fail(
            loader,
            jet_data_loader_kernel_error(error),
        ));
    }
    jet_data_loader_apply_kernel(loader, state);
    Ok(())
}

fn jet_data_loader_bind_text<T>(
    loader: &mut jet_std::DataLoader<T>,
    payload: &String,
) -> Result<(), jet_std::DataError> {
    jet_data_loader_bind(loader, &payload.as_bytes().to_vec())
}

fn jet_data_loader_cancel<T>(loader: &mut jet_std::DataLoader<T>) {
    let mut state = jet_data_loader_kernel_state(loader);
    jet_foundation::PreludeDataFlow::cancel(&mut state);
    jet_data_loader_apply_kernel(loader, state);
}

fn jet_data_loader_offline<T>(
    loader: &mut jet_std::DataLoader<T>,
    enabled: bool,
) {
    let mut state = jet_data_loader_kernel_state(loader);
    jet_foundation::PreludeDataFlow::set_offline(&mut state, enabled);
    jet_data_loader_apply_kernel(loader, state);
}

fn jet_data_loader_invalidate<T>(
    loader: &mut jet_std::DataLoader<T>,
    cause: jet_std::DataInvalidationCause,
) {
    let mut state = jet_data_loader_kernel_state(loader);
    jet_foundation::PreludeDataFlow::invalidate(
        &mut state,
        match cause {
            jet_std::DataInvalidationCause::None => {
                jet_foundation::PreludeDataFlow::InvalidationCause::None
            }
            jet_std::DataInvalidationCause::Loader => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Loader
            }
            jet_std::DataInvalidationCause::Input => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Input
            }
            jet_std::DataInvalidationCause::ArchiveMember => {
                jet_foundation::PreludeDataFlow::InvalidationCause::ArchiveMember
            }
            jet_std::DataInvalidationCause::Parameters => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Parameters
            }
            jet_std::DataInvalidationCause::Credential => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Credential
            }
            jet_std::DataInvalidationCause::Capability => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Capability
            }
            jet_std::DataInvalidationCause::Manual => {
                jet_foundation::PreludeDataFlow::InvalidationCause::Manual
            }
        },
    );
    jet_data_loader_apply_kernel(loader, state);
}

fn jet_data_loader_needs_refresh<T>(loader: &jet_std::DataLoader<T>) -> bool {
    jet_foundation::PreludeDataFlow::needs_refresh(&jet_data_loader_kernel_state(loader))
}

fn jet_data_loader_ready<T>(loader: &jet_std::DataLoader<T>) -> bool {
    jet_foundation::PreludeDataFlow::ready(&jet_data_loader_kernel_state(loader))
}

fn jet_data_loader_status<T>(
    loader: &jet_std::DataLoader<T>,
) -> jet_std::DataLoaderStatus {
    jet_data_loader_status_from_kernel(jet_foundation::PreludeDataFlow::status(
        &jet_data_loader_kernel_state(loader),
    ))
}

fn jet_data_loader_source_identity<T>(
    loader: &jet_std::DataLoader<T>,
) -> jet_std::DataSourceIdentity {
    loader.source.clone()
}

fn jet_data_loader_authority_of<T>(
    loader: &jet_std::DataLoader<T>,
) -> jet_std::DataAuthority {
    loader.authority.clone()
}

fn jet_data_snapshot_reusable(
    previous: &jet_std::DataSnapshotIdentity,
    current: &jet_std::DataSnapshotIdentity,
) -> bool {
    previous.id == current.id
        && previous.source == current.source
        && previous.content == current.content
        && previous.schema == current.schema
        && previous.format == current.format
}

fn jet_data_loader_stream_from_payload<T>(
    loader: &mut jet_std::DataLoader<T>,
    payload: Vec<u8>,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    let tree = match jet_data_loader_tree(&payload, loader.format, &loader.limits) {
        Ok(tree) => tree,
        Err(error) => return Err(jet_data_loader_fail(loader, error)),
    };
    let rows = match tree {
        jet_std::DataTree::Array(rows) => rows,
        value => vec![value],
    };
    if rows.len() as i64 > loader.limits.max_output_rows {
        return Err(jet_data_loader_fail(
            loader,
            jet_data_loader_error(
                jet_std::DataErrorKind::Limit,
                "data.loader.stream",
                format!("max_output_rows {} exceeded", loader.limits.max_output_rows),
            ),
        ));
    }
    let stream = jet_std::DataStream {
        inner: jet_std::DataStreamInner::Provider {
            format: loader.format,
            rows,
            cursor: 0,
        },
        limits: loader.limits.clone(),
        terminal: None,
        eof: false,
        row_index: 0,
        cancelled: false,
    };
    loader.status.buffered_bytes = i64::try_from(payload.len()).unwrap_or(i64::MAX);
    loader.status.backpressure =
        loader.status.buffered_bytes > loader.limits.encoding.buffer_bytes;
    Ok(stream)
}

fn jet_data_loader_stream<T>(
    loader: &mut jet_std::DataLoader<T>,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    if loader.cancelled {
        return Err(jet_data_loader_error(
            jet_std::DataErrorKind::State,
            "data.loader.stream",
            "loader was cancelled",
        ));
    }
    let kernel_state = jet_data_loader_kernel_state(loader);
    let payload = match jet_foundation::PreludeDataFlow::payload(&kernel_state) {
        Ok(Some((payload, _offline))) => Some(payload),
        Ok(None) => None,
        Err(error) => {
            return Err(jet_data_loader_fail(
                loader,
                jet_data_loader_kernel_error(error),
            ))
        }
    };
    if let Some(payload) = payload {
        return jet_data_loader_stream_from_payload(loader, payload);
    }
    if loader.format == jet_std::DataFormat::Parquet {
        let payload = match jet_data_loader_payload(loader) {
            Ok((payload, _offline)) => payload,
            Err(error) => return Err(jet_data_loader_fail(loader, error)),
        };
        return jet_data_loader_stream_from_payload(loader, payload);
    }
    if loader.source.kind != jet_std::DataLoaderKind::File || !loader.source.member.is_empty() {
        return Err(jet_data_loader_fail(
            loader,
            jet_data_loader_error(
                jet_std::DataErrorKind::Bridge,
                "data.loader.stream",
                "provider-owned sources must expose a bounded DataStream directly",
            ),
        ));
    }
    let raw_locator = loader.raw_locator.clone();
    let source_locator = loader.source.locator.clone();
    let file = std::fs::File::open(&raw_locator).map_err(|error| {
        jet_data_loader_error(
            jet_std::DataErrorKind::IO,
            "data.loader.stream",
            format!("could not open `{source_locator}`: {error}"),
        )
    });
    let file = match file {
        Ok(file) => file,
        Err(error) => return Err(jet_data_loader_fail(loader, error)),
    };
    let input = JetFileReader {
        inner: std::io::BufReader::new(file),
        path: raw_locator,
    };
    let stream = match loader.format {
        jet_std::DataFormat::CSV => jet_data_csv_reader(input, loader.limits.clone()),
        jet_std::DataFormat::JSON => jet_data_json_reader(input, loader.limits.clone()),
        jet_std::DataFormat::JSONL => jet_data_jsonl_reader(input, loader.limits.clone()),
        jet_std::DataFormat::Parquet => unreachable!(),
        jet_std::DataFormat::Arrow => Err(jet_data_loader_error(
            jet_std::DataErrorKind::Bridge,
            "data.loader.stream",
            "Arrow IPC has a separate provider boundary; Parquet is the registered reader",
        )),
    };
    let stream = match stream {
        Ok(stream) => stream,
        Err(error) => return Err(jet_data_loader_fail(loader, error)),
    };
    loader.status.buffered_bytes = 0;
    loader.status.backpressure = false;
    Ok(stream)
}

fn jet_data_stream_cancel(stream: &mut jet_std::DataStream) {
    stream.cancelled = true;
    if stream.terminal.is_none() {
        stream.terminal = Some(jet_data_error(
            jet_std::DataErrorKind::State,
            "data.stream",
            "stream was cancelled",
        ));
    }
}

impl JetShow for jet_std::DataFormat {
    fn jet_show(&self) -> String {
        self.as_str().to_string()
    }
}

impl JetShow for jet_std::DataStatus {
    fn jet_show(&self) -> String {
        format!(
            "DataStatus(step: {}, path: {}, copy: {}, ownership: {}, trust: {}, fallback: {}, replacement: {})",
            jet_data_loader_bounded_text(&self.step, JET_DATA_LOADER_MAX_STATUS_TEXT),
            jet_data_loader_bounded_text(&self.path, JET_DATA_LOADER_MAX_STATUS_TEXT),
            jet_data_loader_bounded_text(&self.copy, JET_DATA_LOADER_MAX_STATUS_TEXT),
            jet_data_loader_bounded_text(&self.ownership, JET_DATA_LOADER_MAX_STATUS_TEXT),
            jet_data_loader_bounded_text(&self.trust, JET_DATA_LOADER_MAX_STATUS_TEXT),
            jet_data_loader_bounded_text(&self.fallback, JET_DATA_LOADER_MAX_STATUS_TEXT),
            jet_data_loader_bounded_text(&self.replacement, JET_DATA_LOADER_MAX_STATUS_TEXT),
        )
    }
}

impl JetDisplay for jet_std::DataStatus {
    fn jet_display(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetDebug for jet_std::DataStatus {
    fn jet_debug(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetShow for jet_std::DataLoaderKind {
    fn jet_show(&self) -> String {
        format!("{self:?}")
    }
}

impl JetShow for jet_std::DataFreshness {
    fn jet_show(&self) -> String {
        format!("{self:?}")
    }
}

impl JetShow for jet_std::DataInvalidationCause {
    fn jet_show(&self) -> String {
        format!("{self:?}")
    }
}

impl JetShow for jet_std::DataAuthority {
    fn jet_show(&self) -> String {
        let scope = if jet_data_loader_sensitive_value(&self.scope) {
            "<redacted>".to_string()
        } else {
            jet_data_loader_bounded_text(&self.scope, JET_DATA_LOADER_MAX_PUBLIC_TEXT)
        };
        let revision = if jet_data_loader_sensitive_value(&self.revision) {
            "<redacted>".to_string()
        } else {
            jet_data_loader_bounded_text(&self.revision, JET_DATA_LOADER_MAX_PUBLIC_TEXT)
        };
        if revision.is_empty() {
            scope
        } else {
            format!("{scope}@{revision}")
        }
    }
}

impl JetShow for jet_std::DataSourceIdentity {
    fn jet_show(&self) -> String {
        let member = if self.member.is_empty() {
            String::new()
        } else {
            format!("#{member}", member = jet_data_loader_public_member(&self.member))
        };
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| jet_data_loader_public_parameter(parameter))
            .collect::<Vec<_>>();
        let parameters = if parameters.is_empty() {
            String::new()
        } else {
            format!("?{}", parameters.join("&"))
        };
        let locator = jet_data_loader_public_locator(&self.locator);
        format!("{:?}:{}{}{}", self.kind, locator, member, parameters)
    }
}

impl JetShow for jet_std::DataLoaderStatus {
    fn jet_show(&self) -> String {
        format!(
            "LoaderStatus {{ identity: {}, freshness: {:?}, invalidated_by: {:?}, error: {:?}, cleanup: {}, buffered_bytes: {}, backpressure: {}, last_good: {} }}",
            jet_data_loader_bounded_text(&self.identity, JET_DATA_LOADER_MAX_STATUS_TEXT),
            self.freshness,
            self.invalidated_by,
            jet_data_loader_status_text(&self.error),
            jet_data_loader_bounded_text(&self.cleanup, JET_DATA_LOADER_MAX_STATUS_TEXT),
            self.buffered_bytes.max(0),
            self.backpressure,
            self.last_good
        )
    }
}

impl JetDisplay for jet_std::DataLoaderStatus {
    fn jet_display(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetDebug for jet_std::DataLoaderStatus {
    fn jet_debug(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetShow for jet_std::DataSnapshotIdentity {
    fn jet_show(&self) -> String {
        format!(
            "SnapshotIdentity {{ id: {}, source: {}, content: {}, schema: {}, format: {} }}",
            self.id,
            self.source,
            self.content,
            self.schema,
            self.format.as_str()
        )
    }
}

/// D-COLUMNAR-BOUNDARY1=A: decode only through the row model, then hand the
/// values to the ordinary deferred query carrier. Grouping, limits, and
/// ordering remain the existing DataQuery implementation. The typed Arrow
/// carrier is consumed here so its single release owner cannot be cloned.
fn jet_data_query_arrow<T>(
    batch: jet_std::DataArrowBatch<T>,
) -> Result<jet_std::DataQuery<T>, jet_std::DataError>
where
    T: crate::jet_arrow_data::ArrowRow + Clone + 'static,
{
    let limits = jet_std::DataLimits::safe();
    jet_data_limits_validate(&limits)?;
    let max_rows = usize::try_from(limits.max_output_rows).map_err(|_| {
        jet_data_error(
            jet_std::DataErrorKind::Limit,
            "data.query_arrow",
            "max_output_rows cannot be represented by this target",
        )
    })?;
    if batch.row_count() > max_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "data.query_arrow",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    let mut rows = Vec::with_capacity(batch.row_count());
    for index in 0..batch.row_count() {
        let row = batch.row(index).map_err(|error| jet_std::DataError {
            kind: jet_std::DataErrorKind::Bridge,
            operation: "data.query_arrow".to_string(),
            row: Ok(index as i64),
            column: Err(JetAbsent),
            index: Err(JetAbsent),
            reason: error.to_string(),
            cause: Err(JetAbsent),
        })?;
        rows.push(row);
    }
    Ok(jet_data_query(&rows))
}
/// Consume a producer-vetted Arrow owner for the canonical data namespace.
/// Validation and release ownership are completed by Foundation before this
/// value reaches ordinary Jet code. Import is an ownership-preserving move;
/// no second cleanup authority is created.
fn jet_data_arrow_import<T>(
    batch: jet_std::DataArrowBatch<T>,
) -> jet_std::DataArrowBatch<T> {
    batch
}
// D-DATA-WEB1=A: the Web/Wasm adapter exposes one handle-free wire only.
// JavaScript owns transport and callback publication; this module retains the
// canonical DataLoader/DataQuery/DataStream state and all decode/limit policy.
#[cfg(target_arch = "wasm32")]
mod jet_data_web {
    use super::*;
    use std::cell::RefCell;

    trait WebTypedStream {
        fn next(&mut self) -> Result<jet_std::DataTree, jet_std::DataError>;
        fn collect(&mut self) -> Result<jet_std::DataTree, jet_std::DataError>;
        fn cancel(&mut self);
    }
    trait WebTypedLoader {
        fn bind(&mut self, payload: &Vec<u8>) -> Result<(), jet_std::DataError>;
        fn bind_text(&mut self, payload: &String) -> Result<(), jet_std::DataError>;
        fn snapshot(&mut self) -> Result<jet_std::DataTree, jet_std::DataError>;
        fn cancel(&mut self);
        fn offline(&mut self, enabled: bool);
        fn invalidate(&mut self, cause: jet_std::DataInvalidationCause);
        fn needs_refresh(&self) -> bool;
        fn ready(&self) -> bool;
        fn status(&self) -> jet_std::DataTree;
        fn source_identity(&self) -> jet_std::DataTree;
        fn authority(&self) -> jet_std::DataTree;
        fn stream(&mut self) -> Result<Box<dyn WebTypedStream>, jet_std::DataError>;
    }
    struct TypedStream<T> {
        stream: jet_std::DataStream,
        marker: std::marker::PhantomData<T>,
    }
    struct TypedLoader<T> {
        loader: jet_std::DataLoader<T>,
    }
    impl<T> WebTypedStream for TypedStream<T>
    where
        T: __jet_Decode + __jet_Encode + Clone + 'static,
    {
        fn next(&mut self) -> Result<jet_std::DataTree, jet_std::DataError> {
            match jet_data_stream_next::<T>(&mut self.stream)? {
                Ok(value) => Ok(result_tree(vec![
                    ("tag", text_tree("Some")),
                    ("values", jet_std::DataTree::Array(vec![value.jet_encode()])),
                ])),
                Err(_) => Ok(result_tree(vec![
                    ("tag", text_tree("None")),
                    ("values", jet_std::DataTree::Array(Vec::new())),
                ])),
            }
        }
        fn collect(&mut self) -> Result<jet_std::DataTree, jet_std::DataError> {
            Ok(jet_std::DataTree::Array(
                jet_data_stream_collect::<T>(&mut self.stream)?
                    .into_iter()
                    .map(|value| value.jet_encode())
                    .collect(),
            ))
        }
        fn cancel(&mut self) {
            jet_data_stream_cancel(&mut self.stream);
        }
    }
    impl<T> WebTypedLoader for TypedLoader<T>
    where
        T: __jet_Decode + __jet_Encode + Clone + 'static,
    {
        fn bind(&mut self, payload: &Vec<u8>) -> Result<(), jet_std::DataError> {
            jet_data_loader_bind(&mut self.loader, payload)
        }
        fn bind_text(&mut self, payload: &String) -> Result<(), jet_std::DataError> {
            jet_data_loader_bind_text(&mut self.loader, payload)
        }
        fn snapshot(&mut self) -> Result<jet_std::DataTree, jet_std::DataError> {
            Ok(snapshot(jet_data_loader_snapshot(&mut self.loader)?))
        }
        fn cancel(&mut self) {
            jet_data_loader_cancel(&mut self.loader);
        }
        fn offline(&mut self, enabled: bool) {
            jet_data_loader_offline(&mut self.loader, enabled);
        }
        fn invalidate(&mut self, cause: jet_std::DataInvalidationCause) {
            jet_data_loader_invalidate(&mut self.loader, cause);
        }
        fn needs_refresh(&self) -> bool {
            jet_data_loader_needs_refresh(&self.loader)
        }
        fn ready(&self) -> bool {
            jet_data_loader_ready(&self.loader)
        }
        fn status(&self) -> jet_std::DataTree {
            status(&jet_data_loader_status(&self.loader))
        }
        fn source_identity(&self) -> jet_std::DataTree {
            source_identity(&jet_data_loader_source_identity(&self.loader))
        }
        fn authority(&self) -> jet_std::DataTree {
            result_tree(vec![
                ("scope", text_tree(self.loader.authority.scope.clone())),
                ("revision", text_tree(self.loader.authority.revision.clone())),
            ])
        }
        fn stream(&mut self) -> Result<Box<dyn WebTypedStream>, jet_std::DataError> {
            Ok(Box::new(TypedStream::<T> {
                stream: jet_data_loader_stream(&mut self.loader)?,
                marker: std::marker::PhantomData,
            }))
        }
    }
    type WebTypeFactory = fn(&str, &[jet_std::DataTree]) -> Result<Handle, jet_std::DataError>;
    thread_local! {
        static WEB_TYPE_FACTORIES: RefCell<Vec<(String, WebTypeFactory)>> = const { RefCell::new(Vec::new()) };
    }
    pub fn register_type<T>(key: &'static str)
    where
        T: __jet_Decode + __jet_Encode + Clone + 'static,
    {
        WEB_TYPE_FACTORIES.with(|cell| {
            let mut factories = cell.borrow_mut();
            if !factories.iter().any(|(known, _)| known == key) {
                factories.push((key.to_string(), typed_factory::<T>));
            }
        });
    }
    enum Handle {
        Loader(jet_std::DataLoader<jet_std::DataTree>),
        TypedLoader(Box<dyn WebTypedLoader>),
        Query(jet_std::DataQuery<jet_std::DataTree>),
        Tracked(jet_std::DataTracked<jet_std::DataTree, String>),
        Watch(jet_std::DataWatch<jet_std::DataTree>),
        Grouped(jet_std::DataGroupedQuery<jet_std::DataTree, String>),
        Stream(jet_std::DataStream),
        TypedStream(Box<dyn WebTypedStream>),
    }

    thread_local! {
        static HANDLES: RefCell<Vec<Option<Handle>>> = const { RefCell::new(Vec::new()) };
        static INPUTS: RefCell<[Vec<u8>; 4]> = const { RefCell::new([Vec::new(), Vec::new(), Vec::new(), Vec::new()]) };
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn jet_data_web_callback_bool(callback: u32, pointer: u32, length: u32) -> u32;
        fn jet_data_web_callback_key(callback: u32, pointer: u32, length: u32) -> u64;
        fn jet_data_web_callback_value(callback: u32, pointer: u32, length: u32) -> u64;
    }

    fn quote(value: &str) -> String {
        jet_data_loader_quote(value)
    }
    fn error(kind: jet_std::DataErrorKind, operation: &str, reason: impl Into<String>) -> jet_std::DataError {
        jet_data_error(kind, operation, reason)
    }
    fn field<'a>(value: &'a jet_std::DataTree, name: &str) -> Option<&'a jet_std::DataTree> {
        match value {
            jet_std::DataTree::Object(entries) => entries.iter().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }
    fn args<'a>(root: &'a jet_std::DataTree) -> Result<&'a [jet_std::DataTree], jet_std::DataError> {
        match field(root, "args") {
            Some(jet_std::DataTree::Array(values)) => Ok(values),
            _ => Err(error(jet_std::DataErrorKind::InvalidArgument, "data.web", "wire args must be an array")),
        }
    }
    fn text(value: &jet_std::DataTree, operation: &str) -> Result<String, jet_std::DataError> {
        match value {
            jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value) => Ok(value.clone()),
            _ => Err(error(jet_std::DataErrorKind::InvalidArgument, operation, "expected text")),
        }
    }
    fn boolean(value: &jet_std::DataTree, operation: &str) -> Result<bool, jet_std::DataError> {
        match value {
            jet_std::DataTree::Bool(value) => Ok(*value),
            _ => Err(error(jet_std::DataErrorKind::InvalidArgument, operation, "expected bool")),
        }
    }
    fn integer(value: &jet_std::DataTree, operation: &str) -> Result<i64, jet_std::DataError> {
        match value {
            jet_std::DataTree::Int(value) => Ok(*value),
            jet_std::DataTree::Number(value) => value.parse::<i64>().map_err(|_| error(
                jet_std::DataErrorKind::InvalidArgument, operation, "expected exact integer",
            )),
            _ => Err(error(jet_std::DataErrorKind::InvalidArgument, operation, "expected exact integer")),
        }
    }
    fn handle(value: &jet_std::DataTree, operation: &str) -> Result<usize, jet_std::DataError> {
        let value = field(value, "handle").unwrap_or(value);
        let value = integer(value, operation)?;
        usize::try_from(value).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, operation, "invalid handle"))
    }
    fn limits(value: Option<&jet_std::DataTree>) -> Result<jet_std::DataLimits, jet_std::DataError> {
        let mut result = jet_std::DataLimits::safe();
        let Some(value) = value else { return Ok(result) };
        let Some(encoding) = field(value, "encoding") else { return Ok(result) };
        let number = |name: &str, fallback: i64| {
            field(encoding, name).and_then(|value| integer(value, "data.limits").ok()).unwrap_or(fallback)
        };
        result.encoding.buffer_bytes = number("buffer_bytes", result.encoding.buffer_bytes);
        result.encoding.max_depth = number("max_depth", result.encoding.max_depth);
        result.encoding.max_item_bytes = number("max_item_bytes", result.encoding.max_item_bytes);
        result.encoding.max_expansion_depth = number("max_expansion_depth", result.encoding.max_expansion_depth);
        result.encoding.max_expansion_bytes = number("max_expansion_bytes", result.encoding.max_expansion_bytes);
        for (name, target) in [
            ("max_groups", &mut result.max_groups),
            ("max_sort_rows", &mut result.max_sort_rows),
            ("max_join_rows", &mut result.max_join_rows),
            ("max_output_rows", &mut result.max_output_rows),
        ] {
            if let Some(value) = field(value, name) {
                *target = integer(value, "data.limits")?;
            }
        }
        Ok(result)
    }
    fn has_typed_loader(index: usize) -> bool {
        HANDLES.with(|cell| matches!(cell.borrow().get(index).and_then(Option::as_ref), Some(Handle::TypedLoader(_))))
    }
    fn has_typed_stream(index: usize) -> bool {
        HANDLES.with(|cell| matches!(cell.borrow().get(index).and_then(Option::as_ref), Some(Handle::TypedStream(_))))
    }
    fn with_typed_loader<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&mut dyn WebTypedLoader) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let mut handles = cell.borrow_mut();
            match handles.get_mut(index).and_then(Option::as_mut) {
                Some(Handle::TypedLoader(loader)) => body(loader.as_mut()),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a typed loader")),
            }
        })
    }
    fn with_typed_stream<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&mut dyn WebTypedStream) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let mut handles = cell.borrow_mut();
            match handles.get_mut(index).and_then(Option::as_mut) {
                Some(Handle::TypedStream(stream)) => body(stream.as_mut()),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a typed stream")),
            }
        })
    }
    fn bytes(value: &jet_std::DataTree, operation: &str) -> Result<Vec<u8>, jet_std::DataError> {
        let jet_std::DataTree::Array(values) = value else {
            return Err(error(jet_std::DataErrorKind::InvalidArgument, operation, "expected byte array"));
        };
        values.iter().map(|value| {
            let value = integer(value, operation)?;
            u8::try_from(value).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, operation, "byte outside 0..255"))
        }).collect()
    }
    fn format(value: &jet_std::DataTree, operation: &str) -> Result<String, jet_std::DataError> {
        text(value, operation)
    }
    fn result_tree(entries: Vec<(&str, jet_std::DataTree)>) -> jet_std::DataTree {
        jet_std::DataTree::Object(entries.into_iter().map(|(key, value)| (key.to_string(), value)).collect())
    }
    fn text_tree(value: impl Into<String>) -> jet_std::DataTree {
        jet_std::DataTree::Text(value.into())
    }
    fn handle_tree(handle: usize, kind: &str) -> jet_std::DataTree {
        result_tree(vec![
            ("handle", jet_std::DataTree::Int(handle as i64)),
            ("kind", text_tree(kind)),
        ])
    }
    fn store(handle: Handle) -> jet_std::DataTree {
        HANDLES.with(|cell| {
            let mut handles = cell.borrow_mut();
            let index = handles.iter().position(Option::is_none).unwrap_or_else(|| {
                handles.push(None);
                handles.len() - 1
            });
            let kind = match &handle {
                Handle::Loader(_) | Handle::TypedLoader(_) => "loader",
                Handle::Query(_) => "query",
                Handle::Tracked(_) => "tracked",
                Handle::Watch(_) => "watch",
                Handle::Grouped(_) => "grouped",
                Handle::Stream(_) | Handle::TypedStream(_) => "stream",
            };
            handles[index] = Some(handle);
            handle_tree(index, kind)
        })
    }
    fn with_loader<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&mut jet_std::DataLoader<jet_std::DataTree>) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let mut handles = cell.borrow_mut();
            match handles.get_mut(index).and_then(Option::as_mut) {
                Some(Handle::Loader(loader)) => body(loader),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a loader")),
            }
        })
    }
    fn clone_query(index: usize) -> Result<jet_std::DataQuery<jet_std::DataTree>, jet_std::DataError> {
        HANDLES.with(|cell| match cell.borrow().get(index).and_then(Option::as_ref) {
            Some(Handle::Query(query)) => Ok(query.clone()),
            _ => Err(error(jet_std::DataErrorKind::State, "data.query", "handle is not a query")),
        })
    }
    fn with_tracked<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&jet_std::DataTracked<jet_std::DataTree, String>) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let handles = cell.borrow();
            match handles.get(index).and_then(Option::as_ref) {
                Some(Handle::Tracked(tracked)) => body(tracked),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a tracked source")),
            }
        })
    }
    fn clone_tracked(
        index: usize,
        operation: &str,
    ) -> Result<jet_std::DataTracked<jet_std::DataTree, String>, jet_std::DataError> {
        with_tracked(index, operation, |tracked| Ok(tracked.clone()))
    }
    fn with_watch<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&jet_std::DataWatch<jet_std::DataTree>) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let handles = cell.borrow();
            match handles.get(index).and_then(Option::as_ref) {
                Some(Handle::Watch(watch)) => body(watch),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a watch")),
            }
        })
    }
    fn with_grouped<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&jet_std::DataGroupedQuery<jet_std::DataTree, String>) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let handles = cell.borrow();
            match handles.get(index).and_then(Option::as_ref) {
                Some(Handle::Grouped(grouped)) => body(grouped),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a grouped query")),
            }
        })
    }
    fn with_stream<R>(
        index: usize,
        operation: &str,
        body: impl FnOnce(&mut jet_std::DataStream) -> Result<R, jet_std::DataError>,
    ) -> Result<R, jet_std::DataError> {
        HANDLES.with(|cell| {
            let mut handles = cell.borrow_mut();
            match handles.get_mut(index).and_then(Option::as_mut) {
                Some(Handle::Stream(stream)) => body(stream),
                _ => Err(error(jet_std::DataErrorKind::State, operation, "handle is not a stream")),
            }
        })
    }
    fn callback_filter(callback: u32, row: jet_std::DataTree) -> bool {
        let encoded = jet_data_loader_canonical_tree(&row);
        unsafe { jet_data_web_callback_bool(callback, encoded.as_ptr() as usize as u32, encoded.len() as u32) != 0 }
    }
    fn callback_key(callback: u32, row: jet_std::DataTree) -> String {
        let encoded = jet_data_loader_canonical_tree(&row);
        let packed = unsafe { jet_data_web_callback_key(callback, encoded.as_ptr() as usize as u32, encoded.len() as u32) };
        if packed == 0 { return String::new() }
        let pointer = (packed >> 32) as u32;
        let length = packed as u32;
        let value = unsafe { std::slice::from_raw_parts(pointer as *const u8, length as usize) };
        let value = String::from_utf8_lossy(value).into_owned();
        INPUTS.with(|cell| cell.borrow_mut()[3].clear());
        value
    }
    fn callback_value(
        callback: u32,
        row: jet_std::DataTree,
        operation: &str,
    ) -> Result<jet_std::DataTree, jet_std::DataError> {
        let encoded = jet_data_loader_canonical_tree(&row);
        let packed = unsafe {
            jet_data_web_callback_value(
                callback,
                encoded.as_ptr() as usize as u32,
                encoded.len() as u32,
            )
        };
        if packed == 0 {
            return Err(error(
                jet_std::DataErrorKind::InvalidValue,
                operation,
                "callback did not return a value",
            ));
        }
        let pointer = (packed >> 32) as u32;
        let length = packed as u32;
        let bytes = unsafe { std::slice::from_raw_parts(pointer as *const u8, length as usize) };
        let value = match std::str::from_utf8(bytes) {
            Ok(value) => value.to_owned(),
            Err(_) => {
                INPUTS.with(|cell| cell.borrow_mut()[3].clear());
                return Err(error(
                    jet_std::DataErrorKind::Decode,
                    operation,
                    "callback returned invalid UTF-8",
                ));
            }
        };
        INPUTS.with(|cell| cell.borrow_mut()[3].clear());
        let value = jet_std::parse_json_typed_datatree(&value).map_err(|_| {
            error(
                jet_std::DataErrorKind::Decode,
                operation,
                "callback returned invalid JSON",
            )
        })?;
        Ok(value)
    }
    fn callback_number(
        callback: u32,
        row: jet_std::DataTree,
        operation: &str,
    ) -> Result<f64, jet_std::DataError> {
        match callback_value(callback, row, operation)? {
            jet_std::DataTree::Int(value) => Ok(value as f64),
            jet_std::DataTree::Float(value) => Ok(value),
            jet_std::DataTree::Number(value) => value.parse::<f64>().map_err(|_| {
                error(
                    jet_std::DataErrorKind::InvalidValue,
                    operation,
                    "callback must return a finite number",
                )
            }),
            _ => Err(error(
                jet_std::DataErrorKind::InvalidValue,
                operation,
                "callback must return a number",
            )),
        }
    }
    fn callback_key_value(value: &jet_std::DataTree) -> String {
        match value {
            jet_std::DataTree::Text(value) | jet_std::DataTree::TypedText(value) => value.clone(),
            jet_std::DataTree::Int(value) => value.to_string(),
            jet_std::DataTree::Float(value) => value.to_string(),
            jet_std::DataTree::Number(value) => value.clone(),
            jet_std::DataTree::Bool(value) => value.to_string(),
            jet_std::DataTree::Null => "null".to_string(),
            _ => jet_data_loader_canonical_tree(value),
        }
    }
    fn data_status(value: &jet_std::DataStatus) -> jet_std::DataTree {
        result_tree(vec![
            ("step", text_tree(value.step.clone())),
            ("path", text_tree(value.path.clone())),
            ("copy", text_tree(value.copy.clone())),
            ("ownership", text_tree(value.ownership.clone())),
            ("trust", text_tree(value.trust.clone())),
            ("fallback", text_tree(value.fallback.clone())),
            ("replacement", text_tree(value.replacement.clone())),
        ])
    }
    fn status(value: &jet_std::DataLoaderStatus) -> jet_std::DataTree {
        result_tree(vec![
            ("identity", text_tree(value.identity.clone())),
            ("freshness", text_tree(format!("{:?}", value.freshness))),
            ("invalidated_by", text_tree(format!("{:?}", value.invalidated_by))),
            ("error", text_tree(value.error.clone())),
            ("cleanup", text_tree(value.cleanup.clone())),
            ("buffered_bytes", jet_std::DataTree::Int(value.buffered_bytes)),
            ("backpressure", jet_std::DataTree::Bool(value.backpressure)),
            ("last_good", jet_std::DataTree::Bool(value.last_good)),
        ])
    }
    fn source_identity(value: &jet_std::DataSourceIdentity) -> jet_std::DataTree {
        result_tree(vec![
            ("kind", result_tree(vec![
                ("tag", text_tree(value.kind.jet_show())),
                ("values", jet_std::DataTree::Array(Vec::new())),
            ])),
            ("locator", text_tree(value.locator.clone())),
            ("member", text_tree(value.member.clone())),
            ("parameters", jet_std::DataTree::Array(
                value.parameters.iter().map(|value| text_tree(value.clone())).collect(),
            )),
        ])
    }
    fn snapshot<T: __jet_Encode>(value: jet_std::DataSnapshot<T>) -> jet_std::DataTree {
        result_tree(vec![
            ("value", value.value.jet_encode()),
            ("identity", result_tree(vec![
                ("id", text_tree(value.identity.id)),
                ("source", text_tree(value.identity.source)),
                ("content", text_tree(value.identity.content)),
                ("schema", text_tree(value.identity.schema)),
                ("format", text_tree(value.identity.format.as_str())),
            ])),
            ("provenance", result_tree(vec![
                ("source", source_identity(&value.provenance.source)),
                ("format", text_tree(value.provenance.format.as_str())),
                ("authority", result_tree(vec![
                    ("scope", text_tree(value.provenance.authority.scope)),
                    ("revision", text_tree(value.provenance.authority.revision)),
                ])),
            ])),
            ("schema", text_tree(value.schema.identity)),
            ("status", status(&value.status)),
            ("content", jet_std::DataTree::Bytes(value.content)),
        ])
    }
    fn watch_status(value: &jet_std::DataWatchStatus) -> jet_std::DataTree {
        result_tree(vec![
            ("mode", text_tree(value.mode.clone())),
            ("revision", jet_std::DataTree::Int(value.revision)),
            ("retained_rows", jet_std::DataTree::Int(value.retained_rows)),
            ("recomputations", jet_std::DataTree::Int(value.recomputations)),
            ("active", jet_std::DataTree::Bool(value.active)),
            ("generation", jet_std::DataTree::Int(value.generation)),
            ("dirty", jet_std::DataTree::Bool(value.dirty)),
            ("error", text_tree(value.error.clone())),
            ("freshness_ms", jet_std::DataTree::Int(value.freshness_ms)),
            ("invalidation_cause", text_tree(value.invalidation_cause.clone())),
            ("refreshing", jet_std::DataTree::Bool(value.refreshing)),
            ("cancelled", jet_std::DataTree::Bool(value.cancelled)),
        ])
    }
    fn typed_factory<T>(
        operation: &str,
        values: &[jet_std::DataTree],
    ) -> Result<Handle, jet_std::DataError>
    where
        T: __jet_Decode + __jet_Encode + Clone + 'static,
    {
        let value = |index: usize| {
            values.get(index).ok_or_else(|| {
                error(
                    jet_std::DataErrorKind::InvalidArgument,
                    operation,
                    "missing argument",
                )
            })
        };
        let decode = |value: &jet_std::DataTree| {
            T::jet_decode(value).map_err(|errors| {
                error(
                    jet_std::DataErrorKind::Decode,
                    operation,
                    jet_data_field_errors_reason(errors),
                )
            })
        };
        let loader = match operation {
            "load" => {
                let locator = text(value(0)?, operation)?;
                jet_data_loader_load::<T>(&locator, &limits(values.get(1))?)?
            }
            "load_default" => {
                let locator = text(value(0)?, operation)?;
                jet_data_loader_load_default::<T>(&locator)?
            }
            "file" => {
                let path = text(value(0)?, operation)?;
                let format = format(value(1)?, operation)?;
                jet_data_loader_file::<T>(&path, &format, &limits(values.get(2))?)?
            }
            "file_member" => {
                let path = text(value(0)?, operation)?;
                let member = text(value(1)?, operation)?;
                let format = format(value(2)?, operation)?;
                jet_data_loader_file_member::<T>(&path, &member, &format, &limits(values.get(3))?)?
            }
            "url" => {
                let url = text(value(0)?, operation)?;
                let format = format(value(1)?, operation)?;
                let authority = text(value(2)?, operation)?;
                jet_data_loader_url::<T>(&url, &format, &authority, &limits(values.get(3))?)?
            }
            "database" => {
                let query = text(value(0)?, operation)?;
                let parameters = match value(1)? {
                    jet_std::DataTree::Array(values) => values
                        .iter()
                        .map(|value| text(value, operation))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(error(
                        jet_std::DataErrorKind::InvalidArgument,
                        operation,
                        "parameters must be an array",
                    )),
                };
                let authority = text(value(2)?, operation)?;
                jet_data_loader_database::<T>(
                    &query,
                    &parameters,
                    &authority,
                    &limits(values.get(3))?,
                )?
            }
            "value" => {
                let decoded = decode(value(0)?)?;
                jet_data_loader_value::<T>(&decoded, &limits(values.get(1))?)?
            }
            _ => {
                return Err(error(
                    jet_std::DataErrorKind::InvalidArgument,
                    operation,
                    "operation is not a typed loader constructor",
                ))
            }
        };
        Ok(Handle::TypedLoader(Box::new(TypedLoader { loader })))
    }
    fn type_key(root: &jet_std::DataTree, operation: &str) -> Result<String, jet_std::DataError> {
        let values = match field(root, "type_args") {
            Some(jet_std::DataTree::Array(values)) if values.len() == 1 => values,
            _ => return Err(error(
                jet_std::DataErrorKind::InvalidArgument,
                operation,
                "typed data operation requires one canonical T type argument",
            )),
        };
        let value = text(&values[0], operation)?;
        Ok(value
            .split_once(":abi=")
            .map_or(value.as_str(), |(name, _)| name)
            .to_string())
    }
    fn typed_handle(
        root: &jet_std::DataTree,
        operation: &str,
    ) -> Result<Handle, jet_std::DataError> {
        let key = type_key(root, operation)?;
        let values = args(root)?;
        WEB_TYPE_FACTORIES.with(|cell| {
            let factories = cell.borrow();
            let Some((_, factory)) = factories.iter().find(|(known, _)| known == &key) else {
                return Err(error(
                    jet_std::DataErrorKind::Bridge,
                    operation,
                    format!("typed data codec `{key}` is not registered"),
                ));
            };
            factory(operation, values)
        })
    }
    fn dispatch(root: &jet_std::DataTree) -> Result<jet_std::DataTree, jet_std::DataError> {
        let operation = text(field(root, "op").ok_or_else(|| error(jet_std::DataErrorKind::InvalidArgument, "data.web", "wire operation is missing"))?, "data.web")?;
        let values = args(root)?;
        let value = |index: usize| values.get(index).ok_or_else(|| error(jet_std::DataErrorKind::InvalidArgument, &operation, "missing argument"));
        let typed = matches!(
            operation.as_str(),
            "load"
                | "load_default"
                | "file"
                | "file_member"
                | "url"
                | "database"
                | "value"
                | "snapshot"
                | "stream"
                | "stream_next"
                | "stream_collect"
        );
        if typed {
            match field(root, "type_args") {
                Some(jet_std::DataTree::Array(values)) if values.len() == 1 => {}
                _ => {
                    return Err(error(
                        jet_std::DataErrorKind::InvalidArgument,
                        &operation,
                        "typed data operation requires one canonical T type argument",
                    ))
                }
            }
        }
        match operation.as_str() {
            "load" | "load_default" | "file" | "file_member" | "url" | "database" | "value" => {
                Ok(store(typed_handle(root, &operation)?))
            }
            "bind" => {
                let index = handle(value(0)?, &operation)?;
                let payload = bytes(value(1)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| loader.bind(&payload).map(|_| jet_std::DataTree::Null))
                } else {
                    with_loader(index, &operation, |loader| jet_data_loader_bind(loader, &payload).map(|_| jet_std::DataTree::Null))
                }
            }
            "bind_text" => {
                let index = handle(value(0)?, &operation)?;
                let payload = text(value(1)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| loader.bind_text(&payload).map(|_| jet_std::DataTree::Null))
                } else {
                    with_loader(index, &operation, |loader| jet_data_loader_bind_text(loader, &payload).map(|_| jet_std::DataTree::Null))
                }
            }
            "snapshot" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| loader.snapshot())
                } else {
                    with_loader(index, &operation, |loader| jet_data_loader_snapshot(loader).map(snapshot))
                }
            }
            "loader_cancel" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| { loader.cancel(); Ok(jet_std::DataTree::Null) })
                } else {
                    with_loader(index, &operation, |loader| { jet_data_loader_cancel(loader); Ok(jet_std::DataTree::Null) })
                }
            }
            "offline" => {
                let index = handle(value(0)?, &operation)?;
                let enabled = boolean(value(1)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| { loader.offline(enabled); Ok(jet_std::DataTree::Null) })
                } else {
                    with_loader(index, &operation, |loader| { jet_data_loader_offline(loader, enabled); Ok(jet_std::DataTree::Null) })
                }
            }
            "invalidate" => {
                let index = handle(value(0)?, &operation)?;
                let cause = text(value(1)?, &operation)?;
                let cause = match cause.as_str() {
                    "Loader" => jet_std::DataInvalidationCause::Loader,
                    "Input" => jet_std::DataInvalidationCause::Input,
                    "ArchiveMember" => jet_std::DataInvalidationCause::ArchiveMember,
                    "Parameters" => jet_std::DataInvalidationCause::Parameters,
                    "Credential" => jet_std::DataInvalidationCause::Credential,
                    "Capability" => jet_std::DataInvalidationCause::Capability,
                    "Manual" => jet_std::DataInvalidationCause::Manual,
                    _ => jet_std::DataInvalidationCause::None,
                };
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| { loader.invalidate(cause); Ok(jet_std::DataTree::Null) })
                } else {
                    with_loader(index, &operation, |loader| { jet_data_loader_invalidate(loader, cause); Ok(jet_std::DataTree::Null) })
                }
            }
            "needs_refresh" | "ready" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| Ok(jet_std::DataTree::Bool(if operation == "ready" { loader.ready() } else { loader.needs_refresh() })))
                } else {
                    with_loader(index, &operation, |loader| Ok(jet_std::DataTree::Bool(if operation == "ready" { jet_data_loader_ready(loader) } else { jet_data_loader_needs_refresh(loader) })))
                }
            }
            "loader_status" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| Ok(loader.status()))
                } else {
                    with_loader(index, &operation, |loader| Ok(status(&jet_data_loader_status(loader))))
                }
            }
            "source_identity" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| Ok(loader.source_identity()))
                } else {
                    with_loader(index, &operation, |loader| Ok(source_identity(&jet_data_loader_source_identity(loader))))
                }
            }
            "authority_of" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    with_typed_loader(index, &operation, |loader| Ok(loader.authority()))
                } else {
                    with_loader(index, &operation, |loader| Ok(result_tree(vec![("scope", text_tree(loader.authority.scope.clone())), ("revision", text_tree(loader.authority.revision.clone()))])))
                }
            }
            "stream" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_loader(index) {
                    Ok(store(Handle::TypedStream(with_typed_loader(index, &operation, |loader| loader.stream())?)))
                } else {
                    let stream = with_loader(index, &operation, jet_data_loader_stream)?;
                    Ok(store(Handle::Stream(stream)))
                }
            }
            "stream_next" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_stream(index) {
                    with_typed_stream(index, &operation, |stream| stream.next())
                } else {
                    with_stream(index, &operation, |stream| match jet_data_stream_next::<jet_std::DataTree>(stream)? {
                        Ok(value) => Ok(result_tree(vec![("tag", text_tree("Some")), ("values", jet_std::DataTree::Array(vec![value]))])),
                        Err(_) => Ok(result_tree(vec![("tag", text_tree("None")), ("values", jet_std::DataTree::Array(Vec::new()))])),
                    })
                }
            }
            "stream_collect" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_stream(index) {
                    with_typed_stream(index, &operation, |stream| stream.collect())
                } else {
                    with_stream(index, &operation, |stream| Ok(jet_std::DataTree::Array(
                        jet_data_stream_collect::<jet_std::DataTree>(stream)?,
                    )))
                }
            }
            "stream_cancel" => {
                let index = handle(value(0)?, &operation)?;
                if has_typed_stream(index) {
                    with_typed_stream(index, &operation, |stream| { stream.cancel(); Ok(jet_std::DataTree::Null) })
                } else {
                    with_stream(index, &operation, |stream| { jet_data_stream_cancel(stream); Ok(jet_std::DataTree::Null) })
                }
            }
            "track" => {
                let rows = match value(0)? {
                    jet_std::DataTree::Array(rows) => rows.clone(),
                    _ => {
                        return Err(error(
                            jet_std::DataErrorKind::InvalidArgument,
                            &operation,
                            "data.track expects an array",
                        ))
                    }
                };
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| {
                    error(
                        jet_std::DataErrorKind::InvalidArgument,
                        &operation,
                        "invalid callback",
                    )
                })?;
                let tracked =
                    jet_std::DataTracked::new(rows, move |row| callback_key(callback, row))?;
                Ok(store(Handle::Tracked(tracked)))
            }
            "query" => {
                let rows = match value(0)? { jet_std::DataTree::Array(rows) => rows.clone(), value => vec![value.clone()] };
                Ok(store(Handle::Query(jet_data_query(&rows))))
            }
            "tracked_query" => {
                let index = handle(value(0)?, &operation)?;
                let tracked = clone_tracked(index, &operation)?;
                Ok(store(Handle::Query(jet_std::DataQuery::from_tracked(&tracked))))
            }
            "track_insert" => {
                let index = handle(value(0)?, &operation)?;
                let row = value(1)?.clone();
                with_tracked(index, &operation, |tracked| tracked.insert(row))?;
                Ok(jet_std::DataTree::Null)
            }
            "track_replace" => {
                let index = handle(value(0)?, &operation)?;
                let key = callback_key_value(value(1)?);
                let row = value(2)?.clone();
                with_tracked(index, &operation, |tracked| tracked.replace(key, row))?;
                Ok(jet_std::DataTree::Null)
            }
            "track_remove" => {
                let index = handle(value(0)?, &operation)?;
                let key = callback_key_value(value(1)?);
                with_tracked(index, &operation, |tracked| tracked.remove(key))?;
                Ok(jet_std::DataTree::Null)
            }
            "filter" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = clone_query(index)?;
                Ok(store(Handle::Query(jet_data_query_filter(&query, move |row| callback_filter(callback, row)))))
            }
            "sort_by" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = clone_query(index)?;
                Ok(store(Handle::Query(jet_data_query_sort_by(&query, move |row| callback_key(callback, row)))))
            }
            "map" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = clone_query(index)?;
                Ok(store(Handle::Query(jet_data_query_map_checked(&query, move |row| {
                    callback_value(callback, row, "query.map")
                }))))
            }
            "min" | "max" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = clone_query(index)?;
                let values = if operation == "min" {
                    jet_data_query_min(&query, move |row| callback_key(callback, row))
                } else {
                    jet_data_query_max(&query, move |row| callback_key(callback, row))
                };
                Ok(store(Handle::Query(jet_data_query_map(&values, |value| text_tree(value)))))
            }
            "inner_join" | "left_join" => {
                let left = clone_query(handle(value(0)?, &operation)?)?;
                let right = clone_query(handle(value(1)?, &operation)?)?;
                let left_callback = u32::try_from(integer(value(2)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid left callback"))?;
                let right_callback = u32::try_from(integer(value(3)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid right callback"))?;
                if operation == "inner_join" {
                    let joined = jet_data_query_inner_join(
                        &left,
                        &right,
                        move |row| callback_key(left_callback, row),
                        move |row| callback_key(right_callback, row),
                    );
                    Ok(store(Handle::Query(jet_data_query_map(&joined, |row| {
                        result_tree(vec![("left", row.left), ("right", row.right)])
                    }))))
                } else {
                    let joined = jet_data_query_left_join(
                        &left,
                        &right,
                        move |row| callback_key(left_callback, row),
                        move |row| callback_key(right_callback, row),
                    );
                    Ok(store(Handle::Query(jet_data_query_map(&joined, |row| {
                        let right = match row.right {
                            Ok(value) => value,
                            Err(_) => jet_std::DataTree::Null,
                        };
                        result_tree(vec![("left", row.left), ("right", right)])
                    }))))
                }
            }
            "group_by" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = clone_query(index)?;
                let grouped = jet_data_query_group_by(&query, move |row| callback_key(callback, row));
                Ok(store(Handle::Grouped(grouped)))
            }
            "watch" => {
                let index = handle(value(0)?, &operation)?;
                let query = clone_query(index)?;
                Ok(store(Handle::Watch(jet_data_query_watch(&query)?)))
            }
            "watch_get" => {
                let index = handle(value(0)?, &operation)?;
                Ok(jet_std::DataTree::Array(with_watch(index, &operation, |watch| jet_data_watch_get(watch))?))
            }
            "watch_status" => {
                let index = handle(value(0)?, &operation)?;
                Ok(with_watch(index, &operation, |watch| Ok(watch_status(&jet_data_watch_status(watch))))?)
            }
            "watch_cancel" => {
                let index = handle(value(0)?, &operation)?;
                with_watch(index, &operation, |watch| {
                    jet_data_watch_cancel(watch);
                    Ok(())
                })?;
                Ok(jet_std::DataTree::Null)
            }
            "group_count" => {
                let index = handle(value(0)?, &operation)?;
                let query = with_grouped(index, &operation, |grouped| Ok(jet_data_group_count_query(grouped)))?;
                Ok(store(Handle::Query(jet_data_query_map(&query, |row| {
                    result_tree(vec![("key", text_tree(row.key)), ("value", jet_std::DataTree::Int(row.value))])
                }))))
            }
            "group_sum" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = with_grouped(index, &operation, |grouped| {
                    Ok(jet_data_group_sum_query_checked(grouped, move |row| {
                        callback_number(callback, row, "query.group_by.sum")
                    }))
                })?;
                Ok(store(Handle::Query(jet_data_query_map(&query, |row| {
                    result_tree(vec![("key", text_tree(row.key)), ("value", jet_std::DataTree::Float(row.value))])
                }))))
            }
            "group_mean" => {
                let index = handle(value(0)?, &operation)?;
                let callback = u32::try_from(integer(value(1)?, &operation)?).map_err(|_| error(jet_std::DataErrorKind::InvalidArgument, &operation, "invalid callback"))?;
                let query = with_grouped(index, &operation, |grouped| {
                    Ok(jet_data_group_mean_query_checked(grouped, move |row| {
                        callback_number(callback, row, "query.group_by.mean")
                    }))
                })?;
                Ok(store(Handle::Query(jet_data_query_map(&query, |row| {
                    result_tree(vec![("key", text_tree(row.key)), ("value", jet_std::DataTree::Float(row.value))])
                }))))
            }
            "count" => match value(0)? {
                jet_std::DataTree::Array(values) => Ok(jet_std::DataTree::Int(values.len() as i64)),
                value => {
                    let query = clone_query(handle(value, &operation)?)?;
                    Ok(jet_std::DataTree::Int(jet_data_query_collect(&query)?.len() as i64))
                }
            },
            "snapshot_reusable" => {
                let left = value(0)?;
                let right = value(1)?;
                Ok(jet_std::DataTree::Bool(jet_data_loader_canonical_tree(left) == jet_data_loader_canonical_tree(right)))
            }
            "require_bridge" => {
                let provider = text(value(0)?, &operation)?;
                jet_data_require_bridge(&provider)?;
                Ok(jet_std::DataTree::Null)
            }
            "status" => Ok(jet_std::DataTree::Array(jet_data_status().into_iter().map(|value| data_status(&value)).collect())),
            "release" => {
                let index = handle(value(0)?, &operation)?;
                HANDLES.with(|cell| if let Some(slot) = cell.borrow_mut().get_mut(index) { *slot = None; });
                Ok(jet_std::DataTree::Null)
            }
            _ => Err(error(jet_std::DataErrorKind::Bridge, &operation, "operation is not available on the Web DataFlow bridge")),
        }
    }

    fn wire_error(error: &jet_std::DataError) -> String {
        format!("{{\"error\":{{\"kind\":{},\"operation\":{},\"reason\":{}}}}}", quote(&error.kind.to_string()), quote(&error.operation), quote(&error.reason))
    }
    fn set_output(value: String) {
        OUTPUT.with(|cell| *cell.borrow_mut() = value.into_bytes());
    }
    #[no_mangle]
    pub extern "C" fn jet_data_web_input_alloc(slot: u32, length: u32) -> u32 {
        let Ok(slot) = usize::try_from(slot) else { return 0 };
        if slot >= 4 { return 0 }
        INPUTS.with(|cell| {
            let mut inputs = cell.borrow_mut();
            let input = &mut inputs[slot];
            input.resize(length as usize, 0);
            input.as_mut_ptr() as usize as u32
        })
    }
    #[no_mangle]
    pub extern "C" fn jet_data_web_input_free(slot: u32, _pointer: u32) {
        if let Ok(slot) = usize::try_from(slot) {
            INPUTS.with(|cell| if let Some(input) = cell.borrow_mut().get_mut(slot) { input.clear(); });
        }
    }
    #[no_mangle]
    pub extern "C" fn jet_data_web_output_ptr() -> u32 {
        OUTPUT.with(|cell| cell.borrow().as_ptr() as usize as u32)
    }
    #[no_mangle]
    pub extern "C" fn jet_data_web_output_len() -> u32 {
        OUTPUT.with(|cell| u32::try_from(cell.borrow().len()).unwrap_or(u32::MAX))
    }
    #[no_mangle]
    pub extern "C" fn jet_data_web_output_clear() {
        OUTPUT.with(|cell| cell.borrow_mut().clear());
    }
    #[no_mangle]
    pub extern "C" fn jet_data_web_call(pointer: u32, length: u32) -> i32 {
        let input = unsafe { std::slice::from_raw_parts(pointer as *const u8, length as usize) };
        let parsed = std::str::from_utf8(input).ok().and_then(|value| jet_std::parse_json_typed_datatree(value).ok());
        let result = parsed.map_or_else(
            || Err(error(jet_std::DataErrorKind::Decode, "data.web", "invalid Web DataFlow wire JSON")),
            |value| dispatch(&value),
        );
        match result {
            Ok(value) => { set_output(jet_data_loader_canonical_tree(&value)); 0 }
            Err(error) => { set_output(wire_error(&error)); 1 }
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub fn jet_data_web_register_type<T>(key: &'static str)
where
    T: __jet_Decode + __jet_Encode + Clone + 'static,
{
    jet_data_web::register_type::<T>(key);
}

/// D-SQL-SURFACE1=C / D-QUERY-RETAIN1=A: `[T].query(SQL)`. Internal symbol
/// behind the one public `query` spelling; the checked SQL selects source
/// rows eagerly and the result is the same reusable deferred carrier the
/// list door returns.
fn jet_data_query_sql<T: __jet_Encode + Clone + 'static>(
    rows: &Vec<T>,
    sql: &String,
) -> Result<jet_std::DataQuery<T>, Vec<jet_std::FieldError>> {
    let selected = jet_data_query_rows(rows, sql)?;
    Ok(jet_data_query(&selected).with_step("sql"))
}

fn jet_enc_csv_query<T: __jet_Decode + __jet_Encode + Clone + 'static>(
    path: &String,
    sql: &String,
) -> Result<jet_std::DataQuery<T>, Vec<jet_std::FieldError>> {
    let text = jet_data_query_read(path)?;
    let csv_rows = jet_ring_csv_parse(&text, &",".to_string(), false, false)
        .map_err(jet_std::FieldError::one)?;
    let header = csv_rows.first().cloned().unwrap_or_default();
    jet_data_query_validate_fields(&header, sql)?;
    let rows = jet_enc_csv_decode::<T>(&text)?;
    jet_data_query_sql(&rows, sql)
}
