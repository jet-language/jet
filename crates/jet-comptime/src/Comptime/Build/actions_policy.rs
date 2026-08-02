use super::errors_keys::BuildError;
use super::handles::{ActionId, PluginHandle, ProbeHandle, SigningIdentityHandle, ToolchainHandle};
use super::targets::BuildPath;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;

pub type BuildCapability = crate::BuildEffect;

const MAX_LEGACY_PROJECT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_LEGACY_PROJECT_INPUT_FILES: usize = 100_000;
const MAX_LEGACY_PROJECT_INPUT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildResourcePool {
    Cpu,
    Memory,
    Linker,
    Console,
    GPU,
    Custom(String),
}

impl BuildResourcePool {
    pub fn as_str(&self) -> &str {
        match self {
            BuildResourcePool::Cpu => "cpu",
            BuildResourcePool::Memory => "memory",
            BuildResourcePool::Linker => "linker",
            BuildResourcePool::Console => "console",
            BuildResourcePool::GPU => "gpu",
            BuildResourcePool::Custom(name) => name.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildResourcePoolSpec {
    pub pool: BuildResourcePool,
    pub slots: usize,
}

impl BuildResourcePoolSpec {
    pub fn new(pool: BuildResourcePool, slots: usize) -> Self {
        BuildResourcePoolSpec { pool, slots }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyWrapperKind {
    CMake,
    Make,
    Gradle,
    Npm,
    Cargo,
}

impl LegacyWrapperKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LegacyWrapperKind::CMake => "cmake",
            LegacyWrapperKind::Make => "make",
            LegacyWrapperKind::Gradle => "gradle",
            LegacyWrapperKind::Npm => "npm",
            LegacyWrapperKind::Cargo => "cargo",
        }
    }

    pub fn project_file(self) -> &'static str {
        match self {
            LegacyWrapperKind::CMake => "CMakeLists.txt",
            LegacyWrapperKind::Make => "Makefile",
            LegacyWrapperKind::Gradle => "build.gradle",
            LegacyWrapperKind::Npm => "package.json",
            LegacyWrapperKind::Cargo => "Cargo.toml",
        }
    }

    fn default_argv(self) -> Vec<String> {
        match self {
            LegacyWrapperKind::CMake => vec!["cmake".to_string(), "--build".to_string(), "build".to_string()],
            LegacyWrapperKind::Make => vec!["make".to_string()],
            LegacyWrapperKind::Gradle => vec!["gradle".to_string(), "build".to_string()],
            LegacyWrapperKind::Npm => vec!["npm".to_string(), "run".to_string(), "build".to_string()],
            LegacyWrapperKind::Cargo => vec!["cargo".to_string(), "build".to_string()],
        }
    }
}

#[derive(Default)]
struct LegacyProjectImport {
    argv: Option<Vec<String>>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    caps: BTreeSet<BuildCapability>,
    env: BTreeMap<String, String>,
    env_allowlist: BTreeSet<String>,
    cache: Option<ActionCache>,
    action_kind: Option<ActionKind>,
    resource_pools: BTreeSet<BuildResourcePool>,
    labels: BTreeMap<String, String>,
}

fn legacy_import_error(kind: LegacyWrapperKind, detail: impl Into<String>) -> BuildError {
    BuildError::LegacyProjectFileInvalid(format!(
        "{}: {}",
        kind.project_file(),
        detail.into()
    ))
}

fn push_unique(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn split_import_words(value: &str) -> Vec<String> {
    value
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter_map(|part| {
            let part = part.trim_matches(|ch| ch == '"' || ch == '\'');
            (!part.is_empty()).then(|| part.to_string())
        })
        .collect()
}

fn scalar_import_value(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .to_string()
}

fn parse_import_array(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.starts_with('[') && value.ends_with(']') {
        split_import_words(&value[1..value.len() - 1])
    } else {
        split_import_words(value)
    }
}

fn import_resource_pool(value: &str) -> BuildResourcePool {
    match value.to_ascii_lowercase().as_str() {
        "cpu" => BuildResourcePool::Cpu,
        "memory" => BuildResourcePool::Memory,
        "linker" => BuildResourcePool::Linker,
        "console" => BuildResourcePool::Console,
        "gpu" => BuildResourcePool::GPU,
        _ => BuildResourcePool::Custom(value.to_string()),
    }
}

fn import_action_kind(value: &str) -> Option<ActionKind> {
    match value.to_ascii_lowercase().as_str() {
        "compile" => Some(ActionKind::Compile),
        "docs" => Some(ActionKind::Docs),
        "debug" => Some(ActionKind::Debug),
        "source_archive" | "source-archive" => Some(ActionKind::SourceArchive),
        "generic" => Some(ActionKind::Generic),
        _ => None,
    }
}

fn apply_import_key(
    kind: LegacyWrapperKind,
    key: &str,
    value: &str,
    import: &mut LegacyProjectImport,
) -> Result<(), BuildError> {
    match key {
        "argv" | "args" => import.argv = Some(parse_import_array(value)),
        "input" | "inputs" => {
            for path in parse_import_array(value) {
                push_unique(&mut import.inputs, path);
            }
        }
        "output" | "outputs" => {
            for path in parse_import_array(value) {
                push_unique(&mut import.outputs, path);
            }
        }
        "cap" | "caps" => {
            for cap in parse_import_array(value) {
                let Some(cap) = BuildCapability::parse(&cap) else {
                    return Err(legacy_import_error(kind, format!("unknown capability `{cap}`")));
                };
                import.caps.insert(cap);
            }
        }
        "env" => {
            for entry in parse_import_array(value) {
                let Some((name, value)) = entry.split_once('=') else {
                    return Err(legacy_import_error(kind, "environment entries must use KEY=VALUE"));
                };
                if name.trim().is_empty() {
                    return Err(legacy_import_error(kind, "environment names cannot be empty"));
                }
                import.env.insert(name.to_string(), value.to_string());
            }
        }
        "env_allowlist" | "env-allowlist" => {
            import.env_allowlist.extend(parse_import_array(value));
        }
        "pool" | "pools" => {
            for pool in parse_import_array(value) {
                import.resource_pools.insert(import_resource_pool(&pool));
            }
        }
        "cache" => {
            import.cache = match scalar_import_value(value).to_ascii_lowercase().as_str() {
                "cached" => Some(ActionCache::Cached),
                "phony" | "uncached" => Some(ActionCache::UncachedPhony),
                _ => return Err(legacy_import_error(kind, "cache must be cached or phony")),
            };
        }
        "kind" => {
            let value = scalar_import_value(value);
            import.action_kind = import_action_kind(&value);
            if import.action_kind.is_none() {
                return Err(legacy_import_error(kind, format!("unknown action kind `{value}`")));
            }
        }
        key if key.starts_with("label.") => {
            let name = key.trim_start_matches("label.");
            if name.is_empty() {
                return Err(legacy_import_error(kind, "label names cannot be empty"));
            }
            import
                .labels
                .insert(name.to_string(), scalar_import_value(value));
        }
        _ => {
            return Err(legacy_import_error(
                kind,
                format!("unsupported import field `{key}`"),
            ));
        }
    }
    Ok(())
}

fn apply_import_directives(
    kind: LegacyWrapperKind,
    source: &str,
    import: &mut LegacyProjectImport,
) -> Result<(), BuildError> {
    for line in source.lines() {
        let line = line.trim();
        let line = line
            .strip_prefix('#')
            .or_else(|| line.strip_prefix("//"))
            .map(str::trim)
            .unwrap_or(line);
        let Some(line) = line.strip_prefix("jet:").or_else(|| line.strip_prefix("jet.")) else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            return Err(legacy_import_error(kind, "Jet import directives must use key=value"));
        };
        apply_import_key(kind, key.trim(), value.trim(), import)?;
    }
    Ok(())
}

fn legacy_generated_directory(kind: LegacyWrapperKind, name: &str) -> bool {
    name == ".git"
        || name == ".jet"
        || matches!(
            (kind, name),
            (LegacyWrapperKind::CMake | LegacyWrapperKind::Make, "build")
                | (LegacyWrapperKind::Gradle, "build" | ".gradle")
                | (LegacyWrapperKind::Npm, "build" | "dist" | "node_modules")
                | (LegacyWrapperKind::Cargo, "target")
        )
}

fn collect_legacy_project_inputs(
    root: &Path,
    kind: LegacyWrapperKind,
    import: &mut LegacyProjectImport,
) -> Result<(), BuildError> {
    fn walk(
        root: &Path,
        relative: &Path,
        kind: LegacyWrapperKind,
        import: &mut LegacyProjectImport,
        file_count: &mut usize,
        byte_count: &mut u64,
    ) -> Result<(), BuildError> {
        let directory = root.join(relative);
        let mut entries = fs::read_dir(&directory)
            .map_err(|_| legacy_import_error(kind, format!("cannot read `{}`", relative.display())))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| legacy_import_error(kind, format!("cannot read `{}`", relative.display())))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let child_relative = relative.join(&name);
            let metadata = fs::symlink_metadata(entry.path()).map_err(|_| {
                legacy_import_error(kind, format!("cannot inspect `{}`", child_relative.display()))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(legacy_import_error(
                    kind,
                    format!("source closure cannot contain symlink `{}`", child_relative.display()),
                ));
            }
            if metadata.is_dir() {
                if legacy_generated_directory(kind, name.to_string_lossy().as_ref()) {
                    continue;
                }
                walk(
                    root,
                    &child_relative,
                    kind,
                    import,
                    file_count,
                    byte_count,
                )?;
                continue;
            }
            if !metadata.is_file() {
                return Err(legacy_import_error(
                    kind,
                    format!("source closure contains non-file `{}`", child_relative.display()),
                ));
            }
            let child_path = child_relative.to_string_lossy();
            if import
                .outputs
                .iter()
                .any(|output| output.as_str() == child_path.as_ref())
            {
                continue;
            }
            *file_count += 1;
            *byte_count = (*byte_count).saturating_add(metadata.len());
            if *file_count > MAX_LEGACY_PROJECT_INPUT_FILES {
                return Err(legacy_import_error(
                    kind,
                    format!("source closure exceeds {MAX_LEGACY_PROJECT_INPUT_FILES} files"),
                ));
            }
            if *byte_count > MAX_LEGACY_PROJECT_INPUT_BYTES {
                return Err(legacy_import_error(
                    kind,
                    format!("source closure exceeds {MAX_LEGACY_PROJECT_INPUT_BYTES} bytes"),
                ));
            }
            let path = child_relative.to_str().ok_or_else(|| {
                legacy_import_error(kind, "source closure contains a non-UTF-8 path")
            })?;
            push_unique(&mut import.inputs, path);
        }
        Ok(())
    }

