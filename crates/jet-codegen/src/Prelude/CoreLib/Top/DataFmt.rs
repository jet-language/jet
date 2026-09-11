fn jet_data_pivot_sum<T, FR, FC, FV>(
    rows: &Vec<T>,
    row_key: FR,
    col_key: FC,
    value: FV,
) -> Vec<jet_std::DataPivotCell>
where
    T: Clone,
    FR: Fn(T) -> String,
    FC: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    let mut groups = std::collections::BTreeMap::<(String, String), (i64, f64)>::new();
    for row in rows.iter().cloned() {
        let key = (row_key(row.clone()), col_key(row.clone()));
        let entry = groups.entry(key).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += value(row);
    }
    groups
        .into_iter()
        .map(|((row_key, column_key), (count, sum))| jet_std::DataPivotCell {
            row_key,
            column_key,
            count,
            sum,
            mean: if count == 0 { 0.0 } else { sum / count as f64 },
        })
        .collect()
}


// CSV typed encode: `[T]` → header row (field names from the first row's Object)
// + one record per element. Flat object records and explicit row arrays share the
// same renderer: rows already carry their header as the first row.
fn jet_enc_csv_to_string<T: __jet_Encode>(values: &Vec<T>) -> String {
    let trees: Vec<jet_std::DataTree> = values.iter().map(|v| v.jet_encode()).collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    if let Some(jet_std::DataTree::Object(entries)) = trees.first() {
        let header: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
        rows.push(header.clone());
        for tree in &trees {
            if !matches!(tree, jet_std::DataTree::Object(_)) {
                jet_panic(
                    "<core.encoding.csv>",
                    0,
                    "csv.to_string needs rows or records",
                );
            }
            let mut record = Vec::with_capacity(header.len());
            for key in &header {
                let cell = match jet_std::datatree_get(tree, key) {
                    Some(jet_std::DataTree::Text(s)) => s.clone(),
                    Some(jet_std::DataTree::Int(n)) => jet_std::jet_int_to_string(*n),
                    Some(jet_std::DataTree::Float(f)) => format!("{:?}", f),
                    Some(jet_std::DataTree::Bool(b)) => b.to_string(),
                    Some(jet_std::DataTree::Null) | None => String::new(),
                    Some(other) => jet_std::render_datatree_json(other, false, 0),
                };
                record.push(cell);
            }
            rows.push(record);
        }
    } else {
        for tree in trees {
            let jet_std::DataTree::Array(cells) = tree else {
                jet_panic(
                    "<core.encoding.csv>",
                    0,
                    "csv.to_string needs rows or records",
                );
            };
            rows.push(
                cells
                    .iter()
                    .map(|cell| match cell {
                        jet_std::DataTree::Text(s) => s.clone(),
                        jet_std::DataTree::Int(n) => jet_std::jet_int_to_string(*n),
                        jet_std::DataTree::Float(f) => format!("{:?}", f),
                        jet_std::DataTree::Bool(b) => b.to_string(),
                        jet_std::DataTree::Null => String::new(),
                        other => jet_std::render_datatree_json(other, false, 0),
                    })
                    .collect(),
            );
        }
    }
    jet_ring_csv_render(&rows)
}

/// D-SHAPE-ONE1=A: CSV is only an adapter over the canonical shape projection.
/// The projection supplies field identities and order; this function only
/// renders the selected values.
fn jet_enc_csv_to_string_shape<T: __jet_Encode>(
    values: &Vec<T>,
    projection: &ShapeProjection,
) -> Result<String, Vec<jet_std::FieldError>> {
    if projection.kind != ShapeProjectionKind::Csv {
        return Err(jet_std::FieldError::one("CSV needs a CSV shape projection"));
    }
    let encoded = jet_std::DataTree::Array(
        values
            .iter()
            .map(|value| value.jet_encode())
            .collect::<Vec<_>>(),
    );
    let projected = jet_std::jet_datatree_project(&encoded, projection)?;
    let jet_std::DataTree::Array(rows) = projected else {
        return Err(jet_std::FieldError::one(
            "CSV shape projection needs object rows",
        ));
    };
    let header = match rows.first() {
        Some(jet_std::DataTree::Object(entries)) => entries
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>(),
        Some(_) => {
            return Err(jet_std::FieldError::one(
                "CSV shape projection needs object rows",
            ))
        }
        None => Vec::new(),
    };
    let mut output = vec![header.clone()];
    for row in &rows {
        let jet_std::DataTree::Object(_) = row else {
            return Err(jet_std::FieldError::one(
                "CSV shape projection needs object rows",
            ));
        };
        output.push(
            header
                .iter()
                .map(|name| match jet_std::datatree_get(row, name) {
                    Some(jet_std::DataTree::Text(value)) => value.clone(),
                    Some(jet_std::DataTree::Int(value)) => jet_std::jet_int_to_string(*value),
                    Some(jet_std::DataTree::Float(value)) => format!("{value:?}"),
                    Some(jet_std::DataTree::Bool(value)) => value.to_string(),
                    Some(jet_std::DataTree::Null) | None => String::new(),
                    Some(other) => jet_std::render_datatree_json(other, false, 0),
                })
                .collect::<Vec<_>>(),
        );
    }
    Ok(jet_ring_csv_render(&output))
}

