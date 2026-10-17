// ABI projections over existing filesystem operations; replaced by Jet L7, dies at cutover.
pub(crate) fn jet_browser_path_join_raw(root: &String, path: &String) -> String {
    std::path::Path::new(root).join(path).to_string_lossy().into_owned()
}
pub(crate) fn jet_browser_read_text_raw(path: &String) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
pub(crate) fn jet_browser_is_file_raw(path: &String) -> bool {
    std::path::Path::new(path).is_file()
}
pub(crate) fn jet_browser_file_size_raw(path: &String) -> Option<i64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len() as i64)
}
pub(crate) fn jet_browser_modified_ms_raw(path: &String) -> Option<i64> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(metadata.modified().ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0))
}

pub(crate) fn jet_browser_html_path_raw(path: &String) -> String {
    std::path::Path::new(path).with_extension("html").to_string_lossy().into_owned()
}
