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
) -> Vec<jet_std::DataJoin<T, Option<U>>>
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
                        right: Some(right_row.clone()),
                    });
                }
            }
            None => joined.push(jet_std::DataJoin {
                left: left_row,
                right: None,
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

fn jet_data_rolling_mean(values: &Vec<f64>, width: i64) -> Vec<f64> {
    let width = width.max(1) as usize;
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_add(1).saturating_sub(width);
        let window = &values[start..=i];
        out.push(window.iter().sum::<f64>() / window.len() as f64);
    }
    out
}

fn jet_data_status() -> Vec<jet_std::DataStatus> {
    vec![
        jet_std::DataStatus {
            step: "core.data.csv".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "core.data.stats".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "core.data.table".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "core.data.lazy".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "core.data.missing".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "core.data.schema".to_string(),
            path: "native".to_string(),
            replacement: "native".to_string(),
        },
        jet_std::DataStatus {
            step: "py.* / r.* / gpu.*".to_string(),
            path: "bridge-ready".to_string(),
            replacement: "report via data.status() and jet inspect dossier data".to_string(),
        },
    ]
}

fn jet_data_bar_text(groups: &Vec<jet_std::DataGroup>) -> String {
    let mut lines = Vec::new();
    for g in groups {
        let n = if g.count < 0 { 0 } else { g.count.min(40) } as usize;
        lines.push(format!("{} | {} {}", g.key, "#".repeat(n), g.count));
    }
    lines.join("\n")
}

fn jet_data_bar_svg(groups: &Vec<jet_std::DataGroup>) -> String {
    let width = 320.0f64;
    let row_h = 24.0f64;
    let height = 24.0 + row_h * groups.len() as f64;
    let max = groups.iter().map(|g| g.count).max().unwrap_or(1).max(1) as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"{}\" viewBox=\"0 0 320 {}\">",
        height as i64,
        height as i64
    );
    out.push_str("<rect width=\"320\" height=\"100%\" fill=\"white\"/>");
    for (i, g) in groups.iter().enumerate() {
        let y = 18.0 + i as f64 * row_h;
        let bar_w = ((g.count as f64 / max) * (width - 120.0)).round();
        out.push_str(&format!(
            "<text x=\"8\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            y as i64,
            jet_data_svg_escape(&g.key)
        ));
        out.push_str(&format!(
            "<rect x=\"96\" y=\"{}\" width=\"{}\" height=\"14\" fill=\"#2f6f73\"/>",
            (y - 12.0) as i64,
            bar_w as i64
        ));
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"monospace\" font-size=\"12\">{}</text>",
            (104.0 + bar_w) as i64,
            y as i64,
            g.count
        ));
    }
    out.push_str("</svg>");
    out
}

fn jet_fmt_number(value: i64) -> String {
    comma_int(value)
}

fn jet_fmt_decimal(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal(format!("{:.*}", precision, value))
}

fn jet_fmt_percent(value: f64, precision: i64) -> String {
    format!("{}%", jet_fmt_decimal(value * 100.0, precision))
}

fn jet_fmt_bytes(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let mut size = (value as f64).abs();
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut unit = 0usize;
    while size >= 1000.0 && unit + 1 < units.len() {
        size /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{} {}", sign, size as i64, units[unit])
    } else if size >= 10.0 {
        format!("{}{} {}", sign, size.round() as i64, units[unit])
    } else {
        let shown = format!("{:.1}", size);
        format!("{}{} {}", sign, shown.trim_end_matches(".0"), units[unit])
    }
}

fn jet_fmt_duration(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let mut rest = ms.abs();
    if rest < 1000 {
        return format!("{}{}ms", sign, rest);
    }
    let days = rest / 86_400_000;
    rest %= 86_400_000;
    let hours = rest / 3_600_000;
    rest %= 3_600_000;
    let minutes = rest / 60_000;
    rest %= 60_000;
    let seconds = rest / 1000;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    format!("{}{}", sign, parts.into_iter().take(3).collect::<Vec<_>>().join(" "))
}