    let mut file_count = 0;
    let mut byte_count = 0;
    walk(
        root,
        Path::new(""),
        kind,
        import,
        &mut file_count,
        &mut byte_count,
    )?;
    import.labels.insert(
        "legacy.source-closure".to_string(),
        format!("project-files-v1:{file_count}:{byte_count}"),
    );
    Ok(())
}

fn cmake_command_calls(source: &str) -> Vec<(String, Vec<String>)> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut cursor = 0;
    let mut quote = None;
    while cursor < bytes.len() {
        if let Some(end) = quote {
            if bytes[cursor] == end && (cursor == 0 || bytes[cursor - 1] != b'\\') {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if bytes[cursor] == b'"' || bytes[cursor] == b'\'' {
            quote = Some(bytes[cursor]);
            cursor += 1;
            continue;
        }
        if bytes[cursor] == b'#' {
            cursor = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        if !(bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_') {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
        {
            cursor += 1;
        }
        let name = &source[start..cursor];
        let mut open = cursor;
        while open < bytes.len() && bytes[open].is_ascii_whitespace() {
            open += 1;
        }
        if open >= bytes.len() || bytes[open] != b'(' {
            continue;
        }
        let mut depth = 1usize;
        let mut close = open + 1;
        let mut nested_quote = None;
        while close < bytes.len() && depth > 0 {
            if let Some(end) = nested_quote {
                if bytes[close] == end && (close == 0 || bytes[close - 1] != b'\\') {
                    nested_quote = None;
                }
            } else {
                match bytes[close] {
                    b'"' | b'\'' => nested_quote = Some(bytes[close]),
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
            }
            close += 1;
        }
        if depth != 0 {
            calls.push((name.to_string(), Vec::new()));
            break;
        }
        calls.push((
            name.to_string(),
            split_import_words(&source[open + 1..close - 1]),
        ));
        cursor = close;
    }
    calls
}

fn cmake_commands(source: &str, name: &str) -> Vec<Vec<String>> {
    cmake_command_calls(source)
        .into_iter()
        .filter_map(|(command, args)| command.eq_ignore_ascii_case(name).then_some(args))
        .collect()
}

fn parse_cmake_import(source: &str, import: &mut LegacyProjectImport) -> Result<(), BuildError> {
    let calls = cmake_command_calls(source);
    let allowed = [
        "cmake_minimum_required",
        "project",
        "add_executable",
        "add_library",
        "add_custom_target",
    ];
    for (command, args) in &calls {
        if args.is_empty() {
            return Err(legacy_import_error(
                LegacyWrapperKind::CMake,
                format!("malformed command `{command}`"),
            ));
        }
        if !allowed
            .iter()
            .any(|allowed| command.eq_ignore_ascii_case(allowed))
        {
            return Err(legacy_import_error(
                LegacyWrapperKind::CMake,
                format!("unsupported construct `{command}`"),
            ));
        }
    }
    let mut targets = Vec::new();
    for command in ["add_executable", "add_library", "add_custom_target"] {
        targets.extend(cmake_commands(source, command).into_iter().map(|args| {
            (command.to_string(), args)
        }));
    }
    if targets.len() != 1 {
        return Err(legacy_import_error(
            LegacyWrapperKind::CMake,
            if targets.is_empty() {
                "supported import needs one add_executable, add_library, or add_custom_target"
                    .to_string()
            } else {
                "multiple or ambiguous build targets are not supported".to_string()
            },
        ));
    }
    let (command, args) = targets.pop().expect("one CMake target was checked");
    let Some(name) = args.first().cloned() else {
        return Err(legacy_import_error(
            LegacyWrapperKind::CMake,
            format!("{command} has no target name"),
        ));
    };
    if name.starts_with('$') || name.contains(' ') {
        return Err(legacy_import_error(
            LegacyWrapperKind::CMake,
            "target name must be a literal word",
        ));
    }
    import.argv = Some(vec![
        "cmake".to_string(),
        "--build".to_string(),
        "build".to_string(),
        "--target".to_string(),
        name.clone(),
    ]);
    import
        .labels
        .insert("legacy.target".to_string(), name.clone());
    match command.as_str() {
        "add_custom_target" => {
            if args.iter().skip(1).any(|arg| arg != "ALL") {
                return Err(legacy_import_error(
                    LegacyWrapperKind::CMake,
                    "add_custom_target options and commands are not supported",
                ));
            }
            import.cache = Some(ActionCache::UncachedPhony);
        }
        "add_executable" | "add_library" => {
            let known_flags = [
                "WIN32",
                "MACOSX_BUNDLE",
                "EXCLUDE_FROM_ALL",
                "STATIC",
                "SHARED",
                "MODULE",
                "OBJECT",
                "INTERFACE",
            ];
            for path in args.into_iter().skip(1) {
                if path.starts_with('$') {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::CMake,
                        "target sources must be literal paths",
                    ));
                }
                if known_flags.contains(&path.as_str()) {
                    continue;
                }
                if path == "IMPORTED" || path == "ALIAS" {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::CMake,
                        "imported and alias targets are not buildable project targets",
                    ));
                }
                push_unique(&mut import.inputs, path);
            }
            push_unique(&mut import.outputs, format!("build/{name}"));
        }
        _ => unreachable!("all CMake target commands are handled"),
    }
    Ok(())
}

