// ── E2-M9: First-party ring libraries ────────────────────────────────────────
// Pure-Rust, zero external crates (I6). CSV, TOML, YAML, log, time, crypto.

// ── jet.csv ───────────────────────────────────────────────────────────────────
fn jet_ring_csv_records(
    text: &str,
    delimiter: &str,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<jet_csv_kernel::CsvRecord>, String> {
    jet_csv_kernel::parse(text, jet_csv_options(delimiter, header, skip_blank)?)
}

fn jet_ring_csv_parse(
    text: &str,
    delimiter: &str,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<Vec<String>>, String> {
    jet_ring_csv_records(text, delimiter, header, skip_blank)
        .map(|records| records.into_iter().map(|record| record.fields).collect())
}

fn jet_ring_csv_rows(
    text: &str,
    delimiter: &str,
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