fn jet_fmt_ordinal(value: i64) -> String {
    let n = value.abs();
    let suffix = if (11..=13).contains(&(n % 100)) {
        "th"
    } else {
        match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{}{}", comma_int(value), suffix)
}

fn jet_fmt_plural(count: i64, singular: &String, plural: &String) -> String {
    let word = if count.abs() == 1 { singular } else { plural };
    format!("{} {}", comma_int(count), word)
}

fn jet_fmt_pad_left(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    format!("{}{}", pad_fill(fill, need), text)
}

fn jet_fmt_pad_right(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    format!("{}{}", text, pad_fill(fill, need))
}

fn jet_fmt_pad_center(text: &String, width: i64, fill: &String) -> String {
    let need = pad_need(text, width);
    let left = need / 2;
    let right = need - left;
    format!("{}{}{}", pad_fill(fill, left), text, pad_fill(fill, right))
}

fn pad_need(text: &str, width: i64) -> usize {
    let width = width.max(0) as usize;
    width.saturating_sub(text.chars().count())
}

fn pad_fill(fill: &str, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let fill = if fill.is_empty() { " " } else { fill };
    let mut out = String::new();
    while out.chars().count() < len {
        out.push_str(fill);
    }
    out.chars().take(len).collect()
}

fn comma_int(value: i64) -> String {
    let raw = value.abs().to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut text: String = out.chars().rev().collect();
    if value < 0 {
        text.insert(0, '-');
    }
    text
}

fn comma_decimal(raw: String) -> String {
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    let whole_value = whole.parse::<i64>().unwrap_or(0);
    let whole_text = comma_int(whole_value);
    match frac {
        Some(frac) => format!("{}{}.{}", sign, whole_text, frac),
        None => format!("{}{}", sign, whole_text),
    }
}

fn jet_data_svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// D-MIGRATE3=A: traced sibling of `jet_enc_csv_decode` — see json's for the shape.
fn jet_enc_csv_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<Vec<T>>, jet_std::DecodeError> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::DecodeError::new)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else {
        return Ok(jet_std::DecodeResult {
            value: Vec::new(),
            migration: jet_std::MigrationStatus::fresh(),
        });
    };
    let mut value = Vec::new();
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
        let (v, m) = T::jet_decode_traced(&tree)
            .map_err(|e| jet_std::DecodeError::under(&format!("row {}", i + 1), e))?;
        if m.migrated && !migration.migrated {
            migration = m;
        }
        value.push(v);
    }
    Ok(jet_std::DecodeResult { value, migration })
}

// CSV typed encode: `[T]` → header row (field names from the first row's Object)
// + one record per element. Requires every element to encode to a flat Object.
fn jet_enc_csv_to_string<T: user_Encode>(values: &Vec<T>) -> String {
    let trees: Vec<jet_std::DataTree> = values.iter().map(|v| v.jet_encode()).collect();
    let mut header: Vec<String> = Vec::new();
    if let Some(jet_std::DataTree::Object(entries)) = trees.first() {
        header = entries.iter().map(|(k, _)| k.clone()).collect();
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(header.clone());
    for tree in &trees {
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
fn jet_std_toml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::toml::parse_to_tree(text).map_err(|e| jet_std::JsonError {
        line: e.line as i64,
        message: e.message,
    })
}
fn jet_std_toml_render(d: &jet_std::DataTree) -> String {
    jet_std::toml::render(d)
}

fn jet_enc_toml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    Ok(T::jet_decode_traced(&tree)?.0)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_toml_decode` — see json's for the shape.
fn jet_enc_toml_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let tree = jet_std::toml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid TOML (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&tree)?;
    Ok(jet_std::DecodeResult { value, migration })
}

// YAML typed decode: parse flat scalars into a DataTree::Object of Text, then decode.
// D-ENC-DYN1=A+ / D-ENC-YAML1 (c152): YAML is a full serde adapter over the one
// rich `DataTree` — block + flow maps/sequences, typed core scalars, block scalars,
// comments, documents, anchors/aliases. parse → `Data`; decode<T> → typed tree.
fn jet_std_yaml_parse(text: &String) -> Result<jet_std::DataTree, jet_std::JsonError> {
    jet_std::yaml::parse_to_tree(text).map_err(|e| jet_std::JsonError {
        line: e.line as i64,
        message: e.message,
    })
}
fn jet_std_yaml_render(d: &jet_std::DataTree) -> String {
    jet_std::yaml::render(d)
}

fn jet_enc_yaml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    // D-MIGRATE4: plain decode walks the migration chain silently (see json's).
    Ok(T::jet_decode_traced(&tree)?.0)
}

// D-MIGRATE3=A: traced sibling of `jet_enc_yaml_decode` — see json's for the shape.
fn jet_enc_yaml_decode_traced<T: user_Decode>(
    text: &String,
) -> Result<jet_std::DecodeResult<T>, jet_std::DecodeError> {
    let tree = jet_std::yaml::parse_to_tree(text).map_err(|e| {
        jet_std::DecodeError::new(format!("invalid YAML (line {}): {}", e.line, e.message))
    })?;
    let (value, migration) = T::jet_decode_traced(&tree)?;
    Ok(jet_std::DecodeResult { value, migration })
}
fn jet_enc_toml_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::toml::render(&v.jet_encode())
}
fn jet_enc_yaml_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::yaml::render(&v.jet_encode())
}
