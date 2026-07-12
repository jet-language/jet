/// U11: render one `Float`-lexed version segment (`major.minor`, e.g. `1.4`,
/// `1.0`) back to text. The lexer only ever produces this token from
/// `digits '.' digits`, so it always has a decimal point — but
/// `f64::to_string()` drops it for a whole-number float (`1.0.to_string()`
/// is `"1"`), which would silently turn `use pkg#1.0;` into `1` (a different,
/// wrong version). Force the point back on; the one still-documented edge
/// case is a trailing zero merged into the fraction (`1.10` vs `1.1` are
/// indistinguishable once lexed — see `Parser::inline_version`).
pub(super) fn format_version_segment(f: f64) -> String {
    let s = f.to_string();
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

pub(super) fn parse_invariant_bounds(text: &str) -> Option<(i64, i64)> {
    let mut lo = i64::MIN;
    let mut hi = i64::MAX;
    let mut saw = false;
    for raw in text.split("&&") {
        let clause = raw.trim();
        if clause.is_empty() {
            return None;
        }
        if let Some((new_lo, new_hi)) = parse_invariant_clause(clause) {
            lo = lo.max(new_lo);
            hi = hi.min(new_hi);
            saw = true;
        } else {
            return None;
        }
    }
    if saw && lo != i64::MIN && hi != i64::MAX {
        Some((lo, hi))
    } else {
        None
    }
}

fn parse_invariant_clause(clause: &str) -> Option<(i64, i64)> {
    for op in ["<=", ">=", "==", "<", ">"] {
        if let Some((left, right)) = clause.split_once(op) {
            let left = left.trim();
            let right = right.trim();
            return match (left == "value", right == "value") {
                (true, false) => {
                    let n = right.parse::<i64>().ok()?;
                    match op {
                        ">=" => Some((n, i64::MAX)),
                        ">" => n.checked_add(1).map(|v| (v, i64::MAX)),
                        "<=" => Some((i64::MIN, n)),
                        "<" => n.checked_sub(1).map(|v| (i64::MIN, v)),
                        "==" => Some((n, n)),
                        _ => None,
                    }
                }
                (false, true) => {
                    let n = left.parse::<i64>().ok()?;
                    match op {
                        "<=" => Some((n, i64::MAX)),
                        "<" => n.checked_add(1).map(|v| (v, i64::MAX)),
                        ">=" => Some((i64::MIN, n)),
                        ">" => n.checked_sub(1).map(|v| (i64::MIN, v)),
                        "==" => Some((n, n)),
                        _ => None,
                    }
                }
                _ => None,
            };
        }
    }
    None
}