fn parse_make_import(source: &str, import: &mut LegacyProjectImport) -> Result<(), BuildError> {
    for unsupported in ["$(shell", "$(eval", "include ", "define "] {
        if source.contains(unsupported) {
            return Err(legacy_import_error(
                LegacyWrapperKind::Make,
                format!("unsupported construct `{unsupported}`"),
            ));
        }
    }
    let mut rules = Vec::new();
    let mut phony_targets = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with('\t') {
            return Err(legacy_import_error(
                LegacyWrapperKind::Make,
                "recipe bodies are not representable in the typed import",
            ));
        }
        if let Some((special, targets)) = trimmed.split_once(':') {
            if special == ".PHONY" {
                for target in targets.split_whitespace() {
                    if target.starts_with('$') || target.contains('%') {
                        return Err(legacy_import_error(
                            LegacyWrapperKind::Make,
                            ".PHONY targets must be literal paths",
                        ));
                    }
                    phony_targets.insert(target.to_string());
                }
                continue;
            }
            if special.starts_with('.') {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Make,
                    format!("unsupported special target `{special}`"),
                ));
            }
        }
        let Some((target, prerequisites)) = line.split_once(':') else {
            return Err(legacy_import_error(
                LegacyWrapperKind::Make,
                "only one literal target rule is supported",
            ));
        };
        let target = target.trim();
        if target.is_empty() || target.chars().any(char::is_whitespace) || target.contains('%') {
            return Err(legacy_import_error(
                LegacyWrapperKind::Make,
                "target rules must name one literal target",
            ));
        }
        if prerequisites.contains('|') {
            return Err(legacy_import_error(
                LegacyWrapperKind::Make,
                "order-only prerequisites are not supported",
            ));
        }
        rules.push((target.to_string(), prerequisites.to_string()));
    }
    if rules.len() != 1 {
        return Err(legacy_import_error(
            LegacyWrapperKind::Make,
            if rules.is_empty() {
                "supported import needs one named target rule".to_string()
            } else {
                "multiple or ambiguous target rules are not supported".to_string()
            },
        ));
    }
    let (target, prerequisites) = rules.pop().expect("one Make rule was checked");
    import.argv = Some(vec!["make".to_string(), target.clone()]);
    push_unique(&mut import.outputs, target.clone());
    for path in prerequisites.split_whitespace() {
        if path.starts_with('$') {
            return Err(legacy_import_error(
                LegacyWrapperKind::Make,
                "prerequisites must be literal paths",
            ));
        }
        push_unique(&mut import.inputs, path);
    }
    if phony_targets.iter().any(|phony| phony != &target) {
        return Err(legacy_import_error(
            LegacyWrapperKind::Make,
            ".PHONY must name the imported target only",
        ));
    }
    if phony_targets.contains(&target) {
        import.cache = Some(ActionCache::UncachedPhony);
    }
    import
        .labels
        .insert("legacy.target".to_string(), target);
    Ok(())
}

