//! Off-device nixpkgs index producer and differential verifier.
//!
//! This binary consumes staged evidence.  It never invokes Nix, curl, wget,
//! zstd, or a signing secret.  The surrounding producer job owns those steps.

// This crate spells modules in PascalCase; match `crates/jetpack/src/lib.rs:12`.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use jetpack::SHA256;

mod Store {
    pub use jetpack::Store::Roots;
}

// Only the producer half is used here; the client half is card #2158.
// This producer uses only the producer half of the module; the client half
// is consumed by card #2158. Remove this allow with that card.
#[allow(dead_code)]
#[path = "../NixIndex.rs"]
mod NixIndex;

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.first().map(String::as_str) {
        Some("generate") => {
            args.remove(0);
            generate(&args)
        }
        Some("verify-differential") => {
            args.remove(0);
            verify_differential(&args)
        }
        Some("manifest") => {
            args.remove(0);
            manifest(&args)
        }
        _ => {
            Err("usage: jetpack-nix-index <generate|verify-differential|manifest> ...".to_string())
        }
    };
    if let Err(error) = result {
        eprintln!("jetpack-nix-index: {error}");
        std::process::exit(1);
    }
}

fn generate(args: &[String]) -> Result<(), String> {
    let options = options(args)?;
    let channel = required(&options, "channel")?;
    let system = required(&options, "system")?;
    let requested_revision = required(&options, "revision")?;
    let revision_file = required_path(&options, "git-revision")?;
    let release_metadata_path = required_path(&options, "release-metadata")?;
    let packages_path = required_path(&options, "packages-json")?;
    let hydra_eval_path = required_path(&options, "hydra-eval")?;
    let hydra_build_dir = required_path(&options, "hydra-build-dir")?;
    let oracle_path = required_path(&options, "oracle")?;
    let store_paths_path = required_path(&options, "store-paths")?;
    let output_root = required_path(&options, "output")?;
    let actual_revision = fs::read_to_string(&revision_file)
        .map_err(|error| format!("read git-revision: {error}"))?
        .trim()
        .to_string();
    if actual_revision != requested_revision {
        return Err(format!(
            "release revision `{actual_revision}` disagrees with requested `{requested_revision}`"
        ));
    }
    validate_staged_input(&release_metadata_path, "release metadata")?;
    let packages = read_json_input(&packages_path, 128 * 1024 * 1024, "packages.json")?;
    let hydra_eval = read_json_input(&hydra_eval_path, 64 * 1024 * 1024, "Hydra evaluation")?;
    if !json_value_contains_field_string(&hydra_eval, "revision", &requested_revision) {
        return Err("Hydra evaluation does not bind the requested revision".to_string());
    }
    let hydra_output_paths = read_hydra_output_paths(&hydra_build_dir, &system)?;
    if hydra_output_paths.is_empty() {
        return Err("Hydra build input contains no output paths".to_string());
    }
    let released_unix = options
        .get("released-unix")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "--released-unix must be an integer".to_string())
        })
        .transpose()?
        .or_else(|| {
            fs::read(&release_metadata_path).ok().and_then(|bytes| {
                find_json_integer(&bytes, &["released_unix", "timestamp", "release_time"])
            })
        })
        .ok_or_else(|| {
            "release metadata must supply released_unix, timestamp, or release_time".to_string()
        })?;
    let oracle = NixIndex::parse_oracle_for_producer(
        &read_file(&oracle_path, 256 * 1024 * 1024, "oracle")?,
        &system,
    )
    .map_err(|error| error.to_string())?;
    for candidate in &oracle {
        if !json_value_contains_string(&packages, &candidate.record.version) {
            return Err(format!(
                "packages.json does not contain oracle version for {}",
                candidate.record.attrpath.join(".")
            ));
        }
    }
    let store_paths = read_store_paths(&store_paths_path)?;
    let (decoded, compressed, mut report) = NixIndex::producer_generate_with_hydra_paths(
        &channel,
        &system,
        &requested_revision,
        released_unix,
        &oracle,
        &store_paths,
        &hydra_output_paths,
    )
    .map_err(|error| error.to_string())?;
    let digest = SHA256::sha256_hex(&compressed);
    let target_dir = output_root
        .join("index-v1")
        .join(&requested_revision)
        .join(&system);
    fs::create_dir_all(&target_dir).map_err(|error| format!("create output: {error}"))?;
    let target = target_dir.join(format!("{digest}.json.zst"));
    write_immutable(&target, &compressed)?;
    write_immutable(
        &PathBuf::from(format!("{}.sig.request", target.display())),
        &NixIndex::producer_signature_request(&decoded),
    )?;
    let coverage =
        NixIndex::producer_coverage_report(&decoded).map_err(|error| error.to_string())?;
    write_immutable(
        &target_dir.join(format!("{digest}.coverage.json")),
        coverage.as_bytes(),
    )?;
    if report.ends_with('}') {
        report.pop();
    }
    report.push_str(&format!(
        ",\"target_sha256\":\"{digest}\",\"target\":\"{}\",\"signature_request\":\"{}.sig.request\"}}",
        target.display(),
        target.display()
    ));
    write_immutable(
        &target_dir.join(format!("{digest}.generation-report.json")),
        report.as_bytes(),
    )?;
    println!(
        "generated channel={channel} revision={requested_revision} system={system} compressed_bytes={} decoded_bytes={} target_sha256={digest}",
        compressed.len(),
        decoded.len()
    );
    Ok(())
}

