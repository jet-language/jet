pub(crate) fn jet_testing_temp_dir_path(prefix: &str) -> String {
    let safe: String = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let tid: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let path = std::env::temp_dir().join(format!(
        "jet_test_{}_{}_{}",
        safe,
        std::process::id(),
        tid
    ));
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::create_dir_all(&path);
    path.to_string_lossy().into_owned()
}
