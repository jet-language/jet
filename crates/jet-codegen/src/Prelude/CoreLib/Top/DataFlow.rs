// D-DATAFLOW1=A: typed pull streams, DataLimits, DataError analytics (edition 2027).

fn jet_data_error(
    kind: jet_std::DataErrorKind,
    operation: &str,
    reason: impl Into<String>,
) -> jet_std::DataError {
    jet_std::DataError {
        kind,
        operation: operation.to_string(),
        row: Err(JetAbsent),
        column: Err(JetAbsent),
        index: Err(JetAbsent),
        reason: reason.into(),
        cause: Err(JetAbsent),
    }
}

fn jet_data_error_at(
    kind: jet_std::DataErrorKind,
    operation: &str,
    index: JetOutcome<i64, JetAbsent>,
    reason: impl Into<String>,
) -> jet_std::DataError {
    let mut error = jet_data_error(kind, operation, reason);
    error.index = index;
    error
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
    for (name, value) in [
        ("max_groups", limits.max_groups),
        ("max_sort_rows", limits.max_sort_rows),
        ("max_join_rows", limits.max_join_rows),
        ("max_output_rows", limits.max_output_rows),
    ] {
        if value < 1 {
            return Err(jet_data_error(
                jet_std::DataErrorKind::InvalidArgument,
                "DataLimits",
                format!("{name} must be positive"),
            ));
        }
    }
    Ok(())
}

fn jet_data_normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn jet_data_reject_nonfinite(operation: &str, values: &[f64]) -> Result<(), jet_std::DataError> {
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                operation,
                Ok(index as i64),
                "numeric input must be finite",
            ));
        }
    }
    Ok(())
}

fn jet_data_neumaier_sum(values: &[f64]) -> Result<f64, jet_std::DataError> {
    jet_data_reject_nonfinite("sum", values)?;
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for value in values.iter().copied() {
        let t = sum + value;
        if sum.abs() >= value.abs() {
            compensation += (sum - t) + value;
        } else {
            compensation += (value - t) + sum;
        }
        sum = t;
        if !sum.is_finite() || !compensation.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "sum",
                "finite overflow while summing",
            ));
        }
    }
    let out = sum + compensation;
    if !out.is_finite() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Overflow,
            "sum",
            "finite overflow while summing",
        ));
    }
    Ok(jet_data_normalize_zero(out))
}

fn jet_data_sum_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Ok(0.0);
    }
    jet_data_neumaier_sum(values)
}

fn jet_data_mean_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "mean",
            "mean of empty data is undefined",
        ));
    }
    let sum = jet_data_neumaier_sum(values)?;
    Ok(jet_data_normalize_zero(sum / values.len() as f64))
}

fn jet_data_min_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "min",
            "min of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("min", values)?;
    Ok(jet_data_normalize_zero(
        values.iter().copied().fold(f64::INFINITY, f64::min),
    ))
}

fn jet_data_max_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "max",
            "max of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("max", values)?;
    Ok(jet_data_normalize_zero(
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ))
}

fn jet_data_quantile_checked(values: &Vec<f64>, q: f64) -> Result<f64, jet_std::DataError> {
    if !q.is_finite() || !(0.0..=1.0).contains(&q) {
        return Err(jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            "quantile",
            "quantile q must be a finite value in 0.0 through 1.0",
        ));
    }
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "quantile",
            "quantile of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("quantile", values)?;
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let value = if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    };
    Ok(jet_data_normalize_zero(value))
}

fn jet_data_median_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    jet_data_quantile_checked(values, 0.5).map_err(|mut error| {
        error.operation = "median".to_string();
        error
    })
}

fn jet_data_variance_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "variance",
            "variance of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("variance", values)?;
    // Deterministic Welford population variance in input order.
    let mut count = 0.0f64;
    let mut mean = 0.0f64;
    let mut m2 = 0.0f64;
    for value in values.iter().copied() {
        count += 1.0;
        let delta = value - mean;
        mean += delta / count;
        let delta2 = value - mean;
        m2 += delta * delta2;
        if !mean.is_finite() || !m2.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "variance",
                "finite overflow while computing variance",
            ));
        }
    }
    Ok(jet_data_normalize_zero(m2 / count))
}

fn jet_data_stddev_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    Ok(jet_data_normalize_zero(jet_data_variance_checked(values)?.sqrt()))
}