fn verify_differential(args: &[String]) -> Result<(), String> {
    let options = options(args)?;
    let candidate = required_path(&options, "candidate")?;
    let oracle_path = required_path(&options, "oracle")?;
    let report_path = required_path(&options, "report")?;
    let channel = required(&options, "channel")?;
    let revision = required(&options, "revision")?;
    let system = required(&options, "system")?;
    let candidate_bytes = read_file(
        &candidate,
        NixIndex::MAX_COMPRESSED_BYTES as u64,
        "candidate index",
    )?;
    let (candidate_channel, candidate_revision, candidate_system) =
        NixIndex::producer_index_identity(&candidate_bytes).map_err(|error| error.to_string())?;
    if (candidate_channel, candidate_revision, candidate_system)
        != (channel.clone(), revision.clone(), system.clone())
    {
        return Err("candidate index identity disagrees with differential key".to_string());
    }
    let index_records = NixIndex::decode_index_records_for_producer(&candidate_bytes)
        .map_err(|error| error.to_string())?;
    let oracle = NixIndex::parse_oracle_for_producer(
        &read_file(&oracle_path, NixIndex::MAX_DECODED_BYTES as u64, "oracle")?,
        &system,
    )
    .map_err(|error| error.to_string())?;
    let mut expected = BTreeMap::new();
    for item in oracle {
        if expected
            .insert(item.record.attrpath.clone(), item.record)
            .is_some()
        {
            return Err("differential oracle has duplicate attrpaths".to_string());
        }
    }
    let mut actual = BTreeMap::new();
    for record in index_records {
        if actual.insert(record.attrpath.clone(), record).is_some() {
            return Err("differential index has duplicate attrpaths".to_string());
        }
    }
    let mut mismatches = Vec::new();
    for attrpath in expected.keys().chain(actual.keys()) {
        let left = expected.get(attrpath);
        let right = actual.get(attrpath);
        if left != right {
            mismatches.push(attrpath.join("."));
        }
    }
    mismatches.sort();
    mismatches.dedup();
    let records_compared = expected
        .keys()
        .chain(actual.keys())
        .collect::<BTreeSet<_>>()
        .len();
    let report = format!(
        "{{\"schema\":1,\"channel\":\"{}\",\"revision\":\"{}\",\"system\":\"{}\",\"records_compared\":{},\"mismatches\":{},\"status\":\"{}\"}}\n",
        escape(&channel),
        escape(&revision),
        escape(&system),
        records_compared,
        mismatches.len(),
        if mismatches.is_empty() { "passed" } else { "failed" }
    );
    fs::write(&report_path, report)
        .map_err(|error| format!("write differential report: {error}"))?;
    if mismatches.is_empty() {
        println!("differential verification passed: all records compared, zero mismatches");
        Ok(())
    } else {
        Err(format!(
            "differential verification failed: {} mismatches; see {}",
            mismatches.len(),
            report_path.display()
        ))
    }
}

