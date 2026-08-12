use super::*;

/// `[Float]` argument — `core.data`'s stats functions all take `&Vec<f64>`.
pub(super) fn as_float_list(v: &CtValue, span: Span) -> Result<Vec<f64>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs.iter().map(|x| as_float(x, span)).collect(),
        _ => Err(unsupported("core.data: argument must be `[Float]`", span)),
    }
}

/// `[DataGroup]` argument for the bar and line renderers. Every field the
/// kernel validates is read here, so comptime rejects exactly what AOT rejects.
pub(super) fn as_data_groups(
    v: &CtValue,
    span: Span,
) -> Result<Vec<data_kernel::jet_std::DataGroup>, Diagnostic> {
    let CtValue::List(items) = v else {
        return Err(unsupported("core.data: argument must be `[DataGroup]`", span));
    };
    items
        .iter()
        .map(|item| {
            let CtValue::Struct { type_name, fields } = item else {
                return Err(unsupported("core.data: argument must be `[DataGroup]`", span));
            };
            if type_name != "DataGroup" {
                return Err(unsupported("core.data: argument must be `[DataGroup]`", span));
            }
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let (Some(CtValue::Str(key)), Some(CtValue::Int(count))) =
                (field("key"), field("count"))
            else {
                return Err(unsupported(
                    "core.data: a `DataGroup` needs `key: String` and `count: Int`",
                    span,
                ));
            };
            let (Some(sum), Some(mean)) = (field("sum"), field("mean")) else {
                return Err(unsupported(
                    "core.data: a `DataGroup` needs `sum: Float` and `mean: Float`",
                    span,
                ));
            };
            Ok(data_kernel::jet_std::DataGroup {
                key: key.clone(),
                count: *count,
                sum: as_float(sum, span)?,
                mean: as_float(mean, span)?,
            })
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
        _ => Err(unsupported("DataLineOptions string field has the wrong type", span)),
    };
    let markers = match field("markers")? {
        CtValue::Bool(value) => value,
        _ => return Err(unsupported("DataLineOptions `markers` must be Bool", span)),
    };
    let reference = match field("reference")? {
        CtValue::Present(value) => Ok(as_float(&value, span)?),
        CtValue::Failed(CtReport::Clean(_)) => Err(jet_foundation::Outcome::JetAbsent),
        _ => return Err(unsupported("DataLineOptions `reference` must be Float?", span)),
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
    let index = |slot: &jet_foundation::Outcome::JetOutcome<i64, jet_foundation::Outcome::JetAbsent>| match slot {
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
            ("operation".to_string(), CtValue::Str(error.operation.clone())),
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
    if !data_checked_surface() {
        return to_value(unchecked());
    }
    match checked {
        Ok(value) => CtValue::Present(Box::new(to_value(value))),
        Err(error) => CtValue::failed(Box::new(data_error_value(&error))),
    }
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
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let groups = as_data_groups(
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
        "line_text" => Ok(data_result_value(
            data_plot_rt::jet_data_line_text_plot_checked(&groups, &options).map_err(plot_error),
            || data_plot_rt::jet_data_line_text(&groups, &options),
            CtValue::Str,
        )),
        "line_svg" => Ok(data_result_value(
            data_plot_rt::jet_data_line_svg_plot_checked(&groups, &options).map_err(plot_error),
            || data_plot_rt::jet_data_line_svg(&groups, &options),
            CtValue::Str,
        )),
        _ => Err(unsupported(
            &format!("unsupported core.data line renderer `{method}`"),
            span,
        )),
    }
}