fn jet_data_describe_checked(values: &Vec<f64>) -> Result<jet_std::DataSummary, jet_std::DataError> {
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "describe",
            "describe of empty data is undefined",
        ));
    }
    Ok(jet_std::DataSummary {
        count: values.len() as i64,
        sum: jet_data_sum_checked(values)?,
        mean: jet_data_mean_checked(values)?,
        min: jet_data_min_checked(values)?,
        max: jet_data_max_checked(values)?,
        median: jet_data_median_checked(values)?,
        variance: jet_data_variance_checked(values)?,
        stddev: jet_data_stddev_checked(values)?,
    })
}

fn jet_data_rolling_mean_checked(
    values: &Vec<f64>,
    width: i64,
) -> Result<Vec<f64>, jet_std::DataError> {
    if width < 1 {
        return Err(jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            "rolling_mean",
            "rolling width must be positive",
        ));
    }
    jet_data_reject_nonfinite("rolling_mean", values)?;
    let width = width as usize;
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_add(1).saturating_sub(width);
        let window = &values[start..=i];
        let mean = jet_data_neumaier_sum(window)? / window.len() as f64;
        out.push(jet_data_normalize_zero(mean));
    }
    Ok(out)
}

fn jet_data_bar_text_checked(
    groups: &Vec<jet_std::DataGroup>,
) -> Result<String, jet_std::DataError> {
    for (index, group) in groups.iter().enumerate() {
        if group.count < 0 {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::InvalidArgument,
                "bar_text",
                Ok(index as i64),
                "plot counts must be non-negative",
            ));
        }
        if !group.sum.is_finite() || !group.mean.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                "bar_text",
                Ok(index as i64),
                "plot values must be finite",
            ));
        }
    }
    Ok(jet_data_bar_text(groups))
}

fn jet_data_bar_svg_checked(groups: &Vec<jet_std::DataGroup>) -> Result<String, jet_std::DataError> {
    for (index, group) in groups.iter().enumerate() {
        if group.count < 0 {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::InvalidArgument,
                "bar_svg",
                Ok(index as i64),
                "plot counts must be non-negative",
            ));
        }
        if !group.sum.is_finite() || !group.mean.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                "bar_svg",
                Ok(index as i64),
                "plot values must be finite",
            ));
        }
    }
    Ok(jet_data_bar_svg(groups))
}

fn jet_data_plot_error(error: DataPlotError) -> jet_std::DataError {
    let kind = match error.kind {
        "NonFinite" => jet_std::DataErrorKind::NonFinite,
        _ => jet_std::DataErrorKind::InvalidArgument,
    };
    jet_data_error_at(kind, error.operation, jet_outcome_of(error.index), error.reason)
}

fn jet_data_line_text_checked(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
) -> Result<String, jet_std::DataError> {
    jet_data_line_text_plot_checked(groups, options).map_err(jet_data_plot_error)
}

fn jet_data_line_svg_checked(
    groups: &Vec<jet_std::DataGroup>,
    options: &jet_std::DataLineOptions,
) -> Result<String, jet_std::DataError> {
    jet_data_line_svg_plot_checked(groups, options).map_err(jet_data_plot_error)
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
    let mut groups = std::collections::BTreeMap::<(String, String), (i64, f64)>::new();
    for row in rows.iter().cloned() {
        let rk = row_key(row.clone());
        let ck = col_key(row.clone());
        let v = value(row);
        if !v.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::NonFinite,
                "pivot_sum",
                "pivot values must be finite",
            ));
        }
        if !groups.contains_key(&(rk.clone(), ck.clone()))
            && groups.len() as i64 >= limits.max_groups
        {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                "pivot_sum",
                format!("max_groups {} exceeded", limits.max_groups),
            ));
        }
        let entry = groups.entry((rk, ck)).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += v;
        if !entry.1.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "pivot_sum",
                "finite overflow while pivoting",
            ));
        }
    }
    if groups.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "pivot_sum",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(groups
        .into_iter()
        .map(|((row_key, column_key), (count, sum))| jet_std::DataPivotCell {
            row_key,
            column_key,
            count,
            sum: jet_data_normalize_zero(sum),
            mean: jet_data_normalize_zero(if count == 0 {
                0.0
            } else {
                sum / count as f64
            }),
        })
        .collect())
}

fn jet_data_group_count_checked<T, F>(
    rows: &Vec<T>,
    key: F,
    limits: &jet_std::DataLimits,
) -> Result<Vec<jet_std::DataGroup>, jet_std::DataError>
where
    T: Clone,
    F: Fn(T) -> String,
{
    jet_data_limits_validate(limits)?;
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    for row in rows.iter().cloned() {
        let k = key(row);
        if !groups.contains_key(&k) && groups.len() as i64 >= limits.max_groups {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                "group_count",
                format!("max_groups {} exceeded", limits.max_groups),
            ));
        }
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
    }
    if groups.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "group_count",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum,
            mean: 0.0,
        })
        .collect())
}