fn manifest(args: &[String]) -> Result<(), String> {
    let options = options(args)?;
    let channel = required(&options, "channel")?;
    let endpoint = required(&options, "endpoint")?;
    let generation = required(&options, "generation")?
        .parse::<u64>()
        .map_err(|_| "--generation must be an integer".to_string())?;
    let issued_unix = required(&options, "issued-unix")?
        .parse::<u64>()
        .map_err(|_| "--issued-unix must be an integer".to_string())?;
    let expires_unix = required(&options, "expires-unix")?
        .parse::<u64>()
        .map_err(|_| "--expires-unix must be an integer".to_string())?;
    let target_root = required_path(&options, "target-root")?;
    let output = required_path(&options, "output")?;
    validate_staged_input(&target_root, "target root")?;
    let mut targets = Vec::new();
    let index_root = target_root.join("index-v1");
    for revision_entry in
        fs::read_dir(&index_root).map_err(|error| format!("read target root: {error}"))?
    {
        let revision_entry =
            revision_entry.map_err(|error| format!("read target revision: {error}"))?;
        let revision_path = revision_entry.path();
        let revision_metadata = fs::symlink_metadata(&revision_path)
            .map_err(|error| format!("inspect target revision: {error}"))?;
        if revision_metadata.file_type().is_symlink() {
            return Err(format!(
                "target revision `{}` must not be a symlink",
                revision_path.display()
            ));
        }
        if !revision_metadata.is_dir() {
            continue;
        }
        let revision = revision_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "target revision name is not UTF-8".to_string())?
            .to_string();
        for system_entry in
            fs::read_dir(&revision_path).map_err(|error| format!("read target system: {error}"))?
        {
            let system_entry =
                system_entry.map_err(|error| format!("read target system: {error}"))?;
            let system_path = system_entry.path();
            let system_metadata = fs::symlink_metadata(&system_path)
                .map_err(|error| format!("inspect target system: {error}"))?;
            if system_metadata.file_type().is_symlink() {
                return Err(format!(
                    "target system `{}` must not be a symlink",
                    system_path.display()
                ));
            }
            if !system_metadata.is_dir() {
                continue;
            }
            let system = system_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "target system name is not UTF-8".to_string())?
                .to_string();
            for file_entry in
                fs::read_dir(&system_path).map_err(|error| format!("read target files: {error}"))?
            {
                let file_entry =
                    file_entry.map_err(|error| format!("read target file: {error}"))?;
                let target = file_entry.path();
                if target.extension().and_then(|value| value.to_str()) != Some("zst") {
                    continue;
                }
                let compressed = read_file(
                    &target,
                    NixIndex::MAX_COMPRESSED_BYTES as u64,
                    "index target",
                )?;
                let digest = SHA256::sha256_hex(&compressed);
                let expected_digest = target
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .and_then(|value| value.strip_suffix(".json"))
                    .ok_or_else(|| "index target filename is malformed".to_string())?;
                if digest != expected_digest {
                    return Err(format!("target digest mismatch for {}", target.display()));
                }
                let signature_path = PathBuf::from(format!("{}.sig.json", target.display()));
                let signature = read_file(&signature_path, 16 * 1024, "index signature")?;
                let (decoded_length, record_count) =
                    NixIndex::producer_target_measurements(&compressed)
                        .map_err(|error| error.to_string())?;
                let generation_report =
                    PathBuf::from(format!("{}.generation-report.json", target.display()));
                let released_unix = find_json_integer(
                    &read_file(&generation_report, 16 * 1024, "index generation report")?,
                    &["released_unix"],
                )
                .ok_or_else(|| {
                    format!(
                        "index generation report has no released_unix: {}",
                        generation_report.display()
                    )
                })?;
                let url = format!(
                    "{}/index-v1/{revision}/{system}/{digest}.json.zst",
                    endpoint.trim_end_matches('/')
                );
                targets.push((
                    released_unix,
                    system.clone(),
                    (
                        revision.clone(),
                        system.clone(),
                        url.clone(),
                        format!("{url}.sig.json"),
                        digest,
                        compressed.len() as u64,
                        decoded_length,
                        record_count,
                        SHA256::sha256_hex(&signature),
                        true,
                    ),
                ));
            }
        }
    }
    let discoverable = newest_discoverable_targets(
        targets
            .iter()
            .map(|(released, system, target)| (*released, system.clone(), target.0.clone()))
            .collect(),
    );
    let targets: Vec<_> = targets
        .into_iter()
        .map(|(_, system, mut target)| {
            target.9 = discoverable.contains(&(system, target.0.clone()));
            target
        })
        .collect();
    let target_count = targets.len();
    let bytes =
        NixIndex::producer_manifest_bytes(&channel, generation, issued_unix, expires_unix, targets)
            .map_err(|error| error.to_string())?;
    write_immutable(&output, &bytes)?;
    write_immutable(
        &PathBuf::from(format!("{}.sig.request", output.display())),
        &NixIndex::manifest_signature_request(&bytes),
    )?;
    println!(
        "generated channel manifest generation={} targets={} bytes={}",
        generation,
        target_count,
        bytes.len()
    );
    Ok(())
}

fn options(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index]
            .strip_prefix("--")
            .ok_or_else(|| format!("expected an option, got `{}`", args[index]))?;
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("option `--{name}` needs a value"))?;
        if value.starts_with("--") {
            return Err(format!("option `--{name}` needs a value"));
        }
        if options.insert(name.to_string(), value.clone()).is_some() {
            return Err(format!("duplicate option `--{name}`"));
        }
        index += 2;
    }
    Ok(options)
}