fn parse_gradle_import(source: &str, import: &mut LegacyProjectImport) -> Result<(), BuildError> {
    let mut tasks = Vec::new();
    let mut project = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let declaration = ["tasks.register(", "tasks.create("]
            .iter()
            .find_map(|marker| line.strip_prefix(marker).map(|rest| (*marker, rest)));
        if let Some((marker, rest)) = declaration {
            let Some(argument) = rest.strip_suffix(')') else {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    format!("unsupported task body after {marker}"),
                ));
            };
            let argument = argument.trim();
            let Some(quote) = argument.chars().next() else {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "task declaration needs one quoted literal name",
                ));
            };
            if !matches!(quote, '\'' | '"')
                || argument.len() < 2
                || !argument.ends_with(quote)
                || argument[1..argument.len() - 1].contains(quote)
            {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "task declaration needs one quoted literal name",
                ));
            }
            tasks.push(argument[1..argument.len() - 1].to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("task ") {
            let mut words = rest.split_whitespace();
            let Some(task) = words.next() else {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "task declaration needs one literal name",
                ));
            };
            if words.next().is_some() {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "task bodies and task options are not representable in the typed import",
                ));
            }
            tasks.push(task.to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("rootProject.name") {
            let Some((_, value)) = rest.split_once('=') else {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "rootProject.name must assign one quoted literal",
                ));
            };
            let value = value.trim();
            let Some(quote) = value.chars().next() else {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "rootProject.name must assign one quoted literal",
                ));
            };
            if !matches!(quote, '\'' | '"')
                || value.len() < 2
                || !value.ends_with(quote)
                || value[1..value.len() - 1].contains(quote)
            {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "rootProject.name must assign one quoted literal",
                ));
            }
            if project.replace(value[1..value.len() - 1].to_string()).is_some() {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Gradle,
                    "rootProject.name must be declared once",
                ));
            }
            continue;
        }
        return Err(legacy_import_error(
            LegacyWrapperKind::Gradle,
            format!("unsupported construct `{line}`"),
        ));
    }
    if tasks.len() != 1 {
        return Err(legacy_import_error(
            LegacyWrapperKind::Gradle,
            if tasks.is_empty() {
                "supported import needs tasks.register, tasks.create, or task".to_string()
            } else {
                "multiple or ambiguous task declarations are not supported".to_string()
            },
        ));
    }
    let task = tasks.pop().expect("one Gradle task was checked");
    if task.is_empty() || task.contains(char::is_whitespace) {
        return Err(legacy_import_error(
            LegacyWrapperKind::Gradle,
            "task name must be one literal word",
        ));
    }
    let project = project;
    if matches!(task.as_str(), "build" | "assemble") && project.is_none() {
        return Err(legacy_import_error(
            LegacyWrapperKind::Gradle,
            "build or assemble import needs rootProject.name",
        ));
    }
    import.argv = Some(vec!["gradle".to_string(), task.clone()]);
    push_unique(
        &mut import.outputs,
        if task == "build" || task == "assemble" {
            format!("build/libs/{}.jar", project.as_deref().unwrap_or_default())
        } else {
            format!("build/{task}")
        },
    );
    import
        .labels
        .insert("legacy.task".to_string(), task);
    Ok(())
}

fn parse_cargo_import(
    root: &Path,
    source: &str,
    import: &mut LegacyProjectImport,
) -> Result<(), BuildError> {
    let mut section = String::new();
    let mut package = None;
    let mut version = None;
    let mut bin_name = None;
    let mut bin_path = None;
    let mut bin_count = 0usize;
    let mut dependencies = Vec::new();
    for line in source.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches('[').trim_matches(']').to_string();
            if !matches!(
                section.as_str(),
                "package" | "bin" | "dependencies" | "package.metadata.jet"
            ) {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Cargo,
                    "unsupported Cargo section",
                ));
            }
            if section == "bin" {
                bin_count += 1;
                if bin_count > 1 {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::Cargo,
                        "multiple binary targets are not supported",
                    ));
                }
            }
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            return Err(legacy_import_error(
                LegacyWrapperKind::Cargo,
                "Cargo fields must use key = value",
            ));
        };
        let key = key.trim();
        let value = scalar_import_value(raw);
        match section.as_str() {
            "package" if key == "build" => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Cargo,
                    "build scripts are not supported by the typed importer",
                ));
            }
            "package" if key == "name" => {
                package = Some(value);
                import
                    .labels
                    .insert("legacy.package.name".to_string(), raw.trim().to_string());
            }
            "package" if key == "version" => {
                version = Some(value);
                import
                    .labels
                    .insert("legacy.package.version".to_string(), raw.trim().to_string());
            }
            "package"
                if matches!(
                    key,
                    "edition"
                        | "rust-version"
                        | "authors"
                        | "description"
                        | "license"
                        | "license-file"
                        | "repository"
                        | "homepage"
                        | "documentation"
                        | "readme"
                        | "keywords"
                        | "categories"
                        | "exclude"
                        | "include"
                        | "publish"
                ) => {
                    import
                        .labels
                        .insert(format!("legacy.package.{key}"), raw.trim().to_string());
                }
            "package" => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Cargo,
                    format!("unsupported [package] field {}", key),
                ));
            }
            "bin" if key == "name" => {
                if bin_name.replace(value).is_some() {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::Cargo,
                        "binary target names must be declared once",
                    ));
                }
                import
                    .labels
                    .insert("legacy.bin.name".to_string(), raw.trim().to_string());
            }
            "bin" if key == "path" => {
                if bin_path.replace(value).is_some() {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::Cargo,
                        "binary target paths must be declared once",
                    ));
                }
                import
                    .labels
                    .insert("legacy.bin.path".to_string(), raw.trim().to_string());
            }
            "bin" if key == "required-features" && value != "[]" => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Cargo,
                    "binary required-features are not supported",
                ));
            }
            "bin" if key == "required-features" => {
                import.labels.insert(
                    "legacy.bin.required-features".to_string(),
                    raw.trim().to_string(),
                );
            }
            "bin" => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Cargo,
                    format!("unsupported [[bin]] field {}", key),
                ));
            }
            "dependencies" => dependencies.push((key.to_string(), raw.trim().to_string())),
            "package.metadata.jet" => apply_import_key(LegacyWrapperKind::Cargo, key, raw, import)?,
            _ => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Cargo,
                    "Cargo field appears outside a supported section",
                ));
            }
        }
    }
    let package = package.ok_or_else(|| {
        legacy_import_error(
            LegacyWrapperKind::Cargo,
            "supported import needs [package].name",
        )
    })?;
    let target = bin_name.unwrap_or_else(|| package.clone());
    import.argv = if bin_count == 0 {
        Some(vec!["cargo".to_string(), "build".to_string()])
    } else {
        Some(vec![
            "cargo".to_string(),
            "build".to_string(),
            "--bin".to_string(),
            target.clone(),
        ])
    };
    push_unique(&mut import.outputs, format!("target/debug/{target}"));
    if let Some(path) = bin_path {
        push_unique(&mut import.inputs, path);
    }
    let has_dependencies = !dependencies.is_empty();
    for (dependency, requirement) in dependencies {
        if requirement.contains("path =")
            || requirement.contains("path=")
            || requirement.contains("git =")
            || requirement.contains("git=")
            || requirement.contains("workspace =")
            || requirement.contains("workspace=")
        {
            return Err(legacy_import_error(
                LegacyWrapperKind::Cargo,
                format!("dependency `{dependency}` uses an unsupported non-registry source"),
            ));
        }
        import.labels.insert(
            format!("legacy.dependency.{dependency}"),
            requirement,
        );
    }
    if let Some(version) = version {
        import.labels.insert("legacy.version".to_string(), version);
    }
    let mut has_lock = false;
    if let Ok(metadata) = fs::symlink_metadata(root.join("Cargo.lock")) {
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            has_lock = true;
            push_unique(&mut import.inputs, "Cargo.lock");
        }
    }
    if has_dependencies && !has_lock {
        return Err(legacy_import_error(
            LegacyWrapperKind::Cargo,
            "dependency closure requires a non-symlink Cargo.lock",
        ));
    }
    Ok(())
}