// D-ENC-DYN1=A+ (c152): TOML is a full serde-equivalent adapter over the one rich
// `DataTree` — nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars.
// The dynamic `parse` returns the `Data` value; `decode<T>` walks the rich tree;
// `to_string` renders a `DataTree` back to a nested document.
fn jet_std_toml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::EncodingError> {
    jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::EncodingError::new(
            jet_std::EncodingFormat::TOML,
            jet_std::EncodingErrorKind::Syntax,
            0,
            Ok(e.line as i64),
            Err(JetAbsent),
            "",
            e.message,
        )
    })
}
fn jet_std_toml_render(d: &jet_std::DataTree) -> String {
    jet_std::toml::render(d)
}

fn jet_enc_toml_decode<T: __jet_Decode>(text: &String) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::FieldError::one(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    T::jet_decode(&tree)
}

// YAML typed decode: parse flat scalars into a DataTree::Object of Text, then decode.
// D-ENC-DYN1=A+ / D-ENC-YAML1 (c152): YAML is a full serde adapter over the one
// rich `DataTree` — block + flow maps/sequences, typed core scalars, block scalars,
// comments, documents, anchors/aliases. parse → `Data`; decode<T> → typed tree.
fn jet_std_yaml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::EncodingError> {
    jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::EncodingError::new(
            jet_std::EncodingFormat::YAML,
            jet_std::EncodingErrorKind::Syntax,
            0,
            Ok(e.line as i64),
            Err(JetAbsent),
            "",
            e.message,
        )
    })
}
fn jet_std_yaml_render(d: &jet_std::DataTree) -> String {
    jet_std::yaml::render(d)
}

fn jet_enc_yaml_decode<T: __jet_Decode>(text: &String) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::FieldError::one(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    T::jet_decode(&tree)
}
fn jet_enc_toml_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::toml::render(&v.jet_encode())
}
fn jet_enc_yaml_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::yaml::render(&v.jet_encode())
}

fn jet_enc_toml_to_string_shape<T: __jet_Encode>(
    value: &T,
    projection: &ShapeProjection,
) -> Result<String, Vec<jet_std::FieldError>> {
    if projection.kind != ShapeProjectionKind::Toml {
        return Err(jet_std::FieldError::one("TOML needs a TOML shape projection"));
    }
    let projected = jet_std::jet_datatree_project(&value.jet_encode(), projection)?;
    Ok(jet_std::toml::render(&projected))
}

fn jet_enc_yaml_to_string_shape<T: __jet_Encode>(
    value: &T,
    projection: &ShapeProjection,
) -> Result<String, Vec<jet_std::FieldError>> {
    if projection.kind != ShapeProjectionKind::Yaml {
        return Err(jet_std::FieldError::one("YAML needs a YAML shape projection"));
    }
    let projected = jet_std::jet_datatree_project(&value.jet_encode(), projection)?;
    Ok(jet_std::yaml::render(&projected))
}

// D-SQL-SURFACE1=C: the query plan and field comparison live in the shared
// non-generic Prelude entry. This typed wrapper only encodes rows and maps
// selected positions back to `T`, preserving one query implementation across
// AOT, TIR, and resident adapters.
fn jet_data_query_rows<T: __jet_Encode + Clone>(rows: &Vec<T>, sql: &String) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    let trees = rows.iter().map(|row| row.jet_encode()).collect::<Vec<_>>();
    let selected = jet_data_query_indices(&trees, sql)?;
    Ok(selected.into_iter().map(|index| rows[index].clone()).collect())
}