fn newest_discoverable_targets(
    mut targets: Vec<(u64, String, String)>,
) -> std::collections::BTreeSet<(String, String)> {
    targets.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.as_bytes().cmp(right.1.as_bytes()))
            .then_with(|| left.2.as_bytes().cmp(right.2.as_bytes()))
    });
    let mut counts = BTreeMap::<String, usize>::new();
    let mut discoverable = std::collections::BTreeSet::new();
    for (_, system, revision) in targets {
        let count = counts.entry(system.clone()).or_default();
        if *count < 12 {
            discoverable.insert((system, revision));
        }
        *count += 1;
    }
    discoverable
}

fn required(options: &BTreeMap<String, String>, name: &str) -> Result<String, String> {
    options
        .get(name)
        .cloned()
        .ok_or_else(|| format!("missing required option `--{name}`"))
}

fn required_path(options: &BTreeMap<String, String>, name: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(required(options, name)?))
}

fn validate_staged_input(path: &Path, label: &str) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label} must not be a symlink"));
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(format!("{label} must be a regular file or directory"));
    }
    Ok(())
}

fn read_file(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    validate_staged_input(path, label)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspect {label}: {error}"))?;
    if metadata.len() > limit {
        return Err(format!("{label} exceeds its bound"));
    }
    fs::read(path).map_err(|error| format!("read {label}: {error}"))
}

fn read_store_paths(path: &Path) -> Result<BTreeSet<String>, String> {
    let bytes = read_file(path, 64 * 1024 * 1024, "store-paths")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "store-paths is not UTF-8".to_string())?;
    Ok(text
        .lines()
        .map(str::to_string)
        .filter(|line| !line.is_empty())
        .collect())
}

fn write_immutable(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "immutable output `{}` is not regular",
                path.display()
            ));
        }
        if fs::read(path).map_err(|error| format!("read existing output: {error}"))? != bytes {
            return Err(format!("immutable output `{}` changed", path.display()));
        }
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "output has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create output parent: {error}"))?;
    // The temporary sits beside the target, so only its file name is needed;
    // flattening the whole path in here would also build an absurd name.
    let name = path
        .file_name()
        .ok_or_else(|| "output has no file name".to_string())?;
    let partial = parent.join(format!(
        ".{}.partial-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    fs::write(&partial, bytes).map_err(|error| format!("write temporary output: {error}"))?;
    fs::rename(&partial, path).map_err(|error| format!("publish output: {error}"))
}

fn read_json_input(
    path: &Path,
    limit: u64,
    label: &str,
) -> Result<jet_foundation::EncodingJson::Value, String> {
    let bytes = read_file(path, limit, label)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| format!("{label} is not UTF-8"))?;
    jet_foundation::EncodingJson::parse_json_exact_numbers(text, true)
        .map_err(|error| format!("parse {label}: {}", error.message))
}

fn json_value_contains_string(value: &jet_foundation::EncodingJson::Value, needle: &str) -> bool {
    value_contains_string(value, needle)
}

fn value_contains_string(value: &jet_foundation::EncodingJson::Value, needle: &str) -> bool {
    match value {
        jet_foundation::EncodingJson::Value::Text(value) => value == needle,
        jet_foundation::EncodingJson::Value::Array(values) => values
            .iter()
            .any(|value| value_contains_string(value, needle)),
        jet_foundation::EncodingJson::Value::Object(values) => values
            .iter()
            .any(|(_, value)| value_contains_string(value, needle)),
        _ => false,
    }
}

fn json_value_contains_field_string(
    value: &jet_foundation::EncodingJson::Value,
    field: &str,
    needle: &str,
) -> bool {
    match value {
        jet_foundation::EncodingJson::Value::Object(values) => values.iter().any(|(key, value)| {
            (key == field
                && matches!(
                    value,
                    jet_foundation::EncodingJson::Value::Text(value) if value == needle
                ))
                || json_value_contains_field_string(value, field, needle)
        }),
        jet_foundation::EncodingJson::Value::Array(values) => values
            .iter()
            .any(|value| json_value_contains_field_string(value, field, needle)),
        _ => false,
    }
}

fn collect_named_strings(
    value: &jet_foundation::EncodingJson::Value,
    field: &str,
    values: &mut BTreeSet<String>,
) {
    match value {
        jet_foundation::EncodingJson::Value::Object(entries) => {
            for (key, value) in entries {
                if key == field {
                    if let jet_foundation::EncodingJson::Value::Text(value) = value {
                        values.insert(value.clone());
                    }
                }
                collect_named_strings(value, field, values);
            }
        }
        jet_foundation::EncodingJson::Value::Array(entries) => {
            for value in entries {
                collect_named_strings(value, field, values);
            }
        }
        _ => {}
    }
}

