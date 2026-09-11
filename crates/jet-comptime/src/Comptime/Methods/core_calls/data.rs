use super::*;

/// `[Float]` argument — `core.data`'s stats functions all take `&Vec<f64>`.
pub(super) fn as_float_list(v: &CtValue, span: Span) -> Result<Vec<f64>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs.iter().map(|x| as_float(x, span)).collect(),
        _ => Err(unsupported("core.data: argument must be `[Float]`", span)),
    }
}

/// `[Group<K, Int>]` argument for the bar renderers.
pub(super) fn as_data_bar_groups(
    v: &CtValue,
    span: Span,
) -> Result<Vec<data_kernel::jet_std::GroupValue<CtValue, i64>>, Diagnostic> {
    let CtValue::List(items) = v else {
        return Err(unsupported(
            "core.data: argument must be `[Group<K, Int>]`",
            span,
        ));
    };
    items
        .iter()
        .map(|item| {
            let CtValue::Struct { type_name, fields } = item else {
                return Err(unsupported(
                    "core.data: argument must be `[Group<K, Int>]`",
                    span,
                ));
            };
            if type_name != "Group" {
                return Err(unsupported(
                    "core.data: argument must be `[Group<K, Int>]`",
                    span,
                ));
            }
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let key = field("key").cloned().ok_or_else(|| {
                unsupported(
                    "core.data: a `Group` needs `key` and `value: Int`",
                    span,
                )
            })?;
            let value = match field("value") {
                Some(CtValue::Int(value)) => *value,
                _ => {
                    return Err(unsupported(
                        "core.data: a `Group` needs `key` and `value: Int`",
                        span,
                    ))
                }
            };
            Ok(data_kernel::jet_std::GroupValue { key, value })
        })
        .collect()
}

/// `[Group<K, Float>]` argument for the line renderers.
pub(super) fn as_data_line_groups(
    v: &CtValue,
    span: Span,
) -> Result<Vec<data_kernel::jet_std::GroupValue<CtValue, f64>>, Diagnostic> {
    let CtValue::List(items) = v else {
        return Err(unsupported(
            "core.data: argument must be `[Group<K, Float>]`",
            span,
        ));
    };
    items
        .iter()
        .map(|item| {
            let CtValue::Struct { type_name, fields } = item else {
                return Err(unsupported(
                    "core.data: argument must be `[Group<K, Float>]`",
                    span,
                ));
            };
            if type_name != "Group" {
                return Err(unsupported(
                    "core.data: argument must be `[Group<K, Float>]`",
                    span,
                ));
            }
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let key = field("key").cloned().ok_or_else(|| {
                unsupported(
                    "core.data: a `Group` needs `key` and `value: Float`",
                    span,
                )
            })?;
            let value = field("value")
                .ok_or_else(|| {
                    unsupported(
                        "core.data: a `Group` needs `key` and `value: Float`",
                        span,
                    )
                })
                .and_then(|value| as_float(value, span))?;
            Ok(data_kernel::jet_std::GroupValue { key, value })
        })
        .collect()
}

pub(super) fn as_data_line_options(
    v: &CtValue,
    span: Span,
) -> Result<data_kernel::jet_std::DataLineOptions, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported(
            "core.data line renderers need `DataLineOptions`",
            span,
        ));
    };
    if type_name != "DataLineOptions" {
        return Err(unsupported(
            "core.data line renderers need `DataLineOptions`",
            span,
        ));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| unsupported("DataLineOptions is missing a required field", span))
    };
    let string = |name: &str| match field(name)? {
        CtValue::Str(value) => Ok(value),
        _ => Err(unsupported(
            "DataLineOptions string field has the wrong type",
            span,
        )),
    };
    let markers = match field("markers")? {
        CtValue::Bool(value) => value,
        _ => return Err(unsupported("DataLineOptions `markers` must be Bool", span)),
    };
    let reference = match field("reference")? {
        CtValue::Present(value) => Ok(as_float(&value, span)?),
        CtValue::Failed(CtReport::Clean(_)) => Err(jet_foundation::Outcome::JetAbsent),
        _ => {
            return Err(unsupported(
                "DataLineOptions `reference` must be Float?",
                span,
            ))
        }
    };
    Ok(data_kernel::jet_std::DataLineOptions {
        title: string("title")?,
        x_label: string("x_label")?,
        y_label: string("y_label")?,
        markers,
        reference,
        style: string("style")?,
        color: string("color")?,
        legend: string("legend")?,
    })
}

