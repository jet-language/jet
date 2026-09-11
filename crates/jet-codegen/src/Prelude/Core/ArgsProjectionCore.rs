// D-SHAPE-PROJECT1: host-neutral argument projection helpers. The parser and
// checked shape map stay shared; hosts only choose how to marshal this tree.

#[derive(Clone, Debug)]
pub(crate) enum JetArgsShapeValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Array(Vec<JetArgsShapeValue>),
}

#[derive(Clone, Debug)]
pub(crate) struct JetArgsShapeError {
    pub(crate) path: String,
    pub(crate) reason: String,
}

fn jet_args_projection_name(names: &[(&str, &str)], source: &str) -> String {
    names
        .iter()
        .find(|(field, _)| *field == source)
        .map(|(_, key)| (*key).to_string())
        .unwrap_or_else(|| source.to_string())
}

/// Build one canonical argument tree from the already parsed builder result.
/// No host-specific value carrier or decoder policy belongs here.
fn jet_args_shape_tree_values(
    spec: &JetArgsSpec,
    parsed: &JetParsedArgs,
    projection: &[(&str, &str)],
) -> Result<Vec<(String, JetArgsShapeValue)>, Vec<JetArgsShapeError>> {
    let mut fields = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for entry in jet_args_all_entries(spec) {
        match entry {
            JetArgKind::Flag { name, .. } => {
                let output_name = jet_args_projection_name(projection, name);
                if !seen.insert(output_name.clone()) {
                    return Err(vec![JetArgsShapeError {
                        path: output_name,
                        reason: "ambiguous duplicate argument shape field".to_string(),
                    }]);
                }
                fields.push((
                    output_name,
                    JetArgsShapeValue::Bool(*parsed.flags.get(name).unwrap_or(&false)),
                ));
            }
            JetArgKind::Option {
                name,
                repeat,
                value,
                ..
            } => {
                let output_name = jet_args_projection_name(projection, name);
                if !seen.insert(output_name.clone()) {
                    return Err(vec![JetArgsShapeError {
                        path: output_name,
                        reason: "ambiguous duplicate argument shape field".to_string(),
                    }]);
                }
                let Some(values) = parsed.options.get(name) else {
                    continue;
                };
                let values = values
                    .iter()
                    .map(|raw| jet_args_shape_value(value, raw, &output_name))
                    .collect::<Result<Vec<_>, _>>()?;
                let value = if *repeat {
                    JetArgsShapeValue::Array(values)
                } else {
                    values
                        .into_iter()
                        .last()
                        .unwrap_or_else(|| JetArgsShapeValue::Text(String::new()))
                };
                fields.push((output_name, value));
            }
            JetArgKind::Positional { name, .. } => {
                // A named positional and option share one canonical field.
                let output_name = jet_args_projection_name(projection, name);
                if !seen.insert(output_name.clone()) {
                    continue;
                }
                let Some(value) = parsed.options.get(name).and_then(|values| values.last()) else {
                    continue;
                };
                fields.push((
                    output_name,
                    JetArgsShapeValue::Text(value.clone()),
                ));
            }
            JetArgKind::Subcommand { .. } => {
                // A subcommand selects a nested spec; it is not a field in the
                // decoded record and must not be invented as one.
            }
        }
    }
    Ok(fields)
}

fn jet_args_shape_value(
    kind: &JetArgValueKind,
    value: &str,
    name: &str,
) -> Result<JetArgsShapeValue, Vec<JetArgsShapeError>> {
    match kind {
        JetArgValueKind::String | JetArgValueKind::Choice(_) => {
            Ok(JetArgsShapeValue::Text(value.to_string()))
        }
        JetArgValueKind::Int => value
            .parse::<i64>()
            .map(JetArgsShapeValue::Int)
            .map_err(|_| {
                vec![JetArgsShapeError {
                    path: name.to_string(),
                    reason: format!("expected Int, found {value:?}"),
                }]
            }),
        JetArgValueKind::Float => value
            .parse::<f64>()
            .map(JetArgsShapeValue::Float)
            .map_err(|_| {
                vec![JetArgsShapeError {
                    path: name.to_string(),
                    reason: format!("expected Float, found {value:?}"),
                }]
            }),
    }
}

/// Merge the two already decoded layers using the parser's explicit-presence
/// set. The value type is supplied by the host, so this remains the one shared
/// merge policy without coupling the helper to a runtime carrier.
fn jet_args_merge_fields<T>(
    spec: &JetArgsSpec,
    argv: &Vec<String>,
    names: &[(&str, &str)],
    flag_fields: Vec<(String, T)>,
    mut settings_fields: Vec<(String, T)>,
) -> Result<Vec<(String, T)>, Vec<JetArgsShapeError>> {
    let parsed = jet_args_parse_guided(spec, argv).map_err(|message| {
        vec![JetArgsShapeError {
            path: String::new(),
            reason: message,
        }]
    })?;
    let explicit = parsed
        .explicit_flags
        .iter()
        .chain(parsed.explicit_options.iter())
        .map(|name| jet_args_projection_name(names, name))
        .collect::<std::collections::BTreeSet<String>>();
    for (name, value) in flag_fields {
        if !explicit.contains(&name) {
            continue;
        }
        match settings_fields.iter_mut().find(|(existing, _)| *existing == name) {
            Some(slot) => slot.1 = value,
            None => settings_fields.push((name, value)),
        }
    }
    Ok(settings_fields)
}
