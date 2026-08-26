//! Persistent user-tool profile metadata.
//!
//! The profile projects the realized executable bytes into `~/.jet/bin`.
//! `jetpack` never dispatches through those files: an installed tool remains
//! the tool that the user selected, and profile commands inspect metadata.

use crate::{Syntax, JSON, SHA256};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path};

pub(crate) const CURRENT_SCHEMA: &str = "jet-profile-current-v1";
pub(crate) const GENERATION_SCHEMA: &str = "jet-profile-generation-v2";
pub(crate) const PROFILE_OWNER: &str = "user";

const MAX_TOOLS: usize = 256;
const MAX_BINS: usize = 1024;
const MAX_STRING: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentPointer {
    pub(crate) generation: u64,
    pub(crate) witness: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationMetadata {
    pub(crate) generation: u64,
    pub(crate) created_at: u64,
    pub(crate) tools: Vec<GenerationTool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationTool {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: String,
    pub(crate) reference: String,
    pub(crate) output_hash: String,
    pub(crate) store_root: String,
    pub(crate) bins: Vec<String>,
    pub(crate) members: Vec<String>,
    pub(crate) projection_hashes: Vec<String>,
}

pub(crate) fn format_current_pointer(pointer: &CurrentPointer) -> io::Result<String> {
    if pointer.generation == 0 {
        return Err(invalid("tool generation is zero"));
    }
    validate_digest(&pointer.witness)?;
    let body = format!(
        "{CURRENT_SCHEMA}\ngeneration\t{}\nwitness\t{}\n",
        pointer.generation, pointer.witness
    );
    Ok(format!(
        "{body}checksum\tsha256-{}\n",
        SHA256::sha256_hex(body.as_bytes())
    ))
}

pub(crate) fn parse_current_pointer(text: &str) -> io::Result<CurrentPointer> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != CURRENT_SCHEMA || !text.ends_with('\n') {
        return Err(invalid("current pointer has wrong schema or field count"));
    }
    let generation = lines[1]
        .strip_prefix("generation\t")
        .ok_or_else(|| invalid("current pointer lacks generation"))?
        .parse::<u64>()
        .map_err(|_| invalid("current pointer generation is invalid"))?;
    if generation == 0 {
        return Err(invalid("current pointer generation is zero"));
    }
    let witness = lines[2]
        .strip_prefix("witness\t")
        .ok_or_else(|| invalid("current pointer lacks witness"))?;
    validate_digest(witness)?;
    let checksum = lines[3]
        .strip_prefix("checksum\tsha256-")
        .ok_or_else(|| invalid("current pointer lacks checksum"))?;
    validate_hex64(checksum, "current pointer checksum")?;
    let body_len = text
        .rfind("checksum\t")
        .ok_or_else(|| invalid("current pointer lacks checksum"))?;
    if SHA256::sha256_hex(text[..body_len].as_bytes()) != checksum {
        return Err(invalid("current pointer checksum mismatch"));
    }
    Ok(CurrentPointer {
        generation,
        witness: witness.to_string(),
    })
}

pub(crate) fn format_generation_metadata(metadata: &GenerationMetadata) -> io::Result<String> {
    validate_generation(metadata)?;
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"schema\": {},\n",
        json_string(GENERATION_SCHEMA)
    ));
    out.push_str(&format!("  \"generation\": {},\n", metadata.generation));
    out.push_str(&format!("  \"owner\": {},\n", json_string(PROFILE_OWNER)));
    out.push_str(&format!(
        "  \"profile\": {},\n",
        json_string(Syntax::TOOL_PROFILE_NAME)
    ));
    out.push_str(&format!("  \"created_at\": {},\n", metadata.created_at));
    out.push_str("  \"tools\": [\n");
    for (index, tool) in metadata.tools.iter().enumerate() {
        out.push_str("    {\n");
        for (key, value) in [
            ("name", &tool.name),
            ("version", &tool.version),
            ("source", &tool.source),
            ("reference", &tool.reference),
            ("output_hash", &tool.output_hash),
            ("store_root", &tool.store_root),
        ]
        .into_iter()
        {
            out.push_str(&format!("      \"{key}\": {},\n", json_string(value)));
        }
        write_string_array(&mut out, "bins", &tool.bins, true);
        write_string_array(&mut out, "members", &tool.members, true);
        write_string_array(
            &mut out,
            "projection_hashes",
            &tool.projection_hashes,
            false,
        );
        out.push_str("    }");
        if index + 1 != metadata.tools.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    Ok(out)
}