/// #1657 / I9: the checked `core.data` surface is the edition-2027 default and
/// older editions type the same calls as plain values. Sema picks the return
/// type from this same question (`fixed_sigs.rs`), so comptime asks it too.
fn data_checked_surface() -> bool {
    jet_foundation::PackageEdition::package_edition_at_least("2027")
}

/// One `DataError` value for every `core.data` failure, built from the kernel's
/// own error — comptime never writes its own reason text.
pub(super) fn data_error_value(error: &data_kernel::jet_std::DataError) -> CtValue {
    let index = |slot: &jet_foundation::Outcome::JetOutcome<
        i64,
        jet_foundation::Outcome::JetAbsent,
    >| match slot {
        Ok(value) => CtValue::Present(Box::new(CtValue::Int(*value))),
        Err(_) => CtValue::absent(Type::Int),
    };
    CtValue::Struct {
        type_name: "DataError".to_string(),
        fields: vec![
            (
                "kind".to_string(),
                CtValue::Enum {
                    type_name: "DataErrorKind".to_string(),
                    variant: format!("{:?}", error.kind),
                    args: Vec::new(),
                },
            ),
            (
                "operation".to_string(),
                CtValue::Str(error.operation.clone()),
            ),
            ("row".to_string(), index(&error.row)),
            ("column".to_string(), index(&error.column)),
            ("index".to_string(), index(&error.index)),
            ("reason".to_string(), CtValue::Str(error.reason.clone())),
            (
                "cause".to_string(),
                CtValue::absent(Type::Named("EncodingError".to_string())),
            ),
        ],
    }
}

/// Marshal one kernel result onto the surface the current edition types.
pub(super) fn data_result_value<T>(
    checked: Result<T, data_kernel::jet_std::DataError>,
    unchecked: impl FnOnce() -> T,
    to_value: impl Fn(T) -> CtValue,
) -> CtValue {
    data_result_value_for_type(checked, unchecked, to_value, None)
}

/// Marshal one kernel result while honoring a checked return type supplied by
/// the MIR route.  MIR carries the declaration's result shape even when the
/// ambient package-edition thread is not set to the checked default.
pub(super) fn data_result_value_for_type<T>(
    checked: Result<T, data_kernel::jet_std::DataError>,
    unchecked: impl FnOnce() -> T,
    to_value: impl Fn(T) -> CtValue,
    resolved_ret: Option<&Type>,
) -> CtValue {
    let checked_surface =
        matches!(resolved_ret, Some(Type::Result { .. })) || data_checked_surface();
    if !checked_surface {
        return to_value(unchecked());
    }
    match checked {
        Ok(value) => CtValue::Present(Box::new(to_value(value))),
        Err(error) => CtValue::failed(Box::new(data_error_value(&error))),
    }
}

pub(super) fn data_summary_value(summary: data_kernel::jet_std::DataSummary) -> CtValue {
    CtValue::Struct {
        type_name: "DataSummary".to_string(),
        fields: vec![
            ("count".to_string(), CtValue::Int(summary.count)),
            ("sum".to_string(), data_float_value(summary.sum)),
            ("mean".to_string(), data_float_value(summary.mean)),
            ("min".to_string(), data_float_value(summary.min)),
            ("max".to_string(), data_float_value(summary.max)),
            ("median".to_string(), data_float_value(summary.median)),
            ("variance".to_string(), data_float_value(summary.variance)),
            ("stddev".to_string(), data_float_value(summary.stddev)),
        ],
    }
}

/// Evaluate `core.data.describe` at the stateful MIR boundary without losing
/// the fallible carrier declared by sema.
pub(crate) fn eval_data_describe(
    args: &[CtValue],
    resolved_ret: Option<&Type>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let values = as_float_list(
        args.first()
            .ok_or_else(|| unsupported("core.data.describe(): missing arg 0", span))?,
        span,
    )?;
    Ok(data_result_value_for_type(
        data_kernel::jet_data_describe_checked(&values),
        || data_kernel::jet_data_describe(&values),
        data_summary_value,
        resolved_ret,
    ))
}

