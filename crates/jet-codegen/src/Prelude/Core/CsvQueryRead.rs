// Mechanical part of DataQuery.rs's CSV input adapter. The caller supplies
// the bound and owns bound errors, UTF-8 validation and query ordering.

pub fn jet_data_query_read_bounded(path: &str, max: i64) -> Result<jet_std::__QueryRead, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    let mut bytes = Vec::new();
    // Do not touch an oversized file. Source interprets the reported size;
    // this preserves the old metadata-before-read failure order.
    if size <= max as u64 {
        file.take((max as u64).saturating_add(1))
            .read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    }
    Ok(jet_std::__QueryRead { size, bytes })
}