fn json_string(value: Option<&jet_foundation::JSON::JSONValue>) -> Option<String> {
    match value {
        Some(jet_foundation::JSON::JSONValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn parse_npm_import(
    root: &Path,
    source: &str,
    import: &mut LegacyProjectImport,
) -> Result<(), BuildError> {
    let value = jet_foundation::JSON::parse_json(source)
        .map_err(|_| legacy_import_error(LegacyWrapperKind::Npm, "package.json is not valid JSON"))?;
    let jet_foundation::JSON::JSONValue::Object(object) = &value else {
        return Err(legacy_import_error(
            LegacyWrapperKind::Npm,
            "package.json root must be an object",
        ));
    };
    const SUPPORTED_NPM_FIELDS: &[&str] = &[
        "name",
        "version",
        "scripts",
        "main",
        "module",
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ];
    if let Some(field) = object
        .keys()
        .find(|field| !SUPPORTED_NPM_FIELDS.contains(&field.as_str()))
    {
        return Err(legacy_import_error(
            LegacyWrapperKind::Npm,
            format!("unsupported package.json field `{field}`"),
        ));
    }
    if object.contains_key("name") && json_string(object.get("name")).is_none() {
        return Err(legacy_import_error(
            LegacyWrapperKind::Npm,
            "package name must be a string",
        ));
    }
    if let Some(name) = json_string(object.get("name")) {
        import.labels.insert("legacy.package".to_string(), name.clone());
    }
    if object.contains_key("version") && json_string(object.get("version")).is_none() {
        return Err(legacy_import_error(
            LegacyWrapperKind::Npm,
            "package version must be a string",
        ));
    }
    if let Some(version) = json_string(object.get("version")) {
        import.labels.insert("legacy.version".to_string(), version);
    }
    let scripts = object.get("scripts").and_then(|value| match value {
        jet_foundation::JSON::JSONValue::Object(value) => Some(value),
        _ => None,
    }).ok_or_else(|| {
        legacy_import_error(LegacyWrapperKind::Npm, "package.json needs a scripts object")
    })?;
    let mut script_names = Vec::new();
    for (name, value) in scripts {
        let Some(command) = json_string(Some(value)) else {
            return Err(legacy_import_error(
                LegacyWrapperKind::Npm,
                "every npm script must be a string command",
            ));
        };
        import
            .labels
            .insert(format!("legacy.script.{name}"), command);
        script_names.push(name.clone());
    }
    let script = if scripts.contains_key("build") {
        "build".to_string()
    } else if script_names.len() == 1 {
        script_names.pop().expect("one npm script was checked")
    } else {
        return Err(legacy_import_error(
            LegacyWrapperKind::Npm,
            "without a build script, exactly one npm script is required",
        ));
    };
    import.argv = Some(vec!["npm".to_string(), "run".to_string(), script.clone()]);
    let output = if let Some(value) = object.get("main") {
        json_string(Some(value)).ok_or_else(|| {
            legacy_import_error(LegacyWrapperKind::Npm, "package main must be a string")
        })?
    } else if let Some(value) = object.get("module") {
        match value {
            jet_foundation::JSON::JSONValue::String(path) => path.clone(),
            jet_foundation::JSON::JSONValue::Array(values) => {
                if values.len() != 1 {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::Npm,
                        "package module must contain exactly one string path",
                    ));
                }
                let Some(jet_foundation::JSON::JSONValue::String(path)) = values.first() else {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::Npm,
                        "package module must contain one string path",
                    ));
                };
                path.clone()
            }
            _ => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Npm,
                    "package module must be a string path",
                ));
            }
        }
    } else {
        "dist/index.js".to_string()
    };
    push_unique(&mut import.outputs, output);
    let dependency_sections = [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ];
    let mut has_dependencies = false;
    for section in dependency_sections {
        if let Some(value) = object.get(section) {
            let jet_foundation::JSON::JSONValue::Object(values) = value else {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Npm,
                    format!("package {section} must be an object"),
                ));
            };
            has_dependencies |= !values.is_empty();
            for (name, requirement) in values {
                let Some(requirement) = json_string(Some(requirement)) else {
                    return Err(legacy_import_error(
                        LegacyWrapperKind::Npm,
                        format!("package dependency `{name}` in {section} must be a string"),
                    ));
                };
                import.labels.insert(
                    format!("legacy.dependency.{section}.{name}"),
                    requirement,
                );
            }
        }
    }
    let mut has_lock = false;
    for lock in ["package-lock.json", "npm-shrinkwrap.json"] {
        match fs::symlink_metadata(root.join(lock)) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Npm,
                    format!("lockfile `{lock}` may not be a symlink"),
                ));
            }
            Ok(metadata) if metadata.is_file() => {
                has_lock = true;
                push_unique(&mut import.inputs, lock);
            }
            Ok(_) => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Npm,
                    format!("lockfile `{lock}` is not a regular file"),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(legacy_import_error(
                    LegacyWrapperKind::Npm,
                    format!("cannot inspect lockfile `{lock}`"),
                ));
            }
        }
    }
    if has_dependencies && !has_lock {
        return Err(legacy_import_error(
            LegacyWrapperKind::Npm,
            "dependency closure requires package-lock.json or npm-shrinkwrap.json",
        ));
    }
    Ok(())
}