pub(crate) fn parse_generation_metadata(
    text: &str,
    expected_generation: u64,
) -> io::Result<GenerationMetadata> {
    let JSON::JSONValue::Object(root) = JSON::parse(text).map_err(invalid)? else {
        return Err(invalid("profile metadata root is not an object"));
    };
    expect_exact_keys(
        &root,
        &[
            "created_at",
            "generation",
            "owner",
            "profile",
            "schema",
            "tools",
        ],
        "profile metadata",
    )?;
    if string_field(&root, "schema")? != GENERATION_SCHEMA
        || string_field(&root, "owner")? != PROFILE_OWNER
        || string_field(&root, "profile")? != Syntax::TOOL_PROFILE_NAME
    {
        return Err(invalid("profile metadata identity mismatch"));
    }
    let generation = integer_field(&root, "generation")?;
    if generation != expected_generation || generation == 0 {
        return Err(invalid("tool generation metadata disagrees with path"));
    }
    let created_at = integer_field(&root, "created_at")?;
    let JSON::JSONValue::Array(entries) = root
        .get("tools")
        .ok_or_else(|| invalid("profile metadata lacks tools"))?
    else {
        return Err(invalid("profile tools field is not an array"));
    };
    if entries.len() > MAX_TOOLS {
        return Err(invalid("profile tool count exceeds bound"));
    }
    let mut tools = Vec::with_capacity(entries.len());
    for entry in entries {
        let JSON::JSONValue::Object(tool) = entry else {
            return Err(invalid("profile tool entry is not an object"));
        };
        expect_exact_keys(
            tool,
            &[
                "bins",
                "members",
                "name",
                "output_hash",
                "projection_hashes",
                "reference",
                "source",
                "store_root",
                "version",
            ],
            "profile tool",
        )?;
        tools.push(GenerationTool {
            name: bounded_string(tool, "name")?,
            version: bounded_string(tool, "version")?,
            source: bounded_string(tool, "source")?,
            reference: bounded_string(tool, "reference")?,
            output_hash: bounded_string(tool, "output_hash")?,
            store_root: bounded_string(tool, "store_root")?,
            bins: string_array(tool, "bins")?,
            members: string_array(tool, "members")?,
            projection_hashes: string_array(tool, "projection_hashes")?,
        });
    }
    let metadata = GenerationMetadata {
        generation,
        created_at,
        tools,
    };
    validate_generation(&metadata)?;
    Ok(metadata)
}

pub(crate) fn generation_witness(metadata_text: &str, metadata: &GenerationMetadata) -> String {
    let targets = metadata
        .tools
        .iter()
        .map(|tool| tool.output_hash.as_str())
        .collect::<BTreeSet<_>>();
    let mut canonical = format!(
        "jet-profile-generation-witness-v1\nmetadata\t{}\n",
        SHA256::sha256_hex(metadata_text.as_bytes())
    );
    for digest in targets {
        canonical.push_str("target\t");
        canonical.push_str(digest);
        canonical.push('\n');
    }
    format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()))
}

pub(crate) fn physical_bin_name(logical: &str) -> String {
    physical_bin_name_for(logical, cfg!(windows))
}

fn physical_bin_name_for(logical: &str, windows: bool) -> String {
    if windows && !logical.to_ascii_lowercase().ends_with(".exe") {
        format!("{logical}.exe")
    } else {
        logical.to_string()
    }
}

