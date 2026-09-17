// Lexical path value rules shared by AOT, JIT, and comptime.

fn jet_std_path_lexical_source(path: &String) -> std::borrow::Cow<'_, str> {
    if path.contains('\\') {
        std::borrow::Cow::Owned(path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(path.as_str())
    }
}

#[allow(dead_code)]
pub(crate) fn jet_std_path_join(base: &String, part: &String) -> String {
    let base = jet_std_path_lexical_source(base);
    let part = jet_std_path_lexical_source(part);
    std::path::Path::new(base.as_ref())
        .join(part.as_ref())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn jet_std_path_parent_opt(path: &String) -> Option<String> {
    let source = jet_std_path_lexical_source(path);
    std::path::Path::new(source.as_ref())
        .parent()
        .map(|value| value.to_string_lossy().to_string())
}

pub(crate) fn jet_std_path_extension_opt(path: &String) -> Option<String> {
    let source = jet_std_path_lexical_source(path);
    std::path::Path::new(source.as_ref())
        .extension()
        .map(|value| value.to_string_lossy().to_string())
}

pub(crate) fn jet_std_path_stem_opt(path: &String) -> Option<String> {
    let source = jet_std_path_lexical_source(path);
    std::path::Path::new(source.as_ref())
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
}

pub(crate) fn jet_std_path_normalize(path: &String) -> String {
    let source = jet_std_path_lexical_source(path);
    let source = std::path::Path::new(source.as_ref());
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

/// Compare normalized lexical path components, not filesystem identity. Both
/// slash spellings are separators on every host so the same Path relation is
/// preserved across AOT, JIT, interpreter, and comptime.
pub(crate) fn jet_std_path_is_within(path: &String, base: &String) -> bool {
    fn components(
        path: &std::path::Path,
    ) -> (
        Option<std::ffi::OsString>,
        bool,
        Vec<std::ffi::OsString>,
    ) {
        let mut prefix = None;
        let mut rooted = false;
        let mut parts: Vec<std::ffi::OsString> = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::Prefix(value) => {
                    prefix = Some(value.as_os_str().to_os_string())
                }
                std::path::Component::RootDir => rooted = true,
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if parts.last().is_some_and(|last| {
                        last.as_os_str() != std::ffi::OsStr::new("..")
                    }) {
                        parts.pop();
                    } else if !rooted {
                        parts.push(std::ffi::OsString::from(".."));
                    }
                }
                std::path::Component::Normal(value) => parts.push(value.to_os_string()),
            }
        }
        (prefix, rooted, parts)
    }

    let path_source = jet_std_path_lexical_source(path);
    let base_source = jet_std_path_lexical_source(base);
    let (base_prefix, base_rooted, base_parts) =
        components(std::path::Path::new(base_source.as_ref()));
    let (path_prefix, path_rooted, path_parts) =
        components(std::path::Path::new(path_source.as_ref()));
    let base_is_relative_current = base_prefix.is_none() && !base_rooted && base_parts.is_empty();
    base_prefix == path_prefix
        && base_rooted == path_rooted
        && path_parts.len() >= base_parts.len()
        && path_parts.starts_with(&base_parts)
        && !(base_is_relative_current
            && path_parts
                .first()
                .is_some_and(|part| part.as_os_str() == std::ffi::OsStr::new("..")))
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

/// Validate every existing component of a walk root without following a
/// symlink. Entry enumeration already uses `file_type`, but that protection
/// does not cover a symlink supplied as the root itself (or an ancestor).
fn jet_std_path_validate_walk_root(path: &std::path::Path) -> std::io::Result<()> {
    let source = if path.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        path
    };
    for ancestor in source
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
    {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("filesystem walk path contains symlink: {}", ancestor.display()),
            ));
        }
    }
    let metadata = std::fs::symlink_metadata(source)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("filesystem walk root is not a directory: {}", source.display()),
        ));
    }
    Ok(())
}

pub(crate) fn jet_std_path_walk(path: &String) -> Vec<String> {
    let root = std::path::Path::new(path.as_str());
    if jet_std_path_validate_walk_root(root).is_err() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(path)];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        if jet_std_path_validate_walk_root(&dir).is_err() {
            continue;
        }
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
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                stack.push(path);
            }
        }
    }
    result
}
