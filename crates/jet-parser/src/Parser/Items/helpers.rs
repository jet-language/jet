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
