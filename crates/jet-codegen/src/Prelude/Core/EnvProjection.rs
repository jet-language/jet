// D-SHAPE-ONE1: root-level semantic adapters for typed environment
// projection. This file is included once, after the canonical Shape source and
// raw EnvConfig Prelude source, so multiply-included host fragments stay
// dependency-minimal.

/// D-SHAPE-ONE1=A: adapt collected environment values to a checked env-name
/// projection. The projection owns source and environment identities; this
/// adapter only maps values back to source segments and never renames fields.
fn jet_env_config_entries_for_shape(
    prefix: &str,
    dotenv: Option<&str>,
    projection: &ShapeProjection,
    process: impl IntoIterator<Item = (String, String)>,
) -> Result<Vec<JetEnvConfigEntry>, String> {
    if projection.kind != ShapeProjectionKind::Env {
        return Err("environment decoding needs an env shape projection".to_string());
    }
    let names = projection
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.decode_name.as_str()))
        .collect::<Vec<_>>();
    jet_env_config_entries_for_names(prefix, dotenv, &[], names, process)
}

/// Host-neutral environment projection boundary. Hosts provide the process
/// snapshot and marshal the returned entries into their DataTree carrier.
pub(crate) fn jet_env_config_entries_for_names<'a>(
    prefix: &str,
    dotenv: Option<&str>,
    allow: &[String],
    names: impl IntoIterator<Item = (&'a str, &'a str)>,
    process: impl IntoIterator<Item = (String, String)>,
) -> Result<Vec<JetEnvConfigEntry>, String> {
    let names = names.into_iter().collect::<Vec<_>>();
    let mut seen = std::collections::BTreeSet::new();
    for (env_name, decode_name) in &names {
        if env_name.trim().is_empty() || decode_name.trim().is_empty() {
            return Err("invalid environment shape field".to_string());
        }
        if !seen.insert(decode_name.to_ascii_uppercase()) {
            return Err(format!("ambiguous environment shape field `{decode_name}`"));
        }
    }

    let mut entries = jet_env_config_entries(prefix, dotenv, allow, process)?;
    for entry in &mut entries {
        if let Some((_, decode_name)) = names.iter().find(|(env_name, _)| {
            let full = format!("{prefix}{env_name}");
            entry.name.eq_ignore_ascii_case(env_name)
                || entry.name.eq_ignore_ascii_case(&full)
                || entry
                    .name
                    .get(prefix.len()..)
                    .is_some_and(|suffix| suffix.eq_ignore_ascii_case(env_name))
        }) {
            entry.segments = vec![decode_name.to_string()];
        }
    }
    entries.sort_by(|left, right| {
        left.segments
            .cmp(&right.segments)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}

fn jet_env_config_source_segments(source: &str) -> Vec<String> {
    source
        .split("__")
        .map(|segment| segment.to_ascii_lowercase())
        .filter(|segment| !segment.is_empty())
        .collect()
}
