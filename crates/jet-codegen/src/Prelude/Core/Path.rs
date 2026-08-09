// D-IO1: lexical path value rules shared by AOT, JIT, and comptime.

pub(crate) fn jet_std_path_join(base: &String, part: &String) -> String {
    std::path::Path::new(base.as_str())
        .join(part.as_str())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn jet_std_path_parent(path: &String) -> String {
    jet_std_path_parent_opt(path).unwrap_or_default()
}

pub(crate) fn jet_std_path_parent_opt(path: &String) -> Option<String> {
    std::path::Path::new(path.as_str())
        .parent()
        .map(|value| value.to_string_lossy().to_string())
}

pub(crate) fn jet_std_path_extension(path: &String) -> String {
    jet_std_path_extension_opt(path).unwrap_or_default()
}

pub(crate) fn jet_std_path_extension_opt(path: &String) -> Option<String> {
    std::path::Path::new(path.as_str())
        .extension()
        .map(|value| value.to_string_lossy().to_string())
}

pub(crate) fn jet_std_path_stem_opt(path: &String) -> Option<String> {
    std::path::Path::new(path.as_str())
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
}

pub(crate) fn jet_std_path_normalize(path: &String) -> String {
    let source = std::path::Path::new(path.as_str());
    let rooted = source.has_root();
    let mut normalized = std::path::PathBuf::new();
    let mut normal_depth = 0usize;
    for component in source.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if normal_depth > 0 => {
                normalized.pop();
                normal_depth -= 1;
            }
            std::path::Component::ParentDir if !rooted => normalized.push(".."),
            std::path::Component::ParentDir => {}
            std::path::Component::Normal(part) => {
                normalized.push(part);
                normal_depth += 1;
            }
        }
    }
    normalized.to_string_lossy().into_owned()
}