fn jet_data_group_sum_checked<T, FK, FV>(
    rows: &Vec<T>,
    key: FK,
    value: FV,
    limits: &jet_std::DataLimits,
) -> Result<Vec<jet_std::DataGroup>, jet_std::DataError>
where
    T: Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_limits_validate(limits)?;
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    for row in rows.iter().cloned() {
        let k = key(row.clone());
        let v = value(row);
        if !v.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::NonFinite,
                "group_sum",
                "group values must be finite",
            ));
        }
        if !groups.contains_key(&k) && groups.len() as i64 >= limits.max_groups {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                "group_sum",
                format!("max_groups {} exceeded", limits.max_groups),
            ));
        }
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += v;
        if !entry.1.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "group_sum",
                "finite overflow while grouping",
            ));
        }
    }
    if groups.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "group_sum",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum: jet_data_normalize_zero(sum),
            mean: jet_data_normalize_zero(if count == 0 {
                0.0
            } else {
                sum / count as f64
            }),
        })
        .collect())
}

fn jet_data_group_mean_checked<T, FK, FV>(
    rows: &Vec<T>,
    key: FK,
    value: FV,
    limits: &jet_std::DataLimits,
) -> Result<Vec<jet_std::DataGroup>, jet_std::DataError>
where
    T: Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_group_sum_checked(rows, key, value, limits).map_err(|mut error| {
        if error.operation == "group_sum" {
            error.operation = "group_mean".to_string();
        }
        error
    })
}

fn jet_data_sort_by_checked<T, F>(
    rows: &Vec<T>,
    key: F,
    limits: &jet_std::DataLimits,
) -> Result<Vec<T>, jet_std::DataError>
where
    T: Clone,
    F: Fn(T) -> String,
{
    jet_data_limits_validate(limits)?;
    if rows.len() as i64 > limits.max_sort_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "sort_by",
            format!("max_sort_rows {} exceeded", limits.max_sort_rows),
        ));
    }
    if rows.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "sort_by",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    let mut out = rows.clone();
    out.sort_by_key(|row| key(row.clone()));
    Ok(out)
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
    let mut right_rows = std::collections::BTreeMap::<String, Vec<U>>::new();
    for row in right.iter().cloned() {
        right_rows.entry(right_key(row.clone())).or_default().push(row);
    }
    let mut joined = Vec::new();
    for left_row in left.iter().cloned() {
        if let Some(matches) = right_rows.get(&left_key(left_row.clone())) {
            for right_row in matches {
                if joined.len() as i64 >= limits.max_join_rows {
                    return Err(jet_data_error(
                        jet_std::DataErrorKind::Limit,
                        "inner_join",
                        format!("max_join_rows {} exceeded", limits.max_join_rows),
                    ));
                }
                joined.push(jet_std::DataJoin {
                    left: left_row.clone(),
                    right: right_row.clone(),
                });
            }
        }
    }
    if joined.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "inner_join",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(joined)
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
    let mut right_rows = std::collections::BTreeMap::<String, Vec<U>>::new();
    for row in right.iter().cloned() {
        right_rows.entry(right_key(row.clone())).or_default().push(row);
    }
    let mut joined = Vec::new();
    for left_row in left.iter().cloned() {
        match right_rows.get(&left_key(left_row.clone())) {
            Some(matches) => {
                for right_row in matches {
                    if joined.len() as i64 >= limits.max_join_rows {
                        return Err(jet_data_error(
                            jet_std::DataErrorKind::Limit,
                            "left_join",
                            format!("max_join_rows {} exceeded", limits.max_join_rows),
                        ));
                    }
                    joined.push(jet_std::DataJoin {
                        left: left_row.clone(),
                        right: Ok(right_row.clone()),
                    });
                }
            }
            None => {
                if joined.len() as i64 >= limits.max_join_rows {
                    return Err(jet_data_error(
                        jet_std::DataErrorKind::Limit,
                        "left_join",
                        format!("max_join_rows {} exceeded", limits.max_join_rows),
                    ));
                }
                joined.push(jet_std::DataJoin {
                    left: left_row,
                    right: Err(JetAbsent),
                });
            }
        }
    }
    if joined.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "left_join",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(joined)
}

