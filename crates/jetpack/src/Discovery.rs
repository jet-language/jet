//! U26 (D-JPK-DISCOVER1=A): local, offline package discovery.
//!
//! `jet search` and `jet info` read a project-local JSONL index plus provider
//! metadata already available to the resolver: env/module package refs, offline
//! provider fixtures, and realized hangar records. Nothing here shells out or
//! fetches; missing data stays missing until a normal resolver/update path
//! records it.

use jet_env_model::ModuleEval::AdapterPlan;
use super::Provider;
use super::RefSpec::RefSpec;
use super::Store::StoreEntry;
use super::JSON::{self, JSONValue};
use crate::Syntax;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const INDEX_DIR: &str = "discovery";
const INDEX_FILE: &str = "index.jsonl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionField {
    pub name: String,
    pub default: String,
    pub docs: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRecord {
    pub source: String,
    pub name: String,
    pub reference: String,
    pub version: String,
    pub platforms: Vec<String>,
    pub docs: String,
    pub provenance: String,
    pub options: Vec<OptionField>,
}

impl PackageRecord {
    pub fn display_ref(&self) -> String {
        format!("{}.{}", self.source, self.name)
    }

    fn merge_from(&mut self, other: PackageRecord) {
        if self.version.is_empty() && !other.version.is_empty() {
            self.version = other.version;
        }
        if self.provenance == "declared" && other.provenance != "declared" {
            self.provenance = other.provenance;
        }
        if self.docs.is_empty() && !other.docs.is_empty() {
            self.docs = other.docs;
        }
        for platform in other.platforms {
            if !self.platforms.contains(&platform) {
                self.platforms.push(platform);
            }
        }
        for opt in other.options {
            if !self.options.iter().any(|o| o.name == opt.name) {
                self.options.push(opt);
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub packages: Vec<PackageRecord>,
}

impl Index {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    pub fn add_package(&mut self, record: PackageRecord) {
        if let Some(existing) = self
            .packages
            .iter_mut()
            .find(|r| r.source == record.source && r.name == record.name)
        {
            existing.merge_from(record);
        } else {
            self.packages.push(record);
        }
        self.packages
            .sort_by(|a, b| a.source.cmp(&b.source).then(a.name.cmp(&b.name)));
    }

    pub fn search(&self, query: &str) -> Vec<&PackageRecord> {
        let q = query.to_ascii_lowercase();
        let mut found: Vec<(u8, &PackageRecord)> = self
            .packages
            .iter()
            .filter_map(|record| {
                let name = record.name.to_ascii_lowercase();
                let display = record.display_ref().to_ascii_lowercase();
                let docs = record.docs.to_ascii_lowercase();
                let score = if name == q || display == q {
                    0
                } else if name.starts_with(&q) || display.starts_with(&q) {
                    1
                } else if name.contains(&q) || display.contains(&q) {
                    2
                } else if docs.contains(&q) {
                    3
                } else {
                    return None;
                };
                Some((score, record))
            })
            .collect();
        found.sort_by(|(sa, a), (sb, b)| {
            sa.cmp(sb)
                .then(a.source.cmp(&b.source))
                .then(a.name.cmp(&b.name))
        });
        found.into_iter().map(|(_, r)| r).collect()
    }

    pub fn info(&self, query: &str) -> Option<&PackageRecord> {
        let (source, name) = split_query(query);
        let matches: Vec<&PackageRecord> = self
            .packages
            .iter()
            .filter(|r| match source {
                Some(s) => r.source == s && r.name == name,
                None => r.name == name || r.reference == query,
            })
            .collect();
        if matches.len() == 1 {
            matches.first().copied()
        } else {
            None
        }
    }

    pub fn package_completions(&self, source: &str, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .packages
            .iter()
            .filter(|r| r.source == source && r.name.starts_with(prefix))
            .map(|r| r.name.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    pub fn nearest(&self, query: &str) -> Option<String> {
        let (_, name) = split_query(query);
        self.packages
            .iter()
            .map(|r| (distance(name, &r.name), r.display_ref()))
            .filter(|(d, _)| *d <= 3)
            .min_by(|(da, a), (db, b)| da.cmp(db).then(a.cmp(b)))
            .map(|(_, label)| label)
    }
}

pub fn index_path(project_dir: &Path) -> PathBuf {
    super::Store::managed_dir(project_dir)
        .join(INDEX_DIR)
        .join(INDEX_FILE)
}

pub fn load(project_dir: &Path) -> Result<Option<Index>, String> {
    let path = index_path(project_dir);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut index = Index::default();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let json =
            JSON::parse(line).map_err(|e| format!("{}:{}: {e}", path.display(), line_no + 1))?;
        index.add_package(record_from_json(&json)?);
    }
    Ok(Some(index))
}

pub fn write(project_dir: &Path, index: &Index) -> Result<(), String> {
    let path = index_path(project_dir);
    super::RuntimePolicy::with_project_lock(project_dir, "discovery-index", || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = String::new();
        for record in &index.packages {
            out.push_str(&record_to_json(record));
            out.push('\n');
        }
        std::fs::write(path, out)
    })
    .map_err(|e| e.to_string())
}

pub fn merge_refs(
    index: &mut Index,
    refs: &[RefSpec],
    fixtures: Option<&Path>,
    store_entries: &[StoreEntry],
) {
    for spec in refs {
        let (version, provenance) = fixture_version(fixtures, spec)
            .or_else(|| store_version(store_entries, spec))
            .unwrap_or_else(|| (String::new(), "declared".to_string()));
        index.add_package(record_for_ref(spec, version, provenance));
    }
}

pub fn merge_store_entries(index: &mut Index, store_entries: &[StoreEntry]) {
    for entry in store_entries {
        let (source, name) = if let Some((name, source)) =
            entry.reference.rsplit_once(Syntax::REF_PROVIDER_AT)
        {
            (source, name)
        } else if let Some((source, name)) = entry.reference.split_once(Syntax::REF_SEPARATOR) {
            // Read pre-D-JPK-REF1 hangar metadata without rewriting it.
            (source, name)
        } else {
            continue;
        };
        index.add_package(PackageRecord {
            source: source.to_string(),
            name: name.to_string(),
            reference: entry.reference.clone(),
            version: entry.version.clone(),
            platforms: platform_strings(),
            docs: format!("{} from hangar metadata", entry.reference),
            provenance: format!("hangar:{}", entry.id),
            options: service_option_fields(),
        });
    }
}

pub fn merge_adapters(index: &mut Index, adapters: &[AdapterPlan]) {
    for adapter in adapters {
        index.add_package(PackageRecord {
            source: "adapt".to_string(),
            name: adapter.name.clone(),
            reference: format!("adapt:{}", adapter.name),
            version: String::new(),
            platforms: platform_strings(),
            docs: format!("adapter package from {}", adapter.source),
            provenance: "env:Pkg.adapt".to_string(),
            options: service_option_fields(),
        });
    }
}

pub fn service_option_fields() -> Vec<OptionField> {
    vec![
        field("enable", "required", "Turn the service on or off."),
        field("ports", "[]", "TCP ports the service listens on."),
        field("run", "[String]", "Executable and arguments that start the service."),
        field(
            "shutdown",
            ".Term/.Kill",
            "Typed process-group shutdown policy.",
        ),
        field(
            "data_dir",
            ".jet/services/<name>/data",
            "Persisted state directory.",
        ),
        field("ready", "ServiceProbe", "Typed readiness probe polled until ready."),
        field("after", "[String]", "Services that must be healthy first."),
        field("before_start", "[String]", "Finite tasks to run before start."),
        field("sockets", "[String]", "Project-relative Unix sockets reserved before start."),
    ]
}

pub fn search_json(records: &[&PackageRecord]) -> String {
    let packages = records
        .iter()
        .map(|r| record_to_json(r))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"packages\":[{packages}]}}")
}

pub fn info_json(record: &PackageRecord) -> String {
    record_to_json(record)
}

fn record_for_ref(spec: &RefSpec, version: String, provenance: String) -> PackageRecord {
    PackageRecord {
        source: spec.source.label().to_string(),
        name: spec.package.clone(),
        reference: spec.raw.clone(),
        version,
        platforms: platform_strings(),
        docs: format!("{} from local Jetpack metadata", spec.raw),
        provenance,
        options: service_option_fields(),
    }
}

fn field(name: &str, default: &str, docs: &str) -> OptionField {
    OptionField {
        name: name.to_string(),
        default: default.to_string(),
        docs: docs.to_string(),
    }
}

fn platform_strings() -> Vec<String> {
    super::Platform::TIER_ONE_OSES
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn fixture_version(fixtures: Option<&Path>, spec: &RefSpec) -> Option<(String, String)> {
    let dir = fixtures?;
    let path = dir.join(Provider::fixture_name(spec));
    let text = std::fs::read_to_string(&path).ok()?;
    let out = provider_output_path(&text)?;
    Some((
        store_path_version(&out, spec.short_name()),
        format!("fixture:{}", path.display()),
    ))
}

fn store_version(entries: &[StoreEntry], spec: &RefSpec) -> Option<(String, String)> {
    entries
        .iter()
        .find(|e| e.reference == spec.raw)
        .map(|e| (e.version.clone(), format!("hangar:{}", e.id)))
}

fn provider_output_path(text: &str) -> Option<String> {
    let json = JSON::parse(text.trim()).ok()?;
    let first = json.as_array().ok()?.first()?;
    let outputs = first.get("outputs").ok()?.as_object().ok()?;
    outputs
        .get("bin")
        .or_else(|| outputs.get("out"))
        .and_then(|j| j.as_str().ok())
        .map(|s| s.to_string())
}

fn store_path_version(out: &str, name: &str) -> String {
    const HASH_LEN: usize = 32;
    let base = out.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if let Some(rest) = base.get(HASH_LEN..) {
        if let Some(version) = version_after_name(rest.strip_prefix('-').unwrap_or(rest), name) {
            return version;
        }
    }
    if let Some((_, rest)) = base.split_once('-') {
        // Test fixtures use short fake store hashes. Accept that shape too so
        // discovery can expose fixture versions without running Nix.
        if let Some(version) = version_after_name(rest, name) {
            return version;
        }
    }
    version_after_name(base, name).unwrap_or_default()
}

fn version_after_name<'a>(rest: &'a str, name: &str) -> Option<String> {
    const OUTPUT_SUFFIXES: &[&str] = &["-bin", "-dev", "-lib", "-doc", "-man", "-info", "-out"];
    let Some(mut version) = rest.strip_prefix(name).and_then(|s| s.strip_prefix('-')) else {
        return None;
    };
    for suffix in OUTPUT_SUFFIXES {
        if let Some(stripped) = version.strip_suffix(suffix) {
            version = stripped;
            break;
        }
    }
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        Some(version.to_string())
    } else {
        None
    }
}

fn split_query(query: &str) -> (Option<&str>, &str) {
    if let Some((name, source)) = query.rsplit_once(Syntax::REF_PROVIDER_AT) {
        return (Some(source), name);
    }
    if let Some((source, name)) = query.split_once(Syntax::REF_SEPARATOR) {
        return (Some(source), name);
    }
    if let Some((source, name)) = query.split_once('.') {
        return (Some(source), name);
    }
    (None, query)
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn record_to_json(record: &PackageRecord) -> String {
    let platforms = json_string_array(&record.platforms);
    let options = record
        .options
        .iter()
        .map(option_to_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"source\":{},\"name\":{},\"reference\":{},\"version\":{},\"platforms\":{},\"docs\":{},\"provenance\":{},\"options\":[{}]}}",
        JSON::quote(&record.source),
        JSON::quote(&record.name),
        JSON::quote(&record.reference),
        JSON::quote(&record.version),
        platforms,
        JSON::quote(&record.docs),
        JSON::quote(&record.provenance),
        options
    )
}

fn option_to_json(option: &OptionField) -> String {
    format!(
        "{{\"name\":{},\"default\":{},\"docs\":{}}}",
        JSON::quote(&option.name),
        JSON::quote(&option.default),
        JSON::quote(&option.docs)
    )
}

fn json_string_array(items: &[String]) -> String {
    let body = items
        .iter()
        .map(|s| JSON::quote(s))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

fn record_from_json(json: &JSONValue) -> Result<PackageRecord, String> {
    let obj = json.as_object()?;
    Ok(PackageRecord {
        source: required_str(obj, "source")?.to_string(),
        name: required_str(obj, "name")?.to_string(),
        reference: required_str(obj, "reference")?.to_string(),
        version: required_str(obj, "version").unwrap_or("").to_string(),
        platforms: string_array(obj.get("platforms")),
        docs: required_str(obj, "docs").unwrap_or("").to_string(),
        provenance: required_str(obj, "provenance").unwrap_or("").to_string(),
        options: option_array(obj.get("options")),
    })
}

fn required_str<'a>(obj: &'a BTreeMap<String, JSONValue>, key: &str) -> Result<&'a str, String> {
    obj.get(key)
        .ok_or_else(|| format!("missing key `{key}`"))?
        .as_str()
}

fn string_array(json: Option<&JSONValue>) -> Vec<String> {
    let Some(JSONValue::Array(items)) = json else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|j| j.as_str().ok().map(ToString::to_string))
        .collect()
}

