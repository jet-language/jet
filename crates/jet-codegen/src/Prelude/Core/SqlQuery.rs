// D-SQL-SURFACE1=A: one parser and row-selection kernel for typed in-memory
// tables and the `jet db` console. Callers only marshal row values and plans.
// This fragment is included by the Prelude query entry and the host console;
// neither side owns a second SQL grammar or comparison implementation.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetSqlAggregate {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl JetSqlAggregate {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetSqlSelectItem {
    Wildcard,
    Field {
        field: String,
        alias: Option<String>,
    },
    Aggregate {
        function: JetSqlAggregate,
        field: Option<String>,
        alias: Option<String>,
    },
}

impl JetSqlSelectItem {
    pub fn output_name(&self) -> String {
        match self {
            Self::Wildcard => "*".to_string(),
            Self::Field { field, alias } => alias.clone().unwrap_or_else(|| field.clone()),
            Self::Aggregate {
                function,
                field,
                alias,
            } => alias.clone().unwrap_or_else(|| match field {
                Some(field) => format!("{}_{}", function.as_str(), field),
                None => function.as_str().to_string(),
            }),
        }
    }

    fn field(&self) -> Option<&str> {
        match self {
            Self::Field { field, .. } => Some(field),
            Self::Aggregate {
                field: Some(field),
                ..
            } => Some(field),
            Self::Wildcard | Self::Aggregate { field: None, .. } => None,
        }
    }

