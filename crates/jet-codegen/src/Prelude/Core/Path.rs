// D-IO1: lexical path value rules shared by AOT, JIT, and comptime.

#[allow(dead_code)]
pub(crate) fn jet_std_path_join(base: &String, part: &String) -> String {
    std::path::Path::new(base.as_str())
        .join(part.as_str())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn jet_std_path_parent_opt(path: &String) -> Option<String> {
    std::path::Path::new(path.as_str())
        .parent()
        .map(|value| value.to_string_lossy().to_string())
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

pub(crate) fn jet_std_path_home() -> String {
    if cfg!(windows) {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return profile;
        }
        if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
            return format!("{drive}{path}");
        }
        std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        std::env::var("HOME").unwrap_or_else(|_| {
            std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    }
}

pub(crate) fn jet_std_path_walk(path: &String) -> Vec<String> {
    let mut result = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(path)];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        let canonical = match std::fs::canonicalize(&dir) {
            Ok(canonical) => canonical,
            Err(_) => continue,
        };
        if !visited.insert(canonical) {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            result.push(path.to_string_lossy().into_owned());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    result
}