fn jet_data_collect_checked<T: Clone>(
    plan: &jet_std::DataLazyFrame<T>,
    limits: &jet_std::DataLimits,
) -> Result<jet_std::DataTable<T>, jet_std::DataError> {
    jet_data_limits_validate(limits)?;
    let rows = jet_data_materialize(plan);
    if rows.len() as i64 > limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "collect",
            format!("max_output_rows {} exceeded", limits.max_output_rows),
        ));
    }
    Ok(jet_std::DataTable {
        rows,
        missing: plan.missing,
        plan: plan.plan.clone(),
    })
}

fn jet_data_csv_reader(
    input: JetFileReader,
    limits: jet_std::DataLimits,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    jet_data_limits_validate(&limits)?;
    let reader = jet_enc_csv_reader(input, limits.encoding.clone())
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
    })
}

fn jet_data_json_reader(
    input: JetFileReader,
    limits: jet_std::DataLimits,
) -> Result<jet_std::DataStream, jet_std::DataError> {
    jet_data_limits_validate(&limits)?;
    let reader = jet_enc_json_reader(input, limits.encoding.clone())
        .map_err(|enc| jet_data_from_encoding("json_reader", Err(JetAbsent), enc))?;
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

fn jet_data_stream_decode_csv_row<T: user_Decode>(
    headers: &[String],
    row: Vec<String>,
    row_index: i64,
) -> Result<T, jet_std::DataError> {
    let obj: Vec<(String, jet_std::DataTree)> = headers
        .iter()
        .enumerate()
        .map(|(c, name)| {
            let cell = row.get(c).cloned().unwrap_or_default();
            (name.clone(), jet_std::DataTree::Text(cell))
        })
        .collect();
    T::jet_decode_traced(&jet_std::DataTree::Object(obj))
        .map(|(v, _)| v)
        .map_err(|errors| {
            let mut error = jet_data_error(
                jet_std::DataErrorKind::Decode,
                "csv_reader",
                jet_data_field_errors_reason(errors),
            );
            error.row = Ok(row_index);
            error
        })
}

fn jet_data_json_fold_from_event(
    reader: &mut jet_std::JSONReader,
    first: jet_std::DataEvent,
) -> Result<jet_std::DataTree, jet_std::EncodingError> {
    // Fold one complete JSON value starting from an already-consumed event.
    enum Frame {
        Array(Vec<jet_std::DataTree>),
        Object {
            entries: Vec<(String, jet_std::DataTree)>,
            key: Option<String>,
        },
    }
    let mut root: Option<jet_std::DataTree> = None;
    let mut frames: Vec<Frame> = Vec::new();
    let mut pending = Some(first);
    let push = |frames: &mut Vec<Frame>,
                root: &mut Option<jet_std::DataTree>,
                value: jet_std::DataTree|
     -> Result<(), jet_std::EncodingError> {
        match frames.last_mut() {
            Some(Frame::Array(items)) => items.push(value),
            Some(Frame::Object { entries, key }) => {
                let Some(key) = key.take() else {
                    return Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        0,
                        1,
                        1,
                        "JSON object value without a key",
                    ));
                };
                entries.push((key, value));
            }
            None if root.is_none() => *root = Some(value),
            None => {
                return Err(jet_encoding_error(
                    jet_std::EncodingErrorKind::State,
                    0,
                    1,
                    1,
                    "JSON produced two roots for one value",
                ));
            }
        }
        Ok(())
    };
    loop {
        let event = if let Some(event) = pending.take() {
            event
        } else {
            match reader.next_event()? {
                Some(event) => event,
                None => {
                    return Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::Truncated,
                        reader.offset,
                        reader.line,
                        reader.column,
                        "JSON value ended before it was closed",
                    ));
                }
            }
        };
        match event {
            jet_std::DataEvent::Null => push(&mut frames, &mut root, jet_std::DataTree::Null)?,
            jet_std::DataEvent::Bool(v) => {
                push(&mut frames, &mut root, jet_std::DataTree::Bool(v))?
            }
            jet_std::DataEvent::Int(v) => push(&mut frames, &mut root, jet_std::DataTree::Int(v))?,
            jet_std::DataEvent::Float(v) => {
                push(&mut frames, &mut root, jet_std::DataTree::Float(v))?
            }
            jet_std::DataEvent::Text(v) => {
                push(&mut frames, &mut root, jet_std::DataTree::Text(v))?
            }
            jet_std::DataEvent::Bytes(v) => {
                push(&mut frames, &mut root, jet_std::DataTree::Bytes(v))?
            }
            jet_std::DataEvent::ArrayStart => frames.push(Frame::Array(Vec::new())),
            jet_std::DataEvent::ObjectStart => frames.push(Frame::Object {
                entries: Vec::new(),
                key: None,
            }),
            jet_std::DataEvent::Key(key) => match frames.last_mut() {
                Some(Frame::Object { key: slot, .. }) => *slot = Some(key),
                _ => {
                    return Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        reader.offset,
                        reader.line,
                        reader.column,
                        "JSON key outside an object",
                    ));
                }
            },
            jet_std::DataEvent::ArrayEnd => {
                let Some(Frame::Array(items)) = frames.pop() else {
                    return Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        reader.offset,
                        reader.line,
                        reader.column,
                        "JSON array end without start",
                    ));
                };
                push(&mut frames, &mut root, jet_std::DataTree::Array(items))?;
            }
            jet_std::DataEvent::ObjectEnd => {
                let Some(Frame::Object { entries, .. }) = frames.pop() else {
                    return Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        reader.offset,
                        reader.line,
                        reader.column,
                        "JSON object end without start",
                    ));
                };
                push(&mut frames, &mut root, jet_std::DataTree::Object(entries))?;
            }
        }
        if root.is_some() && frames.is_empty() {
            return Ok(root.unwrap());
        }
    }
}