fn validate_generation(metadata: &GenerationMetadata) -> io::Result<()> {
    if metadata.generation == 0 || metadata.tools.len() > MAX_TOOLS {
        return Err(invalid("tool generation exceeds bounds"));
    }
    let mut identities = BTreeSet::new();
    let mut bins = BTreeSet::new();
    let mut physical_bins = BTreeSet::new();
    for tool in &metadata.tools {
        for value in [
            &tool.name,
            &tool.version,
            &tool.source,
            &tool.reference,
            &tool.store_root,
        ] {
            validate_string(value)?;
        }
        validate_digest(&tool.output_hash)?;
        validate_store_root(&tool.store_root)?;
        if tool.bins.is_empty()
            || tool.bins.len() != tool.members.len()
            || tool.bins.len() != tool.projection_hashes.len()
        {
            return Err(invalid("profile tool has mismatched projection fields"));
        }
        if !identities.insert((&tool.name, &tool.reference)) {
            return Err(invalid("duplicate profile tool identity"));
        }
        for ((bin, member), digest) in tool
            .bins
            .iter()
            .zip(&tool.members)
            .zip(&tool.projection_hashes)
        {
            validate_bin_name(bin)?;
            validate_bin_name(member)?;
            validate_digest(digest)?;
            if !bins.insert(bin)
                || !physical_bins.insert(physical_bin_name_for(bin, true).to_ascii_lowercase())
            {
                return Err(invalid("duplicate profile bin"));
            }
            if bins.len() > MAX_BINS {
                return Err(invalid("profile bin count exceeds bound"));
            }
        }
    }
    if metadata
        .tools
        .windows(2)
        .any(|pair| (&pair[0].name, &pair[0].reference) >= (&pair[1].name, &pair[1].reference))
    {
        return Err(invalid("profile tools are not in canonical order"));
    }
    Ok(())
}

pub(crate) fn validate_bin_name(value: &str) -> io::Result<()> {
    if value.is_empty() || value.len() > 255 {
        return Err(invalid("profile bin name has invalid length"));
    }
    let mut components = Path::new(value).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+'))
    {
        return Err(invalid("profile bin name is not one normal component"));
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || stem
            .strip_prefix("COM")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value))
        || stem
            .strip_prefix("LPT")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value));
    if reserved {
        return Err(invalid("profile bin name is reserved on Windows"));
    }
    if value.eq_ignore_ascii_case("jetpack") || value.eq_ignore_ascii_case("jetpack.exe") {
        return Err(invalid("profile bin name collides with the package engine"));
    }
    Ok(())
}

fn validate_store_root(value: &str) -> io::Result<()> {
    validate_string(value)?;
    let path = Path::new(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid(
            "profile Store authority is not absolute and normalized",
        ));
    }
    Ok(())
}

fn validate_digest(value: &str) -> io::Result<()> {
    let hex = value
        .strip_prefix("sha256-")
        .ok_or_else(|| invalid("profile digest is not sha256"))?;
    validate_hex64(hex, "profile digest")
}

fn validate_hex64(value: &str, label: &str) -> io::Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid(format!("{label} is not canonical")));
    }
    Ok(())
}

fn validate_string(value: &str) -> io::Result<()> {
    if value.len() > MAX_STRING
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(invalid("profile string exceeds bounds"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            value if value.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", value as u32);
            }
            value => out.push(value),
        }
    }
    out.push('"');
    out
}

fn write_string_array(out: &mut String, key: &str, values: &[String], comma: bool) {
    out.push_str(&format!("      \"{key}\": ["));
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn expect_exact_keys(
    object: &BTreeMap<String, JSON::JSONValue>,
    expected: &[&str],
    label: &str,
) -> io::Result<()> {
    let actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err(invalid(format!("{label} has unknown or missing fields")));
    }
    Ok(())
}

fn string_field<'a>(
    object: &'a BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> io::Result<&'a str> {
    object
        .get(key)
        .ok_or_else(|| invalid(format!("missing key `{key}`")))?
        .as_str()
        .map_err(invalid)
}

