// ABI projections over existing BrowserTest.rs report operations; replaced by Jet L6/L7, dies at cutover.
pub(crate) fn jet_browser_report_options_raw(report: &JetBrowserTestReport) -> (String, bool) {
    (report.reporter.clone(), report.ui)
}
pub(crate) fn jet_browser_report_write_bytes_raw(path: &String, bytes: &Vec<u8>) -> Result<(), JetBrowserError> {
    jet_browser_test_write_bytes(std::path::Path::new(path), bytes.as_slice())
}
