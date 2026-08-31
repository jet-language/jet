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

pub(crate) fn jet_data_query_rows<T: __jet_Encode + Clone>(
    rows: &Vec<T>,
    sql: &String,
) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
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