fn bounded_string(object: &BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<String> {
    let value = string_field(object, key)?;
    validate_string(value)?;
    Ok(value.to_string())
}

fn integer_field(object: &BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<u64> {
    match object.get(key) {
        Some(JSON::JSONValue::Number(value)) if *value >= 0 => Ok(*value as u64),
        Some(JSON::JSONValue::Flt(value))
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= 9_007_199_254_740_991.0 =>
        {
            Ok(*value as u64)
        }
        Some(_) => Err(invalid(format!(
            "profile field `{key}` is not an exact integer"
        ))),
        None => Err(invalid(format!("profile field `{key}` is not a number"))),
    }
}

fn string_array(object: &BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<Vec<String>> {
    let Some(JSON::JSONValue::Array(values)) = object.get(key) else {
        return Err(invalid(format!("profile field `{key}` is not an array")));
    };
    values
        .iter()
        .map(|value| {
            let value = value.as_str().map_err(invalid)?;
            if value.len() > 255 {
                return Err(invalid(format!("profile field `{key}` exceeds bounds")));
            }
            Ok(value.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        format!("sha256-{}", byte.to_string().repeat(64))
    }

    fn metadata() -> GenerationMetadata {
        GenerationMetadata {
            generation: 7,
            created_at: 1,
            tools: vec![GenerationTool {
                name: "echo-args".into(),
                version: "1".into(),
                source: "path".into(),
                reference: "path:echo-args".into(),
                output_hash: digest('a'),
                store_root: if cfg!(windows) {
                    "C:\\store".into()
                } else {
                    "/store".into()
                },
                bins: vec!["echo-args".into()],
                members: vec!["echo-args".into()],
                projection_hashes: vec![digest('b')],
            }],
        }
    }

    #[test]
    fn current_pointer_rejects_bitflip_truncation_and_traversal() {
        let pointer = CurrentPointer {
            generation: 7,
            witness: digest('a'),
        };
        let wire = format_current_pointer(&pointer).unwrap();
        assert_eq!(parse_current_pointer(&wire).unwrap(), pointer);
        assert!(parse_current_pointer(&wire.replace("generation\t7", "generation\t8")).is_err());
        assert!(parse_current_pointer(wire.trim_end()).is_err());
        assert!(parse_current_pointer(&wire.replace("generation\t7", "generation\t../7")).is_err());
    }

    #[test]
    fn generation_metadata_roundtrips_and_binds_projection() {
        let metadata = metadata();
        let wire = format_generation_metadata(&metadata).unwrap();
        assert_eq!(parse_generation_metadata(&wire, 7).unwrap(), metadata);
        let witness = generation_witness(&wire, &metadata);
        let changed = wire.replace(&digest('b'), &digest('c'));
        let changed_metadata = parse_generation_metadata(&changed, 7).unwrap();
        assert_ne!(witness, generation_witness(&changed, &changed_metadata));
    }

    #[test]
    fn names_reject_traversal_windows_reserved_and_case_collisions() {
        for invalid in [
            "", "../x", "a/b", "a\\b", "CON", "com1.exe", "nul.txt", "jetpack",
        ] {
            assert!(validate_bin_name(invalid).is_err(), "accepted {invalid:?}");
        }
        let mut case_collision = metadata();
        case_collision.tools.push(GenerationTool {
            name: "other".into(),
            version: "1".into(),
            source: "path".into(),
            reference: "path:other".into(),
            output_hash: digest('c'),
            store_root: case_collision.tools[0].store_root.clone(),
            bins: vec!["ECHO-ARGS".into()],
            members: vec!["other".into()],
            projection_hashes: vec![digest('d')],
        });
        assert!(format_generation_metadata(&case_collision).is_err());

        let mut physical_collision = metadata();
        physical_collision.tools[0].bins = vec!["foo".into()];
        physical_collision.tools.push(GenerationTool {
            name: "other".into(),
            version: "1".into(),
            source: "path".into(),
            reference: "path:other".into(),
            output_hash: digest('c'),
            store_root: physical_collision.tools[0].store_root.clone(),
            bins: vec!["foo.exe".into()],
            members: vec!["other".into()],
            projection_hashes: vec![digest('d')],
        });
        assert!(format_generation_metadata(&physical_collision).is_err());
        assert!(validate_bin_name("jetpack.exe").is_err());
    }

    #[test]
    fn windows_physical_alias_mapping_is_exact_first() {
        assert_eq!(physical_bin_name_for("foo", true), "foo.exe");
        assert_eq!(physical_bin_name_for("foo.exe", true), "foo.exe");
        assert_eq!(physical_bin_name_for("foo.EXE", true), "foo.EXE");
        assert_eq!(physical_bin_name_for("foo", false), "foo");
    }
}
