// ── E2-M9: First-party ring libraries ────────────────────────────────────────
// Pure-Rust, zero external crates (I6). CSV, TOML, YAML, log, time, crypto.

// ── jet.csv ───────────────────────────────────────────────────────────────────
fn jet_ring_csv_records(
    text: &String,
    delimiter: &String,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<jet_csv_kernel::CsvRecord>, String> {
    jet_csv_kernel::parse(text, jet_csv_options(delimiter, header, skip_blank)?)
}

fn jet_ring_csv_parse(
    text: &String,
    delimiter: &String,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<Vec<String>>, String> {
    jet_ring_csv_records(text, delimiter, header, skip_blank)
        .map(|records| records.into_iter().map(|record| record.fields).collect())
}

fn jet_ring_csv_rows(
    text: &String,
    delimiter: &String,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<jet_std::CSVRow>, String> {
    jet_ring_csv_records(text, delimiter, header, skip_blank).map(|records| {
        records
            .into_iter()
            .map(|record| jet_std::CSVRow {
                fields: record.fields,
                line: record.line,
            })
            .collect()
    })
}

fn jet_ring_csv_render(rows: &Vec<Vec<String>>) -> String {
    jet_csv_kernel::render(rows)
}

// D-SHAPE-CTORVERB1=C: generic TTL uses ExpiringValue.new.
fn jet_expiring_new<T: Clone>(value: T, ttl_ms: i64, clock_now: i64) -> JetExpiring<T> {
    JetExpiring::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_expiring_get<T: Clone>(exp: &JetExpiring<T>, now_ms: i64) -> Result<T, JetExpired> {
    exp.get(now_ms)
}
