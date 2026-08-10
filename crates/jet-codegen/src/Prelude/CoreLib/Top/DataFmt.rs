fn jet_data_inner_join<T, U, FL, FR>(
    left: &Vec<T>,
    right: &Vec<U>,
    left_key: FL,
    right_key: FR,
) -> Vec<jet_std::DataJoin<T, U>>
where
    T: Clone,
    U: Clone,
    FL: Fn(T) -> String,
    FR: Fn(U) -> String,
{
    let mut right_rows = std::collections::BTreeMap::<String, Vec<U>>::new();
    for row in right.iter().cloned() {
        right_rows.entry(right_key(row.clone())).or_default().push(row);
    }
    let mut joined = Vec::new();
    for left_row in left.iter().cloned() {
        if let Some(matches) = right_rows.get(&left_key(left_row.clone())) {
            for right_row in matches {
                joined.push(jet_std::DataJoin {
                    left: left_row.clone(),
                    right: right_row.clone(),
                });
            }
        }
    }
    joined
}

fn jet_data_left_join<T, U, FL, FR>(
    left: &Vec<T>,
    right: &Vec<U>,
    left_key: FL,
    right_key: FR,
) -> Vec<jet_std::DataJoin<T, JetOutcome<U, JetAbsent>>>
where
    T: Clone,
    U: Clone,
    FL: Fn(T) -> String,
    FR: Fn(U) -> String,
{
    let mut right_rows = std::collections::BTreeMap::<String, Vec<U>>::new();
    for row in right.iter().cloned() {
        right_rows.entry(right_key(row.clone())).or_default().push(row);
    }
    let mut joined = Vec::new();
    for left_row in left.iter().cloned() {
        match right_rows.get(&left_key(left_row.clone())) {
            Some(matches) => {
                for right_row in matches {
                    joined.push(jet_std::DataJoin {
                        left: left_row.clone(),
                        right: Ok(right_row.clone()),
                    });
                }
            }
            None => joined.push(jet_std::DataJoin {
                left: left_row,
                right: Err(JetAbsent),
            }),
        }
    }
    joined
}

fn jet_data_pivot_sum<T, FR, FC, FV>(
    rows: &Vec<T>,
    row_key: FR,
    col_key: FC,
    value: FV,
) -> Vec<jet_std::DataGroup>
where
    T: Clone,
    FR: Fn(T) -> String,
    FC: Fn(T) -> String,
    FV: Fn(T) -> f64,
{
    let mut groups = std::collections::BTreeMap::<String, (i64, f64)>::new();
    for row in rows.iter().cloned() {
        let key = format!("{}|{}", row_key(row.clone()), col_key(row.clone()));
        let entry = groups.entry(key).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += value(row);
    }
    groups
        .into_iter()
        .map(|(key, (count, sum))| jet_std::DataGroup {
            key,
            count,
            sum,
            mean: if count == 0 { 0.0 } else { sum / count as f64 },
        })
        .collect()
}

// D-MIGRATE3=A: traced sibling of `jet_enc_csv_decode` — see json's for the shape.
fn jet_enc_csv_decode_traced<T: __jet_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<Vec<T>>, Vec<jet_std::FieldError>> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::FieldError::one)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else {
        return Ok(jet_std::DecodeResult {
            value: Vec::new(),
            migration: jet_std::MigrationStatus::fresh(),
        });
    };
    let mut value = Vec::new();
    let mut errors = Vec::new();
    // Each row decodes independently; the record-level status is the first row
    // that actually migrated (a CSV file is one shape per column layout, so a
    // migrated file migrates uniformly — the first hit describes the batch).
    let mut migration = jet_std::MigrationStatus::fresh();
    for (i, row) in it.enumerate() {
        let obj: Vec<(String, jet_std::DataTree)> = header
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let cell = row.get(c).cloned().unwrap_or_default();
                (name.clone(), jet_std::DataTree::Text(cell))
            })
            .collect();
        let tree = jet_std::DataTree::Object(obj);
        match T::jet_decode_traced(&tree) {
            Ok((v, m)) => {
                if m.migrated && !migration.migrated {
                    migration = m;
                }
                value.push(v);
            }
            Err(error) => errors.extend(jet_std::FieldError::under_errors(
                &format!("row {}", i + 1),
                error,
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(jet_std::DecodeResult { value, migration })
}

// CSV typed encode: `[T]` → header row (field names from the first row's Object)
// + one record per element. Requires every element to encode to a flat Object.
fn jet_enc_csv_to_string<T: __jet_Encode>(values: &Vec<T>) -> String {
    let trees: Vec<jet_std::DataTree> = values.iter().map(|v| v.jet_encode()).collect();
    let mut header: Vec<String> = Vec::new();
    if let Some(jet_std::DataTree::Object(entries)) = trees.first() {
        header = entries.iter().map(|(k, _)| k.clone()).collect();
    } else if !trees.is_empty() {
        jet_panic(
            "<core.encoding.csv>",
            0,
            "csv.to_string needs rows or records",
        );
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
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
                Some(jet_std::DataTree::Int(n)) => n.to_string(),
                Some(jet_std::DataTree::Float(f)) => format!("{:?}", f),
                Some(jet_std::DataTree::Bool(b)) => b.to_string(),
                Some(jet_std::DataTree::Null) | None => String::new(),
                Some(other) => jet_std::render_datatree_json(other, false, 0),
            };
            record.push(cell);
        }
        rows.push(record);
    }
    jet_ring_csv_render(&rows)
}

// D-ENC-DYN1=A+ (c152): TOML is a full serde-equivalent adapter over the one rich
// `DataTree` — nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars.
// The dynamic `parse` returns the `Data` value; `decode<T>` walks the rich tree;
// `to_string` renders a `DataTree` back to a nested document.
fn jet_std_toml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JSONError> {
    jet_std::toml::parse_to_tree(text).map_err(|e| jet_std::JSONError {
        line: e.line as i64,
        message: e.message,
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
    Ok(T::jet_decode_traced(&tree)?.0)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_toml_decode` — see json's for the shape.
fn jet_enc_toml_decode_traced<T: __jet_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, Vec<jet_std::FieldError>> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::FieldError::one(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&tree)?;
    Ok(jet_std::DecodeResult { value, migration })
}

// YAML typed decode: parse flat scalars into a DataTree::Object of Text, then decode.
// D-ENC-DYN1=A+ / D-ENC-YAML1 (c152): YAML is a full serde adapter over the one
// rich `DataTree` — block + flow maps/sequences, typed core scalars, block scalars,
// comments, documents, anchors/aliases. parse → `Data`; decode<T> → typed tree.
fn jet_std_yaml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JSONError> {
    jet_std::yaml::parse_to_tree(text).map_err(|e| jet_std::JSONError {
        line: e.line as i64,
        message: e.message,
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
    Ok(T::jet_decode_traced(&tree)?.0)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_yaml_decode` — see json's for the shape.
fn jet_enc_yaml_decode_traced<T: __jet_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, Vec<jet_std::FieldError>> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::FieldError::one(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&tree)?;
    Ok(jet_std::DecodeResult { value, migration })
}
fn jet_enc_toml_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::toml::render(&v.jet_encode())
}
fn jet_enc_yaml_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::yaml::render(&v.jet_encode())
}
