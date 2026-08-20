// D-CONFIG-ENV1 / I9: the source-order, prefix, allowlist, and dotenv rules
// are shared by generated AOT programs and the resident interpreter. Each
// engine supplies its own logical environment snapshot and marshals these
// entries into its existing DataTree carrier.

#[derive(Clone, Debug)]
pub(crate) struct JetEnvConfigEntry {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) segments: Vec<String>,
}

pub(crate) fn jet_env_config_file_is_project_relative(file: &str) -> bool {
    let path = std::path::Path::new(file);
    !path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

/// Collect one config namespace from dotenv text and a process snapshot.
///
/// Environment field segments are folded to lowercase before they enter the
/// existing decoder. This is the case-insensitive env-to-field rule: ordinary
/// Jet record fields use their canonical lowercase spelling, while the source
/// key may use any ASCII case. Process values replace dotenv values by the
/// case-folded environment name.
pub(crate) fn jet_env_config_entries(
    prefix: &str,
    dotenv: Option<&str>,
    allow: &[String],
    process: impl IntoIterator<Item = (String, String)>,
) -> Result<Vec<JetEnvConfigEntry>, String> {
    let allowed = |name: &str| {
        allow.is_empty()
            || allow
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(name))
    };
    let mut values = std::collections::BTreeMap::<String, (String, String)>::new();
    if let Some(text) = dotenv {
        for (line_index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((raw_name, raw_value)) = line.split_once('=') else {
                return Err(format!("invalid .env line {}", line_index + 1));
            };
            let name = raw_name.trim();
            if !jet_env_config_name_is_valid(name) || !allowed(name) {
                continue;
            }
            values.insert(
                name.to_ascii_uppercase(),
                (name.to_string(), jet_env_config_dotenv_value(raw_value.trim())),
            );
        }
    }
    for (name, value) in process {
        if jet_env_config_name_is_valid(&name) && allowed(&name) {
            values.insert(name.to_ascii_uppercase(), (name, value));
        }
    }

    let mut entries = Vec::new();
    for (_, (name, value)) in values {
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let segments: Vec<String> = rest
            .split("__")
            .map(|segment| segment.to_ascii_lowercase())
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.is_empty() {
            continue;
        }
        entries.push(JetEnvConfigEntry {
            name,
            value,
            segments,
        });
    }
    Ok(entries)
}

fn jet_env_config_name_is_valid(name: &str) -> bool {
    !name.is_empty() && !name.contains('=') && !name.contains('\0')
}

fn jet_env_config_dotenv_value(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return value[1..value.len() - 1].to_string();
    }
    value
        .split_once(" #")
        .map_or_else(|| value.to_string(), |(value, _)| value.trim_end().to_string())
}
