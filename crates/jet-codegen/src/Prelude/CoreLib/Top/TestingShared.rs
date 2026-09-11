// Core testing adapters report I/O failures through the shared typed evidence
// error.  Golden mismatches are ordinary failed assertions, not I/O errors.

pub(crate) fn jet_testing_golden(
    path: &String,
    actual: &String,
) -> Result<bool, TestEvidenceError> {
    match std::fs::read_to_string(path) {
        Ok(expected) => Ok(expected == *actual),
        Err(error) => Err(TestEvidenceError::io(
            "read_golden",
            path,
            error.to_string(),
        )),
    }
}

pub(crate) fn jet_testing_fixture(path: &String) -> Result<String, TestEvidenceError> {
    std::fs::read_to_string(path).map_err(|error| {
        TestEvidenceError::io("read_fixture", path, error.to_string())
    })
}

pub(crate) fn jet_testing_temp_dir_path(
    prefix: &str,
) -> Result<String, TestEvidenceError> {
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
    let path_string = path.to_string_lossy().into_owned();
    match std::fs::remove_dir_all(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(TestEvidenceError::io(
                "remove_temp_dir",
                &path_string,
                error.to_string(),
            ));
        }
    }
    std::fs::create_dir_all(&path).map_err(|error| {
        TestEvidenceError::io("create_temp_dir", &path_string, error.to_string())
    })?;
    Ok(path_string)
}