fn read_hydra_output_paths(path: &Path, system: &str) -> Result<BTreeSet<String>, String> {
    validate_staged_input(path, "Hydra build directory")?;
    let mut files = Vec::new();
    collect_json_files(path, &mut files)?;
    if files.is_empty() {
        return Err("Hydra build directory contains no JSON files".to_string());
    }
    files.sort();
    let mut output_paths = BTreeSet::new();
    for file in files {
        let value = read_json_input(&file, 64 * 1024 * 1024, "Hydra build record")?;
        let mut systems = BTreeSet::new();
        collect_named_strings(&value, "system", &mut systems);
        if systems.is_empty() || systems.iter().any(|value| value != system) {
            return Err(format!(
                "Hydra build record has no exact `{system}` system: {}",
                file.display()
            ));
        }
        collect_store_paths(&value, &mut output_paths);
    }
    Ok(output_paths)
}

fn collect_json_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("inspect Hydra build input: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Hydra build input `{}` must not be a symlink",
            path.display()
        ));
    }
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "Hydra build input `{}` is not a file or directory",
            path.display()
        ));
    }
    for entry in fs::read_dir(path).map_err(|error| format!("read Hydra build input: {error}"))? {
        let entry = entry.map_err(|error| format!("read Hydra build entry: {error}"))?;
        collect_json_files(&entry.path(), files)?;
    }
    Ok(())
}

fn collect_store_paths(value: &jet_foundation::EncodingJson::Value, paths: &mut BTreeSet<String>) {
    match value {
        jet_foundation::EncodingJson::Value::Text(value) if value.starts_with("/nix/store/") => {
            paths.insert(value.clone());
        }
        jet_foundation::EncodingJson::Value::Array(values) => {
            for value in values {
                collect_store_paths(value, paths);
            }
        }
        jet_foundation::EncodingJson::Value::Object(values) => {
            for (_, value) in values {
                collect_store_paths(value, paths);
            }
        }
        _ => {}
    }
}

fn find_json_integer(bytes: &[u8], names: &[&str]) -> Option<u64> {
    let text = std::str::from_utf8(bytes).ok()?;
    let value = jet_foundation::EncodingJson::parse_json_exact_numbers(text, true).ok()?;
    find_integer(&value, names)
}

fn find_integer(value: &jet_foundation::EncodingJson::Value, names: &[&str]) -> Option<u64> {
    match value {
        jet_foundation::EncodingJson::Value::Number(_)
        | jet_foundation::EncodingJson::Value::Int(_) => None,
        jet_foundation::EncodingJson::Value::Object(values) => {
            values.iter().find_map(|(key, value)| {
                if names.contains(&key.as_str()) {
                    match value {
                        jet_foundation::EncodingJson::Value::Number(value) => value.parse().ok(),
                        jet_foundation::EncodingJson::Value::Int(value) if *value >= 0 => {
                            Some(*value as u64)
                        }
                        _ => find_integer(value, names),
                    }
                } else {
                    match value {
                        jet_foundation::EncodingJson::Value::Object(_)
                        | jet_foundation::EncodingJson::Value::Array(_) => {
                            find_integer(value, names)
                        }
                        _ => None,
                    }
                }
            })
        }
        jet_foundation::EncodingJson::Value::Array(values) => {
            values.iter().find_map(|value| find_integer(value, names))
        }
        _ => None,
    }
}

fn escape(value: &str) -> String {
    jet_foundation::JSON::json_escape(value)
}

#[cfg(test)]
mod tests {
    use super::newest_discoverable_targets;

    #[test]
    fn manifest_retention_marks_newest_twelve_per_system() {
        let mut targets = (0..13)
            .map(|revision| {
                (
                    revision,
                    "x86_64-linux".to_string(),
                    format!("{revision:040x}"),
                )
            })
            .collect::<Vec<_>>();
        targets.push((1, "aarch64-linux".to_string(), format!("{:040x}", 90)));
        targets.reverse();

        let discoverable = newest_discoverable_targets(targets);
        assert_eq!(discoverable.len(), 13);
        assert!(discoverable.contains(&("x86_64-linux".to_string(), format!("{:040x}", 12))));
        assert!(!discoverable.contains(&("x86_64-linux".to_string(), format!("{:040x}", 0))));
        assert!(discoverable.contains(&("aarch64-linux".to_string(), format!("{:040x}", 90))));
    }
}
