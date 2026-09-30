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

// D-SHAPE-CTORVERB1=C: generic TTL uses ExpiringValue.new. The checked MIR
// route passes the TTL and the Clock by reference.
fn jet_expiring_new<T: Clone>(
    value: T,
    ttl: &jet_std::Duration,
    clock: &jet_std::Clock,
) -> JetExpiring<T> {
    JetExpiring::new(value, clock.now().saturating_add(ttl.as_millis()))
}
fn jet_expiring_get<T: Clone>(exp: &JetExpiring<T>, clock: &jet_std::Clock) -> Result<T, JetExpired> {
    exp.get(clock.now())
}
// D-TTLVAL1=A: the secret wrapper observes the caller's Clock, so later ticks
// on that clock expire it.
fn jet_expiring_secret_new<T>(
    value: T,
    ttl: &jet_std::Duration,
    clock: &jet_std::Clock,
) -> JetExpiringSecret<T> {
    let observer = clock.observer();
    JetExpiringSecret::new(value, ttl.as_millis(), move || observer.now())
}
fn jet_expiring_secret_with<T, F, R>(exp: &JetExpiringSecret<T>, callback: F) -> Result<R, JetExpired>
where
    F: FnOnce(&T) -> R,
{
    exp.with(callback)
}