fn option_array(json: Option<&JSONValue>) -> Vec<OptionField> {
    let Some(JSONValue::Array(items)) = json else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object().ok()?;
            Some(OptionField {
                name: required_str(obj, "name").ok()?.to_string(),
                default: required_str(obj, "default").unwrap_or("").to_string(),
                docs: required_str(obj, "docs").unwrap_or("").to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::RefSpec;
    use super::*;

    #[test]
    fn index_round_trips_and_searches() {
        let mut index = Index::default();
        let spec = RefSpec::classify_in(
            "ripgrep@default",
            &RefSpec::SourceTable::from_decls([(
                "default".to_string(),
                "nixpkgs:nixos-24.05".to_string(),
                RefSpec::ProviderKind::Nix,
            )]),
        )
        .unwrap();
        index.add_package(record_for_ref(
            &spec,
            "14.1.0".to_string(),
            "fixture".to_string(),
        ));
        let json = record_to_json(&index.packages[0]);
        let parsed = record_from_json(&JSON::parse(&json).unwrap()).unwrap();
        assert_eq!(parsed.name, "ripgrep");
        assert_eq!(index.search("rip")[0].display_ref(), "default.ripgrep");
        assert_eq!(index.info("default.ripgrep").unwrap().version, "14.1.0");
    }

    #[test]
    fn completions_and_nearest_are_local_index_only() {
        let mut index = Index::default();
        for name in ["postgres_16", "postgres_17", "ripgrep"] {
            index.add_package(PackageRecord {
                source: "default".to_string(),
                name: name.to_string(),
                reference: format!("{name}@default"),
                version: String::new(),
                platforms: platform_strings(),
                docs: String::new(),
                provenance: "declared".to_string(),
                options: service_option_fields(),
            });
        }
        assert_eq!(
            index.package_completions("default", "post"),
            vec!["postgres_16".to_string(), "postgres_17".to_string()]
        );
        assert_eq!(
            index.nearest("postgress_16"),
            Some("default.postgres_16".to_string())
        );
    }
}