// D-FAIL-CARRIER1=A: the row is `T? ? DataError` — end of stream is a clean
// absence, a broken row is a report.
fn jet_data_stream_next<T: user_Decode>(
    stream: &mut jet_std::DataStream,
) -> Result<JetOutcome<T, JetAbsent>, jet_std::DataError> {
    jet_data_stream_scan(stream).map(jet_outcome_of)
}

fn jet_data_stream_scan<T: user_Decode>(
    stream: &mut jet_std::DataStream,
) -> Result<Option<T>, jet_std::DataError> {
    if let Some(error) = &stream.terminal {
        return Err(error.clone());
    }
    if stream.eof {
        return Ok(None);
    }
    match &mut stream.inner {
        jet_std::DataStreamInner::CSV { reader, headers } => {
            if headers.is_none() {
                match reader.next_record() {
                    Ok(Some(header)) => *headers = Some(header),
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
                    match jet_data_stream_decode_csv_row::<T>(&header, row, stream.row_index) {
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
            match jet_data_json_fold_from_event(reader, first) {
                Ok(tree) => {
                    stream.row_index += 1;
                    match T::jet_decode_traced(&tree) {
                        Ok((value, _)) => Ok(Some(value)),
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
    }
}

fn jet_data_stream_collect<T: user_Decode + Clone>(
    stream: &mut jet_std::DataStream,
) -> Result<Vec<T>, jet_std::DataError> {
    jet_data_limits_validate(&stream.limits)?;
    let mut out = Vec::new();
    loop {
        match jet_data_stream_next::<T>(stream)? {
            Err(JetAbsent) => return Ok(out),
            Ok(row) => {
                if out.len() as i64 >= stream.limits.max_output_rows {
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
                out.push(row);
            }
        }
    }
}

fn jet_data_group_mean_stream<T, FK, FV>(
    stream: &mut jet_std::DataStream,
    key: FK,
    value: FV,
) -> Result<Vec<jet_std::DataGroup>, jet_std::DataError>
where
    T: user_Decode + Clone,
    FK: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    jet_data_limits_validate(&stream.limits)?;
    let mut groups: std::collections::BTreeMap<String, (i64, f64)> =
        std::collections::BTreeMap::new();
    loop {
        let Ok(row) = jet_data_stream_next::<T>(stream)? else {
            break;
        };
        let k = key(row.clone());
        let v = value(row);
        if !v.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::NonFinite,
                "group_mean",
                "group values must be finite",
            ));
        }
        if !groups.contains_key(&k) && groups.len() as i64 >= stream.limits.max_groups {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Limit,
                "group_mean",
                format!("max_groups {} exceeded", stream.limits.max_groups),
            ));
        }
        let entry = groups.entry(k).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += v;
        if !entry.1.is_finite() {
            return Err(jet_data_error(
                jet_std::DataErrorKind::Overflow,
                "group_mean",
                "finite overflow while grouping",
            ));
        }
    }
    if groups.len() as i64 > stream.limits.max_output_rows {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Limit,
            "group_mean",
            format!("max_output_rows {} exceeded", stream.limits.max_output_rows),
        ));
    }
    Ok(groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum: jet_data_normalize_zero(sum),
            mean: jet_data_normalize_zero(if count == 0 {
                0.0
            } else {
                sum / count as f64
            }),
        })
        .collect())
}