    fn is_aggregate(&self) -> bool {
        matches!(self, Self::Aggregate { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetSqlGroupKey {
    Field(String),
    Ordinal(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetSqlQuery {
    pub source: String,
    pub projection: Vec<JetSqlSelectItem>,
    pub group_by: Vec<JetSqlGroupKey>,
    pub filter: Option<(String, String, String)>,
    pub order: Option<(String, bool)>,
    pub limit: Option<usize>,
}

impl JetSqlQuery {
    pub fn is_identity_projection(&self) -> bool {
        self.projection.len() == 1 && matches!(self.projection[0], JetSqlSelectItem::Wildcard)
    }

    pub fn has_aggregate(&self) -> bool {
        self.projection
            .iter()
            .any(JetSqlSelectItem::is_aggregate)
    }

    pub fn needs_projection_execution(&self) -> bool {
        !self.is_identity_projection() || !self.group_by.is_empty() || self.has_aggregate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetSqlValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl JetSqlValue {
    pub fn literal(token: &str) -> Self {
        let trimmed = token.trim();
        if trimmed.len() >= 2
            && matches!(trimmed.as_bytes()[0] as char, '\'' | '"')
            && trimmed.as_bytes()[0] as char == trimmed.as_bytes()[trimmed.len() - 1] as char
        {
            return Self::Text(unquote(trimmed));
        }
        if trimmed.eq_ignore_ascii_case("null") {
            Self::Null
        } else if trimmed.eq_ignore_ascii_case("true") {
            Self::Bool(true)
        } else if trimmed.eq_ignore_ascii_case("false") {
            Self::Bool(false)
        } else if let Ok(value) = trimmed.parse::<i64>() {
            Self::Int(value)
        } else if let Ok(value) = trimmed.parse::<f64>() {
            Self::Float(value)
        } else {
            Self::Text(unquote(trimmed))
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Int(value) => Some(*value as f64),
            Self::Float(value) => Some(*value),
            Self::Null | Self::Bool(_) | Self::Text(_) => None,
        }
    }

    fn canonical_key(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(value) => format!("bool:{value}"),
            Self::Int(value) => format!("int:{value}"),
            Self::Float(value) => format!("float:{}", sql_number(*value)),
            Self::Text(value) => format!("text:{}", sql_escape(value)),
        }
    }

    fn display_text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => sql_number(*value),
            Self::Text(value) => value.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetSqlResultRow {
    pub fields: Vec<(String, JetSqlValue)>,
    pub source_indices: Vec<usize>,
}

fn sql_error(message: impl Into<String>) -> String {
    message.into()
}

fn push_token(tokens: &mut Vec<String>, token: &mut String) {
    if !token.is_empty() {
        tokens.push(std::mem::take(token));
    }
}

fn sql_tokens(sql: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            token.push(ch);
            if ch == delimiter {
                if chars.peek() == Some(&delimiter) {
                    token.push(chars.next().unwrap_or(delimiter));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        if ch == '-' && chars.peek() == Some(&'-') {
            chars.next();
            push_token(&mut tokens, &mut token);
            line_comment = true;
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            push_token(&mut tokens, &mut token);
            block_comment = true;
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                token.push(ch);
            }
            c if c.is_whitespace() => push_token(&mut tokens, &mut token),
            ',' | '(' | ')' => {
                push_token(&mut tokens, &mut token);
                tokens.push(ch.to_string());
            }
            '=' | '!' | '>' | '<' => {
                push_token(&mut tokens, &mut token);
                let mut op = ch.to_string();
                if chars.peek() == Some(&'=') {
                    op.push(chars.next().unwrap_or('='));
                }
                tokens.push(op);
            }
            ';' => {
                push_token(&mut tokens, &mut token);
                tokens.push(";".to_string());
            }
            _ => token.push(ch),
        }
    }
    if quote.is_some() {
        return Err(sql_error("SQL string literal is missing its closing quote"));
    }
    if block_comment {
        return Err(sql_error("SQL block comment is missing its closing `*/`"));
    }
    push_token(&mut tokens, &mut token);
    Ok(tokens)
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0] as char;
        let last = bytes[value.len() - 1] as char;
        if (first == '\'' || first == '"') && first == last {
            let inner = &value[1..value.len() - 1];
            return inner.replace(&format!("{first}{first}"), &first.to_string());
        }
    }
    value.to_string()
}

fn identifier(value: Option<&String>, role: &str) -> Result<String, String> {
    let value = value.ok_or_else(|| sql_error(format!("SQL {role} is missing")))?;
    let raw = value.trim();
    let quoted = raw.len() >= 2
        && matches!(raw.as_bytes()[0] as char, '\'' | '"')
        && raw.as_bytes()[0] as char == raw.as_bytes()[raw.len() - 1] as char;
    let value = unquote(raw).trim().to_string();
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(sql_error(format!("SQL {role} must be an identifier")));
    };
    if (!quoted && !first.is_ascii_alphabetic() && first != '_')
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '$'))
    {
        return Err(sql_error(format!("SQL {role} must be an identifier")));
    }
    Ok(value)
}

fn clause_keyword(token: &str) -> bool {
    token.eq_ignore_ascii_case("from")
        || token.eq_ignore_ascii_case("where")
        || token.eq_ignore_ascii_case("group")
        || token.eq_ignore_ascii_case("order")
        || token.eq_ignore_ascii_case("limit")
        || token == ";"
}

fn parse_alias(tokens: &[String], cursor: &mut usize) -> Result<Option<String>, String> {
    if tokens
        .get(*cursor)
        .is_some_and(|token| token.eq_ignore_ascii_case("as"))
    {
        *cursor += 1;
        let alias = identifier(tokens.get(*cursor), "SELECT alias")?;
        *cursor += 1;
        return Ok(Some(alias));
    }
    if tokens
        .get(*cursor)
        .is_some_and(|token| token != "," && !clause_keyword(token))
    {
        let alias = identifier(tokens.get(*cursor), "SELECT alias")?;
        *cursor += 1;
        return Ok(Some(alias));
    }
    Ok(None)
}

fn parse_select_item(tokens: &[String], cursor: &mut usize) -> Result<JetSqlSelectItem, String> {
    let first = tokens
        .get(*cursor)
        .ok_or_else(|| sql_error("SQL SELECT needs a projection"))?;
    if first == "*" {
        *cursor += 1;
        if tokens
            .get(*cursor)
            .is_some_and(|token| token.eq_ignore_ascii_case("as"))
        {
            return Err(sql_error("SQL wildcard projection cannot have an alias"));
        }
        return Ok(JetSqlSelectItem::Wildcard);
    }
    let name = identifier(Some(first), "SELECT column or function")?;
    *cursor += 1;
    if tokens.get(*cursor).map(String::as_str) != Some("(") {
        let alias = parse_alias(tokens, cursor)?;
        return Ok(JetSqlSelectItem::Field {
            field: name,
            alias,
        });
    }
    *cursor += 1;
    let function = match name.to_ascii_lowercase().as_str() {
        "count" => JetSqlAggregate::Count,
        "sum" => JetSqlAggregate::Sum,
        "avg" | "average" => JetSqlAggregate::Avg,
        "min" => JetSqlAggregate::Min,
        "max" => JetSqlAggregate::Max,
        _ => {
            return Err(sql_error(format!(
                "unsupported SQL aggregate `{name}`; accepted functions are COUNT, SUM, AVG, MIN, and MAX"
            )))
        }
    };
    let field = if tokens.get(*cursor).map(String::as_str) == Some("*") {
        *cursor += 1;
        if function != JetSqlAggregate::Count {
            return Err(sql_error("SQL `*` is only valid inside COUNT(*)"));
        }
        None
    } else {
        let field = identifier(tokens.get(*cursor), "aggregate column")?;
        *cursor += 1;
        Some(field)
    };
    if tokens.get(*cursor).map(String::as_str) != Some(")") {
        return Err(sql_error("SQL aggregate call is missing its closing `)`"));
    }
    *cursor += 1;
    let alias = parse_alias(tokens, cursor)?;
    Ok(JetSqlSelectItem::Aggregate {
        function,
        field,
        alias,
    })
}


fn group_field(query: &JetSqlQuery, key: &JetSqlGroupKey) -> Result<String, String> {
    match key {
        JetSqlGroupKey::Field(field) => Ok(field.clone()),
        JetSqlGroupKey::Ordinal(ordinal) => {
            let Some(item) = ordinal.checked_sub(1).and_then(|index| query.projection.get(index))
            else {
                return Err(sql_error(format!(
                    "SQL GROUP BY ordinal {ordinal} is outside the SELECT list"
                )));
            };
            match item {
                JetSqlSelectItem::Field { field, .. } => Ok(field.clone()),
                JetSqlSelectItem::Wildcard => Err(sql_error(
                    "SQL GROUP BY ordinal cannot reference a wildcard projection",
                )),
                JetSqlSelectItem::Aggregate { .. } => Err(sql_error(
                    "SQL GROUP BY ordinal must reference a non-aggregate projection",
                )),
            }
        }
    }
}

fn query_shape_error(query: &JetSqlQuery) -> Option<String> {
    if query.projection.is_empty() {
        return Some("SQL SELECT needs at least one projection".to_string());
    }
    if query
        .projection
        .iter()
        .filter(|item| matches!(item, JetSqlSelectItem::Wildcard))
        .count()
        > 0
        && query.projection.len() != 1
    {
        return Some("SQL wildcard projection cannot be combined with another projection".to_string());
    }
    if query.projection.iter().any(|item| matches!(item, JetSqlSelectItem::Wildcard))
        && !query.group_by.is_empty()
    {
        return Some("SQL GROUP BY needs explicit projected columns".to_string());
    }
    let mut names = std::collections::BTreeSet::new();
    for item in &query.projection {
        let name = item.output_name();
        if !names.insert(name.clone()) {
            return Some(format!("SQL SELECT repeats output column `{name}`"));
        }
    }
    for (index, key) in query.group_by.iter().enumerate() {
        if let Err(error) = group_field(query, key) {
            return Some(error);
        }
        if query.group_by[..index].contains(key) {
            return Some("SQL GROUP BY repeats a grouping key".to_string());
        }
    }
    let has_aggregate = query.has_aggregate();
    let grouped_fields = query
        .group_by
        .iter()
        .filter_map(|key| group_field(query, key).ok())
        .collect::<std::collections::BTreeSet<_>>();
    for item in &query.projection {
        if let JetSqlSelectItem::Field { field, .. } = item {
            if has_aggregate && !grouped_fields.contains(field) {
                return Some(format!(
                    "SQL SELECT column `{field}` must appear in GROUP BY when an aggregate is selected"
                ));
            }
            if !has_aggregate && !query.group_by.is_empty() && !grouped_fields.contains(field) {
                return Some(format!(
                    "SQL SELECT column `{field}` must appear in GROUP BY"
                ));
            }
        }
    }
    None
}

/// Visit every field referenced by the query's row-level clauses and
/// projection/group expressions.
pub fn sql_query_fields<F>(query: &JetSqlQuery, mut visit: F)
where
    F: FnMut(&str),
{
    for item in &query.projection {
        if let Some(field) = item.field() {
            visit(field);
        }
    }
    for key in &query.group_by {
        if let Ok(field) = group_field(query, key) {
            visit(&field);
        }
    }
    if let Some((field, _, _)) = &query.filter {
        visit(field);
    }
    if let Some((field, _)) = &query.order {
        let is_output_name = query
            .projection
            .iter()
            .any(|item| item.output_name() == *field);
        let is_ordinal = field.parse::<usize>().is_ok();
        if !is_output_name && !is_ordinal {
            visit(field);
        }
    }
}

/// Validate row-field references against the caller's declared schema.
///
/// The callback supplies schema membership; the parser and every tier share
/// this traversal, while the label keeps each public entry point's diagnostic
/// wording stable.
pub fn validate_sql_query_schema<F>(
    query: &JetSqlQuery,
    mut has_field: F,
    label: &str,
) -> Result<(), String>
where
    F: FnMut(&str) -> bool,
{
    let mut missing = None;
    sql_query_fields(query, |field| {
        if missing.is_none() && !has_field(field) {
            missing = Some(field.to_string());
        }
    });
    missing.map_or(Ok(()), |field| {
        Err(sql_error(format!("{label} references unknown field `{field}`")))
    })
}

/// Parse the checked SQL surface. It supports wildcard or explicit field
/// projections, COUNT/SUM/AVG/MIN/MAX, WHERE, GROUP BY (including ordinals),
/// ORDER BY, and LIMIT. Mutating statements, joins, and arbitrary expressions
/// fail before a row is touched.
pub fn parse_sql_query(sql: &str) -> Result<JetSqlQuery, String> {
    let mut tokens = sql_tokens(sql.trim())?;
    if tokens.last().is_some_and(|token| token == ";") {
        tokens.pop();
    }
    if tokens.is_empty() {
        return Err(sql_error("SQL query is empty"));
    }
    if !tokens[0].eq_ignore_ascii_case("select") {
        return Err(sql_error("SQL query needs a SELECT statement"));
    }
    let mut cursor = 1;
    let mut projection = Vec::new();
    loop {
        projection.push(parse_select_item(&tokens, &mut cursor)?);
        if tokens.get(cursor).map(String::as_str) == Some(",") {
            cursor += 1;
            if tokens.get(cursor).map(String::as_str) == Some("from") {
                return Err(sql_error("SQL SELECT has a trailing comma"));
            }
            continue;
        }
        break;
    }
    if !tokens
        .get(cursor)
        .is_some_and(|token| token.eq_ignore_ascii_case("from"))
    {
        return Err(sql_error("SQL query needs `FROM source` after its projection"));
    }
    cursor += 1;
    let source = identifier(tokens.get(cursor), "source after FROM")?;
    cursor += 1;
    let mut query = JetSqlQuery {
        source,
        projection,
        group_by: Vec::new(),
        filter: None,
        order: None,
        limit: None,
    };
    let mut stage = 0_u8;
    while cursor < tokens.len() {
        let keyword = tokens[cursor].as_str();
        if keyword.eq_ignore_ascii_case("where") {
            if stage > 1 || query.filter.is_some() {
                return Err(sql_error("SQL query has an out-of-order or repeated WHERE clause"));
            }
            stage = 1;
            let field = identifier(tokens.get(cursor + 1), "WHERE field")?;
            let operator = tokens
                .get(cursor + 2)
                .ok_or_else(|| sql_error("SQL WHERE needs `field operator value`"))?
                .clone();
            if !matches!(operator.as_str(), "=" | "!=" | ">" | ">=" | "<" | "<=") {
                return Err(sql_error(
                    "SQL WHERE operator must be one of `=`, `!=`, `>`, `>=`, `<`, or `<=`",
                ));
            }
            let value = tokens
                .get(cursor + 3)
                .cloned()
                .ok_or_else(|| sql_error("SQL WHERE needs `field operator value`"))?;
            if clause_keyword(&value) || value == "," || value == ")" {
                return Err(sql_error("SQL WHERE needs a literal value"));
            }
            query.filter = Some((field, operator, value));
            cursor += 4;
            continue;
        }
        if keyword.eq_ignore_ascii_case("group") {
            if stage > 2 || !query.group_by.is_empty() {
                return Err(sql_error("SQL query has an out-of-order or repeated GROUP BY clause"));
            }
            if !tokens
                .get(cursor + 1)
                .is_some_and(|token| token.eq_ignore_ascii_case("by"))
            {
                return Err(sql_error("SQL GROUP BY needs `GROUP BY field[, field]`"));
            }
            stage = 2;
            cursor += 2;
            loop {
                let token = tokens
                    .get(cursor)
                    .ok_or_else(|| sql_error("SQL GROUP BY needs a field or ordinal"))?;
                if clause_keyword(token) || token == "," {
                    return Err(sql_error("SQL GROUP BY needs a field or ordinal"));
                }
                let key = if let Ok(ordinal) = token.parse::<usize>() {
                    if ordinal == 0 {
                        return Err(sql_error("SQL GROUP BY ordinals start at one"));
                    }
                    cursor += 1;
                    JetSqlGroupKey::Ordinal(ordinal)
                } else {
                    let field = identifier(Some(token), "GROUP BY field")?;
                    cursor += 1;
                    JetSqlGroupKey::Field(field)
                };
                query.group_by.push(key);
                if tokens.get(cursor).map(String::as_str) != Some(",") {
                    break;
                }
                cursor += 1;
            }
            continue;
        }
        if keyword.eq_ignore_ascii_case("order") {
            if stage > 3 || query.order.is_some() {
                return Err(sql_error("SQL query has an out-of-order or repeated ORDER BY clause"));
            }
            if !tokens
                .get(cursor + 1)
                .is_some_and(|token| token.eq_ignore_ascii_case("by"))
            {
                return Err(sql_error("SQL ORDER BY needs `ORDER BY field [ASC|DESC]`"));
            }
            stage = 3;
            let field = tokens
                .get(cursor + 2)
                .ok_or_else(|| sql_error("SQL ORDER BY needs a field"))?;
            let field = if field.parse::<usize>().is_ok() {
                field.clone()
            } else {
                identifier(Some(field), "ORDER BY field")?
            };
            let descending = match tokens.get(cursor + 3) {
                None => false,
                Some(token) if token.eq_ignore_ascii_case("asc") => false,
                Some(token) if token.eq_ignore_ascii_case("desc") => true,
                Some(token) if clause_keyword(token) => false,
                Some(_) => {
                    return Err(sql_error("SQL ORDER BY direction must be `ASC` or `DESC`"));
                }
            };
            query.order = Some((field, descending));
            cursor += if tokens
                .get(cursor + 3)
                .is_some_and(|token| token.eq_ignore_ascii_case("asc") || token.eq_ignore_ascii_case("desc"))
            {
                4
            } else {
                3
            };
            continue;
        }
        if keyword.eq_ignore_ascii_case("limit") {
            if stage > 4 || query.limit.is_some() {
                return Err(sql_error("SQL query has an out-of-order or repeated LIMIT clause"));
            }
            stage = 4;
            let value = tokens
                .get(cursor + 1)
                .ok_or_else(|| sql_error("SQL LIMIT needs a non-negative integer"))?;
            let value = value
                .parse::<usize>()
                .map_err(|_| sql_error("SQL LIMIT needs a non-negative integer"))?;
            query.limit = Some(value);
            cursor += 2;
            continue;
        }
        if keyword == ";" {
            return Err(sql_error("SQL statement has more than one terminator"));
        }
        return Err(sql_error(format!(
            "unsupported SQL clause `{keyword}`; accepted clauses are WHERE, GROUP BY, ORDER BY, and LIMIT"
        )));
    }
    if let Some(error) = query_shape_error(&query) {
        return Err(error);
    }
    Ok(query)
}

fn sql_compare_values(left: &JetSqlValue, right: &JetSqlValue) -> std::cmp::Ordering {
    match (left, right) {
        (JetSqlValue::Null, JetSqlValue::Null) => std::cmp::Ordering::Equal,
        (JetSqlValue::Null, _) => std::cmp::Ordering::Less,
        (_, JetSqlValue::Null) => std::cmp::Ordering::Greater,
        (left, right) if left.number().is_some() && right.number().is_some() => left
            .number()
            .and_then(|left| right.number().and_then(|right| left.partial_cmp(&right)))
            .unwrap_or(std::cmp::Ordering::Equal),
        (JetSqlValue::Bool(left), JetSqlValue::Bool(right)) => left.cmp(right),
        _ => left.display_text().cmp(&right.display_text()),
    }
}

fn sql_compare(left: &JetSqlValue, operator: &str, right: &JetSqlValue) -> bool {
    if matches!(left, JetSqlValue::Null) || matches!(right, JetSqlValue::Null) {
        return false;
    }
    let ordering = sql_compare_values(left, right);
    match operator {
        "=" => ordering == std::cmp::Ordering::Equal,
        "!=" => ordering != std::cmp::Ordering::Equal,
        ">" => ordering == std::cmp::Ordering::Greater,
        ">=" => ordering != std::cmp::Ordering::Less,
        "<" => ordering == std::cmp::Ordering::Less,
        "<=" => ordering != std::cmp::Ordering::Greater,
        _ => false,
    }
}

/// Compare one textual cell with a SQL literal. Numeric-looking cells use
/// numeric ordering only when the literal is numeric; all other values use
/// deterministic lexical ordering.
pub fn compare_sql_cell(left: &str, operator: &str, right: &str) -> bool {
    let right = JetSqlValue::literal(right);
    let left = match &right {
        JetSqlValue::Int(_) => left
            .parse::<i64>()
            .map(JetSqlValue::Int)
            .unwrap_or_else(|_| JetSqlValue::Text(left.to_string())),
        JetSqlValue::Float(_) => left
            .parse::<f64>()
            .map(JetSqlValue::Float)
            .unwrap_or_else(|_| JetSqlValue::Text(left.to_string())),
        JetSqlValue::Bool(_) => left
            .parse::<bool>()
            .map(JetSqlValue::Bool)
            .unwrap_or_else(|_| JetSqlValue::Text(left.to_string())),
        JetSqlValue::Null | JetSqlValue::Text(_) => JetSqlValue::Text(left.to_string()),
    };
    sql_compare(&left, operator, &right)
}

fn sql_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_finite() {
        return value.to_string();
    }
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_sign_positive() {
        "inf".to_string()
    } else {
        "-inf".to_string()
    }
}

fn sql_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('=', "\\=")
}

fn sql_group_key<F>(
    group: &[usize],
    query: &JetSqlQuery,
    key: &JetSqlGroupKey,
    cell: &mut F,
) -> Result<String, String>
where
    F: FnMut(usize, &str) -> Option<JetSqlValue>,
{
    let field = group_field(query, key)?;
    let value = group
        .first()
        .and_then(|index| cell(*index, &field))
        .unwrap_or(JetSqlValue::Null);
    Ok(format!("{field}={}", value.canonical_key()))
}

fn sql_aggregate_value<F>(
    group: &[usize],
    function: &JetSqlAggregate,
    field: Option<&str>,
    cell: &mut F,
) -> JetSqlValue
where
    F: FnMut(usize, &str) -> Option<JetSqlValue>,
{
    if *function == JetSqlAggregate::Count && field.is_none() {
        return JetSqlValue::Int(group.len() as i64);
    }
    let mut count = 0_i64;
    let mut integer_sum = 0_i128;
    let mut float_sum = 0.0_f64;
    let mut all_int = true;
    let mut selected: Option<JetSqlValue> = None;
    for index in group {
        let Some(value) = field.and_then(|field| cell(*index, field)) else {
            continue;
        };
        if matches!(value, JetSqlValue::Null) {
            continue;
        }
        if *function == JetSqlAggregate::Count {
            count = count.saturating_add(1);
            continue;
        }
        if let Some(number) = value.number() {
            if !number.is_finite() {
                continue;
            }
            count = count.saturating_add(1);
            match &value {
                JetSqlValue::Int(value) => {
                    integer_sum = integer_sum.saturating_add(i128::from(*value));
                    float_sum += *value as f64;
                }
                JetSqlValue::Float(value) => {
                    all_int = false;
                    float_sum += *value;
                }
                JetSqlValue::Null | JetSqlValue::Bool(_) | JetSqlValue::Text(_) => {}
            }
            if matches!(function, JetSqlAggregate::Min | JetSqlAggregate::Max) {
                selected = Some(match selected {
                    None => value,
                    Some(current) => {
                        let ordering = sql_compare_values(&value, &current);
                        if (*function == JetSqlAggregate::Min
                            && ordering == std::cmp::Ordering::Less)
                            || (*function == JetSqlAggregate::Max
                                && ordering == std::cmp::Ordering::Greater)
                        {
                            value
                        } else {
                            current
                        }
                    }
                });
            }
        } else if matches!(function, JetSqlAggregate::Min | JetSqlAggregate::Max) {
            count = count.saturating_add(1);
            selected = Some(match selected {
                None => value,
                Some(current) => {
                    let ordering = sql_compare_values(&value, &current);
                    if (*function == JetSqlAggregate::Min
                        && ordering == std::cmp::Ordering::Less)
                        || (*function == JetSqlAggregate::Max
                            && ordering == std::cmp::Ordering::Greater)
                    {
                        value
                    } else {
                        current
                    }
                }
            });
        }
    }
    if *function == JetSqlAggregate::Count {
        return JetSqlValue::Int(count);
    }
    if count == 0 {
        return JetSqlValue::Null;
    }
    match function {
        JetSqlAggregate::Sum if all_int => i64::try_from(integer_sum)
            .map(JetSqlValue::Int)
            .unwrap_or_else(|_| JetSqlValue::Float(float_sum)),
        JetSqlAggregate::Sum => JetSqlValue::Float(float_sum),
        JetSqlAggregate::Avg => JetSqlValue::Float(float_sum / count as f64),
        JetSqlAggregate::Min | JetSqlAggregate::Max => selected.unwrap_or(JetSqlValue::Null),
        JetSqlAggregate::Count => JetSqlValue::Int(count),
    }
}

fn sql_project_group<F>(
    fields: &[String],
    query: &JetSqlQuery,
    group: &[usize],
    cell: &mut F,
) -> JetSqlResultRow
where
    F: FnMut(usize, &str) -> Option<JetSqlValue>,
{
    let first = group.first().copied();
    let mut projected = Vec::new();
    for item in &query.projection {
        match item {
            JetSqlSelectItem::Wildcard => {
                if let Some(index) = first {
                    projected.extend(fields.iter().map(|field| {
                        (
                            field.clone(),
                            cell(index, field).unwrap_or(JetSqlValue::Null),
                        )
                    }));
                } else {
                    projected.extend(
                        fields
                            .iter()
                            .map(|field| (field.clone(), JetSqlValue::Null)),
                    );
                }
            }
            JetSqlSelectItem::Field { field, alias } => projected.push((
                alias.clone().unwrap_or_else(|| field.clone()),
                first
                    .and_then(|index| cell(index, field))
                    .unwrap_or(JetSqlValue::Null),
            )),
            JetSqlSelectItem::Aggregate {
                function,
                field,
                // `item.output_name()` below applies an aggregate alias.
                alias: _,
            } => projected.push((
                item.output_name(),
                sql_aggregate_value(group, function, field.as_deref(), cell),
            )),
        }
    }
    JetSqlResultRow {
        fields: projected,
        source_indices: group.to_vec(),
    }
}

fn sql_output_value(row: &JetSqlResultRow, field: &str) -> Option<JetSqlValue> {
    if let Ok(ordinal) = field.parse::<usize>() {
        return ordinal
            .checked_sub(1)
            .and_then(|index| row.fields.get(index))
            .map(|(_, value)| value.clone());
    }
    row.fields
        .iter()
        .find(|(name, _)| name == field)
        .map(|(_, value)| value.clone())
}

/// Execute the checked query against a caller-owned row carrier. Projection,
/// grouping, aggregation, ordering, and limiting all live here; tier adapters
/// only supply typed cell access and render the returned scalar rows.
pub fn execute_sql_rows<F>(
    fields: &[String],
    row_count: usize,
    query: &JetSqlQuery,
    mut cell: F,
) -> Result<Vec<JetSqlResultRow>, String>
where
    F: FnMut(usize, &str) -> Option<JetSqlValue>,
{
    let mut selected = Vec::new();
    for index in 0..row_count {
        let keep = query.filter.as_ref().is_none_or(|(field, operator, value)| {
            cell(index, field)
                .is_some_and(|left| sql_compare(&left, operator, &JetSqlValue::literal(value)))
        });
        if keep {
            selected.push(index);
        }
    }
    let grouped = if query.has_aggregate() || !query.group_by.is_empty() {
        if query.group_by.is_empty() {
            vec![selected]
        } else {
            let mut groups = BTreeMap::<String, Vec<usize>>::new();
            for index in selected {
                let one = vec![index];
                let key = query
                    .group_by
                    .iter()
                    .map(|group| sql_group_key(&one, query, group, &mut cell))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("|");
                groups.entry(key).or_default().push(index);
            }
            groups.into_values().collect()
        }
    } else {
        selected.into_iter().map(|index| vec![index]).collect()
    };
    let mut projected = grouped
        .into_iter()
        .map(|group| {
            let row = sql_project_group(fields, query, &group, &mut cell);
            let order = query.order.as_ref().and_then(|(field, _)| {
                sql_output_value(&row, field).or_else(|| {
                    group
                        .first()
                        .and_then(|index| cell(*index, field))
                })
            });
            (row, order)
        })
        .collect::<Vec<_>>();
    if projected.iter().any(|(row, _)| {
        row.fields.iter().any(|(_, value)| {
            matches!(value, JetSqlValue::Float(number) if !number.is_finite())
        })
    }) {
        return Err(sql_error("SQL query produced a non-finite numeric value"));
    }
    if let Some((_, descending)) = &query.order {
        projected.sort_by(|(left, left_order), (right, right_order)| {
            let value_ordering = sql_compare_values(
                left_order.as_ref().unwrap_or(&JetSqlValue::Null),
                right_order.as_ref().unwrap_or(&JetSqlValue::Null),
            );
            let ordering = if *descending {
                value_ordering.reverse()
            } else {
                value_ordering
            };
            if ordering == std::cmp::Ordering::Equal {
                left.source_indices.cmp(&right.source_indices)
            } else {
                ordering
            }
        });
    }
    if let Some(limit) = query.limit {
        projected.truncate(limit);
    }
    Ok(projected.into_iter().map(|(row, _)| row).collect())
}

/// Apply WHERE, ORDER BY, and LIMIT to row positions. The callback is the
/// caller's typed-tree lookup; it is the only value marshalling at this seam.
/// Projection and grouping are intentionally not collapsed into source
/// positions; callers needing those results use `execute_sql_rows`.
pub fn select_sql_indices<F>(row_count: usize, query: &JetSqlQuery, mut cell: F) -> Vec<usize>
where
    F: FnMut(usize, &str) -> Option<String>,
{
    let mut selected = (0..row_count)
        .filter(|index| {
            let Some((field, operator, value)) = &query.filter else {
                return true;
            };
            cell(*index, field)
                .is_some_and(|left| compare_sql_cell(&left, operator, value))
        })
        .collect::<Vec<_>>();
    if let Some((field, descending)) = &query.order {
        selected.sort_by(|left, right| {
            let left_value = cell(*left, field).unwrap_or_default();
            let right_value = cell(*right, field).unwrap_or_default();
            let ordering = compare_sql_order_values(&left_value, &right_value)
                .then_with(|| left.cmp(right));
            if *descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    if let Some(limit) = query.limit {
        selected.truncate(limit);
    }
    selected
}

fn compare_sql_order_values(left: &str, right: &str) -> std::cmp::Ordering {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => left.cmp(right),
    }
}
