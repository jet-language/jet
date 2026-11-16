use std::collections::BTreeMap;
use std::env;
use std::fs;

fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, String> {
    let bytes = input.as_bytes();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut after_quote = false;
    let mut i = 0;

    while i < bytes.len() {
        let byte = bytes[i];
        if in_quotes {
            if byte == b'"' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    field.push('"');
                    i += 2;
                } else {
                    in_quotes = false;
                    after_quote = true;
                    i += 1;
                }
            } else {
                field.push(byte as char);
                i += 1;
            }
        } else if after_quote {
            match byte {
                b',' => {
                    row.push(std::mem::take(&mut field));
                    after_quote = false;
                    i += 1;
                }
                b'\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    after_quote = false;
                    i += 1;
                }
                b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    after_quote = false;
                    i += 2;
                }
                _ => return Err("characters after closing quote".to_string()),
            }
        } else {
            match byte {
                b'"' if field.is_empty() => {
                    in_quotes = true;
                    i += 1;
                }
                b'"' => return Err("quote inside unquoted field".to_string()),
                b',' => {
                    row.push(std::mem::take(&mut field));
                    i += 1;
                }
                b'\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    i += 1;
                }
                b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    i += 2;
                }
                _ => {
                    field.push(byte as char);
                    i += 1;
                }
            }
        }
    }
    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    if after_quote || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn main() {
    let path = env::args().nth(1).expect("missing input");
    let input = fs::read_to_string(path).expect("read failed");
    let rows = parse_csv(&input).expect("CSV parse failed");
    let mut groups: BTreeMap<String, (i64, f64)> = BTreeMap::new();
    let mut total_count = 0;
    let mut total_sum = 0.0;
    for row in rows.into_iter().skip(1) {
        let amount: f64 = row[3].parse().expect("amount failed");
        if amount <= 0.0 {
            continue;
        }
        let entry = groups.entry(row[1].clone()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += amount;
        total_count += 1;
        total_sum += amount;
    }
    for (region, (count, sum)) in groups {
        println!("{region} n={count} sum={sum:.2}");
    }
    println!("total n={total_count} sum={total_sum:.2}");
}