/// Evaluate `core.data.pivot_sum` through the shared checked aggregation
/// kernel.  Both the stateful MIR route and the AST route call this adapter.
pub(super) fn data_pivot_sum<F>(
    args: &[CtValue],
    span: Span,
    mut call: F,
) -> Result<CtValue, Diagnostic>
where
    F: FnMut(&CtValue, Vec<CtValue>, Span) -> Result<CtValue, Diagnostic>,
{
    let [rows, row_key, column_key, value] = args else {
        return Err(unsupported(
            "`data.pivot_sum()` expects rows and three callbacks",
            span,
        ));
    };
    let CtValue::List(rows) = rows else {
        return Err(unsupported(
            "`data.pivot_sum()` needs a row list",
            span,
        ));
    };
    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        let row_key_value = call(row_key, vec![row.clone()], span)?;
        let row_key = as_string(&row_key_value, span)?.to_owned();
        let column_key_value = call(column_key, vec![row.clone()], span)?;
        let column_key = as_string(&column_key_value, span)?.to_owned();
        let amount_value = call(value, vec![row.clone()], span)?;
        let amount = as_float(&amount_value, span)?;
        values.push((row_key, column_key, amount));
    }

    let limits = data_kernel::jet_std::DataLimits::safe();
    let checked = data_kernel::jet_data_pivot_sum_values(
        &values,
        &data_kernel::jet_data_kernel_limits(
            limits.max_groups,
            limits.max_sort_rows,
            limits.max_join_rows,
            limits.max_output_rows,
        ),
    );
    let to_value = |cells: Vec<data_kernel::JetDataPivotCell>| {
        CtValue::List(
            cells
                .into_iter()
                .map(|cell| CtValue::Struct {
                    type_name: "DataPivotCell".to_string(),
                    fields: vec![
                        ("row_key".to_string(), CtValue::Str(cell.row_key)),
                        (
                            "column_key".to_string(),
                            CtValue::Str(cell.column_key),
                        ),
                        ("count".to_string(), CtValue::Int(cell.count)),
                        ("sum".to_string(), data_float_value(cell.sum)),
                        ("mean".to_string(), data_float_value(cell.mean)),
                    ],
                })
                .collect(),
        )
    };
    Ok(match checked {
        Ok(cells) => CtValue::Present(Box::new(to_value(cells))),
        Err(error) => CtValue::failed(Box::new(data_error_value(&error))),
    })
}


pub(super) fn data_float_value(value: f64) -> CtValue {
    CtValue::Float(CtFloat::f64(value))
}

/// D-DATA-STATUS1 / #708: the `data.status()` rows for `jet inspect dossier`,
/// read from the one kernel rather than a second table.
pub fn data_status_rows() -> Vec<(String, String, String, String, String, String, String)> {
    data_kernel::jet_data_status()
        .into_iter()
        .map(|row| {
            (
                row.step,
                row.path,
                row.copy,
                row.ownership,
                row.trust,
                row.fallback,
                row.replacement,
            )
        })
        .collect()
}

pub fn apply_data_line_call(
    method: &str,
    args: Vec<CtValue>,
    resolved_ret: Option<&Type>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let groups = as_data_line_groups(
        args.first()
            .ok_or_else(|| unsupported("core.data line renderers need groups", span))?,
        span,
    )?;
    let options = as_data_line_options(
        args.get(1)
            .ok_or_else(|| unsupported("core.data line renderers need options", span))?,
        span,
    )?;
    let plot_error = |error: data_plot_rt::DataPlotError| data_kernel::jet_std::DataError {
        kind: match error.kind {
            "NonFinite" => data_kernel::jet_std::DataErrorKind::NonFinite,
            _ => data_kernel::jet_std::DataErrorKind::InvalidArgument,
        },
        operation: error.operation.to_string(),
        row: Err(jet_foundation::Outcome::JetAbsent),
        column: Err(jet_foundation::Outcome::JetAbsent),
        index: match error.index {
            Some(index) => Ok(index),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        reason: error.reason.to_string(),
        cause: Err(jet_foundation::Outcome::JetAbsent),
    };
    match method {
        "line_text" => Ok(data_result_value_for_type(
            data_plot_rt::jet_data_line_text_plot_checked(&groups, &options).map_err(plot_error),
            || data_plot_rt::jet_data_line_text(&groups, &options),
            CtValue::Str,
            resolved_ret,
        )),
        "line_svg" => Ok(data_result_value_for_type(
            data_plot_rt::jet_data_line_svg_plot_checked(&groups, &options).map_err(plot_error),
            || data_plot_rt::jet_data_line_svg(&groups, &options),
            CtValue::Str,
            resolved_ret,
        )),
        _ => Err(unsupported(
            &format!("unsupported core.data line renderer `{method}`"),
            span,
        )),
    }
}
