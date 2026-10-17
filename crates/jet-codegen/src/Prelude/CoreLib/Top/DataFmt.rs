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



// D-ENC-DYN1=A+ (c152): TOML is a full serde-equivalent adapter over the one rich
// `Dyn` — nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars.
// The dynamic `parse` returns a `Dyn` value; `decode<T>` walks the rich tree;
// `to_string` renders a `Dyn` back to a nested document.
fn jet_std_toml_parse(text: &String) -> Result<jet_std::Dyn, jet_std::EncodingError> {
    jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::EncodingError::new(
            jet_std::EncodingFormat::TOML,
            if e.unsupported { jet_std::EncodingErrorKind::Unsupported } else { jet_std::EncodingErrorKind::Syntax },
            0,
            Ok(e.line as i64),
            Err(JetAbsent),
            "",
            e.message,
        )
    })
}

fn jet_enc_toml_decode<T: __jet_Decode>(text: &str) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::FieldError::one(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    T::jet_decode(&tree)
}

// YAML typed decode: parse flat scalars into a Dyn::Object of Text, then decode.
// D-ENC-DYN1=A+ / D-ENC-YAML1 (c152): YAML is a full serde adapter over the one
// rich `Dyn` — block + flow maps/sequences, typed core scalars, block scalars,
// comments, documents, anchors/aliases. parse → `Data`; decode<T> → typed tree.
fn jet_std_yaml_parse(text: &String) -> Result<jet_std::Dyn, jet_std::EncodingError> {
    jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::EncodingError::new(
            jet_std::EncodingFormat::YAML,
            if e.unsupported { jet_std::EncodingErrorKind::Unsupported } else { jet_std::EncodingErrorKind::Syntax },
            0,
            Ok(e.line as i64),
            Err(JetAbsent),
            "",
            e.message,
        )
    })
}

fn jet_enc_yaml_decode<T: __jet_Decode>(text: &str) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::FieldError::one(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    T::jet_decode(&tree)
}

// D-SQL-SURFACE1=C: the query plan and field comparison live in the shared
// non-generic Prelude entry. This typed wrapper only encodes rows and maps
// selected positions back to `T`, preserving one query implementation across
// AOT, TIR, and resident adapters.
fn jet_data_query_rows<T: __jet_Encode + Clone>(rows: &Vec<T>, sql: &jet_std::SQL) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    let trees = rows.iter().map(|row| row.jet_encode()).collect::<Vec<_>>();
    let spec = jet_analytics_checked_query_spec(sql).map_err(jet_analytics_query_error)?;
    let selected = jet_data_query_indices_spec(&trees, &spec)?;
    Ok(selected.into_iter().map(|index| rows[index].clone()).collect())
}