fn parse_legacy_project(
    root: &Path,
    kind: LegacyWrapperKind,
    source: &str,
) -> Result<LegacyProjectImport, BuildError> {
    let mut import = LegacyProjectImport::default();
    import.caps.extend([BuildCapability::Exec, BuildCapability::FS]);
    import
        .labels
        .insert("legacy.import.parser".to_string(), kind.as_str().to_string());
    match kind {
        LegacyWrapperKind::CMake => parse_cmake_import(source, &mut import)?,
        LegacyWrapperKind::Make => parse_make_import(source, &mut import)?,
        LegacyWrapperKind::Gradle => parse_gradle_import(source, &mut import)?,
        LegacyWrapperKind::Npm => parse_npm_import(root, source, &mut import)?,
        LegacyWrapperKind::Cargo => parse_cargo_import(root, source, &mut import)?,
    }
    apply_import_directives(kind, source, &mut import)?;
    // The wrapper command may observe any project file, not only the
    // canonical manifest.  Import the bounded, non-symlink source closure so
    // cache identity and remote execution cannot silently ignore headers,
    // scripts, lockfiles, or auxiliary build inputs.
    collect_legacy_project_inputs(root, kind, &mut import)?;
    Ok(import)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCache {
    Cached,
    UncachedPhony,
}

/// Distinct executable identities under one BuildPlan (E4-JP2 / #419).
/// Compile / docs / debug / source-archive never share a cache key even when
/// argv and declared paths match — each surface observes different outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActionKind {
    Compile,
    Docs,
    Debug,
    SourceArchive,
    Generic,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Compile => "compile",
            ActionKind::Docs => "docs",
            ActionKind::Debug => "debug",
            ActionKind::SourceArchive => "source-archive",
            ActionKind::Generic => "generic",
        }
    }

    /// Exact source bytes remain identity inputs when these surfaces can
    /// observe them (docs, doctests, diagnostics/line maps, debug info,
    /// publication / source archives).
    pub fn observes_exact_source(self) -> bool {
        matches!(
            self,
            ActionKind::Compile
                | ActionKind::Docs
                | ActionKind::Debug
                | ActionKind::SourceArchive
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec {
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Only allowlisted env keys enter the action identity. Empty means the
    /// declared `env` map itself is the allowlist (no ambient leakage).
    pub env_allowlist: BTreeSet<String>,
    pub caps: BTreeSet<BuildCapability>,
    pub cache: ActionCache,
    pub kind: ActionKind,
    pub toolchain: Option<ToolchainHandle>,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
    /// Helper tool versions (formatter, docgen, archive helper, …) keyed into
    /// the complete CAS identity.
    pub helper_versions: BTreeMap<String, String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub legacy_wrapper: Option<LegacyWrapperKind>,
    /// Selected typed variant identity (E4-JP15 / D-JPK-VARIANT1). Canonical
    /// `PackageVariant::identity_key()` string; empty means host defaults were
    /// never materialised into the action (legacy plans).
    pub variant_identity: Option<String>,
}

impl ActionSpec {
    pub fn cached<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ActionSpec {
            inputs: Vec::new(),
            outputs: Vec::new(),
            argv: argv.into_iter().map(Into::into).collect(),
            env: BTreeMap::new(),
            env_allowlist: BTreeSet::new(),
            caps: BTreeSet::new(),
            cache: ActionCache::Cached,
            kind: ActionKind::Generic,
            toolchain: None,
            probes: Vec::new(),
            signing_identity: None,
            labels: BTreeMap::new(),
            helper_versions: BTreeMap::new(),
            resource_pools: BTreeSet::new(),
            legacy_wrapper: None,
            variant_identity: None,
        }
    }

    pub fn uncached_phony<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ActionSpec {
            cache: ActionCache::UncachedPhony,
            ..Self::cached(argv)
        }
    }

    pub fn with_inputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs
            .extend(paths.into_iter().map(|p| BuildPath(p.into())));
        self
    }

    pub fn with_outputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.outputs
            .extend(paths.into_iter().map(|p| BuildPath(p.into())));
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_env_allowlist<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.env_allowlist
            .extend(keys.into_iter().map(Into::into));
        self
    }

    pub fn with_kind(mut self, kind: ActionKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_helper_version(
        mut self,
        helper: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        self.helper_versions.insert(helper.into(), version.into());
        self
    }

    pub fn with_cap(mut self, cap: BuildCapability) -> Self {
        self.caps.insert(cap);
        self
    }

    pub fn with_toolchain(mut self, toolchain: ToolchainHandle) -> Self {
        self.toolchain = Some(toolchain);
        self
    }

    pub fn with_probe(mut self, probe: ProbeHandle) -> Self {
        self.probes.push(probe);
        self
    }

    pub fn with_signing_identity(mut self, identity: SigningIdentityHandle) -> Self {
        self.signing_identity = Some(identity);
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn with_pool(mut self, pool: BuildResourcePool) -> Self {
        self.resource_pools.insert(pool);
        self
    }

    pub fn with_variant_identity(mut self, identity: impl Into<String>) -> Self {
        self.variant_identity = Some(identity.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildAction {
    pub id: ActionId,
    pub name: String,
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub env_allowlist: BTreeSet<String>,
    pub caps: BTreeSet<BuildCapability>,
    pub cache: ActionCache,
    pub kind: ActionKind,
    pub toolchain: ToolchainHandle,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
    pub helper_versions: BTreeMap<String, String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub legacy_wrapper: Option<LegacyWrapperKind>,
    pub plugin: Option<PluginHandle>,
    /// Selected typed variant identity keyed into the CAS action key (E4-JP15).
    pub variant_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicySetting {
    Allow,
    Deny(String),
}

impl PolicySetting {
    pub fn deny(reason: impl Into<String>) -> Self {
        PolicySetting::Deny(reason.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPolicy {
    pub legacy_wrappers: PolicySetting,
    pub wasm_plugins: PolicySetting,
    pub plugin_grants: BTreeMap<String, BTreeSet<BuildCapability>>,
}

impl BuildPolicy {
    /// Production policy for a local build. The caller still supplies the
    /// action's declared capabilities and the build context enforces them;
    /// this policy only selects which optional integration surfaces are
    /// available by default.
    pub fn local_default() -> Self {
        BuildPolicy {
            legacy_wrappers: PolicySetting::Allow,
            wasm_plugins: PolicySetting::Allow,
            plugin_grants: BTreeMap::new(),
        }
    }

    /// Production policy for CI. Legacy wrappers require an explicit local
    /// policy or an imported graph so CI cannot silently escape the typed
    /// build surface.
    pub fn ci_default() -> Self {
        BuildPolicy {
            legacy_wrappers: PolicySetting::deny(
                "legacy build wrappers are disabled in CI by the production policy",
            ),
            wasm_plugins: PolicySetting::Allow,
            plugin_grants: BTreeMap::new(),
        }
    }

    pub fn allow_all() -> Self {
        Self::local_default()
    }

    pub fn deny_legacy_wrappers(reason: impl Into<String>) -> Self {
        BuildPolicy {
            legacy_wrappers: PolicySetting::deny(reason),
            ..Self::allow_all()
        }
    }

    pub fn deny_wasm_plugins(reason: impl Into<String>) -> Self {
        BuildPolicy {
            wasm_plugins: PolicySetting::deny(reason),
            ..Self::allow_all()
        }
    }

    pub fn with_plugin_grant(mut self, plugin: impl Into<String>, cap: BuildCapability) -> Self {
        self.plugin_grants
            .entry(plugin.into())
            .or_default()
            .insert(cap);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyExplanation {
    pub subject: String,
    pub allowed: bool,
    pub reason: String,
    pub required_caps: Vec<BuildCapability>,
}

impl PolicyExplanation {
    fn allowed(subject: impl Into<String>, caps: Vec<BuildCapability>) -> Self {
        PolicyExplanation {
            subject: subject.into(),
            allowed: true,
            reason: "policy allows this declared authority".to_string(),
            required_caps: caps,
        }
    }

    pub(super) fn denied(
        subject: impl Into<String>,
        reason: impl Into<String>,
        caps: Vec<BuildCapability>,
    ) -> Self {
        PolicyExplanation {
            subject: subject.into(),
            allowed: false,
            reason: reason.into(),
            required_caps: caps,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyWrapperSpec {
    pub kind: LegacyWrapperKind,
    pub argv: Vec<String>,
    pub inputs: Vec<BuildPath>,
    pub outputs: Vec<BuildPath>,
    pub caps: BTreeSet<BuildCapability>,
    pub env: BTreeMap<String, String>,
    pub env_allowlist: BTreeSet<String>,
    pub cache: ActionCache,
    pub action_kind: ActionKind,
    pub toolchain: Option<ToolchainHandle>,
    pub probes: Vec<ProbeHandle>,
    pub signing_identity: Option<SigningIdentityHandle>,
    pub labels: BTreeMap<String, String>,
    pub helper_versions: BTreeMap<String, String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub variant_identity: Option<String>,
}

impl LegacyWrapperSpec {
    /// Import the canonical project file into the same typed action fields used
    /// by `b.legacy`. Unsupported or ambiguous project syntax fails closed;
    /// callers must still pass the imported facts through the typed bridge.
    pub fn from_project_file(
        root: impl AsRef<Path>,
        kind: LegacyWrapperKind,
    ) -> Result<Self, BuildError> {
        let root = root.as_ref();
        let bytes = Self::read_legacy_project_file(root, kind)?;
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| legacy_import_error(kind, "project file is not UTF-8"))?;
        let import = parse_legacy_project(root, kind, source)?;
        let mut spec = Self::new(kind, import.argv.unwrap_or_else(|| kind.default_argv()))
            .with_project_file(kind.project_file());
        for input in import.inputs {
            spec = spec.with_inputs([input]);
        }
        for output in import.outputs {
            spec = spec.with_outputs([output]);
        }
        for cap in import.caps {
            spec = spec.with_cap(cap);
        }
        for (name, value) in import.env {
            spec = spec.with_env(name, value);
        }
        if !import.env_allowlist.is_empty() {
            spec = spec.with_env_allowlist(import.env_allowlist);
        }
        if let Some(cache) = import.cache {
            spec = spec.with_cache(cache);
        }
        if let Some(action_kind) = import.action_kind {
            spec = spec.with_kind(action_kind);
        }
        for pool in import.resource_pools {
            spec = spec.with_pool(pool);
        }
        for (name, value) in import.labels {
            spec = spec.with_label(name, value);
        }
        Ok(spec)
    }

    /// Read and validate the one project file owned by a legacy wrapper.
    /// Keeping this check in the build seam lets both the Rust importer and
    /// the production driver apply the same bounded, UTF-8, non-link rule.
    pub fn read_legacy_project_file(
        root: impl AsRef<Path>,
        kind: LegacyWrapperKind,
    ) -> Result<Vec<u8>, BuildError> {
        let root = root.as_ref();
        let root_meta = fs::symlink_metadata(root)
            .map_err(|_| BuildError::LegacyProjectFileInvalid(root.display().to_string()))?;
        if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
            return Err(BuildError::LegacyProjectFileInvalid(root.display().to_string()));
        }
        let relative = kind.project_file();
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| BuildError::LegacyProjectFileMissing(kind))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(BuildError::LegacyProjectFileInvalid(relative.to_string()));
        }
        if metadata.len() > MAX_LEGACY_PROJECT_FILE_BYTES {
            return Err(BuildError::LegacyProjectFileInvalid(relative.to_string()));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(0o400000 | 0o2000000);
        }
        let file = options
            .open(&path)
            .map_err(|_| BuildError::LegacyProjectFileInvalid(relative.to_string()))?;
        let opened_metadata = file
            .metadata()
            .map_err(|_| BuildError::LegacyProjectFileInvalid(relative.to_string()))?;
        if !opened_metadata.is_file() || opened_metadata.len() > MAX_LEGACY_PROJECT_FILE_BYTES {
            return Err(BuildError::LegacyProjectFileInvalid(relative.to_string()));
        }
        let mut contents = Vec::with_capacity(opened_metadata.len() as usize);
        file.take(MAX_LEGACY_PROJECT_FILE_BYTES + 1)
            .read_to_end(&mut contents)
            .map_err(|_| BuildError::LegacyProjectFileInvalid(relative.to_string()))?;
        if contents.len() as u64 > MAX_LEGACY_PROJECT_FILE_BYTES
            || std::str::from_utf8(&contents).is_err()
        {
            return Err(BuildError::LegacyProjectFileInvalid(relative.to_string()));
        }
        Ok(contents)
    }

    pub fn cmake<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::CMake, argv)
    }

    pub fn make<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Make, argv)
    }

    pub fn gradle<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Gradle, argv)
    }

    pub fn npm<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Npm, argv)
    }

    pub fn cargo<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(LegacyWrapperKind::Cargo, argv)
    }

    fn new<I, S>(kind: LegacyWrapperKind, argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        LegacyWrapperSpec {
            kind,
            argv: argv.into_iter().map(Into::into).collect(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            caps: BTreeSet::new(),
            env: BTreeMap::new(),
            env_allowlist: BTreeSet::new(),
            cache: ActionCache::Cached,
            action_kind: ActionKind::Generic,
            toolchain: None,
            probes: Vec::new(),
            signing_identity: None,
            labels: BTreeMap::new(),
            helper_versions: BTreeMap::new(),
            resource_pools: BTreeSet::new(),
            variant_identity: None,
        }
    }

    pub fn with_inputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for path in paths {
            let path = path.into();
            if !self.inputs.iter().any(|input| input.as_str() == path) {
                self.inputs.push(BuildPath(path));
            }
        }
        self
    }

    /// Mark the project file that an optional graph import inspected. The
    /// path is also a declared input, so later execution cannot hide changes
    /// to the imported project file from action identity.
    pub fn with_project_file(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        if !self.inputs.iter().any(|input| input.as_str() == path) {
            self.inputs.push(BuildPath(path.clone()));
        }
        self.labels
            .insert("legacy.import".to_string(), "project-file".to_string());
        self.labels
            .insert("legacy.project-file".to_string(), path);
        self
    }

    pub fn with_outputs<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for path in paths {
            let path = path.into();
            if !self.outputs.iter().any(|output| output.as_str() == path) {
                self.outputs.push(BuildPath(path));
            }
        }
        self
    }

    pub fn with_cap(mut self, cap: BuildCapability) -> Self {
        self.caps.insert(cap);
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_env_allowlist<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.env_allowlist.extend(keys.into_iter().map(Into::into));
        self
    }

    pub fn with_cache(mut self, cache: ActionCache) -> Self {
        self.cache = cache;
        self
    }

    pub fn with_kind(mut self, kind: ActionKind) -> Self {
        self.action_kind = kind;
        self
    }

    pub fn with_toolchain(mut self, toolchain: ToolchainHandle) -> Self {
        self.toolchain = Some(toolchain);
        self
    }

    pub fn with_probe(mut self, probe: ProbeHandle) -> Self {
        self.probes.push(probe);
        self
    }

    pub fn with_signing_identity(mut self, identity: SigningIdentityHandle) -> Self {
        self.signing_identity = Some(identity);
        self
    }

    pub fn with_helper_version(
        mut self,
        helper: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        self.helper_versions.insert(helper.into(), version.into());
        self
    }

    pub fn with_pool(mut self, pool: BuildResourcePool) -> Self {
        self.resource_pools.insert(pool);
        self
    }

    pub fn with_variant_identity(mut self, identity: impl Into<String>) -> Self {
        self.variant_identity = Some(identity.into());
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn explain(&self, policy: &BuildPolicy) -> PolicyExplanation {
        let subject = format!("legacy wrapper {}", self.kind.as_str());
        let caps = self.caps.iter().cloned().collect();
        match &policy.legacy_wrappers {
            PolicySetting::Allow => PolicyExplanation::allowed(subject, caps),
            PolicySetting::Deny(reason) => PolicyExplanation::denied(subject, reason, caps),
        }
    }

    pub fn into_action_spec(self, policy: &BuildPolicy) -> Result<ActionSpec, BuildError> {
        if let PolicySetting::Deny(_) = &policy.legacy_wrappers {
            return Err(BuildError::PolicyDenied(self.explain(policy)));
        }
        if self.inputs.is_empty() {
            return Err(BuildError::LegacyWrapperWithoutInputs(self.kind));
        }
        if self.outputs.is_empty() {
            return Err(BuildError::LegacyWrapperWithoutOutputs(self.kind));
        }
        if self.caps.is_empty() {
            return Err(BuildError::LegacyWrapperWithoutCaps(self.kind));
        }
        let actual = self
            .argv
            .first()
            .map(|value| {
                std::path::Path::new(value)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(value)
                    .trim_end_matches(".cmd")
                    .trim_end_matches(".exe")
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        if actual != self.kind.as_str() {
            return Err(BuildError::LegacyWrapperCommandMismatch {
                wrapper: self.kind,
                actual,
            });
        }
        let mut labels = self.labels;
        labels.insert("legacy.wrapper".to_string(), self.kind.as_str().to_string());
        let spec = ActionSpec {
            inputs: self.inputs,
            outputs: self.outputs,
            argv: self.argv,
            env: self.env,
            env_allowlist: self.env_allowlist,
            caps: self.caps,
            cache: self.cache,
            kind: self.action_kind,
            toolchain: self.toolchain,
            probes: self.probes,
            signing_identity: self.signing_identity,
            labels,
            helper_versions: self.helper_versions,
            resource_pools: self.resource_pools,
            legacy_wrapper: Some(self.kind),
            variant_identity: self.variant_identity,
        };
        super::validation::validate_action(self.kind.as_str(), &spec)?;
        Ok(spec)
    }
}
