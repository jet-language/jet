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
    T::jet_decode(&tree)
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
    T::jet_decode(&tree)
}
fn jet_enc_toml_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::toml::render(&v.jet_encode())
}
fn jet_enc_yaml_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::yaml::render(&v.jet_encode())
}

// D-SQL-SURFACE1=C: one small, std-only query kernel shared by the CSV file
// door and the in-memory list door. The row stays `T`; DataTree is only the
// private field-inspection bridge needed by the SQL head.
//
// Supported first slice: SELECT * FROM source, optional WHERE field operator
// literal, ORDER BY field [ASC|DESC], and LIMIT n. The plan shape leaves room
// for predicate pushdown, memory budgets, and spill policy without putting
// those policies in a tier adapter.
#[derive(Clone)]
struct JetAnalyticsQuery {
    filter: Option<(String, String, String)>,
    order: Option<(String, bool)>,
    limit: Option<usize>,
}

fn jet_analytics_query_error(message: impl Into<String>) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::one(message.into())
}

fn jet_analytics_query_spec(sql: &str) -> Result<JetAnalyticsQuery, String> {
    let tokens: Vec<String> = sql
        .split_whitespace()
        .map(|token| token.trim_matches(',').to_string())
        .collect();
    if tokens.len() < 4
        || !tokens.first().is_some_and(|token| token.eq_ignore_ascii_case("select"))
        || tokens.get(1).is_none_or(|token| token != "*")
        || !tokens.iter().any(|token| token.eq_ignore_ascii_case("from"))
    {
        return Err("analytics query needs `SELECT * FROM source`".to_string());
    }
    let mut filter = None;
    if let Some(where_at) = tokens.iter().position(|token| token.eq_ignore_ascii_case("where")) {
        let field = tokens.get(where_at + 1).cloned().unwrap_or_default();
        let operator = tokens.get(where_at + 2).cloned().unwrap_or_default();
        let value = tokens.get(where_at + 3).cloned().unwrap_or_default();
        if field.is_empty()
            || value.is_empty()
            || !matches!(operator.as_str(), "=" | "!=" | ">" | ">=" | "<" | "<=")
        {
            return Err("analytics WHERE needs `field operator value`".to_string());
        }
        filter = Some((field, operator, value));
    }
    let order = if let Some(order_at) = tokens.iter().position(|token| token.eq_ignore_ascii_case("order")) {
        if !tokens.get(order_at + 1).is_some_and(|token| token.eq_ignore_ascii_case("by")) {
            return Err("analytics ORDER BY needs a field".to_string());
        }
        let field = tokens.get(order_at + 2).cloned().unwrap_or_default();
        if field.is_empty() {
            return Err("analytics ORDER BY needs a field".to_string());
        }
        let descending = tokens
            .get(order_at + 3)
            .is_some_and(|token| token.eq_ignore_ascii_case("desc"));
        Some((field, descending))
    } else {
        None
    };
    let limit = if let Some(limit_at) = tokens.iter().position(|token| token.eq_ignore_ascii_case("limit")) {
        Some(
            tokens
                .get(limit_at + 1)
                .ok_or_else(|| "analytics LIMIT needs a non-negative integer".to_string())?
                .parse::<usize>()
                .map_err(|_| "analytics LIMIT needs a non-negative integer".to_string())?,
        )
    } else {
        None
    };
    Ok(JetAnalyticsQuery { filter, order, limit })
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

fn jet_analytics_compare(left: &str, operator: &str, right: &str) -> bool {
    let right = right.trim_matches('\'').trim_matches('"');
    if let (Ok(left), Ok(right)) = (left.parse::<f64>(), right.parse::<f64>()) {
        return match operator {
            "=" => left == right,
            "!=" => left != right,
            ">" => left > right,
            ">=" => left >= right,
            "<" => left < right,
            "<=" => left <= right,
            _ => false,
        };
    }
    match operator {
        "=" => left == right,
        "!=" => left != right,
        ">" => left > right,
        ">=" => left >= right,
        "<" => left < right,
        "<=" => left <= right,
        _ => false,
    }
}

fn jet_data_query_rows<T: __jet_Encode + Clone>(rows: &Vec<T>, sql: &String) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    let spec = jet_analytics_query_spec(sql).map_err(jet_analytics_query_error)?;
    let mut selected = rows
        .iter()
        .cloned()
        .filter(|row| {
            let tree = row.jet_encode();
            let Some((field, operator, value)) = &spec.filter else {
                return true;
            };
            jet_std::datatree_get(&tree, field)
                .is_some_and(|cell| jet_analytics_compare(&jet_analytics_cell_text(cell), operator, value))
        })
        .collect::<Vec<_>>();
    if let Some((field, descending)) = &spec.order {
        selected.sort_by(|left, right| {
            let left = jet_std::datatree_get(&left.jet_encode(), field)
                .map(jet_analytics_cell_text)
                .unwrap_or_default();
            let right = jet_std::datatree_get(&right.jet_encode(), field)
                .map(jet_analytics_cell_text)
                .unwrap_or_default();
            if *descending { right.cmp(&left) } else { left.cmp(&right) }
        });
    }
    if let Some(limit) = spec.limit {
        selected.truncate(limit);
    }
    Ok(selected)
}

fn jet_enc_csv_query<T: __jet_Decode + __jet_Encode + Clone>(
    path: &String,
    sql: &String,
) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| jet_analytics_query_error(format!("could not read `{path}`: {error}")))?;
    let rows = jet_enc_csv_decode::<T>(&text)?;
    jet_data_query_rows(&rows, sql)
}
